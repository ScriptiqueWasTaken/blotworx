//! Resource binding481f34/481f48 and conversion481f5c/4821e0.
//! Input starts at the graph payload header; source-container selection is separate.
use anyhow::{Context as _, Result};
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum Layout {
    Fixed,
    Dynamic { node_stride: u32, edge_stride: u32 },
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Auxiliary {
    Scalar(u32),
    Tail(u32),
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Payload {
    pub bytes: Vec<u8>,
    pub node_count: u32,
    pub edge_count: u32,
    pub points: u32,
    pub edges: u32,
    pub external: Option<u32>,
    pub external_count: u32,
    pub field38: Auxiliary,
}
fn read(bytes: &[u8], offset: u32) -> Result<u32> {
    let at = offset as usize;
    let end = at
        .checked_add(4)
        .context("Graph normalization offset overflow")?;
    Ok(u32::from_be_bytes(
        bytes
            .get(at..end)
            .context("Graph normalization exceeds payload")?
            .try_into()?,
    ))
}
fn write(bytes: &mut [u8], offset: u32, value: u32) -> Result<()> {
    let at = offset as usize;
    let end = at
        .checked_add(4)
        .context("Graph normalization offset overflow")?;
    bytes
        .get_mut(at..end)
        .context("Graph normalization write exceeds payload")?
        .copy_from_slice(&value.to_be_bytes());
    Ok(())
}
fn swap(bytes: &mut [u8], offset: u32) -> Result<u32> {
    let value = read(bytes, offset)?.swap_bytes();
    write(bytes, offset, value)?;
    Ok(value)
}
pub fn convert(source: &[u8], layout: Layout) -> Result<Payload> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(source.len())?;
    bytes.extend_from_slice(source);
    let d = normalize_in_place(&mut bytes, layout)?;
    Ok(Payload {
        bytes,
        node_count: d.node_count,
        edge_count: d.edge_count,
        points: d.points,
        edges: d.edges,
        external: d.external,
        external_count: d.external_count,
        field38: d.field38,
    })
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Description {
    pub node_count: u32,
    pub edge_count: u32,
    pub points: u32,
    pub edges: u32,
    pub external: Option<u32>,
    pub external_count: u32,
    pub field38: Auxiliary,
}
/// Same retail conversion kernel over an owned caller buffer; source-container
/// selection stays on the loader's actual binding instead of this function.
pub fn normalize_in_place(bytes: &mut [u8], layout: Layout) -> Result<Description> {
    // Original swaps the second header word first.
    let edge_count = swap(bytes, 4)?;
    let node_count = swap(bytes, 0)?;
    let (node_stride, edge_stride) = match layout {
        Layout::Fixed => (32, 16),
        Layout::Dynamic {
            node_stride,
            edge_stride,
        } => (node_stride, edge_stride),
    };
    let points = 8u32;
    let edges = points.wrapping_add(node_count.wrapping_mul(node_stride));
    let mut field38 = match layout {
        Layout::Fixed => Auxiliary::Tail(edges.wrapping_add(edge_count.wrapping_mul(16))),
        Layout::Dynamic { .. } => Auxiliary::Scalar(0),
    };
    let node_fields: &[u32] = match layout {
        Layout::Fixed => &[0, 4, 8, 12, 16, 20, 28],
        Layout::Dynamic { .. } => &[0, 4, 8, 12, 16, 24],
    };
    for index in 0..node_count {
        let at = points.wrapping_add(index.wrapping_mul(node_stride));
        for &field in node_fields {
            swap(bytes, at.wrapping_add(field))?;
        }
    }
    if let Layout::Dynamic { .. } = layout {
        if node_count != 0 {
            let at = points.wrapping_add(20);
            let raw = read(bytes, at)?;
            if raw != 0 {
                // This load/store conversion preserves the original single's
                // bits, including signaling NaNs; no arithmetic is performed.
                field38 = Auxiliary::Scalar(raw.swap_bytes());
                write(bytes, at, 0)?;
            }
        }
    }
    let mut external = None;
    let mut external_count = 0u32;
    for index in 0..node_count {
        let at = points.wrapping_add(index.wrapping_mul(node_stride));
        if read(bytes, at)? >= 0x80000000 {
            if external.is_none() {
                external = Some(at);
            }
            external_count = external_count.wrapping_add(1);
        }
    }
    for index in 0..edge_count {
        let at = edges.wrapping_add(index.wrapping_mul(edge_stride));
        let fields: &[u32] = match layout {
            Layout::Fixed => &[0, 4, 8, 12],
            Layout::Dynamic { .. } => &[0, 4, 8],
        };
        for &field in fields {
            swap(bytes, at.wrapping_add(field))?;
        }
        if let Auxiliary::Tail(tail) = field38 {
            let link = read(bytes, at.wrapping_add(12))?;
            if link & 0x80000000 == 0 {
                let mut offset = link.wrapping_sub(1).wrapping_mul(4);
                let tag = swap(bytes, tail.wrapping_add(offset))? & 0x80000000;
                loop {
                    offset = offset.wrapping_add(4);
                    let value = read(bytes, tail.wrapping_add(offset))?.swap_bytes();
                    if value & 0x80000000 != tag {
                        break;
                    }
                    write(bytes, tail.wrapping_add(offset), value)?;
                }
            }
        }
    }
    Ok(Description {
        node_count,
        edge_count,
        points,
        edges,
        external,
        external_count,
        field38,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn layout(c: &Value) -> Layout {
        if c["layout"] == "fixed" {
            Layout::Fixed
        } else {
            Layout::Dynamic {
                node_stride: c["node_stride"].as_u64().unwrap() as u32,
                edge_stride: c["edge_stride"].as_u64().unwrap() as u32,
            }
        }
    }
    #[test]
    fn original_graph_payload_binding_and_normalization_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_normalization.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let bytes: Vec<u8> = serde_json::from_value(c["bytes"].clone())?;
            let before = bytes.clone();
            let result = convert(&bytes, layout(c))?;
            assert_eq!(serde_json::to_value(result)?, case["payload"], "{c}");
            assert_eq!(bytes, before);
        }
        Ok(())
    }
    #[test]
    fn truncated_payloads_fail_without_changing_the_source() -> Result<()> {
        for bytes in [vec![], vec![0; 4], vec![1, 0, 0, 0, 0, 0, 0, 0]] {
            let before = bytes.clone();
            assert!(convert(&bytes, Layout::Fixed).is_err());
            assert_eq!(bytes, before);
        }
        Ok(())
    }
    #[test]
    #[ignore = "Requires immutable imported game data in BLOTWORX_TEST_DATA"]
    fn original_owned_sector_payload_hashes_and_source_coordinates_match() -> Result<()> {
        fn hash(bytes: &[u8]) -> String {
            ring::digest::digest(&ring::digest::SHA256, bytes)
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        }
        let root = std::path::PathBuf::from(
            std::env::var("BLOTWORX_TEST_DATA").context("Imported game data is required")?,
        );
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_normalization_owned.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            let source = std::fs::read(root.join(path))?;
            assert_eq!(
                hash(&source),
                case["source_sha256"].as_str().unwrap(),
                "{path}"
            );
            let sector = crate::ai_nav::Sector::load(&source)?;
            let offset = case["offset"].as_u64().unwrap() as usize;
            let converted = convert(
                source
                    .get(offset..)
                    .context("Missing selected sector payload")?,
                Layout::Dynamic {
                    node_stride: 36,
                    edge_stride: 12,
                },
            )?;
            let p = &case["payload"];
            assert_eq!(
                hash(&converted.bytes),
                p["sha256"].as_str().unwrap(),
                "{path}"
            );
            assert_eq!(
                serde_json::json!({"node_count":converted.node_count,"edge_count":converted.edge_count,"points":converted.points,"edges":converted.edges,"external":converted.external,"external_count":converted.external_count,"field38":converted.field38}),
                serde_json::json!({"node_count":p["node_count"],"edge_count":p["edge_count"],"points":p["points"],"edges":p["edges"],"external":p["external"],"external_count":p["external_count"],"field38":p["field38"]}),
                "{path}"
            );
            assert_eq!(converted.node_count as usize, sector.nodes.len());
            assert_eq!(converted.edge_count as usize, sector.edges.len());
            for (index, node) in sector.nodes.iter().enumerate() {
                let at = converted.points + index as u32 * 36;
                assert_eq!(read(&converted.bytes, at)?, node.id, "{path} node{index}");
                for axis in 0..3 {
                    assert_eq!(
                        read(&converted.bytes, at + 4 + axis as u32 * 4)?,
                        node.position[axis].to_bits(),
                        "{path} node{index} axis{axis}"
                    );
                }
            }
            assert_eq!(hash(&source), case["source_sha256"].as_str().unwrap());
        }
        Ok(())
    }
    #[test]
    fn in_place_normalization_matches_original_without_copying_or_touching_prefix() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_normalization.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            let raw: Vec<u8> = serde_json::from_value(c["bytes"].clone())?;
            let mut container = vec![0xa5; 16];
            container.extend_from_slice(&raw);
            let allocation = container.as_ptr();
            let d = normalize_in_place(&mut container[16..], layout(c))?;
            let expected: Vec<u8> = serde_json::from_value(case["payload"]["bytes"].clone())?;
            assert_eq!(&container[16..], expected.as_slice(), "payload {index}");
            assert_eq!(&container[..16], &[0xa5; 16]);
            assert_eq!(container.as_ptr(), allocation);
            let mut metadata = case["payload"].clone();
            metadata.as_object_mut().unwrap().remove("bytes");
            assert_eq!(serde_json::to_value(d)?, metadata, "description {index}");
        }
        Ok(())
    }
}
