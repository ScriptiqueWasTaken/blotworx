//! Retail LOD state projection. Camera-distance initialization, subscriptions
//! and scheduled updates remain separate required work.
pub struct State {
    pub remaining: f32,
    pub flags: u8,
    pub enabled: u8,
    pub visible: u8,
}
impl State {
    /// REL357ca0..357ccc; REL4cd754 is exactly -1.
    pub fn constructed() -> Self {
        Self {
            remaining: -1.,
            flags: 0,
            enabled: 1,
            visible: 1,
        }
    }
    /// REL358414: only +44 changes; don't reset visibility or flags.
    pub fn reset(&mut self) {
        self.remaining = 0.;
    }
}
