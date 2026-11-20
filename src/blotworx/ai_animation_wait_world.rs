//! Owned animation-task construction and actual controller-feedback delivery.
//! Native selection/behavior entry must install its child; this does not choose
//! behaviors or create guessed navigation agents.
use super::{
    ai_animation_wait::{Definition, Host, Owned, Wait},
    assets::World,
};
use anyhow::{ensure, Context, Result};
impl World {
    pub(super) fn create_ai_animation_wait(
        &mut self,
        owner: usize,
        definition: Definition,
    ) -> Result<usize> {
        let brain = self
            .ai_brains
            .get_mut(&owner)
            .context("Animation task owner lacks an AI brain")?;
        let id = brain.next_task_identity;
        brain.next_task_identity = id.checked_add(1).context("AI task identities exhausted")?;
        brain.animation_waits.insert(
            id,
            Owned {
                task: Wait::new(definition),
                subscribed: false,
            },
        );
        Ok(id)
    }
    pub(super) fn activate_ai_animation_wait(&mut self, owner: usize, task: usize) -> Result<()> {
        if self.demo_npc_active(owner){return self.activate_demo_npc_wait(owner,task)}
        let definition = self
            .ai_brains
            .get(&owner)
            .and_then(|b| b.animation_waits.get(&task))
            .context("Animation task is not constructed")?
            .task
            .definition;
        definition.activate(owner, &mut Binding { world: self, task })
    }
    pub(super) fn update_ai_animation_wait(
        &mut self,
        owner: usize,
        task: usize,
        delta: f32,
    ) -> Result<u32> {
        Ok(self
            .ai_brains
            .get_mut(&owner)
            .and_then(|b| b.animation_waits.get_mut(&task))
            .context("Animation task is not constructed")?
            .task
            .update(delta))
    }
    pub(super) fn destroy_ai_animation_wait(&mut self, owner: usize, task: usize) -> Result<()> {
        self.ai_brains
            .get_mut(&owner)
            .context("Animation task owner is missing")?
            .animation_waits
            .remove(&task)
            .context("Animation task was already destroyed")?;
        //183a2c frees the task's local subscriptions; it does not unregister
        //the shared brain listener from the global dispatcher.
        Ok(())
    }
}
struct Binding<'a> {
    world: &'a mut World,
    task: usize,
}
impl Binding<'_> {
    fn has(&self, owner: usize, class: &str) -> Result<bool> {
        let entity = self
            .world
            .entities
            .get(owner)
            .context("Animation task owner is missing")?;
        let count = entity
            .components
            .iter()
            .filter(|c| c.class_name == class)
            .count();
        ensure!(
            count <= 1,
            "Animation task requires native component-slot arbitration for {class}"
        );
        Ok(count == 1)
    }
}
impl Host for Binding<'_> {
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>> {
        Ok(self.has(owner, "AnimationComponent")?.then_some(owner))
    }
    fn send_animation_event(&mut self, controller: usize, event: u32) -> Result<()> {
        self.world
            .send_native_actor_animation_event(controller, event)?;
        Ok(())
    }
    fn subscribe(&mut self, owner: usize, kind: u16, any_source: bool) -> Result<()> {
        ensure!(
            kind == 9 && !any_source,
            "Animation task requested an unknown subscription"
        );
        let owned = self
            .world
            .ai_brains
            .get_mut(&owner)
            .and_then(|b| b.animation_waits.get_mut(&self.task))
            .context("Activation lost its owned animation task")?;
        let fresh = !owned.subscribed;
        owned.subscribed = true;
        //183a84 retains an existing local registration on reactivation.
        if fresh {
            self.world
                .cutscene_actions
                .bind_animation_task(owner, &mut self.world.entity_event_registry)?;
        }
        Ok(())
    }
    fn has_agent(&mut self, owner: usize) -> Result<bool> {
        self.world.has_native_ai_agent(owner)
    }
    fn agent(&mut self, _: usize) -> Result<usize> {
        anyhow::bail!("Native AI agent lookup is not connected")
    }
    fn agent_heading(&mut self, _: usize, _: f32) -> Result<()> {
        anyhow::bail!("Native AI agent heading is not connected")
    }
    fn has_basic_controller(&mut self, owner: usize) -> Result<bool> {
        self.has(owner, "BasicAIControllerComponent")
    }
    fn basic_facing(&mut self, owner: usize, facing: [f32; 3]) -> Result<()> {
        self.world.publish_npc_ai_facing(owner, facing)
    }
    fn agent_40(&mut self, _: usize) -> Result<()> {
        anyhow::bail!("Native AI entity slot40 is not connected")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_task_receives_real_controller_and_clip_feedback_in_native_order() -> Result<()>
    {
        use super::super::entity_events::{Animation, Event};
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
            .context("Conversation slot absent")?;
        let definition = Definition {
            start_event: 0xc2,
            done_event: 0xc2,
            timeout: 5.,
            apply_heading: false,
            heading: -1.,
            agent_40: false,
        };
        let task = world.create_ai_animation_wait(owner, definition)?;
        // Explicit selection/child installation boundary. The real graph,
        // controller, shared ordinary dispatcher and source filters run below.
        world.ai_brains.get_mut(&owner).unwrap().selector.current = Some(slot);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .behavior_tasks
            .get_mut(&slot)
            .unwrap()
            .child = Some(task);
        world.activate_ai_animation_wait(owner, task)?;
        assert!(
            !world.ai_brains[&owner].animation_waits[&task]
                .task
                .completed,
            "Task subscribed before its initial start command"
        );
        assert!(world
            .cutscene_actions
            .animation_task_bound(owner, &world.entity_event_registry));
        let player = world.player_entity;
        world.dispatch_camera_animation_feedback(Animation {
            source: Some(player),
            header: None,
            id: 0xc2,
            flag: 1,
        })?;
        assert!(
            !world.ai_brains[&owner].animation_waits[&task]
                .task
                .completed,
            "Foreign actor completed Gus's task"
        );
        world.dispatch_camera_animation_feedback(Animation {
            source: None,
            header: None,
            id: 0xc2,
            flag: 1,
        })?;
        assert!(
            !world.ai_brains[&owner].animation_waits[&task]
                .task
                .completed,
            "Global null-source delivery bypassed the task's own-source filter"
        );
        world.ai_brains.get_mut(&owner).unwrap().selector.current = None;
        world.send_native_actor_animation_event(owner, 0xc2)?;
        assert!(
            !world.ai_brains[&owner].animation_waits[&task]
                .task
                .completed,
            "Noncurrent task received root forwarding"
        );
        world.ai_brains.get_mut(&owner).unwrap().selector.current = Some(slot);
        world.entity_event_queue.enqueue(
            &Event::Animation(Animation {
                source: Some(owner),
                header: None,
                id: 0xc2,
                flag: 1,
            }),
            None,
        )?;
        world.process_deferred_entity_events()?;
        assert_eq!(world.update_ai_animation_wait(owner, task, 0.)?, 1);
        assert_eq!(world.ai_brains[&owner].animation_waits[&task].task.kind, 1);
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .animation_waits
            .get_mut(&task)
            .unwrap()
            .task
            .finish(false);
        assert_eq!(world.update_ai_animation_wait(owner, task, 1.)?, 1);
        world.activate_ai_animation_wait(owner, task)?;
        let retained = &world.ai_brains[&owner].animation_waits[&task].task;
        assert_eq!(
            retained.kind, 1,
            "Reactivation's flag0 feedback missed the existing subscription"
        );
        assert_eq!(retained.elapsed, 1., "Activation reset the task clock");
        world.destroy_ai_animation_wait(owner, task)?;
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .behavior_tasks
            .get_mut(&slot)
            .unwrap()
            .child = None;
        // Settle the real Force_Idle transition without advancing unrelated
        // level scripts. These are the production actor update/export paths.
        for _ in 0..30 {
            let (events, unsupported) = world.authored_havok.step_and_export(owner)?;
            world.deliver_actor_exports(owner, events, unsupported);
        }
        let task = world.create_ai_animation_wait(
            owner,
            Definition {
                start_event: 0x71,
                done_event: 0x72,
                ..definition
            },
        )?;
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .behavior_tasks
            .get_mut(&slot)
            .unwrap()
            .child = Some(task);
        world.activate_ai_animation_wait(owner, task)?;
        let mut finished = false;
        for _ in 0..145 {
            let (events, unsupported) = world.authored_havok.step_and_export(owner)?;
            world.deliver_actor_exports(owner, events, unsupported);
            if world.update_ai_animation_wait(owner, task, super::super::STEP)? == 1 {
                finished = true;
                break;
            }
        }
        let wait = &world.ai_brains[&owner].animation_waits[&task].task;
        assert!(
            finished && wait.elapsed < 5.,
            "Original TalkStartup clip did not supply Done_TalkStartup before timeout: elapsed={}",
            wait.elapsed
        );
        assert_eq!(wait.kind, 1);
        let export_errors = world
            .report
            .unsupported
            .iter()
            .filter(|message| message.contains(" animation export "))
            .collect::<Vec<_>>();
        assert!(export_errors.is_empty(), "{export_errors:?}");
        // Conversation's actual agent_40=true contract must remain an error
        // until its navigation entity exists; do not fake that entity absent.
        let blocked = world.create_ai_animation_wait(
            owner,
            Definition {
                agent_40: true,
                ..definition
            },
        )?;
        let error = world
            .activate_ai_animation_wait(owner, blocked)
            .unwrap_err();
        assert!(
            error.to_string().contains("navigation-agent binding"),
            "{error:#}"
        );
        Ok(())
    }
}
