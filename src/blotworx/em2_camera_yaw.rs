//! Retail PC f376f0 yaw release, shared by hint and focus reset receivers.
use super::{pc_float::Precision, pc_spring};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Debug, serde::Serialize)]
pub(super) struct State {
    pub spring: pc_spring::State,
    pub field_4c4: f32,
    pub field_5ac: f32,
    pub return_force: f32,
}
impl State {
    pub fn new(settings: &BTreeMap<String, String>) -> Result<Self> {
        let value = settings.get("FreeCamera_Spring_FreeYaw_ReturnForce").map(String::as_str).unwrap_or("0.005");
        let return_force = super::config_value::decimal_float(value).map_err(anyhow::Error::msg)?;
        ensure!(return_force.is_finite(), "Nonfinite PC yaw return force");
        Ok(Self {
            // d82120(MAX): identical stored bits at all three x87 precisions.
            spring: pc_spring::State { stiffness: f32::MAX, damping: f32::from_bits(0x5fffffff), velocity: 0., accumulated: 0. },
            field_4c4: 0., field_5ac: 0., return_force,
        })
    }
    fn stiffness(&self, value: f32) -> Option<f32> {
        if self.return_force <= 0. || value == 0. { return None; }
        Some(if value > 0. && value < f32::from_bits(0x7c1071db) { value } else { self.return_force })
    }
    pub fn check_release(&self, value: f32, precision: Option<Precision>) -> Result<()> {
        ensure!(value.is_finite() && self.return_force.is_finite(), "Nonfinite PC yaw release");
        if self.stiffness(value).is_some_and(|k| k != self.spring.stiffness) {
            precision.context("PC camera thread floating-point precision is not verified")?;
        }
        Ok(())
    }
    pub fn release(&mut self, value: f32, precision: Option<Precision>) -> Result<()> {
        self.check_release(value, precision)?;
        if let Some(stiffness) = self.stiffness(value) {
            if stiffness != self.spring.stiffness {
                self.spring.tune(stiffness, precision.context("Missing PC yaw precision")?)?;
            }
            // d820c0 clears velocity only; accumulated time survives release.
            self.spring.velocity = 0.;
            self.field_5ac = 0.;
        }
        self.field_4c4 = 0.;
        Ok(())
    }
}

/// Actual target resolution, constraint query and interpolator snaps still
/// belong to the world/pass. Yaw release now has one concrete retained owner.
pub(super) trait World {
    fn check_snap(&mut self, axis: usize, value: f32) -> Result<()>;
    fn snap(&mut self, axis: usize, value: f32) -> Result<()>;
    fn target_live(&mut self, target: u32) -> Result<bool>;
    fn yaw_constrained(&mut self) -> Result<bool>;
}
pub(super) struct Receivers<'a, W> {
    pub yaw: &'a mut State,
    pub world: &'a mut W,
    pub precision: Option<Precision>,
}
impl<W: World> super::em2_camera_hints::Receiver for Receivers<'_, W> {
    fn check(&mut self, call: super::em2_camera_hints::Call) -> Result<()> {
        match call {
            super::em2_camera_hints::Call::ReleaseYaw(value) => self.yaw.check_release(value, self.precision),
            super::em2_camera_hints::Call::Snap { axis, value } => self.world.check_snap(axis, value),
        }
    }
    fn release_yaw(&mut self, value: f32) -> Result<()> { self.yaw.release(value, self.precision) }
    fn snap(&mut self, axis: usize, value: f32) -> Result<()> { self.world.snap(axis, value) }
}
impl<W: World> super::em2_camera_focus::Receiver for Receivers<'_, W> {
    fn target_live(&mut self, target: u32) -> Result<bool> { self.world.target_live(target) }
    fn yaw_constrained(&mut self) -> Result<bool> { self.world.yaw_constrained() }
    fn release_yaw(&mut self, value: f32) -> Result<()> { self.yaw.release(value, self.precision) }
}

#[cfg(test)]
#[path="../../tools/blotworx/tests/em2_camera_yaw_cases.rs"]
mod tests;
