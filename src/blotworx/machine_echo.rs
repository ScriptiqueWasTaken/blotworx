//! Machine echo update80219318..80219390 and bound-state request8021646c.
//! The graph owner supplies lifecycle notices and commits topology afterwards.
use super::{
    generator_plan::Prepared,
    machine_runtime::{Notice, Runtime},
    transition_select::{Context as SelectionContext, Selection},
};
use anyhow::{ensure, Context, Result};
impl Prepared {
    pub(super) fn service_machine_echo(
        &mut self,
        id: &str,
        mut notify: impl FnMut(&Runtime, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        let runtime = self.machines.get_mut(id).context("Echo machine missing")?;
        if !runtime.echo_next_update {
            return Ok(false);
        }
        let target = self.state.select_start_state(
            id,
            &mut runtime.next_start_index,
            &mut self.random_seed,
        )?;
        let current = runtime.current;
        let mode = self.state.machines[id].settings.self_transition_mode;
        let changed = if target != current && mode != 0 {
            self.request_bound_start(id, target, mode == 2, &mut notify)?
        } else {
            false
        };
        self.machines.get_mut(id).unwrap().echo_next_update = false;
        Ok(changed)
    }
    fn request_bound_start(
        &mut self,
        id: &str,
        target: i32,
        force_fallback: bool,
        notify: &mut impl FnMut(&Runtime, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        let running = self
            .transition_stacks
            .get(id)
            .and_then(|stack| stack.records.last())
            .map(|record| &record.transition);
        let allowed = self
            .dispatchers
            .entry(id.into())
            .or_default()
            .allows_selection(running);
        if allowed {
            let definition = &self.state.machines[id];
            let runtime = &self.machines[id];
            ensure!(
                definition
                    .states
                    .iter()
                    .all(|state| state.info.variable_binding_set.is_none()),
                "State-enable bindings require native bound-state arbitration"
            );
            let state = &definition.states[definition.state_indices[&runtime.current]];
            ensure!(
                !state
                    .transitions
                    .iter()
                    .chain(&definition.wildcard_transitions)
                    .any(|transition| transition.to_state_id == target
                        && transition.flags & 4096 != 0),
                "Nested-source bound-state selection is not implemented for {id}"
            );
            let context = SelectionContext {
                event: -1,
                current_state: runtime.current,
                previous_state: runtime.previous,
                nested_state: None,
                target_filter: Some(target),
                ignore_event: true,
            };
            let enabled = definition
                .states
                .iter()
                .map(|state| state.info.enable)
                .collect::<Vec<_>>();
            let (local, wildcard) = (
                runtime.local_intervals.clone(),
                runtime.wildcard_intervals.clone(),
            );
            let mut selection = Selection::default();
            self.state.select_transitions(
                id,
                &context,
                &enabled,
                &local,
                &wildcard,
                &mut selection,
            )?;
            if let Some(candidate) = selection.candidates.first() {
                let definition = &self.state.machines[id];
                let runtime = &self.machines[id];
                let table = if candidate.wildcard {
                    &definition.wildcard_transitions
                } else {
                    &definition.states[definition.state_indices[&runtime.current]].transitions
                };
                let transition = table[candidate.index].clone();
                //80216740 executes the selected effect with forced initiation;
                // do not enqueue a second ordinary-event request here.
                self.start_transition(id, &transition, false, notify)?;
                return Ok(true);
            }
        }
        if !force_fallback {
            return Ok(false);
        }
        if let Some(last) = self
            .transition_stacks
            .get(id)
            .and_then(|stack| stack.records.len().checked_sub(1))
        {
            self.retire_transitions(id, last, &mut *notify)?;
        }
        self.dispatchers
            .entry(id.into())
            .or_default()
            .pending
            .clear();
        let definition = &self.state.machines[id];
        let sync = definition.settings.sync_variable_index;
        self.machines
            .get_mut(id)
            .unwrap()
            .enter(definition, target, false, notify)?;
        if sync != -1 {
            self.state.set_integer(sync as usize, target)?;
        }
        self.transitions.topology_changed = true;
        Ok(true)
    }
}
