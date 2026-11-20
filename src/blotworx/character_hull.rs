//! Verified 32-bit Wii Havok 7 and PC Havok 2012 character convex-point layouts.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;

#[derive(Clone, serde::Serialize)]
pub struct ConvexHull {
    pub vertices: Vec<[f32; 3]>,
    pub radius: f32,
}

pub fn load(file: &HkxFile) -> Result<ConvexHull> {
    Ok(load_with_support(file)?.0)
}

/// Retain the original padded support lanes alongside the logical Box3D hull.
pub fn load_with_support(file: &HkxFile) -> Result<(ConvexHull, super::native_gsk::ConvexSupport)> {
    ensure!(
        file.header.pointer_size == 4
            && ((file.header.endianness == 0
                && file.header.contents_version.starts_with("Havok-7.0"))
                || (file.header.endianness == 1
                    && file.header.contents_version == "hk_2012.1.0-r1")),
        "Unsupported character hull layout"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Missing character hull data section")?;
    let class = |offset: u32| -> Option<&str> {
        let fix = file
            .virtual_fixups
            .get(section)?
            .iter()
            .find(|f| f.src_offset == offset)?;
        let bytes = file
            .section_data
            .get(fix.section_index as usize)?
            .get(fix.class_name_offset as usize..)?
            .split(|b| *b == 0)
            .next()?;
        std::str::from_utf8(bytes).ok()
    };
    let bodies: Vec<_> = file.virtual_fixups[section]
        .iter()
        .filter(|f| class(f.src_offset) == Some("hkpRigidBody"))
        .collect();
    ensure!(
        bodies.len() == 1,
        "Character hull requires exactly one verified rigid body"
    );
    let shape = file.global_fixups[section]
        .iter()
        .find(|f| f.src_offset == bodies[0].src_offset + 0x10)
        .context("Missing character body shape pointer")?;
    ensure!(
        shape.dst_section as usize == section
            && class(shape.dst_offset) == Some("hkpConvexVerticesShape"),
        "Character body does not use a supported convex vertex shape"
    );
    let pointer = file.local_fixups[section]
        .iter()
        .find(|f| f.src_offset == shape.dst_offset + 0x40)
        .context("Missing character convex vertices pointer")?;
    decode_with_support_endian(
        &file.section_data[section],
        shape.dst_offset as usize,
        pointer.dst_offset as usize,
        file.header.endianness == 1,
    )
}

pub(super) fn decode(data: &[u8], shape: usize, pointer: usize) -> Result<ConvexHull> {
    Ok(decode_with_support(data, shape, pointer)?.0)
}

/// Decode a verified shape within a containing physics resource.
pub(super) fn decode_endian(
    data: &[u8],
    shape: usize,
    pointer: usize,
    little: bool,
) -> Result<ConvexHull> {
    Ok(decode_with_support_endian(data, shape, pointer, little)?.0)
}

fn decode_with_support(
    data: &[u8],
    shape: usize,
    pointer: usize,
) -> Result<(ConvexHull, super::native_gsk::ConvexSupport)> {
    decode_with_support_endian(data, shape, pointer, false)
}
fn decode_with_support_endian(
    data: &[u8],
    shape: usize,
    pointer: usize,
    little: bool,
) -> Result<(ConvexHull, super::native_gsk::ConvexSupport)> {
    let word = |offset: usize| -> Result<u32> {
        let bytes = data
            .get(offset..offset.checked_add(4).context("Hull offset overflow")?)
            .context("Truncated character hull")?
            .try_into()?;
        Ok(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let bounds = super::shape_bounds::ConvexBounds::decode_endian(data, shape, little)?;
    let radius = bounds.radius;
    let blocks = word(shape + 0x44)? as usize;
    let count = word(shape + 0x4c)? as usize;
    ensure!(
        count >= 4 && blocks == count.div_ceil(4),
        "Invalid character convex vertex count"
    );
    let size = blocks
        .checked_mul(48)
        .context("Character hull size overflow")?;
    let packed = data
        .get(
            pointer
                ..pointer
                    .checked_add(size)
                    .context("Character hull pointer overflow")?,
        )
        .context("Truncated transposed character vertices")?;
    let mut packed_vertices = Vec::with_capacity(blocks * 4);
    for block in packed.chunks_exact(48) {
        for lane in 0..4 {
            let p = std::array::from_fn(|axis| {
                let at = axis * 16 + lane * 4;
                let bytes = block[at..at + 4].try_into().unwrap();
                if little {
                    f32::from_le_bytes(bytes)
                } else {
                    f32::from_be_bytes(bytes)
                }
            });
            ensure!(
                p.iter().all(|v| v.is_finite()),
                "Non-finite character vertex"
            );
            packed_vertices.push(p);
        }
    }
    let vertices = packed_vertices[..count].to_vec();
    // The cached AABB and the support vertices are independent serialized
    // fields. Native getAabb (DOL8027b630) uses the cache, while support queries
    // use the packed points. Reconstructing endpoints from center +/- half is
    // not an integrity check: rounding alone rejects retail HM Zone M hulls.
    // Preserve both sources rather than recooking either to make them agree.
    // Native virtual+4c copies all four lanes at shape+30, including W.
    let mut stored_center = [0.; 4];
    for (i, value) in stored_center.iter_mut().enumerate() {
        *value = f32::from_bits(word(shape + 0x30 + i * 4)?);
    }
    ensure!(
        stored_center.iter().all(|v| v.is_finite()),
        "Nonfinite character support center"
    );
    Ok((
        ConvexHull { vertices, radius },
        super::native_gsk::ConvexSupport {
            packed_vertices,
            vertex_count: count as u32,
            stored_center,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_mickey_support_against_authored_triangle() -> Result<()> {
        use super::super::{
            collision,
            native_gsk::{Cache, Features},
            native_hk_transform, shape_welding,
        };
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/_Dynamic.pak"),
        )?);
        let source = pak
            .files
            .iter()
            .find(|f| {
                f.path.eq_ignore_ascii_case(
                    "Characters/Mickey/Mickey_Pencil_Collision_dynamic_hull.hkx_wii",
                )
            })
            .context("Missing Mickey authored hull")?;
        let hkx = HkxFile::load_bytes(&source.data).map_err(anyhow::Error::msg)?;
        let (hull, mut support) = load_with_support(&hkx)?;
        let level = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/eb_villagetest.pak"),
        )?);
        let mut selected = None;
        for file in &level.files {
            if !file.path.to_ascii_lowercase().ends_with(".hkx_wii") {
                continue;
            }
            let Ok(hkx) = HkxFile::load_bytes(&file.data) else {
                continue;
            };
            let Ok(parts) = collision::body_local_meshes(&hkx) else {
                continue;
            };
            let identity =
                native_hk_transform::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
            let layouts = super::super::dynamic_body::layout(&parts, &identity)?;
            for part in &parts {
                if part.convex {
                    continue;
                }
                let owner = layouts
                    .iter()
                    .find(|body| body.body_offset == part.body_offset)
                    .context("Missing imported triangle owner")?;
                let prepared = owner
                    .prepare_welding(&parts)?
                    .context("Missing prepared triangle welding")?;
                for i in 0..part.indices.len() / 3 {
                    let triangle = part.triangle_support(i)?;
                    if !shape_welding::degenerate_triangle(triangle.vertices)? {
                        selected = Some((
                            file.path.clone(),
                            part.prepared_triangle_contact(i, &prepared)?,
                            i,
                        ));
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (asset, mut triangle, triangle_index) =
            selected.context("No authored village triangle")?;
        let center: [f32; 3] = std::array::from_fn(|i| {
            (triangle.support.vertices[0][i]
                + triangle.support.vertices[1][i]
                + triangle.support.vertices[2][i])
                / 3.
        });
        let rotation = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let world_b = native_hk_transform::from_entity(rotation, [0.; 3]);
        let mut observations = Vec::new();
        let table = shape_welding::ContactTable::retail_world()?;
        for axis in 0..3 {
            for sign in [-1., 1.] {
                let mut position = center;
                position[axis] += sign * 100.;
                let world_a = native_hk_transform::from_entity(rotation, position);
                let relative = native_hk_transform::relative(&world_a, &world_b);
                let mut cache = Cache([0; 12]);
                cache.initialize_triangle(&mut support, triangle.support.vertices, &relative)?;
                let mut features =
                    Features::load(&cache, &mut support, &mut triangle.support, &relative)?;
                let result = triangle.query(
                    &mut features,
                    &mut support,
                    &world_a,
                    &world_b,
                    hull.radius,
                    f32::MAX,
                    false,
                    &mut [[0.; 4]; 4],
                    None,
                    &table,
                )?;
                match result {
                    shape_welding::ClosestQuery::Contact(contact)=> {
                        ensure!(contact.separating.iter().chain(contact.position.iter()).all(|v|v.is_finite()),"Nonfinite production contact");
                        observations.push(serde_json::json!({"axis":axis,"sign":sign,"result":"contact","position":contact.position,"separating":contact.separating,"features":[features.count_a,features.count_b]}));
                    }
                    shape_welding::ClosestQuery::NoContact=>observations.push(serde_json::json!({
                        "axis":axis,"sign":sign,"result":"welding_rejected","features":[features.count_a,features.count_b]})),
                    shape_welding::ClosestQuery::Unresolved=>anyhow::bail!("Separated production query unresolved: axis{axis} sign{sign}"),
                }
            }
        }
        ensure!(
            observations.iter().any(|o| o["result"] == "contact"),
            "All production contacts rejected"
        );
        std::fs::write(
            "target/production-mickey-native-query.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "hull":source.path,"triangle_asset":asset,"triangle_index":triangle_index,
            "welding_mode":triangle.mode,"welding_word":triangle.word,"logical_vertices":hull.vertices.len(),
            "packed_vertices":support.packed_vertices.len(),"queries":observations,"original_execution_validated":false}))?,
        )?;
        Ok(())
    }

    fn fixture() -> Vec<u8> {
        let mut data = vec![0u8; 0xb0];
        let mut put = |at: usize, v: u32| data[at..at + 4].copy_from_slice(&v.to_be_bytes());
        put(0x10, 0.05f32.to_bits());
        put(0x44, 1);
        put(0x4c, 4);
        for axis in 0..3 {
            put(0x20 + axis * 4, 0.5f32.to_bits());
            put(0x30 + axis * 4, 0.5f32.to_bits());
        }
        for (i, v) in [0f32, 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
            .iter()
            .enumerate()
        {
            put(0x80 + i * 4, v.to_bits());
        }
        data
    }
    #[test]
    fn retains_serialized_padding_for_native_support() -> Result<()> {
        use super::super::native_gsk::SupportShape;
        let mut data = fixture();
        data.resize(0xe0, 0);
        data[0x44..0x48].copy_from_slice(&2u32.to_be_bytes());
        data[0x4c..0x50].copy_from_slice(&5u32.to_be_bytes());
        // Logical vertex4 is zero. Padded vertex7 is deliberately different:
        // retaining source bytes is essential even beyond the logical bounds.
        data[0xbc..0xc0].copy_from_slice(&9f32.to_be_bytes());
        data[0x3c..0x40].copy_from_slice(&7f32.to_be_bytes());
        let (hull, mut support) = decode_with_support(&data, 0, 0x80)?;
        assert_eq!(hull.vertices.len(), 5);
        assert_eq!(support.packed_vertices.len(), 8);
        assert_eq!(support.center()?, [0.5, 0.5, 0.5, 7.]);
        assert_eq!(support.support([1., 0., 0., 0.])?.0, 7);
        assert_eq!(support.vertex(7)?[0], 9.);
        assert_eq!(support.sphere_count()?, 5);
        data[0xbc..0xc0].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(decode_with_support(&data, 0, 0x80).is_err());
        Ok(())
    }
    #[test]
    fn decodes_transposed_points_and_margin() {
        let hull = decode(&fixture(), 0, 0x80).unwrap();
        assert_eq!(
            hull.vertices,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
        );
        assert_eq!(hull.radius, 0.05);
    }
    #[test]
    fn rejects_invalid_or_truncated_geometry() {
        let mut data = fixture();
        assert!(decode(&data[..0xa0], 0, 0x80).is_err());
        data[0x4c..0x50].copy_from_slice(&5u32.to_be_bytes());
        assert!(decode(&data, 0, 0x80).is_err());
        data = fixture();
        data[0x80..0x84].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(decode(&data, 0, 0x80).is_err());
    }

    #[test]
    fn retail_zone_m_cache_rounding_preserves_original_vertices() -> Result<()> {
        // NOS_HM_ZoneM_Toon017.nif_wii, embedded shape5232. Asset SHA256:
        // 3a7c2510ec1a0b30766f03f0c0780005a7161e3e63ead73b269c13f02a89ce49.
        let points = [
            [205.65130615234375, 85.05745697021484, -88.81061553955078],
            [205.62998962402344, 85.0285873413086, -90.78990936279297],
            [205.60813903808594, 87.91149139404297, -90.78990936279297],
            [205.59971618652344, 87.91149139404297, -88.81061553955078],
        ];
        let half = [0.02579498291015625, 1.4414520263671875, 0.9896469116210938];
        let center = [205.62551879882812, 86.47003936767578, -89.80026245117188];
        let mut data = fixture();
        for axis in 0..3 {
            data[0x20 + axis * 4..0x24 + axis * 4]
                .copy_from_slice(&(half[axis] as f32).to_be_bytes());
            data[0x30 + axis * 4..0x34 + axis * 4]
                .copy_from_slice(&(center[axis] as f32).to_be_bytes());
            for lane in 0..4 {
                let at = 0x80 + axis * 16 + lane * 4;
                data[at..at + 4].copy_from_slice(&(points[lane][axis] as f32).to_be_bytes());
            }
        }
        let (hull, support) = decode_with_support(&data, 0, 0x80)?;
        assert_eq!(hull.vertices, points.map(|p| p.map(|v| v as f32)));
        assert_eq!(
            support.stored_center,
            [center[0] as f32, center[1] as f32, center[2] as f32, 0.]
        );
        let cached = super::super::shape_bounds::ConvexBounds::decode(&data, 0)?;
        assert_eq!(cached.half, half.map(|v| v as f32));
        assert!(
            (cached.center[0] - cached.half[0] - hull.vertices[3][0]).abs() > 1e-5,
            "Fixture must reproduce the old false rejection"
        );
        Ok(())
    }
}
