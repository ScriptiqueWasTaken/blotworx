//! PC a3b6d0/a3ba50: packed disable count, distinct from permanent locks.
//! Cancellation and status-event delivery remain mandatory external owners.
use anyhow::Result;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct Change {
    pub before: u32,
    pub after: u32,
    pub cancel_active: bool,
}
impl Change {
    pub fn new(before: u32, increment: bool) -> Self {
        let count = (before >> 1) & 7;
        let count = if increment { (count + 1).min(7) } else { count.saturating_sub(1) };
        Self { before, after: (before & !14) | (count << 1),
            cancel_active: increment && before & 15 == 0 && before & 16 != 0 }
    }
    pub fn changed(self) -> bool { self.before != self.after }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) enum Group { Sketch, Paint }

/// PC f185c0..f1896a initializes this mask to zero. f169d0 clears each bit
/// only after its corresponding component decrement method has returned.
#[derive(Default, Debug, serde::Serialize)]
pub(super) struct CameraRestore {
    pub mask: u32,
}
impl CameraRestore {
    pub fn restore(&mut self, component_present: bool, mut decrement: impl FnMut(Group, &'static str) -> Result<()>) -> Result<()> {
        if !component_present { return Ok(()); }
        for (bit, group, reason) in [(1, Group::Sketch, "Camera_AllSketch"), (2, Group::Paint, "Camera_AllPaint")] {
            if self.mask & bit != 0 {
                decrement(group, reason)?;
                self.mask &= !bit;
            }
        }
        Ok(())
    }
}
