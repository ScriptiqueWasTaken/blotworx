//!4a88e4 resource binding and steering pool setup. Actual global services and
//! exact entity-attribute queries are required; no automatic World publication.
use super::ai_goto_gap::{Cache24, Cache34, Pool, State};
use anyhow::{ensure, Context, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Profiles,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribute {
    Diameter,
    Length,
    Height,
}
pub trait Host {
    /// Must query the current global context on each call.
    fn world_service(&mut self, state: &mut State, kind: Service) -> Result<Option<usize>>;
    ///45f994(entity,metadata,1): exact registered-slot lookup, no creation.
    fn entity_attribute(
        &mut self,
        state: &mut State,
        entity: usize,
        kind: Attribute,
    ) -> Result<Option<usize>>;
    fn cache34(&mut self, _: &mut State, capacity: u32) -> Result<Option<Pool<Cache34>>> {
        Ok(Some(Pool::new(capacity, Cache34::node)?))
    }
    fn cache24(&mut self, _: &mut State, capacity: u32) -> Result<Option<Pool<Cache24>>> {
        Ok(Some(Pool::new(capacity, Cache24::node)?))
    }
}
pub fn initialize(state: &mut State, host: &mut impl Host) -> Result<bool> {
    state.managers_4c_50[0] = Some(host.world_service(state, Service::Profiles)?);
    state.managers_4c_50[1] = Some(host.world_service(state, Service::Gap)?);
    if state.managers_4c_50[1].flatten().is_none() {
        return Ok(false);
    }
    for (kind, index) in [
        (Attribute::Diameter, 1),
        (Attribute::Length, 2),
        (Attribute::Height, 0),
    ] {
        let entity = state.entity_58.context("Steering setup has no entity")?;
        state.attributes_68_6c_70[index] = Some(host.entity_attribute(state, entity, kind)?);
    }
    let width = state
        .scalar_48
        .context("Steering half-width is uninitialized")?;
    let spacing = state.scalars_10_20[3].context("Steering candidate spacing is uninitialized")?;
    let ratio = (2. * width) / spacing;
    ensure!(
        ratio.is_finite() && (-2147483648.0..2147483648.0).contains(&(ratio as f64)),
        "Steering capacity requires unverified native fctiwz handling for {ratio}"
    );
    let capacity = (ratio as i32).wrapping_add(1) as u32;
    let pool = host.cache34(state, capacity)?;
    state.pool_d4 = pool;
    let pool = host.cache24(state, 36)?;
    state.pool_118 = pool;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Construction;
    impl super::super::ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(None)
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            Ok(20)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected profiler")
        }
    }
    struct Binding<'a> {
        case: &'a Value,
        context: usize,
        trace: Vec<Value>,
    }
    impl Host for Binding<'_> {
        fn world_service(&mut self, _: &mut State, kind: Service) -> Result<Option<usize>> {
            let name = if kind == Service::Profiles {
                "profiles"
            } else {
                "gap"
            };
            self.trace.push(json!(["service", name, self.context]));
            if kind == Service::Profiles && self.case["context_change"] == true {
                self.context = 2;
            }
            Ok(
                (self.case[name] != false).then_some(if kind == Service::Profiles {
                    80
                } else {
                    90
                }),
            )
        }
        fn entity_attribute(
            &mut self,
            s: &mut State,
            entity: usize,
            kind: Attribute,
        ) -> Result<Option<usize>> {
            let (name, index) = match kind {
                Attribute::Diameter => ("diameter", 0),
                Attribute::Length => ("length", 1),
                Attribute::Height => ("height", 2),
            };
            self.trace.push(json!(["attribute", name, entity]));
            if self.case["entity_change"] == true {
                s.entity_58 = Some(21);
            }
            Ok(
                (self.case["attributes"].as_u64().unwrap_or(7) & (1 << index) != 0)
                    .then_some(100 + index),
            )
        }
        fn cache34(&mut self, s: &mut State, capacity: u32) -> Result<Option<Pool<Cache34>>> {
            self.trace
                .push(json!(["pool", 34, capacity, s.pool_d4.is_some()]));
            if self.case["fail34"] == true {
                return Ok(None);
            }
            Ok(Some(Pool::new(capacity, Cache34::node)?))
        }
        fn cache24(&mut self, s: &mut State, capacity: u32) -> Result<Option<Pool<Cache24>>> {
            self.trace
                .push(json!(["pool", 24, capacity, s.pool_d4.is_some()]));
            if self.case["fail24"] == true {
                return Ok(None);
            }
            Ok(Some(Pool::new(capacity, Cache24::node)?))
        }
    }
    fn snapshot<T>(pool: &Pool<T>, payload: impl Fn(&T) -> Value) -> Value {
        json!({"active_head":pool.active_head,"free_head":pool.free_head,"tail":pool.tail,"count":pool.count,
            "nodes":pool.nodes.iter().map(|n|json!({"payload":payload(&n.payload),"previous":n.previous,"next":n.next,"active":n.active})).collect::<Vec<_>>()})
    }
    #[test]
    fn navigation_goto_setup_matches_original_services_attributes_capacity_and_pool_publication(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_gap_setup.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut s = State::construct(60, 50, b"IGoto", &mut Construction)?;
            s.scalar_48 = Some(f32::from_bits(c["width"].as_u64().unwrap() as u32));
            s.scalars_10_20[3] = Some(f32::from_bits(c["spacing"].as_u64().unwrap() as u32));
            if c["old"] == true {
                s.managers_4c_50 = [Some(Some(70)), Some(Some(71))];
                s.attributes_68_6c_70 = [Some(Some(120)), Some(Some(121)), Some(Some(122))];
            }
            let mut host = Binding {
                case: c,
                context: 1,
                trace: vec![],
            };
            let result = initialize(&mut s, &mut host)?;
            assert!(s.managers_4c_50.iter().all(Option::is_some));
            if result {
                assert!(s.attributes_68_6c_70.iter().all(Option::is_some));
            } else if c["old"] != true {
                assert!(s.attributes_68_6c_70.iter().all(Option::is_none));
            }
            let actual_pools = [
                s.pool_d4
                    .as_ref()
                    .map(|p| snapshot(p, |n| json!(n.words_00_24))),
                s.pool_118
                    .as_ref()
                    .map(|p| snapshot(p, |n| json!(n.words_00_14))),
            ];
            let expected_pools = expected["pools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    id.as_u64()
                        .map(|i| fixture["pool_images"][i as usize].clone())
                })
                .collect::<Vec<_>>();
            assert_eq!(
                json!({"result":result,"trace":host.trace,"managers":s.managers_4c_50.map(Option::flatten),
                "attributes":s.attributes_68_6c_70.map(Option::flatten),"entity":s.entity_58,"pools":actual_pools}),
                json!({"result":expected["result"],"trace":expected["trace"],"managers":expected["managers"],"attributes":expected["attributes"],"entity":expected["entity"],"pools":expected_pools}),
                "{c}"
            );
        }
        for (width, spacing) in [(f32::NAN, 1.), (1., 0.), (f32::MAX, f32::MIN_POSITIVE)] {
            let mut s = State::construct(60, 50, b"IGoto", &mut Construction)?;
            s.scalar_48 = Some(width);
            s.scalars_10_20[3] = Some(spacing);
            let case = json!({});
            let mut host = Binding {
                case: &case,
                context: 1,
                trace: vec![],
            };
            let error = initialize(&mut s, &mut host).unwrap_err();
            assert!(error.to_string().contains("unverified native fctiwz"));
            assert!(s.pool_d4.is_none() && s.pool_118.is_none());
        }
        Ok(())
    }
}
