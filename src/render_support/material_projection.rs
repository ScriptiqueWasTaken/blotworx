//! Active-camera parameters for EM1's pinned texture projection.
//! Source REL177928..177a80; the stock zero-initialized BSS threshold at11118
//! has no direct writer in the retail module. External runtime overrides and
//! exact GX arithmetic/quantization remain separate fidelity work.
use anyhow::{ensure, Result};

pub fn bound_parameters(
    view_projection: &[f32; 16],
    view: &[f32; 16],
    center: [f32; 3],
) -> Result<[f32; 3]> {
    ensure!(
        center
            .iter()
            .chain(view_projection)
            .chain(view)
            .all(|v| v.is_finite()),
        "Nonfinite projection bound/camera"
    );
    let point = [center[0], center[1], center[2], 1.];
    let clip: [f32; 4] = std::array::from_fn(|row| {
        (0..4)
            .map(|column| view_projection[column * 4 + row] * point[column])
            .sum()
    });
    ensure!(
        clip[3] != 0.,
        "Pinned projection bound lies on the camera plane"
    );
    let forward_depth = -(0..4)
        .map(|column| view[column * 4 + 2] * point[column])
        .sum::<f32>();
    let depth = forward_depth.max(f32::from_bits(0x358637bd));
    let parameters = [
        0.5 * (clip[0] / clip[3] + 1.),
        0.5 * (1. - clip[1] / clip[3]),
        depth * f32::from_bits(0x3dcccccd),
    ];
    ensure!(
        parameters.iter().all(|v| v.is_finite()),
        "Pinned projection overflow"
    );
    Ok(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_world_bound_camera_depth_and_preserves_behind_camera_anchor() -> Result<()> {
        let view = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let projection = [
            2., 0., 0., 0., 0., 3., 0., 0., 0., 0., 0., -1., 0., 0., 0., 0.,
        ];
        let front = bound_parameters(&projection, &view, [1., 2., -10.])?;
        for (value, expected) in front.into_iter().zip([0.6, 0.2, 1.]) {
            assert!((value - expected).abs() < 1e-7);
        }
        let behind = bound_parameters(&projection, &view, [1., 2., 10.])?;
        for (value, expected) in behind[..2].iter().zip([0.4, 0.8]) {
            assert!((*value - expected).abs() < 1e-7);
        }
        assert_eq!(
            behind[2],
            f32::from_bits(0x358637bd) * f32::from_bits(0x3dcccccd)
        );
        assert!(bound_parameters(&projection, &view, [0.; 3]).is_err());
        Ok(())
    }
}
