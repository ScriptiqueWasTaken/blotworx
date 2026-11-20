//! Concrete selected StandIdle receiver. Selection must establish the current
//! slot; an ExitCutscene action does not select this behavior by itself.
use super::{
    ai_stand_idle::{Host, StandIdle},
    assets::World,
};
use anyhow::{ensure, Context, Result};

impl World {
    fn require_selected_stand_idle(&self, owner: usize) -> Result<()> {
        let brain = self
            .ai_brains
            .get(&owner)
            .context("StandIdle brain is absent")?;
        let slot = brain
            .selector
            .current
            .context("StandIdle has no selected behavior")?;
        ensure!(
            brain
                .behaviors
                .get(slot)
                .is_some_and(|name| name == "StandIdle"),
            "Selected behavior is not StandIdle"
        );
        Ok(())
    }
    pub(super) fn enter_selected_ai_stand_idle(&mut self, owner: usize) -> Result<()> {
        self.require_selected_stand_idle(owner)?;
        StandIdle.enter(owner, &mut Binding { world: self })
    }
    pub(super) fn update_selected_ai_stand_idle(&self, owner: usize) -> Result<u32> {
        self.require_selected_stand_idle(owner)?;
        Ok(StandIdle.update_result())
    }
}
struct Binding<'a> {
    world: &'a mut World,
}
impl Host for Binding<'_> {
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>> {
        let entity = self
            .world
            .entities
            .get(owner)
            .context("StandIdle owner is absent")?;
        let count = entity
            .components
            .iter()
            .filter(|c| c.class_name == "AnimationComponent")
            .count();
        ensure!(
            count <= 1,
            "StandIdle requires native animation-component slot arbitration"
        );
        Ok((count == 1).then_some(owner))
    }
    fn send_animation_event(&mut self, controller: usize, event: u32) -> Result<()> {
        self.world
            .send_native_actor_animation_event(controller, event)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gus_rejects_undeclared_stand_idle_without_forced_selection() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        let owner = world
            .scripts
            .resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus missing")?;
        ensure!(
            world.enter_selected_ai_stand_idle(owner).is_err(),
            "An unselected behavior must not send an idle animation"
        );
        let brain = &world.ai_brains[&owner];
        println!(
            "Original Gus root {:?}, behaviors {:?}",
            brain.root, brain.behaviors
        );
        ensure!(
            !brain.behaviors.iter().any(|name| name == "StandIdle"),
            "The imported Gus fixture changed; reassess its declared behaviors"
        );
        for slot in 0..brain.behaviors.len() {
            // Verify rejection with each actual declared slot. Never append
            // a synthetic behavior to make this actor accept an idle reset.
            world.ai_brains.get_mut(&owner).unwrap().selector.current = Some(slot);
            ensure!(
                world.enter_selected_ai_stand_idle(owner).is_err(),
                "An unrelated behavior was treated as StandIdle"
            );
            ensure!(
                world.update_selected_ai_stand_idle(owner).is_err(),
                "An unrelated behavior was updated as StandIdle"
            );
        }
        world.ai_brains.get_mut(&owner).unwrap().selector.current = None;
        Ok(())
    }
}
