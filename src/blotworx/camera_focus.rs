//! Zero-steering focus orientation, REL1e38e8..1e3c30.
//! Target resolution, world-bound publication and steering state are separate.
use anyhow::{ensure, Result};
type V = [f32; 3];
#[derive(Clone, Copy, Debug)]
pub struct Basis {
    pub left: V,
    pub up: V,
    pub direction: V,
}
fn length(value: V) -> Result<f32> {
    // REL1fad0: paired square/add order, estimate rounded to single, one
    // refinement. Do not replace it with host sqrt or reorder the sum.
    let squared = value[2].mul_add(value[2], value[0] * value[0]) + value[1] * value[1];
    ensure!(squared.is_finite(), "Camera focus length overflow");
    if squared == 0. {
        return Ok(0.);
    }
    Ok(squared * super::native_hk_transform::inverse_root(squared)?)
}
fn normalize(mut value: V) -> Result<(V, f32)> {
    let magnitude = length(value)?;
    if magnitude > f32::from_bits(0x38d1b717) {
        let scale = 1. / magnitude;
        value = value.map(|v| v * scale);
    }
    Ok((value, magnitude))
}
fn cross(a: V, b: V) -> V {
    [
        a[1].mul_add(b[2], -(b[1] * a[2])),
        a[2].mul_add(b[0], -(b[2] * a[0])),
        a[0].mul_add(b[1], -(b[0] * a[1])),
    ]
}
/// None leaves the authored basis intact, matching the native coincident and
/// positive-up-pole branches. No guessed alternate up vector is introduced.
pub fn orientation(eye: V, target: V) -> Result<Option<Basis>> {
    ensure!(
        eye.iter().chain(&target).all(|v| v.is_finite()),
        "Nonfinite camera focus position"
    );
    let (direction, magnitude) = normalize(std::array::from_fn(|i| target[i] - eye[i]))?;
    let epsilon = f32::from_bits(0x38d1b717);
    if magnitude <= epsilon {
        return Ok(None);
    }
    let up = [0., 1., 0.];
    if (0..3).all(|i| (direction[i] - up[i]).abs() <= epsilon) {
        return Ok(None);
    }
    let (right, _) = normalize(cross(direction, up))?;
    let (up, _) = normalize(cross(right, direction))?;
    Ok(Some(Basis {
        left: right.map(|v| -v),
        up,
        direction,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_tracks_target_without_changing_eye_and_preserves_native_pole_rules() -> Result<()> {
        let eye = [3., 2., 1.];
        let target = [7., 2., 1.];
        let Basis { direction, up, .. } = orientation(eye, target)?.unwrap();
        assert!((direction[0] - 1.).abs() < 1e-6);
        assert_eq!(&direction[1..], &[0., 0.]);
        assert!((up[1] - 1.).abs() < 1e-6);
        assert!(orientation(eye, eye)?.is_none());
        assert!(orientation(eye, [3., 9., 1.])?.is_none());
        let down = orientation(eye, [3., -9., 1.])?.unwrap();
        assert!(down.direction[1] < -0.99999);
        assert_eq!(down.up, [0.; 3]);
        assert!(down.left.iter().all(|v| *v == 0.));
        Ok(())
    }
    #[test]
    fn general_focus_basis_aims_at_target_and_has_no_authored_roll() -> Result<()> {
        let eye = [2., 3., 4.];
        let target = [7., 5., -1.];
        let Basis { direction, up, .. } = orientation(eye, target)?.unwrap();
        let delta: V = std::array::from_fn(|i| target[i] - eye[i]);
        let off_axis = cross(direction, delta);
        assert!(off_axis.iter().all(|v| v.abs() < 1e-5));
        let dot = direction.iter().zip(up).map(|(a, b)| a * b).sum::<f32>();
        assert!(dot.abs() < 1e-6);
        assert!(up[1] > 0.);
        assert!(orientation(eye, [f32::NAN, 0., 0.]).is_err());
        Ok(())
    }
}
