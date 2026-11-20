//! EM1 camera blend clock REL1d9a60 and rotation interpolation DOL8014d208.
//! Controller ownership, default-camera generation and attributes are separate.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Clock {
    pub duration: f32,
    pub elapsed: f32,
    pub ease_in: f32,
    pub ease_out: f32,
    pub weight: f32,
}
impl Clock {
    pub fn new(duration: f32, ease_in: f32, ease_out: f32) -> Result<Self> {
        ensure!(
            [duration, ease_in, ease_out].iter().all(|v| v.is_finite()),
            "Invalid camera blend parameters"
        );
        Ok(Self {
            duration,
            elapsed: 0.,
            ease_in,
            ease_out,
            weight: 0.,
        })
    }
    pub fn advance(&mut self, delta: f32) -> Result<()> {
        ensure!(
            delta.is_finite() && delta >= 0.,
            "Invalid camera update interval"
        );
        let mut weight = 1.;
        if self.duration < 0. {
            self.elapsed = self.duration - 1.;
            if self.duration >= -1. {
                weight = -self.duration;
            }
        } else {
            self.elapsed += delta;
            if self.elapsed < self.duration {
                weight = self.elapsed / self.duration;
            }
            // Native's piecewise quadratic ease has a 0.01 endpoint slope.
            let half_sum = (self.ease_in + self.ease_out) * 0.5;
            let slope = (-half_sum).mul_add(0.01, 1.) / (1. - half_sum);
            if weight < self.ease_in {
                let ramp = (weight / self.ease_in).mul_add(slope - 0.01, 0.01);
                weight *= 0.5 * (0.01 + ramp);
            } else if weight > 1. - self.ease_out {
                let remaining = 1. - weight;
                let ramp = (remaining / self.ease_out).mul_add(slope - 0.01, 0.01);
                weight = (-(0.5 * (0.01 + ramp))).mul_add(remaining, 1.);
            } else {
                let base = self.ease_in * ((0.01 + slope) * 0.5);
                weight = slope.mul_add(weight - self.ease_in, base);
            }
        }
        ensure!(
            weight.is_finite(),
            "Camera ease parameters produce an invalid blend"
        );
        self.weight = weight;
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.elapsed >= self.duration
    }
}
pub fn rotation(a: [f32; 4], b: [f32; 4], weight: f32) -> Result<[f32; 4]> {
    ensure!(
        a.iter()
            .chain(b.iter())
            .chain([weight].iter())
            .all(|v| v.is_finite()),
        "Invalid camera rotation blend"
    );
    let dot = a[2].mul_add(b[2], a[0] * b[0]) + a[3].mul_add(b[3], a[1] * b[1]);
    let sign = if dot < 0. { -1. } else { 1. };
    let cosine = dot.abs();
    let (source, destination) = if cosine < 0.999 {
        let angle = super::native_angle::acos(f64::from(cosine)) as f32;
        let portion = weight * angle;
        let inverse = super::native_hk_transform::inverse_root((-cosine).mul_add(cosine, 1.))?;
        let source = super::native_angle::sin_cos_medium(f64::from(angle - portion))?.0 as f32;
        let destination = super::native_angle::sin_cos_medium(f64::from(portion))?.0 as f32;
        (inverse * source, (inverse * destination) * sign)
    } else {
        (1. - weight, weight * sign)
    };
    let q: [f32; 4] = std::array::from_fn(|i| b[i].mul_add(destination, a[i] * source));
    let squared = q[2].mul_add(q[2], q[0] * q[0]) + q[3].mul_add(q[3], q[1] * q[1]);
    let inverse = super::native_hk_transform::inverse_root(squared)?;
    Ok(q.map(|v| v * inverse))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_one_is_linear_mode_three_eases_and_negative_duration_holds() -> Result<()> {
        let mut linear = Clock::new(2., 0., 0.)?;
        linear.advance(0.5)?;
        assert_eq!(linear.weight, 0.25);
        let mut eased = Clock::new(2., 0.2, 0.2)?;
        eased.advance(0.2)?;
        assert!((eased.weight - 0.0319375).abs() < 1e-7);
        assert!(!eased.complete());
        eased.advance(1.8)?;
        assert_eq!(eased.weight, 1.);
        assert!(eased.complete());
        let mut held = Clock::new(-0.25, 0.2, 0.2)?;
        held.advance(1.)?;
        assert_eq!((held.elapsed, held.weight), (-1.25, 0.25));
        assert!(!held.complete());
        held.advance(100.)?;
        assert_eq!(held.weight, 0.25);
        Ok(())
    }
    #[test]
    fn quaternion_hemisphere_and_half_rotation_follow_native_interpolation() -> Result<()> {
        let a = [0., 0., 0., 1.];
        let b = [0., 1., 0., 0.];
        let q = rotation(a, b, 0.5)?;
        assert!((q[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((q[3] - q[1]).abs() < 1e-6);
        let same = rotation(a, [0., 0., 0., -1.], 0.5)?;
        assert_eq!(&same[..3], &[0.; 3]);
        // One native reciprocal-root refinement preserves its rounding;
        // replacing it with host normalization would force exactly 1 here.
        assert_eq!(same[3].to_bits(), 0x3f7fffff);
        Ok(())
    }
}
