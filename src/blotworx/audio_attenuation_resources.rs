//! Class14 runtime resource storage. Native create8031e980, AddRef8031f370,
//! Release8031f3d0 and the already recovered payload reader define this state.
//! Loaded-bank ownership, memory pools and RTPC subscriptions remain separate.
use super::{
    audio_bank::{attenuation::Attenuation, Bank, Rtpc},
    audio_voice_init::Binding,
};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy, Debug)]
pub struct Storage {
    pub cone_bits: [u32; 4],
    pub flags61: u8,
}
struct Resource {
    id: u32,
    references: u32,
    definition: Arc<Attenuation>,
    cone_bits: [u32; 4],
    flags61: u8,
    tables: Vec<(u32, usize)>,
}
#[derive(Default)]
pub struct Resources {
    next: u32,
    index: BTreeMap<u32, u32>,
    resources: BTreeMap<u32, Resource>,
    tables: BTreeMap<u32, (u32, usize)>,
}
impl Resources {
    /// The original outer loader must ensure ID uniqueness: native Release
    /// removes an index entry by ID. Do not create ambiguous duplicate objects.
    pub fn create(
        &mut self,
        id: u32,
        definition: Arc<Attenuation>,
        storage: Storage,
    ) -> Result<u32> {
        ensure!(
            !self.index.contains_key(&id),
            "Duplicate live attenuation ID {id:08x} requires loaded-bank ownership resolution"
        );
        definition.validate_native_definition()?;
        let cone_bits = definition
            .native_cone()?
            .map_or(storage.cone_bits, |cone| cone.map(f32::to_bits));
        let flags61 = (storage.flags61 & 0x7f) | if definition.cone_flag != 0 { 0x80 } else { 0 };
        let indices = definition.runtime_rtpc_indices();
        let needed = u32::try_from(indices.len())
            .context("Attenuation table count overflow")?
            .checked_add(1)
            .context("Attenuation handle count overflow")?;
        let last = self
            .next
            .checked_add(needed)
            .context("Attenuation handle space exhausted")?;
        let handle = self.next + 1;
        let tables = indices
            .into_iter()
            .enumerate()
            .map(|(offset, index)| (handle + offset as u32 + 1, index))
            .collect::<Vec<_>>();
        for &(table, index) in &tables {
            self.tables.insert(table, (handle, index));
        }
        self.resources.insert(
            handle,
            Resource {
                id,
                references: 1,
                definition,
                cone_bits,
                flags61,
                tables,
            },
        );
        self.index.insert(id, handle);
        self.next = last;
        Ok(handle)
    }
    pub fn create_from_bank(
        &mut self,
        bank: &Bank<'_>,
        id: u32,
        storage: Storage,
    ) -> Result<Option<u32>> {
        bank.objects
            .get(&id)
            .filter(|object| object.kind == 14)
            .map(|object| self.create(id, Arc::new(object.attenuation()?), storage))
            .transpose()
    }
    pub fn acquire(&mut self, id: u32) -> Result<Option<u32>> {
        let Some(&handle) = self.index.get(&id) else {
            return Ok(None);
        };
        self.add_ref(handle)?;
        Ok(Some(handle))
    }
    pub fn add_ref(&mut self, handle: u32) -> Result<u32> {
        let resource = self
            .resources
            .get_mut(&handle)
            .context("Unknown attenuation handle")?;
        resource.references = resource.references.wrapping_add(1);
        Ok(resource.references)
    }
    pub fn release(&mut self, handle: u32) -> Result<u32> {
        let resource = self
            .resources
            .get_mut(&handle)
            .context("Unknown attenuation handle")?;
        resource.references = resource.references.wrapping_sub(1);
        let remaining = resource.references;
        if remaining == 0 {
            let resource = self.resources.remove(&handle).unwrap();
            self.index.remove(&resource.id);
            for (table, _) in resource.tables {
                self.tables.remove(&table);
            }
        }
        Ok(remaining)
    }
    pub fn references(&self, handle: u32) -> Option<u32> {
        self.resources.get(&handle).map(|r| r.references)
    }
    pub fn contains_id(&self, id: u32) -> bool {
        self.index.contains_key(&id)
    }
    pub fn cone_bits(&self, handle: u32) -> Result<[u32; 4]> {
        Ok(self
            .resources
            .get(&handle)
            .context("Unknown attenuation handle")?
            .cone_bits)
    }
    pub fn flags61(&self, handle: u32) -> Result<u8> {
        Ok(self
            .resources
            .get(&handle)
            .context("Unknown attenuation handle")?
            .flags61)
    }
    pub fn values(&self, handle: u32) -> Result<[f32; 2]> {
        let values = self.cone_bits(handle)?;
        Ok([f32::from_bits(values[2]), f32::from_bits(values[3])])
    }
    pub fn bindings(&self, handle: u32) -> Result<Vec<Binding>> {
        let resource = self
            .resources
            .get(&handle)
            .context("Unknown attenuation handle")?;
        resource
            .tables
            .iter()
            .map(|&(table, index)| {
                let rtpc = &resource.definition.rtpc[index];
                Ok(Binding {
                    words: [
                        rtpc.control,
                        rtpc.parameter,
                        rtpc.curve,
                        rtpc.scaling as u32,
                        table,
                        u32::try_from(rtpc.points.len())
                            .context("Attenuation point count overflow")?,
                    ],
                })
            })
            .collect()
    }
    pub fn curve(&self, table: u32) -> Result<&Rtpc> {
        let &(resource, index) = self
            .tables
            .get(&table)
            .context("Unknown attenuation curve handle")?;
        Ok(&self
            .resources
            .get(&resource)
            .context("Attenuation curve owner was released")?
            .definition
            .rtpc[index])
    }
    pub fn live_resources(&self) -> usize {
        self.resources.len()
    }
    pub fn live_tables(&self) -> usize {
        self.tables.len()
    }
}
impl super::audio_position_state::ResourceControl for Resources {
    fn release_attenuation(&mut self, resource: u32) -> Result<()> {
        self.release(resource)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn original_attenuation_construction_defaults_binding_tables_and_reference_lifecycle(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_attenuation_resources.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let encoded = case["encoded"].as_str().unwrap();
            let bytes = (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let fill = case["fill"].as_u64().unwrap() as u8;
            let id = case["id"].as_u64().unwrap() as u32;
            let mut resources = Resources::default();
            let handle = resources.create(
                id,
                Arc::new(Attenuation::parse(&bytes)?),
                Storage {
                    cone_bits: [u32::from_be_bytes([fill; 4]); 4],
                    flags61: fill,
                },
            )?;
            assert_eq!(
                json!(resources.cone_bits(handle)?),
                case["cone_bits"],
                "case {index}"
            );
            assert_eq!(
                resources.flags61(handle)? as u64,
                case["flags61"].as_u64().unwrap()
            );
            assert_eq!(
                resources.values(handle)?.map(f32::to_bits),
                [
                    resources.cone_bits(handle)?[2],
                    resources.cone_bits(handle)?[3]
                ]
            );
            let bindings = resources.bindings(handle)?;
            let mut actual = Vec::new();
            for binding in &bindings {
                let rtpc = resources.curve(binding.words[4])?;
                assert_eq!(binding.words[5] as usize, rtpc.points.len());
                actual.push(json!({"control":binding.words[0],"parameter":binding.words[1],"curve":binding.words[2],"scaling":binding.words[3],
                    "points":rtpc.points.iter().map(|p|[p.x.to_bits(),p.y.to_bits(),p.interpolation]).collect::<Vec<_>>()}));
            }
            assert_eq!(json!(actual), case["tracks"], "case {index}");
            resources.resources.get_mut(&handle).unwrap().references =
                case["initial_references"].as_u64().unwrap() as u32;
            for step in case["steps"].as_array().unwrap() {
                let count = if step["op"] == "add" {
                    resources.add_ref(handle)?
                } else {
                    resources.release(handle)?
                };
                assert_eq!(
                    count as u64,
                    step["count"].as_u64().unwrap(),
                    "case {index}"
                );
                assert_eq!(json!(resources.contains_id(id)), step["present"]);
                assert_eq!(
                    resources.live_resources() as u64,
                    step["index_count"].as_u64().unwrap()
                );
                if !resources.contains_id(id) {
                    assert_eq!(resources.live_tables(), 0);
                    for binding in &bindings {
                        assert!(resources.curve(binding.words[4]).is_err());
                    }
                }
            }
        }
        Ok(())
    }
    #[test]
    fn acquisition_and_position_cache_release_keep_bank_reference_and_class_namespace() -> Result<()>
    {
        let definition = Attenuation {
            cone_flag: 0,
            cone: None,
            mapping: [0, 255, 255, 255, 255],
            curves: vec![super::super::audio_bank::attenuation::Conversion {
                scaling: 0,
                points: vec![super::super::audio_bank::RtpcPoint {
                    x: 0.,
                    y: 0.,
                    interpolation: 4,
                }],
            }],
            rtpc: Vec::new(),
        };
        let mut resources = Resources::default();
        let storage = Storage {
            cone_bits: [0x12345678; 4],
            flags61: 0x7f,
        };
        let h = resources.create(55, Arc::new(definition.clone()), storage)?;
        assert!(resources.create(55, Arc::new(definition), storage).is_err());
        assert_eq!(resources.references(h), Some(1));
        assert_eq!(resources.acquire(55)?, Some(h));
        assert_eq!(resources.references(h), Some(2));
        let mut position = super::super::audio_position_state::State::new(
            super::super::audio_position_state::Storage {
                byte11: 0,
                scalar_bits: [0; 4],
            },
        );
        position.cached_attenuation = Some(h);
        position.release_cached(&mut resources)?;
        assert_eq!(resources.references(h), Some(1));
        assert_eq!(resources.release(h)?, 0);
        assert_eq!(resources.acquire(55)?, None);
        assert!(resources.release(h).is_err());
        let bank = Bank {
            id: 7,
            language: 0,
            feedback: false,
            media: Default::default(),
            objects: BTreeMap::from([(
                55,
                super::super::audio_bank::Object {
                    kind: 7,
                    payload: &[],
                },
            )]),
        };
        assert_eq!(resources.create_from_bank(&bank, 55, storage)?, None);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_attenuation_resources_create_acquire_and_release_without_stale_tables() -> Result<()> {
        fn collect(dir: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    collect(&path, paths)?;
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("bnk"))
                {
                    paths.push(path);
                }
            }
            Ok(())
        }
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut paths = Vec::new();
        collect(&root.join("Audio/Wii"), &mut paths)?;
        paths.sort();
        ensure!(!paths.is_empty(), "No owned banks");
        let mut resources = Resources::default();
        let mut definitions = 0;
        let mut cones = 0;
        let mut bindings_count = 0;
        for path in &paths {
            let bytes = std::fs::read(path)?;
            let bank = Bank::parse(&bytes)?;
            for (&id, object) in &bank.objects {
                if object.kind != 14 {
                    continue;
                }
                let handle = resources
                    .create_from_bank(
                        &bank,
                        id,
                        Storage {
                            cone_bits: [0; 4],
                            flags61: 0,
                        },
                    )
                    .with_context(|| format!("Create {} attenuation {id:08x}", path.display()))?
                    .unwrap();
                definitions += 1;
                if resources.flags61(handle)? & 0x80 != 0 {
                    cones += 1;
                }
                let bindings = resources.bindings(handle)?;
                bindings_count += bindings.len();
                for binding in &bindings {
                    assert_eq!(
                        resources.curve(binding.words[4])?.points.len(),
                        binding.words[5] as usize
                    );
                }
                assert_eq!(resources.acquire(id)?, Some(handle));
                assert_eq!(resources.release(handle)?, 1);
                assert_eq!(resources.release(handle)?, 0);
                assert!(!resources.contains_id(id));
                assert_eq!(resources.live_resources(), 0);
                assert_eq!(resources.live_tables(), 0);
            }
        }
        std::fs::create_dir_all("target")?;
        std::fs::write(
            "target/audio-attenuation-resource-corpus.json",
            serde_json::to_vec_pretty(
                &json!({"scope":"Independent bank-local resource lifecycles, not loaded-bank ownership or playback parity",
            "banks":paths.iter().map(|p|p.strip_prefix(&root).unwrap().to_string_lossy()).collect::<Vec<_>>(),"definitions":definitions,"cones":cones,"bindings":bindings_count}),
            )?,
        )?;
        Ok(())
    }
}
