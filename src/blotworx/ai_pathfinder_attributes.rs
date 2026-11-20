//!492440 scalar/resource attributes. Modifier tables are a required separate
//!handler, not accepted as no-ops.48fec4 initialization remains mandatory.
use super::{ai_pathfinder_constructor::State, ai_traversal_registry::names_equal};
use anyhow::{Context, Result};

pub trait Host {
    fn name(&mut self, state: &mut State, node: usize) -> Result<Option<Vec<u8>>>;
    fn value(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn text(&mut self, state: &mut State, value: usize) -> Result<Vec<u8>>;
    fn integer(&mut self, state: &mut State, value: Option<usize>) -> Result<i32>;
    fn number(&mut self, state: &mut State, value: usize) -> Result<f64>;
    fn unit_scale(&mut self, state: &mut State) -> Result<Option<f32>>;
    fn find_database(
        &mut self,
        state: &mut State,
        manager: usize,
        value: usize,
    ) -> Result<Option<usize>>;
    ///4754ec registers489520 and this service as its callback context.
    fn register_database_callback(&mut self, state: &mut State, database: usize) -> Result<()>;
    /// Mandatory continuation492b14..4957f8 for modifier groups/instances.
    /// Return None only for an unrecognized tag, so ForceSmoothOnPathChange
    /// can be examined afterward. An unsupported recognized modifier is an error.
    fn modifier_attribute(&mut self, state: &mut State, node: usize) -> Result<Option<bool>>;
}

fn tag(state: &mut State, node: usize, expected: &[u8], host: &mut impl Host) -> Result<bool> {
    let name = host
        .name(state, node)?
        .context("Pathfinder attribute has no name")?;
    Ok(names_equal(&name, expected))
}
fn boolean(state: &mut State, node: usize, host: &mut impl Host) -> Result<Option<bool>> {
    // Native comparisons retrieve the value again each time; do not snapshot
    // a value before callbacks or collapse the six comparisons into one read.
    for (word, value) in [
        (b"yes".as_slice(), true),
        (b"true", true),
        (b"1", true),
        (b"no", false),
        (b"false", false),
        (b"0", false),
    ] {
        let handle = host
            .value(state, node)?
            .context("Pathfinder boolean attribute has no value")?;
        if names_equal(&host.text(state, handle)?, word) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
pub fn initialize(state: &mut State, node: usize, host: &mut impl Host) -> Result<bool> {
    let name = host
        .name(state, node)?
        .context("Pathfinder attribute has no name")?;
    if name.first() == Some(&b'_') {
        return Ok(true);
    }
    // LPF is initialized by48fec4 before this pass. These two authored flags
    // intentionally have no writes in the original492440 handler.
    if tag(state, node, b"LpfModule", host)? {
        return Ok(true);
    }
    if tag(state, node, b"MaxConsideredSubGraphsForAStar", host)? {
        if let Some(value) = host.value(state, node)? {
            state.word_38 = (host.integer(state, Some(value))? as u32).clamp(1, 4096);
        }
        return Ok(true);
    }
    if tag(state, node, b"EnhancePrecision", host)? {
        let Some(value) = boolean(state, node, host)? else {
            return Ok(false);
        };
        // False is accepted without clearing an existing true value.
        if value {
            state.precision_cc = Some(1);
        }
        return Ok(true);
    }
    for (name, minimum, maximum, field) in [
        (b"SolverBufferLength".as_slice(), 8, 2048, 0),
        (
            b"SearchNodeUIDFromPositionContextSize".as_slice(),
            1024,
            262144,
            1,
        ),
    ] {
        if tag(state, node, name, host)? {
            if let Some(value) = host.value(state, node)? {
                let value = (host.integer(state, Some(value))? as u32).clamp(minimum, maximum);
                if field == 0 {
                    state.word_30 = value;
                } else {
                    state.words_90_94_98[1] = value;
                }
            }
            return Ok(true);
        }
    }
    if tag(
        state,
        node,
        b"SearchNodeUIDFromPositionMaxTypeCheckedCandidates",
        host,
    )? {
        if let Some(value) = host.value(state, node)? {
            state.words_90_94_98[2] = host.integer(state, Some(value))? as u32;
        }
        return Ok(true);
    }
    for (name, field, callback) in [
        (b"Database".as_slice(), 2, true),
        (b"Database_AIMesh".as_slice(), 3, false),
    ] {
        if tag(state, node, name, host)? {
            if let Some(value) = host.value(state, node)? {
                let Some(manager) = state.references_3c_4c_50_54_58_60_6c[1] else {
                    return Ok(false);
                };
                let database = host.find_database(state, manager, value)?;
                state.references_3c_4c_50_54_58_60_6c[field] = database;
                let Some(database) = database else {
                    return Ok(false);
                };
                if callback {
                    host.register_database_callback(state, database)?;
                    state.flags_9c_9d[0] = 1;
                }
            }
            return Ok(true);
        }
    }
    if tag(state, node, b"DataMode", host)? {
        let Some(value) = host.value(state, node)? else {
            return Ok(false);
        };
        if names_equal(&host.text(state, value)?, b"Flat") {
            state.mode_28 = Some(1);
            return Ok(true);
        }
        if names_equal(&host.text(state, value)?, b"Hierarchical") {
            state.mode_28 = Some(2);
            return Ok(true);
        }
        return Ok(false);
    }
    for name in [
        b"PathObjectsGetCostCanBlock".as_slice(),
        b"ConstraintsGetCostCanBlock",
    ] {
        if tag(state, node, name, host)? {
            return Ok(true);
        }
    }
    for (name, index) in [
        (b"MaxEdgeLength".as_slice(), 0),
        (b"FlatDataModeSearchRadius".as_slice(), 1),
    ] {
        if tag(state, node, name, host)? {
            let Some(value) = host.value(state, node)? else {
                return Ok(false);
            };
            let scale = host.unit_scale(state)?.unwrap_or(1.);
            let result = scale * (host.number(state, value)? as f32);
            if result >= 0. {
                state.distances_2c_34[index] = Some(result);
            }
            return Ok(true);
        }
    }
    if tag(state, node, b"AvoidCPUPeaks", host)? {
        let Some(value) = boolean(state, node, host)? else {
            return Ok(false);
        };
        if value {
            state.flag_41c = 1;
        }
        return Ok(true);
    }
    if tag(state, node, b"CpuPrudence", host)? {
        let Some(value) = host.value(state, node)? else {
            return Ok(false);
        };
        let result = host.number(state, value)? as f32;
        if result >= 0. && result <= 1. {
            state.scalar_428 = result;
        }
        return Ok(true);
    }
    if tag(state, node, b"MaxPathObjectPerRetrieval", host)? {
        let value = host.value(state, node)?;
        let result = host.integer(state, value)?;
        if result > 0 {
            state.word_42c = Some(result as u32);
        }
        return Ok(true);
    }
    if let Some(result) = host.modifier_attribute(state, node)? {
        return Ok(result);
    }
    if tag(state, node, b"ForceSmoothOnPathChange", host)? {
        let Some(value) = boolean(state, node, host)? else {
            return Ok(false);
        };
        state.base.flag_20 = u8::from(value);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ai_navigation_action::{Class, Registry},
        ai_navigation_construction::{ActionClass, ActionFactory, Constructed, Layout},
        ai_pathfinder_constructor::{Base, BaseHost, Counter, Service},
    };
    use super::*;
    use serde_json::{json, Value};
    struct Construction;
    impl BaseHost for Construction {
        fn unit_scale(&mut self, _: &mut Base) -> Result<Option<f32>> {
            Ok(None)
        }
        fn profiling_enabled(&mut self, _: &mut Base) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut Base, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected profiler registration")
        }
    }
    impl super::super::ai_pathfinder_constructor::Host for Construction {
        fn world_service(&mut self, _: &mut State, _: Service) -> Result<Option<usize>> {
            Ok(None)
        }
        fn point_attribute_index(&mut self, _: &mut State) -> Result<i32> {
            Ok(0)
        }
    }
    fn state() -> Result<State> {
        let registry = || Registry {
            count: 2,
            heading: Class {
                index: 0,
                constructor: true,
            },
            speed: Class {
                index: 1,
                constructor: true,
            },
        };
        let owner = Constructed::new(
            20,
            Some(10),
            Some(30),
            &ActionClass {
                metadata: 90,
                factory: Some(ActionFactory::BaseAi),
            },
            &Layout {
                agent_classes: 3,
                service_classes: 4,
                attributes: registry(),
            },
        )?;
        State::construct(
            50,
            &owner,
            40,
            &registry(),
            &mut Counter::default(),
            &mut Construction,
        )
    }
    struct Binding<'a> {
        case: &'a Value,
        values: usize,
        trace: Vec<Value>,
    }
    impl Binding<'_> {
        fn value_text(&self, index: usize) -> Option<String> {
            let values = &self.case["values"];
            if let Some(values) = values.as_array() {
                values[index.min(values.len() - 1)]
                    .as_str()
                    .map(str::to_string)
            } else {
                self.case["value"].as_str().map(str::to_string)
            }
        }
    }
    impl Host for Binding<'_> {
        fn name(&mut self, _: &mut State, _: usize) -> Result<Option<Vec<u8>>> {
            Ok(Some(
                self.case["name"].as_str().unwrap().as_bytes().to_vec(),
            ))
        }
        fn value(&mut self, _: &mut State, _: usize) -> Result<Option<usize>> {
            let index = self.values;
            self.values += 1;
            let value = self.value_text(index);
            self.trace.push(json!(["value", value]));
            Ok(value.map(|_| index + 1))
        }
        fn text(&mut self, _: &mut State, value: usize) -> Result<Vec<u8>> {
            Ok(self.value_text(value - 1).unwrap().into_bytes())
        }
        fn integer(&mut self, _: &mut State, value: Option<usize>) -> Result<i32> {
            let value = value
                .and_then(|v| self.value_text(v - 1))
                .map(|v| v.split('\0').next().unwrap().to_string());
            self.trace.push(json!(["integer", value]));
            Ok(self.case["integer"].as_i64().unwrap_or(0) as i32)
        }
        fn number(&mut self, _: &mut State, value: usize) -> Result<f64> {
            self.trace.push(json!([
                "number",
                self.value_text(value - 1)
                    .unwrap()
                    .split('\0')
                    .next()
                    .unwrap()
            ]));
            Ok(f64::from_bits(self.case["number"].as_u64().unwrap_or(0)))
        }
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok((self.case["world"] != false)
                .then(|| f32::from_bits(self.case["scale"].as_u64().unwrap_or(0x3f800000) as u32)))
        }
        fn find_database(
            &mut self,
            _: &mut State,
            manager: usize,
            value: usize,
        ) -> Result<Option<usize>> {
            self.trace.push(json!([
                "database",
                manager,
                self.value_text(value - 1)
                    .unwrap()
                    .split('\0')
                    .next()
                    .unwrap()
            ]));
            Ok((self.case["found"] != false).then_some(90))
        }
        fn register_database_callback(&mut self, _: &mut State, database: usize) -> Result<()> {
            self.trace.push(json!(["register", database]));
            Ok(())
        }
        fn modifier_attribute(&mut self, _: &mut State, _: usize) -> Result<Option<bool>> {
            let name = self.case["name"].as_str().unwrap();
            if name.starts_with("Modifiers_") || name == "IGoto" {
                anyhow::bail!("Modifier service is not supplied by this scalar/resource test")
            }
            Ok(None)
        }
    }
    #[test]
    fn navigation_pathfinder_attributes_match_original_scalar_resources_and_boolean_writes(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_pathfinder_attributes.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut s = state()?;
            s.mode_28 = Some(7);
            s.distances_2c_34 = [Some(5.), Some(3.)];
            s.word_30 = 64;
            s.word_38 = 32;
            s.words_90_94_98[1] = 1024;
            s.words_90_94_98[2] = 19;
            s.scalar_428 = 1.;
            s.word_42c = Some(256);
            s.base.flag_20 = 3;
            s.precision_cc = Some(3);
            s.flag_41c = 3;
            s.references_3c_4c_50_54_58_60_6c[1] = (c["manager"] != false).then_some(80);
            s.references_3c_4c_50_54_58_60_6c[2] = Some(90);
            s.references_3c_4c_50_54_58_60_6c[3] = Some(90);
            let mut host = Binding {
                case: c,
                values: 0,
                trace: vec![],
            };
            let result = initialize(&mut s, 1, &mut host)?;
            let fields = [
                s.mode_28.unwrap(),
                s.distances_2c_34[0].unwrap().to_bits(),
                s.word_30,
                s.distances_2c_34[1].unwrap().to_bits(),
                s.word_38,
                s.words_90_94_98[1],
                s.words_90_94_98[2],
                s.scalar_428.to_bits(),
                s.word_42c.unwrap(),
            ];
            assert_eq!(
                json!({"result":result,"trace":host.trace,"fields":fields,
                "flags":[s.base.flag_20,s.precision_cc.unwrap(),s.flag_41c,s.flags_9c_9d[0]],
                "databases":[s.references_3c_4c_50_54_58_60_6c[2],s.references_3c_4c_50_54_58_60_6c[3]]}),
                json!({"result":expected["result"],"trace":expected["trace"],"fields":expected["fields"],"flags":expected["flags"],"databases":expected["databases"]}),
                "{c}"
            );
        }
        let c = json!({"name":"Modifiers_IGoto"});
        assert!(initialize(
            &mut state()?,
            1,
            &mut Binding {
                case: &c,
                values: 0,
                trace: vec![]
            }
        )
        .is_err());
        Ok(())
    }
}
