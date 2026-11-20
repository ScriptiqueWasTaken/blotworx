//! Retail Lua184ce0/184f20: synchronous ordinary actions11/12. These are
//! separate from input restriction and camera ownership. Full AI/controller
//! receiver construction and selection remain incomplete.
use super::{
    action_event_dispatch::{Admission, EventRegistry, Host as EventHost},
    action_event_queue::ActionEvent,
    assets::World,
    entity_events::Event,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Enter,
    Exit,
}
pub trait Host {
    fn send_action(&mut self, entity: usize, action: u32) -> Result<()>;
    fn has_animation_controller(&mut self, entity: usize) -> Result<bool>;
    fn write_cutscene_variable(&mut self, entity: usize, value: i32) -> Result<()>;
    fn send_animation_event(&mut self, entity: usize, event: u32) -> Result<()>;
}
pub fn execute(
    command: Command,
    entity: Option<usize>,
    variant: i32,
    host: &mut impl Host,
) -> Result<()> {
    let Some(entity) = entity else { return Ok(()) };
    host.send_action(entity, if command == Command::Enter { 11 } else { 12 })?;
    //184ea4 checks the -1 sentinel only after ordinary dispatch. A missing
    // controller declines both operations; an unmapped variable does not.
    if command == Command::Enter && variant != -1 && host.has_animation_controller(entity)? {
        host.write_cutscene_variable(entity, variant)?;
        host.send_animation_event(entity, 0x90)?;
    }
    Ok(())
}

pub struct State {
    listeners: BTreeMap<u32, usize>,
    next_listener: u32,
    pub requests: u64,
    pub receiver_calls: u64,
    pub counter_receiver_calls:u64,
    pub story_start_actions:u64,
    pub story_finish_actions:u64,
    pub last_counter_event:Option<super::entity_events::CounterStatus>,
    pub last: Option<ActionEvent>,
    pub last_receivers: Vec<usize>,
    pub last_admission: Option<Admission>,
    pub queries: u64,
    pub last_query_entity: Option<usize>,
    pub last_query_value: Option<bool>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            listeners: BTreeMap::new(),
            next_listener: 2,
            requests: 0,
            receiver_calls: 0,
            counter_receiver_calls:0,
            story_start_actions:0,
            story_finish_actions:0,
            last_counter_event:None,
            last: None,
            last_receivers: vec![],
            last_admission: None,
            queries: 0,
            last_query_entity: None,
            last_query_value: None,
        }
    }
}
impl State {
    pub(super) fn listener_owner(&self,listener:u32)->Option<usize> {self.listeners.get(&listener).copied()}
    pub(super) fn bind_robot_arm(&mut self,owner:usize,registry:&mut EventRegistry<Event>)->Result<()> {
        self.bind_animation_task(owner,registry)?;
        let listener=self.listeners.iter().find_map(|(&id,&entity)|(entity==owner).then_some(id)).context("Arm listener missing")?;
        registry.register_for(0x0d,listener,Some(owner))?;Ok(())
    }

    pub(super) fn animation_task_bound(&self,owner:usize,registry:&EventRegistry<Event>)->bool {
        self.listeners.iter().any(|(&listener,&entity)|entity==owner && registry.registered_node_for(9,listener))
    }
    pub(super) fn bind_animation_task(&mut self,owner:usize,registry:&mut EventRegistry<Event>)->Result<()> {
        let existing=self.listeners.iter().find_map(|(&listener,&entity)|(entity==owner).then_some(listener));
        let listener=if let Some(listener)=existing {listener} else {
            let listener=self.allocate_listener()?;self.listeners.insert(listener,owner);listener
        };
        //A fresh local subscription calls2de6c0 even for an existing node,
        //which can reactivate that node after global deactivation.
        registry.register_for(9,listener,Some(owner))?;
        Ok(())
    }
    pub(super) fn dispatch_story_conversation(
        &mut self, owner: usize, notification: super::ai_conversation::Notification,
        registry: &mut EventRegistry<Event>,
        brains: &mut BTreeMap<usize, super::ai_behavior_events::Brain>,
    ) -> Result<()> {
        let Some(event) = super::ai_conversation::notification_event(Some(owner), notification) else {
            return Ok(());
        };
        let action = event.action;
        self.dispatch(event, registry, brains)?;
        if action == 21 { self.story_start_actions = self.story_start_actions.wrapping_add(1); }
        else { self.story_finish_actions = self.story_finish_actions.wrapping_add(1); }
        Ok(())
    }
    pub(super) fn allocate_listener(&mut self)->Result<u32> {
        let listener=self.next_listener;
        self.next_listener=listener.checked_add(1).context("Event listener identities exhausted")?;
        Ok(listener)
    }
    /// ScriptedIdle1b8a54 and Paused1b87dc request own-source kind0c through
    /// 198ca8 ->183a84 ->2de6c0. Listener IDs are stable portable identities;
    /// reproducing native pointer hash/constructor order is still required.
    pub fn bind(
        &mut self,
        owner: usize,
        brain: &super::ai_behavior_events::Brain,
        registry: &mut EventRegistry<Event>,
    ) -> Result<()> {
        if brain.paused.is_empty() && brain.scripted_idle.is_empty() && brain.scripted_move.is_empty() && brain.patrol.is_empty() && brain.npc_puddle.is_empty() && brain.conversations.is_empty() {
            return Ok(());
        }
        ensure!(
            !self.listeners.values().any(|&entity| entity == owner),
            "Duplicate brain action listener"
        );
        let listener = self.next_listener;
        self.next_listener = listener
            .checked_add(1)
            .context("Action listener identities exhausted")?;
        if !brain.paused.is_empty() || !brain.scripted_idle.is_empty() || !brain.scripted_move.is_empty() || !brain.patrol.is_empty() || !brain.conversations.is_empty() {
            registry.register_for(0x0c, listener, Some(owner))?;
        }
        // NPCPuddle constructor68e48 ->183b8c requests own-source kind75.
        if !brain.npc_puddle.is_empty() {registry.register_for(0x75,listener,Some(owner))?;}
        // Conversation1b6c60 subscribes to ordinary actions AND line events.
        // Kind81 payload handling remains explicit unsupported, not a no-op.
        if !brain.conversations.is_empty() {registry.register_for(0x81,listener,Some(owner))?;}
        self.listeners.insert(listener, owner);
        Ok(())
    }
    pub fn retire(&mut self, owner: usize, registry: &mut EventRegistry<Event>) {
        self.listeners.retain(|&listener, entity| {
            if *entity == owner {
                registry.deactivate_for(0x0c, listener);
                registry.deactivate_for(0x75, listener);
                registry.deactivate_for(0x81, listener);
                registry.deactivate_for(9, listener);
                registry.deactivate_for(0x0d,listener);
                false
            } else {
                true
            }
        });
    }
    pub(super) fn receive_deferred(&mut self,listener:u32,event:&Event,brains:&mut BTreeMap<usize,super::ai_behavior_events::Brain>,resolve:&mut dyn FnMut(usize)->Result<Option<[u32;4]>>)->Result<()> {
        self.receive_deferred_with_patrol(listener,event,brains,resolve,&mut |_|anyhow::bail!("Patrol departure requires the shared event queue"))
    }
    pub(super) fn receive_deferred_with_patrol(&mut self,listener:u32,event:&Event,brains:&mut BTreeMap<usize,super::ai_behavior_events::Brain>,resolve:&mut dyn FnMut(usize)->Result<Option<[u32;4]>>,
        notify:&mut dyn FnMut(super::entity_events::Basic)->Result<()>)->Result<()> {
        let owner=*self.listeners.get(&listener).context("Deferred AI listener has no constructed owner")?;
        let brain=brains.get_mut(&owner).context("Deferred AI owner was retired without removing its listener")?;
        match event {
            Event::Animation(event)=>{brain.receive_animation_feedback(owner,event)?;}
            Event::Action(event)=>{brain.receive_action_with_patrol(owner,event,resolve,notify)?;}
            Event::CounterStatus(event)=>{
                brain.receive_counter_status(owner,event);
                self.counter_receiver_calls=self.counter_receiver_calls.wrapping_add(1);
                self.last_counter_event=Some(event.clone());
            }
            _=>anyhow::bail!("AI listener received an unimplemented payload"),
        }
        self.receiver_calls=self.receiver_calls.wrapping_add(1);
        self.last_receivers.push(owner);
        Ok(())
    }
    pub fn dispatch(
        &mut self,
        event: ActionEvent,
        registry: &mut EventRegistry<Event>,
        brains: &mut BTreeMap<usize, super::ai_behavior_events::Brain>,
    ) -> Result<()> {
        self.dispatch_resolved(event,registry,brains,&mut |_|anyhow::bail!("Ordinary ScriptedMove reference requires a world position resolver"))
    }
    pub fn dispatch_resolved(
        &mut self, event:ActionEvent, registry:&mut EventRegistry<Event>,
        brains:&mut BTreeMap<usize,super::ai_behavior_events::Brain>,
        resolve:&mut dyn FnMut(usize)->Result<Option<[u32;4]>>,
    )->Result<()> {
        self.dispatch_resolved_with_patrol(event,registry,brains,resolve,&mut |_|anyhow::bail!("Patrol departure requires the shared event queue"))
    }
    pub fn dispatch_resolved_with_patrol(
        &mut self,event:ActionEvent,registry:&mut EventRegistry<Event>,
        brains:&mut BTreeMap<usize,super::ai_behavior_events::Brain>,
        resolve:&mut dyn FnMut(usize)->Result<Option<[u32;4]>>,
        notify:&mut dyn FnMut(super::entity_events::Basic)->Result<()>,
    )->Result<()> {
        struct Receivers<'a> {
            listeners: &'a BTreeMap<u32, usize>,
            brains: &'a mut BTreeMap<usize, super::ai_behavior_events::Brain>,
            count: &'a mut u64,
            last: &'a mut Vec<usize>,
            resolve: &'a mut dyn FnMut(usize)->Result<Option<[u32;4]>>,
            notify: &'a mut dyn FnMut(super::entity_events::Basic)->Result<()>,
        }
        impl EventHost<Event> for Receivers<'_> {
            fn receive(
                &mut self,
                _: &mut EventRegistry<Event>,
                listener: u32,
                event: &Event,
            ) -> Result<()> {
                let owner = *self
                    .listeners
                    .get(&listener)
                    .context("Ordinary action listener has no concrete receiver")?;
                let Event::Action(event) = event else {
                    anyhow::bail!("Brain action binding received another event kind")
                };
                let brain = self
                    .brains
                    .get_mut(&owner)
                    .context("Ordinary action brain was retired without removing its listener")?;
                brain.receive_action_with_patrol(owner,event,self.resolve,self.notify)?;
                *self.count = self.count.wrapping_add(1);
                self.last.push(owner);
                Ok(())
            }
        }
        self.requests = self.requests.wrapping_add(1);
        self.last = Some(event.clone());
        self.last_receivers.clear();
        self.last_admission = None;
        let result = registry.dispatch(
            &event.into(),
            None,
            &mut Receivers {
                listeners: &self.listeners,
                brains,
                count: &mut self.receiver_calls,
                last: &mut self.last_receivers,
                resolve,
                notify,
            },
        )?;
        self.last_admission = Some(result);
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"requests":self.requests,"receiver_calls":self.receiver_calls,"counter_receiver_calls":self.counter_receiver_calls,"last_counter_event":self.last_counter_event,"last":self.last,
            "story_start_actions":self.story_start_actions,"story_finish_actions":self.story_finish_actions,
            "queries":self.queries,"last_query":{"entity":self.last_query_entity,"value":self.last_query_value},
            "last_receivers":self.last_receivers,"last_admitted":self.last_admission.as_ref().map(|value|*value==Admission::Delivered),
            "known_brain_listeners":self.listeners.len(),"complete_receiver_construction":false,
            "full_behavior_selection_and_controller_output":false,"native_pointer_hash_order_verified":false})
    }
}

impl Host for World {
    fn send_action(&mut self, entity: usize, action: u32) -> Result<()> {
        self.cutscene_actions.dispatch(
            ActionEvent {
                source: Some(entity),
                header: None,
                action,
                vector_bits: [0; 4],
                reference: None,
            },
            &mut self.entity_event_registry,
            &mut self.ai_brains,
        )?;
        self.refresh_demo_npc_behavior(entity)?;
        let limitation="Ordinary actions reach known AI component/Paused/ScriptedIdle/Conversation receiver stages. Full root/child forwarding, behavior selection/controller output, other subscription constructors and native pointer hash order remain incomplete.";
        if !self
            .report
            .unsupported
            .iter()
            .any(|entry| entry == limitation)
        {
            self.report.unsupported.push(limitation.into());
        }
        Ok(())
    }
    fn has_animation_controller(&mut self, entity: usize) -> Result<bool> {
        let count = self.entities[entity]
            .components
            .iter()
            .filter(|c| c.class_name == "AnimationComponent")
            .count();
        ensure!(
            count <= 1,
            "Cutscene animation controller requires native component-slot arbitration"
        );
        Ok(count == 1)
    }
    fn write_cutscene_variable(&mut self, entity: usize, value: i32) -> Result<()> {
        self.write_animation_variable(entity, 24, 1, value as u32)?;
        Ok(())
    }
    fn send_animation_event(&mut self, entity: usize, event: u32) -> Result<()> {
        self.send_native_actor_animation_event(entity, event)?;
        Ok(())
    }
}
impl World {
    pub(super) fn cutscene_action_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if name == "IsInCutscene" {
            if args.len() != 1 {
                self.report.warnings.push(format!("{name}: native argument count diagnostic ({} arguments); continued with source stack order",args.len()));
            }
            let entity = self
                .scripts
                .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
                .filter(|entity| !self.removed_entities.contains(entity));
            //2d5918 resolves the AIBrain component;2e0238 reads its byte48.
            //278408 ->3f6aec pushes one Lua boolean, including absent targets.
            let value = match entity {
                Some(entity) => {
                    let count = self.entities[entity]
                        .components
                        .iter()
                        .filter(|component| component.class_name == "AIBrainComponent")
                        .count();
                    if count == 0 {
                        false
                    } else {
                        if count != 1 {
                            return Err(mlua::Error::external(
                                "Cutscene query requires native AI component-slot arbitration",
                            ));
                        }
                        self.ai_brains
                            .get(&entity)
                            .ok_or_else(|| {
                                mlua::Error::external(
                                    "Cutscene query AI component was not initialized",
                                )
                            })?
                            .motion_suspended
                    }
                }
                None => false,
            };
            self.cutscene_actions.queries = self.cutscene_actions.queries.wrapping_add(1);
            self.cutscene_actions.last_query_entity = entity;
            self.cutscene_actions.last_query_value = Some(value);
            return Ok(mlua::MultiValue::from_vec(vec![mlua::Value::Boolean(
                value,
            )]));
        }
        let command = match name {
            "EnterCutscene" => Command::Enter,
            "ExitCutscene" => Command::Exit,
            _ => return Err(mlua::Error::external("Unknown cutscene callback")),
        };
        let count = args.len();
        if (command == Command::Enter && count < 1) || (command == Command::Exit && count != 1) {
            // Original callbacks diagnose count but continue consuming the top
            // argument; never turn that diagnostic into a new script exception.
            self.report.warnings.push(format!("{name}: native argument count diagnostic ({count} arguments); continued with source stack order"));
        }
        let variant = if command == Command::Enter && count > 1 {
            super::score_state::integer_argument(lua, args.pop_back().unwrap())?
        } else {
            -1
        };
        let entity = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|entity| !self.removed_entities.contains(entity));
        execute(command, entity, variant, self).map_err(mlua::Error::external)?;
        Ok(mlua::MultiValue::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_task_subscription_reactivates_shared_animation_listener_and_retirement_clears_it()->Result<()> {
        let mut state=State::default();
        let mut registry=EventRegistry::<Event>::default();
        state.bind_animation_task(7,&mut registry)?;
        let listener=*state.listeners.keys().next().unwrap();
        assert!(registry.contains_for(9,listener,Some(7))?);
        registry.deactivate_for(9,listener);
        assert!(!registry.contains_for(9,listener,Some(7))?);
        state.bind_animation_task(7,&mut registry)?;
        assert_eq!(state.listeners.len(),1);
        assert!(registry.contains_for(9,listener,Some(7))?);
        assert!(!registry.contains_for(9,listener,Some(8))?);
        state.retire(7,&mut registry);
        assert!(!registry.contains_for(9,listener,Some(7))?);
        Ok(())
    }
    #[derive(Default)]
    struct Recorder {
        log: Vec<String>,
        controller: bool,
        fail: Option<&'static str>,
    }
    impl Recorder {
        fn push(&mut self, value: String) -> Result<()> {
            self.log.push(value.clone());
            ensure!(self.fail != Some(value.as_str()), "Receiver failed");
            Ok(())
        }
    }
    impl Host for Recorder {
        fn send_action(&mut self, entity: usize, action: u32) -> Result<()> {
            self.push(format!("action:{entity}:{action}"))
        }
        fn has_animation_controller(&mut self, entity: usize) -> Result<bool> {
            self.push(format!("controller:{entity}"))?;
            Ok(self.controller)
        }
        fn write_cutscene_variable(&mut self, entity: usize, value: i32) -> Result<()> {
            self.push(format!("variable:{entity}:{value}"))
        }
        fn send_animation_event(&mut self, entity: usize, event: u32) -> Result<()> {
            self.push(format!("animation:{entity}:{event}"))
        }
    }
    #[test]
    fn enter_and_exit_preserve_native_order_and_optional_controller_gate() -> Result<()> {
        let mut h = Recorder {
            controller: true,
            ..Default::default()
        };
        execute(Command::Enter, Some(4), 3, &mut h)?;
        assert_eq!(
            h.log,
            [
                "action:4:11",
                "controller:4",
                "variable:4:3",
                "animation:4:144"
            ]
        );
        h.log.clear();
        execute(Command::Exit, Some(4), 3, &mut h)?;
        assert_eq!(h.log, ["action:4:12"]);
        h.log.clear();
        execute(Command::Enter, Some(4), -1, &mut h)?;
        assert_eq!(h.log, ["action:4:11"]);
        h.log.clear();
        h.controller = false;
        execute(Command::Enter, Some(4), 2, &mut h)?;
        assert_eq!(h.log, ["action:4:11", "controller:4"]);
        h.log.clear();
        execute(Command::Enter, None, 2, &mut h)?;
        assert!(h.log.is_empty());
        Ok(())
    }
    #[test]
    fn receiver_failure_keeps_prior_effects_and_stops_later_operations() {
        let mut h = Recorder {
            controller: true,
            fail: Some("variable:4:2"),
            ..Default::default()
        };
        assert!(execute(Command::Enter, Some(4), 2, &mut h).is_err());
        assert_eq!(h.log, ["action:4:11", "controller:4", "variable:4:2"]);
    }
    #[test]
    fn callback_sequence_matches_original_instruction_fixture() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/cutscene_actions.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let command = if case["command"] == "enter" {
                Command::Enter
            } else {
                Command::Exit
            };
            let mut h = Recorder {
                controller: case["controller"].as_bool().unwrap(),
                ..Default::default()
            };
            execute(
                command,
                case["entity_found"].as_bool().unwrap().then_some(4),
                case["variant"].as_i64().unwrap() as i32,
                &mut h,
            )?;
            let expected = case["trace"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| match entry["op"].as_str().unwrap() {
                    "action" => {
                        assert_eq!(entry["kind"], 12);
                        assert_eq!(entry["source_word"], 5);
                        assert_eq!(entry["header_word"], 0);
                        assert_eq!(entry["vector_bits"], serde_json::json!([0, 0, 0, 0]));
                        assert_eq!(entry["reference_word"], 0);
                        format!("action:4:{}", entry["action"].as_u64().unwrap())
                    }
                    "controller" => "controller:4".into(),
                    "variable" => {
                        assert_eq!(entry["id"], 24);
                        format!(
                            "variable:4:{}",
                            entry["bits"].as_u64().unwrap() as u32 as i32
                        )
                    }
                    "animation" => format!("animation:4:{}", entry["id"].as_u64().unwrap()),
                    _ => panic!("Unknown native fixture operation"),
                })
                .collect::<Vec<_>>();
            assert_eq!(h.log, expected, "{case}");
        }
        Ok(())
    }
    #[test]
    fn ordinary_action_subscription_filters_live_owners_and_retirement() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut component = crate::component_schema::CATALOG.components["AIBrainComponent"]
            .instantiate("AIBrainComponent", &scene)?;
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Behavior List")
            .unwrap()
            .value = serde_json::json!(["Paused", "ScriptedIdle"]);
        let mut brains = BTreeMap::from([
            (
                4,
                super::super::ai_behavior_events::Brain::from_component(&component)?,
            ),
            (
                7,
                super::super::ai_behavior_events::Brain::from_component(&component)?,
            ),
        ]);
        let mut registry = EventRegistry::default();
        let mut state = State::default();
        for (&owner, brain) in &brains {
            state.bind(owner, brain, &mut registry)?;
        }
        let event = |source, action| ActionEvent {
            source: Some(source),
            header: None,
            action,
            vector_bits: [0; 4],
            reference: None,
        };
        state.dispatch(event(4, 11), &mut registry, &mut brains)?;
        assert!(brains[&4].motion_suspended && brains[&4].reevaluation_requested);
        assert!(!brains[&7].motion_suspended);
        assert_eq!(state.last_receivers, [4]);
        brains.get_mut(&4).unwrap().reevaluation_requested = false;
        state.dispatch(event(4, 12), &mut registry, &mut brains)?;
        assert!(!brains[&4].motion_suspended && !brains[&4].reevaluation_requested);
        state.dispatch(event(7, 13), &mut registry, &mut brains)?;
        assert!(brains[&7].paused.values().all(|p| p.active));
        state.retire(4, &mut registry);
        brains.remove(&4);
        state.dispatch(event(4, 11), &mut registry, &mut brains)?;
        assert!(state.last_receivers.is_empty());
        assert!(state.bind(7, &brains[&7], &mut registry).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_cutscene_lua_delivers_actions_and_variant_to_live_graph_without_input_alias(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .ai_brains
            .iter()
            .find(|(_, brain)| {
                brain.ai_type.starts_with("Gremlin") && !brain.scripted_idle.is_empty()
            })
            .map(|(&owner, _)| owner)
            .context("Original Gremlin ScriptedIdle absent")?;
        let name = world.entities[owner].name.clone();
        let restrictions = world.cutscene_input.count;
        let camera = world.nif_camera.map(|c| (c.entity, c.node));
        world.run_callback_fixture(&format!("assert(select('#',EnterCutscene({name:?}))==0)"))?;
        world.run_callback_fixture(&format!(
            "assert(type(IsInCutscene({name:?}))=='boolean'); assert(IsInCutscene({name:?})==true)"
        ))?;
        assert!(world.ai_brains[&owner].motion_suspended);
        assert_eq!(world.cutscene_actions.last_receivers, [owner]);
        assert_eq!(world.cutscene_actions.last.as_ref().unwrap().action, 11);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .reevaluation_requested = false;
        world.run_callback_fixture(&format!("assert(select('#',ExitCutscene({name:?}))==0)"))?;
        world.run_callback_fixture(&format!(
            "assert(select('#',IsInCutscene({name:?}))==1); assert(IsInCutscene({name:?})==false)"
        ))?;
        assert!(
            !world.ai_brains[&owner].motion_suspended
                && !world.ai_brains[&owner].reevaluation_requested
        );
        let source = world.cutscene_actions.last.as_ref().unwrap();
        assert_eq!(source.vector_bits, [0; 4]);
        assert_eq!(source.reference, None);
        assert_eq!(source.header, None);
        assert_eq!(world.cutscene_input.count, restrictions);
        assert_eq!(world.nif_camera.map(|c| (c.entity, c.node)), camera);
        let requests = world.cutscene_actions.requests;
        world.run_callback_fixture("EnterCutscene(nil,2); ExitCutscene(false); ExitCutscene('missing'); EnterCutscene(); ExitCutscene()")?;
        assert_eq!(
            world.cutscene_actions.requests, requests,
            "Invalid targets generated actions"
        );
        world.run_callback_fixture("assert(IsInCutscene(GetPlayer())==false and IsInCutscene(nil)==false and IsInCutscene(false)==false and IsInCutscene('missing')==false and IsInCutscene()==false)")?;
        let missing = world
            .run_callback_fixture("BwxCutsceneType=2; EnterCutscene(GetPlayer(),'BwxCutsceneType')")
            .unwrap_err();
        assert!(
            format!("{missing:#}")
                .contains("mickey_ShadowBlot_IGC_Death.hkx requires loading its animation list"),
            "{missing:#}"
        );
        let local = world.player_graph.state.index("VAR_Cutscene")?;
        assert_eq!(world.player_graph.state.words()[local], 2);
        assert_eq!(
            world.cutscene_actions.last.as_ref().unwrap().source,
            Some(world.player_entity)
        );
        assert_eq!(world.cutscene_actions.last.as_ref().unwrap().action, 11);
        let variables = world.animation_variable_changes.requests;
        world.run_callback_fixture("EnterCutscene(GetPlayer(),-1); ExitCutscene(GetPlayer())")?;
        assert_eq!(
            world.animation_variable_changes.requests, variables,
            "Exit/sentinel wrote an animation variable"
        );
        let warnings = world.report.warnings.len();
        world.run_callback_fixture(&format!(
            "ExitCutscene('ignored',{name:?}); EnterCutscene('ignored',{name:?},-1)"
        ))?;
        assert_eq!(
            world.cutscene_actions.last.as_ref().unwrap().source,
            Some(owner)
        );
        assert_eq!(world.report.warnings.len(), warnings + 1);
        world.retire_entity(owner)?;
        let requests = world.cutscene_actions.requests;
        world.run_callback_fixture(&format!("ExitCutscene({name:?})"))?;
        world.run_callback_fixture(&format!("assert(IsInCutscene({name:?})==false)"))?;
        assert_eq!(world.cutscene_actions.requests, requests);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_boss_cutscene_variant_activates_its_authored_cinematic_clip() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/ShadowBlot_Boss.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture("assert(select('#',EnterCutscene(GetPlayer(),2))==0)")?;
        let (id, clip) = world
            .player_graph
            .state
            .clips
            .iter()
            .find(|(id, clip)| {
                clip.settings
                    .animation_name
                    .to_ascii_lowercase()
                    .contains("shadowblot_igc_death")
                    && world.player_graph.clips.get(id).is_ok()
            })
            .context("Cutscene event did not activate the original death cinematic generator")?;
        assert!(world
            .player_graph
            .plan
            .nodes
            .iter()
            .any(|node| node.object == *id));
        let resource = world
            .player_animations
            .clip(&clip.settings.animation_name)?;
        assert!(resource.info.duration > 0. && resource.info.num_tracks > 0);
        let local = world.player_graph.state.index("VAR_Cutscene")?;
        assert_eq!(world.player_graph.state.words()[local], 2);
        assert_eq!(world.cutscene_actions.last.as_ref().unwrap().action, 11);
        Ok(())
    }
}
