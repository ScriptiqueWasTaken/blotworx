//! Concrete Conversation update services. Selection/common-child scheduling
//! must call this with its actual current slot; this never chooses a behavior.
use super::{
    ai_conversation::{self, Conversation, UpdateHost},
    assets::World,
};
use anyhow::{ensure, Context, Result};

impl World {
    pub(super) fn update_selected_ai_conversation(&mut self, owner: usize) -> Result<u32> {
        let slot = self
            .ai_brains
            .get(&owner)
            .context("Conversation brain is absent")?
            .selector
            .current
            .context("Conversation update has no selected behavior")?;
        ensure!(
            self.ai_brains[&owner].conversations.contains_key(&slot),
            "Selected AI behavior is not Conversation"
        );
        ai_conversation::update(&mut Binding {
            world: self,
            owner,
            slot,
        })
    }

    ///198a54 with a concrete1986c0 child. Keep the common state inside the
    /// brain while activation dispatches; its receiver needs that very child.
    pub(super) fn install_ai_animation_wait(
        &mut self,
        owner: usize,
        slot: usize,
        definition: super::ai_animation_wait::Definition,
    ) -> Result<usize> {
        // The native caller constructs the new child before replacing the old.
        let task = self.create_ai_animation_wait(owner, definition)?;
        self.install_ai_child(owner, slot, task, true)?;
        Ok(task)
    }
}

struct Binding<'a> {
    world: &'a mut World,
    owner: usize,
    slot: usize,
}
impl Binding<'_> {
    fn state(&self) -> Result<&Conversation> {
        self.world
            .ai_brains
            .get(&self.owner)
            .and_then(|b| b.conversations.get(&self.slot))
            .context("Conversation state was removed during update")
    }
    fn flag(&self, property: &str) -> Result<bool> {
        let config = self
            .state()?
            .configuration
            .as_ref()
            .context("Conversation attributes are not bound")?;
        let key = match property {
            "startup" => &config.play_startup_animation,
            "resolve" => &config.play_resolve_animation,
            "facing" => &config.turn_to_face_player,
            _ => anyhow::bail!("Unknown Conversation attribute"),
        };
        let value = self
            .world
            .runtime_settings
            .get(key)
            .with_context(|| format!("Bound AI attribute disappeared: {key}"))?;
        super::config_value::boolean(Some(value)).map_err(|e| anyhow::anyhow!("{key}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_conversation_update_uses_bound_flags_owned_tasks_and_actual_controller(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
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
            .context("Original Conversation absent")?;
        assert!(world.update_selected_ai_conversation(owner).is_err());
        // Supplied selection boundary. The real selector is not yet World-bound.
        world.ai_brains.get_mut(&owner).unwrap().selector.current = Some(slot);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .conversations
            .get_mut(&slot)
            .unwrap()
            .phase = 1;
        let error = world.update_selected_ai_conversation(owner).unwrap_err();
        assert!(
            error.to_string().contains("navigation-agent binding"),
            "{error:#}"
        );
        let brain = &world.ai_brains[&owner];
        let task = brain.behavior_tasks[&slot]
            .child
            .context("Wait was not installed before activation")?;
        assert!(brain.animation_waits[&task].subscribed);
        assert_eq!(
            brain.conversations[&slot].phase, 1,
            "Failed activation must not claim phase2"
        );
        assert_eq!(brain.behavior_tasks[&slot].mode, 0);
        assert_eq!(
            brain.animation_waits[&task].task.definition.start_event,
            0x71
        );
        assert_eq!(
            brain.animation_waits[&task].task.definition.done_event,
            0x72
        );
        assert!(brain.animation_waits[&task].task.definition.agent_40);
        world.destroy_ai_animation_wait(owner, task)?;
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .behavior_tasks
            .get_mut(&slot)
            .unwrap()
            .child = None;
        let configuration = world.ai_brains[&owner].conversations[&slot]
            .configuration
            .as_ref()
            .unwrap();
        let (startup, resolve) = (
            configuration.play_startup_animation.clone(),
            configuration.play_resolve_animation.clone(),
        );
        // Exercise live configuration paths; no original asset is changed.
        world.runtime_settings.insert(startup, "false".into());
        world.runtime_settings.insert(resolve, "false".into());
        assert_eq!(world.update_selected_ai_conversation(owner)?, 1);
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 3);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .conversations
            .get_mut(&slot)
            .unwrap()
            .phase = 5;
        assert_eq!(world.update_selected_ai_conversation(owner)?, 2);
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 5);
        // Concrete common-task replacement retains previous kind and shared
        // listener while replacing its owned local subscription.
        let definition = super::super::ai_animation_wait::Definition {
            start_event: 0xc2,
            done_event: 0xc2,
            timeout: 5.,
            apply_heading: false,
            heading: -1.,
            agent_40: false,
        };
        let first = world.install_ai_animation_wait(owner, slot, definition)?;
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .animation_waits
            .get_mut(&first)
            .unwrap()
            .task
            .finish(false);
        let second = world.install_ai_animation_wait(owner, slot, definition)?;
        let brain = &world.ai_brains[&owner];
        assert!(!brain.animation_waits.contains_key(&first));
        assert!(brain.animation_waits[&second].subscribed);
        assert_eq!(brain.behavior_tasks[&slot].child, Some(second));
        assert_eq!(brain.behavior_tasks[&slot].previous_kind, 2);
        assert_eq!(brain.behavior_tasks[&slot].mode, 1);
        // A genuinely missing reference completes a turn with failure before
        // querying any navigation entity (197bd0 path). This exercises the
        //real owned World task without fabricating an absent Gus agent.
        let missing = world.entities.len();
        let turn = world.create_ai_turn(
            owner,
            super::super::ai_turn_task::Turn::toward_entity(Some(missing)),
        )?;
        world.install_ai_child(owner, slot, turn, false)?;
        assert!(!world.ai_brains[&owner]
            .animation_waits
            .contains_key(&second));
        assert_eq!(
            world.ai_brains[&owner].behavior_tasks[&slot].mode, 1,
            "Nonblocking installation must retain mode"
        );
        assert_eq!(world.update_ai_turn(owner, turn)?, 1);
        assert_eq!(world.ai_child_kind(owner, turn)?, 2);
        world.dispatch_camera_animation_feedback(super::super::entity_events::Animation {
            source: Some(owner),
            header: None,
            id: 0xc2,
            flag: 0,
        })?;
        assert_eq!(
            world.ai_child_kind(owner, turn)?,
            2,
            "Unsubscribed turn accepted an old wait's feedback"
        );
        world.set_npc_ai_speed(owner, false, 1.25)?;
        let speed = world.npc_relocations[&owner].desired_speed;
        let replacement = world.install_ai_animation_wait(owner, slot, definition)?;
        assert!(!world.ai_brains[&owner].turns.contains_key(&turn));
        assert_eq!(
            world.ai_brains[&owner].behavior_tasks[&slot].previous_kind,
            2
        );
        assert_eq!(
            world.npc_relocations[&owner].desired_speed.to_bits(),
            speed.to_bits(),
            "Turn stop must not cancel translation"
        );
        // A retained child gates facing before player/agent queries.
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .conversations
            .get_mut(&slot)
            .unwrap()
            .phase = 3;
        assert_eq!(world.update_selected_ai_conversation(owner)?, 1);
        world.destroy_ai_child(owner, replacement)?;
        let common = world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .behavior_tasks
            .get_mut(&slot)
            .unwrap();
        common.child = None;
        common.mode = 0;
        // Supplied transform-query boundary: matching heading needs no turn;
        //a perpendicular heading installs the actual player-reference task.
        let source = world.entities[owner].transform;
        let player = world.player_entity;
        world.entities[owner].runtime_rotation = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        world.entities[player].transform[12..15].copy_from_slice(&[
            source[12],
            source[13],
            source[14] + 2.,
        ]);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .conversations
            .get_mut(&slot)
            .unwrap()
            .phase = 4;
        assert_eq!(world.update_selected_ai_conversation(owner)?, 1);
        assert!(world.ai_brains[&owner].behavior_tasks[&slot]
            .child
            .is_none());
        world.entities[player].transform[12..15].copy_from_slice(&[
            source[12] + 2.,
            source[13],
            source[14],
        ]);
        let error = world.update_selected_ai_conversation(owner).unwrap_err();
        assert!(
            error.to_string().contains("navigation-agent binding"),
            "{error:#}"
        );
        let child = world.ai_brains[&owner].behavior_tasks[&slot]
            .child
            .context("Facing turn was not installed")?;
        assert_eq!(
            world.ai_brains[&owner].turns[&child].reference,
            Some(player)
        );
        assert_eq!(world.ai_brains[&owner].behavior_tasks[&slot].mode, 0);
        assert_eq!(world.ai_brains[&owner].conversations[&slot].phase, 4);
        Ok(())
    }
}
impl UpdateHost for Binding<'_> {
    fn animation_controller(&mut self) -> Result<Option<usize>> {
        let owner = self
            .world
            .entities
            .get(self.owner)
            .context("Conversation actor is absent")?;
        let count = owner
            .components
            .iter()
            .filter(|c| c.class_name == "AnimationComponent")
            .count();
        ensure!(
            count <= 1,
            "Conversation requires native animation-component slot arbitration"
        );
        Ok((count == 1).then_some(self.owner))
    }
    fn phase(&self) -> Result<u32> {
        Ok(self.state()?.phase)
    }
    fn set_phase(&mut self, phase: u32) -> Result<()> {
        self.world
            .ai_brains
            .get_mut(&self.owner)
            .and_then(|b| b.conversations.get_mut(&self.slot))
            .context("Conversation state was removed during update")?
            .phase = phase;
        Ok(())
    }
    fn line_pending(&self) -> Result<bool> {
        Ok(self.state()?.line_pending)
    }
    fn startup_enabled(&mut self) -> Result<bool> {
        self.flag("startup")
    }
    fn resolve_enabled(&mut self) -> Result<bool> {
        self.flag("resolve")
    }
    fn send(&mut self, controller: Option<usize>, event: u32) -> Result<()> {
        self.world.send_native_actor_animation_event(
            controller.context("Conversation animation controller is absent")?,
            event,
        )?;
        Ok(())
    }
    fn install_wait(&mut self, definition: super::ai_animation_wait::Definition) -> Result<()> {
        self.world
            .install_ai_animation_wait(self.owner, self.slot, definition)?;
        Ok(())
    }
    fn face_player(&mut self, threshold: f32) -> Result<()> {
        if self.world.demo_npc_active(self.owner){
            if self.flag("facing")?{self.world.demo_npc_face_player(self.owner)?;}
            return Ok(());
        }
        ai_conversation::face_player(threshold, self)
    }
    fn play_lines(&mut self) -> Result<()> {
        if self.world.demo_npc_active(self.owner){
            let limitation="Temporary NPC line presentation uses the functional BSQ/dialogue scheduler; standalone native primary/reaction-line playback is incomplete.";
            if !self.world.report.unsupported.iter().any(|s|s==limitation){self.world.report.unsupported.push(limitation.into());}
            return Ok(());
        }
        anyhow::bail!("Conversation requires native primary/reaction-line playback1b7444")
    }
}
impl ai_conversation::FacingHost for Binding<'_> {
    fn facing_enabled(&mut self) -> Result<bool> {
        self.flag("facing")
    }
    fn has_child(&mut self) -> Result<bool> {
        Ok(self
            .world
            .ai_brains
            .get(&self.owner)
            .and_then(|b| b.behavior_tasks.get(&self.slot))
            .context("Conversation common state is absent")?
            .child
            .is_some())
    }
    fn desired_heading(&mut self) -> Result<f32> {
        let player = self.world.player_entity;
        ensure!(
            !self.world.removed_entities.contains(&player),
            "Conversation player reference has been retired"
        );
        let target = self
            .world
            .entities
            .get(player)
            .context("Conversation player reference is absent")?
            .transform;
        let actor = self
            .world
            .entities
            .get(self.owner)
            .context("Conversation actor is absent")?;
        //297564 ->2975b4: paired single-precision position subtraction,
        //then the original double atan2 rounded back to single.
        let dx = target[12] - actor.transform[12];
        let dz = target[14] - actor.transform[14];
        Ok(super::native_angle::atan2(dx as f64, dz as f64) as f32)
    }
    fn current_heading(&mut self) -> Result<f32> {
        let r = self
            .world
            .entities
            .get(self.owner)
            .context("Conversation actor is absent")?
            .runtime_rotation;
        Ok(super::native_angle::atan2(r[2] as f64, r[8] as f64) as f32)
    }
    fn install_turn(&mut self) -> Result<()> {
        let player = self.world.player_entity;
        let task = self.world.create_ai_turn(
            self.owner,
            super::ai_turn_task::Turn::toward_entity(Some(player)),
        )?;
        self.world
            .install_ai_child(self.owner, self.slot, task, false)
    }
}
