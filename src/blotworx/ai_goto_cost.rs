//! Retail goto base4a17c8, pair wrapper47fbb0, inline acceptance41b6e4,
//! and CGoto_GapDynamicAvoidance_v2 cost4a4ff8. Concrete refresh4a5564,
//! graph adjacency/traversal and other acceptance classes remain required.
use super::ai_goto_gap::State;
use anyhow::{ensure, Context as _, Result};
use std::collections::BTreeSet;
#[derive(Clone, Copy)]
pub struct Args {
    pub entity: usize,
    pub start: u32,
    pub goal: u32,
    pub acceptance: u32,
    pub cost: u32,
}
#[derive(Clone, Copy)]
pub struct Cursor<P: Copy> {
    pub connection: u32,
    pub point: P,
}
pub trait GraphHost {
    type Point: Copy;
    fn point(&mut self, graph: usize) -> Result<Self::Point>;
    fn capture(&mut self, graph: usize) -> Result<Cursor<Self::Point>>;
    /// Set word4 and clear word8 only; preserve connection0 and traversal cacheC.
    fn set_point(&mut self, graph: usize, point: Self::Point) -> Result<()>;
    /// Restore words0/4, clearing both caches8/C before delegated policy calls.
    fn restore(&mut self, graph: usize, cursor: Cursor<Self::Point>) -> Result<()>;
    /// Actual47fea8 with mode0, target point and null optional traversal filter.
    fn connection_index(
        &mut self,
        state: &mut State,
        graph: usize,
        target: Self::Point,
    ) -> Result<u32>;
}
pub trait Host: GraphHost {
    fn read(&mut self, address: u32) -> Result<u32>;
    fn write(&mut self, address: u32, value: u32) -> Result<()>;
    fn graph(&mut self, state: &mut State) -> Result<Option<usize>>;
    fn marker(&mut self, point: u32) -> Result<Option<u32>>;
    fn ready(&mut self, graph: usize) -> Result<u32>;
    fn bind(&mut self, state: &mut State, graph: usize, node: u32) -> Result<u32>;
    fn traversal(&mut self, state: &mut State, graph: usize) -> Result<Option<usize>>;
    fn fallback(&mut self, state: &mut State) -> Result<Option<usize>>;
    fn arrival_detector(&mut self, state: &mut State) -> Result<usize>;
    ///496268 returns/refills the real source point; virtual3c is an acceptance
    ///policy and must never be substituted for this position query.
    fn current_position(&mut self, state: &mut State) -> Result<u32>;
    fn reached(
        &mut self,
        state: &mut State,
        service: usize,
        current: u32,
        start: u32,
    ) -> Result<u32>;
    fn service_cost(&mut self, state: &mut State, service: usize, args: Args) -> Result<u32>;
    fn acceptance_cost(&mut self, state: &mut State, args: Args) -> Result<u32>;
}
pub fn pair<H: GraphHost>(
    h: &mut H,
    s: &mut State,
    graph: usize,
    start: H::Point,
    target: H::Point,
) -> Result<bool> {
    let saved = h.point(graph)?;
    h.set_point(graph, start)?;
    let index = h.connection_index(s, graph, target)?;
    h.set_point(graph, saved)?;
    Ok(index != u32::MAX)
}
pub fn base(h: &mut impl Host, s: &mut State, a: Args) -> Result<u32> {
    let graph = h.graph(s)?;
    let start_marker = h.marker(a.start)?;
    let goal_marker = h.marker(a.goal)?;
    if let Some(graph) = graph {
        if h.ready(graph)? & 255 != 0 {
            if let (Some(start), Some(goal)) = (start_marker, goal_marker) {
                let saved = h.capture(graph)?;
                let id = h.read(start.wrapping_add(4))?;
                if h.bind(s, graph, id)? & 255 != 0 {
                    let start = h.point(graph)?;
                    let id = h.read(goal.wrapping_add(4))?;
                    if h.bind(s, graph, id)? & 255 != 0 {
                        let goal = h.point(graph)?;
                        if pair(h, s, graph, start, goal)? {
                            if let Some(service) = h.traversal(s, graph)? {
                                h.restore(graph, saved)?;
                                return h.service_cost(s, service, a);
                            }
                        }
                    }
                }
                h.restore(graph, saved)?;
            }
        }
    }
    if h.fallback(s)?.is_some() {
        let detector = h.arrival_detector(s)?;
        let position = h.current_position(s)?;
        if h.reached(s, detector, position, a.start)? & 255 != 0 {
            let service = h
                .fallback(s)?
                .context("Goto fallback disappeared before its virtual cost call")?;
            return h.service_cost(s, service, a);
        }
    }
    h.acceptance_cost(s, a)
}
///Actual41b6e4: default inline policy writes the native 3D distance and returns1.
///Dispatch this only for the verified class, not as a default for unknown policies.
pub fn inline_distance(h: &mut impl Host, a: Args) -> Result<u32> {
    let dz = f32::from_bits(h.read(a.goal.wrapping_add(12))?)
        - f32::from_bits(h.read(a.start.wrapping_add(12))?);
    let dx = f32::from_bits(h.read(a.goal.wrapping_add(4))?)
        - f32::from_bits(h.read(a.start.wrapping_add(4))?);
    let dy = f32::from_bits(h.read(a.goal.wrapping_add(8))?)
        - f32::from_bits(h.read(a.start.wrapping_add(8))?);
    let distance = super::cursor::native_sqrt((dz * dz + dx * dx) + dy * dy);
    h.write(a.cost, distance.to_bits())?;
    Ok(1)
}
pub trait GapHost: Host {
    ///True only when captured parent byte74 and base byte20 are both nonzero.
    fn bypass(&mut self, state: &mut State) -> Result<bool>;
    ///Actual virtual30=4a5564; it must update real actor/gap caches, not return
    ///success merely because this cost routine can be evaluated independently.
    fn refresh(&mut self, state: &mut State) -> Result<()>;
    fn entity_xz(&mut self, state: &mut State, entity: usize) -> Result<[f32; 2]>;
    fn zero_direction(&mut self, state: &mut State) -> Result<[f32; 3]>;
}
pub fn gap(h: &mut impl GapHost, s: &mut State, a: Args) -> Result<u32> {
    if h.bypass(s)? {
        return base(h, s, a);
    }
    h.refresh(s)?;
    if base(h, s, a)? & 255 == 0 {
        return Ok(0);
    }
    if s.pool_118.as_ref().is_none_or(|p| p.count == 0) {
        return Ok(1);
    }
    let detector = h.arrival_detector(s)?;
    let current = h.current_position(s)?;
    if h.reached(s, detector, current, a.start)? & 255 == 0 {
        return Ok(1);
    }
    let goal_z = f32::from_bits(h.read(a.goal.wrapping_add(12))?);
    let goal_x = f32::from_bits(h.read(a.goal.wrapping_add(4))?);
    let [entity_x, entity_z] = h.entity_xz(s, a.entity)?;
    let dz = entity_z - goal_z;
    let dx = entity_x - goal_x;
    if dz * dz + dx * dx == 0. {
        return Ok(1);
    }
    let [entity_x, entity_z] = h.entity_xz(s, a.entity)?;
    let x = goal_x - entity_x;
    let z = goal_z - entity_z;
    let length = super::cursor::native_sqrt(z * z + (x * x + 0f32 * 0f32));
    let direction = if length == 0. {
        h.zero_direction(s)?
    } else {
        [x / length, 0. / length, z / length]
    };
    let pool = s
        .pool_118
        .as_ref()
        .context("Goto gap pool disappeared during direction query")?;
    let mut next = pool.active_head;
    let mut visited = BTreeSet::new();
    while let Some(index) = next {
        ensure!(visited.insert(index), "Cyclic goto direction-cone cache");
        let node = pool
            .nodes
            .get(index)
            .context("Goto direction-cone cache link is out of bounds")?;
        let read = |i: usize| -> Result<f32> {
            Ok(f32::from_bits(
                node.payload.words_00_14[i].context("Goto direction-cone scalar is unwritten")?,
            ))
        };
        let dot = direction[2] * read(4)? + direction[0] * read(2)?;
        // Native ble rejects unordered comparisons too. Multiply only once.
        if dot > read(5)? {
            let cost = f32::from_bits(h.read(a.cost)?);
            h.write(a.cost, (cost * 1000.).to_bits())?;
            break;
        }
        next = node.next;
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::super::ai_goto_gap::{self, Cache24, Pool};
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const B: usize = 0x60000000;
    const P: usize = 0x61000000;
    const Q: usize = P + 0x1000;
    const E: usize = 0x62000000;
    const START: u32 = 0x63000000;
    const GOAL: u32 = START + 0x100;
    const ACCEPT: u32 = START + 0x200;
    const COST: u32 = START + 0x300;
    const POSITION: u32 = START + 0x400;
    const G: usize = 0x64000000;
    const SP: u32 = 0x65000000;
    const TP: u32 = SP + 0x100;
    const OLD: u32 = SP + 0x200;
    const SVC: usize = 0x67000000;
    const ALT: usize = SVC + 0x100;
    const ALT2: usize = SVC + 0x200;
    const LPF: usize = 0x68000000;
    const MK: u32 = 0x6a000000;
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
    struct Replay<'a> {
        input: &'a Value,
        memory: BTreeMap<u32, u32>,
        trace: Vec<Value>,
        bind: usize,
    }
    impl Replay<'_> {
        fn image(&self) -> [u32; 4] {
            std::array::from_fn(|i| self.memory[&(G as u32 + i as u32 * 4)])
        }
        fn service(&mut self, args: Args, service: Option<usize>) -> Result<u32> {
            if let Some(service) = service {
                self.trace.push(json!([
                    "service",
                    [
                        service as u32,
                        args.entity as u32,
                        args.start,
                        args.goal,
                        args.acceptance,
                        args.cost
                    ],
                    self.image()
                ]));
                self.write(args.cost, self.input["cost"].as_u64().unwrap() as u32)?;
                Ok(self.input["service_result"].as_u64().unwrap() as u32)
            } else {
                self.trace.push(json!([
                    "acceptance",
                    [
                        args.acceptance,
                        args.entity as u32,
                        args.start,
                        args.goal,
                        args.cost
                    ],
                    self.image()
                ]));
                if self.input["acceptance"] == "native" {
                    inline_distance(self, args)
                } else {
                    self.write(args.cost, self.input["cost"].as_u64().unwrap() as u32)?;
                    Ok(self.input["acceptance_result"].as_u64().unwrap() as u32)
                }
            }
        }
    }
    impl GraphHost for Replay<'_> {
        type Point = u32;
        fn point(&mut self, graph: usize) -> Result<u32> {
            self.read(graph as u32 + 4)
        }
        fn capture(&mut self, graph: usize) -> Result<Cursor<u32>> {
            Ok(Cursor {
                connection: self.read(graph as u32)?,
                point: self.point(graph)?,
            })
        }
        fn set_point(&mut self, graph: usize, point: u32) -> Result<()> {
            self.write(graph as u32 + 4, point)?;
            self.write(graph as u32 + 8, 0)
        }
        fn restore(&mut self, graph: usize, c: Cursor<u32>) -> Result<()> {
            self.write(graph as u32, c.connection)?;
            self.write(graph as u32 + 12, 0)?;
            self.write(graph as u32 + 4, c.point)?;
            self.write(graph as u32 + 8, 0)
        }
        fn connection_index(&mut self, _: &mut State, graph: usize, target: u32) -> Result<u32> {
            let point = self.point(graph)?;
            self.trace
                .push(json!(["index", graph, 0, point, target, 0]));
            self.write(graph as u32, 0x77778888)?;
            self.write(graph as u32 + 8, 0x9999aabb)?;
            self.write(graph as u32 + 12, 0xbbbbcccc)?;
            Ok(self.input["index"].as_i64().unwrap() as u32)
        }
    }
    impl Host for Replay<'_> {
        fn read(&mut self, p: u32) -> Result<u32> {
            self.memory
                .get(&p)
                .copied()
                .with_context(|| format!("Unmapped goto-cost data {p:08x}"))
        }
        fn write(&mut self, p: u32, v: u32) -> Result<()> {
            self.memory.insert(p, v);
            Ok(())
        }
        fn graph(&mut self, s: &mut State) -> Result<Option<usize>> {
            self.trace.push(json!(["graph", s.pathfinder]));
            if self.input["owner_change"].as_bool().unwrap() {
                s.pathfinder = Q;
            }
            Ok(self.input["graph"].as_bool().unwrap().then_some(G))
        }
        fn marker(&mut self, p: u32) -> Result<Option<u32>> {
            self.trace.push(json!(["marker", p]));
            let i = usize::from(p == GOAL);
            Ok(self.input["markers"][i]
                .as_bool()
                .unwrap()
                .then_some(MK + i as u32 * 0x10))
        }
        fn ready(&mut self, graph: usize) -> Result<u32> {
            self.trace.push(json!(["ready", graph]));
            Ok(u32::from(self.input["ready"].as_bool().unwrap()))
        }
        fn bind(&mut self, _: &mut State, graph: usize, id: u32) -> Result<u32> {
            self.trace.push(json!(["bind", graph, id]));
            self.write(graph as u32 + 4, if self.bind == 0 { SP } else { TP })?;
            self.write(graph as u32 + 8, 0xccccdddd)?;
            if self.input["marker_change"].as_bool().unwrap() {
                self.write(MK + 0x14, 99)?;
            }
            let result = self.input["bind"][self.bind].as_u64().unwrap() as u32;
            self.bind += 1;
            Ok(result)
        }
        fn traversal(&mut self, _: &mut State, graph: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["traversal", graph]));
            Ok(self.input["traversal"].as_bool().unwrap().then_some(SVC))
        }
        fn fallback(&mut self, s: &mut State) -> Result<Option<usize>> {
            let p = self.read(s.pathfinder as u32 + 0x364)?;
            Ok((p != 0).then_some(p as usize))
        }
        fn arrival_detector(&mut self, s: &mut State) -> Result<usize> {
            Ok(self.read(s.pathfinder as u32 + 0x170)? as usize)
        }
        fn current_position(&mut self, s: &mut State) -> Result<u32> {
            self.trace.push(json!(["position", s.pathfinder]));
            if self.input["current_change"].as_bool().unwrap() {
                s.pathfinder = Q;
            }
            Ok(POSITION)
        }
        fn reached(
            &mut self,
            _: &mut State,
            service: usize,
            current: u32,
            start: u32,
        ) -> Result<u32> {
            self.trace
                .push(json!(["contains", service, current, start]));
            Ok(self.input["contains"].as_u64().unwrap() as u32)
        }
        fn service_cost(&mut self, _: &mut State, service: usize, a: Args) -> Result<u32> {
            self.service(a, Some(service))
        }
        fn acceptance_cost(&mut self, _: &mut State, a: Args) -> Result<u32> {
            self.service(a, None)
        }
    }
    impl GapHost for Replay<'_> {
        fn bypass(&mut self, _: &mut State) -> Result<bool> {
            Ok(self.input["bypass"][0].as_u64().unwrap() != 0
                && self.input["bypass"][1].as_u64().unwrap() != 0)
        }
        fn refresh(&mut self, s: &mut State) -> Result<()> {
            self.trace.push(json!(["refresh"]));
            if self.input["refresh_remove"].as_bool().unwrap() {
                s.pool_118 = None;
            }
            if self.input["refresh_owner"].as_bool().unwrap() {
                s.pathfinder = Q;
            }
            Ok(())
        }
        fn entity_xz(&mut self, _: &mut State, entity: usize) -> Result<[f32; 2]> {
            Ok([
                f32::from_bits(self.read(entity as u32 + 0x34)?),
                f32::from_bits(self.read(entity as u32 + 0x3c)?),
            ])
        }
        fn zero_direction(&mut self, _: &mut State) -> Result<[f32; 3]> {
            anyhow::bail!("Native finite fixture unexpectedly used zero direction")
        }
    }
    #[test]
    fn original_goto_cost_restoration_policy_dispatch_and_directional_penalties_match() -> Result<()>
    {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_cost.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let mut h = Replay {
                input,
                memory: BTreeMap::new(),
                trace: vec![],
                bind: 0,
            };
            let mut s = State::construct(B, P, b"goto", &mut Construction)?;
            for (i, v) in [0x11112222, OLD, 0x33334444, 0x55556666]
                .into_iter()
                .enumerate()
            {
                h.write(G as u32 + i as u32 * 4, v)?;
            }
            h.write(MK + 4, 12)?;
            h.write(MK + 0x14, 34)?;
            h.write(COST, 3f32.to_bits())?;
            for parent in [P, Q] {
                h.write(parent as u32 + 0x170, LPF as u32)?;
                h.write(
                    parent as u32 + 0x364,
                    if input["fallback"].as_bool().unwrap() {
                        if parent == P {
                            ALT as u32
                        } else {
                            ALT2 as u32
                        }
                    } else {
                        0
                    },
                )?;
            }
            for i in 0..3 {
                h.write(
                    E as u32 + 0x34 + i * 4,
                    input["entity"][i as usize].as_u64().unwrap() as u32,
                )?;
                h.write(
                    START + 4 + i * 4,
                    input["start"][i as usize].as_u64().unwrap() as u32,
                )?;
                h.write(
                    GOAL + 4 + i * 4,
                    input["goal"][i as usize].as_u64().unwrap() as u32,
                )?;
            }
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
                s.pool_118 = Some(pool);
            }
            let args = Args {
                entity: E,
                start: START,
                goal: GOAL,
                acceptance: ACCEPT,
                cost: COST,
            };
            let result = if input["op"] == "base" {
                base(&mut h, &mut s, args)?
            } else {
                gap(&mut h, &mut s, args)?
            };
            assert_eq!(json!(result), case["result"], "result {index}");
            assert_eq!(json!(h.read(COST)?), case["cost"], "cost {index}");
            assert_eq!(json!(h.image()), case["graph"], "graph {index}");
            assert_eq!(json!(s.pathfinder), case["parent"], "parent {index}");
            assert_eq!(json!(s.pool_118.is_some()), case["pool"], "pool {index}");
            assert_eq!(json!(h.trace), case["trace"], "trace {index}");
        }
        Ok(())
    }
}
