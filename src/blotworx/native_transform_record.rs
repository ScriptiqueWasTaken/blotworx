//! NiTransform reset, DOL804c026c via804c4930, used at Transform+e8
//! during parent detachment REL353e54. Storage is 13 big-endian float words.
use anyhow::{ensure, Result};
pub type Transform = [f32; 13];
/// DOL804c0384. Translation is negated, rotated, then multiplied by the
/// reciprocal scale with a rounding boundary at each stage. Rotation is
/// transposed, even for a nonorthogonal input; no matrix repair is performed.
pub fn inverse(input: &Transform) -> Transform {
    let rotation = std::array::from_fn(|i| input[(i % 3) * 3 + i / 3]);
    let scale = 1. / input[12];
    let negative = [input[9] * -1., input[10] * -1., input[11] * -1.];
    let translated = super::native_matrix::transform(&rotation, negative).map(|v| v * scale);
    std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            translated[i - 9]
        } else {
            scale
        }
    })
}
/// REL3542e0..354570: world pose composed with the cached parent's inverse.
/// Do not fold translation into (world-parent) before rotating: native rounds
/// inverse translation and rotated/scaled world coordinates separately.
pub fn local_pose(
    parent: &Transform,
    position: [f32; 3],
    rotation: &super::native_matrix::Matrix,
) -> ([f32; 3], super::native_matrix::Matrix) {
    let inverse = inverse(parent);
    let matrix: super::native_matrix::Matrix = inverse[..9].try_into().unwrap();
    let rotated = super::native_matrix::transform(&matrix, position);
    let position = std::array::from_fn(|i| rotated[i] * inverse[12] + inverse[9 + i]);
    (position, super::native_matrix::multiply(&matrix, rotation))
}
/// REL355d7c: cached parent * local pose, retaining rotate, scale, add stages.
pub fn world_pose(
    parent: &Transform,
    position: [f32; 3],
    rotation: &super::native_matrix::Matrix,
) -> ([f32; 3], super::native_matrix::Matrix) {
    let matrix: super::native_matrix::Matrix = parent[..9].try_into().unwrap();
    let rotated = super::native_matrix::transform(&matrix, position);
    (
        std::array::from_fn(|i| rotated[i] * parent[12] + parent[9 + i]),
        super::native_matrix::multiply(&matrix, rotation),
    )
}
#[derive(Clone, Copy)]
pub struct Constants {
    pub matrix_one: u32,       //80698f20
    pub matrix_zero: u32,      //80698f24
    pub translation: [u32; 3], // live80767140, file-unbacked BSS
    pub scale: u32,            //80698bd8
}
pub fn reset(record: &mut [u8], constants: Constants) -> Result<()> {
    ensure!(record.len() >= 0x34, "NiTransform record requires 52 bytes");
    for i in 0..9 {
        let word = if matches!(i, 0 | 4 | 8) {
            constants.matrix_one
        } else {
            constants.matrix_zero
        };
        record[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    for (i, word) in constants.translation.into_iter().enumerate() {
        record[0x24 + i * 4..0x28 + i * 4].copy_from_slice(&word.to_be_bytes());
    }
    record[0x30..0x34].copy_from_slice(&constants.scale.to_be_bytes());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_inverse_and_local_pose_keep_scale_translation_and_transpose() {
        let parent = [0., 1., 0., -1., 0., 0., 0., 0., 1., 2., 4., 6., 2.];
        let inv = inverse(&parent);
        assert_eq!(&inv[..9], &[0., -1., 0., 1., 0., 0., 0., 0., 1.]);
        assert_eq!(&inv[9..], &[2., -1., -3., 0.5]);
        let rotation = parent[..9].try_into().unwrap();
        let (p, r) = local_pose(&parent, [10., 14., 18.], &rotation);
        assert_eq!(p, [-5., 4., 6.]);
        assert_eq!(r, [1., 0., 0., 0., 1., 0., 0., 0., 1.]);
        let mut negative = parent;
        negative[12] = -2.;
        assert_eq!(&inverse(&negative)[9..], &[-2., 1., 3., -0.5]);
    }
    #[test]
    fn native_record_reset_writes_full_transform_only() -> Result<()> {
        let constants = Constants {
            matrix_one: 0x3f800000,
            matrix_zero: 0,
            translation: [0x80000000, 0x7fc01234, 0x40400000],
            scale: 0x3f800000,
        };
        let mut owner = [0xa5; 0x140];
        reset(&mut owner[0xe8..], constants)?;
        let words: Vec<u32> = owner[0xe8..0x11c]
            .chunks_exact(4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(
            words,
            [
                0x3f800000, 0, 0, 0, 0x3f800000, 0, 0, 0, 0x3f800000, 0x80000000, 0x7fc01234,
                0x40400000, 0x3f800000
            ]
        );
        assert!(owner[..0xe8].iter().all(|b| *b == 0xa5));
        assert!(owner[0x11c..].iter().all(|b| *b == 0xa5));
        let mut short = [0x5a; 51];
        assert!(reset(&mut short, constants).is_err());
        assert_eq!(short, [0x5a; 51]);
        Ok(())
    }
}
