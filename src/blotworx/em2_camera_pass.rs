//! Retail PC reset leaves f372d0/f37360/f373b0/f377d0/f39d10.
//! These retain pass data; camera generation remains a separate consumer.
use anyhow::{ensure, Result};
#[derive(Debug, serde::Serialize)]
pub(super) struct State {
    pub soft_painting_ignore: i32,
    pub soft_ignores_wrapping: bool,
    pub look_ahead_active: bool,
    pub look_ahead_soft: [f32; 2],
    pub ledge_values: [f32; 2],
    pub modes: u32,
}
impl Default for State {
    fn default() -> Self {
        // f3d7d2, f3ddbd..f3de41 and f3dff8 constructor stores.
        Self {soft_painting_ignore:0,soft_ignores_wrapping:false,look_ahead_active:false,
            look_ahead_soft:[f32::MAX;2],ledge_values:[f32::MAX;2],modes:0}
    }
}
impl State {
    fn accepted_pair(destination: &mut [f32;2], values:[f32;2]) -> Result<()> {
        ensure!(values.into_iter().all(f32::is_finite),"Nonfinite PC camera pass parameters");
        for (destination,value) in destination.iter_mut().zip(values) {
            if value >= f32::from_bits(0xfc1071db) { *destination = value; }
        }
        Ok(())
    }
    pub fn look_ahead(&mut self, active:bool, soft:[f32;2]) -> Result<()> {
        Self::accepted_pair(&mut self.look_ahead_soft,soft)?;
        self.look_ahead_active=active;Ok(())
    }
    pub fn ledge(&mut self, values:[f32;2]) -> Result<()> { Self::accepted_pair(&mut self.ledge_values,values) }
    pub fn deactivate_reset_mode(&mut self, mask:u32) -> Result<()> {
        // ClearAll invokes precisely these masks. Mode8 has an additional
        // yaw-release side effect and must not go through this reset-only API.
        ensure!(matches!(mask,1|32),"Camera mode removal requires its native side effects");
        self.modes &= !mask;Ok(())
    }
}

#[cfg(test)]
#[path="../../tools/blotworx/tests/em2_camera_pass_cases.rs"]
mod tests;
