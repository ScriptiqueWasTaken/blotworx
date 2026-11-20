//! Ordinary Patrol action delivery from the original Lua commands30116c,
//!301344,301598. Movement/selection scheduling remains separate.
use super::{action_event_queue::ActionEvent, assets::World, entity_events::Event};
use anyhow::{ensure, Context, Result};
impl World {
    fn patrol_component_present(&self, entity: usize, class: &str) -> Result<bool> {
        let count = self
            .entities
            .get(entity)
            .context("Patrol entity is absent")?
            .components
            .iter()
            .filter(|component| component.class_name == class)
            .count();
        ensure!(
            count <= 1,
            "Patrol requires component-slot arbitration for {class}"
        );
        Ok(count == 1)
    }
    pub(super) fn dispatch_native_patrol_action(
        &mut self,
        owner: usize,
        action: u32,
        reference: Option<usize>,
    ) -> Result<()> {
        let entities = &self.entities;
        let removed = &self.removed_entities;
        let queue = &mut self.entity_event_queue;
        self.cutscene_actions.dispatch_resolved_with_patrol(
            ActionEvent {
                source: Some(owner),
                header: None,
                action,
                reference,
                vector_bits: [0; 4],
            },
            &mut self.entity_event_registry,
            &mut self.ai_brains,
            &mut |index| {
                if removed.contains(&index) {
                    return Ok(None);
                }
                Ok(entities.get(index).map(|entity| {
                    super::native_hk_transform::entity_position_vector(
                        entity.transform[12..15].try_into().unwrap(),
                    )
                    .map(f32::to_bits)
                }))
            },
            &mut |event| {
                queue.enqueue(&Event::Basic(event), None)?;
                Ok(())
            },
        )
    }
    pub(super) fn patrol_command_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        let expected = if name == "SetNextPatrolNode" { 2 } else { 1 };
        if args.len() != expected {
            self.report.warnings.push(format!("{name}: native argument count diagnostic ({} arguments); continued with source stack order",args.len()));
        }
        let target = if expected == 2 {
            self.scripts
                .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
        } else {
            None
        };
        let owner = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|index| !self.removed_entities.contains(index));
        let Some(owner) = owner else {
            return Ok(mlua::MultiValue::new());
        };
        if !self
            .patrol_component_present(owner, "BasicAIControllerComponent")
            .map_err(mlua::Error::external)?
        {
            return Ok(mlua::MultiValue::new());
        }
        if expected == 2 {
            let Some(target) = target.filter(|index| !self.removed_entities.contains(index)) else {
                return Ok(mlua::MultiValue::new());
            };
            if !self
                .patrol_component_present(target, "PatrolNodeComponent")
                .map_err(mlua::Error::external)?
            {
                return Ok(mlua::MultiValue::new());
            }
        }
        self.dispatch_native_patrol_action(
            owner,
            if name == "ResumePatrol" { 26 } else { 25 },
            target,
        )
        .map_err(mlua::Error::external)?;
        // Keep the explicitly provisional combat planner's route in agreement
        // with accepted commands until native task/controller scheduling lands.
        match name {
            "SetNextPatrolNode" => {
                self.demo_story.ai.set_patrol_destination(owner, target);
            }
            "ClearNextPatrolNode" => {
                self.demo_story.ai.set_patrol_destination(owner, None);
            }
            "ResumePatrol" => {}
            _ => return Err(mlua::Error::external("Unknown Patrol command")),
        }
        Ok(mlua::MultiValue::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_patrol_lua_actions_reach_receivers_queue_and_preserve_disabled_motion() -> Result<()>
    {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .ai_brains
            .iter()
            .find_map(|(&owner, brain)| {
                (!brain.patrol.is_empty()
                    && world
                        .character_physics
                        .get(&owner)
                        .is_some_and(Result::is_ok))
                .then_some(owner)
            })
            .context("Original actor with Patrol and a body missing")?;
        let slot = *world.ai_brains[&owner].patrol.keys().next().unwrap();
        let node = world
            .entities
            .iter()
            .position(|entity| {
                entity
                    .components
                    .iter()
                    .any(|c| c.class_name == "PatrolNodeComponent")
            })
            .context("Original PatrolNode missing")?;
        let owner_name = world.entities[owner].name.clone();
        ensure!(
            world.npc_relocations[&owner]
                .turn_rates
                .initialization_applied,
            "BasicMotion turn caches were not initialized before ordinary gameplay"
        );
        let motion_properties = world.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "BasicMotionControllerComponent")
            .context("Original actor motion properties missing")?
            .properties
            .iter()
            .map(|p| (p.name.clone(), p.value.clone()))
            .collect::<Vec<_>>();
        world.run_callback_fixture(&format!("SetOverrideTurnRate({owner_name:?},0.75)"))?;
        world.initialize_npc_motion_turn_rates(owner..owner + 1)?;
        ensure!(
            world.npc_relocations[&owner].turn_rates.stationary == Some(0.75f32.to_bits())
                && world.npc_relocations[&owner].turn_rates.moving == Some(0.75f32.to_bits()),
            "Native turn override did not store its raw scalar in both caches"
        );
        world.run_callback_fixture(&format!("ResetOverrideTurnRate({owner_name:?})"))?;
        ensure!(
            world.npc_relocations[&owner]
                .turn_rates
                .stationary
                .is_some()
                && world.npc_relocations[&owner].turn_rates.moving.is_some(),
            "Native cache reset did not read the actual actor properties"
        );
        let after = world.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "BasicMotionControllerComponent")
            .unwrap()
            .properties
            .iter()
            .map(|p| (p.name.clone(), p.value.clone()))
            .collect::<Vec<_>>();
        ensure!(
            motion_properties == after,
            "Cache override/reset modified the actor's authored property values"
        );
        let node_name = world.entities[node].name.clone();
        let player_name = world.entities[world.player_entity].name.clone();
        world.run_callback_fixture(&format!(
            "SetNextPatrolNode({owner_name:?},{player_name:?})"
        ))?;
        ensure!(
            world.ai_brains[&owner].patrol[&slot].target.is_none(),
            "Non-node destination was accepted"
        );
        world.run_callback_fixture(&format!("SetNextPatrolNode({owner_name:?},{node_name:?})"))?;
        ensure!(
            world.ai_brains[&owner].patrol[&slot].target == Some(node)
                && world.ai_brains[&owner].patrol[&slot].phase == 1,
            "Lua did not synchronously update the native Patrol receiver"
        );
        world
            .ai_brains
            .get_mut(&owner)
            .unwrap()
            .patrol
            .get_mut(&slot)
            .unwrap()
            .phase = 3;
        world.run_callback_fixture(&format!("ClearNextPatrolNode({owner_name:?})"))?;
        ensure!(
            world.ai_brains[&owner].patrol[&slot].target.is_none(),
            "Clear did not deliver null action25 reference"
        );
        ensure!(world.entity_event_queue.entries().iter().any(|entry|matches!(&entry.event,
            Event::Basic(event) if event.event_kind==0x8e && event.source==Some(node) && event.header==Some(owner))),
            "Departure lost its original node source/NPC activator");
        world.set_entity_motion_enabled(owner, false)?;
        {
            let patrol = world
                .ai_brains
                .get_mut(&owner)
                .unwrap()
                .patrol
                .get_mut(&slot)
                .unwrap();
            patrol.phase = 4;
            patrol.wait = 9.;
        }
        let motion = world.character_physics[&owner]
            .as_ref()
            .unwrap()
            .motion
            .current
            .motion_type();
        world.run_callback_fixture(&format!("ResumePatrol({owner_name:?})"))?;
        ensure!(
            world.ai_brains[&owner].patrol[&slot].phase == 8
                && world.ai_brains[&owner].patrol[&slot].wait == 0.,
            "Resume did not advance the native Patrol sequence"
        );
        ensure!(
            world.character_physics[&owner]
                .as_ref()
                .unwrap()
                .motion
                .current
                .motion_type()
                == motion,
            "ResumePatrol incorrectly enabled motion"
        );
        // The deferred queue must expose the same receiver and may enqueue a
        // departure during its current pass, preserving normal queue semantics.
        world.entity_event_queue.enqueue(
            &Event::Action(ActionEvent {
                source: Some(owner),
                header: None,
                action: 25,
                reference: Some(node),
                vector_bits: [0; 4],
            }),
            None,
        )?;
        world.process_deferred_entity_events()?;
        ensure!(
            world.ai_brains[&owner].patrol[&slot].target == Some(node),
            "Deferred Patrol action was not delivered"
        );
        Ok(())
    }
}
