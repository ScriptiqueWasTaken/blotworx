//! Complete CGoto_GapDynamicAvoidance_v2 attribute handler4a8d54.
//! Numeric conversion and exact entity-attribute queries are required boundaries.
use super::{
    ai_goto_gap::{self, State},
    ai_traversal_registry::names_equal,
};
use anyhow::{Context, Result};

pub trait Host: ai_goto_gap::Host {
    /// Retained string handle: later callbacks may alter its contents.
    fn name(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn value(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn text(&mut self, state: &mut State, value: usize) -> Result<Vec<u8>>;
    fn number(&mut self, state: &mut State, value: Option<usize>) -> Result<f64>;
    fn integer(&mut self, state: &mut State, value: Option<usize>) -> Result<i32>;
    ///45f994(entity,CEntityDiameter,1): exact registered-slot lookup, no creation.
    fn diameter_attribute(&mut self, state: &mut State, entity: usize) -> Result<Option<usize>>;
    fn diameter_value(&mut self, state: &mut State, attribute: usize) -> Result<f32>;
}
fn parse(state: &mut State, node: usize, scaled: bool, host: &mut impl Host) -> Result<f32> {
    // Context is queried before the value/parser callbacks.
    let scale = if scaled {
        host.unit_scale(state)?.unwrap_or(1.)
    } else {
        1.
    };
    let value = host.value(state, node)?;
    let result = host.number(state, value)? as f32;
    Ok(if scaled { result * scale } else { result })
}
fn diameter(state: &mut State, host: &mut impl Host) -> Result<Option<f32>> {
    if let Some(entity) = state.entity_58 {
        if host.diameter_attribute(state, entity)?.is_some() {
            let entity = state
                .entity_58
                .context("Steering owner disappeared during diameter lookup")?;
            let value = host
                .diameter_attribute(state, entity)?
                .context("Steering diameter disappeared on its second query")?;
            return Ok(Some(host.diameter_value(state, value)?));
        }
    }
    Ok(None)
}
fn boolean(state: &mut State, node: usize, host: &mut impl Host) -> Result<Option<bool>> {
    for (word, result) in [
        (b"yes".as_slice(), true),
        (b"true", true),
        (b"1", true),
        (b"no", false),
        (b"false", false),
        (b"0", false),
    ] {
        let value = host
            .value(state, node)?
            .context("Steering boolean has no value")?;
        if names_equal(&host.text(state, value)?, word) {
            return Ok(Some(result));
        }
    }
    Ok(None)
}
fn tag(state: &mut State, name: usize, expected: &[u8], host: &mut impl Host) -> Result<bool> {
    Ok(names_equal(&host.text(state, name)?, expected))
}
pub fn initialize(state: &mut State, node: usize, host: &mut impl Host) -> Result<bool> {
    let name = host.name(state, node)?;
    let Some(name) = name else { return Ok(false) };
    for (tag, field) in [
        (b"EntityDistMax".as_slice(), 0),
        (b"DiagramMaxLength", 2),
        (b"DiagramHalfWidth", 4),
    ] {
        if self::tag(state, name, tag, host)? {
            let value = parse(state, node, true, host)?;
            // Native unordered comparisons deliberately differ from value>0:
            // NaN proceeds to the diameter check and write.
            if !(value <= 0.) {
                if let Some(diameter) = diameter(state, host)? {
                    let extent = if field == 0 { value } else { 2. * value };
                    if extent <= diameter {
                        return Ok(false);
                    }
                }
                if field == 4 {
                    state.scalar_48 = Some(value);
                    state.inverse_100 = Some(1. / value);
                } else {
                    state.scalars_10_20[field] = Some(value);
                }
            }
            return Ok(true);
        }
    }
    if tag(state, name, b"DiagramRefreshPeriod", host)? {
        let value = parse(state, node, false, host)?;
        if value >= 0. {
            state.scalars_10_20[1] = Some(value);
        }
        return Ok(true);
    }
    if tag(state, name, b"CandidateSpacing", host)? {
        let value = parse(state, node, true, host)?;
        if value > 0. {
            state.scalars_10_20[3] = Some(value);
        }
        return Ok(true);
    }
    if tag(state, name, b"ExtraGap", host)? {
        let value = parse(state, node, true, host)?;
        if value >= 0. {
            state.scalars_10_20[4] = Some(value);
        }
        return Ok(true);
    }
    // Unlike the numeric tags, these two compare a fresh name query.
    let current = host
        .name(state, node)?
        .context("Steering name disappeared before ContinuousSpeed comparison")?;
    if tag(state, current, b"ContinuousSpeed", host)? {
        let Some(value) = boolean(state, node, host)? else {
            return Ok(false);
        };
        state.flag_24 = Some(u8::from(value));
        return Ok(true);
    }
    if tag(state, name, b"MinSpeed", host)? {
        let value = parse(state, node, true, host)?;
        if value > 0. {
            state.scalars_28_38[0] = Some(value);
        }
        return Ok(true);
    }
    if tag(state, name, b"Courtesy", host)? {
        let value = parse(state, node, false, host)?;
        if value >= 0. && value <= 1. {
            state.scalars_28_38[1] = Some(value);
        }
        return Ok(true);
    }
    for (tag, index) in [
        (b"QueueingDelay".as_slice(), 2),
        (b"PushingDelay", 3),
        (b"StuckDelay", 4),
    ] {
        if self::tag(state, name, tag, host)? {
            let value = parse(state, node, false, host)?;
            if value >= 0. {
                state.scalars_28_38[index] = Some(value);
            }
            return Ok(true);
        }
    }
    if tag(state, name, b"MaxCollisionTestsPerFrame", host)? {
        let value = host.value(state, node)?;
        let value = host.integer(state, value)?;
        if value >= 0 {
            state.word_3c = Some(value as u32);
        }
        return Ok(true);
    }
    if tag(state, name, b"MaxAngularSpeed", host)? {
        let value = parse(state, node, false, host)?;
        if value > 0. {
            state.scalar_40 = Some(value);
        }
        return Ok(true);
    }
    let current = host
        .name(state, node)?
        .context("Steering name disappeared before CrowdMode comparison")?;
    if tag(state, current, b"CrowdMode", host)? {
        let Some(value) = boolean(state, node, host)? else {
            return Ok(false);
        };
        state.flag_44 = Some(u8::from(value));
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Constructor;
    impl ai_goto_gap::Host for Constructor {
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
            anyhow::bail!("Unexpected constructor profiler")
        }
    }
    struct Binding<'a> {
        case: &'a Value,
        names: usize,
        values: usize,
        queries: usize,
        strings: std::collections::BTreeMap<usize, Vec<u8>>,
        trace: Vec<Value>,
    }
    impl Binding<'_> {
        fn next(&self, key: &str, plural: &str, index: usize) -> Option<String> {
            let value = if let Some(values) = self.case[plural].as_array() {
                &values[index.min(values.len() - 1)]
            } else {
                &self.case[key]
            };
            value.as_str().map(str::to_string)
        }
    }
    impl ai_goto_gap::Host for Binding<'_> {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            let scale = (self.case["world"] != false)
                .then(|| self.case["scale"].as_u64().unwrap_or(0x3f800000) as u32);
            self.trace.push(json!(["scale", scale]));
            Ok(scale.map(f32::from_bits))
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            anyhow::bail!("Unexpected constructor owner query")
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            anyhow::bail!("Unexpected profiler query")
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected budget query")
        }
    }
    impl Host for Binding<'_> {
        fn name(&mut self, _: &mut State, _: usize) -> Result<Option<usize>> {
            let name = self.next("name", "names", self.names);
            let handle = if self.case["alias_name"] == true {
                1000
            } else {
                1000 + self.names
            };
            self.names += 1;
            self.trace.push(json!(["name", name]));
            Ok(name.map(|n| {
                self.strings.insert(handle, n.into_bytes());
                handle
            }))
        }
        fn value(&mut self, _: &mut State, _: usize) -> Result<Option<usize>> {
            let value = self.next("value", "values", self.values);
            self.values += 1;
            self.trace.push(json!(["value", value]));
            Ok(value.map(|v| {
                self.strings.insert(100, v.into_bytes());
                100
            }))
        }
        fn text(&mut self, _: &mut State, handle: usize) -> Result<Vec<u8>> {
            self.strings
                .get(&handle)
                .cloned()
                .context("String handle missing")
        }
        fn number(&mut self, s: &mut State, value: Option<usize>) -> Result<f64> {
            let text = value.map(|h| {
                String::from_utf8(self.strings[&h].clone())
                    .unwrap()
                    .split('\0')
                    .next()
                    .unwrap()
                    .to_string()
            });
            self.trace.push(json!(["number", text]));
            if self.case["mutation"] == true {
                s.scalars_10_20[0] = Some(123.);
            }
            Ok(f64::from_bits(self.case["number"].as_u64().unwrap_or(0)))
        }
        fn integer(&mut self, _: &mut State, value: Option<usize>) -> Result<i32> {
            let text = value.map(|h| {
                String::from_utf8(self.strings[&h].clone())
                    .unwrap()
                    .split('\0')
                    .next()
                    .unwrap()
                    .to_string()
            });
            self.trace.push(json!(["integer", text]));
            Ok(self.case["integer"].as_i64().unwrap_or(0) as i32)
        }
        fn diameter_attribute(&mut self, s: &mut State, entity: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["diameter_query", entity]));
            let handle = 50 + self.queries;
            self.queries += 1;
            if self.case["owner_change"] == true {
                s.entity_58 = Some(21);
            }
            Ok((self.case["attribute"] != false).then_some(handle))
        }
        fn diameter_value(&mut self, _: &mut State, handle: usize) -> Result<f32> {
            self.trace.push(json!(["diameter_value", handle]));
            Ok(f32::from_bits(
                self.case["diameter"].as_u64().unwrap_or(0x3f333333) as u32,
            ))
        }
    }
    #[test]
    fn navigation_goto_gap_attributes_match_original_all_options_limits_nan_and_retained_names(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_gap_attributes.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut state = State::construct(60, 50, b"IGoto", &mut Constructor)?;
            state.flag_24 = Some(3);
            state.flag_44 = Some(3);
            state.entity_58 = (c["owner"] != false).then_some(20);
            let mut host = Binding {
                case: c,
                names: 0,
                values: 0,
                queries: 0,
                strings: Default::default(),
                trace: vec![],
            };
            let result = initialize(&mut state, 1, &mut host)?;
            let mut fields = state
                .scalars_10_20
                .iter()
                .chain(state.scalars_28_38.iter())
                .map(|v| v.unwrap().to_bits())
                .collect::<Vec<_>>();
            fields.extend([
                state.word_3c.unwrap(),
                state.scalar_40.unwrap().to_bits(),
                state.scalar_48.unwrap().to_bits(),
                state.inverse_100.unwrap().to_bits(),
            ]);
            let mut expected_fields: Vec<u32> =
                serde_json::from_value(fixture["initial_fields"].clone())?;
            for (index, value) in
                serde_json::from_value::<Vec<(usize, u32)>>(expected["field_overrides"].clone())?
            {
                expected_fields[index] = value;
            }
            assert_eq!(
                json!({"result":result,"trace":host.trace,"fields":fields,
                "flags":[state.flag_24.unwrap(),state.flag_44.unwrap()],"entity":state.entity_58}),
                json!({"result":expected["result"],"trace":expected["trace"],"fields":expected_fields,"flags":expected["flags"],"entity":expected["entity"]}),
                "{c}"
            );
        }
        Ok(())
    }
}
