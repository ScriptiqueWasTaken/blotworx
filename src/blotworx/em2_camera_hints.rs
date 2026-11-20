//! PC fec3b0/f387f0/f36db0/f36fc0/f37080 hint ownership.
//! Yaw release and interpolator snaps have required receivers; this does not
//! stand in for native camera pose generation or interpolator updates.
use anyhow::{ensure, Result};
const LOW: f32 = f32::from_bits(0xfc1071db);
const HIGH: f32 = f32::from_bits(0x7c1071db);

#[derive(Clone, Copy, Debug)]
pub(super) enum Call { ReleaseYaw(f32), Snap { axis: usize, value: f32 } }
pub(super) trait Receiver {
    /// Validate all required pure-data receivers before mutating a leaf.
    /// The receiver must keep those bindings valid for the synchronous call.
    fn check(&mut self, call: Call) -> Result<()>;
    fn release_yaw(&mut self, duration: f32) -> Result<()>;
    fn snap(&mut self, axis: usize, value: f32) -> Result<()>;
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Hint {
    pub value: f32,
    pub forced: f32,
    pub flags: [bool; 2],
    pub range: f32,
    pub approach: f32,
    pub release: f32,
    pub approach_cached: f32,
    pub release_cached: f32,
    pub bound: bool,
    pub timer: f32,
}
impl Default for Hint {
    fn default() -> Self {
        Self { value: f32::MAX, forced: f32::MAX, flags: [false; 2], range: 0., approach: 0., release: 0.,
            approach_cached: 0., release_cached: 0., bound: false, timer: 0. }
    }
}
#[derive(Default, Debug, serde::Serialize)]
pub(super) struct Hints {
    pub hints: [Hint; 5],
    pub yaw_dirty: bool,
}
impl Hints {
    pub fn set(&mut self, args: [f32; 5], receiver: &mut impl Receiver) -> Result<()> {
        ensure!(args.into_iter().all(f32::is_finite), "Nonfinite PC camera hint");
        if args[1] >= LOW && args[1] != self.hints[1].value { receiver.check(Call::ReleaseYaw(f32::MAX))?; }
        for (axis, value) in args.into_iter().enumerate() {
            if value < LOW { continue; }
            if axis == 1 && value != self.hints[axis].value { receiver.release_yaw(f32::MAX)?; }
            self.hints[axis].value = if value >= HIGH { f32::MAX } else { value };
            if axis == 1 && value < HIGH { self.yaw_dirty = true; }
        }
        Ok(())
    }
    pub fn force(&mut self, args: [f32; 5], defaults: [f32; 5], receiver: &mut impl Receiver) -> Result<()> {
        ensure!(args.into_iter().chain(defaults).all(f32::is_finite), "Nonfinite PC forced camera hint");
        for (axis, value) in args.into_iter().enumerate() {
            if value >= LOW { receiver.check(Call::Snap { axis, value: if value >= HIGH { defaults[axis] } else { value } })?; }
        }
        for (axis, value) in args.into_iter().enumerate() {
            if value < LOW { continue; }
            let inherit = value >= HIGH;
            self.hints[axis].forced = if inherit { f32::MAX } else { value };
            receiver.snap(axis, if inherit { defaults[axis] } else { value })?;
        }
        Ok(())
    }
    pub fn ranges(&mut self, args: [f32; 3]) -> Result<()> {
        ensure!(args.into_iter().all(f32::is_finite), "Nonfinite PC camera hint range");
        for (axis, value) in args.into_iter().enumerate() {
            if value < LOW { continue; }
            self.hints[axis].range = value;
            // Native setters latch these flags; a zero range does not clear them.
            if axis != 0 && value > 0. { self.hints[axis].flags[1] = true; }
        }
        Ok(())
    }
    pub fn soft(&mut self, args: [f32; 10], receiver: &mut impl Receiver) -> Result<()> {
        ensure!(args.into_iter().all(f32::is_finite), "Nonfinite PC camera hint soft time");
        // The PC setter's last four guards and copied values use different
        // argument slots. Preserve that observed behavior, including finite
        // negative values copied from an earlier independently guarded slot.
        let copied = [[0, 1], [2, 3], [4, 5], [4, 3], [4, 5]];
        for axis in 0..5 {
            for side in 0..2 {
                if args[axis * 2 + side] < LOW { continue; }
                let hint = &self.hints[axis];
                let cached = if side == 0 { hint.approach_cached } else { hint.release_cached };
                if args[copied[axis][side]] > cached && hint.bound && hint.forced < HIGH {
                    receiver.check(Call::Snap { axis, value: hint.forced })?;
                }
            }
        }
        for axis in 0..5 {
            for side in 0..2 {
                if args[axis * 2 + side] < LOW { continue; }
                let value = args[copied[axis][side]];
                let hint = &mut self.hints[axis];
                let cached = if side == 0 { hint.approach = value; hint.approach_cached } else { hint.release = value; hint.release_cached };
                if value > cached && hint.bound && hint.forced < HIGH { receiver.snap(axis, hint.forced)?; }
                if side == 0 { hint.approach_cached = value; } else { hint.release_cached = value; }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path="../../tools/blotworx/tests/em2_camera_hint_cases.rs"]
mod tests;
