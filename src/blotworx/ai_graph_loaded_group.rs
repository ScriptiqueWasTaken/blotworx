//! Native482848 loading/token gate driving the canonical owned file buffers.
//! Direct allocations transfer once; readiness/linking remains later stages.
use super::{
    ai_graph_buffers::Bindings,
    ai_graph_handles::State,
    ai_graph_link_passes::Memory,
    ai_graph_loaded_buffer::Allocation,
    ai_graph_resource_group::{Group, Inputs},
};
use anyhow::{ensure, Context as _, Result};
use std::collections::BTreeMap;
impl Group {
    pub fn normalize_loaded(
        &self,
        state: &mut State,
        inputs: &mut Inputs,
        bindings: &Bindings,
        pending: &mut BTreeMap<usize, Allocation>,
        memory: &mut Memory,
    ) -> Result<()> {
        self.normalize_with(state, inputs, |id, input| {
            ensure!(
                input.source.is_empty() && input.normalized.is_none(),
                "Graph resource has competing conversion buffer owners"
            );
            let binding = bindings
                .resources
                .get(&id)
                .context("Loaded graph binding is absent")?;
            let allocation = binding
                .payload
                .allocation
                .context("Loaded graph allocation identity is absent")?;
            let owner = pending
                .remove(&allocation)
                .context("Completed graph allocation is absent")?;
            memory.bind_loaded_allocation(id, owner, binding, input.layout)?;
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::super::ai_graph_buffers::{Binding, Pointer};
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn original_group_state_and_token_gates_drive_owned_buffers_once() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_resource_group.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            if c["op"] != "normalize" {
                continue;
            }
            let group: Group = serde_json::from_value(c["group"].clone())?;
            let mut state: State = serde_json::from_value(c["state"].clone())?;
            let mut inputs: Inputs = serde_json::from_value(c["inputs"].clone())?;
            let mut bindings = Bindings::default();
            let mut pending = BTreeMap::new();
            let mut m = Memory::default();
            for (&id, input) in &mut inputs.resources {
                let identity = id as usize * 16 + 1;
                let mut bytes = vec![0xcd; 16];
                bytes.extend(std::mem::take(&mut input.source));
                pending.insert(
                    identity,
                    Allocation {
                        identity,
                        base: 0x70000000 + id * 0x10000,
                        bytes,
                    },
                );
                let mut b = Binding {
                    payload: Pointer {
                        allocation: Some(identity),
                        offset: 16,
                    },
                    header: Pointer::default(),
                    points: Pointer::default(),
                };
                b.bind();
                bindings.resources.insert(id, b);
            }
            group.normalize_loaded(&mut state, &mut inputs, &bindings, &mut pending, &mut m)?;
            assert_eq!(
                serde_json::to_value(&state)?,
                case["state"],
                "loaded group state {index}"
            );
            for (&id, input) in &inputs.resources {
                let identity = id as usize * 16 + 1;
                let expected = &case["inputs"]["resources"][id.to_string()]["normalized"];
                assert!(input.source.is_empty() && input.normalized.is_none());
                if expected.is_null() {
                    assert!(pending.contains_key(&identity));
                    assert!(!m.resources.contains_key(&id));
                } else {
                    assert!(!pending.contains_key(&identity));
                    let base = 0x70000000 + id * 0x10000;
                    let payload: Vec<u8> = serde_json::from_value(expected["bytes"].clone())?;
                    assert_eq!(
                        &m.regions[&base][16..],
                        payload.as_slice(),
                        "loaded group bytes {index}/{id}"
                    );
                    assert_eq!(&m.regions[&base][..16], &[0xcd; 16]);
                    let view = m.view(id)?;
                    assert_eq!(view.header, base + 16);
                    let mut auxiliary = expected["field38"].clone();
                    if auxiliary["kind"] == "tail" {
                        auxiliary["value"] =
                            json!(base + 16 + auxiliary["value"].as_u64().unwrap() as u32);
                    }
                    assert_eq!(
                        serde_json::to_value(view.auxiliary.unwrap())?,
                        auxiliary,
                        "loaded auxiliary {index}/{id}"
                    );
                }
            }
            let before = m.resources.len();
            group.normalize_loaded(&mut state, &mut inputs, &bindings, &mut pending, &mut m)?;
            assert_eq!(m.resources.len(), before, "Converted a buffer twice");
        }
        Ok(())
    }
}
