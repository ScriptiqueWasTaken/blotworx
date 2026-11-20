//! Native traversal-cost collection4997f0..499ab4.
//! Concrete spatial query40, exclusion484c80 and contribution2c are required.
use super::{ai_graph_link_passes::Memory, ai_segment_query::WalkPoint};
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
fn word(m: &Memory, b: u32, o: u32) -> Result<u32> {
    m.word(b.wrapping_add(o))
}
fn compatible(m: &Memory, mut class: u32, target: u32) -> Result<bool> {
    let mut visited = BTreeSet::new();
    loop {
        if class == target {
            return Ok(true);
        }
        ensure!(
            visited.insert(class),
            "Cyclic traversal-cost class ancestry"
        );
        class = word(m, class, 0x14)?;
        if class == 0 {
            return Ok(false);
        }
    }
}
#[derive(Clone, Copy)]
pub struct Classes {
    pub spatial: u32,
    pub contribution: u32,
}
pub trait Host: super::ai_traversal_exclusion::Services {
    fn exclusion_world(&self) -> super::ai_traversal_exclusion::World;
    fn class18(&mut self, m: &mut Memory, interface: u32) -> Result<u32>;
    fn query40(
        &mut self,
        m: &mut Memory,
        service: u32,
        start: &mut WalkPoint,
        end: &mut [u32; 3],
        radius: u32,
        provider: &mut u32,
        pool: u32,
    ) -> Result<u32>;
    fn excluded484c80(&mut self, m: &mut Memory, interface: u32, context_owner: u32) -> Result<u32>
    where
        Self: Sized,
    {
        super::ai_traversal_exclusion::cached(
            m,
            self.exclusion_world(),
            interface,
            context_owner,
            self,
        )
    }
    fn contribution2c(
        &mut self,
        m: &mut Memory,
        interface: u32,
        context_owner: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        word_c: u32,
        cost: &mut u32,
    ) -> Result<u32>;
}
fn recycle(m: &mut Memory, pool: u32) -> Result<()> {
    let mut visited = BTreeSet::new();
    loop {
        let node = word(m, pool, 8)?;
        if node == 0 {
            break;
        }
        ensure!(visited.insert(node), "Cyclic traversal-cost result pool");
        m.write_byte(node.wrapping_add(0x14), 0)?;
        let head = word(m, pool, 8)?;
        let next = word(m, head, 0x10)?;
        let free = word(m, pool, 0x10)?;
        m.write(head.wrapping_add(0x10), free)?;
        let head = word(m, pool, 8)?;
        m.write(pool.wrapping_add(0x10), head)?;
        m.write(pool.wrapping_add(8), next)?;
    }
    m.write(pool.wrapping_add(12), 0)?;
    m.write(pool.wrapping_add(0x14), 0)
}
pub fn evaluate(
    m: &mut Memory,
    classes: Classes,
    owner: u32,
    start: &mut WalkPoint,
    end: &mut WalkPoint,
    radius: u32,
    cost: &mut u32,
    host: &mut impl Host,
) -> Result<u32> {
    let service = word(m, owner, 0x184)?;
    if service == 0 {
        return Ok(1);
    }
    let class = host.class18(m, service)?;
    if !compatible(m, class, classes.spatial)? {
        return Ok(1);
    }
    let service = word(m, owner, 0x184)?;
    let mut provider = word(m, service, 0x10)?;
    if provider == 0 || word(m, provider, 0x14)? == 0 {
        return Ok(1);
    }
    let pool = word(m, owner, 0x430)?;
    recycle(m, pool)?;
    let mut query_start = WalkPoint {
        position: start.position,
        auxiliary: [0; 16],
    };
    let pool = word(m, owner, 0x430)?;
    if host.query40(
        m,
        service,
        &mut query_start,
        &mut end.position,
        radius,
        &mut provider,
        pool,
    )? as u8
        == 0
    {
        return Ok(0);
    }
    let mut node = word(m, word(m, owner, 0x430)?, 8)?;
    let mut visited = BTreeSet::new();
    while node != 0 {
        ensure!(visited.insert(node), "Cyclic traversal-cost result chain");
        let wrapper = word(m, node, 8)?;
        if wrapper == 0 {
            return Ok(0);
        }
        let mut part = 0f32.to_bits();
        let interface = word(m, wrapper, 0)?;
        if interface != 0 {
            let class = host.class18(m, interface)?;
            if compatible(m, class, classes.contribution)? {
                let interface = word(m, wrapper, 0)?;
                let context = word(m, word(m, owner, 4)?, 0x14)?;
                if host.excluded484c80(m, interface, context)? as u8 != 0 {
                    return Ok(0);
                }
                let context = word(m, word(m, owner, 4)?, 0x14)?;
                let word_c = word(m, owner, 12)?;
                if host.contribution2c(m, interface, context, start, end, word_c, &mut part)? as u8
                    == 0
                {
                    return Ok(0);
                }
            }
        }
        *cost = (f32::from_bits(*cost) + f32::from_bits(part)).to_bits();
        node = word(m, node, 0x10)?;
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::super::{ai_edge_crossing::Globals, ai_spatial_queries as spatial};
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const OWN: u32 = 0x60000000;
    const CTX: u32 = 0x61000000;
    const PROVIDER: u32 = 0x62000000;
    const SERVICE: u32 = 0x63000000;
    const CLASS: u32 = 0x66000000;
    const PROP: u32 = 0x67000000;
    const Q: u32 = 0x68000000;
    const ROOT: u32 = 0x69000000;
    const POOL: u32 = 0x70000000;
    const NODES: u32 = 0x71000000;
    const WRAPS: u32 = 0x72000000;
    const IFACE: u32 = 0x73000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn classes() -> Classes {
        Classes {
            spatial: 0x11864,
            contribution: 0x672c4,
        }
    }
    fn payload(p: &WalkPoint) -> Value {
        json!([p.position, p.auxiliary])
    }
    struct Spy<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        published: Vec<u32>,
    }
    impl super::super::ai_traversal_exclusion::Services for Spy<'_> {
        fn class20(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected world-service lookup in cost-only fixture")
        }
    }
    impl Host for Spy<'_> {
        fn exclusion_world(&self) -> super::super::ai_traversal_exclusion::World {
            super::super::ai_traversal_exclusion::World {
                root_slot: 0x5a108,
                service_class: 0x6af44,
            }
        }
        fn class18(&mut self, m: &mut Memory, interface: u32) -> Result<u32> {
            self.trace.push(json!(["class", interface]));
            if interface == SERVICE || interface == SERVICE + 0x1000 {
                if self.case["swap_service"] == true {
                    m.write(OWN + 0x184, SERVICE + 0x1000)?;
                }
                Ok(if self.case["class_kind"] == 0 {
                    0x11864
                } else {
                    CLASS
                })
            } else {
                let index = (interface - IFACE) / 0x100;
                let kind = bits(&self.case["kinds"][index as usize]);
                if self.case["swap_wrapper"] == true {
                    m.write(WRAPS + index * 0x10, IFACE + index * 0x100 + 0x80)?;
                }
                Ok(match kind {
                    4 => CLASS + 0x1000,
                    3 => CLASS + 0x2000,
                    _ => 0x672c4,
                })
            }
        }
        fn query40(
            &mut self,
            m: &mut Memory,
            service: u32,
            s: &mut WalkPoint,
            e: &mut [u32; 3],
            radius: u32,
            provider: &mut u32,
            pool: u32,
        ) -> Result<u32> {
            self.trace.push(json!([
                "query",
                service,
                payload(s),
                e,
                radius,
                *provider,
                pool
            ]));
            for (index, kind) in self.case["kinds"].as_array().unwrap().iter().enumerate() {
                let i = index as u32;
                let node = m.word(POOL + 16)?;
                ensure!(node != 0, "Supplied spatial result pool exhausted");
                let next = m.word(node + 16)?;
                m.write(POOL + 16, next)?;
                m.write(node + 16, 0)?;
                m.write(node + 8, if *kind == 0 { 0 } else { WRAPS + i * 0x10 })?;
                m.write(
                    WRAPS + i * 0x10,
                    if *kind == 2 { 0 } else { IFACE + i * 0x100 },
                )?;
                m.write_byte(node + 20, 1)?;
                let tail = m.word(POOL + 12)?;
                if tail != 0 {
                    m.write(tail + 16, node)?;
                } else {
                    m.write(POOL + 8, node)?;
                }
                m.write(POOL + 12, node)?;
                let count = m.word(POOL + 20)?;
                m.write(POOL + 20, count + 1)?;
                self.published.push(node);
            }
            s.auxiliary[3] = 99;
            if self.case["mutate_end"] == true {
                e[1] = 8f32.to_bits();
            }
            *provider = 0x12345678;
            Ok(bits(&self.case["query_result"]))
        }
        fn excluded484c80(&mut self, m: &mut Memory, interface: u32, context: u32) -> Result<u32> {
            let index = (interface - IFACE) / 0x100;
            self.trace.push(json!(["excluded", interface, context]));
            if self.case["mutate_context"] == true {
                m.write(CTX + 0x14, 901)?;
                m.write(OWN + 12, 334)?;
            }
            Ok(if self.case["veto_at"].as_i64() == Some(index as i64) {
                bits(&self.case["veto_result"])
            } else {
                0
            })
        }
        fn contribution2c(
            &mut self,
            m: &mut Memory,
            interface: u32,
            context: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            word_c: u32,
            cost: &mut u32,
        ) -> Result<u32> {
            let index = (interface - IFACE) / 0x100;
            self.trace.push(json!([
                "contribution",
                interface,
                context,
                payload(s),
                payload(e),
                word_c,
                *cost
            ]));
            let costs = self.case["costs"].as_array().unwrap();
            *cost = bits(&costs[index as usize % costs.len()]);
            s.auxiliary[0] = 111 + index;
            e.auxiliary[1] = 222 + index;
            if self.case["truncate_at"].as_i64() == Some(index as i64) {
                m.write(self.published[index as usize] + 16, 0)?;
            }
            Ok(if self.case["fail_at"].as_i64() == Some(index as i64) {
                bits(&self.case["fail_result"])
            } else {
                1
            })
        }
    }
    // Compose the existing real49d924/radius lookup with the real4997f0;
    // only the named lower services above remain supplied boundaries.
    impl super::super::ai_spatial_walk::Host for Spy<'_> {
        fn find_cell42f8a8(&mut self, _: &mut Memory, _: u32, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected cell search in cost-only fixture")
        }
        fn trace42e46c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: &mut Option<super::super::ai_spatial_walk::Filter>,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected cell trace in cost-only fixture")
        }
    }
    impl spatial::Host for Spy<'_> {
        fn cost_classes(&self) -> Classes {
            classes()
        }
        fn owner_tag3c(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected owner tag")
        }
        fn polyline(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: &mut u32,
            _: Option<u32>,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected polyline")
        }
        fn sweep34(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected sweep")
        }
        fn direct1c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected direct query")
        }
        fn adjust20(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<()> {
            anyhow::bail!("Unexpected cost adjustment")
        }
    }
    fn memory(c: &Value) -> Result<Memory> {
        let mut m = Memory {
            regions: BTreeMap::new(),
            resources: BTreeMap::new(),
            stride: 32,
            journal: BTreeMap::new(),
        };
        for (at, size) in [
            (OWN, 0x440),
            (CTX, 0x18),
            (PROVIDER, 0x18),
            (SERVICE, 0x18),
            (SERVICE + 0x1000, 0x18),
            (CLASS, 0x18),
            (CLASS + 0x1000, 0x18),
            (CLASS + 0x2000, 0x18),
            (POOL, 0x18),
            (NODES, 0x60),
            (WRAPS, 0x40),
            (IFACE, 0x400),
            (Q, 0x80),
            (0x5a108, 4),
            (ROOT, 16),
            (0x361b4, 12),
            (900, 0x68),
            (0x6d000000, 4),
            (PROP, 8),
        ] {
            m.regions.insert(at, vec![0; size]);
        }
        m.regions.insert(NODES, vec![0xa5; 0x60]);
        for (at, v) in [
            (OWN + 4, CTX),
            (OWN + 12, 333),
            (CTX + 0x14, 900),
            (OWN + 0x184, if c["service"] == true { SERVICE } else { 0 }),
            (OWN + 0x430, POOL),
            (PROVIDER + 0x14, u32::from(c["ready"] == true)),
            (CLASS + 0x14, if c["class_kind"] == 1 { 0x11864 } else { 0 }),
            (CLASS + 0x1000 + 0x14, 0x672c4),
            (Q + 0x7c, OWN),
            (0x5a108, if c["scale"].is_null() { 0 } else { ROOT }),
            (ROOT + 12, c["scale"].as_u64().unwrap_or(0) as u32),
            (900 + 0x60, 1),
            (900 + 0x64, 0x6d000000),
            (0x6d000000, if c["property"].is_null() { 0 } else { PROP }),
            (PROP + 4, c["property"].as_u64().unwrap_or(0) as u32),
        ] {
            m.write(at, v)?;
        }
        for service in [SERVICE, SERVICE + 0x1000] {
            m.write(service, 0x65000000)?;
            m.write(
                service + 0x10,
                if c["provider"] == true { PROVIDER } else { 0 },
            )?;
        }
        let active = bits(&c["active"]);
        for (at, v) in [
            (POOL + 8, if active != 0 { NODES } else { 0 }),
            (
                POOL + 12,
                if active != 0 {
                    NODES + (active - 1) * 0x18
                } else {
                    0
                },
            ),
            (
                POOL + 16,
                if active < 4 { NODES + active * 0x18 } else { 0 },
            ),
            (POOL + 20, active),
        ] {
            m.write(at, v)?;
        }
        for i in 0..4 {
            let node = NODES + i * 0x18;
            let next = if if i < active {
                i + 1 < active
            } else {
                i + 1 < 4
            } {
                NODES + (i + 1) * 0x18
            } else {
                0
            };
            m.write(node + 8, 0xdead0000 + i)?;
            m.write(node + 16, next)?;
            m.write_byte(node + 20, u8::from(i < active))?;
            m.write(WRAPS + i * 0x10, IFACE + i * 0x100)?;
            for off in [0, 0x80] {
                m.write(IFACE + i * 0x100 + off, 0x65001000)?;
            }
        }
        Ok(m)
    }
    fn points() -> (WalkPoint, WalkPoint) {
        (
            WalkPoint {
                position: [1f32, 2., 3.].map(f32::to_bits),
                auxiliary: std::array::from_fn(|i| 100 + i as u32),
            },
            WalkPoint {
                position: [4f32, 5., 6.].map(f32::to_bits),
                auxiliary: std::array::from_fn(|i| 200 + i as u32),
            },
        )
    }
    #[test]
    fn original_traversal_cost_pool_filters_partial_writes_and_query_composition_match(
    ) -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_traversal_cost.json"
        ))?;
        for expected in data["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut m = memory(c)?;
            let (mut s, mut e) = points();
            let mut cost = bits(&c["initial"]);
            let spy = Spy {
                case: c,
                trace: vec![],
                published: vec![],
            };
            let mut native = spatial::Native {
                globals: Globals {
                    root_slot: 0x5a108,
                    intersections: 0,
                    polygon_visits: 0,
                },
                spatial_class: 0x11864,
                radius_metadata: 0x361b4,
                host: spy,
            };
            let result = if c["op"] == "query" {
                native.cost(&mut m, Q, &mut s, &mut e, &mut cost)? as u32
            } else {
                evaluate(
                    &mut m,
                    classes(),
                    OWN,
                    &mut s,
                    &mut e,
                    bits(&c["radius"]),
                    &mut cost,
                    &mut native.host,
                )?
            };
            let wrappers = (0..4)
                .map(|i| m.word(WRAPS + i * 0x10))
                .collect::<Result<Vec<_>>>()?;
            let actual = json!({"result":result,"trace":native.host.trace,"cost":cost,"first":payload(&s),"last":payload(&e),
                "pool":m.regions[&POOL],"nodes":m.regions[&NODES],"wrappers":wrappers,"service":m.word(OWN+0x184)?,"context":m.word(CTX+0x14)?,"word_c":m.word(OWN+12)?});
            for key in actual.as_object().unwrap().keys() {
                assert_eq!(actual[key], expected[key], "{key}: {c}");
            }
        }
        Ok(())
    }
    #[test]
    fn corrupt_cost_pool_is_an_error_not_successful_clearance() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_traversal_cost.json"
        ))?;
        let c = &data["cases"][5]["input"];
        let mut m = memory(c)?;
        m.write(POOL + 8, NODES)?;
        m.write(NODES + 16, NODES)?;
        let (mut s, mut e) = points();
        let mut spy = Spy {
            case: c,
            trace: vec![],
            published: vec![],
        };
        assert!(evaluate(&mut m, classes(), OWN, &mut s, &mut e, 0, &mut 0, &mut spy).is_err());
        Ok(())
    }
}
