//! State-entry/activation lifecycle recovered from DOL 0x80217adc and
//! 0x8021a368. Generator activation, transition stacks and deactivation are
//! separate scheduler operations, not implicit side effects of this module.
use super::{
    state_machine::{Event, Machine, Reference},
    state_start,
    transition_select::IntervalState,
};
use anyhow::{ensure, Context, Result};

/// Notifications must be handled by the caller; there is no default no-op sink.
#[derive(Debug)]
pub enum Notice<'a> {
    Event(&'a Event),
    Listener {
        reference: &'a Reference,
        entering: bool,
    },
    SyncVariable {
        index: usize,
        value: i32,
    },
}

#[derive(Default)]
pub struct Runtime {
    pub active: bool,
    pub current: i32,
    pub previous: i32,
    pub time: f32,
    pub next_start_index: Option<usize>,
    pub reset: bool,
    pub echo_next_update: bool,
    pub local_intervals: Vec<IntervalState>,
    pub wildcard_intervals: Vec<IntervalState>,
}

fn state_index(machine: &Machine, id: i32) -> Option<usize> {
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

impl Runtime {
    /// Clone initialization precedes graph-variable binding. PC cloneNode
    /// 65ce80 -> 65c830 -> 65a660 copies authored startStateId (+6c) to
    /// current (+84) and previous (+dc), without selection or notifications.
    /// Wii U's clone constructor independently performs the same assignment.
    pub fn from_definition(machine: &Machine) -> Self {
        Self {
            current: machine.settings.start_state_id,
            previous: machine.settings.start_state_id,
            ..Default::default()
        }
    }
    /// DOL 0x80218670..0x80218a1c, after the owner retires all transition
    /// instances and clears pending requests. Exit callbacks observe active=true.
    pub fn deactivate(
        &mut self,
        machine: &Machine,
        notify: &mut impl FnMut(&Self, Notice<'_>) -> Result<()>,
    ) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let state = &machine.states
            [state_index(machine, self.current).context("Missing state during deactivation")?];
        for event in std::iter::once(&state.info.exit_notify_event).chain(&state.exit_notify_events)
        {
            if event.id != -1 {
                notify(self, Notice::Event(event))?;
            }
        }
        for reference in &state.info.listeners {
            notify(
                self,
                Notice::Listener {
                    reference,
                    entering: false,
                },
            )?;
        }
        self.active = false;
        Ok(())
    }
    /// Apply the event to BOTH tables before candidate arbitration. Even an
    /// event that cannot take a transition may change a retained interval.
    /// DOL 0x8021977c..0x802199e4. This deliberately does not enter a state:
    /// the selected transition may have a delayed start or an animation effect.
    pub fn event_windows(&mut self, machine: &Machine, event: i32) -> Result<()> {
        let local = &machine.states
            [state_index(machine, self.current).context("Invalid active state")?]
        .transitions;
        ensure!(
            self.local_intervals.len() == local.len()
                && self.wildcard_intervals.len() == machine.wildcard_transitions.len(),
            "State-machine interval storage differs from active state"
        );
        for (window, transition) in self.local_intervals.iter_mut().zip(local) {
            window.event(transition, event);
        }
        for (window, transition) in self
            .wildcard_intervals
            .iter_mut()
            .zip(&machine.wildcard_transitions)
        {
            window.event(transition, event);
        }
        Ok(())
    }

    /// The interval/clock phase of update. The owner must run state listeners
    /// first, then this phase, transition retirement and eventless arbitration,
    /// followed by change notification. Inactive machines do not advance.
    pub fn advance_windows(&mut self, machine: &Machine, dt: f32) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        ensure!(
            dt.is_finite() && self.time.is_finite() && (self.time + dt).is_finite(),
            "Invalid state-machine clock"
        );
        let local = &machine.states
            [state_index(machine, self.current).context("Invalid active state")?]
        .transitions;
        ensure!(
            self.local_intervals.len() == local.len()
                && self.wildcard_intervals.len() == machine.wildcard_transitions.len(),
            "State-machine interval storage differs from active state"
        );
        for (window, transition) in self.local_intervals.iter_mut().zip(local) {
            window.step(transition, self.time, dt);
        }
        for (window, transition) in self
            .wildcard_intervals
            .iter_mut()
            .zip(&machine.wildcard_transitions)
        {
            window.step(transition, self.time, dt);
        }
        self.time += dt;
        Ok(())
    }
    /// The graph scheduler separately activates this state's generator.
    pub fn current_generator<'a>(&self, machine: &'a Machine) -> Result<&'a Reference> {
        Ok(
            &machine.states[state_index(machine, self.current).context("Invalid active state")?]
                .info
                .generator,
        )
    }

    /// Entry changes machine state, not the transition-effect stack. Call only
    /// after the scheduler has performed the required transition bookkeeping.
    pub fn enter(
        &mut self,
        machine: &Machine,
        target: i32,
        reset_previous: bool,
        notify: &mut impl FnMut(&Self, Notice<'_>) -> Result<()>,
    ) -> Result<()> {
        let new = &machine.states[state_index(machine, target).context("Missing target state")?];
        let old = if self.active {
            Some(
                &machine.states
                    [state_index(machine, self.current).context("Missing prior active state")?],
            )
        } else {
            None
        };
        self.local_intervals.clear();
        self.local_intervals
            .extend(new.transitions.iter().map(IntervalState::entering));
        self.wildcard_intervals.clear();
        self.wildcard_intervals.extend(
            machine
                .wildcard_transitions
                .iter()
                .map(IntervalState::entering),
        );
        // Retail resets the NEW state's windows before issuing OLD exit notices.
        if let Some(old) = old {
            for event in std::iter::once(&old.info.exit_notify_event).chain(&old.exit_notify_events)
            {
                if event.id != -1 {
                    notify(self, Notice::Event(event))?;
                }
            }
            for reference in &old.info.listeners {
                notify(
                    self,
                    Notice::Listener {
                        reference,
                        entering: false,
                    },
                )?;
            }
        }
        self.previous = if reset_previous { target } else { self.current };
        self.current = target;
        self.time = 0.;
        for event in std::iter::once(&new.info.enter_notify_event).chain(&new.enter_notify_events) {
            if event.id != -1 {
                notify(self, Notice::Event(event))?;
            }
        }
        for reference in &new.info.listeners {
            notify(
                self,
                Notice::Listener {
                    reference,
                    entering: true,
                },
            )?;
        }
        Ok(())
    }

    /// Bound values are supplied after graph variable binding. Returns false
    /// for an already active machine, without consuming an override or RNG.
    pub fn activate(
        &mut self,
        machine: &Machine,
        bound_start: i32,
        sync_value: Option<i32>,
        random_seed: &mut u32,
        mut notify: impl FnMut(&Self, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        if self.active {
            return Ok(false);
        }
        if machine.settings.start_state_mode != 1 || self.next_start_index.is_some() {
            let enabled: Vec<_> = machine.states.iter().map(|s| s.info.enable).collect();
            let probabilities: Vec<_> = machine.states.iter().map(|s| s.info.probability).collect();
            let target = state_start::select(
                machine,
                bound_start,
                sync_value,
                &mut self.next_start_index,
                &enabled,
                &probabilities,
                random_seed,
            )?;
            self.enter(machine, target, true, &mut notify)?;
            if machine.settings.sync_variable_index != -1 {
                notify(
                    self,
                    Notice::SyncVariable {
                        index: machine.settings.sync_variable_index as usize,
                        value: target,
                    },
                )?;
            }
        } else {
            // SET_ONCE retains state, including a disabled state. If its old ID
            // disappeared, retail uses the first state without enabled filtering.
            if state_index(machine, self.current).is_none() {
                self.current = machine
                    .states
                    .first()
                    .context("Empty state machine")?
                    .info
                    .state_id;
                self.previous = self.current;
            }
            let previous = self.previous;
            self.enter(machine, self.current, false, &mut notify)?;
            self.previous = previous;
        }
        self.active = true;
        self.reset = true;
        self.echo_next_update = false;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{behavior::Document, behavior_state::State};
    use super::*;

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retained_machine_routes_captured_locomotion_events_without_premature_entry() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(&graph, State::new(&graph)?, 0)?;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/root_transitions.json"
        ))?;
        let mut selection = super::super::transition_select::Selection::default();
        for sample in fixture["observations"].as_array().unwrap() {
            let state = &mut prepared.state;
            for (i, kind) in state.types().to_vec().into_iter().enumerate() {
                let word = sample["words"][i].as_u64().unwrap() as u32;
                match kind {
                    3 => state.set_integer(i, word as i32)?,
                    4 => state.set_real(i, f32::from_bits(word))?,
                    _ => anyhow::bail!("Unexpected variable type"),
                }
            }
            let runtime = prepared.machines.get_mut("2:240").unwrap();
            let from = sample["from_state"].as_i64().unwrap() as i32;
            let target = sample["to_state"].as_i64().unwrap() as i32;
            assert_eq!(runtime.current, from);
            state.select_machine_event(
                "2:240",
                runtime,
                sample["event"].as_i64().unwrap() as i32,
                None,
                true,
                &mut selection,
            )?;
            assert_eq!(runtime.current, from); // Selection must not bypass the effect.
            assert_eq!(selection.candidates.len(), 1);
            let candidate = selection.candidates[0];
            let machine = &state.machines["2:240"];
            let table = if candidate.wildcard {
                &machine.wildcard_transitions
            } else {
                &machine.states[machine.state_indices[&from]].transitions
            };
            let transition = &table[candidate.index];
            assert_eq!(transition.to_state_id, target);
            let effect = &graph.objects[&transition.transition.as_ref().unwrap().id];
            assert_eq!(effect.klass, sample["effect_class"].as_str().unwrap());
            // Explicitly simulate the owner's completed effect bookkeeping;
            // this test does not claim to execute the transition animation.
            runtime.enter(machine, target, false, &mut |_, notice| {
                ensure!(
                    !matches!(notice, Notice::Listener { .. }),
                    "Unexpected listener"
                );
                Ok(())
            })?;
            assert_eq!(runtime.previous, from);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn interruption_guard_does_not_discard_interval_events() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        let machine = state.machines.get_mut("2:240").unwrap();
        let t = &mut machine.states[0].transitions[0];
        t.flags |= 1;
        t.trigger_interval.enter_event_id = 152;
        t.trigger_interval.exit_event_id = 153;
        let mut runtime = Runtime::default();
        state.activate_machine("2:240", &mut runtime, &mut 0, |_, _| Ok(()))?;
        let mut selection = super::super::transition_select::Selection::default();
        state.select_machine_event("2:240", &mut runtime, 152, None, false, &mut selection)?;
        assert_eq!(runtime.local_intervals[0].0 & 1, 1);
        assert!(selection.candidates.is_empty());
        runtime.advance_windows(&state.machines["2:240"], 0.25)?;
        assert_eq!(runtime.time, 0.25);
        assert_eq!(runtime.local_intervals[0].0 & 1, 1);
        state.select_machine_event("2:240", &mut runtime, 153, None, false, &mut selection)?;
        assert_eq!(runtime.local_intervals[0].0 & 1, 0);
        Ok(())
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn captured_startup_activations_match_scalar_state() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let state = State::new(&graph)?;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/machine_activation.json"
        ))?;
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 4);
        for sample in samples {
            let before = &sample["before"];
            let index = before["next_start_index"].as_i64().unwrap();
            let mut runtime = Runtime {
                current: before["current"].as_i64().unwrap() as i32,
                previous: before["previous"].as_i64().unwrap() as i32,
                time: before["time"].as_f64().unwrap() as f32,
                active: before["active"].as_bool().unwrap(),
                reset: before["reset"].as_bool().unwrap(),
                echo_next_update: before["echo_next_update"].as_bool().unwrap(),
                next_start_index: (index >= 0).then_some(index as usize),
                ..Default::default()
            };
            let machine = &state.machines[sample["object"].as_str().unwrap()];
            assert_eq!(
                machine.settings.start_state_mode,
                sample["mode"].as_u64().unwrap() as u8
            );
            let mut seed = 123;
            runtime.activate(
                machine,
                sample["bound_start"].as_i64().unwrap() as i32,
                sample["sync_value"].as_i64().map(|v| v as i32),
                &mut seed,
                |_, _| Ok(()),
            )?;
            let actual = serde_json::json!({"current":runtime.current,"previous":runtime.previous,
                "time":runtime.time,"active":runtime.active,"reset":runtime.reset,
                "echo_next_update":runtime.echo_next_update,
                "next_start_index":runtime.next_start_index.map(|i|i as i64).unwrap_or(-1)});
            assert_eq!(actual, sample["after"], "{}", sample["name"]);
            assert_eq!(seed, 123);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retail_activation_reads_bound_state_and_writes_sync_before_marking_active() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        let run = state
            .machines
            .iter()
            .find(|(_, m)| m.settings.name == "RunWalkGraph")
            .unwrap()
            .0
            .clone();
        state.set_integer(47, 1)?;
        let mut runtime = Runtime::default();
        let mut notices = vec![];
        let mut seed = 123;
        assert!(
            state.activate_machine(&run, &mut runtime, &mut seed, |r, n| {
                if let Notice::SyncVariable { index, value } = n {
                    notices.push((index, value, r.active));
                }
                Ok(())
            })?
        );
        assert_eq!(notices, [(47, 1, false)]);
        assert_eq!(
            (
                runtime.current,
                runtime.previous,
                runtime.active,
                runtime.reset
            ),
            (1, 1, true, true)
        );
        assert_eq!(state.words()[47], 1);
        assert!(runtime.current_generator(&state.machines[&run]).is_ok());
        runtime.next_start_index = Some(0);
        assert!(
            !state.activate_machine(&run, &mut runtime, &mut seed, |_, _| panic!(
                "Active machine emitted notices"
            ))?
        );
        assert_eq!(runtime.next_start_index, Some(0));
        assert_eq!(seed, 123);
        Ok(())
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn entry_notification_order_and_set_once_retention() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut state = State::new(&graph)?;
        let m = state.machines.get_mut("2:1728").unwrap();
        m.states[0].info.exit_notify_event.id = 9;
        m.states[1].info.enter_notify_event.id = 10;
        m.states[0].info.listeners = vec![Reference {
            id: "old-listener".into(),
        }];
        m.states[1].info.listeners = vec![Reference {
            id: "new-listener".into(),
        }];
        let mut r = Runtime {
            active: true,
            current: 0,
            previous: 7,
            time: 5.,
            ..Default::default()
        };
        let mut order = vec![];
        r.enter(m, 1, false, &mut |r, n| {
            let label = match n {
                Notice::Event(e) => format!("event {}", e.id),
                Notice::Listener {
                    reference,
                    entering,
                } => format!("{} {entering}", reference.id),
                Notice::SyncVariable { .. } => unreachable!(),
            };
            order.push((label, r.current, r.previous, r.time));
            Ok(())
        })?;
        assert_eq!(
            order,
            vec![
                ("event 9".into(), 0, 7, 5.),
                ("old-listener false".into(), 0, 7, 5.),
                ("event 10".into(), 1, 0, 0.),
                ("new-listener true".into(), 1, 0, 0.)
            ]
        );
        m.settings.start_state_mode = 1;
        m.states[1].info.enable = false;
        m.states[1].info.exit_notify_event.id = 11;
        let mut exits = vec![];
        r.deactivate(m, &mut |r, n| {
            assert!(r.active);
            exits.push(match n {
                Notice::Event(e) => format!("event {}", e.id),
                Notice::Listener {
                    reference,
                    entering,
                } => format!("{} {entering}", reference.id),
                Notice::SyncVariable { .. } => unreachable!(),
            });
            Ok(())
        })?;
        assert_eq!(exits, ["event 11", "new-listener false"]);
        assert!(!r.active);
        r.deactivate(m, &mut |_, _| {
            anyhow::bail!("Duplicate deactivation notice")
        })?;
        r.previous = 7;
        r.time = 8.;
        r.activate(m, 0, Some(0), &mut 123, |r, n| {
            assert_eq!(r.current, 1);
            assert!(!matches!(n, Notice::SyncVariable { .. }));
            Ok(())
        })?;
        assert_eq!((r.current, r.previous, r.time), (1, 7, 0.));
        Ok(())
    }
}
