//! Goto cost dispatch over existing pathfinder, graph, entity and modifier
//! owners. The subject stays borrowed in its registry during nested queries.
use super::{
    ai_astar_graph_services::GraphContext,
    ai_goto_cost::{self as cost, Args, Cursor, GapHost, GraphHost, Host},
    ai_goto_gap::State,
    ai_graph_link_passes::Memory,
    ai_graph_queries::{Point, QueryStore},
    ai_navigation_entity,
    ai_pathfinder_constructor::State as Parent,
    ai_pathfinder_modifier_factory::Owned,
};
use anyhow::{ensure, Context as _, Result};
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
pub struct Context {
    pub marker_metadata: u32,
    pub zero: u32,
}
pub trait Backend {
    fn connection_index(
        &mut self,
        s: &mut State,
        g: &mut GraphContext<'_, Memory>,
        graph: usize,
        target: Option<Point>,
    ) -> Result<u32>;
    fn traversal(
        &mut self,
        s: &mut State,
        g: &mut GraphContext<'_, Memory>,
        graph: usize,
    ) -> Result<Option<usize>>;
    fn service_cost(
        &mut self,
        s: &mut State,
        parents: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
        service: usize,
        args: Args,
    ) -> Result<u32>;
    fn external_acceptance_cost(
        &mut self,
        s: &mut State,
        parents: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
        args: Args,
    ) -> Result<u32>;
    fn refresh_gap(
        &mut self,
        s: &mut State,
        parents: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
    ) -> Result<()>;
}
pub struct Binding<'a, B> {
    pub graph: GraphContext<'a, Memory>,
    pub parents: &'a mut BTreeMap<usize, Parent>,
    pub entities: &'a BTreeMap<usize, ai_navigation_entity::Owned>,
    pub modifiers: &'a [&'a Owned],
    pub context: Context,
    pub backend: &'a mut B,
}
impl<B> Binding<'_, B> {
    fn parent(&self, s: &State) -> Result<&Parent> {
        let p = self
            .parents
            .get(&s.pathfinder)
            .context("Goto parent is absent")?;
        ensure!(p.identity == s.pathfinder, "Goto parent identity differs");
        Ok(p)
    }
    fn position(&self, p: u32) -> Result<[f32; 3]> {
        if let Some(parent) = p
            .checked_sub(0x200)
            .and_then(|id| self.parents.get(&(id as usize)))
        {
            return Ok(parent.points_200_274_2c4_314_378[0].position);
        }
        super::ai_refine_goal::position(self.graph.tables, p)
    }
}
impl<B: Backend> GraphHost for Binding<'_, B> {
    type Point = Option<Point>;
    fn point(&mut self, graph: usize) -> Result<Self::Point> {
        Ok(self
            .graph
            .graph
            .nodes
            .get(&graph)
            .context("Goto graph cursor absent")?
            .point)
    }
    fn capture(&mut self, graph: usize) -> Result<Cursor<Self::Point>> {
        let node = self
            .graph
            .graph
            .nodes
            .get(&graph)
            .context("Goto graph cursor absent")?;
        Ok(Cursor {
            connection: node.connection.unwrap_or(0),
            point: node.point,
        })
    }
    fn set_point(&mut self, graph: usize, point: Self::Point) -> Result<()> {
        let node = self
            .graph
            .graph
            .nodes
            .get_mut(&graph)
            .context("Goto graph cursor absent")?;
        node.point = point;
        node.graph = None;
        Ok(())
    }
    fn restore(&mut self, graph: usize, c: Cursor<Self::Point>) -> Result<()> {
        let node = self
            .graph
            .graph
            .nodes
            .get_mut(&graph)
            .context("Goto graph cursor absent")?;
        node.connection = (c.connection != 0).then_some(c.connection);
        node.point = c.point;
        node.graph = None;
        node.traversal = None;
        Ok(())
    }
    fn connection_index(
        &mut self,
        s: &mut State,
        graph: usize,
        target: Self::Point,
    ) -> Result<u32> {
        self.backend
            .connection_index(s, &mut self.graph, graph, target)
    }
}
impl<B: Backend> Host for Binding<'_, B> {
    fn read(&mut self, p: u32) -> Result<u32> {
        self.graph.tables.word(p)
    }
    fn write(&mut self, p: u32, value: u32) -> Result<()> {
        self.graph.tables.write(p, value)
    }
    fn graph(&mut self, s: &mut State) -> Result<Option<usize>> {
        self.parent(s)?.graph_provider(self.graph.tables)
    }

    fn marker(&mut self, p: u32) -> Result<Option<u32>> {
        let index = self
            .graph
            .tables
            .word(self.context.marker_metadata.wrapping_add(8))?;
        let pointer = self
            .graph
            .tables
            .word(p.wrapping_add(16).wrapping_add(index.wrapping_mul(4)))?;
        Ok((pointer != 0).then_some(pointer))
    }
    fn ready(&mut self, graph: usize) -> Result<u32> {
        Ok(u32::from(self.graph.graph.ready(Some(graph))?))
    }
    fn bind(&mut self, _: &mut State, graph: usize, id: u32) -> Result<u32> {
        Ok(u32::from(self.graph.tables.find_point(
            self.graph.graph,
            Some(graph),
            id,
            self.graph.metadata,
        )?))
    }
    fn traversal(&mut self, s: &mut State, graph: usize) -> Result<Option<usize>> {
        self.backend.traversal(s, &mut self.graph, graph)
    }
    fn fallback(&mut self, s: &mut State) -> Result<Option<usize>> {
        let id = self
            .parent(s)?
            .word_364
            .context("Goto fallback policy is unwritten")?;
        Ok((id != 0).then_some(id as usize))
    }
    fn arrival_detector(&mut self, s: &mut State) -> Result<usize> {
        self.parent(s)?.tables_fc_1fc[super::ai_pathfinder_modifiers::Kind(6).table()]
            .selected
            .context("Goto arrival selector is unwritten")?
            .context("Goto arrival detector is null")
    }
    fn current_position(&mut self, s: &mut State) -> Result<u32> {
        let parent = s.pathfinder;
        let update = self
            .parent(s)?
            .flag_250
            .context("Goto source-point update flag is unwritten")?;
        if update != 0 {
            let owner = self
                .parents
                .get_mut(&parent)
                .context("Goto source parent is absent")?;
            ensure!(
                owner.identity == parent,
                "Goto source parent identity differs"
            );
            super::ai_source_point::refresh(owner, self.graph.tables)?;
        }
        Ok(u32::try_from(parent)?.wrapping_add(0x200))
    }
    fn reached(&mut self, _: &mut State, service: usize, current: u32, start: u32) -> Result<u32> {
        let source = self.position(current)?;
        let target = self.position(start)?;
        let object = self
            .modifiers
            .iter()
            .copied()
            .find(|o| o.identity() == service)
            .context("Goto arrival detector is not owned")?;
        let Owned::NodeReached(detector) = object else {
            anyhow::bail!("Goto arrival detector has an unsupported concrete class")
        };
        detector.query_positions(source, target)
    }
    fn service_cost(&mut self, s: &mut State, service: usize, args: Args) -> Result<u32> {
        self.backend
            .service_cost(s, self.parents, &mut self.graph, service, args)
    }
    fn acceptance_cost(&mut self, s: &mut State, args: Args) -> Result<u32> {
        let inline = args
            .acceptance
            .checked_sub(0x3e0)
            .and_then(|p| self.parents.get(&(p as usize)))
            .is_some_and(|p| p.word_3e0 == Some(0x517490));
        if inline {
            cost::inline_distance(self, args)
        } else {
            self.backend
                .external_acceptance_cost(s, self.parents, &mut self.graph, args)
        }
    }
}
impl<B: Backend> GapHost for Binding<'_, B> {
    fn bypass(&mut self, s: &mut State) -> Result<bool> {
        let p = self.parent(s)?;
        Ok(p.reset_fields.flags_369_36a_3c8_400_401_368_74[6]
            .context("Goto parent reset flag74 is unwritten")?
            != 0
            && p.base.flag_20 != 0)
    }
    fn refresh(&mut self, s: &mut State) -> Result<()> {
        self.backend.refresh_gap(s, self.parents, &mut self.graph)
    }
    fn entity_xz(&mut self, _: &mut State, entity: usize) -> Result<[f32; 2]> {
        let e = self
            .entities
            .get(&entity)
            .context("Goto owner entity absent")?;
        ensure!(e.entity.identity == entity, "Goto entity identity differs");
        Ok([e.entity.values_34_58[0], e.entity.values_34_58[2]])
    }
    fn zero_direction(&mut self, _: &mut State) -> Result<[f32; 3]> {
        let p = self.context.zero;
        Ok([
            f32::from_bits(self.read(p)?),
            f32::from_bits(self.read(p.wrapping_add(4))?),
            f32::from_bits(self.read(p.wrapping_add(8))?),
        ])
    }
}
/// Borrow disjoint objects without removing the subject from its owner map or
/// making another identity/owner registry. Views last only for this query.
pub fn with_gap<R>(
    objects: &mut BTreeMap<usize, Owned>,
    identity: usize,
    query: impl FnOnce(&mut State, &[&Owned]) -> Result<R>,
) -> Result<R> {
    let mut subject = None;
    let mut others = Vec::with_capacity(objects.len().saturating_sub(1));
    for (&id, object) in objects.iter_mut() {
        if id == identity {
            let Owned::GapAvoidance(state) = object else {
                anyhow::bail!("Goto subject has a different class")
            };
            subject = Some(state.as_mut());
        } else {
            others.push(&*object);
        }
    }
    query(subject.context("Goto subject is not owned")?, &others)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ai_goto_gap::{self, Cache24, Pool},
        ai_graph_handles as handles,
        ai_graph_link_passes::View,
        ai_graph_points::{Location, Table},
        ai_graph_queries as graphs,
        ai_node_reached::{self, Class as ArrivalClass},
    };
    use super::*;
    use serde_json::{json, Value};
    const B: usize = 0x60000000;
    const P: usize = 0x61000000;
    const Q: usize = P + 0x1000;
    const E: usize = 0x62000000;
    const START: u32 = 0x63000000;
    const GOAL: u32 = START + 0x100;
    const COST: u32 = START + 0x300;
    const G: usize = 0x64000000;
    const SP: u32 = 0x65000000;
    const RESOURCE: u32 = 0x6b000000;
    const DETECTOR: usize = 0x68000000;
    fn address(memory: &Memory, point: Option<Point>) -> Result<u32> {
        let Some(point) = point else { return Ok(0) };
        let view = memory.view(point.resource)?;
        Ok(match point.location.table {
            Table::Internal => view.points,
            Table::External => view.external,
        }
        .wrapping_add(point.location.offset))
    }
    fn image(g: &GraphContext<'_, Memory>) -> Result<[u32; 4]> {
        let n = &g.graph.nodes[&G];
        Ok([
            n.connection.unwrap_or(0),
            address(g.tables, n.point)?,
            n.graph.unwrap_or(0),
            n.traversal.unwrap_or(0) as u32,
        ])
    }
    struct Services<'a> {
        input: &'a Value,
        trace: Vec<Value>,
    }
    impl Backend for Services<'_> {
        fn connection_index(
            &mut self,
            _: &mut State,
            g: &mut GraphContext<'_, Memory>,
            graph: usize,
            target: Option<Point>,
        ) -> Result<u32> {
            self.trace.push(json!([
                "index",
                graph,
                0,
                address(g.tables, g.graph.nodes[&graph].point)?,
                address(g.tables, target)?,
                0
            ]));
            let n = g.graph.nodes.get_mut(&graph).unwrap();
            n.connection = Some(0x77778888);
            n.graph = Some(0x9999aabb);
            n.traversal = Some(0xbbbbcccc);
            Ok(self.input["index"].as_i64().unwrap() as u32)
        }
        fn traversal(
            &mut self,
            _: &mut State,
            _: &mut GraphContext<'_, Memory>,
            graph: usize,
        ) -> Result<Option<usize>> {
            self.trace.push(json!(["traversal", graph]));
            Ok(self.input["traversal"]
                .as_bool()
                .unwrap()
                .then_some(0x67000000))
        }
        fn service_cost(
            &mut self,
            _: &mut State,
            _: &mut BTreeMap<usize, Parent>,
            g: &mut GraphContext<'_, Memory>,
            service: usize,
            a: Args,
        ) -> Result<u32> {
            self.trace.push(json!([
                "service",
                [
                    service as u32,
                    a.entity as u32,
                    a.start,
                    a.goal,
                    a.acceptance,
                    a.cost
                ],
                image(g)?
            ]));
            g.tables
                .write(a.cost, self.input["cost"].as_u64().unwrap() as u32)?;
            Ok(self.input["service_result"].as_u64().unwrap() as u32)
        }
        fn external_acceptance_cost(
            &mut self,
            _: &mut State,
            _: &mut BTreeMap<usize, Parent>,
            _: &mut GraphContext<'_, Memory>,
            _: Args,
        ) -> Result<u32> {
            anyhow::bail!("Canonical inline acceptance was not dispatched")
        }
        fn refresh_gap(
            &mut self,
            s: &mut State,
            _: &mut BTreeMap<usize, Parent>,
            _: &mut GraphContext<'_, Memory>,
        ) -> Result<()> {
            self.trace.push(json!(["refresh"]));
            if self.input["refresh_remove"].as_bool().unwrap() {
                s.pool_118 = None;
            }
            if self.input["refresh_owner"].as_bool().unwrap() {
                s.pathfinder = Q;
            }
            Ok(())
        }
    }
    struct Construction;
    impl ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(None)
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            Ok(E)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            unreachable!()
        }
    }
    struct Shape;
    impl ai_navigation_entity::Host for Shape {
        fn radius(&mut self, _: &mut ai_navigation_entity::Owned) -> Result<f32> {
            Ok(1.)
        }
        fn height(&mut self, _: &mut ai_navigation_entity::Owned) -> Result<f32> {
            Ok(2.)
        }
        fn maximum_speed(&mut self, _: &mut ai_navigation_entity::Owned) -> Result<f32> {
            Ok(1.)
        }
        fn unit_scale(&mut self, _: &mut ai_navigation_entity::Owned) -> Result<Option<f32>> {
            Ok(None)
        }
    }
    #[test]
    fn goto_cost_uses_canonical_graph_parent_inline_policy_and_arrival_detector() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_cost_composed.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let mut parents = BTreeMap::new();
            for identity in [P, Q] {
                let mut parent = super::super::ai_pathfinder_modifiers::tests::state()?;
                parent.identity = identity;
                parent.word_70 = if input["graph"].as_bool().unwrap() {
                    G as u32
                } else {
                    0
                };
                parent.word_364 = Some(if input["fallback"].as_bool().unwrap() {
                    if identity == P {
                        0x67000100
                    } else {
                        0x67000200
                    }
                } else {
                    0
                });
                parent.flag_250 = Some(0);
                parent.points_200_274_2c4_314_378[0].position = std::array::from_fn(|i| {
                    f32::from_bits(input["position"][i].as_u64().unwrap_or(0) as u32)
                });
                parent.reset_fields.flags_369_36a_3c8_400_401_368_74[6] =
                    Some(input["bypass"][0].as_u64().unwrap() as u8);
                parent.base.flag_20 = input["bypass"][1].as_u64().unwrap() as u8;
                parent.tables_fc_1fc[super::super::ai_pathfinder_modifiers::Kind(6).table()]
                    .selected = Some(Some(DETECTOR));
                parent.references_3c_4c_50_54_58_60_6c[2] =
                    input["manager"].as_bool().unwrap().then_some(0x6d000000);
                parent.references_3c_4c_50_54_58_60_6c[0] =
                    Some(input["manager_index"].as_u64().unwrap() as usize);
                parents.insert(identity, parent);
            }
            let mut memory = Memory::default();
            for (p, n) in [
                (START, 0x500),
                (SP, 0x300),
                (0x6e000000, 0x84),
                (0x6e001000, 8),
                (0x6a000000, 0x20),
                (0x6729c, 12),
                (0x564d0, 12),
                (0x6d000000, 0x34),
                (0x6d001000, 0x50),
            ] {
                memory.regions.insert(p, vec![0; n]);
            }
            memory.write(SP, 12)?;
            memory.write(SP + 0x100, 34)?;
            memory.write(SP + 0x200, 56)?;
            memory.write(0x6e001000, 2)?;
            memory.write(0x6729c + 8, 7)?;
            memory.write(COST, 3f32.to_bits())?;
            for i in 0..3 {
                memory.write(
                    START + 4 + i * 4,
                    input["start"][i as usize].as_u64().unwrap() as u32,
                )?;
                memory.write(
                    GOAL + 4 + i * 4,
                    input["goal"][i as usize].as_u64().unwrap() as u32,
                )?;
            }
            for (i, p) in [START, GOAL].into_iter().enumerate() {
                if input["markers"][i].as_bool().unwrap() {
                    memory.write(p + 16 + 7 * 4, 0x6a000000 + i as u32 * 0x10)?;
                    memory.write(
                        0x6a000004 + i as u32 * 0x10,
                        if i == 0 {
                            12
                        } else if input["marker_change"].as_bool().unwrap() {
                            99
                        } else {
                            34
                        },
                    )?;
                }
            }
            memory.write(0x6d000030, 2)?;
            memory.write(0x6d000028, 0x6d001000)?;
            for i in 0..2 {
                memory.write(
                    0x6d001004 + i * 0x28,
                    if input["graph"].as_bool().unwrap() {
                        G as u32
                    } else {
                        0
                    },
                )?;
                memory.write_byte(
                    0x6d001024 + i * 0x28,
                    u8::from(input["manager_active"].as_bool().unwrap()),
                )?;
            }
            memory.resources.insert(
                RESOURCE,
                View {auxiliary:None,payload:None,edges:None,edge_stride:None,
                    header: 0x6e001000,
                    points: SP,
                    external: 0,
                    external_count: 0,
                    stride: 0x100,
                },
            );
            let old = Point {
                resource: RESOURCE,
                location: Location {
                    table: Table::Internal,
                    offset: 0x200,
                },
            };
            let node = handles::Node {
                connection: Some(0x11112222),
                traversal: Some(0x55556666),
                point: Some(old),
                graph: Some(0x33334444),
                free: 0,
                word14: RESOURCE,
                word18: 0,
                manager_1c: None,
                word20: 0,
                byte24: 0,
                scalar28: 0,
                scalar2c: 0,
                byte30: 0,
                word34: 0,
                byte38: 0,
                byte39: 0,
                word3c: 0,
                next: None,
                serial: 0,
            };
            let mut graph = handles::State {
                phase: 0,
                serial: 0,
                pool: vec![],
                nodes: BTreeMap::from([(G, node)]),
                resources: BTreeMap::from([(
                    RESOURCE,
                    handles::Resource {
                        size18: None,
                        byte8: 0,
                        priority: 0,
                        state: if input["ready"].as_bool().unwrap() {
                            5
                        } else {
                            4
                        },
                        word1c: 0,
                    },
                )]),
                current: None,
                tracked_resource: None,
                tracked158: 0,
                tracked160: 0,
                deferred_enabled: 0,
            };
            let mut metadata = graphs::Metadata {
                resources: BTreeMap::from([(
                    RESOURCE,
                    graphs::ResourceMetadata {
                        class: graphs::Class::Concrete,
                        header: Some(0x6e001000),
                    },
                )]),
                headers: BTreeMap::new(),
            };
            let registry = ai_navigation_entity::Registry {
                count: 3,
                diameter: ai_navigation_entity::Class {
                    index: 0,
                    constructor: Some(ai_navigation_entity::Kind::Diameter),
                },
                height: ai_navigation_entity::Class {
                    index: 1,
                    constructor: Some(ai_navigation_entity::Kind::Height),
                },
                maximum_speed: ai_navigation_entity::Class {
                    index: 2,
                    constructor: Some(ai_navigation_entity::Kind::MaximumSpeed),
                },
            };
            let mut entity =
                ai_navigation_entity::Owned::new(E, None, [0; 2], &registry, &mut Shape)?;
            for i in 0..3 {
                entity.entity.values_34_58[i] =
                    f32::from_bits(input["entity"][i].as_u64().unwrap() as u32);
            }
            let entities = BTreeMap::from([(E, entity)]);
            let mut subject = State::construct(B, P, b"goto", &mut Construction)?;
            if input["pool"].as_bool().unwrap() {
                let cones = input["cones"].as_array().unwrap();
                let mut pool = Pool::new(cones.len() as u32, Cache24::node)?;
                pool.active_head = (!cones.is_empty()).then_some(0);
                pool.count = input["pool_count"].as_u64().unwrap() as u32;
                for (i, cone) in cones.iter().enumerate() {
                    for (field, j) in [(2, 0), (4, 1), (5, 2)] {
                        pool.nodes[i].payload.words_00_14[field] =
                            Some(cone[j].as_u64().unwrap() as u32);
                    }
                }
                subject.pool_118 = Some(pool);
            }
            let mut detector = ai_node_reached::State::construct(
                if input["arrival_class"] == 0 {
                    ArrivalClass::Distance2D5
                } else {
                    ArrivalClass::Distance3D
                },
                DETECTOR,
                P,
                b"node",
            );
            detector.distance = Some(f32::from_bits(
                input["arrival_distance"].as_u64().unwrap() as u32
            ));
            detector.height = Some(f32::from_bits(
                input["arrival_height"].as_u64().unwrap() as u32
            ));
            let mut objects = BTreeMap::from([
                (B, Owned::GapAvoidance(Box::new(subject))),
                (DETECTOR, Owned::NodeReached(Box::new(detector))),
            ]);
            let mut backend = Services {
                input,
                trace: vec![],
            };
            let result = with_gap(&mut objects, B, |s, modifiers| {
                let mut binding = Binding {
                    graph: GraphContext {
                        graph: &mut graph,
                        tables: &mut memory,
                        metadata: &mut metadata,
                    },
                    parents: &mut parents,
                    entities: &entities,
                    modifiers,
                    context: Context {
                        marker_metadata: 0x6729c,
                        zero: 0x564d0,
                    },
                    backend: &mut backend,
                };
                let a = Args {
                    entity: E,
                    start: START,
                    goal: GOAL,
                    acceptance: P as u32 + 0x3e0,
                    cost: COST,
                };
                if input["op"] == "base" {
                    cost::base(&mut binding, s, a)
                } else {
                    cost::gap(&mut binding, s, a)
                }
            })?;
            assert_eq!(json!(result), case["result"], "result {index}");
            assert_eq!(json!(memory.word(COST)?), case["cost"], "cost {index}");
            let context = GraphContext {
                graph: &mut graph,
                tables: &mut memory,
                metadata: &mut metadata,
            };
            assert_eq!(json!(image(&context)?), case["graph"], "graph {index}");
            let Owned::GapAvoidance(subject) = &objects[&B] else {
                unreachable!()
            };
            assert_eq!(json!(subject.pathfinder), case["parent"], "parent {index}");
            assert_eq!(
                json!(subject.pool_118.is_some()),
                case["pool"],
                "pool {index}"
            );
            let trace: Vec<_> = case["trace"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    matches!(
                        row[0].as_str(),
                        Some("index" | "traversal" | "service" | "refresh")
                    )
                })
                .cloned()
                .collect();
            assert_eq!(backend.trace, trace, "trace {index}");
            assert_eq!(
                objects.len(),
                2,
                "Nested cost call removed its owning object"
            );
        }
        Ok(())
    }
}
