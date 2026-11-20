//! Typed authored state-machine definitions. Execution order is a separate
//! runtime concern; flags and array order must survive compilation unchanged.
use super::behavior::Document;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    #[serde(rename = "ref")]
    pub id: String,
}
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: i32,
    pub payload: Option<Reference>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Interval {
    pub enter_event_id: i32,
    pub exit_event_id: i32,
    pub enter_time: f32,
    pub exit_time: f32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transition {
    pub trigger_interval: Interval,
    pub initiate_interval: Interval,
    pub transition: Option<Reference>,
    pub condition: Option<Reference>,
    pub event_id: i32,
    pub to_state_id: i32,
    pub from_nested_state_id: i32,
    pub to_nested_state_id: i32,
    pub priority: i16,
    pub flags: u16,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateInfo {
    pub variable_binding_set: Option<Reference>,
    pub listeners: Vec<Reference>,
    pub enter_notify_event: Event,
    pub exit_notify_event: Event,
    pub transitions: Option<Reference>,
    pub generator: Reference,
    pub name: String,
    pub state_id: i32,
    pub probability: f32,
    pub enable: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub variable_binding_set: Option<Reference>,
    pub user_data: u64,
    pub name: String,
    pub event_to_send_when_state_or_transition_changes: Event,
    pub start_state_id: i32,
    pub return_to_previous_state_event_id: i32,
    pub random_transition_event_id: i32,
    pub transition_to_next_higher_state_event_id: i32,
    pub transition_to_next_lower_state_event_id: i32,
    pub sync_variable_index: i32,
    pub wrap_around_state_id: bool,
    pub max_simultaneous_transitions: i8,
    /// Execution mode in the Havok-7 vocabulary. The original reflected
    /// document retains its raw enum; 2012 SYNC/RANDOM map to 2/3 here.
    pub start_state_mode: u8,
    pub self_transition_mode: u8,
    pub states: Vec<Reference>,
    pub wildcard_transitions: Option<Reference>,
}
pub struct State {
    pub info: StateInfo,
    pub transitions: Vec<Transition>,
    pub enter_notify_events: Vec<Event>,
    pub exit_notify_events: Vec<Event>,
}
pub struct Machine {
    pub settings: Settings,
    pub states: Vec<State>,
    pub wildcard_transitions: Vec<Transition>,
    pub state_indices: BTreeMap<i32, usize>,
}
fn object<'a>(
    graph: &'a Document,
    reference: &Reference,
    expected: &str,
) -> Result<&'a serde_json::Value> {
    let object = graph
        .objects
        .get(&reference.id)
        .context("Missing state-machine reference")?;
    ensure!(
        object.klass == expected,
        "Expected {expected}, got {}",
        object.klass
    );
    Ok(&object.fields)
}
fn transitions(graph: &Document, reference: &Option<Reference>) -> Result<Vec<Transition>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Array {
        transitions: Vec<Transition>,
    }
    match reference {
        None => Ok(vec![]),
        Some(reference) => Ok(serde_json::from_value::<Array>(
            object(graph, reference, "hkbStateMachineTransitionInfoArray")?.clone(),
        )?
        .transitions),
    }
}
pub fn compile(graph: &Document) -> Result<BTreeMap<String, Machine>> {
    let event_count = graph.single("hkbBehaviorGraphStringData")?["eventNames"]
        .as_array()
        .context("Missing event names")?
        .len();
    let variable_count = graph.single("hkbBehaviorGraphStringData")?["variableNames"]
        .as_array()
        .context("Missing variable names")?
        .len();
    let valid_event = |id: i32| -> Result<()> {
        ensure!(
            id == -1 || (id >= 0 && (id as usize) < event_count),
            "Invalid state-machine event {id}"
        );
        Ok(())
    };
    let mut machines = BTreeMap::new();
    for (id, obj) in &graph.objects {
        if obj.klass != "hkbStateMachine" {
            continue;
        }
        let mut fields = obj.fields.clone();
        if let Some(chooser) = fields.as_object_mut().unwrap().remove("startStateChooser") {
            ensure!(
                chooser.is_null(),
                "Custom start-state choosers are not implemented: {id}"
            );
        }
        let mut settings: Settings = serde_json::from_value(fields)?;
        if graph.contents_version()==Some("hk_2012.1.0-r1") {
            // PC reflected enum 1228d38 and getStartState 65bb70:
            // 0 DEFAULT, 1 SYNC, 2 RANDOM, 3 CHOOSER. Havok 7 has
            // DEFAULT, SYNC-once, SYNC, RANDOM instead. Never interpret
            // 2012's SYNC as the older retained-state activation mode.
            settings.start_state_mode=match settings.start_state_mode {
                0=>0,1=>2,2=>3,
                3=>anyhow::bail!("Custom 2012 start-state chooser execution is not implemented: {id}"),
                mode=>anyhow::bail!("Unknown 2012 state-machine start mode {mode}"),
            };
            ensure!(settings.self_transition_mode<=2,"Unknown 2012 self-transition mode");
        }
        ensure!(
            settings.start_state_mode <= 3 && settings.self_transition_mode <= 3,
            "Unknown state-machine mode"
        );
        ensure!(
            settings.sync_variable_index == -1
                || (settings.sync_variable_index >= 0
                    && (settings.sync_variable_index as usize) < variable_count),
            "Invalid sync variable"
        );
        for event in [
            settings.event_to_send_when_state_or_transition_changes.id,
            settings.return_to_previous_state_event_id,
            settings.random_transition_event_id,
            settings.transition_to_next_higher_state_event_id,
            settings.transition_to_next_lower_state_event_id,
        ] {
            valid_event(event)?;
        }
        let mut states = vec![];
        let mut state_indices = BTreeMap::new();
        let mut references = HashSet::new();
        for reference in &settings.states {
            ensure!(
                references.insert(&reference.id),
                "Duplicate state reference"
            );
            let mut fields = object(graph, reference, "hkbStateMachineStateInfo")?.clone();
            let mut notification_lists = Vec::new();
            for (array_name, single_name) in [
                ("enterNotifyEvents", "enterNotifyEvent"),
                ("exitNotifyEvents", "exitNotifyEvent"),
            ] {
                let mut events: Vec<Event> = Vec::new();
                if let Some(array) = fields.as_object_mut().unwrap().remove(array_name) {
                    ensure!(
                        !fields.as_object().unwrap().contains_key(single_name),
                        "Ambiguous state notification layout"
                    );
                    if !array.is_null() {
                        let reference: Reference = serde_json::from_value(array)?;
                        events = serde_json::from_value(
                            object(graph, &reference, "hkbStateMachineEventPropertyArray")?
                                ["events"]
                                .clone(),
                        )?;
                    }
                    fields[single_name] = serde_json::json!({"id": -1, "payload": null});
                }
                for event in &events {
                    valid_event(event.id)?;
                }
                notification_lists.push(events);
            }
            let exit_notify_events = notification_lists.pop().unwrap();
            let enter_notify_events = notification_lists.pop().unwrap();
            let info: StateInfo = serde_json::from_value(fields)?;
            ensure!(
                state_indices.insert(info.state_id, states.len()).is_none(),
                "Duplicate state id in {}",
                settings.name
            );
            ensure!(
                info.probability.is_finite() && info.probability >= 0.,
                "Invalid state probability"
            );
            valid_event(info.enter_notify_event.id)?;
            valid_event(info.exit_notify_event.id)?;
            states.push(State {
                enter_notify_events,
                exit_notify_events,
                transitions: transitions(graph, &info.transitions)?,
                info,
            });
        }
        let wildcard_transitions = transitions(graph, &settings.wildcard_transitions)?;
        for t in states
            .iter()
            .flat_map(|s| &s.transitions)
            .chain(&wildcard_transitions)
        {
            ensure!(t.flags & !0x7fff == 0, "Unknown transition flags");
            ensure!(
                state_indices.contains_key(&t.to_state_id),
                "Transition target {} absent in {}",
                t.to_state_id,
                settings.name
            );
            valid_event(t.event_id)?;
            for interval in [&t.trigger_interval, &t.initiate_interval] {
                valid_event(interval.enter_event_id)?;
                valid_event(interval.exit_event_id)?;
                ensure!(
                    interval.enter_time.is_finite() && interval.exit_time.is_finite(),
                    "Non-finite transition interval"
                );
            }
        }
        machines.insert(
            id.clone(),
            Machine {
                settings,
                states,
                wildcard_transitions,
                state_indices,
            },
        );
    }
    Ok(machines)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn compiles_all_retail_machines_without_flattening_nested_rules() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let machines = compile(&graph)?;
        assert_eq!(machines.len(), 24);
        assert_eq!(
            machines.values().map(|m| m.states.len()).sum::<usize>(),
            109
        );
        let root = &machines["2:240"];
        assert_eq!(root.settings.start_state_id, 0);
        assert_eq!(
            root.states[root.state_indices[&5]].info.name,
            "RunWalkState"
        );
        let idle = &root.states[root.state_indices[&0]];
        assert_eq!(idle.transitions[0].event_id, 6);
        assert_eq!(idle.transitions[0].to_state_id, 5);
        assert_eq!(idle.transitions[0].condition.as_ref().unwrap().id, "2:1120");
        assert!(root
            .wildcard_transitions
            .iter()
            .any(|t| t.flags & 8192 != 0));
        Ok(())
    }
}
