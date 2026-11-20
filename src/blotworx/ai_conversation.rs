//! Conversation behavior's ordinary-action receiver and lifecycle gates,
//! REL1b6cd0..1b6e40/1b726c. Kind81 line payload and task execution are separate.
//! This is not SetCharacterInConversation's distinct kind85 player event.
use super::ai_behavior_events::Event;

///1b7378..1b738c deliberately compares the raw angle difference. The task's
///completion uses wrapped angles; these are two distinct native predicates.
pub fn needs_turn(desired: f32, current: f32, threshold: f32) -> bool {
    (desired - current).abs() > threshold
}
pub trait FacingHost {
    fn facing_enabled(&mut self) -> anyhow::Result<bool>;
    fn has_child(&mut self) -> anyhow::Result<bool>;
    fn desired_heading(&mut self) -> anyhow::Result<f32>;
    fn current_heading(&mut self) -> anyhow::Result<f32>;
    fn install_turn(&mut self) -> anyhow::Result<()>;
}
///1b72ac. Do not query the player or actor transforms when disabled or while
///an existing child is active. Turning is a nonblocking common child.
pub fn face_player(threshold: f32, host: &mut impl FacingHost) -> anyhow::Result<()> {
    if !host.facing_enabled()? || host.has_child()? {
        return Ok(());
    }
    let desired = host.desired_heading()?;
    let current = host.current_heading()?;
    if needs_turn(desired, current, threshold) {
        host.install_turn()?;
    }
    Ok(())
}

/// Services used by retail1b6e44. State remains in its owner during callbacks:
/// animation dispatch may synchronously change the conversation or its child.
pub trait UpdateHost {
    fn animation_controller(&mut self) -> anyhow::Result<Option<usize>>;
    fn phase(&self) -> anyhow::Result<u32>;
    fn set_phase(&mut self, phase: u32) -> anyhow::Result<()>;
    fn line_pending(&self) -> anyhow::Result<bool>;
    fn startup_enabled(&mut self) -> anyhow::Result<bool>;
    fn resolve_enabled(&mut self) -> anyhow::Result<bool>;
    fn send(&mut self, controller: Option<usize>, event: u32) -> anyhow::Result<()>;
    fn install_wait(
        &mut self,
        definition: super::ai_animation_wait::Definition,
    ) -> anyhow::Result<()>;
    ///1b72ac, including its configuration/child gates and native turn task.
    fn face_player(&mut self, threshold: f32) -> anyhow::Result<()>;
    ///1b7444, with the original owner and retained primary/reaction lines.
    fn play_lines(&mut self) -> anyhow::Result<()>;
}

/// Behavior update only; common198b3c must finish a blocking child first.
/// In particular phase2 does not mean its animation has independently ended.
pub fn update(host: &mut impl UpdateHost) -> anyhow::Result<u32> {
    let controller = host.animation_controller()?;
    let wait = |start_event, done_event| super::ai_animation_wait::Definition {
        start_event,
        done_event,
        timeout: 5.,
        apply_heading: false,
        heading: -1.,
        agent_40: true,
    };
    let phase = host.phase()?;
    match phase {
        1 => {
            if host.startup_enabled()? {
                host.install_wait(wait(0x71, 0x72))?;
                host.set_phase(2)?;
            } else {
                host.send(controller, 0xc2)?;
                host.set_phase(3)?;
            }
        }
        2 | 3 => {
            if phase == 2 {
                host.set_phase(3)?;
            }
            host.face_player(0.)?;
            // Read after facing: native child activation can dispatch events.
            if host.line_pending()? {
                host.send(controller, 0x6c)?;
                host.set_phase(4)?;
                host.play_lines()?;
            }
        }
        4 => host.face_player(f32::from_bits(0x3f490fdb))?,
        5 => {
            if host.resolve_enabled()? {
                host.install_wait(wait(0x6d, 0xc1))?;
                host.set_phase(6)?;
            } else {
                host.send(controller, 0xc1)?;
                return Ok(2);
            }
        }
        6 => return Ok(2),
        _ => {}
    }
    Ok(1)
}

#[derive(Clone, Copy)]
pub enum Notification {
    Started,
    Finished { aborted: bool },
}
/// Manager281314 sends21 before instance activation. Its2809b0 completion
/// callback sends22 only on normal completion; an aborted callback declines.
pub fn notification_event(
    source: Option<usize>,
    notification: Notification,
) -> Option<super::action_event_queue::ActionEvent> {
    let action = match notification {
        Notification::Started => 21,
        Notification::Finished { aborted: false } => 22,
        Notification::Finished { aborted: true } => return None,
    };
    Some(super::action_event_queue::ActionEvent {
        source,
        header: None,
        action,
        vector_bits: [0; 4],
        reference: None,
    })
}

#[derive(Default, Debug, serde::Serialize)]
pub struct Conversation {
    /// Native behavior+38. Keep states5/6 distinct from inactive0: they still
    /// have exit-animation/task work even though candidate eligibility is false.
    pub phase: u32,
    pub line_pending: bool,
    pub configuration: Option<Configuration>,
}
#[derive(Debug, serde::Serialize)]
pub struct Configuration {
    pub turn_to_face_player: String,
    pub play_startup_animation: String,
    pub play_resolve_animation: String,
}
#[derive(Debug, PartialEq)]
pub struct Settings {
    pub turn_to_face_player: bool,
    pub play_startup_animation: bool,
    pub play_resolve_animation: bool,
}
impl Configuration {
    pub fn values(
        &self,
        settings: &std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<Settings> {
        use anyhow::Context as _;
        let read = |key: &str| -> anyhow::Result<bool> {
            let value = settings
                .get(key)
                .with_context(|| format!("Bound AI attribute disappeared: {key}"))?;
            super::config_value::boolean(Some(value))
                .map_err(|message| anyhow::anyhow!("{key}: {message}"))
        };
        Ok(Settings {
            turn_to_face_player: read(&self.turn_to_face_player)?,
            play_startup_animation: read(&self.play_startup_animation)?,
            play_resolve_animation: read(&self.play_resolve_animation)?,
        })
    }
}
impl Conversation {
    ///1b68b0 initializes48/4c/50 in this order, all with literal fallback false.
    pub fn configure(
        &mut self,
        ai_type: &str,
        settings: &mut std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let mut key = |property| {
            super::ai_attributes::resolve(settings, "Conversation", ai_type, property, "false")
        };
        self.configuration = Some(Configuration {
            turn_to_face_player: key("TurnToFacePlayer")?,
            play_startup_animation: key("PlayStartupAnimation")?,
            play_resolve_animation: key("PlayResolveAnimation")?,
        });
        Ok(())
    }
    pub fn eligible(&self) -> bool {
        (1..=4).contains(&self.phase)
    }
    /// Subscription/source admission is performed by the owning brain first.
    /// Action22 changes state but deliberately returns false in retail.
    pub fn receive_action(&mut self, event: Event, is_current: bool) -> bool {
        if event.kind != 0x0c {
            return false;
        }
        match event.action {
            21 if !self.eligible() => {
                self.phase = 1;
                true
            }
            22 if self.eligible() => {
                self.phase = if is_current { 5 } else { 0 };
                self.line_pending = false;
                false
            }
            _ => false,
        }
    }
    pub fn priority(&self) -> u32 {
        42
    }
    pub fn enter(&mut self) {
        self.phase = 1;
    }
    pub fn exit(&mut self) {
        if !self.eligible() {
            self.phase = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversation_facing_gates_and_raw_angle_threshold_match_original() -> anyhow::Result<()> {
        use serde_json::{json, Value};
        struct Binding<'a> {
            case: &'a Value,
            trace: Vec<Value>,
        }
        impl FacingHost for Binding<'_> {
            fn facing_enabled(&mut self) -> anyhow::Result<bool> {
                Ok(self.case["enabled"].as_bool().unwrap())
            }
            fn has_child(&mut self) -> anyhow::Result<bool> {
                Ok(self.case["child"].as_bool().unwrap())
            }
            fn desired_heading(&mut self) -> anyhow::Result<f32> {
                self.trace.extend([json!(["resolve"]), json!(["heading"])]);
                Ok(f32::from_bits(self.case["desired"].as_u64().unwrap() as u32))
            }
            fn current_heading(&mut self) -> anyhow::Result<f32> {
                self.trace.extend([json!(["axis"]), json!(["atan2"])]);
                Ok(f32::from_bits(self.case["current"].as_u64().unwrap() as u32))
            }
            fn install_turn(&mut self) -> anyhow::Result<()> {
                self.trace.extend([
                    json!(["allocate"]),
                    json!(["construct"]),
                    json!(["install"]),
                ]);
                Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_conversation_facing.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let case = &expected["input"];
            let mut host = Binding {
                case,
                trace: vec![],
            };
            face_player(
                f32::from_bits(case["threshold"].as_u64().unwrap() as u32),
                &mut host,
            )?;
            assert_eq!(json!(host.trace), expected["trace"], "{case}");
        }
        Ok(())
    }
    #[test]
    fn conversation_update_matches_original_phases_tasks_and_callback_order() -> anyhow::Result<()>
    {
        use serde_json::{json, Value};
        struct Binding<'a> {
            case: &'a Value,
            phase: u32,
            pending: bool,
            trace: Vec<Value>,
        }
        impl UpdateHost for Binding<'_> {
            fn animation_controller(&mut self) -> anyhow::Result<Option<usize>> {
                self.trace.push(json!(["controller"]));
                Ok(self.case["controller"].as_bool().unwrap().then_some(7))
            }
            fn phase(&self) -> anyhow::Result<u32> {
                Ok(self.phase)
            }
            fn set_phase(&mut self, phase: u32) -> anyhow::Result<()> {
                self.phase = phase;
                Ok(())
            }
            fn line_pending(&self) -> anyhow::Result<bool> {
                Ok(self.pending)
            }
            fn startup_enabled(&mut self) -> anyhow::Result<bool> {
                Ok(self.case["startup"].as_bool().unwrap())
            }
            fn resolve_enabled(&mut self) -> anyhow::Result<bool> {
                Ok(self.case["resolve"].as_bool().unwrap())
            }
            fn send(&mut self, controller: Option<usize>, event: u32) -> anyhow::Result<()> {
                assert_eq!(
                    controller,
                    self.case["controller"].as_bool().unwrap().then_some(7)
                );
                self.trace.push(json!(["send", event, self.phase]));
                if self.case["mutation"] == "send" {
                    self.phase = 78;
                }
                Ok(())
            }
            fn install_wait(
                &mut self,
                d: super::super::ai_animation_wait::Definition,
            ) -> anyhow::Result<()> {
                self.trace.push(json!(["allocate", self.phase]));
                self.trace.push(json!([
                    "construct",
                    [
                        json!(d.start_event),
                        json!(d.done_event),
                        json!(d.timeout.to_bits()),
                        json!(d.apply_heading),
                        json!(d.heading.to_bits()),
                        json!(d.agent_40)
                    ],
                    self.phase
                ]));
                self.trace.push(json!(["install", self.phase]));
                if self.case["mutation"] == "install" {
                    self.phase = 77;
                }
                Ok(())
            }
            fn face_player(&mut self, threshold: f32) -> anyhow::Result<()> {
                self.trace
                    .push(json!(["face", threshold.to_bits(), self.phase]));
                match self.case["mutation"].as_str() {
                    Some("face_pending") => self.pending = true,
                    Some("face_clear") => self.pending = false,
                    Some("face_phase") => self.phase = 79,
                    _ => {}
                }
                Ok(())
            }
            fn play_lines(&mut self) -> anyhow::Result<()> {
                self.trace.push(json!(["lines", self.phase]));
                if self.case["mutation"] == "lines" {
                    self.phase = 80;
                }
                Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_conversation_update.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let case = &expected["input"];
            let mut host = Binding {
                case,
                phase: case["phase"].as_u64().unwrap() as u32,
                pending: case["pending"].as_bool().unwrap(),
                trace: vec![],
            };
            let result = update(&mut host)?;
            assert_eq!(
                json!({"phase":host.phase,"pending":host.pending,"trace":host.trace,"returned":result}),
                json!({"phase":expected["phase"],"pending":expected["pending"],"trace":expected["trace"],"returned":expected["returned"]}),
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn conversation_receiver_and_gates_match_original_instructions() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_conversation.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            let mut state = Conversation {
                phase: input["phase"].as_u64().unwrap() as u32,
                line_pending: input["line_pending"].as_bool().unwrap(),
                ..Default::default()
            };
            let returned = match input["operation"].as_str().unwrap() {
                "action" => Some(u32::from(state.receive_action(
                    Event {
                        kind: input["kind"].as_u64().unwrap() as u16,
                        source: Some(7),
                        action: input["action"].as_u64().unwrap() as u32,
                    },
                    input["current"].as_bool().unwrap(),
                ))),
                "enter" => {
                    state.enter();
                    None
                }
                "exit" => {
                    state.exit();
                    None
                }
                _ => unreachable!(),
            };
            assert_eq!(
                state.phase as u64,
                case["phase"].as_u64().unwrap(),
                "{input}"
            );
            assert_eq!(
                state.line_pending,
                case["line_pending"].as_bool().unwrap(),
                "{input}"
            );
            assert_eq!(
                state.eligible(),
                case["eligible"].as_bool().unwrap(),
                "{input}"
            );
            assert_eq!(
                returned,
                case["returned"].as_u64().map(|v| v as u32),
                "{input}"
            );
            assert_eq!(state.priority(), 42);
        }
        Ok(())
    }
    #[test]
    fn manager_bookends_match_original_event_construction_and_abort_gate() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_conversation.json"
        ))?;
        for case in fixture["notifications"].as_array().unwrap() {
            let notification = if case["operation"] == "start" {
                Notification::Started
            } else {
                Notification::Finished {
                    aborted: case["aborted"].as_bool().unwrap(),
                }
            };
            let event =
                notification_event(case["source"].as_u64().map(|v| v as usize), notification);
            assert_eq!(serde_json::to_value(event)?, case["event"], "{case}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_story_playback_sends_real_conversation_bookends() -> anyhow::Result<()> {
        use super::super::{assets, story_sequences::Admission, STEP};
        use anyhow::Context as _;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        // Isolate one original story request, without replacing its events,
        // native action notifications, receivers or source data.
        world.story_sequences.queue.clear();
        let owner = world
            .scripts
            .resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus absent")?;
        let slot = *world.ai_brains[&owner]
            .conversations
            .keys()
            .next()
            .context("Gus Conversation missing")?;
        let name = "DBC_1st_Gus_Introduction";
        let before = (
            world.cutscene_actions.story_start_actions,
            world.cutscene_actions.story_finish_actions,
        );
        assert_eq!(world.force_story_sequence(owner, name)?, Admission::Queued);
        assert_eq!(
            world.force_story_sequence(owner, name)?,
            Admission::QueuedAlready
        );
        assert_eq!(
            world.cutscene_actions.story_start_actions, before.0,
            "Queue admission sent a start event"
        );
        world.advance_demo_story(STEP);
        assert_eq!(world.cutscene_actions.story_start_actions, before.0 + 1);
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 1);
        for _ in 0..300 {
            if world.demo_story.dialogue.is_some() {
                world.acknowledge_demo_story(0);
            }
            world.advance_demo_story(STEP);
            if world.demo_story.completed.iter().any(|n| n == name) {
                break;
            }
        }
        assert!(
            world.demo_story.errors.is_empty(),
            "{:?}",
            world.demo_story.errors
        );
        assert!(
            world.demo_story.completed.iter().any(|n| n == name),
            "Original Gus introduction did not complete"
        );
        assert_eq!(world.cutscene_actions.story_start_actions, before.0 + 1);
        assert_eq!(world.cutscene_actions.story_finish_actions, before.1 + 1);
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 0);
        assert_eq!(world.cutscene_actions.last.as_ref().unwrap().action, 22);
        assert!(!world.cutscene_input.restricted);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_conversation_actions_reach_live_ordinary_and_deferred_receivers(
    ) -> anyhow::Result<()> {
        use super::super::{action_event_queue::ActionEvent, assets, entity_events};
        use anyhow::Context as _;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        let owner = world
            .scripts
            .resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus absent")?;
        let slot = *world.ai_brains[&owner]
            .conversations
            .keys()
            .next()
            .context("Original Gus Conversation receiver absent")?;
        let event = |source, action| ActionEvent {
            source: Some(source),
            header: None,
            action,
            vector_bits: [0; 4],
            reference: None,
        };
        let dispatch = |world: &mut assets::World, event| {
            world.cutscene_actions.dispatch(
                event,
                &mut world.entity_event_registry,
                &mut world.ai_brains,
            )
        };
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 0);
        dispatch(&mut world, event(owner, 21))?;
        assert!(world.ai_brains[&owner].conversations[&slot].eligible());
        assert!(world.ai_brains[&owner].reevaluation_requested);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .reevaluation_requested = false;
        dispatch(&mut world, event(owner, 21))?;
        assert!(
            !world.ai_brains[&owner].reevaluation_requested,
            "Repeated start incorrectly requested selection"
        );
        // The selected identity is a supplied boundary here, not evidence of
        // native World arbitration. Exercise both branches of the real receiver.
        {
            let brain = world.ai_brains.get_mut(&owner).unwrap();
            brain.selector.current = Some(slot);
            brain.conversations.get_mut(&slot).unwrap().line_pending = true;
        }
        dispatch(&mut world, event(owner, 22))?;
        let conversation = &world.ai_brains[&owner].conversations[&slot];
        assert_eq!(conversation.phase, 5);
        assert!(!conversation.eligible() && !conversation.line_pending);
        assert!(!world.ai_brains[&owner].reevaluation_requested);
        dispatch(&mut world, event(owner, 21))?;
        world.ai_brains.get_mut(&owner).unwrap().selector.current = None;
        dispatch(&mut world, event(owner, 22))?;
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 0);
        let player = world.player_entity;
        dispatch(&mut world, event(player, 21))?;
        assert_eq!(
            world.ai_brains[&owner].conversations[&slot].phase, 0,
            "Foreign source reached Gus"
        );
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .reevaluation_requested = false;
        world
            .entity_event_queue
            .enqueue(&entity_events::Event::Action(event(owner, 21)), None)?;
        world.process_deferred_entity_events()?;
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 1);
        assert!(world.ai_brains[&owner].reevaluation_requested);
        // Player/camera conversation85 is a different protocol and must not
        // start/stop this NPC behavior through the common known receiver.
        let brain = world.ai_brains.get_mut(&owner).unwrap();
        assert!(!brain.receive_known(
            owner,
            Event {
                kind: 0x85,
                source: Some(owner),
                action: 22
            }
        ));
        assert_eq!(brain.conversations[&slot].phase, 1);
        Ok(())
    }
}
