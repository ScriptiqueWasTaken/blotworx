//! Application-owned MickeyTracker entity (REL129ec..12b50) and camera proxy.
use super::{
    assets::{RuntimeEntity, World},
    entity_tracker::{self, Runtime},
    physics::Player,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
impl World {
    pub(super) fn install_mickey_tracker(&mut self, files: &[VirtualFile]) -> Result<()> {
        ensure!(
            self.mickey_tracker.is_none(),
            "MickeyTracker already installed"
        );
        let global = files
            .iter()
            .find(|file| {
                file.path
                    .replace('\\', "/")
                    .eq_ignore_ascii_case("Palettes/_Dynamic.bin")
            })
            .context("MickeyTracker global palette missing")?;
        let scene = super::scene_loading::parse_scene(&global.path, &global.data)?;
        let definition = scene
            .objects
            .iter()
            .find(|entity| entity.name.eq_ignore_ascii_case("MickeyTracker"))
            .context("MickeyTracker prefab missing")?;
        let components =
            crate::render_support::scene::get_effective_components(definition, &scene.objects);
        ensure!(
            components.iter().all(|c| matches!(
                c.class_name.as_str(),
                "JPSTransformationComponent" | "InputOutputComponent" | "EntityTrackerComponent"
            )),
            "MickeyTracker prefab contains an unimplemented component lifecycle"
        );
        let component = components
            .iter()
            .find(|c| c.class_name == "EntityTrackerComponent")
            .context("MickeyTracker component missing")?;
        let parameters = entity_tracker::Parameters::load(component)?;
        let tuning = entity_tracker::Tuning::load(&self.runtime_settings)?;
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let transform = super::render_transform::from_entity(identity, [0.; 3], 1.);
        let id = self
            .entities
            .iter()
            .map(|entity| entity.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("MickeyTracker entity ID exhausted")?;
        let owner = self.entities.len();
        let target = self.player_entity;
        self.entities.push(RuntimeEntity {
            authored_scale: 1.,
            authored_rotation: identity,
            runtime_rotation: identity,
            model_asset: None,
            mesh_draws: Default::default(),
            jigsaw_asset: None,
            jigsaw_states: vec![],
            scene: global.path.clone(),
            id,
            name: definition.name.clone(),
            transform,
            authored_transform: transform,
            components,
            draws: self.draws.len()..self.draws.len(),
            collision_parts: self.report.collision_parts.len()..self.report.collision_parts.len(),
        });
        self.scripts
            .register_entity(owner, &self.entities[owner].name, &[])?;
        self.mickey_tracker = Some(Runtime {
            owner,
            target,
            parameters,
            tuning,
            state: Default::default(),
            error: None,
        });
        self.reset_mickey_tracker()?;
        self.assign_camera_target(Some(owner), true)?;
        self.report.geometry.push(
            serde_json::json!({"kind":"mickey_tracker","entity":owner,"target":target,
            "source":global.path,"prefab":definition.name,"original_update_order_validated":false}),
        );
        Ok(())
    }
    pub(super) fn reset_mickey_tracker(&mut self) -> Result<()> {
        let Some(tracker) = &mut self.mickey_tracker else {
            return Ok(());
        };
        let target = self
            .entities
            .get(tracker.target)
            .context("MickeyTracker reset target missing")?;
        tracker.state.reset(
            &tracker.parameters,
            target.transform[12..15].try_into().unwrap(),
            target.runtime_rotation,
            Some(self.script_player.motion_state()),
        )?;
        tracker.error = None;
        let entity = &mut self.entities[tracker.owner];
        entity.runtime_rotation = tracker.state.rotation;
        entity.transform = super::render_transform::from_entity(
            tracker.state.rotation,
            tracker.state.position,
            1.,
        );
        Ok(())
    }
    pub(super) fn update_mickey_tracker(&mut self, player: &Player) {
        let Some(tracker) = &mut self.mickey_tracker else {
            return;
        };
        // Native null target leaves the prior publication intact.
        if self.removed_entities.contains(&tracker.target)
            || self.removed_entities.contains(&tracker.owner)
        {
            return;
        }
        let result = (|| -> Result<()> {
            let target = &self.entities[tracker.target];
            let ground = player
                .motion_ground
                .as_ref()
                .context("Tracker target motion ground record is not published")?;
            let distance = ground
                .distance
                .context("Tracker ground clearance unavailable")?;
            let velocity = ground
                .surface_velocity
                .context("Tracker support body velocity ownership unresolved")?;
            let point = ground
                .last_hit
                .context("Tracker native retained ground point is uninitialized")?;
            let component = target
                .components
                .iter()
                .position(|c| c.class_name == "MickeyPlayerMotionControllerComponent")
                .context("Tracker target motion component missing")?;
            // Base ctor69e38 clears byte20 bit6; getter56624 reads it.
            // Explicitly published disabled state overrides that initial value.
            let inactive = self
                .component_control
                .disabled
                .get(&tracker.target)
                .and_then(|states| states.get(&component))
                .copied()
                .unwrap_or(false);
            tracker.state.update(
                &tracker.parameters,
                tracker.tuning,
                player.position,
                target.runtime_rotation,
                Some(entity_tracker::Motion {
                    state: player.motion_state(),
                    support: ground.support,
                    distance,
                    surface_velocity_y: velocity[1],
                    ground_point: point[..3].try_into().unwrap(),
                    maximum_speed: player.max_move_speed(),
                    inactive,
                }),
                false,
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                tracker.error = None;
                let entity = &mut self.entities[tracker.owner];
                entity.runtime_rotation = tracker.state.rotation;
                entity.transform = super::render_transform::from_entity(
                    tracker.state.rotation,
                    tracker.state.position,
                    1.,
                );
            }
            Err(error) => {
                let message = format!("MickeyTracker update: {error:#}");
                if !self.report.unsupported.contains(&message) {
                    self.report.unsupported.push(message.clone());
                }
                tracker.error = Some(message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_tracker_prefab_publishes_motion_and_camera_focus() -> Result<()> {
        use super::super::{
            assets,
            controller::{Controller, Input, TickPhase},
        };
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let tracker = world
            .mickey_tracker
            .as_ref()
            .context("Original tracker not installed")?;
        let owner = tracker.owner;
        let target = tracker.target;
        assert_eq!(target, world.player_entity);
        assert_eq!(tracker.parameters.buffer_size, 2);
        assert_eq!(tracker.parameters.offset, [0., 0.7, 0.]);
        assert!(tracker.parameters.ground_on);
        assert_eq!(tracker.tuning.up, 0.1);
        assert_eq!(tracker.tuning.down, 0.2);
        assert_eq!(
            world.scripts.resolve_name(&world.entities[owner].name),
            Some(owner)
        );
        world.attach_player_body()?;
        let camera = (0..world.entities.len())
            .find(|&i| {
                world
                    .entity_bound_hierarchy(i)
                    .ok()
                    .flatten()
                    .is_some_and(|h| !h.cameras.is_empty())
            })
            .context("Original authored camera missing")?;
        world.select_nif_camera(Some(camera), Some(target), 1, 0.)?;
        assert_eq!(world.nif_camera.unwrap().focus, Some(owner));
        let initial = world
            .active_nif_camera_frame()?
            .context("Focused camera missing")?;
        let mut controller =
            Controller::new(Player::new(world.report.spawn, world.report.motion.clone()));
        for tick in 0..90 {
            controller.advance_with_tick_phases(
                &mut world,
                |w| &w.collision,
                1. / 30.,
                true,
                Input {
                    movement: if tick > 30 { [0., 1.] } else { [0.; 2] },
                    jump_pressed: tick == 60,
                    jump_held: (60..66).contains(&tick),
                    ..Default::default()
                },
                |phase, player, _, _, _, world| -> Result<()> {
                    if phase == TickPhase::BeforeMovement {
                        world.publish_player_ground_record(player)?;
                    }
                    if phase == TickPhase::SimulatePhysics {
                        world.advance_player_physics(player, super::super::STEP)?;
                    }
                    if phase == TickPhase::AfterMovement {
                        world.entities[world.player_entity]
                            .set_controller_pose(player.position, player.yaw);
                        world.update_mickey_tracker(player);
                        ensure!(
                            world.mickey_tracker.as_ref().unwrap().error.is_none(),
                            "Tracker input failed at tick {tick}: {:?}; {:?}",
                            world.mickey_tracker.as_ref().unwrap().error,
                            player.motion_ground
                        );
                    }
                    Ok(())
                },
            )?;
        }
        let tracker = world.mickey_tracker.as_ref().unwrap();
        assert_eq!(tracker.state.updates, 90);
        assert_eq!(
            world.camera_focus_point(owner)?,
            Some(tracker.state.position)
        );
        let frame = world
            .active_nif_camera_frame()?
            .context("Live player focus missing")?;
        assert_eq!(frame.eye, initial.eye);
        assert_ne!(frame.direction, initial.direction);
        assert_eq!(controller.player.respawns, 0);
        Ok(())
    }
}
