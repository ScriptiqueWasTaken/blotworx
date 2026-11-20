//! CGapManager43efc0/43f194 configuration and43f640 interval reset.
//! Gap computation43f7d0 and World service registration are separate stages.
use super::{
    ai_goto_gap::{Node, Pool},
    ai_navigation_service_init,
    ai_traversal_registry::names_equal,
};
use anyhow::{Context, Result};
#[derive(Clone, Copy)]
pub struct Gap {
    pub parameters_08_0c_10: [Option<f32>; 3],
    pub limit_14: Option<f32>,
    pub flag_18: Option<u8>,
}
impl Gap {
    pub fn node() -> Node<Self> {
        Node {
            payload: Self {
                parameters_08_0c_10: [None; 3],
                limit_14: None,
                flag_18: None,
            },
            previous: None,
            next: None,
            active: 0,
        }
    }
}
pub struct State {
    pub identity: usize,
    pub pool: Option<Pool<Gap>>,
    // Attribute replacement does not destroy the old native allocation.
    pub retired_pools: Vec<Pool<Gap>>,
    pub parameters_08_0c_14: [Option<f32>; 3],
    pub limit_10: Option<f32>,
}
pub trait Host: ai_navigation_service_init::Host<State> {
    fn value(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn integer(&mut self, state: &mut State, value: usize) -> Result<i32>;
    fn allocate_pool(&mut self, _: &mut State, capacity: u32) -> Result<Option<Pool<Gap>>> {
        Ok(Some(Pool::new(capacity, Gap::node)?))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search {
    Above,
    Below,
}
pub struct SearchOutput {
    pub value: f32,
    pub interval: Option<usize>,
}
///440708/440780: scan through `end` inclusively, retaining outputs on failure.
/// Node activity/count are intentionally not additional filtering conditions.
pub fn search(
    pool: &Pool<Gap>,
    start: Option<usize>,
    end: Option<usize>,
    bounds: [f32; 2],
    threshold: f32,
    direction: Search,
    output: &mut SearchOutput,
) -> Result<bool> {
    let mut current = start;
    let mut steps = 0usize;
    while let Some(index) = current {
        let node = pool
            .nodes
            .get(index)
            .context("Gap search interval is outside owned nodes")?;
        anyhow::ensure!(steps < pool.nodes.len(), "Cycle in bounded gap search");
        steps += 1;
        let depth = node.payload.parameters_08_0c_10[2]
            .context("Gap search threshold field is uninitialized")?;
        let matches = match direction {
            Search::Above => depth > threshold,
            Search::Below => depth < threshold,
        };
        if matches {
            let lower = node.payload.parameters_08_0c_10[0]
                .context("Gap search lower endpoint is uninitialized")?;
            let (bound, positive, negative) = match direction {
                Search::Above => (bounds[0], lower, bounds[0]),
                Search::Below => (bounds[1], bounds[1], lower),
            };
            // fsubs/fsel, rather than host min/max: preserves signed-zero and
            // unordered operand selection from the original helper.
            output.value = if lower - bound >= 0. {
                positive
            } else {
                negative
            };
            output.interval = Some(index);
            return Ok(true);
        }
        if Some(index) == end {
            break;
        }
        current = node.next;
    }
    Ok(false)
}
impl State {
    pub fn new(identity: usize) -> Self {
        Self {
            identity,
            pool: None,
            retired_pools: vec![],
            parameters_08_0c_14: [None; 3],
            limit_10: None,
        }
    }
    fn replace_pool(&mut self, pool: Option<Pool<Gap>>) {
        if let Some(old) = self.pool.take() {
            self.retired_pools.push(old);
        }
        self.pool = pool;
    }
    pub fn initialize(&mut self, root: usize, host: &mut impl Host) -> Result<bool> {
        if !ai_navigation_service_init::initialize(self, root, host)? {
            return Ok(false);
        }
        if self.pool.is_none() {
            let pool = host.allocate_pool(self, 50)?;
            self.replace_pool(pool);
        }
        self.configure([-1., 1., f32::MAX])?;
        Ok(true)
    }
    pub fn attribute(&mut self, node: usize, host: &mut impl Host) -> Result<bool> {
        if host.name(self, node)?.is_some() {
            let name = host
                .name(self, node)?
                .context("Gap-manager name disappeared")?;
            if name.first() == Some(&b'_') {
                return Ok(true);
            }
        }
        let name = host
            .name(self, node)?
            .context("Gap-manager attribute has no name")?;
        if !names_equal(&name, b"MaxGaps") {
            return Ok(false);
        }
        if let Some(value) = host.value(self, node)? {
            let count = host.integer(self, value)?;
            if count > 0 {
                let pool = host.allocate_pool(self, count as u32)?;
                self.replace_pool(pool);
            }
        }
        Ok(true)
    }
    pub fn configure(&mut self, parameters: [f32; 3]) -> Result<()> {
        self.limit_10 = Some(f32::MAX);
        let pool = self
            .pool
            .as_mut()
            .context("Gap-manager interval reset has no allocated pool")?;
        pool.clear()?;
        if let Some(index) = pool.free_head {
            let node = pool
                .nodes
                .get_mut(index)
                .context("Gap free-list head is outside owned nodes")?;
            pool.count = pool.count.wrapping_add(1);
            pool.free_head = node.next;
            node.payload.parameters_08_0c_10 = parameters.map(Some);
            node.payload.limit_14 = Some(f32::MAX);
            node.payload.flag_18 = Some(0);
            node.active = 1;
            node.previous = None;
            node.next = None;
            pool.active_head = Some(index);
            pool.tail = Some(index);
        }
        self.parameters_08_0c_14 = parameters.map(Some);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn navigation_gap_search_matches_original_inclusive_bounds_and_float_selection() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_gap_search.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let rows = c["nodes"].as_array().unwrap();
            let mut pool = Pool::new(rows.len() as u32, Gap::node)?;
            for (node, row) in pool.nodes.iter_mut().zip(rows) {
                node.payload.parameters_08_0c_10[0] =
                    Some(f32::from_bits(row["lower"].as_u64().unwrap() as u32));
                node.payload.parameters_08_0c_10[2] =
                    Some(f32::from_bits(row["depth"].as_u64().unwrap() as u32));
                node.next = row["next"].as_u64().map(|i| i as usize);
            }
            let direction = if c["direction"] == "above" {
                Search::Above
            } else {
                Search::Below
            };
            let mut output = SearchOutput {
                value: f32::from_bits(c["old_value"].as_u64().unwrap() as u32),
                interval: c["old_interval"].as_u64().map(|i| i as usize),
            };
            let result = search(
                &pool,
                c["start"].as_u64().map(|i| i as usize),
                c["end"].as_u64().map(|i| i as usize),
                [
                    f32::from_bits(c["bounds"][0].as_u64().unwrap() as u32),
                    f32::from_bits(c["bounds"][1].as_u64().unwrap() as u32),
                ],
                f32::from_bits(c["threshold"].as_u64().unwrap() as u32),
                direction,
                &mut output,
            )?;
            assert_eq!(
                json!({"result":result,"value":output.value.to_bits(),"interval":output.interval}),
                json!({"result":expected["result"],"value":expected["value"],"interval":expected["interval"]}),
                "{c}"
            );
        }
        let mut pool = Pool::new(1, Gap::node)?;
        pool.nodes[0].next = Some(0);
        pool.nodes[0].payload.parameters_08_0c_10[2] = Some(0.);
        let mut output = SearchOutput {
            value: 99.,
            interval: None,
        };
        assert!(search(
            &pool,
            Some(0),
            None,
            [0., 1.],
            0.,
            Search::Above,
            &mut output
        )
        .unwrap_err()
        .to_string()
        .contains("Cycle"));
        assert_eq!(output.value, 99.);
        assert!(output.interval.is_none());
        Ok(())
    }
    struct Binding<'a> {
        case: &'a Value,
        rows: Vec<Value>,
        trace: Vec<Value>,
        headers: usize,
    }
    impl ai_navigation_service_init::Host<State> for Binding<'_> {
        fn child_count(&mut self, _: &mut State, _: usize) -> Result<u32> {
            Ok(self.rows.len() as u32)
        }
        fn child(&mut self, _: &mut State, _: usize, index: u32) -> Result<Option<usize>> {
            self.trace.push(json!(["child", index]));
            Ok(Some(index as usize))
        }
        fn name(&mut self, _: &mut State, node: usize) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["name", node]));
            Ok(Some(
                self.rows[node]["name"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
            ))
        }
        fn initialize_attribute(&mut self, state: &mut State, node: usize) -> Result<bool> {
            state.attribute(node, self)
        }
    }
    impl Host for Binding<'_> {
        fn value(&mut self, _: &mut State, node: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["value", node]));
            Ok(self.rows[node]["value"].as_str().map(|_| node))
        }
        fn integer(&mut self, _: &mut State, node: usize) -> Result<i32> {
            self.trace
                .push(json!(["integer", self.rows[node]["value"]]));
            Ok(self.rows[node]["integer"].as_i64().unwrap() as i32)
        }
        fn allocate_pool(&mut self, state: &mut State, capacity: u32) -> Result<Option<Pool<Gap>>> {
            self.headers += 1;
            self.trace
                .push(json!(["pool", capacity, state.pool.is_some()]));
            if self.case["fail_headers"].as_array().is_some_and(|values| {
                values
                    .iter()
                    .any(|v| v.as_u64() == Some(self.headers as u64))
            }) {
                return Ok(None);
            }
            Ok(Some(Pool::new(capacity, Gap::node)?))
        }
    }
    fn pool_image(pool: &Pool<Gap>, poison: u8) -> Value {
        let index = |i: Option<usize>| i.map_or(0, |i| i + 1);
        let nodes = pool
            .nodes
            .iter()
            .map(|node| {
                let mut image = vec![poison; 0x28];
                for (at, value) in [
                    (0, Some(0x5199f0)),
                    (4, Some(0x519ad8)),
                    (8, node.payload.parameters_08_0c_10[0].map(f32::to_bits)),
                    (0xc, node.payload.parameters_08_0c_10[1].map(f32::to_bits)),
                    (0x10, node.payload.parameters_08_0c_10[2].map(f32::to_bits)),
                    (0x14, node.payload.limit_14.map(f32::to_bits)),
                    (0x1c, Some(index(node.previous) as u32)),
                    (0x20, Some(index(node.next) as u32)),
                ] {
                    if let Some(value) = value {
                        image[at..at + 4].copy_from_slice(&value.to_be_bytes());
                    }
                }
                if let Some(flag) = node.payload.flag_18 {
                    image[0x18] = flag;
                }
                image[0x24] = node.active;
                image
            })
            .collect::<Vec<_>>();
        json!({"head":index(pool.active_head),"tail":index(pool.tail),"free":index(pool.free_head),"count":pool.count,"nodes":nodes})
    }
    #[test]
    fn navigation_gap_manager_matches_original_configuration_pool_ownership_and_active_insertion(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_gap_manager.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let poison = c["poison"].as_u64().unwrap_or(0xa5) as u8;
            let mut state = State::new(50);
            let mut host = Binding {
                case: c,
                rows: c["rows"].as_array().cloned().unwrap_or_default(),
                trace: vec![],
                headers: 0,
            };
            let result = if c["mode"] == "construct" {
                None
            } else {
                Some(state.initialize(1, &mut host)?)
            };
            if let Some(active) = c["active"].as_array() {
                let active = active
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect::<Vec<_>>();
                let pool = state.pool.as_mut().context("Test active pool missing")?;
                let free = (0..pool.nodes.len())
                    .filter(|i| !active.contains(i))
                    .collect::<Vec<_>>();
                for (i, node) in pool.nodes.iter_mut().enumerate() {
                    let chain = if active.contains(&i) {
                        active.as_slice()
                    } else {
                        free.as_slice()
                    };
                    let pos = chain.iter().position(|&v| v == i).unwrap();
                    node.previous = pos.checked_sub(1).map(|p| chain[p]);
                    node.next = chain.get(pos + 1).copied();
                    node.active = u8::from(active.contains(&i));
                    if active.contains(&i) {
                        node.payload.parameters_08_0c_10 = [Some(99.); 3];
                        node.payload.limit_14 = Some(99.);
                        node.payload.flag_18 = Some(1);
                    }
                }
                pool.active_head = active.first().copied();
                pool.tail = active.last().copied();
                pool.free_head = free.first().copied();
                pool.count = active.len() as u32;
            }
            if let Some(parameters) = c["parameters"].as_array() {
                let values = parameters
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect::<Vec<_>>();
                state.configure(values.try_into().unwrap())?;
            }
            let mut image = vec![poison; 0x18];
            for (at, value) in [
                (0, Some(0x519a38)),
                (4, Some(u32::from(state.pool.is_some()))),
                (8, state.parameters_08_0c_14[0].map(f32::to_bits)),
                (0xc, state.parameters_08_0c_14[1].map(f32::to_bits)),
                (0x10, state.limit_10.map(f32::to_bits)),
                (0x14, state.parameters_08_0c_14[2].map(f32::to_bits)),
            ] {
                if let Some(value) = value {
                    image[at..at + 4].copy_from_slice(&value.to_be_bytes());
                }
            }
            let expected_pool = expected["pool"]
                .as_u64()
                .map(|i| fixture["pool_images"][i as usize].clone());
            let retired = expected["retired"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| fixture["pool_images"][i.as_u64().unwrap() as usize].clone())
                .collect::<Vec<_>>();
            assert_eq!(
                json!({"result":result,"trace":host.trace,"bytes":image,
                "pool":state.pool.as_ref().map(|pool|pool_image(pool,poison)),"retired":state.retired_pools.iter().map(|pool|pool_image(pool,poison)).collect::<Vec<_>>()}),
                json!({"result":expected["result"],"trace":expected["trace"],"bytes":expected["bytes"],"pool":expected_pool,"retired":retired}),
                "{c}"
            );
        }
        let c = json!({"fail_headers":[1]});
        let mut state = State::new(50);
        let mut host = Binding {
            case: &c,
            rows: vec![],
            trace: vec![],
            headers: 0,
        };
        assert!(
            state.initialize(1, &mut host).is_err(),
            "Missing default pool was accepted as usable"
        );
        let mut state = State::new(50);
        state.pool = Some(Pool::new(0, Gap::node)?);
        state.configure([2., 4., 8.])?;
        assert_eq!(state.pool.as_ref().unwrap().count, 0);
        assert_eq!(state.parameters_08_0c_14, [Some(2.), Some(4.), Some(8.)]);
        Ok(())
    }
}
