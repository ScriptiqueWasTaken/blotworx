//! Retained expression-modifier activation state for a character graph.
//! Update ordering and event scope are supplied by the active-node scheduler.
use super::{
    behavior::Document, behavior_state::State, state_machine::Event, variable_bindings::Scalar,
};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Instances {
    active: BTreeSet<String>,
    scratch: Vec<usize>,
}
impl Instances {
    /// Even a zero-timestep frame evaluates expressions. Enable bindings are
    /// read before lifecycle checks, as in the retail active-node update pass.
    fn synchronize_activation(
        &mut self,
        graph: &Document,
        state: &mut State,
        id: &str,
    ) -> Result<bool> {
        let enabled = match state.bindings.read(id, "enable", state.words(), None)? {
            Some(Scalar::Boolean(value)) => value,
            Some(_) => anyhow::bail!("Expression enable binding is not boolean"),
            None => graph
                .objects
                .get(id)
                .context("Missing expression modifier")?
                .fields["enable"]
                .as_bool()
                .context("Missing expression enable flag")?,
        };
        state
            .expression_modifiers
            .get_mut(id)
            .context("Missing compiled expression modifier")?
            .enabled = enabled;
        if !enabled {
            self.active.remove(id);
            return Ok(false);
        }
        if !self.active.contains(id) {
            state.activate_expression_modifier(id)?;
            self.active.insert(id.into());
        }
        Ok(true)
    }
    /// Modifier event wrapper 0x80211e40 performs enable/activation checks.
    /// Expression virtual +0x20 is 0x801ed7c8 (blr), not evaluate/update.
    pub fn handle_event(&mut self, graph: &Document, state: &mut State, id: &str) -> Result<()> {
        self.synchronize_activation(graph, state, id)?;
        Ok(())
    }
    pub fn update(
        &mut self,
        graph: &Document,
        state: &mut State,
        id: &str,
        discard_events: bool,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.update_inner(graph, state, id, discard_events, events, None)
    }
    pub fn update_with_random(
        &mut self,
        graph: &Document,
        state: &mut State,
        id: &str,
        discard_events: bool,
        events: &mut Vec<Event>,
        random: &mut u32,
    ) -> Result<()> {
        self.update_inner(graph, state, id, discard_events, events, Some(random))
    }
    fn update_inner(
        &mut self,
        graph: &Document,
        state: &mut State,
        id: &str,
        discard_events: bool,
        events: &mut Vec<Event>,
        random: Option<&mut u32>,
    ) -> Result<()> {
        if !self.synchronize_activation(graph, state, id)? {
            return Ok(());
        }
        self.scratch.clear();
        if let Some(random) = random {
            state.evaluate_expression_modifier_with_random(id, &mut self.scratch, random)?;
        } else {
            state.evaluate_expression_modifier(id, &mut self.scratch)?;
        }
        // Discarding delivery must not suppress assignment or event-history
        // updates. A once-only event remains sent after a discarded frame.
        if !discard_events {
            for &id in &self.scratch {
                events.push(Event {
                    id: id.try_into()?,
                    payload: None,
                });
            }
        }
        Ok(())
    }
    pub fn deactivate(&mut self, id: &str) {
        self.active.remove(id);
    }
    pub fn is_active(&self, id: &str) -> bool {
        self.active.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn startup_modifier_writes_character_state_and_retains_activation() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        let mut instances = Instances::default();
        let mut events = vec![];
        let index = state.index("InternalVar_InCombatIdle")?;
        instances.update(&graph, &mut state, "2:6240", false, &mut events)?;
        assert!(instances.is_active("2:6240"));
        assert_eq!(state.words()[index], 1);
        assert!(events.is_empty());
        instances.update(&graph, &mut state, "2:6240", true, &mut events)?;
        assert!(events.is_empty());
        instances.deactivate("2:6240");
        assert!(!instances.is_active("2:6240"));
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn discarded_events_update_history_and_reactivation_resets_it() -> Result<()> {
        let mut graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let variables: Vec<String> = serde_json::from_value(strings["variableNames"].clone())?;
        let names: Vec<String> = serde_json::from_value(strings["eventNames"].clone())?;
        // Synthetic event fixture isolates lifecycle from authored event gates.
        state
            .expression_modifiers
            .get_mut("2:6240")
            .unwrap()
            .statements = vec![super::super::behavior_expression::Statement::compile(
            &super::super::behavior_expression::Authored {
                expression: "1".into(),
                assignment_variable_index: -1,
                assignment_event_index: 0,
                event_mode: 0,
            },
            &variables,
            &names,
        )?];
        let mut instances = Instances::default();
        let mut events = vec![];
        instances.update(&graph, &mut state, "2:6240", true, &mut events)?;
        instances.update(&graph, &mut state, "2:6240", false, &mut events)?;
        assert!(events.is_empty());
        graph.objects.get_mut("2:6240").unwrap().fields["enable"] = false.into();
        instances.update(&graph, &mut state, "2:6240", false, &mut events)?;
        assert!(!instances.is_active("2:6240"));
        graph.objects.get_mut("2:6240").unwrap().fields["enable"] = true.into();
        instances.update(&graph, &mut state, "2:6240", false, &mut events)?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, 0);
        instances.update(&graph, &mut state, "2:6240", false, &mut events)?;
        assert_eq!(events.len(), 1);
        Ok(())
    }
}
