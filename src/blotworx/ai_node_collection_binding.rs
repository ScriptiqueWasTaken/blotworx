//! Canonical graph-arena provider for retail node collection. No point copies,
//! alternate node database or guessed ready/clear query is maintained.
use super::{ai_graph_link_passes::Memory, ai_nearest_search, ai_node_collection as collection};
use anyhow::{ensure, Result};
pub trait Backend {
    fn marker_index(&mut self, memory: &mut Memory) -> Result<u32>;
    fn filter(
        &mut self,
        memory: &mut Memory,
        key: u32,
        source: u32,
        target: u32,
        cost: u32,
        data: u32,
    ) -> Result<u32>;
    fn clock(&mut self, memory: &mut Memory) -> Result<u32>;
    fn coefficient(&mut self, memory: &mut Memory, clock: u32, index: u32) -> Result<f32>;
}
pub struct Binding<'a, B> {
    pub memory: &'a mut Memory,
    pub backend: &'a mut B,
}
impl<B: Backend> collection::Host for Binding<'_, B> {
    fn read(&mut self, p: u32) -> Result<u32> {
        self.memory.word(p)
    }
    fn write(&mut self, p: u32, value: u32) -> Result<()> {
        self.memory.write(p, value)
    }
    fn byte(&mut self, p: u32) -> Result<u8> {
        self.memory.byte(p)
    }
    fn half(&mut self, p: u32) -> Result<u16> {
        ensure!(p & 1 == 0, "Unaligned graph index");
        Ok(u16::from_be_bytes([
            self.memory.byte(p)?,
            self.memory.byte(p.wrapping_add(1))?,
        ]))
    }
    fn count(&mut self, r: u32) -> Result<u32> {
        // Actual51ea08+18=475108 and51ea58+18=47505c both read resource28->word0.
        // Another virtual class needs its own implementation, not a default count.
        let class = self.memory.word(r)?;
        ensure!(
            matches!(class, 0x51ea08 | 0x51ea58),
            "Unsupported node collection resource class {class:08x}"
        );
        let header = self.memory.word(r.wrapping_add(0x28))?;
        self.memory.word(header)
    }
    fn marker_index(&mut self) -> Result<u32> {
        self.backend.marker_index(self.memory)
    }
    fn filter(&mut self, key: u32, source: u32, target: u32, cost: u32, data: u32) -> Result<u32> {
        self.backend
            .filter(self.memory, key, source, target, cost, data)
    }
    fn clock(&mut self) -> Result<u32> {
        self.backend.clock(self.memory)
    }
    fn coefficient(&mut self, clock: u32, index: u32) -> Result<f32> {
        self.backend.coefficient(self.memory, clock, index)
    }
}
impl<B: Backend> Binding<'_, B> {
    pub fn query(&mut self, context: collection::Context, args: collection::Args) -> Result<u32> {
        collection::query(self, context, args)
    }
    /// Actual47e5a8 collection mode: pointer output and sorting are both1. Bounds
    /// live in this call's reserved frame, separate from point/filter temporaries.
    pub fn nearest(
        &mut self,
        context: collection::Context,
        graph: usize,
        request: ai_nearest_search::Collection,
    ) -> Result<u32> {
        let minimum = context.frame.wrapping_add(0x140);
        let maximum = context.frame.wrapping_add(0x150);
        for i in 0..3 {
            self.memory
                .write(minimum + i * 4, request.minimum[i as usize].to_bits())?;
            self.memory
                .write(maximum + i * 4, request.maximum[i as usize].to_bits())?;
        }
        self.query(
            context,
            collection::Args {
                graph: u32::try_from(graph)?,
                minimum,
                maximum,
                output: request.output,
                capacity: request.capacity,
                pointers: 1,
                sort: 1,
                filter: request.filter.function,
                filter_data: request.filter.data,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::ai_node_collection::tests::replay;
    use super::*;
    use serde_json::{json, Value};
    struct Services<'a> {
        input: &'a Value,
        trace: Vec<Value>,
    }
    fn xyz(memory: &Memory, p: u32) -> Result<[u32; 3]> {
        Ok([
            memory.word(p + 4)?,
            memory.word(p + 8)?,
            memory.word(p + 12)?,
        ])
    }
    impl Backend for Services<'_> {
        fn marker_index(&mut self, _: &mut Memory) -> Result<u32> {
            Ok(7)
        }
        fn filter(
            &mut self,
            memory: &mut Memory,
            key: u32,
            source: u32,
            target: u32,
            cost: u32,
            data: u32,
        ) -> Result<u32> {
            assert_eq!(key, 1);
            let marker = memory.word(target + 16 + 7 * 4)?;
            let id = memory.word(marker + 4)?;
            self.trace.push(json!([
                "filter",
                xyz(memory, source)?,
                xyz(memory, target)?,
                id,
                data
            ]));
            let entry = self.input["filter"].get(id.to_string());
            let result = entry.map_or(1, |e| e[0].as_u64().unwrap() as u32);
            let bits = entry.map_or_else(
                || ((id & 0x7fffffff) as f32).to_bits(),
                |e| e[1].as_u64().unwrap() as u32,
            );
            memory.write(cost, bits)?;
            if self.input["mutate"] == "flags" {
                let value = memory.word(0x62e60)?;
                memory.write(0x62e60, value | 0x100)?;
            }
            Ok(result)
        }
        fn clock(&mut self, _: &mut Memory) -> Result<u32> {
            self.trace.push(json!(["clock"]));
            Ok(0x67000000)
        }
        fn coefficient(&mut self, _: &mut Memory, clock: u32, index: u32) -> Result<f32> {
            self.trace.push(json!(["coefficient", clock, index]));
            Ok(f32::from_bits(
                self.input["coefficient"].as_u64().unwrap() as u32
            ))
        }
    }
    #[test]
    fn node_collection_uses_canonical_arena_resources_outputs_and_callback_data() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_node_collection.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            for adapter in [false, true] {
                if adapter
                    && (input["pointers"].as_u64().unwrap() & 255 != 1
                        || input["sort"].as_u64().unwrap() & 255 != 1)
                {
                    continue;
                }
                let (original, size) = replay(input)?;
                let mut memory = Memory::default();
                let mut region = None;
                let mut next = 0;
                for (address, byte) in original.bytes {
                    if region.is_none() || address != next {
                        region = Some(address);
                        memory.regions.insert(address, vec![]);
                    }
                    memory.regions.get_mut(&region.unwrap()).unwrap().push(byte);
                    next = address + 1;
                }
                let mut services = Services {
                    input,
                    trace: vec![],
                };
                let context = collection::Context {
                    scratch: 0x63000000,
                    flags: 0x62e60,
                    visited_total: 0x61e50,
                    frame: 0x70000000,
                    point_kind: 0x4ec1b0,
                    marker_kind: 0x4ebe80,
                };
                let args = collection::Args {
                    graph: 0x60000000,
                    minimum: 0x61000000,
                    maximum: 0x61000010,
                    output: 0x62000000,
                    capacity: input["capacity"].as_u64().unwrap() as u32,
                    pointers: input["pointers"].as_u64().unwrap() as u32,
                    sort: input["sort"].as_u64().unwrap() as u32,
                    filter: if input["filter"].is_null() { 0 } else { 1 },
                    filter_data: 0x68000000,
                };
                let mut binding = Binding {
                    memory: &mut memory,
                    backend: &mut services,
                };
                let result = if adapter {
                    binding.nearest(
                        context,
                        args.graph as usize,
                        ai_nearest_search::Collection {
                            minimum: std::array::from_fn(|i| {
                                f32::from_bits(input["minimum"][i].as_u64().unwrap() as u32)
                            }),
                            maximum: std::array::from_fn(|i| {
                                f32::from_bits(input["maximum"][i].as_u64().unwrap() as u32)
                            }),
                            capacity: args.capacity,
                            output: args.output,
                            filter: ai_nearest_search::Filter {
                                function: args.filter,
                                data: args.filter_data,
                            },
                        },
                    )?
                } else {
                    binding.query(context, args)?
                };
                assert_eq!(
                    json!(result),
                    case["result"],
                    "result {index} adapter={adapter}"
                );
                let output: Vec<_> = (0..result)
                    .map(|i| memory.word(0x62000000 + i * 4).unwrap())
                    .collect();
                assert_eq!(
                    json!(output),
                    case["output"],
                    "output {index} adapter={adapter}"
                );
                let scratch: Vec<_> = (0..size * 2)
                    .map(|i| memory.word(0x63000000 + i * 4).unwrap())
                    .collect();
                assert_eq!(
                    json!(scratch),
                    case["scratch"],
                    "scratch {index} adapter={adapter}"
                );
                assert_eq!(json!(memory.word(0x62e60)?), case["flags"], "flags {index}");
                assert_eq!(
                    json!(memory.word(0x61e50)?),
                    case["visited"],
                    "visited {index}"
                );
                assert_eq!(
                    json!(memory.word(0x6700007c)?),
                    case["clock"],
                    "clock {index}"
                );
                let trace: Vec<_> = case["trace"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|v| v[0] != "count")
                    .cloned()
                    .collect();
                assert_eq!(
                    services.trace, trace,
                    "service trace {index} adapter={adapter}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn node_collection_count_reads_both_native_classes_and_live_header_without_cache() -> Result<()>
    {
        use collection::Host as _;
        let mut memory = Memory::default();
        memory.regions.insert(0x1000, vec![0; 0x40]);
        memory.regions.insert(0x2000, vec![0; 4]);
        memory.write(0x1000, 0x51ea08)?;
        memory.write(0x1028, 0x2000)?;
        memory.write(0x2000, 3)?;
        let input = json!({});
        let mut services = Services {
            input: &input,
            trace: vec![],
        };
        let mut binding = Binding {
            memory: &mut memory,
            backend: &mut services,
        };
        assert_eq!(binding.count(0x1000)?, 3);
        binding.memory.write(0x1000, 0x51ea58)?;
        binding.memory.write(0x2000, 17)?;
        assert_eq!(binding.count(0x1000)?, 17);
        binding.memory.write(0x1000, 0x1234)?;
        assert!(binding.count(0x1000).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod owned_tests {
    use super::super::{
        ai_graph_link_passes::View,
        ai_graph_normalization::{self, Layout},
    };
    use super::*;
    use anyhow::Context as _;
    use serde_json::{json, Value};
    struct Services;
    impl Backend for Services {
        fn marker_index(&mut self, _: &mut Memory) -> Result<u32> {
            anyhow::bail!("Unfiltered owned query requested marker metadata")
        }
        fn filter(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unfiltered owned query requested goto cost")
        }
        fn clock(&mut self, _: &mut Memory) -> Result<u32> {
            anyhow::bail!("Unprofiled owned query requested clock")
        }
        fn coefficient(&mut self, _: &mut Memory, _: u32, _: u32) -> Result<f32> {
            anyhow::bail!("Unprofiled owned query requested coefficient")
        }
    }
    fn hash(bytes: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    #[test]
    #[ignore = "requires immutable imported EM1 files in BLOTWORX_TEST_DATA"]
    fn node_collection_original_dbc_gv_and_building_sector_indexes_match() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_node_collection_owned.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let path = case["source"]["path"].as_str().unwrap();
            let source = std::fs::read(root.join(path))?;
            assert_eq!(
                hash(&source),
                case["source"]["sha256"].as_str().unwrap(),
                "{path}"
            );
            // This is the explicitly selected sector payload, not a claim that the
            // production source-container handoff or World admission is implemented.
            let offset = case["source"]["offset"].as_u64().unwrap() as usize;
            let payload = ai_graph_normalization::convert(
                source.get(offset..).context("Selected payload missing")?,
                Layout::Dynamic {
                    node_stride: 36,
                    edge_stride: 12,
                },
            )?;
            let count = payload.node_count;
            let size = count.saturating_add(2).max(32);
            let mut memory = Memory {
                stride: 36,
                ..Memory::default()
            };
            for (p, n) in [
                (0x60000000, 0x100),
                (0x61000000, 0x20),
                (0x62000000, size * 4),
                (0x63000000, size * 8),
                (0x64000000, 0x100),
                (0x65000000, 0x40),
                (0x65000800, 4),
                (0x67000000, 0x80),
                (0x61e50, 4),
                (0x62e60, 4),
                (0x70000000, 0x1a0),
            ] {
                memory.regions.insert(p, vec![0; n as usize]);
            }
            let points = payload
                .bytes
                .get(payload.points as usize..(payload.points + count * 36) as usize)
                .context("Selected point records truncated")?
                .to_vec();
            memory.regions.insert(0x65001000, points);
            memory.resources.insert(
                0x65000000,
                View {auxiliary:None,payload:None,edges:None,edge_stride:None,
                    header: 0x65000800,
                    points: 0x65001000,
                    external: 0,
                    external_count: 0,
                    stride: 36,
                },
            );
            memory.write(0x60000014, 0x65000000)?;
            memory.write(0x60000048, u32::MAX)?;
            memory.write(0x65000000, 0x51ea08)?;
            memory.write(0x65000004, 0x64000000)?;
            memory.write(0x65000028, 0x65000800)?;
            memory.write(0x6500002c, 0x65001000)?;
            memory.write(0x65000800, count)?;
            memory.write(0x6400007c, 36)?;
            memory.write(0x6400001c, 2)?;
            memory.write(0x61e50, 7)?;
            memory.write(0x62e60, 0xdead)?;
            memory.write(0x6700007c, 0.5f32.to_bits())?;
            memory.convert_vertex_triplets(0x65000000, &[[0, 2, 0], [0; 3], [0; 3], [0; 3]])?;
            for i in 0..3 {
                memory.write(
                    0x61000000 + i * 4,
                    case["query"]["minimum"][i as usize].as_u64().unwrap() as u32,
                )?;
                memory.write(
                    0x61000010 + i * 4,
                    case["query"]["maximum"][i as usize].as_u64().unwrap() as u32,
                )?;
            }
            let mut services = Services;
            let result = Binding {
                memory: &mut memory,
                backend: &mut services,
            }
            .query(
                collection::Context {
                    scratch: 0x63000000,
                    flags: 0x62e60,
                    visited_total: 0x61e50,
                    frame: 0x70000000,
                    point_kind: 0x4ec1b0,
                    marker_kind: 0x4ebe80,
                },
                collection::Args {
                    graph: 0x60000000,
                    minimum: 0x61000000,
                    maximum: 0x61000010,
                    output: 0x62000000,
                    capacity: case["query"]["capacity"].as_u64().unwrap() as u32,
                    pointers: 1,
                    sort: 1,
                    filter: 0,
                    filter_data: 0x68000000,
                },
            )?;
            assert_eq!(json!(result), case["result"], "result {index} {path}");
            let output: Vec<_> = (0..result)
                .map(|i| memory.word(0x62000000 + i * 4).unwrap())
                .collect();
            assert_eq!(json!(output), case["output"], "pointers {index}");
            let ids: Vec<_> = output
                .into_iter()
                .map(|p| memory.word(p).unwrap())
                .collect();
            assert_eq!(json!(ids), case["node_ids"], "IDs {index}");
            assert_eq!(
                hash(&memory.regions[&0x63000000]),
                case["scratch_sha256"].as_str().unwrap(),
                "scratch {index}"
            );
            assert_eq!(json!(memory.word(0x62e60)?), case["flags"], "flags {index}");
            assert_eq!(
                json!(memory.word(0x61e50)?),
                case["visited"],
                "visited {index}"
            );
            assert_eq!(
                json!(memory.word(0x6700007c)?),
                case["clock"],
                "clock {index}"
            );
        }
        Ok(())
    }
}
