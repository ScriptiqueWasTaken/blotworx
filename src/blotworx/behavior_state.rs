//! Mutable graph variables, separate from the immutable cooked graph.
use super::{
    behavior::Document,
    expression::Program,
    locomotion_animation::{Bindings, Output},
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub struct State {
    pub bindings: super::variable_bindings::Bindings,
    pub clips: BTreeMap<String, super::clip_runtime::Clip>,
    pub transition_effects: BTreeMap<String, super::transition_effect::Definition>,
    pub machines: BTreeMap<String, super::state_machine::Machine>,
    pub expression_modifiers: BTreeMap<String, super::behavior_expression::Modifier>,
    pub attribute_modifiers: BTreeMap<String, super::animation_attributes::Modifier>,
    pub attribute_defaults: Vec<f32>,
    names: BTreeMap<String, usize>,
    types: Vec<u8>,
    words: Vec<u32>,
    initial_words: Vec<u32>,
    quads: Vec<[f32; 4]>,
    initial_quads: Vec<[f32; 4]>,
    start_bindings: BTreeMap<String, usize>,
    conditions: BTreeMap<String, (Program, Vec<f32>)>,
    locomotion: Bindings,
}
impl State {
    pub fn new(graph: &Document) -> Result<Self> {
        let data = graph.single("hkbBehaviorGraphData")?;
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let initial_ref = data["variableInitialValues"]["ref"]
            .as_str()
            .context("Missing initial variable reference")?;
        let initial = graph
            .objects
            .get(initial_ref)
            .context("Missing initial variable values")?;
        ensure!(
            initial.klass == "hkbVariableValueSet",
            "Invalid initial variable object"
        );
        let infos = data["variableInfos"]
            .as_array()
            .context("Missing variable types")?;
        let variable_names = strings["variableNames"]
            .as_array()
            .context("Missing variable names")?;
        let values = initial.fields["wordVariableValues"]
            .as_array()
            .context("Missing variable words")?;
        ensure!(
            infos.len() == variable_names.len() && infos.len() == values.len(),
            "Behavior variable table sizes differ"
        );
        // Quad variables retain their indexed backing store. A scalar read of
        // a vector remains an error, rather than interpreting its index as data.
        let quads: Vec<[f32; 4]> =
            serde_json::from_value(initial.fields["quadVariableValues"].clone())
                .context("Invalid quad variable storage")?;
        ensure!(
            quads.iter().flatten().all(|v| v.is_finite()),
            "Non-finite quad variable"
        );
        ensure!(
            initial.fields["variantVariableValues"]
                .as_array()
                .context("Missing variant variable storage")?
                .is_empty(),
            "Object behavior variables are not implemented"
        );
        let mut names = BTreeMap::new();
        let mut types = vec![];
        let mut words = vec![];
        for (i, ((info, name), value)) in infos.iter().zip(variable_names).zip(values).enumerate() {
            let name = name.as_str().context("Invalid variable name")?;
            ensure!(
                names.insert(name.into(), i).is_none(),
                "Duplicate behavior variable {name}"
            );
            let kind = info["type"].as_u64().context("Invalid variable type")?;
            ensure!(
                kind <= 4 || (6..=8).contains(&kind),
                "Unsupported variable {name}, type {kind}"
            );
            types.push(kind as u8);
            let raw = value["value"].as_i64().context("Invalid variable word")?;
            ensure!(
                (i32::MIN as i64..=u32::MAX as i64).contains(&raw),
                "Variable word overflow"
            );
            if kind >= 6 {
                ensure!(
                    raw >= 0 && (raw as usize) < quads.len(),
                    "Invalid quad backing index for {name}"
                );
            }
            words.push(raw as u32);
        }
        let conditions = graph
            .condition_programs()?
            .into_iter()
            .map(|(id, program)| {
                let scratch = program.scratch();
                (id, (program, scratch))
            })
            .collect();
        Ok(Self {
            bindings: super::variable_bindings::Bindings::compile(graph)?,
            clips: super::clip_runtime::compile(graph, &types)?,
            transition_effects: super::transition_effect::compile(graph)?,
            start_bindings: super::state_start::bindings(graph, &types)?,
            machines: super::state_machine::compile(graph)?,
            expression_modifiers: super::behavior_expression::compile(graph)?,
            attribute_modifiers: super::animation_attributes::compile(graph)?,
            attribute_defaults: serde_json::from_value(data["attributeDefaults"].clone())
                .context("Invalid graph attribute defaults")?,
            names,
            types,
            initial_words: words.clone(),
            initial_quads: quads.clone(),
            quads,
            words,
            conditions,
            locomotion: Bindings::new(graph)?,
        })
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
    /// Native2f3718/2f34cc writes the mapped scalar word directly. The Lua
    /// setter does not check the graph's declared type or editor min/max.
    /// Missing symbols are legitimate, and are distinct from unavailable graphs.
    pub(super) fn write_native_variable(&mut self, name: &str, bits: u32) -> Option<usize> {
        let index = self.names.get(name).copied()?;
        self.words[index] = bits;
        Some(index)
    }
    pub fn real_member(&self, graph: &Document, object: &str, member: &str) -> Result<f32> {
        let value = match self.bindings.read(object, member, &self.words, None)? {
            Some(super::variable_bindings::Scalar::Real(value)) => value,
            Some(_) => anyhow::bail!("Bound member {object}/{member} is not a real scalar"),
            None => graph
                .objects
                .get(object)
                .context("Missing bound object")?
                .fields[member]
                .as_f64()
                .context("Missing authored real member")? as f32,
        };
        ensure!(value.is_finite(), "Non-finite bound real member");
        Ok(value)
    }
    /// Resolve an authored vector/quaternion binding without losing its fourth lane.
    pub fn quad_member(&self, graph: &Document, object: &str, member: &str) -> Result<[f32; 4]> {
        if let Some(value) = self
            .bindings
            .read_quad(object, member, &self.words, &self.quads)?
        {
            return Ok(value);
        }
        let value = graph
            .objects
            .get(object)
            .context("Missing bound object")?
            .fields[member]
            .clone();
        let quad: [f32; 4] =
            serde_json::from_value(value).context("Invalid authored quad member")?;
        ensure!(
            quad.iter().all(|v| v.is_finite()),
            "Non-finite authored quad member"
        );
        Ok(quad)
    }
    /// Resolve the original start-state choice using current graph variables.
    /// SET_ONCE reactivation may bypass selection; the scheduler must decide
    /// whether to call this, then execute notifications and generator activation.
    pub fn select_start_state(
        &self,
        id: &str,
        next_start_index: &mut Option<usize>,
        random_seed: &mut u32,
    ) -> Result<i32> {
        let machine = self
            .machines
            .get(id)
            .context("Unknown start-state machine")?;
        let bound_start = self
            .start_bindings
            .get(id)
            .map(|&i| self.words[i] as i32)
            .unwrap_or(machine.settings.start_state_id);
        let sync = machine.settings.sync_variable_index;
        let sync_value = if sync == -1 {
            None
        } else {
            ensure!(
                self.types.get(sync as usize) == Some(&3),
                "State sync requires int32 variable"
            );
            Some(self.words[sync as usize] as i32)
        };
        let enabled: Vec<_> = machine.states.iter().map(|s| s.info.enable).collect();
        let probabilities: Vec<_> = machine.states.iter().map(|s| s.info.probability).collect();
        super::state_start::select(
            machine,
            bound_start,
            sync_value,
            next_start_index,
            &enabled,
            &probabilities,
            random_seed,
        )
    }
    pub fn types(&self) -> &[u8] {
        &self.types
    }
    pub fn activate_machine(
        &mut self,
        id: &str,
        runtime: &mut super::machine_runtime::Runtime,
        random_seed: &mut u32,
        mut notify: impl FnMut(
            &super::machine_runtime::Runtime,
            super::machine_runtime::Notice<'_>,
        ) -> Result<()>,
    ) -> Result<bool> {
        if runtime.active {
            return Ok(false);
        }
        let machine = self
            .machines
            .get(id)
            .context("Unknown activation machine")?;
        let bound_start = self
            .start_bindings
            .get(id)
            .map(|&i| self.words[i] as i32)
            .unwrap_or(machine.settings.start_state_id);
        let sync = machine.settings.sync_variable_index;
        let sync_value = if sync == -1 {
            None
        } else {
            ensure!(
                self.types.get(sync as usize) == Some(&3),
                "State sync requires int32 variable"
            );
            Some(self.words[sync as usize] as i32)
        };
        let words = &mut self.words;
        runtime.activate(machine, bound_start, sync_value, random_seed, |r, n| {
            if let super::machine_runtime::Notice::SyncVariable { index, value } = &n {
                words[*index] = *value as u32;
            }
            notify(r, n)
        })
    }
    pub fn index(&self, name: &str) -> Result<usize> {
        self.names
            .get(name)
            .copied()
            .with_context(|| format!("Unknown behavior variable {name}"))
    }
    /// Explicitly restore authored initial values; not an inferred level/reset
    /// lifecycle. Callers must also reset generator state when appropriate.
    pub fn restore_initial_values(&mut self) {
        self.words.copy_from_slice(&self.initial_words);
        self.quads.copy_from_slice(&self.initial_quads);
    }
    pub fn set_real(&mut self, index: usize, value: f32) -> Result<()> {
        ensure!(
            self.types.get(index) == Some(&4),
            "Behavior variable is not real"
        );
        // AnimationComponent::SetVariable at REL 0x2f34cc replaces NaN with 0.
        // It does not clamp to the editor min/max values stored in the asset.
        self.words[index] = if value.is_nan() { 0 } else { value.to_bits() };
        Ok(())
    }
    pub fn set_integer(&mut self, index: usize, value: i32) -> Result<()> {
        ensure!(
            self.types.get(index) == Some(&3),
            "Behavior variable is not int32"
        );
        self.words[index] = value as u32;
        Ok(())
    }
    pub fn condition(&mut self, id: &str) -> Result<bool> {
        let (program, scratch) = self
            .conditions
            .get_mut(id)
            .with_context(|| format!("Unknown behavior condition {id}"))?;
        Ok(program.evaluate(&self.words, &self.types, scratch)? != 0.)
    }
    pub fn apply_locomotion(&mut self, output: &Output) -> Result<Option<usize>> {
        self.locomotion.apply(output, &mut self.words)
    }
    pub fn activate_expression_modifier(&mut self, id: &str) -> Result<()> {
        self.expression_modifiers
            .get_mut(id)
            .context("Unknown expression modifier")?
            .activate();
        Ok(())
    }
    pub fn evaluate_expression_modifier(
        &mut self,
        id: &str,
        events: &mut Vec<usize>,
    ) -> Result<()> {
        self.expression_modifiers
            .get_mut(id)
            .context("Unknown expression modifier")?
            .evaluate(&mut self.words, &self.types, events)
    }
    pub fn evaluate_expression_modifier_with_random(
        &mut self,
        id: &str,
        events: &mut Vec<usize>,
        random: &mut u32,
    ) -> Result<()> {
        self.expression_modifiers
            .get_mut(id)
            .context("Unknown expression modifier")?
            .evaluate_with_random(&mut self.words, &self.types, events, Some(random))
    }
    pub fn select_transitions(
        &mut self,
        machine: &str,
        context: &super::transition_select::Context,
        enabled: &[bool],
        local: &[super::transition_select::IntervalState],
        wildcard: &[super::transition_select::IntervalState],
        selection: &mut super::transition_select::Selection,
    ) -> Result<()> {
        let machine = self
            .machines
            .get(machine)
            .context("Unknown state machine")?;
        let conditions = &mut self.conditions;
        let words = &self.words;
        let types = &self.types;
        selection.select(machine, context, enabled, local, wildcard, |id| {
            let (program, scratch) = conditions
                .get_mut(id)
                .context("Unknown transition condition")?;
            Ok(program.evaluate(words, types, scratch)? != 0.)
        })
    }

    /// Event entry for a retained machine without pending transitions. Windows
    /// change even when the active effect disallows interruption. Pending
    /// transitions must use event_windows first, execute any newly ready
    /// pending transition, then select_transitions against the resulting state.
    /// Selection does not enter a state or discard its running effect.
    pub fn select_machine_event(
        &mut self,
        machine: &str,
        runtime: &mut super::machine_runtime::Runtime,
        event: i32,
        nested_state: Option<i32>,
        allow_arbitration: bool,
        selection: &mut super::transition_select::Selection,
    ) -> Result<()> {
        let definition = self
            .machines
            .get(machine)
            .context("Unknown state machine")?;
        ensure!(runtime.active, "Cannot dispatch to inactive state machine");
        runtime.event_windows(definition, event)?;
        self.select_machine_candidates(
            machine,
            runtime,
            event,
            nested_state,
            allow_arbitration,
            selection,
        )
    }
    /// Selection after event windows and pending initiation have been serviced.
    pub fn select_random_machine_candidates(
        &mut self,
        machine: &str,
        runtime: &super::machine_runtime::Runtime,
        event: i32,
        nested_state: Option<i32>,
        seed: &mut u32,
        selection: &mut super::transition_select::Selection,
    ) -> Result<()> {
        let definition = self
            .machines
            .get(machine)
            .context("Unknown state machine")?;
        ensure!(
            definition
                .states
                .iter()
                .all(|s| s.info.variable_binding_set.is_none()),
            "State enable bindings must be resolved before arbitration"
        );
        let enabled: Vec<_> = definition.states.iter().map(|s| s.info.enable).collect();
        let context = super::transition_select::Context {
            event,
            current_state: runtime.current,
            previous_state: runtime.previous,
            nested_state,
            target_filter: None,
            ignore_event: false,
        };
        let conditions = &mut self.conditions;
        let words = &self.words;
        let types = &self.types;
        selection.select_random(
            definition,
            &context,
            &enabled,
            &runtime.local_intervals,
            &runtime.wildcard_intervals,
            seed,
            |id| {
                let (program, scratch) = conditions
                    .get_mut(id)
                    .context("Unknown transition condition")?;
                Ok(program.evaluate(words, types, scratch)? != 0.)
            },
        )
    }
    pub fn select_machine_candidates(
        &mut self,
        machine: &str,
        runtime: &super::machine_runtime::Runtime,
        event: i32,
        nested_state: Option<i32>,
        allow_arbitration: bool,
        selection: &mut super::transition_select::Selection,
    ) -> Result<()> {
        let definition = self
            .machines
            .get(machine)
            .context("Unknown state machine")?;
        selection.candidates.clear();
        if !allow_arbitration {
            return Ok(());
        }
        ensure!(
            definition
                .states
                .iter()
                .all(|s| s.info.variable_binding_set.is_none()),
            "State enable bindings must be resolved before arbitration"
        );
        let enabled: Vec<_> = definition.states.iter().map(|s| s.info.enable).collect();
        let context = super::transition_select::Context {
            event,
            current_state: runtime.current,
            previous_state: runtime.previous,
            nested_state,
            target_filter: None,
            ignore_event: false,
        };
        self.select_transitions(
            machine,
            &context,
            &enabled,
            &runtime.local_intervals,
            &runtime.wildcard_intervals,
            selection,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retail_start_selection_uses_bound_variables_sync_and_override_indices() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        assert_eq!(state.start_bindings.len(), 7);
        let machine_id = |name: &str| {
            state
                .machines
                .iter()
                .find(|(_, m)| m.settings.name == name)
                .unwrap()
                .0
                .clone()
        };
        let root = machine_id("MickeyBehaviorGraph");
        let idle = machine_id("IdleTopGraph");
        let land = machine_id("LandGraph");
        let run = machine_id("RunWalkGraph");
        let mut seed = 123;
        assert_eq!(state.select_start_state(&root, &mut None, &mut seed)?, 0);
        state.set_integer(52, 1)?;
        assert_eq!(state.select_start_state(&idle, &mut None, &mut seed)?, 1);
        state.set_integer(5, 3)?;
        assert_eq!(state.select_start_state(&land, &mut None, &mut seed)?, 3);
        state.set_integer(5, 99)?;
        assert_eq!(state.select_start_state(&land, &mut None, &mut seed)?, 0);
        state.set_integer(47, 1)?;
        assert_eq!(state.select_start_state(&run, &mut None, &mut seed)?, 1);
        let mut override_index = Some(4);
        assert_eq!(
            state.select_start_state(&land, &mut override_index, &mut seed)?,
            4
        );
        assert_eq!(override_index, None);
        assert_eq!(seed, 123); // No random choice in these authored modes.
        state.machines.get_mut(&idle).unwrap().states[1].info.enable = false;
        assert_eq!(state.select_start_state(&idle, &mut None, &mut seed)?, 0);
        state.machines.get_mut(&idle).unwrap().states[0].info.enable = false;
        assert_eq!(state.select_start_state(&idle, &mut None, &mut seed)?, 0);
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retail_initial_values_drive_conditions_and_receive_controller_inputs() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        assert_eq!(state.words().len(), 54);
        assert_eq!(state.expression_modifiers.len(), 13);
        assert_eq!(
            state
                .expression_modifiers
                .values()
                .map(|m| m.statements.len())
                .sum::<usize>(),
            24
        );
        let initial = state.words().to_vec();
        let combat = state.index("InternalVar_InCombatIdle")?;
        assert!(state.condition("2:1120")?);
        assert!(!state.condition("2:1424")?);
        state.set_integer(combat, 1)?;
        assert!(!state.condition("2:1120")?);
        assert!(state.condition("2:1424")?);
        assert!(state.set_real(combat, 1.).is_err());
        let vertical = state.index("VAR_Y_Velocity")?;
        state.set_real(vertical, -20.)?;
        assert_eq!(f32::from_bits(state.words()[vertical]), -20.); // authoring bounds aren't clamps
        state.set_real(vertical, f32::NAN)?;
        assert_eq!(state.words()[vertical], 0);
        let output = super::super::locomotion_animation::ground(0., 5., 5., 5., 2.)?;
        assert_eq!(state.apply_locomotion(&output)?, Some(6));
        assert_eq!(
            f32::from_bits(state.words()[state.index("VAR_Scaled_Run_Speed")?]),
            1.
        );
        let mut events = vec![];
        state.activate_expression_modifier("2:38784")?;
        state.evaluate_expression_modifier("2:38784", &mut events)?;
        assert_eq!(
            f32::from_bits(state.words()[state.index("internalVar_Scaled_TipToeSpeed")?]),
            10.
        );
        let normalized = state.index("VAR_Normalized_Forward_Speed")?;
        state.set_real(normalized, 0.5)?;
        state.activate_expression_modifier("2:40048")?;
        state.evaluate_expression_modifier("2:40048", &mut events)?;
        assert_eq!(events, [125]); // authored InternalEvent_StartWalkState
        events.clear();
        state.set_real(normalized, 0.28)?;
        state.evaluate_expression_modifier("2:40048", &mut events)?;
        assert_eq!(events, [127]); // authored <= 0.28 sneak boundary
        state.restore_initial_values();
        assert_eq!(state.words(), initial);
        Ok(())
    }
}
