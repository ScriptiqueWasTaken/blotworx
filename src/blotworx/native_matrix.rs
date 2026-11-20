//! Retail NiMatrix3 arithmetic. Arrays follow the nine floats in native memory.
//! Preserve the PPC single-precision/FMA boundaries, including dot order y,x,z.
pub type Matrix = [f32; 9];

/// DOL 0x804c4b30, after its double sin/cos calls and frsp conversions.
/// The caller supplies those results explicitly; host trigonometry is not
/// implicitly substituted for the still-unported retail sin/cos routines.
pub fn rotation(axis: [f32; 3], sin: f32, cos: f32) -> Matrix {
    let [x, y, z] = axis;
    let omc = 1. - cos;
    let xy = omc * (x * y);
    let xz = omc * (x * z);
    let yz = omc * (y * z);
    let xs = x * sin;
    let ys = y * sin;
    let zs = z * sin;
    [
        (x * x).mul_add(omc, cos),
        xy + zs,
        xz - ys,
        xy - zs,
        (y * y).mul_add(omc, cos),
        yz + xs,
        xz + ys,
        yz - xs,
        (z * z).mul_add(omc, cos),
    ]
}

/// REL 0x3b39d4..0x3b3af4: rotator delta * current entity rotation.
pub fn multiply(left: &Matrix, right: &Matrix) -> Matrix {
    std::array::from_fn(|i| {
        let row = i / 3;
        let col = i % 3;
        let middle = left[row * 3 + 1] * right[3 + col];
        let first = left[row * 3].mul_add(right[col], middle);
        left[row * 3 + 2].mul_add(right[6 + col], first)
    })
}

/// DOL804c50b4/804c48e4 transposes, including nonorthogonal matrices.
/// REL354638..3547d8 computes local rotation from this cached parent transpose
/// and captured world rotation. Do not substitute a general inverse or repair
/// authored matrices; native uses the same y,x,z fused dot boundaries.
pub fn local_rotation(parent: &Matrix, world: &Matrix) -> Matrix {
    let transpose = std::array::from_fn(|i| parent[(i % 3) * 3 + i / 3]);
    multiply(&transpose, world)
}

/// Preserve an attached object's local rotation when its world rotation changes.
/// The effective parent includes the current animated socket, not merely the
/// owning entity. Recover it from world = socket * local before publishing.
pub fn attached_rotation(local: &Matrix, previous_world: &Matrix, next_world: &Matrix) -> anyhow::Result<Matrix> {
    // Bone query/Qs conversion can retain small scale/orthogonality errors.
    // A transpose would apply those errors again on every component update.
    let [a,b,c,d,e,f,g,h,i] = previous_world.map(f64::from);
    let inverse = [e*i-f*h,c*h-b*i,b*f-c*e,f*g-d*i,a*i-c*g,c*d-a*f,d*h-e*g,b*g-a*h,a*e-b*d];
    let determinant = a*inverse[0]+b*inverse[3]+c*inverse[6];
    anyhow::ensure!(determinant.is_finite() && determinant != 0., "Singular animated socket rotation");
    let correction = std::array::from_fn(|index| {
        let row=index/3;let column=index%3;
        (0..3).map(|k|inverse[row*3+k]*f64::from(next_world[k*3+column])/determinant).sum::<f64>() as f32
    });
    Ok(multiply(local, &correction))
}

/// REL 0x3b2d5c..0x3b2dc4: LocalAxisOfRotation uses current entity rotation.
pub fn transform(matrix: &Matrix, vector: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| {
        matrix[i * 3 + 2].mul_add(
            vector[2],
            matrix[i * 3].mul_add(vector[0], matrix[i * 3 + 1] * vector[1]),
        )
    })
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1]))
}

/// DOL 0x804c5128. Native failure leaves partially modified columns intact;
/// do not replace the matrix with identity or normalize the untouched columns.
pub fn reorthogonalize(matrix: &mut Matrix) -> bool {
    const EPSILON: f32 = f32::from_bits(0x3727c5ac); // DOL 0x80698f2c
    for column in 0..3 {
        let mut value = std::array::from_fn(|r| matrix[r * 3 + column]);
        for previous in 0..column {
            let basis = std::array::from_fn(|r| matrix[r * 3 + previous]);
            let projection = dot(basis, value);
            // fnmsubs negates the rounded fused subtract. Moving the sign
            // onto the multiplier would lose the native negative-zero result.
            value = std::array::from_fn(|r| -projection.mul_add(basis[r], -value[r]));
        }
        // Stores precede the length/epsilon check in the original routine.
        for row in 0..3 {
            matrix[row * 3 + column] = value[row];
        }
        let length = super::cursor::native_sqrt(dot(value, value));
        if length < EPSILON {
            return false;
        }
        let inverse = 1. / length;
        for row in 0..3 {
            matrix[row * 3 + column] = value[row] * inverse;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: Matrix = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    #[test]
    fn attached_rotator_does_not_accumulate_animated_socket_rotation() -> anyhow::Result<()> {
        let socket = rotation([0., 1., 0.], 1., 0.);
        let delta = rotation([1., 0., 0.], 1., 0.);
        let mut local = ID;
        let mut world = socket;
        for _ in 0..16 {
            let next = multiply(&delta, &world);
            local = attached_rotation(&local, &world, &next)?;
            world = multiply(&socket, &local);
            assert_eq!(world, next);
        }
        Ok(())
    }
    #[test]
    fn local_rotation_uses_native_transpose_without_matrix_repair() {
        let parent = [0., 1., 0., -1., 0., 0., 0., 0., 1.];
        let local = [1., 0., 0., 0., 0., 1., 0., -1., 0.];
        let world = multiply(&parent, &local);
        assert_eq!(local_rotation(&parent, &world), local);
        // Native transpose semantics remain observable for a nonrigid matrix.
        let authored = [1., 2., 3., 4., 5., 6., 7., 8., 9.];
        assert_eq!(
            local_rotation(&authored, &ID),
            [1., 4., 7., 2., 5., 8., 3., 6., 9.]
        );
    }
    #[test]
    fn authored_z_rotation_uses_native_direction_and_local_axis() {
        let z = rotation([0., 0., 1.], 1., 0.);
        assert_eq!(transform(&z, [1., 0., 0.]), [0., -1., 0.]);
        assert_eq!(multiply(&z, &ID), z);
        let axis = transform(&z, [1., 0., 0.]);
        let local = rotation(axis, 1., 0.);
        assert_eq!(transform(&local, [0., 0., 1.]), [1., 0., 0.]);
    }
    #[test]
    fn native_orthogonalization_removes_shear_and_preserves_handedness() {
        let mut m = [2., 1., 3., 0., 3., 2., 0., 0., -4.];
        assert!(reorthogonalize(&mut m));
        // The one-refinement Wii sqrt is not required to produce exact units.
        for (actual, expected) in m.into_iter().zip([1., 0., 0., 0., 1., 0., 0., 0., -1.]) {
            assert!((actual - expected).abs() < 0.000001);
        }
    }
    #[test]
    fn singular_second_column_keeps_partial_native_result() {
        let mut m = [2., 0., 7., 0., 0., 8., 0., 0., 9.];
        assert!(!reorthogonalize(&mut m));
        assert!(m[0] > 0.99999 && m[0] < 1.00001);
        assert_eq!([m[2], m[5], m[8]], [7., 8., 9.]);
        let mut zero = [0.; 9];
        assert!(!reorthogonalize(&mut zero));
        assert_eq!(zero, [0.; 9]);
    }
    #[test]
    fn fused_negative_subtraction_retains_signed_zero() {
        let mut matrix = ID;
        assert!(reorthogonalize(&mut matrix));
        assert_eq!(matrix[1].to_bits(), (-0f32).to_bits());
        assert_eq!(matrix[7].to_bits(), (-0f32).to_bits());
    }
}
