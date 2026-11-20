//! Group selectors482744/4827b8 and conversion gate482848.
//! Source payload selection, streaming, linking and ready-state admission are separate.
use super::{
    ai_graph_handles::State,
    ai_graph_normalization::{convert, Layout, Payload},
};
use anyhow::{Context as _, Result};
use std::collections::BTreeMap;
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Input {
    pub word24: u32,
    pub layout: Layout,
    pub source: Vec<u8>,
    pub normalized: Option<Payload>,
}
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct Inputs {
    pub resources: BTreeMap<u32, Input>,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub resources: Vec<u32>,
}
impl Group {
    pub fn first_state6(&self, state: &State) -> Result<Option<u32>> {
        for &id in &self.resources {
            if state
                .resources
                .get(&id)
                .context("Graph group resource is absent")?
                .state
                == 6
            {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
    pub fn first_state3_unprioritized(
        &self,
        state: &State,
        index: &mut u32,
    ) -> Result<Option<u32>> {
        for (slot, &id) in self.resources.iter().enumerate() {
            let resource = state
                .resources
                .get(&id)
                .context("Graph group resource is absent")?;
            if resource.state == 3 && resource.priority != 1 {
                *index = u32::try_from(slot).context("Graph group index exceeds native domain")?;
                return Ok(Some(id));
            }
        }
        // Native does not overwrite the caller's index on a miss.
        Ok(None)
    }
    /// Shared482848 gate. Concrete conversion may keep its buffer on Inputs
    /// (legacy adapters) or transfer it to the canonical graph arena.
    pub fn normalize_with(
        &self,
        state: &mut State,
        inputs: &mut Inputs,
        mut convert: impl FnMut(u32, &mut Input) -> Result<()>,
    ) -> Result<()> {
        for &id in &self.resources {
            if state
                .resources
                .get(&id)
                .context("Graph group resource is absent")?
                .state
                != 2
            {
                continue;
            }
            let input = inputs
                .resources
                .get_mut(&id)
                .context("Graph conversion input metadata is absent")?;
            if input.word24 == 0 {
                continue;
            }
            state.resources.get_mut(&id).unwrap().state = 3;
            convert(id, input)?;
        }
        Ok(())
    }
    pub fn normalize(&self, state: &mut State, inputs: &mut Inputs) -> Result<()> {
        self.normalize_with(state, inputs, |_, input| {
            let normalized = convert(&input.source, input.layout)?;
            input.normalized = Some(normalized);
            input.source = Vec::new();
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn original_group_selectors_and_concrete_conversion_gate_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_resource_group.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let group: Group = serde_json::from_value(c["group"].clone())?;
            let mut state: State = serde_json::from_value(c["state"].clone())?;
            let mut inputs: Inputs = serde_json::from_value(c["inputs"].clone())?;
            let mut index = 99;
            let result = match c["op"].as_str().unwrap() {
                "state6" => group.first_state6(&state)?,
                "state3" => group.first_state3_unprioritized(&state, &mut index)?,
                "normalize" => {
                    group.normalize(&mut state, &mut inputs)?;
                    None
                }
                _ => unreachable!(),
            };
            assert_eq!(json!(result), case["result"], "{c}");
            assert_eq!(json!(index), case["index"], "{c}");
            assert_eq!(serde_json::to_value(&state)?, case["state"], "{c}");
            assert_eq!(serde_json::to_value(&inputs)?, case["inputs"], "{c}");
            if c["op"] == "normalize" {
                let first = serde_json::to_value(&inputs)?;
                group.normalize(&mut state, &mut inputs)?;
                assert_eq!(
                    serde_json::to_value(inputs)?,
                    first,
                    "Conversion must not replay in state3"
                );
            }
        }
        Ok(())
    }
}
