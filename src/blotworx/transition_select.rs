//! State-machine candidate filtering and arbitration recovered from DOL
//! 0x80216f4c and 0x802173b8. Lifecycle, interval clocks, delayed execution,
//! random selection and transition effects are separate runtime stages.
use super::state_machine::{Machine, Transition};
use anyhow::{ensure, Context as _, Result};

const TRIGGER: u16 = 1;
const INITIATE: u16 = 2;
const DISABLED: u16 = 32;
const NO_RETURN: u16 = 64;
const NO_RANDOM: u16 = 128;
const NO_CONDITION: u16 = 256;
const ALLOW_SELF: u16 = 512;
const LOCAL_WILDCARD: u16 = 2048;
const FROM_NESTED: u16 = 4096;
const ABUT: u16 = 16384;

#[derive(Clone, Copy, Debug, Default)]
pub struct IntervalState(pub u8);
impl IntervalState {
    /// Retail update uses the entire step to test overlap: the entering edge
    /// compares the NEW time, while the exiting edge compares the OLD time.
    /// Thus a narrow window crossed in one update is still eligible. A zero
    /// or negative exit time means no timed exit. Event windows are untouched.
    /// DOL 0x802180a8..0x802182d8.
    pub fn step(&mut self, transition: &Transition, old_time: f32, dt: f32) {
        for (flag, interval) in [
            (INITIATE, &transition.initiate_interval),
            (TRIGGER, &transition.trigger_interval),
        ] {
            if transition.flags & flag != 0
                && interval.enter_event_id == -1
                && interval.exit_event_id == -1
            {
                let open = old_time + dt >= interval.enter_time
                    && (interval.exit_time <= 0. || old_time <= interval.exit_time);
                if open {
                    self.0 |= flag as u8;
                } else {
                    self.0 &= !(flag as u8);
                }
            }
        }
    }
    /// DOL 0x8021a408..0x8021a57c, when entering a state. An interval
    /// without an enter event starts open if it has an exit event, or if
    /// its time-based enter boundary is already reached at time zero.
    pub fn entering(transition: &Transition) -> Self {
        let mut bits = 0;
        for (flag, interval) in [
            (TRIGGER, &transition.trigger_interval),
            (INITIATE, &transition.initiate_interval),
        ] {
            if transition.flags & flag != 0
                && interval.enter_event_id == -1
                && (interval.exit_event_id != -1 || interval.enter_time <= 0.)
            {
                bits |= flag as u8;
            }
        }
        Self(bits)
    }
    /// Handle enter before exit, so an event shared by both closes the window.
    /// DOL 0x8021977c applies this before considering event transitions.
    pub fn event(&mut self, transition: &Transition, event: i32) {
        for (flag, interval) in [
            (TRIGGER, &transition.trigger_interval),
            (INITIATE, &transition.initiate_interval),
        ] {
            if transition.flags & flag != 0 {
                if interval.enter_event_id == event {
                    self.0 |= flag as u8;
                }
                if interval.exit_event_id == event {
                    self.0 &= !(flag as u8);
                }
            }
        }
    }
    fn initiation_ready(self, t: &Transition) -> bool {
        t.flags & INITIATE == 0 || self.0 & INITIATE as u8 != 0
    }
    fn waits(self, t: &Transition) -> bool {
        !self.initiation_ready(t) || t.flags & ABUT != 0
    }
}

pub struct Context {
    pub event: i32,
    pub current_state: i32,
    pub previous_state: i32,
    pub nested_state: Option<i32>,
    pub target_filter: Option<i32>,
    pub ignore_event: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub wildcard: bool,
    pub index: usize,
}

/// Retain the highest priority; at equal priority a ready candidate replaces
/// pending ones. Otherwise the first ready candidate wins in authored order.
fn collect_table(
    machine: &Machine,
    table: &[Transition],
    intervals: &[IntervalState],
    wildcard: bool,
    context: &Context,
    enabled: &[bool],
    condition: &mut impl FnMut(&str) -> Result<bool>,
    output: &mut Vec<usize>,
    retain_all: bool,
) -> Result<()> {
    ensure!(
        intervals.len() == table.len(),
        "Transition interval count differs"
    );
    output.clear();
    for (i, t) in table.iter().enumerate() {
        let target = machine
            .state_indices
            .get(&t.to_state_id)
            .context("Missing transition target")?;
        if context.target_filter.is_some_and(|id| id != t.to_state_id) || !enabled[*target] {
            continue;
        }
        if wildcard
            && (t.flags & LOCAL_WILDCARD == 0
                || (t.to_state_id == context.current_state && t.flags & ALLOW_SELF == 0))
        {
            continue;
        }
        if t.flags & TRIGGER != 0 && intervals[i].0 & TRIGGER as u8 == 0 {
            continue;
        }
        if t.flags & DISABLED != 0 {
            continue;
        }
        let return_previous = context.event != -1
            && context.event == machine.settings.return_to_previous_state_event_id
            && t.to_state_id == context.previous_state
            && context.previous_state != context.current_state
            && t.flags & NO_RETURN == 0;
        let random = context.event != -1
            && context.event == machine.settings.random_transition_event_id
            && t.flags & NO_RANDOM == 0;
        if !(context.ignore_event || t.event_id == context.event || return_previous || random) {
            continue;
        }
        if t.flags & FROM_NESTED != 0 && context.nested_state != Some(t.from_nested_state_id) {
            continue;
        }
        if t.flags & NO_CONDITION == 0 {
            if let Some(reference) = &t.condition {
                if !condition(&reference.id)? {
                    continue;
                }
            }
        }
        if retain_all {
            output.push(i);
            continue;
        }
        if let Some(&first) = output.first() {
            if t.priority < table[first].priority {
                continue;
            }
            if t.priority == table[first].priority {
                if !intervals[first].waits(&table[first]) {
                    continue;
                }
                if intervals[i].waits(t) {
                    output.push(i);
                    continue;
                }
            }
            output.clear();
        }
        output.push(i);
    }
    Ok(())
}

#[derive(Default)]
pub struct Selection {
    local: Vec<usize>,
    wildcard: Vec<usize>,
    pub candidates: Vec<Candidate>,
}
impl Selection {
    /// DOL 0x80217420..0x802175a4 gathers ALL eligible local transitions,
    /// then wildcards, and weights each entry by its destination state's
    /// probability. Priority/readiness arbitration is bypassed for this event.
    pub fn select_random(
        &mut self,
        machine: &Machine,
        context: &Context,
        enabled: &[bool],
        local_intervals: &[IntervalState],
        wildcard_intervals: &[IntervalState],
        seed: &mut u32,
        mut condition: impl FnMut(&str) -> Result<bool>,
    ) -> Result<()> {
        self.candidates.clear();
        ensure!(
            enabled.len() == machine.states.len(),
            "State enable count differs"
        );
        ensure!(
            context.event != -1 && context.event == machine.settings.random_transition_event_id,
            "Random selection requires the authored random event"
        );
        let current = *machine
            .state_indices
            .get(&context.current_state)
            .context("Unknown current state")?;
        let local = &machine.states[current].transitions;
        let wild = &machine.wildcard_transitions;
        collect_table(
            machine,
            local,
            local_intervals,
            false,
            context,
            enabled,
            &mut condition,
            &mut self.local,
            true,
        )?;
        collect_table(
            machine,
            wild,
            wildcard_intervals,
            true,
            context,
            enabled,
            &mut condition,
            &mut self.wildcard,
            true,
        )?;
        let entries: Vec<_> = self
            .local
            .iter()
            .map(|&index| Candidate {
                wildcard: false,
                index,
            })
            .chain(self.wildcard.iter().map(|&index| Candidate {
                wildcard: true,
                index,
            }))
            .collect();
        if entries.is_empty() {
            return Ok(());
        }
        let probabilities: Vec<_> = entries
            .iter()
            .map(|c| {
                let t = if c.wildcard {
                    &wild[c.index]
                } else {
                    &local[c.index]
                };
                machine.states[machine.state_indices[&t.to_state_id]]
                    .info
                    .probability
            })
            .collect();
        self.candidates
            .push(entries[super::state_start::random_index(&probabilities, seed)]);
        Ok(())
    }
    /// `enabled` must contain the current (binding-resolved) state enable flags.
    /// Interval flags are maintained externally; no inferred windows/defaults.
    /// Reuses buffers across calls; no fixed candidate-count limit.
    pub fn select(
        &mut self,
        machine: &Machine,
        context: &Context,
        enabled: &[bool],
        local_intervals: &[IntervalState],
        wildcard_intervals: &[IntervalState],
        mut condition: impl FnMut(&str) -> Result<bool>,
    ) -> Result<()> {
        self.candidates.clear();
        ensure!(
            enabled.len() == machine.states.len(),
            "State enable count differs"
        );
        ensure!(
            context.event == -1
                || (context.event != machine.settings.random_transition_event_id
                    && context.event != machine.settings.transition_to_next_higher_state_event_id
                    && context.event != machine.settings.transition_to_next_lower_state_event_id),
            "Random/adjacent-state routing must run before ordinary candidate arbitration"
        );
        let current = machine
            .state_indices
            .get(&context.current_state)
            .context("Unknown current state")?;
        let local = &machine.states[*current].transitions;
        let wild = &machine.wildcard_transitions;
        collect_table(
            machine,
            local,
            local_intervals,
            false,
            context,
            enabled,
            &mut condition,
            &mut self.local,
            false,
        )?;
        collect_table(
            machine,
            wild,
            wildcard_intervals,
            true,
            context,
            enabled,
            &mut condition,
            &mut self.wildcard,
            false,
        )?;
        let a = self.local.first().copied();
        let b = self.wildcard.first().copied();
        let a_ready = a.is_some_and(|i| local_intervals[i].initiation_ready(&local[i]));
        let b_ready = b.is_some_and(|i| wildcard_intervals[i].initiation_ready(&wild[i]));
        // A ready lower-priority table beats a pending higher-priority table.
        // Ties between the tables favor the current state's own transitions.
        if a_ready || b_ready {
            let choose_local =
                a_ready && (!b_ready || local[a.unwrap()].priority >= wild[b.unwrap()].priority);
            self.candidates.push(Candidate {
                wildcard: !choose_local,
                index: if choose_local { a.unwrap() } else { b.unwrap() },
            });
        } else {
            let choose_local = a.is_some()
                && (b.is_none() || local[a.unwrap()].priority >= wild[b.unwrap()].priority);
            let (indices, wildcard) = if choose_local {
                (&self.local, false)
            } else {
                (&self.wildcard, true)
            };
            self.candidates
                .extend(indices.iter().map(|&index| Candidate { wildcard, index }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blotworx::{behavior::Document, behavior_state::State};

    #[test]
    fn timed_windows_test_step_overlap_and_leave_event_windows_alone() {
        let mut t = transition(0, TRIGGER | INITIATE);
        t.trigger_interval = super::super::state_machine::Interval {
            enter_event_id: -1,
            exit_event_id: -1,
            enter_time: 0.2,
            exit_time: 0.21,
        };
        t.initiate_interval = super::super::state_machine::Interval {
            enter_event_id: 8,
            exit_event_id: 9,
            enter_time: 0.,
            exit_time: 0.,
        };
        let mut w = IntervalState(2);
        w.step(&t, 0.1, 0.3); // Entire time window is crossed in a single tick.
        assert_eq!(w.0, 3);
        w.step(&t, 0.21, 0.1); // Old time at the exit is still inclusive.
        assert_eq!(w.0, 3);
        w.step(&t, 0.22, 0.1);
        assert_eq!(w.0, 2);
        w.event(&t, 9);
        w.step(&t, 0., 0.1);
        assert_eq!(w.0, 0);
        t.trigger_interval.exit_time = 0.;
        w.step(&t, 100., 0.);
        assert_eq!(w.0, 1);
    }

    #[test]
    fn state_entry_initializes_event_and_time_windows_independently() {
        let mut t = transition(0, TRIGGER | INITIATE);
        t.trigger_interval.enter_event_id = -1;
        t.trigger_interval.exit_event_id = 6;
        t.trigger_interval.enter_time = 10.;
        t.initiate_interval.enter_event_id = 5;
        t.initiate_interval.exit_event_id = -1;
        t.initiate_interval.enter_time = 0.;
        let mut interval = IntervalState::entering(&t);
        assert_eq!(interval.0, 1); // Exit-only event window is initially open.
        interval.event(&t, 5);
        assert_eq!(interval.0, 3);
        interval.event(&t, 6);
        assert_eq!(interval.0, 2);
        t.trigger_interval.exit_event_id = -1;
        t.trigger_interval.enter_time = 0.;
        t.initiate_interval.enter_event_id = -1;
        t.initiate_interval.enter_time = 0.01;
        assert_eq!(IntervalState::entering(&t).0, 1);
        t.flags = 0;
        assert_eq!(IntervalState::entering(&t).0, 0);
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn selection_matches_observed_idle_run_skid_resolve_outcomes() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/root_transitions.json"
        ))?;
        let mut state = State::new(&graph)?;
        let mut selection = Selection::default();
        let observations = fixture["observations"].as_array().unwrap();
        assert_eq!(observations.len(), 3);
        for observation in observations {
            let words: Vec<u32> = serde_json::from_value(observation["words"].clone())?;
            for (i, kind) in state.types().to_vec().into_iter().enumerate() {
                match kind {
                    3 => state.set_integer(i, words[i] as i32)?,
                    4 => state.set_real(i, f32::from_bits(words[i]))?,
                    _ => panic!("Unexpected Mickey variable type"),
                }
            }
            let context = Context {
                event: observation["event"].as_i64().unwrap() as i32,
                current_state: observation["from_state"].as_i64().unwrap() as i32,
                previous_state: -1,
                nested_state: None,
                target_filter: None,
                ignore_event: false,
            };
            let root = &state.machines["2:240"];
            let enabled: Vec<_> = root.states.iter().map(|s| s.info.enable).collect();
            let flags = vec![
                IntervalState::default();
                root.states[root.state_indices[&context.current_state]]
                    .transitions
                    .len()
            ];
            let wildcard = vec![IntervalState::default(); root.wildcard_transitions.len()];
            state.select_transitions(
                "2:240",
                &context,
                &enabled,
                &flags,
                &wildcard,
                &mut selection,
            )?;
            assert_eq!(selection.candidates.len(), 1);
            let candidate = selection.candidates[0];
            let root = &state.machines["2:240"];
            let transition = if candidate.wildcard {
                &root.wildcard_transitions[candidate.index]
            } else {
                &root.states[root.state_indices[&context.current_state]].transitions
                    [candidate.index]
            };
            assert_eq!(
                transition.event_id as i64,
                observation["event"].as_i64().unwrap()
            );
            assert_eq!(
                transition.to_state_id as i64,
                observation["to_state"].as_i64().unwrap()
            );
            assert_eq!(
                transition.flags as u64,
                observation["flags"].as_u64().unwrap()
            );
            let effect = &graph.objects[&transition.transition.as_ref().unwrap().id];
            assert_eq!(effect.klass, observation["effect_class"].as_str().unwrap());
            if effect.klass == "hkbBlendingTransitionEffect" {
                assert_eq!(
                    effect.fields["duration"].as_f64(),
                    observation["duration"].as_f64()
                );
            }
        }
        Ok(())
    }

    fn transition(priority: i16, flags: u16) -> Transition {
        let interval = super::super::state_machine::Interval {
            enter_event_id: 10,
            exit_event_id: 11,
            enter_time: 0.,
            exit_time: 1.,
        };
        Transition {
            trigger_interval: interval.clone(),
            initiate_interval: interval,
            transition: None,
            condition: None,
            event_id: 7,
            to_state_id: 0,
            from_nested_state_id: 4,
            to_nested_state_id: 0,
            priority,
            flags,
        }
    }
    fn machine(local: Vec<Transition>, wildcard: Vec<Transition>) -> Machine {
        use super::super::state_machine::{Event, Reference, Settings, State, StateInfo};
        let no_event = Event {
            id: -1,
            payload: None,
        };
        Machine {
            settings: Settings {
                variable_binding_set: None,
                user_data: 0,
                name: "fixture".into(),
                event_to_send_when_state_or_transition_changes: no_event.clone(),
                start_state_id: 0,
                return_to_previous_state_event_id: 8,
                random_transition_event_id: 9,
                transition_to_next_higher_state_event_id: -1,
                transition_to_next_lower_state_event_id: -1,
                sync_variable_index: -1,
                wrap_around_state_id: false,
                max_simultaneous_transitions: 32,
                start_state_mode: 0,
                self_transition_mode: 0,
                states: vec![],
                wildcard_transitions: None,
            },
            states: vec![State {
                enter_notify_events: Vec::new(),
                exit_notify_events: Vec::new(),
                info: StateInfo {
                    variable_binding_set: None,
                    listeners: vec![],
                    enter_notify_event: no_event.clone(),
                    exit_notify_event: no_event,
                    transitions: None,
                    generator: Reference {
                        id: "fixture".into(),
                    },
                    name: "fixture".into(),
                    state_id: 0,
                    probability: 1.,
                    enable: true,
                },
                transitions: local,
            }],
            wildcard_transitions: wildcard,
            state_indices: [(0, 0)].into_iter().collect(),
        }
    }
    fn context() -> Context {
        Context {
            event: 7,
            current_state: 0,
            previous_state: -1,
            nested_state: None,
            target_filter: None,
            ignore_event: false,
        }
    }
    #[test]
    fn random_event_keeps_all_priorities_and_weights_each_transition_entry() -> Result<()> {
        let mut m = machine(
            vec![
                transition(100, NO_RANDOM),
                transition(99, 0),
                transition(-10, INITIATE),
            ],
            vec![transition(200, LOCAL_WILDCARD | ALLOW_SELF)],
        );
        let mut destination = machine(vec![], vec![]).states.remove(0);
        destination.info.state_id = 1;
        destination.info.probability = 1.;
        destination.transitions.clear();
        m.states[0].info.probability = 0.;
        m.states.push(destination);
        m.state_indices.insert(1, 1);
        m.states[0].transitions[2].to_state_id = 1;
        let mut ctx = context();
        ctx.event = 9;
        let mut selection = Selection::default();
        let mut seed = 0;
        selection.select_random(
            &m,
            &ctx,
            &[true, true],
            &[IntervalState(0); 3],
            &[IntervalState(0)],
            &mut seed,
            |_| Ok(true),
        )?;
        assert_eq!(
            selection.candidates,
            [Candidate {
                wildcard: false,
                index: 2
            }]
        );
        assert_eq!(seed, 1_013_904_223);
        // No candidate means no draw from the shared random stream.
        selection.select_random(
            &m,
            &ctx,
            &[false, false],
            &[IntervalState(0); 3],
            &[IntervalState(0)],
            &mut seed,
            |_| Ok(true),
        )?;
        assert!(selection.candidates.is_empty());
        assert_eq!(seed, 1_013_904_223);
        Ok(())
    }
    #[test]
    fn ready_and_pending_priority_rules_preserve_authored_order() -> Result<()> {
        let mut selection = Selection::default();
        let machine = machine(
            vec![
                transition(0, INITIATE),
                transition(0, ABUT),
                transition(0, 0),
                transition(0, 0),
            ],
            vec![transition(10, LOCAL_WILDCARD | ALLOW_SELF | INITIATE)],
        );
        let local = vec![IntervalState(0); 4];
        selection.select(
            &machine,
            &context(),
            &[true],
            &local,
            &[IntervalState(0)],
            |_| Ok(true),
        )?;
        assert_eq!(
            selection.candidates,
            [Candidate {
                wildcard: false,
                index: 2
            }]
        );
        selection.select(
            &machine,
            &context(),
            &[true],
            &local,
            &[IntervalState(2)],
            |_| Ok(true),
        )?;
        assert_eq!(
            selection.candidates,
            [Candidate {
                wildcard: true,
                index: 0
            }]
        );
        let pending = self::machine(
            vec![transition(0, INITIATE), transition(0, INITIATE)],
            vec![],
        );
        selection.select(
            &pending,
            &context(),
            &[true],
            &[IntervalState(0); 2],
            &[],
            |_| Ok(true),
        )?;
        assert_eq!(
            selection.candidates,
            [
                Candidate {
                    wildcard: false,
                    index: 0
                },
                Candidate {
                    wildcard: false,
                    index: 1
                }
            ]
        );
        Ok(())
    }
    #[test]
    fn interval_events_nested_filters_and_disabled_states() -> Result<()> {
        let mut t = transition(0, TRIGGER | INITIATE | FROM_NESTED);
        let mut flags = IntervalState(0);
        flags.event(&t, 10);
        assert_eq!(flags.0, 3);
        flags.event(&t, 11);
        assert_eq!(flags.0, 0);
        t.trigger_interval.exit_event_id = 10;
        flags.event(&t, 10);
        assert_eq!(flags.0, 2); // exit wins for shared trigger event
        let machine = machine(vec![t], vec![]);
        let mut selection = Selection::default();
        let mut ctx = context();
        selection.select(&machine, &ctx, &[true], &[IntervalState(3)], &[], |_| {
            Ok(true)
        })?;
        assert!(selection.candidates.is_empty());
        ctx.nested_state = Some(4);
        selection.select(&machine, &ctx, &[true], &[IntervalState(3)], &[], |_| {
            Ok(true)
        })?;
        assert_eq!(selection.candidates.len(), 1);
        selection.select(&machine, &ctx, &[false], &[IntervalState(3)], &[], |_| {
            Ok(true)
        })?;
        assert!(selection.candidates.is_empty());
        assert!(selection
            .select(&machine, &ctx, &[true], &[], &[], |_| Ok(true))
            .is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retail_idle_candidates_obey_combat_condition_and_array_order() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let machines = super::super::state_machine::compile(&graph)?;
        let root = &machines["2:240"];
        let mut state = State::new(&graph)?;
        let context = Context {
            event: 6,
            current_state: 0,
            previous_state: 0,
            nested_state: None,
            target_filter: None,
            ignore_event: false,
        };
        let enabled: Vec<_> = root.states.iter().map(|s| s.info.enable).collect();
        let flags =
            vec![IntervalState::default(); root.states[root.state_indices[&0]].transitions.len()];
        let wildcard = vec![IntervalState::default(); root.wildcard_transitions.len()];
        let mut selection = Selection::default();
        selection.select(root, &context, &enabled, &flags, &wildcard, |id| {
            state.condition(id)
        })?;
        assert_eq!(
            selection.candidates,
            [Candidate {
                wildcard: false,
                index: 0
            }]
        );
        let combat = state.index("InternalVar_InCombatIdle")?;
        state.set_integer(combat, 1)?;
        selection.select(root, &context, &enabled, &flags, &wildcard, |id| {
            state.condition(id)
        })?;
        assert_eq!(
            selection.candidates,
            [Candidate {
                wildcard: false,
                index: 3
            }]
        );
        Ok(())
    }
}
