//! Original RotatorComponent entity aim, REL3b33e0..3b376c.
//! Resolve live positions/axis before this calculation. Speed limiting and
//! completion IO remain in the existing scalar rotator update.
use super::{native_angle, native_matrix as matrix};
use anyhow::{ensure, Result};

pub fn target_angle(
    angle: f32,
    axis: [f32; 3],
    rotation: &matrix::Matrix,
    source: [f32; 3],
    destination: [f32; 3],
    distance: f32,
) -> Result<Option<f32>> {
    ensure!(
        angle.is_finite()
            && distance.is_finite()
            && axis
                .into_iter()
                .chain(source)
                .chain(destination)
                .chain(rotation.iter().copied())
                .all(f32::is_finite),
        "Nonfinite rotator tracking input"
    );
    let direction = std::array::from_fn::<_, 3, _>(|i| destination[i] - source[i]);
    // Native paired operations: z*z+x*x, then y*y (3b3428..3434).
    let squared = direction[2].mul_add(direction[2], direction[0] * direction[0])
        + direction[1] * direction[1];
    if squared >= distance * distance {
        return Ok(None);
    }
    let basis = |a: f32| -> Result<[f32; 3]> {
        let (sin, cos) = native_angle::sin_cos_medium(a as f64)?;
        let offset = matrix::rotation(axis, sin as f32, cos as f32);
        let mut result = matrix::multiply(&offset, rotation);
        // Native ignores the boolean return and consumes partially modified
        // columns on failure. Do not silently replace them with identity.
        matrix::reorthogonalize(&mut result);
        Ok([result[2], result[5], result[8]])
    };
    let forward = basis(-angle)?;
    let side_angle = (-1f32).mul_add(angle, f32::from_bits(0x3fc90fdb));
    let side = basis(side_angle)?;
    //805aea70 uses paired y/z products, fused x product, then adds z.
    let dot = |a: [f32; 3]| a[0].mul_add(direction[0], a[1] * direction[1]) + a[2] * direction[2];
    let target = native_angle::atan2(dot(side) as f64, dot(forward) as f64) as f32;
    ensure!(target.is_finite(), "Rotator tracking angle overflow");
    Ok(Some(target))
}
