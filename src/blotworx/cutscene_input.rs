//! Player restriction subset of EM1 RELd3e0/d450. Native input-context10,
//! menu guards, cursor/modal UI and return-context publication remain open.
#[derive(Default)]
pub struct State {
    pub count: u32,
    pub restricted: bool,
}
impl State {
    pub fn restrict(&mut self) {
        self.count = self.count.wrapping_add(1);
        self.restricted = true;
    }
    pub fn unrestrict(&mut self) {
        if self.count == 0 {
            return;
        }
        if self.count == 1 {
            self.restricted = false;
        }
        self.count -= 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_restrictions_release_player_only_on_last_unrestrict_and_preserve_native_wrap() {
        let mut state = State::default();
        state.unrestrict();
        assert!(!state.restricted);
        state.restrict();
        state.restrict();
        state.unrestrict();
        assert_eq!(state.count, 1);
        assert!(state.restricted);
        state.unrestrict();
        assert_eq!(state.count, 0);
        assert!(!state.restricted);
        state.count = u32::MAX;
        state.restrict();
        assert_eq!(state.count, 0);
        assert!(state.restricted);
        state.unrestrict();
        assert!(state.restricted);
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_world_scripts_block_player_controls_without_stopping_physics_or_coroutines(
    ) -> anyhow::Result<()> {
        use super::super::{
            assets,
            controller::{Controller, Input, TickPhase},
            STEP,
        };
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut controller = Controller::new(world.take_startup_player());
        world.attach_player_body()?;
        let advance = |controller: &mut Controller, world: &mut assets::World, input| {
            controller.advance_with_tick_phases(
                world,
                |world| &world.collision,
                STEP as f64,
                true,
                input,
                |phase, player, tick, _, _, world| -> anyhow::Result<()> {
                    if phase == TickPhase::BeforeMovement {
                        player.script_input_restricted = world.cutscene_input.restricted;
                    }
                    if phase == TickPhase::SimulatePhysics {
                        world.advance_player_physics(player, STEP)?;
                    }
                    if phase == TickPhase::AfterMovement {
                        world.advance_player_models(player, tick as f32 * STEP)?;
                    }
                    Ok(())
                },
            )
        };
        for _ in 0..60 {
            advance(&mut controller, &mut world, Input::default())?;
        }
        let baseline = world.cutscene_input.count;
        let position = controller.player.position;
        let yaw = controller.camera_yaw;
        let pitch = controller.camera_pitch;
        world.with_live_player(&mut controller.player,|world| {
            world.run_script("RestrictCutSceneInput(); RestrictCutSceneInput(); wait(.1); UnrestrictCutSceneInput(); wait(.1); UnrestrictCutSceneInput(); BwxCutsceneDone=true")?;
            world.advance_models(world.animation_time)
        })?;
        assert_eq!(world.cutscene_input.count, baseline + 2);
        assert!(controller.player.script_input_restricted);
        let start_tick = controller.ticks;
        let mut nested_release_seen = false;
        let mut moved_after_release = false;
        for _ in 0..12 {
            let blocked = world.cutscene_input.restricted;
            advance(
                &mut controller,
                &mut world,
                Input {
                    movement: [0., 1.],
                    camera_stick: [1., 1.],
                    camera_delta: [0.2, 0.1],
                    jump_pressed: true,
                    jump_held: true,
                },
            )?;
            if blocked {
                assert_eq!(controller.player.position[0], position[0]);
                assert_eq!(controller.player.position[2], position[2]);
                assert!(
                    !controller.player.ground_jump_started
                        && !controller.player.second_jump_started
                );
                assert_eq!(controller.camera_yaw, yaw);
                assert_eq!(controller.camera_pitch, pitch);
            } else {
                moved_after_release |= controller.player.position != position;
            }
            if world.cutscene_input.count == baseline + 1 {
                nested_release_seen = true;
                assert!(controller.player.script_input_restricted);
            }
        }
        assert!(nested_release_seen);
        assert_eq!(world.cutscene_input.count, baseline);
        assert_eq!(controller.player.script_input_restricted, baseline != 0);
        if baseline == 0 {
            assert!(moved_after_release);
        } else {
            assert!(
                !moved_after_release,
                "Nested test callbacks released an authored restriction"
            );
            // The fixture owns this explicit release; gameplay does not clear
            // another script's restrictions when its own nesting reaches zero.
            world.with_live_player(&mut controller.player, |world| {
                world.run_script(&format!(
                    "for i=1,{baseline} do UnrestrictCutSceneInput() end"
                ))?;
                world.advance_models(world.animation_time)
            })?;
        }
        assert_eq!(world.cutscene_input.count, 0);
        assert!(!controller.player.script_input_restricted);
        assert_eq!(controller.ticks, start_tick + 12);
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
