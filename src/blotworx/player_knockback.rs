//! Live player admission and motion/animation wiring for original state10.
//! Full action arbitration, facing controller and landing/game-event effects
//! remain explicit compatibility work rather than an impulse/mass fallback.
use super::{
    assets::World,
    knockback::{self, Effect, Settings, State},
    physics::Player,
    support::State as Support,
};
use anyhow::{ensure, Context, Result};
#[derive(Clone, Debug, serde::Serialize)]
pub struct Runtime {
    pub settings: Settings,
    pub state: State,
    pub landing_speed: Option<f32>,
    pub declined: u64,
}
impl World {
    fn player_motion_disabled(&self) -> bool {
        self.entities[self.player_entity]
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                matches!(
                    c.class_name.as_str(),
                    "MickeyPlayerMotionControllerComponent" | "BasicMotionControllerComponent"
                )
            })
            .any(|(index, _)| {
                self.component_control
                    .disabled
                    .get(&self.player_entity)
                    .and_then(|values| values.get(&index))
                    .copied()
                    .unwrap_or(false)
            })
    }
    pub(super) fn begin_player_knockback(
        &mut self,
        direction: [f32; 3],
        amount: f32,
    ) -> Result<()> {
        ensure!(
            self.script_player_available,
            "Knockback admission requires the live player scope"
        );
        ensure!(
            amount.is_finite() && direction.iter().all(|v| v.is_finite()),
            "Nonfinite player knockback"
        );
        let disabled = self.player_motion_disabled();
        let eligible = self
            .runtime_health
            .get(&self.player_entity)
            .context("Player lacks native health eligibility latch")?
            .knockback_eligible();
        if self.player_knockback.is_none() {
            let component = self.entities[self.player_entity]
                .components
                .iter()
                .find(|c| c.class_name == "MickeyPlayerMotionControllerComponent")
                .context("Player lacks resolved motion controller")?;
            self.player_knockback = Some(Runtime {
                settings: Settings::load(&self.runtime_settings, component)?,
                state: Default::default(),
                landing_speed: None,
                declined: 0,
            });
        }
        let runtime = self.player_knockback.as_mut().unwrap();
        if runtime.state.respawns != self.script_player.respawns
            || !self.script_player.native_knockback
        {
            runtime.state.active = false;
        }
        if !knockback::admits(
            amount,
            runtime.settings.maximum_stimulus,
            disabled,
            runtime.state.active,
            eligible,
        ) {
            runtime.declined += 1;
            return Ok(());
        }
        let velocity = knockback::launch(direction, amount, &runtime.settings)?;
        runtime.state.begin(velocity, self.script_player.respawns);
        self.script_player.native_knockback = true;
        //309a90 retains a facing target, rather than teleporting orientation.
        // The native turn-controller update is still pending; do not fabricate
        // a rotation or modify the original asset to hide that discrepancy.
        let message="Player knockback uses native stimulus cap, motion state10 and animation feedback. Native facing, complete ability arbitration, landing classification/effects and game events0xb1/0x72 remain incomplete.";
        if !self.report.unsupported.iter().any(|value| value == message) {
            self.report.unsupported.push(message.into());
        }
        Ok(())
    }
    pub(super) fn synchronize_player_knockback(&mut self, player: &mut Player) {
        if let Some(runtime) = &mut self.player_knockback {
            if runtime.state.respawns != player.respawns || !player.native_knockback {
                runtime.state.active = false;
            }
            player.native_knockback = runtime.state.active;
        }
    }
    pub(super) fn advance_player_knockback(&mut self, player: &mut Player, dt: f32) -> Result<()> {
        let disabled = self.player_motion_disabled();
        self.synchronize_player_knockback(player);
        let Some(runtime) = &mut self.player_knockback else {
            return Ok(());
        };
        if !runtime.state.active {
            return Ok(());
        }
        //3065f0 exits knockback on disabled motion before normal action work.
        if disabled {
            runtime.state.animation(35);
            player.native_knockback = false;
            return Ok(());
        }
        let support = player
            .published_support()
            .context("Knockback requires freshly published character support")?;
        let mut velocity = [
            player.velocity[0],
            player.velocity[1],
            player.velocity[2],
            0.,
        ];
        let mut effects = Vec::new();
        runtime.state.advance(
            &mut velocity,
            support,
            &self.frame_clock.clock,
            dt,
            [-player.motion.gravity_up, -player.motion.gravity_down],
            &runtime.settings,
            |effect| effects.push(effect),
        )?;
        player.velocity.copy_from_slice(&velocity[..3]);
        player.knockback_velocity_override = true;
        if let Some(distance) = player
            .motion_ground
            .as_ref()
            .and_then(|state| state.distance)
        {
            //3053d8 stores motion+290 at input+c0; +51 is motion+2e1.
            //308d24 clears it every update.308df8..308e14 only admits
            //motion0/3 to its sole set-to-one branch308e6c. During motion10
            //it is therefore false, independently of geometric proximity.
            if runtime
                .state
                .next_motion(support, false, distance, &runtime.settings)
                .is_some()
            {
                runtime.state.active = false;
            }
        }
        player.native_knockback = runtime.state.active;
        for effect in effects {
            match effect {
                Effect::Animation(id) => {
                    self.send_native_actor_animation_event(self.player_entity, id)?;
                }
                Effect::LandingSpeed(speed) => {
                    self.player_knockback.as_mut().unwrap().landing_speed = Some(speed);
                }
            }
        }
        Ok(())
    }
}
