//! Retail PC f41a20 null-target branch and f37500 speed setter.
//! Target resolution and yaw release remain mandatory world/pass services.
use anyhow::{ensure, Result};

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Focus {
    pub target: u32,
    pub state: [u32; 2],
    pub flags: [bool; 2],
    pub speed: f32,
    pub active: bool,
    pub maximum: f32,
    pub values: [f32; 4],
}
impl Default for Focus {
    fn default() -> Self {
        // f3d88b..f3d96f initializes these same semantic fields. Padding
        // bytes are deliberately not represented as editable state.
        Self { target: 0, state: [0; 2], flags: [false; 2], speed: f32::MAX,
            active: false, maximum: f32::MAX, values: [0.; 4] }
    }
}
pub(super) trait Receiver {
    fn target_live(&mut self, target: u32) -> Result<bool>;
    fn yaw_constrained(&mut self) -> Result<bool>;
    fn release_yaw(&mut self, duration: f32) -> Result<()>;
}
impl Focus {
    pub fn clear(&mut self, receiver: &mut impl Receiver) -> Result<()> {
        if self.target != 0 && receiver.target_live(self.target)? && !receiver.yaw_constrained()? {
            // The native callback precedes every field write. A failed
            // receiver must therefore retain the complete prior focus.
            receiver.release_yaw(f32::MAX)?;
        }
        *self = Self::default();
        Ok(())
    }
    pub fn set_speed(&mut self, value: f32) -> Result<()> {
        ensure!(value.is_finite(), "Nonfinite PC focus speed");
        if value >= f32::from_bits(0xfc1071db) { self.speed = value; }
        Ok(())
    }
}

#[cfg(test)]
#[path="../../tools/blotworx/tests/em2_camera_focus_cases.rs"]
mod tests;
