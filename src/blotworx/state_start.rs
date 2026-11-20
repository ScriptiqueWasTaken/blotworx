//! State-machine start selection, DOL 0x8021b648 and 0x8021b974.
//! Call after variable bindings. This does not activate generators or dispatch
//! state notifications; those belong to the surrounding graph lifecycle.
use super::{behavior::Document, state_machine::Machine};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub fn bindings(graph: &Document, types: &[u8]) -> Result<BTreeMap<String, usize>> {
    let mut result = BTreeMap::new();
    for (id, object) in &graph.objects {
        if !matches!(
            object.klass.as_str(),
            "hkbStateMachine" | "hkbStateMachineStateInfo"
        ) {
            continue;
        }
        let Some(reference) = object.fields["variableBindingSet"]["ref"].as_str() else {
            continue;
        };
        let binding = graph
            .objects
            .get(reference)
            .context("Missing machine binding set")?;
        ensure!(
            binding.klass == "hkbVariableBindingSet",
            "Invalid machine binding set"
        );
        for b in binding.fields["bindings"]
            .as_array()
            .context("Missing machine bindings")?
        {
            ensure!(
                object.klass == "hkbStateMachine"
                    && b["memberPath"] == "startStateId"
                    && b["bindingType"] == 0
                    && b["bitIndex"] == -1,
                "Unsupported state-machine binding on {id}: {b}"
            );
            let variable = b["variableIndex"]
                .as_u64()
                .context("Invalid start-state variable")? as usize;
            ensure!(
                types.get(variable) == Some(&3),
                "Start-state binding requires int32 variable"
            );
            // Multiple writes to the same member retain authored order: last wins.
            result.insert(id.clone(), variable);
        }
    }
    Ok(result)
}

fn index(machine: &Machine, id: i32) -> Option<usize> {
    // Retail's zero-ID fast path only checks the first state.
    if id == 0 {
        machine
            .states
            .first()
            .filter(|s| s.info.state_id == 0)
            .map(|_| 0)
    } else {
        machine.state_indices.get(&id).copied()
    }
}

/// Probability selection updates the graph's shared RNG even for one candidate.
pub(super) fn random_index(probabilities: &[f32], seed: &mut u32) -> usize {
    let sum = probabilities.iter().fold(0_f32, |sum, &p| sum + p);
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let sample = (*seed as f32) * (1. / 4_294_967_296.);
    let mut cumulative = 0_f32;
    for (i, p) in probabilities
        .iter()
        .enumerate()
        .take(probabilities.len() - 1)
    {
        cumulative += p / sum;
        if sample <= cumulative {
            return i;
        }
    }
    probabilities.len() - 1
}

pub fn select(
    machine: &Machine,
    bound_start: i32,
    sync_value: Option<i32>,
    next_start_index: &mut Option<usize>,
    enabled: &[bool],
    probabilities: &[f32],
    random_seed: &mut u32,
) -> Result<i32> {
    ensure!(
        !machine.states.is_empty()
            && enabled.len() == machine.states.len()
            && probabilities.len() == machine.states.len(),
        "Invalid start-state tables"
    );
    ensure!(
        probabilities.iter().all(|p| p.is_finite() && *p >= 0.),
        "Invalid state probabilities"
    );
    let desired = if let Some(i) = *next_start_index {
        let id = machine
            .states
            .get(i)
            .context("Start-state override index out of bounds")?
            .info
            .state_id;
        *next_start_index = None;
        id
    } else {
        match machine.settings.start_state_mode {
            0 => bound_start,
            1 | 2 => sync_value
                .filter(|&id| index(machine, id).is_some())
                .unwrap_or(bound_start),
            3 => {
                let candidates: Vec<_> = enabled
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &e)| e.then_some(i))
                    .collect();
                if candidates.is_empty() {
                    machine.states[0].info.state_id
                } else {
                    let probabilities: Vec<_> =
                        candidates.iter().map(|&i| probabilities[i]).collect();
                    machine.states[candidates[random_index(&probabilities, random_seed)]]
                        .info
                        .state_id
                }
            }
            mode => anyhow::bail!("Unknown start-state mode {mode}"),
        }
    };
    Ok(if index(machine, desired).is_some_and(|i| enabled[i]) {
        desired
    } else {
        machine.states[enabled.iter().position(|&e| e).unwrap_or(0)]
            .info
            .state_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rng_uses_original_lcg_and_inclusive_probability_boundary() {
        let mut seed = 0;
        assert_eq!(random_index(&[0., 1.], &mut seed), 1);
        assert_eq!(seed, 1_013_904_223);
        let sample = (seed as f32) * (1. / 4_294_967_296.);
        seed = 0;
        assert_eq!(random_index(&[sample, 1. - sample], &mut seed), 0);
        seed = 0;
        assert_eq!(random_index(&[0., 0.], &mut seed), 1);
        seed = 0;
        assert_eq!(random_index(&[1.], &mut seed), 0);
        assert_eq!(seed, 1_013_904_223);
    }
}
