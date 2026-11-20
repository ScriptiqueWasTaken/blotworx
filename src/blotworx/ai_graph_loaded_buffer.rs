//! Loader Pointer binding to a single owned runtime allocation. No caller
//! payload offset assumption and no separate point/edge or container copy.
use super::{
    ai_graph_buffers::Binding,
    ai_graph_link_passes::{Memory, View},
    ai_graph_normalization::{self as normalization, Auxiliary, Layout},
};
use anyhow::{ensure, Context as _, Result};
pub struct Allocation {
    pub identity: usize,
    pub base: u32,
    pub bytes: Vec<u8>,
}
impl Memory {
    pub fn bind_loaded_allocation(
        &mut self,
        resource: u32,
        mut allocation: Allocation,
        binding: &Binding,
        layout: Layout,
    ) -> Result<u32> {
        ensure!(
            binding.payload.allocation == Some(allocation.identity),
            "Graph payload refers to another allocation"
        );
        ensure!(
            binding.header == binding.payload && binding.points == binding.payload.add(8),
            "Graph loader binding is incomplete"
        );
        ensure!(
            !self.resources.contains_key(&resource),
            "Graph resource already has a bound payload"
        );
        let size = u32::try_from(allocation.bytes.len())?;
        let end = allocation
            .base
            .checked_add(size)
            .context("Graph allocation address overflow")?;
        for (&base, data) in &self.regions {
            let other_end = base
                .checked_add(u32::try_from(data.len())?)
                .context("Existing graph allocation overflow")?;
            ensure!(
                end <= base || allocation.base >= other_end,
                "Graph allocation overlaps live storage"
            );
        }
        let offset = binding.payload.offset;
        let d = normalization::normalize_in_place(
            allocation
                .bytes
                .get_mut(offset as usize..)
                .context("Loader payload is outside its allocation")?,
            layout,
        )?;
        let base = allocation
            .base
            .checked_add(offset)
            .context("Graph payload pointer overflow")?;
        let (stride, edge_stride) = match layout {
            Layout::Fixed => (32, 16),
            Layout::Dynamic {
                node_stride,
                edge_stride,
            } => (node_stride, edge_stride),
        };
        let view = View {
            payload: Some(base),
            header: base,
            points: base.wrapping_add(d.points),
            edges: Some(base.wrapping_add(d.edges)),
            edge_stride: Some(edge_stride),
            external: d.external.map_or(0, |o| base.wrapping_add(o)),
            external_count: d.external_count,
            stride,
            auxiliary: Some(match d.field38 {
                Auxiliary::Scalar(v) => Auxiliary::Scalar(v),
                Auxiliary::Tail(o) => Auxiliary::Tail(base.wrapping_add(o)),
            }),
        };
        self.regions.insert(allocation.base, allocation.bytes);
        self.resources.insert(resource, view);
        Ok(size - offset)
    }
}
#[cfg(test)]
mod tests {
    use super::super::ai_graph_buffers::Pointer;
    use super::*;
    fn binding(identity: usize, offset: u32) -> Binding {
        let mut b = Binding {
            payload: Pointer {
                allocation: Some(identity),
                offset,
            },
            header: Pointer::default(),
            points: Pointer::default(),
        };
        b.bind();
        b
    }
    #[test]
    fn loaded_buffer_uses_actual_binding_offset_and_keeps_one_owned_allocation() -> Result<()> {
        let mut bytes = vec![0xa5; 32];
        bytes.extend_from_slice(&[0; 8]);
        let allocation = bytes.as_ptr();
        let mut m = Memory::default();
        let size = m.bind_loaded_allocation(
            7,
            Allocation {
                identity: 11,
                base: 0x1000,
                bytes,
            },
            &binding(11, 32),
            Layout::Dynamic {
                node_stride: 36,
                edge_stride: 12,
            },
        )?;
        assert_eq!(size, 8);
        assert_eq!(m.regions[&0x1000].as_ptr(), allocation);
        assert_eq!(&m.regions[&0x1000][..32], &[0xa5; 32]);
        assert_eq!(m.view(7)?.payload, Some(0x1020));
        assert_eq!(m.view(7)?.points, 0x1028);
        assert_eq!(m.regions.len(), 1);
        Ok(())
    }
    #[test]
    fn invalid_allocation_identity_or_incomplete_binding_never_publishes() -> Result<()> {
        let mut m = Memory::default();
        assert!(m
            .bind_loaded_allocation(
                7,
                Allocation {
                    identity: 12,
                    base: 0x1000,
                    bytes: vec![0; 8]
                },
                &binding(11, 0),
                Layout::Fixed
            )
            .is_err());
        let mut b = binding(12, 0);
        b.points = Pointer::default();
        assert!(m
            .bind_loaded_allocation(
                7,
                Allocation {
                    identity: 12,
                    base: 0x1000,
                    bytes: vec![0; 8]
                },
                &b,
                Layout::Fixed
            )
            .is_err());
        assert!(m.regions.is_empty() && m.resources.is_empty());
        Ok(())
    }
}
#[cfg(test)]
mod owned_tests {
    use super::super::{
        ai_graph_buffers::{Bindings, Pointer},
        ai_graph_handles as handles, ai_graph_loading as loading,
        ai_graph_resource_group::{Input, Inputs},
    };
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    fn hash(b: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, b)
            .as_ref()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect()
    }
    struct FinalizationEvents { resource:u32, arrivals:usize }
    impl super::super::ai_graph_finalize::Host for FinalizationEvents {
        fn kind(&mut self,_:&mut Memory,r:u32)->Result<u32> {
            ensure!(r==self.resource,"Unexpected finalized resource");
            Ok(1) // This test constructs the original concrete/dynamic resource class.
        }
        fn tag_edge(&mut self,_:&mut Memory,_:&mut super::super::ai_graph_finalize::Context,_:bool,_:u32,_:u32,_:u32)->Result<()> {
            anyhow::bail!("Disabled policy service dispatched an edge")
        }
    }
    impl super::super::ai_graph_finalize_lifecycle::Events for FinalizationEvents {
        fn arrived(&mut self,_:&mut Memory,_:&mut super::super::ai_graph_admission::Admission,_:&mut loading::Loader,s:&mut handles::State,r:Option<u32>,listener:super::super::ai_graph_admission::Listener)->Result<()> {
            ensure!(r==Some(self.resource) && s.resources[&self.resource].state==5,"Listener ran before completed finalization");
            ensure!(listener.function==1 && listener.data==7,"Listener identity changed");
            self.arrivals+=1;Ok(())
        }
        fn clock_bits(&mut self,_:&mut Memory,_:&mut super::super::ai_graph_admission::Admission,_:&mut loading::Loader,_:&mut handles::State)->Result<u32> {
            anyhow::bail!("This resource-only test has no registered agent clock")
        }
    }
    struct FileIo {
        allocation: Option<Allocation>,
        expected_id: u32,
    }
    impl loading::Io for FileIo {
        fn direct(
            &mut self,
            _: &mut loading::Loader,
            _: &mut handles::State,
            inputs: &mut Inputs,
            _: &mut Bindings,
            r: loading::Direct,
            out: &mut Pointer,
        ) -> Result<u32> {
            ensure!(
                r.id == self.expected_id,
                "Wrong direct-file request identity"
            );
            *out = Pointer {
                allocation: Some(
                    self.allocation
                        .as_ref()
                        .context("Missing loaded allocation")?
                        .identity,
                ),
                offset: 0,
            };
            inputs.resources.get_mut(&r.resource).unwrap().word24 = 1;
            Ok(0)
        }
        fn packed(
            &mut self,
            _: &mut loading::Loader,
            _: &mut handles::State,
            _: &mut Inputs,
            _: &mut Bindings,
            _: loading::Packed,
        ) -> Result<u32> {
            anyhow::bail!("Direct-file regression requested packed IO")
        }
    }
    #[test]
    #[ignore = "requires immutable imported EM1 data in BLOTWORX_TEST_DATA"]
    fn original_direct_loader_and_group_gate_handoff_owned_gv_dbc_building_files() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_normalization_owned.json"
        ))?;
        let template: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_metadata.json"
        ))?;
        let finalized:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_finalize_owned.json"))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let path = root.join(case["path"].as_str().unwrap());
            let original = std::fs::read(&path)?;
            assert_eq!(hash(&original), case["source_sha256"].as_str().unwrap());
            let prefix: Vec<u8> = original.iter().copied().take(16).collect();
            let id = u32::from_le_bytes(
                original
                    .get(4..8)
                    .context("Missing sector identity")?
                    .try_into()?,
            );
            let payload_size = u32::try_from(original.len())?
                .checked_sub(16)
                .context("Missing direct-file prefix")?;
            let allocation_pointer = original.as_ptr();
            let mut graph: handles::State =
                serde_json::from_value(template["cases"][0]["input"]["state"].clone())?;
            graph.resources.clear();
            graph.resources.insert(
                id,
                handles::Resource {
                    size18: None,
                    byte8: 0,
                    priority: 0,
                    state: 1,
                    word1c: 1,
                },
            );
            graph.pool.clear();
            let layout = Layout::Dynamic {
                node_stride: 36,
                edge_stride: 12,
            };
            let mut inputs = Inputs {
                resources: BTreeMap::from([(
                    id,
                    Input {
                        word24: 99,
                        layout,
                        source: vec![],
                        normalized: None,
                    },
                )]),
            };
            let mut bindings = Bindings {
                resources: BTreeMap::from([(
                    id,
                    super::super::ai_graph_buffers::Binding {
                        payload: Pointer::default(),
                        header: Pointer::default(),
                        points: Pointer::default(),
                    },
                )]),
            };
            // File callback/directory metadata are supplied, matching the already native-
            // validated loader contract. The caller supplies no payload slice or offset.
            let mut loader = loading::Loader {
                source: 1,
                direct_mode: 1,
                name: vec![],
                extra_records: 0,
                directory: vec![1],
                records: BTreeMap::from([(
                    1,
                    loading::Record {
                        id,
                        word1c: 0,
                        size20: payload_size,
                        offset24: 0,
                        word28: 0,
                        word2c: 0,
                    },
                )]),
                bad: vec![0],
                groups: [
                    Some(super::super::ai_graph_resource_group::Group {
                        resources: vec![id],
                    }),
                    None,
                ],
            };
            let base = finalized["base"].as_u64().unwrap() as u32 - 16;
            let mut io = FileIo {
                allocation: Some(Allocation {
                    identity: 13,
                    base,
                    bytes: original,
                }),
                expected_id: id,
            };
            assert_eq!(
                loader.start_next(&mut graph, &mut inputs, &mut bindings, &mut io)?,
                0
            );
            let b = &bindings.resources[&id];
            let mut memory = Memory::default();
            assert_eq!(
                graph.resources[&id].state, 2,
                "Load did not enter native pending conversion"
            );
            let owner = io.allocation.take().unwrap();
            let mut pending = BTreeMap::from([(owner.identity, owner)]);
            loader.groups[0].as_ref().unwrap().normalize_loaded(
                &mut graph,
                &mut inputs,
                &bindings,
                &mut pending,
                &mut memory,
            )?;
            assert!(pending.is_empty());
            let count = u32::try_from(memory.regions[&base].len())? - b.payload.offset;
            assert_eq!(count, graph.resources[&id].size18.unwrap());
            assert_eq!(
                graph.resources[&id].state, 3,
                "Conversion did not enter native linking state"
            );
            assert!(
                inputs.resources[&id].source.is_empty()
                    && inputs.resources[&id].normalized.is_none()
            );
            assert_eq!(memory.regions[&base].as_ptr(), allocation_pointer);
            assert_eq!(&memory.regions[&base][..prefix.len()], prefix.as_slice());
            let offset = (memory.view(id)?.payload.unwrap() - base) as usize;
            assert_eq!(
                serde_json::to_value(memory.view(id)?.auxiliary.unwrap())?,
                case["payload"]["field38"]
            );
            assert_eq!(
                hash(&memory.regions[&base][offset..]),
                case["payload"]["sha256"].as_str().unwrap()
            );
            assert_eq!(
                memory.word(memory.view(id)?.header)?,
                case["payload"]["node_count"].as_u64().unwrap() as u32
            );
            // Run actual admission scheduling and finalization over the one
            // owned allocation. Policy service is deliberately disabled, as in
            // this original-code fixture. This does not construct a World agent.
            let expected=&finalized["cases"][index];
            assert_eq!(expected["path"],case["path"]);
            memory.stride=36;
            for (address,size) in [(0x64000000,32),(0x64100000,8),(0x64200000,32),(0x64300000,32)] {memory.regions.insert(address,vec![0;size]);}
            for (off,value) in [(8,0x64100000),(12,1),(16,0x64200000),(24,0x64300000)] {memory.write(0x64000000+off,value)?;}
            loader.groups[1]=Some(super::super::ai_graph_resource_group::Group{resources:vec![]});
            graph.tracked_resource=None;graph.tracked158=0;graph.tracked160=0;
            let mut admission=super::super::ai_graph_admission::Admission{index154:0,index15c:0,word82e4:0,changed14:0,listeners:vec![super::super::ai_graph_admission::Listener{function:1,data:7}]};
            let context=super::super::ai_graph_finalize::test_context(&mut memory,serde_json::from_value(finalized["attributes"].clone())?,false,true,0xdeadbeef,0x64000000)?;
            let lifecycle=super::super::ai_graph_finalize_lifecycle::Native{
                context,
                callbacks:FinalizationEvents{resource:id,arrivals:0},
            };
            let mut host=super::super::ai_graph_linking::Native{
                directory:super::super::ai_graph_linking::Directory{float_mode:1,bounds:BTreeMap::new()},
                metadata:super::super::ai_graph_queries::Metadata::default(),
                backend:super::super::ai_graph_link_passes::Concrete{memory,lifecycle},
            };
            admission.tick(&mut loader,&mut graph,&mut host)?;
            assert_eq!(graph.resources[&id].state,4);
            admission.tick(&mut loader,&mut graph,&mut host)?;
            assert_eq!(graph.resources[&id].state,4);
            admission.tick(&mut loader,&mut graph,&mut host)?;
            assert_eq!(graph.resources[&id].state,5);
            assert!(graph.tracked_resource.is_none() && graph.pool.is_empty());
            assert_eq!(host.backend.lifecycle.callbacks.arrivals,1);
            assert_eq!(host.backend.memory.word(0x6d000000)?,expected["slot"].as_u64().unwrap() as u32);
            let memory=&host.backend.memory;
            assert_eq!(memory.regions[&base].as_ptr(),allocation_pointer,"Finalization copied the graph allocation");
            assert_eq!(&memory.regions[&base][..16],prefix.as_slice());
            assert_eq!(hash(&memory.regions[&base][offset..]),expected["payload_sha256"].as_str().unwrap());
            for (name,address) in [("provider",0x64000000),("slots",0x64100000),("removed",0x64200000),("added",0x64300000)] {assert_eq!(serde_json::to_value(&memory.regions[&address])?,expected[name],"{name}");}
            assert_eq!(
                hash(&std::fs::read(&path)?),
                case["source_sha256"].as_str().unwrap()
            );
        }
        Ok(())
    }
}
