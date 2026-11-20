//! Spline pass suppression, REL3bb734..3bb7fc. Hold threshold3bbc7c.
#[derive(Default)]
pub struct PassState { last: Option<usize> }
impl PassState {
    pub fn pass(&mut self,knot:Option<usize>)->Option<usize> {
        let knot=knot?;
        if self.last==Some(knot) {return None;}
        self.last=Some(knot);Some(knot)
    }
}
pub const HOLD_EPSILON:f32=f32::from_bits(0x34000000);

/// REL3bbc38..3bbce4: terminal knots bypass the hold property entirely.
/// An existing hold at or above epsilon also bypasses the authored read.
pub fn enter_hold(current: &mut f32, onward: bool, authored: impl FnOnce() -> f32) -> bool {
    if !onward || !(*current < HOLD_EPSILON) { return false; }
    *current = authored();
    *current > HOLD_EPSILON
}
