//! Counter color workspace64..98, native320ff0. Tick rates are fixed EM1
//! constants; RGB smoothing does not use delta, while the hold timer does.
use anyhow::{ensure, Result};
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Effect {
    pub remaining: f32,
    pub current: [f32; 4],
    pub target: [f32; 4],
    pub baseline: [f32; 4],
    pub accumulator: f32,
}
impl Effect {
    /// REL3215d0: equal-strength impacts refresh the hold; weaker ones leave
    /// the entire workspace untouched. Publication occurs in the update helper.
    pub fn activate(&mut self, strength: f32, color: [f32; 4]) -> Result<bool> {
        ensure!(
            strength.is_finite()
                && self.accumulator.is_finite()
                && color.iter().all(|value| value.is_finite()),
            "Invalid transient color activation"
        );
        if strength < self.accumulator {
            return Ok(false);
        }
        self.accumulator = strength;
        self.remaining = f32::from_bits(0x3dcccccd);
        self.target = color;
        Ok(true)
    }
    pub fn initialize(&mut self, baseline: Option<[u8; 4]>) {
        // Native3211e8 captures an existing per-object regular1 override.
        // Missing userdata leaves the zero-initialized baseline intact.
        if let Some(color) = baseline {
            self.baseline = color.map(|v| f32::from(v) / 255.);
        }
        self.current = self.baseline;
    }
    pub fn advance(&mut self, delta: f32) -> Result<Option<[u8; 4]>> {
        ensure!(
            delta.is_finite()
                && delta >= 0.
                && self.remaining.is_finite()
                && self
                    .current
                    .iter()
                    .chain(&self.target)
                    .chain(&self.baseline)
                    .all(|v| v.is_finite()),
            "Invalid transient color workspace"
        );
        self.remaining = (self.remaining - delta).max(0.);
        let before = self.current;
        if self.remaining <= 0. {
            self.target = self.baseline;
            self.accumulator = 0.;
        }
        for channel in 0..3 {
            let factor = if self.current[channel] < self.target[channel] {
                f32::from_bits(0x3ed70a3d)
            } else {
                f32::from_bits(0x3f666666)
            };
            self.current[channel] =
                self.current[channel].mul_add(factor, self.target[channel] * (1. - factor));
            if self.current[channel] < f32::from_bits(0x3c23d70a) {
                self.current[channel] = 0.;
            }
        }
        // Alpha is retained, not interpolated by this native helper.
        if self.current == before {
            return Ok(None);
        }
        super::paint_counter::pack_color(self.current).map(Some)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_priority_refresh_and_expiry_follow_native_workspace() -> Result<()> {
        let mut state = Effect::default();
        state.initialize(Some([0, 0, 0, 128]));
        assert!(state.activate(1., [1., 0., 0., 1.])?);
        assert_eq!(state.remaining.to_bits(), 0x3dcccccd);
        let current = state.current;
        state.advance(0.025)?;
        let before = serde_json::to_value(&state)?;
        assert!(!state.activate(0.5, [0., 1., 0., 1.])?);
        assert_eq!(serde_json::to_value(&state)?, before);
        assert!(state.activate(1., [0., 1., 0., 1.])?);
        assert_eq!(state.remaining.to_bits(), 0x3dcccccd);
        assert_ne!(
            state.current, current,
            "Activation must not replace the smoothed current color"
        );
        state.advance(f32::from_bits(0x3dcccccd))?;
        assert_eq!(state.accumulator, 0.);
        assert!(state.activate(0.5, [0., 0., 1., 1.])?);
        assert_eq!(state.current[3], 128. / 255.);
        let before = serde_json::to_value(&state)?;
        assert!(state.activate(f32::NAN, [1.; 4]).is_err());
        assert_eq!(serde_json::to_value(&state)?, before);
        Ok(())
    }
    #[test]
    fn hold_expiry_restores_baseline_before_smoothing_and_keeps_alpha() -> Result<()> {
        let mut state = Effect::default();
        state.initialize(Some([0, 0, 0, 128]));
        state.remaining = 0.25;
        state.target = [1., 0.5, 0., 1.];
        state.accumulator = 9.;
        let color = state.advance(0.125)?.unwrap();
        assert_eq!(state.current[0], f32::from_bits(0x3f147ae2));
        assert_eq!(color[3], 128);
        state.advance(0.125)?;
        assert_eq!(state.remaining, 0.);
        assert_eq!(state.target, state.baseline);
        assert_eq!(state.accumulator, 0.);
        for _ in 0..50 {
            state.advance(1. / 30.)?;
        }
        assert_eq!(state.current, state.baseline);
        assert!(state.advance(0.)?.is_none());
        Ok(())
    }
    #[test]
    fn smoothing_is_per_tick_but_timer_uses_delta() -> Result<()> {
        let state = Effect {
            remaining: 1.,
            target: [1.; 4],
            ..Default::default()
        };
        let mut a = state.clone();
        let mut b = state;
        a.advance(0.01)?;
        b.advance(0.2)?;
        assert_eq!(a.current, b.current);
        assert_ne!(a.remaining, b.remaining);
        Ok(())
    }
}
