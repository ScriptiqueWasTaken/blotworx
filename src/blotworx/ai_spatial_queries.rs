//! Concrete query forwarding49b9b0/49bc44/49d628/49d924/49d9e8.
//! Spatial service implementations are mandatory; absence is not clear space.
use super::{
    ai_edge_crossing::{Acceptance, Globals},
    ai_graph_link_passes::Memory,
    ai_path_construction::Queries,
    ai_segment_query::{Context, WalkPoint},
};
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
fn word(m: &Memory, p: u32, o: u32) -> Result<u32> {
    m.word(p.wrapping_add(o))
}
fn owner(m: &Memory, q: u32) -> Result<u32> {
    word(m, q, 0x7c)
}
fn context_owner(m: &Memory, q: u32) -> Result<u32> {
    word(m, word(m, owner(m, q)?, 4)?, 0x14)
}
fn point(p: [u32; 3]) -> WalkPoint {
    WalkPoint {
        position: p,
        auxiliary: [0; 16],
    }
}

///45f994's indexed (flag1) branch, with signed index/count comparisons.
/// Metadata and table are runtime data references, never executable pointers.
pub fn indexed_property(m: &Memory, owner: u32, metadata: u32) -> Result<u32> {
    let index = word(m, metadata, 8)? as i32;
    if index < 0 || index >= word(m, owner, 0x60)? as i32 {
        return Ok(0);
    }
    word(m, word(m, owner, 0x64)?, (index as u32).wrapping_mul(4))
}
pub trait Host: super::ai_traversal_cost::Host + super::ai_spatial_walk::Host {
    fn cost_classes(&self) -> super::ai_traversal_cost::Classes;
    fn owner_tag3c(&mut self, m: &mut Memory, owner: u32) -> Result<u32>;
    ///43168c: the remaining pointer arguments are null in this caller.
    fn polyline(
        &mut self,
        m: &mut Memory,
        provider: u32,
        points: u32,
        count: u32,
        radius: u32,
        output: &mut u32,
        service_value: Option<u32>,
    ) -> Result<u32>
    where
        Self: Sized,
    {
        let mut filter = service_value.map(super::ai_spatial_walk::Filter::Value);
        super::ai_spatial_walk::polyline(
            m,
            provider,
            points,
            count,
            radius,
            None,
            None,
            Some(output),
            &mut filter,
            0,
            self,
        )
    }
    fn sweep34(
        &mut self,
        m: &mut Memory,
        service: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        flags: u32,
        radius: u32,
    ) -> Result<u32>;
    fn cost4997f0(
        &mut self,
        m: &mut Memory,
        owner: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        radius: u32,
        cost: &mut u32,
    ) -> Result<u32>
    where
        Self: Sized,
    {
        let classes = self.cost_classes();
        super::ai_traversal_cost::evaluate(m, classes, owner, start, end, radius, cost, self)
    }
    fn direct1c(
        &mut self,
        m: &mut Memory,
        service: u32,
        context_owner: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        tag: u32,
        cost: &mut u32,
    ) -> Result<u32>;
    fn adjust20(
        &mut self,
        m: &mut Memory,
        service: u32,
        context_owner: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        tag: u32,
        cost: &mut u32,
    ) -> Result<()>;
}
pub struct Native<H> {
    pub globals: Globals,
    pub spatial_class: u32,
    pub radius_metadata: u32,
    pub host: H,
}
impl<H: Host> Native<H> {
    fn radius(&mut self, m: &mut Memory, q: u32, default: f32) -> Result<u32> {
        let property = indexed_property(m, context_owner(m, q)?, self.radius_metadata)?;
        let value = if property != 0 {
            0.5 * f32::from_bits(word(m, property, 4)?)
        } else {
            default * self.globals.scale(m)?.unwrap_or(1.)
        };
        Ok(value.to_bits())
    }
    fn compatible(&mut self, m: &mut Memory, q: u32) -> Result<bool> {
        let service = word(m, owner(m, q)?, 0x184)?;
        let mut class = self.host.class18(m, service)?;
        let mut visited = BTreeSet::new();
        loop {
            if class == self.spatial_class {
                return Ok(true);
            }
            ensure!(
                visited.insert(class),
                "Cyclic native spatial-service ancestry"
            );
            class = word(m, class, 0x14)?;
            if class == 0 {
                return Ok(false);
            }
        }
    }
    pub fn filter(
        &mut self,
        m: &mut Memory,
        q: u32,
        points: u32,
        count: u32,
        output: &mut u32,
    ) -> Result<u8> {
        let provider = word(m, owner(m, q)?, 0x50)?;
        if provider == 0 || word(m, provider, 0x4c)? == 0 {
            return Ok(0);
        }
        let radius = self.radius(m, q, 0.3)?;
        let service_value = if self.compatible(m, q)? {
            Some(word(m, word(m, owner(m, q)?, 0x184)?, 12)?)
        } else {
            None
        };
        // Property and class callbacks can replace the owner or provider.
        let provider = word(m, word(m, owner(m, q)?, 0x50)?, 0x4c)?;
        Ok(self
            .host
            .polyline(m, provider, points, count, radius, output, service_value)? as u8)
    }
    pub fn accept(&mut self, m: &mut Memory, q: u32, start: [u32; 3], end: [u32; 3]) -> Result<u8> {
        self.accept_points(m, q, &mut point(start), &mut point(end))
    }
    pub fn accept_points(
        &mut self,
        m: &mut Memory,
        q: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
    ) -> Result<u8> {
        if !self.compatible(m, q)? {
            return Ok(0);
        }
        let service = word(m, owner(m, q)?, 0x184)?;
        let radius = self.radius(m, q, 0.3)?;
        Ok(self.host.sweep34(m, service, start, end, 4, radius)? as u8)
    }
    pub fn direct(
        &mut self,
        m: &mut Memory,
        q: u32,
        start: [u32; 3],
        end: [u32; 3],
        cost: &mut u32,
    ) -> Result<u8> {
        let owner = owner(m, q)?;
        let service = word(m, owner, 0x1e8)?;
        if service == 0 {
            return Ok(0);
        }
        let mut start = point(start);
        let mut end = point(end);
        let tag = self.host.owner_tag3c(m, owner)?;
        let context = context_owner(m, q)?;
        Ok(self
            .host
            .direct1c(m, service, context, &mut start, &mut end, tag, cost)? as u8)
    }
    pub fn cost(
        &mut self,
        m: &mut Memory,
        q: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        cost: &mut u32,
    ) -> Result<u8> {
        let radius = self.radius(m, q, 0.5)?;
        Ok(self
            .host
            .cost4997f0(m, owner(m, q)?, start, end, radius, cost)? as u8)
    }
    pub fn adjust(
        &mut self,
        m: &mut Memory,
        q: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        cost: &mut u32,
    ) -> Result<()> {
        let owner = owner(m, q)?;
        let service = word(m, owner, 0x1ac)?;
        let tag = self.host.owner_tag3c(m, owner)?;
        let context = context_owner(m, q)?;
        self.host
            .adjust20(m, service, context, start, end, tag, cost)
    }
}
impl<H: Host> Acceptance for Native<H> {
    fn accept(&mut self, c: &mut Context<'_, '_>, q: u32, s: [u32; 3], e: [u32; 3]) -> Result<u8> {
        Native::accept(self, c.graph.tables, q, s, e)
    }
}
impl<H: Host> Queries for Native<H> {
    fn filter(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        p: u32,
        n: u32,
        o: &mut u32,
    ) -> Result<u8> {
        Native::filter(self, c.graph.tables, q, p, n, o)
    }
    fn cost70(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        s: &mut WalkPoint,
        e: &mut WalkPoint,
        o: &mut u32,
    ) -> Result<u8> {
        self.cost(c.graph.tables, q, s, e, o)
    }
    fn cost74(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        s: &mut WalkPoint,
        e: &mut WalkPoint,
        o: &mut u32,
    ) -> Result<()> {
        self.adjust(c.graph.tables, q, s, e, o)
    }
    fn direct60(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        s: [u32; 3],
        e: [u32; 3],
        o: &mut u32,
    ) -> Result<u8> {
        self.direct(c.graph.tables, q, s, e, o)
    }
}

/// Outer walking and invalidation are still concrete required services.
pub trait Traversal: Host + super::ai_region_geometry::Children {
    fn invalidate(
        &mut self,
        r: &mut super::ai_actor_queries::Runtime,
        g: &mut super::ai_astar_graph_services::GraphContext<'_, Memory>,
        i: &mut super::ai_traversal_instances::Instance,
        f: super::ai_actor_queries::Filter,
    ) -> Result<u8>;
    fn walk(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        p: &mut WalkPoint,
        e: [u32; 3],
        i: &mut u32,
        o: &mut [u32; 2],
    ) -> Result<u8>;
}
impl<H: Host + super::ai_region_geometry::Children> super::ai_region_geometry::Children
    for Native<H>
{
    fn contains(&mut self, m: &Memory, child: u32, p: [u32; 3], flag: u32) -> Result<u8> {
        self.host.contains(m, child, p, flag)
    }
}
impl<H: Traversal> super::ai_path_construction::Bindings for Native<H> {
    fn invalidate(
        &mut self,
        r: &mut super::ai_actor_queries::Runtime,
        g: &mut super::ai_astar_graph_services::GraphContext<'_, Memory>,
        i: &mut super::ai_traversal_instances::Instance,
        f: super::ai_actor_queries::Filter,
    ) -> Result<u8> {
        self.host.invalidate(r, g, i, f)
    }
    fn walk(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        p: &mut WalkPoint,
        e: [u32; 3],
        i: &mut u32,
        o: &mut [u32; 2],
    ) -> Result<u8> {
        self.host.walk(c, q, p, e, i, o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const Q: u32 = 0x68000000;
    const OWN: u32 = 0x60000000;
    const CTX: u32 = 0x61000000;
    const PROVIDER: u32 = 0x62000000;
    const SERVICE: u32 = 0x63000000;
    const CLASS: u32 = 0x66000000;
    const PROP: u32 = 0x67000000;
    const ROOT: u32 = 0x69000000;
    const POINTS: u32 = 0x6a000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn payload(p: &WalkPoint) -> Value {
        json!([p.position, p.auxiliary])
    }
    struct Spy<'a> {
        case: &'a Value,
        trace: Vec<Value>,
    }
    impl Spy<'_> {
        fn swap(&self, m: &mut Memory, stage: &str) -> Result<()> {
            if self.case["swap"] == stage {
                m.write(Q + 0x7c, OWN + 0x1000)?;
            }
            Ok(())
        }
        fn mutate(&self, s: &mut WalkPoint, e: &mut WalkPoint, c: Option<&mut u32>) {
            s.auxiliary[0] = 111;
            e.auxiliary[1] = 222;
            if let Some(c) = c {
                *c = 42f32.to_bits();
            }
        }
        fn result(&self) -> u32 {
            bits(&self.case["result"])
        }
    }
    impl super::super::ai_spatial_walk::Host for Spy<'_> {
        fn find_cell42f8a8(&mut self, _: &mut Memory, _: u32, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected cell search in forwarding-only fixture")
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
            anyhow::bail!("Unexpected cell trace in forwarding-only fixture")
        }
    }
    impl super::super::ai_traversal_exclusion::Services for Spy<'_> {
        fn class20(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected world-service lookup in forwarding-only fixture")
        }
    }
    impl super::super::ai_traversal_cost::Host for Spy<'_> {
        fn exclusion_world(&self) -> super::super::ai_traversal_exclusion::World {
            super::super::ai_traversal_exclusion::World {
                root_slot: 0x5a108,
                service_class: 0x6af44,
            }
        }
        fn class18(&mut self, m: &mut Memory, s: u32) -> Result<u32> {
            self.trace.push(json!(["class", s]));
            self.swap(m, "class")?;
            Ok(if self.case["class_kind"] == 0 {
                0x11864
            } else {
                CLASS
            })
        }
        fn query40(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: &mut WalkPoint,
            _: &mut [u32; 3],
            _: u32,
            _: &mut u32,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected traversal query in forwarding-only fixture")
        }
        fn excluded484c80(&mut self, _: &mut Memory, _: u32, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected exclusion in forwarding-only fixture")
        }
        fn contribution2c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected contribution in forwarding-only fixture")
        }
    }
    impl Host for Spy<'_> {
        fn cost_classes(&self) -> super::super::ai_traversal_cost::Classes {
            super::super::ai_traversal_cost::Classes {
                spatial: 0x11864,
                contribution: 0x672c4,
            }
        }
        fn owner_tag3c(&mut self, m: &mut Memory, o: u32) -> Result<u32> {
            self.trace.push(json!(["tag", o]));
            self.swap(m, "tag")?;
            Ok(777)
        }
        fn polyline(
            &mut self,
            m: &mut Memory,
            provider: u32,
            p: u32,
            n: u32,
            radius: u32,
            o: &mut u32,
            v: Option<u32>,
        ) -> Result<u32> {
            self.trace
                .push(json!(["polyline", provider, p, n, radius, *o, v]));
            *o = n - 1;
            m.write(p, 13f32.to_bits())?;
            Ok(self.result())
        }
        fn sweep34(
            &mut self,
            _: &mut Memory,
            service: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            flags: u32,
            radius: u32,
        ) -> Result<u32> {
            self.trace.push(json!([
                "sweep",
                service,
                payload(s),
                payload(e),
                flags,
                radius
            ]));
            self.mutate(s, e, None);
            Ok(self.result())
        }
        fn cost4997f0(
            &mut self,
            _: &mut Memory,
            owner: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            radius: u32,
            c: &mut u32,
        ) -> Result<u32> {
            self.trace
                .push(json!(["cost", owner, payload(s), payload(e), radius, *c]));
            self.mutate(s, e, Some(c));
            Ok(self.result())
        }
        fn direct1c(
            &mut self,
            _: &mut Memory,
            service: u32,
            owner: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            tag: u32,
            c: &mut u32,
        ) -> Result<u32> {
            self.trace.push(json!([
                "direct",
                service,
                owner,
                payload(s),
                payload(e),
                tag,
                *c
            ]));
            self.mutate(s, e, Some(c));
            Ok(self.result())
        }
        fn adjust20(
            &mut self,
            _: &mut Memory,
            service: u32,
            owner: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            tag: u32,
            c: &mut u32,
        ) -> Result<()> {
            self.trace.push(json!([
                "adjust",
                service,
                owner,
                payload(s),
                payload(e),
                tag,
                *c
            ]));
            self.mutate(s, e, Some(c));
            Ok(())
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
            (Q, 0x80),
            (0x5a108, 4),
            (0x361b4, 12),
            (ROOT, 16),
            (PROP, 8),
            (CLASS, 0x18),
            (POINTS, 36),
        ] {
            m.regions.insert(at, vec![0; size]);
        }
        m.write(Q + 0x7c, OWN)?;
        m.write(0x5a108, if c["scale"].is_null() { 0 } else { ROOT })?;
        m.write(ROOT + 12, c["scale"].as_u64().unwrap_or(0) as u32)?;
        m.write(PROP + 4, c["property"].as_u64().unwrap_or(0) as u32)?;
        m.write(
            0x361b4 + 8,
            c["property_index"].as_i64().unwrap_or(0) as u32,
        )?;
        m.write(CLASS + 0x14, if c["class_kind"] == 1 { 0x11864 } else { 0 })?;
        for i in 0..2u32 {
            let off = i * 0x1000;
            for at in [
                OWN + off,
                CTX + off,
                PROVIDER + off,
                SERVICE + off,
                0x6c000000 + off,
                0x6d000000 + off,
            ] {
                m.regions.insert(at, vec![0; 0x300]);
            }
            for (at, value) in [
                (OWN + off + 4, CTX + off),
                (
                    OWN + off + 0x50,
                    if c["provider"] == true {
                        PROVIDER + off
                    } else {
                        0
                    },
                ),
                (OWN + off + 0x184, SERVICE + off),
                (OWN + off + 0x1ac, SERVICE + off + 0x100),
                (
                    OWN + off + 0x1e8,
                    if c["direct"] == true {
                        SERVICE + off + 0x200
                    } else {
                        0
                    },
                ),
                (CTX + off + 0x14, 0x6c000000 + off),
                (
                    0x6c000000 + off + 0x60,
                    c["property_count"].as_i64().unwrap_or(2) as u32,
                ),
                (0x6c000000 + off + 0x64, 0x6d000000 + off),
                (
                    PROVIDER + off + 0x4c,
                    if c["resource"] == true { 1000 + i } else { 0 },
                ),
                (SERVICE + off + 12, 123 + i),
            ] {
                m.write(at, value)?;
            }
            for j in 0..2 {
                m.write(
                    0x6d000000 + off + j * 4,
                    if c["property"].is_null() { 0 } else { PROP },
                )?;
            }
        }
        for i in 0..9 {
            m.write(POINTS + i * 4, (i as f32).to_bits())?;
        }
        Ok(m)
    }
    #[test]
    fn original_spatial_query_dispatch_radius_metadata_and_callback_reloads_match() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_spatial_queries.json"
        ))?;
        for expected in data["cases"].as_array().unwrap() {
            let case = &expected["input"];
            let mut m = memory(case)?;
            let mut native = Native {
                globals: Globals {
                    root_slot: 0x5a108,
                    intersections: 0,
                    polygon_visits: 0,
                },
                spatial_class: 0x11864,
                radius_metadata: 0x361b4,
                host: Spy {
                    case,
                    trace: vec![],
                },
            };
            let mut first = WalkPoint {
                position: [1f32, 2., 3.].map(f32::to_bits),
                auxiliary: std::array::from_fn(|i| 100 + i as u32),
            };
            let mut last = WalkPoint {
                position: [4f32, 5., 6.].map(f32::to_bits),
                auxiliary: std::array::from_fn(|i| 200 + i as u32),
            };
            let mut cost = 9f32.to_bits();
            let result = match case["op"].as_str().unwrap() {
                "filter" => native.filter(&mut m, Q, POINTS, 3, &mut cost)?,
                "accept" => native.accept_points(&mut m, Q, &mut first, &mut last)?,
                "direct" => native.direct(&mut m, Q, first.position, last.position, &mut cost)?,
                "cost" => native.cost(&mut m, Q, &mut first, &mut last, &mut cost)?,
                "adjust" => {
                    native.adjust(&mut m, Q, &mut first, &mut last, &mut cost)?;
                    0
                }
                _ => unreachable!(),
            };
            let actual = json!({"result":result,"trace":native.host.trace,"cost":cost,"first":payload(&first),"last":payload(&last),"point_word":m.word(POINTS)?,"owner":m.word(Q+0x7c)?});
            for key in actual.as_object().unwrap().keys() {
                assert_eq!(actual[key], expected[key], "{key}: {case}");
            }
        }
        Ok(())
    }
    #[test]
    fn missing_service_storage_and_cyclic_metadata_fail_explicitly() -> Result<()> {
        let c = json!({"scale":1f32.to_bits(),"property":null,"provider":true,"resource":true,"direct":true,"class_kind":2,"swap":"none","result":1});
        let mut m = memory(&c)?;
        m.write(CLASS + 0x14, CLASS)?;
        let mut n = Native {
            globals: Globals {
                root_slot: 0x5a108,
                intersections: 0,
                polygon_visits: 0,
            },
            spatial_class: 0x11864,
            radius_metadata: 0x361b4,
            host: Spy {
                case: &c,
                trace: vec![],
            },
        };
        assert!(n.accept(&mut m, Q, [0; 3], [0; 3]).is_err());
        m.write(Q + 0x7c, 0)?;
        assert!(n.filter(&mut m, Q, POINTS, 3, &mut 0).is_err());
        Ok(())
    }
}
