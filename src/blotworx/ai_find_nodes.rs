//! Native1c8b90 nearest-reachable wrapper and1c8b30 goto-cost filter.
//! The default search uses the complete47e5a8 control; providers must supply
//! real collection, goto-cost, spatial, graph and task services.
use super::ai_nearest_search as nearest;
use anyhow::Result;
pub struct State {
    pub identity: usize,
    pub pathfinder: usize,
    pub name: Vec<u8>,
    pub start_flags: u32,
    pub goal_flags: u32,
}
#[derive(Clone, Copy)]
pub struct Context {
    pub search: nearest::Context,
    pub frame: u32,
    pub callback_key: u32,
}
#[derive(Clone, Copy)]
pub struct Args {
    pub start: u32,
    pub goal: u32,
    pub graph: usize,
    pub workspace: u32,
    pub start_node: u32,
    pub goal_node: u32,
}
pub trait Host: nearest::Host + Sized {
    ///18dd90 returns base+c acceptance;496268 is the separate position query.
    fn acceptance_policy(&mut self, state: &mut State, parent: usize) -> Result<u32>;
    /// Override only when an actual owning runtime supplies a different search
    /// dispatcher. The production default is the ported complete native control.
    fn search(
        &mut self,
        _: &mut State,
        context: nearest::Context,
        args: nearest::Args,
    ) -> Result<u32> {
        nearest::query(self, context, args)
    }
    fn goto_cost(
        &mut self,
        goto: usize,
        entity: usize,
        source: u32,
        target: u32,
        current: u32,
        cost: u32,
    ) -> Result<u32>;
}
fn pointer(h: &mut impl nearest::Host, parent: usize, offset: u32) -> Result<u32> {
    h.read(u32::try_from(parent)?.wrapping_add(offset))
}
fn info(
    h: &mut impl nearest::Host,
    p: u32,
    goto: u32,
    current: u32,
    entity: usize,
    graph: usize,
    start: bool,
) -> Result<()> {
    for (i, value) in [goto, current, u32::try_from(entity)?, u32::try_from(graph)?]
        .into_iter()
        .enumerate()
    {
        h.write(p.wrapping_add(i as u32 * 4), value)?;
    }
    let padding = h.read(p.wrapping_add(16))? & 0x00ffffff;
    h.write(p.wrapping_add(16), padding | u32::from(start) << 24)
}
/// Boolean admission is low-byte only; the actual goto owns cost publication.
pub fn filter(h: &mut impl Host, source: u32, target: u32, cost: u32, data: u32) -> Result<bool> {
    let goto = h.read(data)? as usize;
    let entity = h.read(data.wrapping_add(8))? as usize;
    let current = h.read(data.wrapping_add(4))?;
    Ok(h.goto_cost(goto, entity, source, target, current, cost)? & 255 != 0)
}
impl State {
    pub const CLASS: &'static [u8] = b"Mpd::CFindNodesFromPositions_NearestReachable";
    pub fn construct(identity: usize, pathfinder: usize, name: &[u8]) -> Self {
        let name = &name[..name.iter().position(|&v| v == 0).unwrap_or(name.len())];
        Self {
            identity,
            pathfinder,
            name: name.to_vec(),
            start_flags: 0,
            goal_flags: 0,
        }
    }
    ///2b7c00: flags survive resets; they are replaced only after explicit-workspace searches.
    pub fn reset(&mut self) {}
    pub fn query(&mut self, h: &mut impl Host, c: Context, a: Args) -> Result<bool> {
        let parent = self.pathfinder;
        // Capture selected goto before virtual3c; callbacks may change the owner.
        let goto = pointer(h, parent, 0x1ac)?;
        let current = h.acceptance_policy(self, parent)?;
        let entity = h.entity(self.pathfinder)?;
        if h.read(a.start_node)? == u32::MAX {
            let data = c.frame.wrapping_add(0x2c);
            info(h, data, goto, current, entity, a.graph, true)?;
            let limit = pointer(h, parent, 0x98)?;
            let result = h.search(
                self,
                c.search,
                nearest::Args {
                    graph: a.graph,
                    point: a.start,
                    mode: 1,
                    output: a.start_node,
                    parent,
                    direction: 1,
                    workspace: a.workspace,
                    reset: 0,
                    limit,
                    filter: nearest::Filter {
                        function: c.callback_key,
                        data,
                    },
                },
            )?;
            if a.workspace != 0 {
                self.start_flags = h.read(a.workspace.wrapping_add(20))?;
            }
            if result != 0 {
                return Ok(false);
            }
            if h.read(a.start_node)? == u32::MAX {
                return Ok(true);
            }
        }
        let marker_index = h.marker_index()?;
        let marker = h.read(
            a.goal
                .wrapping_add(16)
                .wrapping_add(marker_index.wrapping_mul(4)),
        )?;
        if marker != 0 {
            let node = h.read(marker.wrapping_add(4))?;
            h.write(a.goal_node, node)?;
            return Ok(true);
        }
        let fresh_parent = self.pathfinder;
        let goto = pointer(h, fresh_parent, 0x1ac)?;
        let current = h.acceptance_policy(self, fresh_parent)?;
        let entity = h.entity(self.pathfinder)?;
        let data = c.frame.wrapping_add(0x18);
        info(h, data, goto, current, entity, a.graph, false)?;
        let limit = pointer(h, parent, 0x98)?;
        let result = h.search(
            self,
            c.search,
            nearest::Args {
                graph: a.graph,
                point: a.goal,
                mode: 1,
                output: a.goal_node,
                parent,
                direction: 0,
                workspace: a.workspace,
                reset: 0,
                limit,
                filter: nearest::Filter {
                    function: c.callback_key,
                    data,
                },
            },
        )?;
        if a.workspace != 0 {
            self.goal_flags = h.read(a.workspace.wrapping_add(20))?;
        }
        Ok(result == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const B: usize = 0x60000000;
    const P: usize = 0x61000000;
    const Q: usize = P + 0x1000;
    const R: usize = P + 0x2000;
    const POINT: u32 = 0x62000000;
    const GOAL: u32 = POINT + 0x100;
    const SN: u32 = POINT + 0x200;
    const GN: u32 = SN + 4;
    const W: u32 = 0x63000000;
    const G: usize = 0x64000000;
    const MARK: u32 = 0x66000000;
    const DATA: u32 = 0x67000000;
    const COST: u32 = DATA + 0x100;
    struct Replay<'a> {
        input: &'a Value,
        memory: BTreeMap<u32, u32>,
        trace: Vec<Value>,
        search: usize,
        current: usize,
    }
    macro_rules! unused {($($name:ident($($arg:ident:$ty:ty),*)->$ret:ty;)*)=>{$(fn $name(&mut self,$($arg:$ty),*)->Result<$ret>{anyhow::bail!(concat!("Unexpected lower-search boundary: ",stringify!($name)))})*}}
    impl nearest::Host for Replay<'_> {
        fn read(&mut self, p: u32) -> Result<u32> {
            Ok(*self.memory.get(&p).unwrap_or(&0))
        }
        fn write(&mut self, p: u32, v: u32) -> Result<()> {
            self.memory.insert(p, v);
            Ok(())
        }
        fn entity(&mut self, parent: usize) -> Result<usize> {
            Ok(parent + 0x400)
        }
        fn marker_index(&mut self) -> Result<u32> {
            Ok(7)
        }
        unused! {
         default_workspace(graph:usize)->u32;graph_radius(graph:usize)->f32;graph_modes(graph:usize)->[Option<u32>;4];collect(graph:usize,request:nearest::Collection)->u32;
         query_flags()->u32;traversal_service()->Option<usize>;task_index(graph:usize)->u32;clock()->usize;task(op:nearest::Task,clock:usize,index:u32,entity:usize)->u32;
         bind(graph:usize,node:u32)->u32;connections(graph:usize)->u32;select_connection(graph:usize,index:u32)->();allowed(graph:usize,entity:usize,mode:u32)->u32;base_accept(graph:usize,entity:usize)->u32;
         spatial_service(parent:usize)->usize;class(service:usize)->usize;class_parent(class:usize)->Option<usize>;diameter(entity:usize)->Option<f32>;unit_scale()->Option<f32>;
         query34(service:usize,start:u32,end:u32,mode:u32,radius:f32)->u32;query58(parent:usize,start:u32,end:u32)->u32;lpf(parent:usize)->Option<usize>;lpf_ready(lpf:usize)->u32;lpf_query(lpf:usize,start:u32,end:u32,direction:u32,output:u32)->u32;
        }
    }
    impl Host for Replay<'_> {
        fn acceptance_policy(&mut self, state: &mut State, parent: usize) -> Result<u32> {
            self.trace.push(json!(["current", parent]));
            self.current += 1;
            if self.input["owner_change"].as_u64().unwrap() >= self.current as u64 {
                state.pathfinder = if self.current == 1 { Q } else { R };
            }
            Ok(parent as u32 + 0x600)
        }
        fn search(
            &mut self,
            state: &mut State,
            _: nearest::Context,
            a: nearest::Args,
        ) -> Result<u32> {
            use nearest::Host as _;
            let info: Vec<_> = (0..4)
                .map(|i| self.read(a.filter.data + i * 4).unwrap())
                .collect();
            let flag = self.read(a.filter.data + 16)? >> 24;
            self.trace.push(json!([
                "search",
                [
                    a.graph as u32,
                    a.point,
                    a.mode,
                    a.output,
                    a.parent as u32,
                    a.direction,
                    a.workspace,
                    a.reset
                ],
                a.limit,
                a.filter.function == 1,
                info,
                flag
            ]));
            self.write(
                a.output,
                self.input["nodes"][self.search].as_u64().unwrap() as u32,
            )?;
            if a.workspace != 0 {
                self.write(
                    a.workspace + 20,
                    self.input["flags"][self.search].as_u64().unwrap() as u32,
                )?;
            }
            if self.input["search_change"].as_bool().unwrap() {
                state.pathfinder = R;
                self.write(P as u32 + 0x98, 37)?;
            }
            let result = self.input["results"][self.search].as_u64().unwrap() as u32;
            self.search += 1;
            Ok(result)
        }
        fn goto_cost(
            &mut self,
            goto: usize,
            entity: usize,
            source: u32,
            target: u32,
            current: u32,
            cost: u32,
        ) -> Result<u32> {
            self.trace.push(json!([
                "cost",
                [goto as u32, entity as u32, source, target, current, cost]
            ]));
            nearest::Host::write(self, cost, 0xfeed1234)?;
            Ok(self.input["cost_result"].as_u64().unwrap() as u32)
        }
    }
    #[test]
    fn original_nearest_wrapper_retains_owner_live_budget_marker_and_pending_contracts(
    ) -> Result<()> {
        use nearest::Host as _;
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_find_nodes.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            if input["op"] == "init" {
                continue;
            }
            let mut h = Replay {
                input,
                memory: BTreeMap::new(),
                trace: vec![],
                search: 0,
                current: 0,
            };
            let mut s = State::construct(B, P, b"reachable\0ignored");
            assert_eq!(s.name, b"reachable");
            assert_eq!([s.start_flags, s.goal_flags], [0, 0]);
            s.start_flags = 11;
            s.goal_flags = 22;
            s.reset();
            for (i, parent) in [P, Q, R].into_iter().enumerate() {
                h.write(parent as u32 + 0x1ac, parent as u32 + 0x500)?;
                h.write(parent as u32 + 0x98, 10 + i as u32)?;
            }
            h.write(SN, input["start"].as_u64().unwrap() as u32)?;
            h.write(GN, 0x1234)?;
            if let Some(marker) = input["marker"].as_u64() {
                h.write(GOAL + 16 + 7 * 4, MARK)?;
                h.write(MARK + 4, marker as u32)?;
            }
            let result = if input["op"] == "callback" {
                for (i, v) in [
                    P as u32 + 0x500,
                    POINT,
                    P as u32 + 0x400,
                    G as u32,
                    0x01000000,
                ]
                .into_iter()
                .enumerate()
                {
                    h.write(DATA + i as u32 * 4, v)?;
                }
                filter(&mut h, POINT, GOAL, COST, DATA)?
            } else {
                s.query(
                    &mut h,
                    Context {
                        search: nearest::Context {
                            frame: 0x70001000,
                            point_kind: 0,
                            marker_kind: 0x4ebe80,
                            spatial_class: 0x11864,
                        },
                        frame: 0x70000000,
                        callback_key: 1,
                    },
                    Args {
                        start: POINT,
                        goal: GOAL,
                        graph: G,
                        workspace: if input["workspace"].as_bool().unwrap() {
                            W
                        } else {
                            0
                        },
                        start_node: SN,
                        goal_node: GN,
                    },
                )?
            };
            assert_eq!(json!(u32::from(result)), case["result"], "result {index}");
            assert_eq!(json!(s.pathfinder), case["parent"], "parent {index}");
            assert_eq!(json!(h.read(SN)?), case["start"], "start {index}");
            assert_eq!(json!(h.read(GN)?), case["goal"], "goal {index}");
            assert_eq!(
                json!([s.start_flags, s.goal_flags]),
                case["flags"],
                "flags {index}"
            );
            assert_eq!(json!(h.read(COST)?), case["cost"], "cost {index}");
            assert_eq!(json!(h.trace), case["trace"], "trace {index}");
        }
        Ok(())
    }
}
