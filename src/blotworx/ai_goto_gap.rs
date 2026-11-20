//! Concrete CGoto_GapDynamicAvoidance_v2 construction4a831c and reset4a95d8.
//! Configuration, pool allocation and steering updates remain separate stages.
use anyhow::{ensure, Context, Result};

pub struct Node<T> {
    pub payload: T,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    pub active: u8,
}
pub struct Pool<T> {
    pub nodes: Vec<Node<T>>,
    pub active_head: Option<usize>,
    pub free_head: Option<usize>,
    pub tail: Option<usize>,
    pub count: u32,
}
impl<T> Pool<T> {
    /// Initial free-list topology used by4a88e4 for both cache layouts.
    pub fn new(capacity: u32, mut create: impl FnMut() -> Node<T>) -> Result<Self> {
        let capacity = capacity as usize;
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(capacity)?;
        for index in 0..capacity {
            let mut node = create();
            node.previous = index.checked_sub(1);
            node.next = (index + 1 < capacity).then_some(index + 1);
            nodes.push(node);
        }
        Ok(Self {
            nodes,
            active_head: None,
            free_head: (capacity != 0).then_some(0),
            tail: None,
            count: 0,
        })
    }
    pub fn clear(&mut self) -> Result<()> {
        let mut steps = 0usize;
        while let Some(head) = self.active_head {
            ensure!(
                head < self.nodes.len(),
                "Steering cache link is outside owned nodes"
            );
            ensure!(
                steps < self.nodes.len(),
                "Cycle in active steering cache list"
            );
            steps += 1;
            let node = self
                .nodes
                .get_mut(head)
                .context("Steering cache link is outside owned nodes")?;
            node.active = 0;
            let next = node.next;
            node.next = self.free_head;
            self.free_head = Some(head);
            self.active_head = next;
        }
        self.count = 0;
        self.tail = None;
        Ok(())
    }
}
pub struct Cache34 {
    pub words_00_24: [Option<u32>; 10],
}
pub struct Cache24 {
    pub words_00_14: [Option<u32>; 6],
}
impl Cache34 {
    ///4a8d00: cache payload constructor; pool links are initialized separately.
    pub fn node() -> Node<Self> {
        let mut words = [Some(0); 10];
        words[0] = Some(0x5213b4);
        words[1] = Some(0x521128);
        Node {
            payload: Self { words_00_24: words },
            previous: None,
            next: None,
            active: 0,
        }
    }
}
impl Cache24 {
    ///4a8cb4: its inline scalar is one, not zero-filled cache storage.
    pub fn node() -> Node<Self> {
        let mut words = [Some(0); 6];
        words[0] = Some(0x521420);
        words[1] = Some(0x521208);
        words[5] = Some(1f32.to_bits());
        Node {
            payload: Self { words_00_14: words },
            previous: None,
            next: None,
            active: 0,
        }
    }
}
pub struct State {
    pub identity: usize,
    pub pathfinder: usize,
    pub name: Vec<u8>,
    pub budget_0c: Option<i32>,
    pub scalars_10_20: [Option<f32>; 5],
    pub flag_24: Option<u8>,
    pub scalars_28_38: [Option<f32>; 5],
    pub word_3c: Option<u32>,
    pub scalar_40: Option<f32>,
    pub flag_44: Option<u8>,
    pub scalar_48: Option<f32>,
    pub managers_4c_50: [Option<Option<usize>>; 2],
    pub reference_54: Option<Option<usize>>,
    pub entity_58: Option<usize>,
    pub attributes_68_6c_70: [Option<Option<usize>>; 3],
    pub vectors_5c_80_8c_9c_a8_b4: [[Option<f32>; 3]; 6],
    pub scales_74_78: [Option<f32>; 2],
    pub flag_7c: Option<u8>,
    pub scalar_c4: Option<f32>,
    pub word_98: Option<u32>,
    pub scalars_cc_d0: [Option<f32>; 2],
    pub pool_d4: Option<Pool<Cache34>>,
    pub vector_d8: [Option<f32>; 3],
    pub scalar_e4: Option<f32>,
    pub word_f4: Option<u32>,
    pub flags_f8_f9_fa: [Option<u8>; 3],
    pub scalar_fc: Option<f32>,
    pub inverse_100: Option<f32>,
    pub vector_104: [Option<f32>; 3],
    pub word_110: Option<u32>,
    pub word_114: Option<u32>,
    pub pool_118: Option<Pool<Cache24>>,
}
pub trait Host {
    fn unit_scale(&mut self, state: &mut State) -> Result<Option<f32>>;
    /// Read the current pathfinder->brain->entity link at the native query point.
    fn owner_entity(&mut self, state: &mut State) -> Result<usize>;
    fn profiling_enabled(&mut self, state: &mut State) -> Result<bool>;
    fn register_budget(&mut self, state: &mut State, name: &str) -> Result<i32>;
}
impl State {
    pub fn construct(
        identity: usize,
        pathfinder: usize,
        name: &[u8],
        host: &mut impl Host,
    ) -> Result<Self> {
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let mut s = Self {
            identity,
            pathfinder,
            name: name.to_vec(),
            budget_0c: None,
            scalars_10_20: [None; 5],
            flag_24: None,
            scalars_28_38: [None; 5],
            word_3c: None,
            scalar_40: None,
            flag_44: None,
            scalar_48: None,
            managers_4c_50: [None; 2],
            reference_54: None,
            entity_58: None,
            attributes_68_6c_70: [None; 3],
            vectors_5c_80_8c_9c_a8_b4: [[None; 3]; 6],
            scales_74_78: [None; 2],
            flag_7c: None,
            scalar_c4: None,
            word_98: None,
            scalars_cc_d0: [None; 2],
            pool_d4: None,
            vector_d8: [None; 3],
            scalar_e4: None,
            word_f4: None,
            flags_f8_f9_fa: [None; 3],
            scalar_fc: None,
            inverse_100: None,
            vector_104: [None; 3],
            word_110: None,
            word_114: None,
            pool_118: None,
        };
        s.scalars_10_20[0] = Some(20. * host.unit_scale(&mut s)?.unwrap_or(1.));
        s.scalars_10_20[1] = Some(f32::from_bits(0x3dcccccd));
        s.scalars_10_20[2] = Some(3. * host.unit_scale(&mut s)?.unwrap_or(1.));
        s.scalars_10_20[3] =
            Some(f32::from_bits(0x3dcccccd) * host.unit_scale(&mut s)?.unwrap_or(1.));
        s.scalars_10_20[4] =
            Some(f32::from_bits(0x3dcccccd) * host.unit_scale(&mut s)?.unwrap_or(1.));
        s.flag_24 = Some(0);
        s.scalars_28_38[0] = Some(host.unit_scale(&mut s)?.unwrap_or(1.));
        for (i, value) in [(1, 1.), (2, 5.), (3, 5.), (4, 1.)] {
            s.scalars_28_38[i] = Some(value);
        }
        s.word_3c = Some(100);
        s.scalar_40 = Some(360.);
        s.flag_44 = Some(1);
        s.scalar_48 = Some(3. * host.unit_scale(&mut s)?.unwrap_or(1.));
        s.reference_54 = Some(None);
        let entity = host.owner_entity(&mut s)?;
        ensure!(entity != 0, "Steering modifier has no owner entity");
        s.entity_58 = Some(entity);
        s.vectors_5c_80_8c_9c_a8_b4 = [[Some(0.); 3]; 6];
        s.word_f4 = Some(0);
        let scale = host.unit_scale(&mut s)?.unwrap_or(1.);
        s.inverse_100 = Some(1. / (3. * scale));
        s.vector_104 = [Some(0.); 3];
        s.word_114 = Some(0);
        s.reset(host)?;
        s.budget_0c = Some(-1);
        if host.profiling_enabled(&mut s)? {
            s.budget_0c = Some(host.register_budget(&mut s, "UpdateCollisionDiagram")?);
        }
        Ok(s)
    }
    pub fn reset(&mut self, host: &mut impl Host) -> Result<()> {
        self.flags_f8_f9_fa[2] = Some(1);
        self.vectors_5c_80_8c_9c_a8_b4[1] = [Some(f32::MAX); 3];
        self.vectors_5c_80_8c_9c_a8_b4[2] = [Some(f32::MAX); 3];
        self.word_98 = Some(0);
        self.word_110 = Some(0);
        self.scales_74_78[0] = Some(host.unit_scale(self)?.unwrap_or(1.));
        self.scales_74_78[1] = Some(host.unit_scale(self)?.unwrap_or(1.));
        self.scalars_cc_d0 = [Some(0.); 2];
        if let Some(pool) = &mut self.pool_d4 {
            pool.clear()?;
        }
        self.vector_d8 = [Some(0.); 3];
        self.word_f4 = Some(0);
        self.flags_f8_f9_fa[0] = Some(1);
        self.flags_f8_f9_fa[1] = Some(0);
        self.scalar_e4 = Some(0.);
        self.scalar_fc = Some(0.);
        if let Some(pool) = &mut self.pool_118 {
            pool.clear()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn navigation_goto_cache_pool_links_match_original_setup_lists() -> Result<()> {
        fn snapshot<T>(pool: &Pool<T>) -> Value {
            json!({"active_head":pool.active_head,"free_head":pool.free_head,"tail":pool.tail,"active_count":pool.count,
                "nodes":pool.nodes.iter().map(|n|json!({"previous":n.previous,"next":n.next,"active":n.active})).collect::<Vec<_>>()})
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_gap_pool.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let count = case["count"].as_u64().unwrap() as u32;
            let actual = if case["stride"] == 0x34 {
                snapshot(&Pool::new(count, Cache34::node)?)
            } else {
                snapshot(&Pool::new(count, Cache24::node)?)
            };
            assert_eq!(
                actual,
                json!({"active_head":case["active_head"],"free_head":case["free_head"],"tail":case["tail"],"active_count":case["active_count"],"nodes":case["nodes"]})
            );
        }
        let empty = Pool::new(0, Cache34::node)?;
        assert!(empty.nodes.is_empty() && empty.free_head.is_none());
        Ok(())
    }
    #[test]
    fn navigation_goto_cache_node_constructors_match_original_payload_links_and_padding(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_gap_nodes.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let size = case["size"].as_u64().unwrap() as usize;
            let (words, previous, next, active) = if size == 0x34 {
                let node = Cache34::node();
                (
                    node.payload.words_00_24.to_vec(),
                    node.previous,
                    node.next,
                    node.active,
                )
            } else {
                let node = Cache24::node();
                (
                    node.payload.words_00_14.to_vec(),
                    node.previous,
                    node.next,
                    node.active,
                )
            };
            let mut image = vec![case["poison"].as_u64().unwrap() as u8; size];
            for (i, value) in words.into_iter().enumerate() {
                image[i * 4..i * 4 + 4].copy_from_slice(
                    &value
                        .context("Missing initialized cache word")?
                        .to_be_bytes(),
                );
            }
            assert!(previous.is_none() && next.is_none());
            image[size - 12..size - 4].fill(0);
            image[size - 4] = active;
            assert_eq!(json!(image), case["bytes"]);
        }
        Ok(())
    }
    struct Binding<'a> {
        case: &'a Value,
        scales: Vec<Option<u32>>,
        index: usize,
        entity: usize,
        trace: Vec<Value>,
    }
    impl Host for Binding<'_> {
        fn unit_scale(&mut self, s: &mut State) -> Result<Option<f32>> {
            let index = self.index;
            self.index += 1;
            let scale = self.scales[index.min(self.scales.len() - 1)];
            self.trace.push(json!(["scale", scale]));
            if self.case["owner_change"] == true && index == 2 {
                self.entity = 21;
            }
            if self.case["mutation"] == true && index == 1 {
                s.scalars_10_20[0] = Some(123.);
            }
            Ok(scale.map(f32::from_bits))
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            self.trace.push(json!(["owner"]));
            Ok(self.entity)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            self.trace.push(json!(["profiler"]));
            Ok(self.case["profiling"].as_u64().unwrap() != 0)
        }
        fn register_budget(&mut self, _: &mut State, name: &str) -> Result<i32> {
            self.trace.push(json!(["profiler"]));
            self.trace.push(json!(["budget", name]));
            Ok(self.case["budget"].as_i64().unwrap() as i32)
        }
    }
    fn bytes(s: &State) -> Vec<(usize, u8)> {
        let mut bytes = std::collections::BTreeMap::new();
        {
            let mut put = |at: usize, value: u32| {
                for (i, b) in value.to_be_bytes().into_iter().enumerate() {
                    bytes.insert(at + i, b);
                }
            };
            put(0, 0x521508);
            put(4, s.pathfinder as u32);
            put(8, 40);
            put(0xf0, 0x5210b8);
            if let Some(v) = s.budget_0c {
                put(0xc, v as u32);
            }
            for (base, values) in [
                (0x10, s.scalars_10_20.as_slice()),
                (0x28, s.scalars_28_38.as_slice()),
                (0x74, s.scales_74_78.as_slice()),
                (0xcc, s.scalars_cc_d0.as_slice()),
                (0xd8, s.vector_d8.as_slice()),
                (0x104, s.vector_104.as_slice()),
            ] {
                for (i, v) in values.iter().enumerate() {
                    if let Some(v) = v {
                        put(base + 4 * i, v.to_bits());
                    }
                }
            }
            for (i, base) in [0x5c, 0x80, 0x8c, 0x9c, 0xa8, 0xb4].into_iter().enumerate() {
                for (j, v) in s.vectors_5c_80_8c_9c_a8_b4[i].iter().enumerate() {
                    if let Some(v) = v {
                        put(base + 4 * j, v.to_bits());
                    }
                }
            }
            for (at, v) in [
                (0x40, s.scalar_40),
                (0x48, s.scalar_48),
                (0xe4, s.scalar_e4),
                (0xfc, s.scalar_fc),
                (0x100, s.inverse_100),
            ] {
                if let Some(v) = v {
                    put(at, v.to_bits());
                }
            }
            for (at, v) in [
                (0x3c, s.word_3c),
                (0x98, s.word_98),
                (0xf4, s.word_f4),
                (0x110, s.word_110),
                (0x114, s.word_114),
            ] {
                if let Some(v) = v {
                    put(at, v);
                }
            }
            if let Some(v) = s.reference_54 {
                put(0x54, v.unwrap_or(0) as u32);
            }
            if let Some(v) = s.entity_58 {
                put(0x58, v as u32);
            }
            put(0xd4, if s.pool_d4.is_some() { 60 } else { 0 });
            put(0x118, if s.pool_118.is_some() { 70 } else { 0 });
        }
        for (at, v) in [
            (0x24, s.flag_24),
            (0x44, s.flag_44),
            (0xf8, s.flags_f8_f9_fa[0]),
            (0xf9, s.flags_f8_f9_fa[1]),
            (0xfa, s.flags_f8_f9_fa[2]),
        ] {
            if let Some(v) = v {
                bytes.insert(at, v);
            }
        }
        bytes.into_iter().collect()
    }
    fn pool<T>(n: usize, active: &[usize], mut payload: impl FnMut(usize) -> T) -> Pool<T> {
        let free = (0..n).filter(|i| !active.contains(i)).collect::<Vec<_>>();
        let nodes = (0..n)
            .map(|i| {
                let chain = if active.contains(&i) {
                    active
                } else {
                    free.as_slice()
                };
                let index = chain.iter().position(|&v| v == i).unwrap();
                Node {
                    payload: payload(i),
                    previous: i.checked_sub(1),
                    next: chain.get(index + 1).copied(),
                    active: if active.contains(&i) { 7 } else { 4 },
                }
            })
            .collect();
        Pool {
            nodes,
            active_head: active.first().copied(),
            free_head: free.first().copied(),
            tail: active.last().copied(),
            count: active.len() as u32 + 9,
        }
    }
    fn pool_json<T>(pool: &Pool<T>, payload: impl Fn(&T) -> Value) -> Value {
        json!({"active_head":pool.active_head,"free_head":pool.free_head,"tail":pool.tail,"count":pool.count,
            "nodes":pool.nodes.iter().map(|n|json!({"payload":payload(&n.payload),"previous":n.previous,"next":n.next,"active":n.active})).collect::<Vec<_>>()})
    }
    #[test]
    fn navigation_goto_gap_construction_and_reset_match_original_owned_fields_and_pool_links(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_gap.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut host = Binding {
                case: c,
                scales: serde_json::from_value(c["scales"].clone())?,
                index: 0,
                entity: 20,
                trace: vec![],
            };
            let mut source = c["name"].as_str().unwrap().as_bytes().to_vec();
            let mut s = State::construct(60, 50, &source, &mut host)?;
            source.fill(0xff);
            if c["mode"] == "reset" {
                s.word_98 = Some(0x12345678);
                s.word_110 = Some(0x12345678);
                s.word_114 = Some(0x12345678);
                s.word_f4 = Some(0x12345678);
                s.vectors_5c_80_8c_9c_a8_b4[1] = [Some(99.); 3];
                s.vectors_5c_80_8c_9c_a8_b4[2] = [Some(99.); 3];
                s.scales_74_78 = [Some(99.); 2];
                s.scalars_cc_d0 = [Some(99.); 2];
                s.vector_d8 = [Some(99.); 3];
                s.scalar_e4 = Some(99.);
                s.scalar_fc = Some(99.);
                s.flags_f8_f9_fa = [Some(9), Some(8), Some(7)];
                let active34: Vec<usize> = serde_json::from_value(c["active34"].clone())?;
                let active24: Vec<usize> = serde_json::from_value(c["active24"].clone())?;
                s.pool_d4 = Some(pool(4, &active34, |i| Cache34 {
                    words_00_24: std::array::from_fn(|j| Some((((i + 1) << 16) + j) as u32)),
                }));
                s.pool_118 = Some(pool(3, &active24, |i| Cache24 {
                    words_00_14: std::array::from_fn(|j| Some((((i + 1) << 16) + j) as u32)),
                }));
                host.scales = serde_json::from_value(c["reset_scales"].clone())?;
                host.index = 0;
                host.trace.clear();
                s.reset(&mut host)?;
            }
            let mut image: std::collections::BTreeMap<usize, u8> =
                serde_json::from_value::<Vec<(usize, u8)>>(fixture["initial_bytes"].clone())?
                    .into_iter()
                    .collect();
            for (at, v) in
                serde_json::from_value::<Vec<(usize, u8)>>(expected["byte_overrides"].clone())?
            {
                ensure!(
                    image.contains_key(&at),
                    "Fixture changes initialized-byte mask"
                );
                image.insert(at, v);
            }
            assert_eq!(
                bytes(&s),
                image.into_iter().collect::<Vec<_>>(),
                "fields:{c}"
            );
            assert_eq!(json!(host.trace), expected["trace"], "callbacks:{c}");
            assert_eq!(
                String::from_utf8(s.name.clone())?,
                expected["name"].as_str().unwrap()
            );
            let pools = if let (Some(a), Some(b)) = (&s.pool_d4, &s.pool_118) {
                json!([
                    pool_json(a, |n| json!(n.words_00_24)),
                    pool_json(b, |n| json!(n.words_00_14))
                ])
            } else {
                Value::Null
            };
            assert_eq!(pools, expected["pools"], "pool links/payload:{c}");
        }
        Ok(())
    }
}
