//! Render matrices are affine column-major 4x4 matrices. Native hkTransform
//! records use four-lane vectors with padding, not a homogeneous bottom row.
//! Convert only at the presentation boundary; never change native storage.
pub fn from_havok(source: &super::native_hk_transform::Transform, scale: f32) -> [f32; 16] {
    let mut matrix = *source;
    for column in 0..3 {
        for row in 0..3 {
            matrix[column * 4 + row] *= scale;
        }
        matrix[column * 4 + 3] = 0.;
    }
    matrix[15] = 1.;
    matrix
}
pub fn from_entity(rotation: [f32; 9], position: [f32; 3], scale: f32) -> [f32; 16] {
    from_havok(
        &super::native_hk_transform::from_entity(rotation, position),
        scale,
    )
}
pub fn is_affine(matrix: &[f32; 16]) -> bool {
    matrix[3] == 0. && matrix[7] == 0. && matrix[11] == 0. && matrix[15] == 1.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_padding_never_becomes_homogeneous_coordinates() {
        let rotation = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
        let mut native = super::super::native_hk_transform::from_entity(rotation, [5., 6., 7.]);
        assert_eq!(native[15], 0.);
        for (i, bits) in [
            (3, 0x7fc00001),
            (7, 0x7f800000),
            (11, 0xff800000),
            (15, 0x41200000),
        ] {
            native[i] = f32::from_bits(bits);
        }
        let before = native.map(f32::to_bits);
        let matrix = from_havok(&native, 0.2);
        assert_eq!(
            native.map(f32::to_bits),
            before,
            "Presentation must not repair native padding in place"
        );
        assert_eq!(
            [matrix[3], matrix[7], matrix[11], matrix[15]],
            [0., 0., 0., 1.]
        );
        assert_eq!(matrix[12..15], [5., 6., 7.]);
        assert!(matrix.iter().all(|v| v.is_finite()));
        // Full homogeneous multiplication, including W, as in the vertex shader.
        let input = [2., 3., 4., 1.];
        let output: [f32; 4] = std::array::from_fn(|row| {
            (0..4)
                .map(|column| matrix[column * 4 + row] * input[column])
                .sum()
        });
        assert_eq!(output, [5.8, 6.6, 6.6, 1.]);
    }
    #[test]
    fn zero_and_negative_scale_retain_affine_position_and_w() {
        for scale in [0., -2., 1.] {
            let matrix = from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [1., 2., 3.], scale);
            assert_eq!([matrix[0], matrix[5], matrix[10]], [scale; 3]);
            assert_eq!(matrix[12..16], [1., 2., 3., 1.]);
        }
    }
}
