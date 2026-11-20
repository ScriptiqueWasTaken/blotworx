//! Synchronous player relocation through the live script host. Retail request
//! flags / probe selection are retained; Box3D cast/solver parity and the full
//! character context reset remain required compatibility work.
use super::{
    assets::World,
    native_hk_transform as hk,
    physics::{self, Player},
};
use anyhow::{ensure, Context, Result};
impl World {
    /// Transfer startup state once. Keep a reusable parking slot so script ticks
    /// can borrow the actual controller without allocation, cloning or pointers.
    pub fn take_startup_player(&mut self) -> Player {
        self.script_player_available = false;
        std::mem::replace(
            &mut self.script_player,
            Player::new(self.report.spawn, self.report.motion.clone()),
        )
    }
    pub fn with_live_player<R>(
        &mut self,
        player: &mut Player,
        run: impl FnOnce(&mut Self) -> Result<R>,
    ) -> Result<R> {
        let previous = self.script_player_available;
        std::mem::swap(&mut self.script_player, player);
        self.script_player_available = true;
        self.script_player.script_input_restricted = self.cutscene_input.restricted;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self)));
        self.script_player_available = previous;
        std::mem::swap(&mut self.script_player, player);
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    pub(super) fn move_player_to_entity(
        &mut self,
        request: super::entity_relocation::Request,
    ) -> Result<()> {
        ensure!(
            self.script_player_available,
            "Player relocation requires the live controller script scope"
        );
        let target = self
            .entities
            .get(request.target)
            .context("Player destination is not loaded")?;
        let rotation = target.runtime_rotation;
        let mut position = hk::entity_position_vector(target.transform[12..15].try_into().unwrap());
        if request.probe {
            let segment = super::controller_ground::prepare(&mut position, 4.);
            let flags = super::physics_filter::QueryFlags {
                bytes: [1, 1, 1, 0, 1],
                excluded_group: 0,
            };
            // REL305774 selects602003 for the primary player,600003 otherwise.
            // This entry point is exclusively the primary player's controller.
            let mask = 0x0060_2003;
            let collision = &self.collision;
            let filter = &mut self.dynamic_runtime_filter;
            let hits = collision.with_player_queries(|| {
                collision.sphere_hits_game_query(
                    segment.start[..3].try_into().unwrap(),
                    physics::sub(
                        segment.end[..3].try_into().unwrap(),
                        segment.start[..3].try_into().unwrap(),
                    ),
                    segment.margin,
                    filter,
                    Some(flags),
                    |shared, candidate| {
                        Ok(shared.collision_info(
                            mask,
                            collision.simulation_filter_word(candidate.shape)?,
                        ))
                    },
                )
            })?;
            let mut collector = super::cast_contacts::Collector::default();
            let query = usize::MAX;
            collector.begin_query(query);
            for hit in hits {
                collector.push(super::cast_contacts::RawContact {
                    position: [hit.position[0], hit.position[1], hit.position[2], 0.]
                        .map(f32::to_bits),
                    normal: [hit.normal[0], hit.normal[1], hit.normal[2], hit.fraction]
                        .map(f32::to_bits),
                    collidables: [query, hit.shape],
                    shape_keys: [u32::MAX, u32::MAX],
                })?;
            }
            let distance = super::controller_ground::select_collector(
                position,
                4.,
                &mut collector,
                |shape| self.collision.simulation_filter_word(shape),
            )?;
            if distance < 4. {
                position[1] -= distance;
            }
        }
        self.relocate_player_pose(position[..3].try_into().unwrap(), rotation, request.reset)
    }
    /// Common publication for MoveToEntity and SetTransformation. This retains
    /// the admitted controller reset; full native action/animation reset is open.
    pub(super) fn relocate_player_pose(
        &mut self,
        position: [f32; 3],
        rotation: [f32; 9],
        reset: bool,
    ) -> Result<()> {
        ensure!(
            self.script_player_available,
            "Player relocation requires the live controller script scope"
        );
        ensure!(
            position
                .iter()
                .chain(rotation.iter())
                .all(|x| x.is_finite()),
            "Nonfinite player relocation pose"
        );
        self.side_scroller.plane=None;
        self.script_player.side_scroll_plane=None;
        let source = &self.entities[self.player_entity];
        ensure!(
            source.transform_parent_id()?.is_none(),
            "Player relocation requires parent detachment integration"
        );
        let entity = &mut self.entities[self.player_entity];
        entity.runtime_rotation = rotation;
        entity.transform =
            super::render_transform::from_entity(rotation, position, entity.authored_scale);
        for draw in entity.draws.clone() {
            self.draws[draw].trans_mat = entity.transform;
        }
        super::rotator_runtime::accept_physics_pose(
            self.player_entity,
            &mut self.transform_hierarchy,
            &mut self.entities,
            &mut self.draws,
        )?;
        if reset {
            let yaw = super::native_angle::atan2(rotation[2] as f64, rotation[8] as f64) as f32;
            self.script_player.accept_script_relocation(position, yaw);
        } else {
            self.script_player.position = position;
            self.script_player.previous = position;
        }
        if let Some(body) = &mut self.player_physics {
            let transform = self.entities[self.player_entity].physics_transform();
            body.record.set_transform(transform)?;
            body.orientation = body.record.swept_state().orientation;
            self.collision
                .set_shape_transform(body.shape, position, body.orientation)?;
            self.collision
                .set_simulation_velocity(body.shape, self.script_player.velocity)?;
            body.record.set_linear_velocity([
                self.script_player.velocity[0],
                self.script_player.velocity[1],
                self.script_player.velocity[2],
                0.,
            ]);
        }
        self.refresh_relocated_model(self.player_entity, self.animation_time)?;
        if self.entities[self.player_entity]
            .components
            .iter()
            .any(|c| c.class_name == "LODComponent")
        {
            self.reset_relocated_lod(self.player_entity)?;
        }
        self.scene_motion_prepared_at = None;
        let limitation="Player MoveToEntity and SetTransformation are connected to live pose, body and supported controller reset. Original cast contact/order parity, complete character-state/animation reset and native component publication ordering remain unvalidated.";
        if !self
            .report
            .unsupported
            .iter()
            .any(|message| message == limitation)
        {
            self.report.unsupported.push(limitation.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_player_lua_relocation_is_synchronous_and_survives_the_next_gameplay_tick(
    ) -> Result<()> {
        use super::super::controller::{Controller, Input, TickPhase};
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let marker = world
            .entities
            .iter()
            .position(|entity| entity.name.ends_with(".ExitProjectorMarker"))
            .context("Original GV projector entry marker not placed")?;
        let target = world.entities[marker].transform[12..15].to_vec();
        let name = world.entities[marker].name.clone();
        let mut controller = Controller::new(world.take_startup_player());
        world.attach_player_body()?;
        let shape = world.player_physics.as_ref().unwrap().shape;
        let deaths = controller.player.respawns;
        let ownership = serde_json::to_value(&world.player_abilities)?;
        let script=format!("MoveToEntity(GetPlayer(),{name:?},false,false); local p=GetPosition(GetPlayer()); assert(p.r=={:.17e} and p.z=={:.17e}); BwxMoved=true; wait(.1); assert(BwxMoved); TeleportToEntity(GetPlayer(),{name:?})",target[0],target[2]);
        world.with_live_player(&mut controller.player, |world| {
            world.run_script(&script)?;
            world.advance_models(0.)
        })?;
        assert_eq!(controller.player.position[0], target[0]);
        assert_eq!(controller.player.position[2], target[2]);
        assert_eq!(controller.player.position, controller.player.previous);
        assert_eq!(
            world.entities[world.player_entity].transform[12..15],
            controller.player.position
        );
        assert!(super::super::render_transform::is_affine(
            &world.entities[world.player_entity].transform
        ));
        assert_eq!(
            world.collision.simulation_pose(shape)?.position,
            controller.player.position
        );
        assert_eq!(controller.player.respawns, deaths);
        assert_eq!(serde_json::to_value(&world.player_abilities)?, ownership);
        // A warp preserves the old swept base time, even if the world's
        // current time has advanced. The former hand-written path stamped now.
        {
            let body = world.player_physics.as_mut().unwrap();
            let mut state = body.record.swept_state();
            state.previous_center[3] = -8.25;
            state.center[3] = 30.;
            state.delta_angle = [1., 2., 3., 4.];
            body.record.set_swept_state(&state);
        }
        world.dynamic_runtime_time = 99.;
        world.with_live_player(&mut controller.player, |world| {
            world.move_player_to_entity(super::super::entity_relocation::Request {
                entity: world.player_entity,
                target: marker,
                reset: false,
                probe: false,
            })
        })?;
        let state = world.player_physics.as_ref().unwrap().record.swept_state();
        assert_eq!(state.previous_center[3], -8.25);
        assert_eq!(state.center[3], 0.);
        assert_eq!(state.delta_angle, [0.; 4]);
        assert_eq!(state.previous_orientation, state.orientation);
        assert!(!world.script_player_available);
        let error: Result<()> = world.with_live_player(&mut controller.player, |_| {
            anyhow::bail!("Expected scope failure")
        });
        assert!(error.is_err());
        assert!(!world.script_player_available);
        assert_eq!(controller.player.position[0], target[0]);
        let mut completed = 0;
        for _ in 0..5 {
            controller.advance_with_tick_phases(
                &mut world,
                |world| &world.collision,
                super::super::STEP as f64,
                true,
                Input::default(),
                |phase, player, tick, _, _, world| -> Result<()> {
                    if phase == TickPhase::SimulatePhysics {
                        world.advance_player_physics(player, super::super::STEP)?;
                    }
                    if phase == TickPhase::AfterMovement {
                        world.advance_player_models(player, tick as f32 * super::super::STEP)?;
                        completed += 1;
                    }
                    Ok(())
                },
            )?;
        }
        assert_eq!(completed, 5);
        assert_eq!(controller.player.position[0], target[0]);
        assert_eq!(controller.player.position[2], target[2]);
        assert!(
            !world
                .report
                .unsupported
                .iter()
                .any(|message| message.contains("runtime animation console")),
            "{:?}",
            world.report.unsupported
        );
        Ok(())
    }
}
