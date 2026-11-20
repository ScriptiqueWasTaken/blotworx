//! EM1 NiCamera projection, DOL8044d254..8044d454, adapted to OpenGL depth.
//! Camera basis is handled by the view transform. Application FOV/viewport
//! adjustments and original combined-matrix rounding remain separate work.
use anyhow::{ensure, Result};

pub fn matrix(frustum: [f32; 6], orthographic: bool) -> Result<[f32; 16]> {
    let [left, right, top, bottom, near, far] = frustum;
    // Native8044d264..d294 divides signed side intervals without sorting
    // them or testing their sign. A negative scripted FOV mirrors both axes.
    ensure!(
        frustum.iter().all(|value| value.is_finite())
            && right != left
            && top != bottom
            && far > near
            && (orthographic || near > 0.),
        "Invalid authored camera frustum"
    );
    if orthographic {
        let width = 1. / (right - left);
        let height = 1. / (top - bottom);
        let depth = 1. / (far - near);
        // Native8044d2a8/d2cc keeps these sums positive. The editor's
        // conventional orthographic projection negates them, which shifts
        // asymmetric authored cameras. Preserve EM1's observable convention.
        Ok([
            2. * width,
            0.,
            0.,
            0.,
            0.,
            2. * height,
            0.,
            0.,
            0.,
            0.,
            -2. * depth,
            0.,
            (right + left) * width,
            (top + bottom) * height,
            -(far + near) * depth,
            1.,
        ])
    } else {
        Ok([
            2. / (right - left),
            0.,
            0.,
            0.,
            0.,
            2. / (top - bottom),
            0.,
            0.,
            (right + left) / (right - left),
            (top + bottom) / (top - bottom),
            (far + near) / (near - far),
            -1.,
            0.,
            0.,
            2. * far * near / (near - far),
            0.,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(matrix: [f32; 16], point: [f32; 3]) -> [f32; 3] {
        let clip: [f32; 4] = std::array::from_fn(|row| {
            matrix[row] * point[0]
                + matrix[4 + row] * point[1]
                + matrix[8 + row] * point[2]
                + matrix[12 + row]
        });
        [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
    }
    #[test]
    fn native_orthographic_offsets_are_positive_and_accept_signed_near_planes() -> Result<()> {
        let projection = matrix([-0.5, 1.5, 0.75, -0.25, -1., 3.], true)?;
        // Coefficients follow native8044d2a8..d2d8, not a conventional
        // GL orthographic helper. Native [0,1] Z is adapted to [-1,1].
        assert_eq!(project(projection, [0., 0., 0.]), [0.5, 0.5, -0.5]);
        assert_eq!(project(projection, [-1.5, -0.75, 1.]), [-1., -1., -1.]);
        assert_eq!(project(projection, [0.5, 0.25, -3.]), [1., 1., 1.]);
        assert!(matrix([-1., 1., 1., -1., 0., 3.], true).is_ok());
        assert!(matrix([-1., 1., 1., -1., 0., 3.], false).is_err());
        Ok(())
    }
    #[test]
    fn perspective_keeps_authored_offsets_and_opengl_near_clipping() -> Result<()> {
        let projection = matrix([-0.5, 1.5, 0.75, -0.25, 1., 2.], false)?;
        assert_eq!(project(projection, [-0.5, -0.25, -1.]), [-1., -1., -1.]);
        assert_eq!(project(projection, [1.5, 0.75, -1.]), [1., 1., -1.]);
        assert_eq!(project(projection, [0., 0., -2.])[2], 1.);
        assert!(project(projection, [0., 0., -0.5])[2] < -1.);
        Ok(())
    }
    #[test]
    fn malformed_intervals_and_nonfinite_planes_reject() {
        for orthographic in [false, true] {
            for frustum in [
                [1., 1., 1., -1., 1., 2.],
                [-1., 1., 1., 1., 1., 2.],
                [-1., 1., 1., -1., 2., 2.],
                [-1., 1., 1., -1., 1., f32::INFINITY],
            ] {
                assert!(matrix(frustum, orthographic).is_err());
            }
        }
    }
    #[test]
    fn reversed_side_intervals_preserve_native_screen_axis_mirroring() -> Result<()> {
        for orthographic in [false, true] {
            let projection = matrix([1., -1., -1., 1., 1., 3.], orthographic)?;
            assert_eq!(project(projection, [1., 1., -1.])[..2], [-1., -1.]);
            assert_eq!(project(projection, [-1., -1., -1.])[..2], [1., 1.]);
        }
        Ok(())
    }
}
