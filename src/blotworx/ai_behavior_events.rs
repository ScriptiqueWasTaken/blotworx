//! Retail AI event filtering and Paused behavior. Full behavior selection,
//! controller output and the other behavior classes remain separate.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub kind: u16,
    pub source: Option<usize>,
    pub action: u32,
}
/// REL183b8c: missing subscription declines; wildcard admits any source;
/// otherwise compare the event's entity identity with the behavior owner.
#[derive(Default)]
pub struct Subscriptions {
    entries: BTreeMap<u16, bool>,
}
impl Subscriptions {
    pub fn register(&mut self, kind: u16, any_source: bool) {
        self.entries.insert(kind, any_source);
    }
    pub fn accepts(&self, owner: Option<usize>, event: Event) -> bool {
        self.entries
            .get(&event.kind)
            .is_some_and(|&any| any || owner == event.source)
    }
}
/// REL199b08: root-child forwarding first (its result is discarded), then
/// the root's virtual slot5c, then supplemental slot60 receivers in authored
/// order. Those slots are not interchangeable (REL199b80/199bd4).
pub trait Receivers {
    fn forward_root_child(&mut self, event: Event) -> Result<()>;
    fn accepts(&mut self, behavior: usize, event: Event) -> bool;
    fn receive_root_5c(&mut self, behavior: usize, event: Event) -> Result<bool>;
    fn receive_supplemental_60(&mut self, behavior: usize, event: Event) -> Result<bool>;
    fn request_reevaluation(&mut self) -> Result<()>;
}
pub fn dispatch(
    host: &mut impl Receivers,
    root: Option<usize>,
    behaviors: &[usize],
    event: Event,
) -> Result<()> {
    if root.is_some() {
        host.forward_root_child(event)?;
    }
    let mut changed = false;
    if let Some(root) = root {
        if host.accepts(root, event) {
            changed = host.receive_root_5c(root, event)?;
        }
    }
    for &behavior in behaviors {
        if host.accepts(behavior, event) {
            changed |= host.receive_supplemental_60(behavior, event)?;
        }
    }
    if changed {
        host.request_reevaluation()?;
    }
    Ok(())
}
#[derive(Default, Debug)]
pub struct Paused {
    pub active: bool,
}
impl Paused {
    /// REL1b87f4: repeated actions still request reevaluation.
    pub fn receive(&mut self, event: Event) -> bool {
        if event.kind != 0x0c {
            return false;
        }
        match event.action {
            13 => self.active = true,
            14 => self.active = false,
            _ => return false,
        }
        true
    }
    /// REL1b8840: retain native result codes rather than guessing enum names.
    pub fn update_result(&self) -> u32 {
        if self.active {
            1
        } else {
            2
        }
    }
    /// REL1b889c.
    pub fn priority(&self) -> u32 {
        0x28
    }
    /// REL1b8858: emit through the actual animation controller if present.
    pub fn entry_animation_event(&self, has_controller: bool) -> Option<u32> {
        has_controller.then_some(0xc2)
    }
}
/// ScriptedIdle reads the owning AIBrain's byte48 rather than keeping its own
/// activation flag. REL1b8a68 ->2e0238,1b8a74..1b8b44.
#[derive(Default, Debug)]
pub struct ScriptedIdle;
impl ScriptedIdle {
    pub fn receive(&self, event: Event) -> bool {
        event.kind == 0x0c && event.action == 11
    }
    /// Separate virtual slot40, not the ordinary slot60 receiver.
    pub fn exit_event(&self, event: Event) -> bool {
        event.kind == 0x0c && event.action == 12
    }
    pub fn eligible(&self, motion_suspended: bool) -> bool {
        motion_suspended
    }
    pub fn priority(&self) -> u32 {
        43
    }
    pub fn update_result(&self, motion_suspended: bool) -> u32 {
        if motion_suspended {
            1
        } else {
            2
        }
    }
    pub fn entry_animation_event(&self, has_controller: bool) -> Option<u32> {
        has_controller.then_some(194)
    }
}
/// NPCPuddle supplemental slot60, REL68f58. Initialization68e48 subscribes
/// to kind75 with any-source=false; the receiver itself reads current at14.
pub fn npc_puddle_status_changed(event: &super::entity_events::CounterStatus) -> bool {
    event.current == 2
}
pub struct Brain {
    pub ai_type: String,
    pub root: String,
    pub behaviors: Vec<String>,
    pub paused: BTreeMap<usize, Paused>,
    pub reevaluation_requested: bool,
    pub scripted_idle: BTreeMap<usize, ScriptedIdle>,
    pub conversations: BTreeMap<usize, super::ai_conversation::Conversation>,
    pub scripted_move: BTreeMap<usize, super::ai_scripted_move::ScriptedMove>,
    pub patrol: BTreeMap<usize, super::ai_patrol::State>,
    pub behavior_tasks: BTreeMap<usize, super::ai_task_runtime::State>,
    pub animation_waits: BTreeMap<usize, super::ai_animation_wait::Owned>,
    pub turns: BTreeMap<usize, super::ai_turn_task::Turn>,
    pub next_task_identity: usize,
    pub npc_puddle: Vec<usize>,
    pub selector: super::ai_selection::Selector,
    pub selection_lifecycle: super::ai_selection::Lifecycle,
    /// Common behavior byte14, initialized false by1989f4. Kept separately
    /// from current selection and from component disable/input restrictions.
    pub background_behaviors: BTreeSet<usize>,
    pub motion_suspended: bool,
    pub uninterruptible_fidget: bool,
}
impl Brain {
    pub fn receive_animation_feedback(&mut self,owner:usize,event:&super::entity_events::Animation)->Result<()> {
        self.receive_known(owner,Event {kind:9,source:event.source,action:event.id});
        let child=self.selector.current.and_then(|slot|self.behavior_tasks.get(&slot))
            .and_then(|state|state.child);
        if let Some(child)=child {
            //197a60/197acc/197b28 construct no local subscriptions. Their
            //inherited183b8c gate declines feedback even when another child
            //previously registered the brain's shared kind9 listener.
            if self.turns.contains_key(&child) {return Ok(());}
            let owned=self.animation_waits.get_mut(&child)
                .context("Current AI child has an unimplemented animation-event receiver")?;
            if owned.subscribed && event.source==Some(owner) {owned.task.receive(9,event.id);}
        }
        Ok(())
    }
    /// Concrete supplemental receiver stage, after registry/behavior admission.
    /// Queue scheduling and native root-child forwarding remain caller-owned.
    pub fn receive_counter_status(
        &mut self,
        owner: usize,
        event: &super::entity_events::CounterStatus,
    ) -> bool {
        // Subscription183b8c is distinct from the receiver's current==2 test.
        if event.source != Some(owner) {
            return false;
        }
        let changed = !self.npc_puddle.is_empty() && npc_puddle_status_changed(event);
        if changed {
            //199b08 ORs receiver results and requests reevaluation once.
            self.reevaluation_requested = true;
        }
        changed
    }
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "AIBrainComponent",
            "Not an AI brain component"
        );
        let string = |name: &str| -> Result<String> {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| {
                    p.value
                        .as_str()
                        .map(str::to_owned)
                        .with_context(|| format!("Invalid AI string {name}"))
                })
                .transpose()
                .map(|value| value.unwrap_or_default())
        };
        let behaviors = component
            .properties
            .iter()
            .find(|p| p.name == "Behavior List")
            .map(|p| {
                p.value
                    .as_array()
                    .context("AI Behavior List is not an array")
                    .and_then(|values| {
                        values
                            .iter()
                            .map(|v| {
                                v.as_str()
                                    .map(str::to_owned)
                                    .context("AI behavior name is not a string")
                            })
                            .collect::<Result<Vec<_>>>()
                    })
            })
            .transpose()?
            .unwrap_or_default();
        let paused = behaviors
            .iter()
            .enumerate()
            .filter(|(_, name)| name.as_str() == "Paused")
            .map(|(index, _)| (index, Paused::default()))
            .collect();
        let scripted_idle = behaviors
            .iter()
            .enumerate()
            .filter(|(_, name)| name.as_str() == "ScriptedIdle")
            .map(|(index, _)| (index, ScriptedIdle))
            .collect();
        let selector = super::ai_selection::Selector::new((0..behaviors.len()).collect());
        let conversations = behaviors.iter().enumerate()
            .filter(|(_,name)| name.as_str() == "Conversation")
            .map(|(index,_)| (index, Default::default())).collect();
        let behavior_tasks = (0..behaviors.len()).map(|index|(index,Default::default())).collect();
        let scripted_move = behaviors.iter().enumerate()
            .filter(|(_, name)| name.as_str() == "ScriptedMove")
            .map(|(index, _)| (index, Default::default())).collect();
        let patrol=behaviors.iter().enumerate().filter(|(_,name)|name.as_str()=="Patrol")
            .map(|(index,_)|(index,Default::default())).collect();
        let npc_puddle = behaviors
            .iter()
            .enumerate()
            .filter(|(_, name)| name.as_str() == "NPCPuddle")
            .map(|(index, _)| index)
            .collect();
        Ok(Self {
            ai_type: string("AIType")?,
            root: string("Toplevel Brain")?,
            behaviors,
            paused,
            scripted_idle,
            conversations,
            scripted_move,
            patrol,
            behavior_tasks,
            animation_waits: BTreeMap::new(),
            turns: BTreeMap::new(),
            next_task_identity: 1,
            npc_puddle,
            reevaluation_requested: false,
            selector,
            selection_lifecycle: Default::default(),
            background_behaviors: BTreeSet::new(),
            motion_suspended: false,
            uninterruptible_fidget: false,
        })
    }
    /// Known receiver execution only. Caller must report missing callbacks and
    /// must not claim this dispatches the complete behavior tree.
    pub fn receive_action(&mut self, owner: usize, event: &super::action_event_queue::ActionEvent,
        resolve: &mut dyn FnMut(usize) -> Result<Option<[u32;4]>>) -> Result<bool> {
        self.receive_action_with_patrol(owner,event,resolve,&mut |_|anyhow::bail!("Patrol departure requires the shared event queue"))
    }
    pub fn receive_action_with_patrol(&mut self, owner:usize,event:&super::action_event_queue::ActionEvent,
        resolve:&mut dyn FnMut(usize)->Result<Option<[u32;4]>>,
        notify:&mut dyn FnMut(super::entity_events::Basic)->Result<()>)->Result<bool> {
        let mut changed = self.receive_known(owner, Event {kind:0x0c, source:event.source, action:event.action});
        if event.source == Some(owner) {
            for behavior in self.scripted_move.values_mut() {
                changed |= behavior.receive(event, resolve)?;
            }
            for behavior in self.patrol.values_mut() {
                changed |= behavior.receive_action(0x0c,event.action,event.reference,owner,
                    &mut |source,owner,event_kind|notify(super::entity_events::Basic {
                        event_kind,source,header:Some(owner),
                    }))?;
            }
        }
        self.reevaluation_requested |= changed;
        Ok(changed)
    }
    pub fn receive_known(&mut self, owner: usize, event: Event) -> bool {
        // Component receiver2de788 writes48 for action11/12, and49 for
        // animation8e/8f before it forwards to the actual behavior receivers.
        // This stage has no source comparison. Subscription admission happens
        // later in199b08, and must not suppress these component writes.
        match (event.kind, event.action) {
            (0x0c, 11) => self.motion_suspended = true,
            (0x0c, 12) => self.motion_suspended = false,
            (9, 0x8e) => self.uninterruptible_fidget = true,
            (9, 0x8f) => self.uninterruptible_fidget = false,
            _ => {}
        }
        if event.source != Some(owner) {
            return false;
        }
        let mut changed = false;
        for behavior in self.paused.values_mut() {
            changed |= behavior.receive(event);
        }
        for behavior in self.scripted_idle.values() {
            changed |= behavior.receive(event);
        }
        for (&slot, conversation) in &mut self.conversations {
            changed |= conversation.receive_action(event, self.selector.current == Some(slot));
        }
        self.reevaluation_requested |= changed;
        changed
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn puddle_status_receiver_filters_owner_and_requests_only_thinned_reevaluation() -> Result<()> {
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
            .value = serde_json::json!(["NPCPuddle", "NPCPuddle"]);
        let mut brain = Brain::from_component(&component)?;
        let mut event = super::super::entity_events::CounterStatus {
            source: Some(7),
            header: None,
            previous: 0,
            current: 2,
            auxiliary: 0,
            reference: None,
        };
        assert!(!brain.receive_counter_status(8, &event));
        assert!(!brain.reevaluation_requested);
        for status in [0, 1, 3, -1] {
            event.current = status;
            assert!(!brain.receive_counter_status(7, &event));
            assert!(!brain.reevaluation_requested);
        }
        event.current = 2;
        assert!(brain.receive_counter_status(7, &event));
        assert!(brain.reevaluation_requested);
        brain.reevaluation_requested = false;
        event.previous = 2;
        assert!(brain.receive_counter_status(7, &event));
        assert!(
            brain.reevaluation_requested,
            "Receiver does not test previous status"
        );
        brain.npc_puddle.clear();
        brain.reevaluation_requested = false;
        assert!(!brain.receive_counter_status(7, &event));
        assert!(!brain.reevaluation_requested);
        Ok(())
    }
    #[test]
    fn scripted_idle_uses_owner_motion_state_and_keeps_exit_predicate_separate() -> Result<()> {
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
            .value = serde_json::json!(["Paused", "ScriptedIdle", "ScriptedIdle"]);
        let mut brain = Brain::from_component(&component)?;
        assert_eq!(
            brain.scripted_idle.keys().copied().collect::<Vec<_>>(),
            [1, 2]
        );
        let event = |kind, action| Event {
            kind,
            action,
            source: Some(7),
        };
        assert!(!brain.receive_known(8, event(0x0c, 11)));
        assert!(brain.motion_suspended);
        assert!(
            !brain.reevaluation_requested,
            "Foreign source must not enter ScriptedIdle's receiver"
        );
        assert!(brain.receive_known(7, event(0x0c, 11)));
        assert!(brain.motion_suspended && brain.reevaluation_requested);
        let idle = &brain.scripted_idle[&1];
        assert!(idle.eligible(brain.motion_suspended));
        assert_eq!(idle.update_result(brain.motion_suspended), 1);
        assert_eq!(idle.priority(), 43);
        assert!(idle.priority() > brain.paused[&0].priority());
        assert_eq!(idle.entry_animation_event(true), Some(194));
        assert_eq!(idle.entry_animation_event(false), None);
        assert!(idle.exit_event(event(0x0c, 12)));
        assert!(!idle.receive(event(0x0c, 12)));
        assert!(!idle.receive(event(9, 11)));
        brain.reevaluation_requested = false;
        assert!(
            brain.receive_known(7, event(0x0c, 11)),
            "Repeated suspension must reevaluate"
        );
        brain.reevaluation_requested = false;
        assert!(!brain.receive_known(7, event(0x0c, 12)));
        assert!(!brain.motion_suspended && !brain.reevaluation_requested);
        assert_eq!(
            brain.scripted_idle[&1].update_result(brain.motion_suspended),
            2
        );
        Ok(())
    }
    #[test]
    fn component_flags_precede_subscription_source_filtering() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let component = crate::component_schema::CATALOG.components["AIBrainComponent"]
            .instantiate("AIBrainComponent", &scene)?;
        let mut brain = Brain::from_component(&component)?;
        let event = |kind, action| Event {
            kind,
            source: Some(7),
            action,
        };
        assert!(!brain.receive_known(7, event(9, 0x8e)));
        assert!(brain.uninterruptible_fidget);
        assert!(
            !brain.reevaluation_requested,
            "Component byte changes alone do not force behavior selection"
        );
        brain.receive_known(7, event(9, 0x8f));
        assert!(!brain.uninterruptible_fidget);
        brain.receive_known(7, event(0x0c, 11));
        assert!(brain.motion_suspended);
        brain.receive_known(7, event(0x0c, 12));
        assert!(!brain.motion_suspended);
        brain.receive_known(8, event(9, 0x8e));
        assert!(brain.uninterruptible_fidget);
        brain.receive_known(8, event(0x0c, 11));
        assert!(brain.motion_suspended);
        brain.receive_known(8, event(0x0c, 12));
        assert!(!brain.motion_suspended);
        assert!(!brain.reevaluation_requested);
        Ok(())
    }
    #[test]
    fn actual_paused_receiver_selects_and_requests_its_native_entry_event_once() -> Result<()> {
        struct Host {
            paused: Paused,
            events: Vec<u32>,
            draws: usize,
        }
        impl super::super::ai_selection::Host for Host {
            fn score_20(&mut self, _: usize) -> Result<i32> {
                Ok(self.paused.priority() as i32)
            }
            fn current_gate_0c(&mut self, _: usize) -> Result<bool> {
                Ok(false)
            } // REL12e630.
            fn candidate_gate_24(&mut self, _: usize) -> Result<bool> {
                Ok(self.paused.active)
            } // REL1b88a4.
            fn candidate_gate_28(&mut self, _: usize) -> Result<bool> {
                Ok(false)
            } // REL12e648.
            fn current_gate_2c(&mut self, _: usize) -> Result<bool> {
                Ok(false)
            } // REL12e650.
            fn leave_and_notify(&mut self, _: usize) -> Result<()> {
                anyhow::bail!("Unexpected Paused exit in this scenario")
            }
            fn enter_4c(&mut self, _: usize, same: bool) -> Result<()> {
                assert!(!same);
                self.events.extend(self.paused.entry_animation_event(true));
                Ok(())
            }
            fn random(&mut self) -> Result<i32> {
                self.draws += 1;
                Ok(0)
            }
        }
        let mut host = Host {
            paused: Paused::default(),
            events: vec![],
            draws: 0,
        };
        let mut selector = super::super::ai_selection::Selector::new(vec![0]);
        assert_eq!(selector.choose(&mut host)?, None);
        assert_eq!(host.draws, 0);
        let event = Event {
            kind: 0x0c,
            source: Some(4),
            action: 13,
        };
        host.paused.receive(event);
        assert_eq!(selector.choose(&mut host)?, Some(0));
        assert_eq!(host.events, [194]);
        assert_eq!(host.draws, 1);
        // Repeated requests reevaluate, without restarting the active behavior.
        host.paused.receive(event);
        assert_eq!(selector.choose(&mut host)?, Some(0));
        assert_eq!(host.events, [194]);
        assert_eq!(host.draws, 1);
        Ok(())
    }
    #[test]
    fn subscriptions_use_event_kind_and_native_source_policy() {
        let mut subscriptions = Subscriptions::default();
        let event = Event {
            kind: 12,
            source: Some(2),
            action: 13,
        };
        assert!(!subscriptions.accepts(Some(2), event));
        subscriptions.register(12, false);
        assert!(subscriptions.accepts(Some(2), event));
        assert!(!subscriptions.accepts(Some(1), event));
        subscriptions.register(12, true);
        assert!(subscriptions.accepts(None, event));
        assert!(!subscriptions.accepts(Some(2), Event { kind: 9, ..event }));
    }
    #[test]
    fn paused_receiver_repeats_reevaluation_and_preserves_native_results() {
        let mut paused = Paused::default();
        let pause = Event {
            kind: 12,
            source: Some(3),
            action: 13,
        };
        assert_eq!(paused.update_result(), 2);
        assert!(paused.receive(pause));
        assert!(paused.receive(pause));
        assert_eq!(paused.update_result(), 1);
        assert_eq!(paused.priority(), 40);
        assert_eq!(paused.entry_animation_event(false), None);
        assert_eq!(paused.entry_animation_event(true), Some(194));
        assert!(!paused.receive(Event { kind: 9, ..pause }));
        assert!(paused.active);
        assert!(paused.receive(Event {
            action: 14,
            ..pause
        }));
        assert_eq!(paused.update_result(), 2);
    }
    #[test]
    fn dispatch_keeps_root_forwarding_then_authored_order_and_one_reevaluation() -> Result<()> {
        struct Host(Vec<usize>);
        impl Receivers for Host {
            fn forward_root_child(&mut self, _: Event) -> Result<()> {
                self.0.push(99);
                Ok(())
            }
            fn accepts(&mut self, id: usize, _: Event) -> bool {
                id != 2
            }
            fn receive_root_5c(&mut self, id: usize, _: Event) -> Result<bool> {
                self.0.push(500 + id);
                Ok(false)
            }
            fn receive_supplemental_60(&mut self, id: usize, _: Event) -> Result<bool> {
                self.0.push(600 + id);
                Ok(id == 3)
            }
            fn request_reevaluation(&mut self) -> Result<()> {
                self.0.push(100);
                Ok(())
            }
        }
        let mut host = Host(vec![]);
        dispatch(
            &mut host,
            Some(1),
            &[2, 3, 4],
            Event {
                kind: 12,
                source: Some(0),
                action: 13,
            },
        )?;
        assert_eq!(host.0, [99, 501, 603, 604, 100]);
        // The same behavior in the two roles must call different virtual slots.
        host.0.clear();
        dispatch(
            &mut host,
            Some(3),
            &[3],
            Event {
                kind: 12,
                source: None,
                action: 13,
            },
        )?;
        assert_eq!(host.0, [99, 503, 603, 100]);
        Ok(())
    }
}
