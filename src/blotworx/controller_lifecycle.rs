//! StartAnimations804bf484 and HUD template preparation15b620 share a walk:
//! own controllers, linked properties, then effective child-array slots.
use super::scene_bounds::{Controller, Hierarchy};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};

pub struct Entry {
    pub block: u32,
    pub class_name: String,
    pub property: Option<u32>,
}
pub struct Plan {
    pub entries: Vec<Entry>,
}
impl Plan {
    pub fn load(nif: &NifFile, graph: &Hierarchy, root: u32) -> Result<Self> {
        let mut entries = Vec::new();
        let mut pending = vec![(root, false)];
        let mut ancestors = std::collections::BTreeSet::new();
        while let Some((id, exit)) = pending.pop() {
            if exit {
                ancestors.remove(&id);
                continue;
            }
            ensure!(
                ancestors.insert(id),
                "Cyclic controller lifecycle object {id}"
            );
            pending.push((id, true));
            let object = nif
                .blocks
                .get(id as usize)
                .context("Lifecycle object link missing")?;
            if let Some(av) = object.as_av_object() {
                let node = graph
                    .nodes
                    .get(&id)
                    .context("Lifecycle AV object missing from hierarchy")?;
                // Texture-map controllers also appear on hierarchy nodes as
                // render bindings. Start the actual controller at its property.
                entries.extend(node.controllers.iter().filter(|controller| {
                    controller.texture_transform.is_none() || controller.target != Some(id)
                }).map(|controller| Entry {
                    block: controller.block,
                    class_name: controller.class_name.clone(),
                    property: None,
                }));
                // Native804bf5f8 bounds by effective size(b6), skips nulls;
                // unlike the custom START_EMITTERS used-count walk(b8).
                if super::nif_types::is_a(&node.class_name, "NiNode")? {
                    pending.extend(
                        node.children
                            .iter()
                            .rev()
                            .flatten()
                            .map(|child| (*child, false)),
                    );
                }
                //80444f78..80445088 links properties at the HEAD. Pushing
                // serialized forward order here visits that runtime reverse.
                for property in av.property_links.iter().flatten() {
                    let value = nif
                        .blocks
                        .get(*property as usize)
                        .context("Missing property link")?;
                    ensure!(
                        super::nif_types::is_a(
                            super::nif_types::block_name(nif, *property)?,
                            "NiProperty"
                        )?,
                        "Lifecycle property link targets {}",
                        value.type_name()
                    );
                    if property_head(nif, *property)?.is_some() {
                        pending.push((*property, false));
                    }
                }
            } else {
                ensure!(
                    super::nif_types::is_a(object.type_name(), "NiProperty")?,
                    "Unsupported lifecycle object {}",
                    object.type_name()
                );
                let chain = super::scene_bounds::controllers(nif, property_head(nif, id)?)?;
                append(&mut entries, &chain, Some(id));
            }
        }
        Ok(Self { entries })
    }
    pub fn controller_ids(&self) -> std::collections::BTreeSet<u32> {
        self.entries.iter().map(|e| e.block).collect()
    }
    pub fn start(&self, time: f32, mut start: impl FnMut(u32, f32) -> Result<()>) -> Result<()> {
        for entry in &self.entries {
            start(entry.block, time)?;
        }
        Ok(())
    }
}
fn append(out: &mut Vec<Entry>, chain: &[Controller], property: Option<u32>) {
    out.extend(chain.iter().map(|c| Entry {
        block: c.block,
        class_name: c.class_name.clone(),
        property,
    }));
}
fn property_head(nif: &NifFile, id: u32) -> Result<Option<u32>> {
    match nif.blocks.get(id as usize).context("Missing property")? {
        NiObject::NiMaterialProperty(p) => Ok(p.base.controller_link),
        NiObject::NiAlphaProperty(p) => Ok(p.base.controller_link),
        NiObject::Unknown(p) => {
            ensure!(
                matches!(nif.version, 0x14020007 | 0x14030109 | 0x14060500),
                "Unadmitted property NiObjectNET version {:08x}",
                nif.version
            );
            let uint = |at: usize| -> Result<u32> {
                let end = at
                    .checked_add(4)
                    .context("Property header offset overflow")?;
                let b = p
                    .raw_data
                    .get(at..end)
                    .context("Truncated property NiObjectNET")?
                    .try_into()?;
                Ok(if nif.source_is_little_endian {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                })
            };
            // String-table name index, extra-data count/links, controller.
            let extras = usize::try_from(uint(4)?)?;
            let at = extras
                .checked_mul(4)
                .and_then(|n| n.checked_add(8))
                .context("Property extra-data count overflow")?;
            let link = uint(at)?;
            ensure!(
                link == u32::MAX || (link as usize) < nif.blocks.len(),
                "Missing property controller {link}"
            );
            Ok((link != u32::MAX).then_some(link))
        }
        _ => anyhow::bail!("Property NiObjectNET prefix is unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::nif_file::{
        NiMaterialPropertyData, NiMeshData, NiNodeData, RawBlockData,
    };
    fn control(next: u32, little: bool) -> NiObject {
        let mut bytes = Vec::new();
        bytes.extend(if little {
            next.to_le_bytes()
        } else {
            next.to_be_bytes()
        });
        bytes.extend(if little {
            0x4cu16.to_le_bytes()
        } else {
            0x4cu16.to_be_bytes()
        });
        for v in [1f32.to_bits(), 0, 0, 10f32.to_bits(), 0] {
            bytes.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            });
        }
        NiObject::Unknown(RawBlockData {
            type_name: "NiVisController".into(),
            raw_data: bytes,
        })
    }
    #[test]
    fn startup_order_matches_native_properties_holes_shared_references_and_time() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/controller_lifecycle.json"
        ))?;
        assert_eq!(data["cases"].as_array().unwrap().len(), 15);
        for case in data["cases"].as_array().unwrap() {
            for little in [false, true] {
                let mut nif = NifFile {
                    version: 0x14020007,
                    source_is_little_endian: little,
                    roots: vec![Some(0)],
                    blocks: vec![
                        NiObject::Unknown(RawBlockData {
                            type_name: "NiObject".into(),
                            raw_data: vec![]
                        });
                        31
                    ],
                    ..Default::default()
                };
                for object in case["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .chain(case["properties"].as_array().unwrap())
                {
                    let id = object["id"].as_u64().unwrap() as usize;
                    let controls = object["controllers"].as_array().unwrap();
                    let head = controls.first().map(|v| v.as_u64().unwrap() as u32);
                    for (i, c) in controls.iter().enumerate() {
                        nif.blocks[c.as_u64().unwrap() as usize] = control(
                            controls
                                .get(i + 1)
                                .map_or(u32::MAX, |v| v.as_u64().unwrap() as u32),
                            little,
                        );
                    }
                    let properties = object["properties"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| Some(v.as_u64().unwrap() as u32))
                        .collect();
                    nif.blocks[id] = match object["class"].as_str().unwrap() {
                        "NiNode" => {
                            let mut node = NiNodeData::default();
                            node.base.transform.scale = 1.;
                            node.base.base.controller_link = head;
                            node.base.property_links = properties;
                            node.children = object["children"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| v.as_u64().map(|v| v as u32))
                                .collect();
                            NiObject::NiNode(node)
                        }
                        "NiMesh" => {
                            let mut mesh = NiMeshData::default();
                            mesh.base.base.transform.scale = 1.;
                            mesh.base.base.base.controller_link = head;
                            mesh.base.base.property_links = properties;
                            NiObject::NiMesh(mesh)
                        }
                        "NiMaterialProperty" => {
                            let mut property = NiMaterialPropertyData::default();
                            property.base.controller_link = head;
                            NiObject::NiMaterialProperty(property)
                        }
                        _ => unreachable!(),
                    };
                }
                let graph = Hierarchy::load(&nif)?;
                let plan = Plan::load(&nif, &graph, 0)?;
                let mut calls = Vec::new();
                plan.start(case["time"].as_f64().unwrap() as f32, |block, time| {
                    calls.push(serde_json::json!([block, time.to_bits()]));
                    Ok(())
                })?;
                assert_eq!(serde_json::json!(calls), case["calls"], "{case}");
            }
        }
        Ok(())
    }
    #[test]
    fn raw_property_prefix_handles_extra_links_and_rejects_truncation() -> Result<()> {
        for (little, version) in [false, true].into_iter().flat_map(|little| {
            [0x14020007, 0x14030109, 0x14060500].map(|version| (little, version))
        }) {
            let mut bytes = Vec::new();
            for v in [u32::MAX, 1, 0, 1] {
                bytes.extend(if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                });
            }
            let mut nif = NifFile {
                version,
                source_is_little_endian: little,
                blocks: vec![
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiTexturingProperty".into(),
                        raw_data: bytes,
                    }),
                    control(u32::MAX, little),
                ],
                ..Default::default()
            };
            assert_eq!(property_head(&nif, 0)?, Some(1));
            let NiObject::Unknown(p) = &mut nif.blocks[0] else {
                unreachable!()
            };
            p.raw_data.truncate(15);
            assert!(property_head(&nif, 0).is_err());
        }
        Ok(())
    }
}
