//! Directional Lua stimulus geometry, REL289fd0..28a100.
use anyhow::{ensure, Result};
pub fn direction(source: [f32; 3], target: [f32; 3], radius: f32) -> Result<Option<[f32; 3]>> {
    ensure!(
        source
            .iter()
            .chain(target.iter())
            .all(|value| value.is_finite())
            && radius.is_finite(),
        "Non-finite directional stimulus geometry"
    );
    let mut delta = std::array::from_fn(|i| target[i] - source[i]);
    let squared = delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1];
    let distance = if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    };
    // Radius zero bypasses the range comparison; equality at any nonzero
    // radius does not deliver. Negative finite radii naturally admit nothing.
    if !(distance < radius || radius == 0.) {
        return Ok(None);
    }
    // REL1fad0 returns length without modifying the vector.28a0c4 divides
    // by that returned length only above4bd034 (the native0.0001 threshold).
    if distance > 0.0001 {
        let reciprocal = 1. / distance;
        for lane in &mut delta {
            *lane *= reciprocal;
        }
    }
    Ok(Some(delta))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_strict_radius_zero_bypass_and_small_direction() -> Result<()> {
        assert_eq!(direction([0.; 3], [3., 0., 4.], 5.)?, None);
        assert_eq!(direction([0.; 3], [3., 0., 4.], 0.)?, Some([0.6, 0., 0.8]));
        assert_eq!(direction([0.; 3], [0.; 3], 0.)?, Some([0.; 3]));
        assert_eq!(
            direction([0.; 3], [0.00001, 0., 0.], 1.)?,
            Some([0.00001, 0., 0.])
        );
        assert_eq!(direction([0.; 3], [0.; 3], -1.)?, None);
        Ok(())
    }
}
