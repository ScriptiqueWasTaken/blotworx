//! Owned turn tasks and common198a54 replacement in the live World. This is
//! task execution, not an invented navigation entity or behavior selector.
use super::{
    ai_turn_task::{Host, Turn},
    assets::World,
};
use anyhow::{ensure, Context, Result};

impl World {
    pub(super) fn has_native_ai_agent(&self, owner: usize) -> Result<bool> {
        let actor = self
            .entities
            .get(owner)
            .context("AI task owner is absent")?;
        let count = actor
            .components
            .iter()
            .filter(|c| c.class_name == "BasicAIControllerComponent")
            .count();
        ensure!(
            count <= 1,
            "AI agent query requires native component-slot arbitration"
        );
        if count == 0 {
            return Ok(false);
        }
        //18b0e4 queries BasicAI+40 then its wrapper+18. The portable World
        //has not constructed these objects. This is not a native null result.
        anyhow::bail!(
            "AI task requires the actual BasicAI navigation-agent binding for {}",
            actor.name
        )
    }
    pub(super) fn create_ai_turn(&mut self, owner: usize, turn: Turn) -> Result<usize> {
        let brain = self
            .ai_brains
            .get_mut(&owner)
            .context("Turn owner lacks an AI brain")?;
        let id = brain.next_task_identity;
        brain.next_task_identity = id.checked_add(1).context("AI task identities exhausted")?;
        brain.turns.insert(id, turn);
        Ok(id)
    }
    fn turn_definition(&self, owner: usize, task: usize) -> Result<Turn> {
        Ok(self
            .ai_brains
            .get(&owner)
            .and_then(|b| b.turns.get(&task))
            .context("AI turn task is absent")?
            .clone())
    }
    pub(super) fn activate_ai_turn(&mut self, owner: usize, task: usize) -> Result<()> {
        self.turn_definition(owner, task)?
            .activate(owner, &mut Binding { world: self })
    }
    pub(super) fn update_ai_turn(&mut self, owner: usize, task: usize) -> Result<u32> {
        let completion = self
            .turn_definition(owner, task)?
            .completion(owner, &mut Binding { world: self })?;
        if let Some(kind) = completion {
            self.ai_brains
                .get_mut(&owner)
                .and_then(|b| b.turns.get_mut(&task))
                .context("Controller callback removed its active turn task")?
                .kind = kind;
            Ok(1)
        } else {
            Ok(0)
        }
    }
    pub(super) fn ai_child_kind(&self, owner: usize, task: usize) -> Result<u32> {
        let brain = self
            .ai_brains
            .get(&owner)
            .context("AI child owner is absent")?;
        if let Some(task) = brain.animation_waits.get(&task) {
            return Ok(task.task.kind);
        }
        if let Some(task) = brain.turns.get(&task) {
            return Ok(task.kind);
        }
        anyhow::bail!("AI child kind requires its constructed concrete task")
    }
    pub(super) fn stop_ai_child(&mut self, owner: usize, task: usize) -> Result<()> {
        let brain = self
            .ai_brains
            .get(&owner)
            .context("AI child owner is absent")?;
        if brain.animation_waits.contains_key(&task) {
            return Ok(());
        } //1988a4
        self.turn_definition(owner, task)?
            .stop(owner, &mut Binding { world: self })
    }
    pub(super) fn destroy_ai_child(&mut self, owner: usize, task: usize) -> Result<()> {
        let brain = self
            .ai_brains
            .get_mut(&owner)
            .context("AI child owner is absent")?;
        if brain.turns.remove(&task).is_some() {
            return Ok(());
        }
        self.destroy_ai_animation_wait(owner, task)
    }
    pub(super) fn install_ai_child(
        &mut self,
        owner: usize,
        slot: usize,
        child: usize,
        wait: bool,
    ) -> Result<()> {
        let old = self
            .ai_brains
            .get(&owner)
            .and_then(|b| b.behavior_tasks.get(&slot))
            .context("AI child parent is absent")?
            .child;
        if let Some(old) = old {
            let kind = self.ai_child_kind(owner, old)?;
            self.ai_brains
                .get_mut(&owner)
                .unwrap()
                .behavior_tasks
                .get_mut(&slot)
                .unwrap()
                .previous_kind = kind;
            self.stop_ai_child(owner, old)?;
            //198a54 reads child afresh after stop. It can be replaced by a
            //callback; never destroy only a stale pre-callback handle.
            if let Some(current) = self
                .ai_brains
                .get(&owner)
                .and_then(|b| b.behavior_tasks.get(&slot))
                .context("Stop removed its AI parent")?
                .child
            {
                self.destroy_ai_child(owner, current)?;
            }
        }
        self.ai_brains
            .get_mut(&owner)
            .context("AI parent brain is absent")?
            .behavior_tasks
            .get_mut(&slot)
            .context("AI child parent is absent")?
            .child = Some(child);
        if self.ai_brains[&owner].turns.contains_key(&child) {
            self.activate_ai_turn(owner, child)?;
        } else {
            self.activate_ai_animation_wait(owner, child)?;
        }
        if wait {
            self.ai_brains
                .get_mut(&owner)
                .context("Activation removed its AI brain")?
                .behavior_tasks
                .get_mut(&slot)
                .context("Activation removed its AI parent")?
                .mode = 1;
        }
        Ok(())
    }
}
struct Binding<'a> {
    world: &'a mut World,
}
impl Binding<'_> {
    fn position(&self, entity: usize) -> Result<Option<[f32; 4]>> {
        if self.world.removed_entities.contains(&entity) {
            return Ok(None);
        }
        Ok(self
            .world
            .entities
            .get(entity)
            .map(|e| [e.transform[12], e.transform[13], e.transform[14], 0.]))
    }
    fn basic(&self, owner: usize) -> Result<bool> {
        let count = self
            .world
            .entities
            .get(owner)
            .context("AI turn owner is absent")?
            .components
            .iter()
            .filter(|c| c.class_name == "BasicAIControllerComponent")
            .count();
        ensure!(
            count <= 1,
            "AI turn requires native component-slot arbitration"
        );
        Ok(count == 1)
    }
}
impl Host for Binding<'_> {
    fn resolve_position(&mut self, entity: usize) -> Result<Option<[f32; 4]>> {
        self.position(entity)
    }
    fn owner_position(&mut self, owner: usize) -> Result<[f32; 4]> {
        self.position(owner)?
            .context("AI turn owner has no live position")
    }
    fn owner_facing(&mut self, owner: usize) -> Result<[f32; 3]> {
        let r = self
            .world
            .entities
            .get(owner)
            .context("AI turn owner is absent")?
            .runtime_rotation;
        Ok([r[2], r[5], r[8]])
    }
    fn has_agent(&mut self, owner: usize) -> Result<bool> {
        self.world.has_native_ai_agent(owner)
    }
    fn agent_heading_degrees(&mut self, _: usize, _: f32) -> Result<()> {
        anyhow::bail!("Native AI agent heading is not connected")
    }
    fn has_basic_controller(&mut self, owner: usize) -> Result<bool> {
        self.basic(owner)
    }
    fn basic_facing(&mut self, owner: usize, facing: [f32; 3]) -> Result<()> {
        self.world.publish_npc_ai_facing(owner, facing)
    }
    fn stop_basic_controller(&mut self, owner: usize) -> Result<()> {
        self.world.stop_npc_ai_turn(owner)
    }
}
