//! Native connection services over canonical typed handles and shared buffers.
use super::{
    ai_astar_graph_services::GraphContext,
    ai_graph_connection::{self as native, Host},
    ai_graph_link_passes::Memory,
    ai_graph_points::Table,
    ai_graph_queries::{Class, Point},
};
use anyhow::{ensure, Context as _, Result};
pub struct Binding<'a, 'b> {
    pub graph: &'a mut GraphContext<'b, Memory>,
}
pub fn point_address(m: &Memory, p: Option<Point>) -> Result<u32> {
    let Some(p) = p else { return Ok(0) };
    let view = m.view(p.resource)?;
    Ok(match p.location.table {
        Table::Internal => view.points,
        Table::External => view.external,
    }
    .wrapping_add(p.location.offset))
}
impl Host for Binding<'_, '_> {
    fn read(&mut self, p: u32) -> Result<u32> {
        self.graph.tables.word(p)
    }
    fn write(&mut self, p: u32, v: u32) -> Result<()> {
        self.graph.tables.write(p, v)
    }
    fn byte(&mut self, p: u32) -> Result<u8> {
        self.graph.tables.byte(p)
    }
    fn handle(&mut self, id: u32, offset: u32) -> Result<u32> {
        let n = self
            .graph
            .graph
            .nodes
            .get(&(id as usize))
            .context("Connection handle is absent")?;
        Ok(match offset {
            0 => n.connection.unwrap_or(0),
            4 => point_address(self.graph.tables, n.point)?,
            8 => n.graph.unwrap_or(0),
            12 => u32::try_from(n.traversal.unwrap_or(0))?,
            0x14 => n.word14,
            0x1c => u32::try_from(
                n.manager_1c
                    .context("Graph handle manager ownership is unwritten")?,
            )?,
            0x3c => n.word3c,
            0x40 => u32::try_from(n.next.unwrap_or(0))?,
            _ => anyhow::bail!("Unbound connection handle field {offset:#x}"),
        })
    }
    fn set_handle(&mut self, id: u32, offset: u32, v: u32) -> Result<()> {
        let n = self
            .graph
            .graph
            .nodes
            .get_mut(&(id as usize))
            .context("Connection handle is absent")?;
        match offset {
            0 => n.connection = (v != 0).then_some(v),
            8 => n.graph = (v != 0).then_some(v),
            12 => n.traversal = (v != 0).then_some(v as usize),
            _ => anyhow::bail!("Unbound connection handle write {offset:#x}"),
        };
        Ok(())
    }
    fn kind(&mut self, r: u32) -> Result<u32> {
        Ok(
            match self
                .graph
                .metadata
                .resources
                .get(&r)
                .context("Connection resource class is absent")?
                .class
            {
                Class::Concrete => 1,
                Class::Abstract => 0,
            },
        )
    }
    fn resource_state(&mut self, r: u32) -> Result<u32> {
        Ok(self
            .graph
            .graph
            .resources
            .get(&r)
            .context("Connection resource is absent")?
            .state)
    }
    fn resource_range(&mut self, r: u32) -> Result<(u32, u32)> {
        let size = self
            .graph
            .graph
            .resources
            .get(&r)
            .context("Connection resource is absent")?
            .size18
            .context("Connection resource allocation size is unwritten")?;
        Ok((
            self.graph
                .tables
                .view(r)?
                .payload
                .context("Connection payload binding is unwritten")?,
            size,
        ))
    }
    fn points(&mut self, r: u32) -> Result<u32> {
        Ok(self.graph.tables.view(r)?.points)
    }
    fn edges(&mut self, r: u32) -> Result<u32> {
        self.graph
            .tables
            .view(r)?
            .edges
            .context("Connection edge binding is unwritten")
    }
    fn point_stride(&mut self, r: u32) -> Result<u32> {
        Ok(self.graph.tables.view(r)?.stride)
    }
    fn edge_stride(&mut self, r: u32) -> Result<u32> {
        self.graph
            .tables
            .view(r)?
            .edge_stride
            .context("Connection edge stride is unwritten")
    }
    fn edge_count(&mut self, r: u32) -> Result<u32> {
        let header = self.graph.tables.view(r)?.header;
        self.graph.tables.word(header.wrapping_add(4))
    }
}
pub fn connection_index(
    g: &mut GraphContext<'_, Memory>,
    c: native::Context,
    handle: usize,
    target: Option<Point>,
) -> Result<u32> {
    let target = point_address(g.tables, target)?;
    native::connection_index(
        &mut Binding { graph: g },
        c,
        u32::try_from(handle)?,
        0,
        target,
        0,
    )
}
pub fn traversal(g: &mut GraphContext<'_, Memory>, handle: usize) -> Result<Option<usize>> {
    Ok(native::traversal(&mut Binding { graph: g }, u32::try_from(handle)?)?.map(|v| v as usize))
}
#[cfg(test)]
mod tests {
    use super::super::{
        ai_goto_cost_binding::Backend, ai_goto_gap, ai_graph_connection as core,
        ai_graph_handles as handles, ai_graph_link_passes::View, ai_graph_points::Location,
        ai_graph_queries as queries,
    };
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const G: usize = 0x60000000;
    const PARENT: usize = G + 0x100;
    const RES: u32 = 0x62000000;
    const DATA: u32 = 0x64000000;
    const PTS: u32 = DATA + 0x1000;
    const HEADER: u32 = DATA + 0x80;
    const MAN: usize = 0x65000000;
    const COUNT: u32 = 0x62e64;
    const IDS: u32 = 0x62e68;
    fn number(v: &Value) -> u32 {
        v.as_u64().unwrap_or_else(|| v.as_i64().unwrap() as u64) as u32
    }
    fn prepare(c: &Value) -> Result<(handles::State, Memory, queries::Metadata)> {
        let words = core::tests::prepared_words(c)?;
        let dynamic = c["kind"] == 1;
        let stride = if dynamic {
            number(&c["point_stride"])
        } else {
            32
        };
        let optional = |p: u32| (p != 0).then_some(p);
        let nodes = [G, PARENT]
            .into_iter()
            .map(|id| {
                let w = |off: u32| words[&(id as u32 + off)];
                (
                    id,
                    handles::Node {
                        connection: optional(w(0)),
                        traversal: optional(w(12)).map(|v| v as usize),
                        point: optional(w(4)).map(|p| Point {
                            resource: RES,
                            location: Location {
                                table: Table::Internal,
                                offset: p - PTS,
                            },
                        }),
                        graph: optional(w(8)),
                        free: 0,
                        word14: RES,
                        word18: 0,
                        manager_1c: Some(MAN),
                        word20: 0,
                        byte24: 0,
                        scalar28: 0,
                        scalar2c: 0,
                        byte30: 0,
                        word34: 0,
                        byte38: 0,
                        byte39: 0,
                        word3c: w(0x3c),
                        next: optional(w(0x40)).map(|v| v as usize),
                        serial: 0,
                    },
                )
            })
            .collect();
        let graph = handles::State {
            phase: 0,
            serial: 0,
            pool: vec![G, PARENT],
            nodes,
            resources: BTreeMap::from([(
                RES,
                handles::Resource {
                    size18: Some(0x4000),
                    byte8: 0,
                    priority: 0,
                    state: if c["ready"] == true { 5 } else { 4 },
                    word1c: 0,
                },
            )]),
            current: None,
            tracked_resource: None,
            tracked158: 0,
            tracked160: 0,
            deferred_enabled: 0,
        };
        let mut m = Memory::default();
        let mut base = None;
        let mut bytes: Vec<u8> = vec![];
        for (p, v) in words {
            if (G as u32..G as u32 + 0x200).contains(&p) || (RES..RES + 0x40).contains(&p) {
                continue;
            }
            if base.is_some_and(|b| b + bytes.len() as u32 != p) {
                m.regions
                    .insert(base.take().unwrap(), std::mem::take(&mut bytes));
            }
            if base.is_none() {
                base = Some(p)
            }
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        if let Some(base) = base {
            m.regions.insert(base, bytes);
        }
        m.resources.insert(
            RES,
            View {auxiliary:None,
                payload: Some(DATA),
                edges: Some(DATA + 0x2000),
                edge_stride: Some(if dynamic {
                    number(&c["edge_stride"])
                } else {
                    16
                }),
                header: HEADER,
                points: PTS,
                external: 0,
                external_count: 0,
                stride,
            },
        );
        let metadata = queries::Metadata {
            resources: BTreeMap::from([(
                RES,
                queries::ResourceMetadata {
                    class: if dynamic {
                        Class::Concrete
                    } else {
                        Class::Abstract
                    },
                    header: Some(HEADER as usize),
                },
            )]),
            headers: BTreeMap::new(),
        };
        ensure!(
            m.word(G as u32).is_err() && m.word(G as u32 + 4).is_err(),
            "Fixture retained a raw graph handle mirror"
        );
        ensure!(
            m.word(RES + 0x10).is_err() && m.word(RES + 0x18).is_err(),
            "Fixture retained mirrored resource state"
        );
        Ok((graph, m, metadata))
    }
    fn check(g: &mut GraphContext<'_, Memory>, case: &Value, index: usize) -> Result<()> {
        let mut h = Binding { graph: g };
        let image: Vec<u32> = (0..4)
            .map(|i| h.handle(G as u32, i * 4))
            .collect::<Result<_>>()?;
        assert_eq!(json!(image), case["graph"], "bound graph {index}");
        assert_eq!(json!(h.read(COUNT)?), case["count"], "bound count {index}");
        let scratch: Vec<u32> = (0..32)
            .map(|i| h.read(IDS + i * 4))
            .collect::<Result<_>>()?;
        assert_eq!(json!(scratch), case["scratch"], "bound scratch {index}");
        Ok(())
    }
    #[test]
    fn original_connections_use_canonical_handles_resources_and_shared_tables() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_connection.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            let (mut graph, mut memory, mut metadata) = prepare(c)?;
            let mut g = GraphContext {
                graph: &mut graph,
                tables: &mut memory,
                metadata: &mut metadata,
            };
            let mut h = Binding { graph: &mut g };
            let result = match c["op"].as_str().unwrap() {
                "index" => {
                    let stride = if c["kind"] == 1 {
                        number(&c["point_stride"])
                    } else {
                        32
                    };
                    let target = if c["by_index"] != 0 {
                        number(&c["target"])
                    } else {
                        PTS + number(&c["target"]) * stride
                    };
                    core::connection_index(
                        &mut h,
                        core::Context {
                            count: COUNT,
                            ids: IDS,
                        },
                        G as u32,
                        number(&c["mode"]),
                        target,
                        number(&c["by_index"]),
                    )?
                }
                "owner" => core::owner(
                    &mut h,
                    G as u32,
                    DATA.wrapping_add(number(&c["owner_offset"])),
                )?,
                "traversal" => core::traversal(&mut h, G as u32)?.unwrap_or(0),
                _ => unreachable!(),
            };
            assert_eq!(json!(result), case["result"], "bound result {index}");
            check(&mut g, case, index)?;
        }
        Ok(())
    }
    struct Construction;
    impl ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut ai_goto_gap::State) -> Result<Option<f32>> {
            Ok(Some(1.))
        }
        fn owner_entity(&mut self, _: &mut ai_goto_gap::State) -> Result<usize> {
            Ok(1)
        }
        fn profiling_enabled(&mut self, _: &mut ai_goto_gap::State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut ai_goto_gap::State, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected profile")
        }
    }
    struct NoDelegation;
    impl Backend for NoDelegation {
        fn connection_index(
            &mut self,
            _: &mut ai_goto_gap::State,
            _: &mut GraphContext<'_, Memory>,
            _: usize,
            _: Option<Point>,
        ) -> Result<u32> {
            anyhow::bail!("Delegated actual graph index")
        }
        fn traversal(
            &mut self,
            _: &mut ai_goto_gap::State,
            _: &mut GraphContext<'_, Memory>,
            _: usize,
        ) -> Result<Option<usize>> {
            anyhow::bail!("Delegated actual graph traversal")
        }
        fn service_cost(
            &mut self,
            _: &mut ai_goto_gap::State,
            _: &mut BTreeMap<usize, super::super::ai_pathfinder_constructor::State>,
            _: &mut GraphContext<'_, Memory>,
            _: usize,
            _: super::super::ai_goto_cost::Args,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected policy cost")
        }
        fn external_acceptance_cost(
            &mut self,
            _: &mut ai_goto_gap::State,
            _: &mut BTreeMap<usize, super::super::ai_pathfinder_constructor::State>,
            _: &mut GraphContext<'_, Memory>,
            _: super::super::ai_goto_cost::Args,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected policy cost")
        }
        fn refresh_gap(
            &mut self,
            _: &mut ai_goto_gap::State,
            _: &mut BTreeMap<usize, super::super::ai_pathfinder_constructor::State>,
            _: &mut GraphContext<'_, Memory>,
        ) -> Result<()> {
            anyhow::bail!("Unexpected refresh")
        }
    }
    #[test]
    fn goto_cost_native_graph_dispatch_never_uses_supplied_index_or_traversal_stubs() -> Result<()>
    {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_connection.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            if c["op"] != "traversal"
                && !(c["op"] == "index" && c["mode"] == 0 && c["by_index"] == 0)
            {
                continue;
            }
            let (mut graph, mut memory, mut metadata) = prepare(c)?;
            let mut g = GraphContext {
                graph: &mut graph,
                tables: &mut memory,
                metadata: &mut metadata,
            };
            let mut s = ai_goto_gap::State::construct(1, 2, b"native graph", &mut Construction)?;
            let mut backend = super::super::ai_goto_cost_native_graph::NativeGraph {
                backend: &mut NoDelegation,
                context: core::Context {
                    count: COUNT,
                    ids: IDS,
                },
            };
            let result = if c["op"] == "traversal" {
                backend
                    .traversal(&mut s, &mut g, G)?
                    .map_or(0, |v| v as u32)
            } else {
                let stride = if c["kind"] == 1 {
                    number(&c["point_stride"])
                } else {
                    32
                };
                backend.connection_index(
                    &mut s,
                    &mut g,
                    G,
                    Some(Point {
                        resource: RES,
                        location: Location {
                            table: Table::Internal,
                            offset: number(&c["target"]) * stride,
                        },
                    }),
                )?
            };
            assert_eq!(
                json!(result),
                case["result"],
                "cost dispatch result {index}"
            );
            check(&mut g, case, index)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod owned_tests {
    use super::super::{
        ai_graph_admission::Admission,
        ai_graph_handles as handles,
        ai_graph_linking::Pass,
        ai_graph_normalization::{convert, Layout},
        ai_graph_queries::{Metadata, ResourceMetadata},
    };
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    const ROOT: usize = 100;
    const COUNT: u32 = 0x62e64;
    const IDS: u32 = 0x62e68;
    fn hash(b: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, b)
            .as_ref()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect()
    }
    fn capture(g: &mut GraphContext<'_, Memory>, result: u32) -> Result<Value> {
        let n = &g.graph.nodes[&ROOT];
        let connection = n
            .connection
            .map(|p| {
                let resource = n.traversal.context("Selected edge has no owned resource")? as u32;
                let edges = g
                    .tables
                    .view(resource)?
                    .edges
                    .context("Selected resource has no edge binding")?;
                Ok::<_, anyhow::Error>(json!({"resource":resource,"offset":p.wrapping_sub(edges)}))
            })
            .transpose()?;
        Ok(
            json!({"result":result,"graph":n.graph,"traversal":n.traversal,"connection":connection,"count":g.tables.word(COUNT)?}),
        )
    }
    fn reset(g: &mut GraphContext<'_, Memory>, point: Point) {
        let n = g.graph.nodes.get_mut(&ROOT).unwrap();
        n.connection = None;
        n.graph = None;
        n.traversal = None;
        n.point = Some(point);
    }
    #[test]
    #[ignore = "requires immutable imported EM1 data in BLOTWORX_TEST_DATA"]
    fn original_owned_gv_adjacency_crosses_resources_using_live_validated_buffers() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let source: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_rings_owned.json"
        ))?;
        let expected: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_connection_owned.json"
        ))?;
        let stride = source["stride"].as_u64().unwrap() as u32;
        let template: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_metadata.json"
        ))?;
        let base = template["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| {
                v["input"]["state"]["nodes"].get("100").is_some()
                    && v["input"]["state"]["nodes"].get("101").is_some()
            })
            .unwrap();
        let mut graph: handles::State = serde_json::from_value(base["input"]["state"].clone())?;
        graph.resources.clear();
        let mut m = Memory {
            stride,
            ..Memory::default()
        };
        let mut metadata = Metadata::default();
        let records = source["records"].as_array().unwrap();
        let total: u32 = records
            .iter()
            .map(|r| r["point_count"].as_u64().unwrap() as u32)
            .sum();
        m.regions.insert(COUNT, vec![0; (4 + total * 4) as usize]);
        for (i, r) in records.iter().enumerate() {
            let original = std::fs::read(root.join(r["path"].as_str().unwrap()))?;
            ensure!(
                hash(&original) == r["source_sha256"].as_str().unwrap(),
                "Owned source changed"
            );
            let layout = Layout::Dynamic {
                node_stride: stride,
                edge_stride: 12,
            };
            let payload = convert(&original[16..], layout)?;
            let id = r["id"].as_u64().unwrap() as u32;
            let header = r["header"].as_u64().unwrap() as u32;
            let bytes = m.bind_payload(id, header, payload, layout)?;
            graph.resources.insert(
                id,
                handles::Resource {
                    size18: Some(bytes),
                    byte8: 0,
                    priority: 0,
                    state: 5,
                    word1c: id,
                },
            );
            metadata.resources.insert(
                id,
                ResourceMetadata {
                    class: Class::Concrete,
                    header: Some(header as usize),
                },
            );
            let node = graph
                .nodes
                .get_mut(&(ROOT + i))
                .context("Fixture graph handle missing")?;
            node.word14 = id;
            node.word3c = 0;
            node.connection = None;
            node.traversal = None;
            node.point = None;
            node.graph = None;
            node.next = if i + 1 < records.len() {
                Some(ROOT + i + 1)
            } else {
                None
            };
        }
        let order: Vec<u32> = serde_json::from_value(source["runs"][0]["order"].clone())?;
        let attributes = serde_json::from_value(source["attributes"].clone())?;
        let mut admission = Admission {
            index154: 0,
            index15c: 0,
            word82e4: 0,
            changed14: 0,
            listeners: vec![],
        };
        m.splice_external(order[0], true)?;
        m.convert_vertex_triplets(order[0], &attributes)?;
        m.run(&mut admission, order[1], order[0], Pass::DynamicPeers)?;
        m.splice_external(order[1], true)?;
        m.convert_vertex_triplets(order[1], &attributes)?;
        for r in records {
            let id = r["id"].as_u64().unwrap() as u32;
            let count = r["point_count"].as_u64().unwrap() as usize;
            let h = m.view(id)?.header;
            assert_eq!(
                hash(&m.regions[&h][8..8 + count * stride as usize]),
                r["live_points_sha256"].as_str().unwrap()
            );
            assert!(
                m.word(id + 0x14).is_err(),
                "Fixture mapped a resource ID as an address"
            );
        }
        let mut g = GraphContext {
            graph: &mut graph,
            tables: &mut m,
            metadata: &mut metadata,
        };
        let context = native::Context {
            count: COUNT,
            ids: IDS,
        };
        let mut crossings = 0;
        for (index, q) in expected["queries"].as_array().unwrap().iter().enumerate() {
            let id = q["resource"].as_u64().unwrap() as u32;
            let r = records.iter().find(|r| r["id"] == id).unwrap();
            let node = q["node"].as_u64().unwrap() as u32;
            let internal = r["point_count"].as_u64().unwrap() as u32
                - r["external_count"].as_u64().unwrap() as u32;
            let point = Point {
                resource: id,
                location: super::super::ai_graph_points::Location {
                    table: if node >= internal {
                        Table::External
                    } else {
                        Table::Internal
                    },
                    offset: if node >= internal {
                        (node - internal) * stride
                    } else {
                        node * stride
                    },
                },
            };
            reset(&mut g, point);
            let result = native::connection_index(
                &mut Binding { graph: &mut g },
                context,
                ROOT as u32,
                1,
                0,
                0,
            )?;
            assert_eq!(
                capture(&mut g, result)?,
                q["enumerated"],
                "owned enumeration {index}"
            );
            let ids: Vec<u32> = (0..g.tables.word(COUNT)?)
                .map(|i| g.tables.word(IDS + i * 4))
                .collect::<Result<_>>()?;
            assert_eq!(json!(ids), q["ids"], "owned adjacency order {index}");
            for s in q["selections"].as_array().unwrap() {
                reset(&mut g, point);
                let result = native::connection_index(
                    &mut Binding { graph: &mut g },
                    context,
                    ROOT as u32,
                    0,
                    s["index"].as_u64().unwrap() as u32,
                    1,
                )?;
                let actual = capture(&mut g, result)?;
                for (k, v) in actual.as_object().unwrap() {
                    assert_eq!(v, &s[k], "owned selection {index} {k}");
                }
                if g.graph.nodes[&ROOT]
                    .traversal
                    .is_some_and(|r| r as u32 != id)
                {
                    crossings += 1;
                }
            }
        }
        assert_eq!(json!(crossings), expected["cross_resource_selections"]);
        for r in records {
            let header = r["header"].as_u64().unwrap() as u32;
            let count = r["point_count"].as_u64().unwrap() as usize;
            assert_eq!(
                hash(&g.tables.regions[&header][8..8 + count * stride as usize]),
                r["live_points_sha256"].as_str().unwrap()
            );
            assert_eq!(
                hash(&std::fs::read(root.join(r["path"].as_str().unwrap()))?),
                r["source_sha256"].as_str().unwrap()
            );
        }
        Ok(())
    }
}
