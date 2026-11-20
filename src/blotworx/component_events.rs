//! GameEventsComponent stimulus listener, retail REL31f6a0..31f808.
//! Registry traversal and child-source remapping are not yet native-equivalent.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub entity: usize,
    pub script: String,
    pub audio: String,
    pub effect: String,
    pub action: Option<u32>,
}
struct Listener {
    entity: usize,
    started: bool,
    types: Vec<u32>,
    aliases: Vec<String>,
    delays: Vec<f32>,
    scripts: Vec<String>,
    audio: Vec<String>,
    effects: Vec<String>,
    actions: Vec<u32>,
}
struct Pending {
    time: f32,
    action: Action,
}
#[derive(Default)]
pub struct Runtime {
    listeners: HashMap<usize, Vec<Listener>>,
    owners: Vec<usize>,
    pending: Vec<Pending>,
    pub deliveries: u64,
}

/// Presence tests, not positive effective amounts. Each category occurs once
/// even when a grouped event contains duplicate stimulus IDs.
pub fn stimulus_categories(types: &[u32]) -> Vec<u32> {
    let mut categories = vec![9];
    for (stimulus, category) in [(0, 11), (1, 10), (3, 12), (11, 25)] {
        if types.contains(&stimulus) {
            categories.push(category);
        }
    }
    categories
}
impl Runtime {
    pub fn bind(&mut self, entity: usize, component: &Component) -> Result<()> {
        ensure!(
            component.class_name == "GameEventsComponent",
            "Not a GameEventsComponent"
        );
        let array = |name: &str| -> Result<Vec<serde_json::Value>> {
            let mut found = component.properties.iter().filter(|p| p.name == name);
            let Some(property) = found.next() else {
                return Ok(vec![]);
            }; // Native optional count starts at zero.
            ensure!(
                found.next().is_none(),
                "Duplicate GameEvents property {name}"
            );
            property
                .value
                .as_array()
                .cloned()
                .with_context(|| format!("Invalid GameEvents array {name}"))
        };
        let strings = |name: &str| -> Result<Vec<String>> {
            array(name)?
                .into_iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .with_context(|| format!("Invalid GameEvents string in {name}"))
                })
                .collect()
        };
        let types = array("GameEventTypes")?
            .into_iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .context("Invalid GameEventTypes entry")
            })
            .collect::<Result<Vec<_>>>()?;
        let delays = array("EventDelays")?
            .into_iter()
            .map(|v| {
                let delay = v.as_f64().context("Invalid EventDelays entry")? as f32;
                ensure!(delay.is_finite(), "Non-finite event delay");
                Ok(delay)
            })
            .collect::<Result<Vec<_>>>()?;
        let actions = array("ActionEventNames")?
            .into_iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .context("Invalid ActionEventNames integer entry")
            })
            .collect::<Result<Vec<_>>>()?;
        let listener = Listener {
            entity,
            started: false,
            types,
            delays,
            aliases: strings("EventSourceAliases")?,
            scripts: strings("ScriptFunctionNames")?,
            audio: strings("AudioEventNames")?,
            effects: strings("EffectEventNames")?,
            actions,
        };
        self.listeners.entry(entity).or_insert_with(|| {
            self.owners.push(entity);
            Vec::new()
        }).push(listener);
        Ok(())
    }
    /// REL31e628..31e694 writes byte3c before category3, then drains
    /// that component's delayed queue. The caller controls owner scheduling.
    pub fn update(&mut self, target: usize, time: f32) -> Result<Vec<Action>> {
        ensure!(time.is_finite(), "Invalid gameplay event time");
        let mut immediate = Vec::new();
        for listener in self.listeners.get_mut(&target).into_iter().flatten() {
            if !listener.started {
                listener.started = true;
                Self::receive_listener(listener, 3, "", time, &mut self.pending, &mut immediate);
            }
        }
        self.deliveries += immediate.len() as u64;
        immediate.extend(self.take_ready(time, Some(target)));
        Ok(immediate)
    }
    pub fn owners(&self) -> Vec<usize> {
        self.owners.clone()
    }
    pub fn receive_stimuli(
        &mut self,
        target: usize,
        types: &[u32],
        tag: &str,
        time: f32,
    ) -> Result<Vec<Action>> {
        self.receive_categories(target, &stimulus_categories(types), tag, time)
    }
    /// REL31f518 maps health event0x10 to category1 with an empty alias.
    pub fn receive_damage(&mut self, target: usize, time: f32) -> Result<Vec<Action>> {
        self.receive_categories(target, &[1], "", time)
    }
    /// Animation feedback31f43c invokes0/2 with the original empty alias.
    pub fn receive_footstep(
        &mut self,
        target: usize,
        variant: bool,
        time: f32,
    ) -> Result<Vec<Action>> {
        self.receive_categories(target, &[if variant { 2 } else { 0 }], "", time)
    }
    /// REL323f08 broadcasts0x35(category24); actions1/2 also broadcast0x11(category5).
    pub fn receive_zero_health(
        &mut self,
        target: usize,
        death_event: bool,
        time: f32,
    ) -> Result<Vec<Action>> {
        self.receive_categories(target, if death_event { &[24, 5] } else { &[24] }, "", time)
    }
    pub fn remove(&mut self, entity: usize) {
        if self.listeners.remove(&entity).is_some() {
            self.owners.retain(|&owner| owner != entity);
        }
        self.pending.retain(|event| event.action.entity != entity);
    }
    fn receive_categories(
        &mut self,
        target: usize,
        categories: &[u32],
        tag: &str,
        time: f32,
    ) -> Result<Vec<Action>> {
        ensure!(time.is_finite(), "Invalid gameplay event time");
        let mut immediate = Vec::new();
        if let Some(listeners) = self.listeners.get(&target) {
            for &category in categories {
                for listener in listeners {
                    Self::receive_listener(listener, category, tag, time, &mut self.pending, &mut immediate);
                }
            }
        }
        self.deliveries += immediate.len() as u64;
        Ok(immediate)
    }
    fn receive_listener(
        listener: &Listener,
        category: u32,
        tag: &str,
        time: f32,
        pending: &mut Vec<Pending>,
        immediate: &mut Vec<Action>,
    ) {
        for (index, &kind) in listener.types.iter().enumerate() {
            if kind != category {
                continue;
            }
            let alias = listener.aliases.get(index).map(String::as_str).unwrap_or("");
            if !alias.is_empty() && alias != tag {
                continue;
            }
            let entry = |values: &[String]| values.get(index).cloned().unwrap_or_default();
            let action = Action {
                entity: listener.entity,
                script: entry(&listener.scripts),
                audio: entry(&listener.audio),
                effect: entry(&listener.effects),
                action: listener.actions.get(index).copied(),
            };
            let delay = listener.delays.get(index).copied().unwrap_or(0.);
            if delay > 0. {
                pending.push(Pending { time: time + delay, action });
            } else {
                immediate.push(action);
            }
        }
    }
    pub fn ready(&mut self, time: f32) -> Vec<Action> {
        self.take_ready(time, None)
    }
    fn take_ready(&mut self, time: f32, owner: Option<usize>) -> Vec<Action> {
        let mut ready = Vec::new();
        self.pending.retain(|pending| {
            if pending.time <= time && owner.is_none_or(|owner| owner == pending.action.entity) {
                ready.push(pending.action.clone());
                false
            } else {
                true
            }
        });
        self.deliveries += ready.len() as u64;
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn first_update_component() -> Result<Component> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut c = crate::component_schema::CATALOG.components["GameEventsComponent"]
            .instantiate("GameEventsComponent", &scene)?;
        for p in &mut c.properties {
            p.value = match p.name.as_str() {
                "GameEventTypes" => serde_json::json!([3, 1]),
                "EventSourceAliases" => serde_json::json!(["", ""]),
                "EventDelays" => serde_json::json!([0, 0.5]),
                "ScriptFunctionNames" => serde_json::json!(["FirstUpdate", "DelayedDamage"]),
                _ => p.value.clone(),
            };
        }
        Ok(c)
    }
    #[test]
    fn first_update_matches_original_flag_branch_and_precedes_delayed_queue() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/component_first_update.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut runtime = Runtime::default();
            runtime.bind(4, &first_update_component()?)?;
            runtime.listeners.get_mut(&4).unwrap()[0].started = case["initial_flag"].as_u64().unwrap() != 0;
            assert!(runtime.receive_damage(4, 0.)?.is_empty());
            let actions = runtime.update(4, 1.)?;
            let mut expected = case["result"]["trace"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["call"] == "category")
                .map(|call| {
                    assert_eq!(call["first_update_flag"], 1);
                    assert_eq!(call["category"], 3);
                    "FirstUpdate"
                })
                .collect::<Vec<_>>();
            expected.push("DelayedDamage");
            assert_eq!(
                actions
                    .iter()
                    .map(|a| a.script.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(runtime.listeners[&4][0].started);
            assert!(
                runtime.update(4, 1.)?.is_empty(),
                "Re-entry must not repeat first update"
            );
            assert!(runtime.update(4, 2.)?.is_empty());
        }
        Ok(())
    }
    #[test]
    fn first_update_is_per_registration_and_owner_delays_do_not_leak() -> Result<()> {
        let mut runtime = Runtime::default();
        let component = first_update_component()?;
        runtime.bind(4, &component)?;
        runtime.bind(9, &component)?;
        assert_eq!(runtime.owners(), vec![4, 9]);
        runtime.receive_damage(9, 0.)?;
        assert_eq!(
            runtime
                .update(4, 1.)?
                .iter()
                .map(|a| a.script.as_str())
                .collect::<Vec<_>>(),
            vec!["FirstUpdate"]
        );
        let actions = runtime.update(9, 1.)?;
        assert_eq!(
            actions
                .iter()
                .map(|a| a.script.as_str())
                .collect::<Vec<_>>(),
            vec!["FirstUpdate", "DelayedDamage"]
        );
        runtime.remove(4);
        runtime.bind(4, &component)?;
        assert_eq!(
            runtime.update(4, 1.)?.len(),
            1,
            "New entity registration needs its own first update"
        );
        assert!(runtime.update(9, 1.)?.is_empty());
        Ok(())
    }
    #[test]
    fn grouped_stimuli_route_presence_in_original_order() {
        assert_eq!(stimulus_categories(&[3, 5, 8]), vec![9, 12]);
        assert_eq!(
            stimulus_categories(&[1, 0, 1, 11, 3]),
            vec![9, 11, 10, 12, 25]
        );
    }
    #[test]
    fn aliases_delay_and_short_optional_arrays_do_not_drop_callbacks() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut c = crate::component_schema::CATALOG.components["GameEventsComponent"]
            .instantiate("GameEventsComponent", &scene)?;
        for p in &mut c.properties {
            p.value = match p.name.as_str() {
                "GameEventTypes" => serde_json::json!([9, 10, 10]),
                "EventSourceAliases" => serde_json::json!(["", "Brush", "Other"]),
                "EventDelays" => serde_json::json!([0, 0.5]),
                "ScriptFunctionNames" => serde_json::json!(["Any", "Paint", "Wrong"]),
                "ActionEventNames" => serde_json::json!([0, 3]),
                _ => p.value.clone(),
            };
        }
        let mut runtime = Runtime::default();
        runtime.bind(4, &c)?;
        assert!(runtime.receive_stimuli(5, &[1], "Brush", 0.)?.is_empty());
        let immediate = runtime.receive_stimuli(4, &[1, 1], "Brush", 1.)?;
        assert_eq!(
            immediate
                .iter()
                .map(|a| a.script.as_str())
                .collect::<Vec<_>>(),
            vec!["Any"]
        );
        assert_eq!(immediate[0].action, Some(0));
        assert!(runtime.ready(1.49).is_empty());
        let delayed = runtime.ready(1.5);
        assert_eq!(delayed[0].script, "Paint");
        assert_eq!(delayed[0].action, Some(3));
        assert!(runtime.ready(2.).is_empty());
        assert_eq!(runtime.deliveries, 2);
        Ok(())
    }
}
