//! Default EM1 Projection texture generator, REL 0x177708..0x177bdc.
//! Object-oriented/projector overrides and live GPU binding remain pending.
use anyhow::{ensure, Result};
/// Row-major 3x4 homogeneous texture matrix, applied to camera-space vertices.
/// Normal branch only: no object projection flag, skinning bypass, or animated
/// rotation/post transform. CameraIndex is not consulted in this native branch.
pub fn camera_projection(
    right: f32,
    top: f32,
    near: f32,
    scale: [f32; 2],
    translation: [f32; 2],
) -> Result<[[f32; 4]; 3]> {
    ensure!(
        [
            right,
            top,
            near,
            scale[0],
            scale[1],
            translation[0],
            translation[1]
        ]
        .into_iter()
        .all(f32::is_finite),
        "Nonfinite projection input"
    );
    let near = if near == 0. { 0.0001 } else { near };
    let right = near * (2. * right);
    let top = near * (2. * top);
    ensure!(right != 0. && top != 0., "Degenerate projection frustum");
    // DOL 0x805ae680: reciprocal extents then separately rounded multiplies.
    let inv_x = 1. / (right - (-right));
    let inv_y = 1. / (top - (-top));
    let twice_near = 2. * near;
    let s = scale[0] * (twice_near * inv_x);
    let t = (-scale[1]) * (twice_near * inv_y);
    // Native offsets are computed in double precision then narrowed (REL 177a44).
    let offset_s = (0.5f64 + translation[0] as f64) as f32;
    let offset_t = (0.5f64 - translation[1] as f64) as f32;
    let z_s = scale[0] * (inv_x * (right + (-right))) - offset_s;
    let z_t = (-scale[1]) * (inv_y * (top + (-top))) - offset_t;
    Ok([[s, 0., z_s, 0.], [0., t, z_t, 0.], [0., 0., -1., 0.]])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn uv(m: [[f32; 4]; 3], p: [f32; 3]) -> [f32; 2] {
        let v = m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]);
        [v[0] / v[2], v[1] / v[2]]
    }
    #[test]
    fn native_projection_doubles_frustum_extents_and_flips_t() -> Result<()> {
        let m = camera_projection(1., 1., 0.1, [1., 1.], [0., 0.])?;
        assert_eq!(uv(m, [0., 0., -1.]), [0.5, 0.5]);
        assert_eq!(uv(m, [1., 1., -1.]), [1., 0.]);
        assert_eq!(uv(m, [-1., -1., -1.]), [0., 1.]);
        let moved = camera_projection(1., 1., 0.1, [2., 2.], [0.25, 0.125])?;
        assert_eq!(uv(moved, [0., 0., -1.]), [0.75, 0.375]);
        assert!(camera_projection(1., 1., 0., [1.; 2], [0.; 2])?
            .iter()
            .flatten()
            .all(|v| v.is_finite()));
        assert!(camera_projection(0., 1., 0.1, [1.; 2], [0.; 2]).is_err());
        Ok(())
    }
}
