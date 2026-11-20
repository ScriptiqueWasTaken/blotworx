//! EM1 rigid-body loader arithmetic. This is not a physics integrator.
use anyhow::{ensure, Result};
pub type Transform = [f32; 16];

/// Wii hkpConvexTransformShape: child+18 and padded hkTransform+20.
/// DOL80673760 reflects transform at+20;80278ddc composes all sixteen
/// lanes through8014c1d8 before asking the child for its bounds.
pub fn decode_shape_transform(data: &[u8], shape: usize) -> Result<Transform> {
    let start = shape
        .checked_add(0x20)
        .ok_or_else(|| anyhow::anyhow!("Shape transform offset overflow"))?;
    let end = start
        .checked_add(0x40)
        .ok_or_else(|| anyhow::anyhow!("Shape transform size overflow"))?;
    let bytes = data
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated convex shape transform"))?;
    let transform =
        std::array::from_fn(|i| f32::from_be_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    ensure!(
        transform.iter().all(|value| value.is_finite()),
        "Nonfinite convex shape transform"
    );
    Ok(transform)
}

/// REL296c44 returns XYZ from the resolved owner transform and explicitly
/// merges positive zero (REL4bcf70) into lane W.
pub fn entity_position_vector(position: [f32; 3]) -> [f32; 4] {
    [position[0], position[1], position[2], 0.]
}
/// REL378564..378610 preserves a shape phantom's existing padded rotation
/// and replaces all four translation lanes via23bb9c. Do not rotate it to
/// the new entity orientation or synthesize W for raw caller vectors.
pub fn relocated_shape_phantom(current: &Transform, position: [f32; 4]) -> Transform {
    let mut result = *current;
    result[12..16].copy_from_slice(&position);
    result
}

/// 8014c298: inverse(left) * right for rigid transforms. Native paired
/// shuffles duplicate column 2's X/Y into the padded output lane.
pub fn relative(left: &Transform, right: &Transform) -> Transform {
    std::array::from_fn(|i| {
        let col = i / 4;
        let row = i % 4;
        let v: [f32; 3] = std::array::from_fn(|axis| {
            if col == 3 {
                right[12 + axis] - left[12 + axis]
            } else {
                right[col * 4 + axis]
            }
        });
        let base = if row == 3 { 8 } else { row * 4 };
        let z = if row == 3 { 11 } else { base + 2 };
        left[z].mul_add(v[2], left[base + 1].mul_add(v[1], left[base] * v[0]))
    })
}

/// 801684a0: rotation only, XYZ input and all four output lanes.
pub fn rotate_vector(matrix: &Transform, value: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| {
        matrix[8 + i].mul_add(
            value[2],
            matrix[4 + i].mul_add(value[1], matrix[i] * value[0]),
        )
    })
}

/// 801684f8 starts with translation inside the FIRST fused add. This is
/// intentionally different from compose's translation-after-rotation order.
pub fn transform_point(matrix: &Transform, value: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| {
        matrix[8 + i].mul_add(
            value[2],
            matrix[4 + i].mul_add(value[1], matrix[i].mul_add(value[0], matrix[12 + i])),
        )
    })
}

/// REL 0x23b998/0x23ba00: native row-major NiMatrix3 and translation to
/// padded Havok columns. The conversion does not read NiTransform scale.
pub fn from_entity(rotation: [f32; 9], position: [f32; 3]) -> Transform {
    [
        rotation[0],
        rotation[3],
        rotation[6],
        0.,
        rotation[1],
        rotation[4],
        rotation[7],
        0.,
        rotation[2],
        rotation[5],
        rotation[8],
        0.,
        position[0],
        position[1],
        position[2],
        0.,
    ]
}

/// DOL 0x8014c414: left *= right, x product then y/z fused adds.
/// Translation adds the saved left translation after the rotated right one.
pub fn compose(left: &Transform, right: &Transform) -> Transform {
    std::array::from_fn(|i| {
        let col = i / 4;
        let row = i % 4;
        let x = left[row] * right[col * 4];
        let y = left[4 + row].mul_add(right[col * 4 + 1], x);
        let z = left[8 + row].mul_add(right[col * 4 + 2], y);
        if col == 3 {
            z + left[12 + row]
        } else {
            z
        }
    })
}

// 0x8014ce58/0x8014d174: frsp directly after frsqrte, one Newton step.
// Only positive finite inputs reach this helper; reject unsupported degenerate
// matrices explicitly rather than substituting identity or host sqrt.
pub(super) fn inverse_root(value: f32) -> Result<f32> {
    ensure!(
        value.is_finite() && value > 0.,
        "Invalid Havok rotation normalization length"
    );
    let estimate = super::pose_blend::estimate(value);
    let correction = -value.mul_add(estimate * estimate, -1.);
    let refined = correction.mul_add(0.5 * estimate, estimate);
    Ok(if refined >= 0. { refined } else { 0. })
}

/// DOL 0x8014ce58 (also reached through 0x8014d060), without the separate
/// normalization pass. Placement uses this result directly. Returns xyzw.
pub(super) fn rotation_quaternion_raw(matrix: &Transform) -> Result<[f32; 4]> {
    ensure!(
        matrix.iter().all(|v| v.is_finite()),
        "Nonfinite Havok transform"
    );
    let trace = matrix[10] + (matrix[0] + matrix[5]);
    let mut q = [0.; 4];
    if trace > 0. {
        let squared = 1. + trace;
        let root = squared * inverse_root(squared)?;
        let factor = 0.5 / root;
        q = [
            factor * (matrix[6] - matrix[9]),
            factor * (matrix[8] - matrix[2]),
            factor * (matrix[1] - matrix[4]),
            0.5 * root,
        ];
    } else {
        let mut i = if matrix[5] > matrix[0] { 1 } else { 0 };
        if matrix[10] > matrix[i * 5] {
            i = 2;
        }
        let j = [1, 2, 0][i];
        let k = [1, 2, 0][j];
        let squared = 1. + (matrix[i * 5] - (matrix[j * 5] + matrix[k * 5]));
        let root = squared * inverse_root(squared)?;
        let factor = 0.5 / root;
        q[i] = 0.5 * root;
        q[3] = factor * (matrix[j * 4 + k] - matrix[k * 4 + j]);
        q[j] = factor * (matrix[i * 4 + j] + matrix[j * 4 + i]);
        q[k] = factor * (matrix[i * 4 + k] + matrix[k * 4 + i]);
    }
    Ok(q)
}

/// DOL 0x8014ce58 followed by 0x8014d174, including largest-diagonal
/// branch and paired-lane quaternion normalization. Returns xyzw.
pub fn rotation_quaternion(matrix: &Transform) -> Result<[f32; 4]> {
    let q = rotation_quaternion_raw(matrix)?;
    let squared = q[2].mul_add(q[2], q[0] * q[0]) + q[3].mul_add(q[3], q[1] * q[1]);
    let inverse = inverse_root(squared)?;
    Ok(q.map(|v| v * inverse))
}

/// DOL 0x8014bdd4 writes rotation columns only; translation remains intact.
pub fn set_rotation(matrix: &mut Transform, q: [f32; 4]) {
    let [x, y, z, w] = q;
    let x2 = x + x;
    let y2 = y + y;
    let z2 = z + z;
    let xx = x2 * x;
    let yy = y2 * y;
    let zz = z2 * z;
    let xy = y2 * x;
    let xz = z2 * x;
    let yz = z2 * y;
    let xw = x2 * w;
    let yw = y2 * w;
    let zw = z2 * w;
    matrix[..12].copy_from_slice(&[
        1. - (yy + zz),
        xy + zw,
        xz - yw,
        0.,
        xy - zw,
        1. - (xx + zz),
        yz + xw,
        0.,
        xz + yw,
        yz - xw,
        1. - (xx + yy),
        0.,
    ]);
}

/// REL 0x382c54..0x382ce8: compose, normalize rotation, set body transform.
/// Does not apply component scale or alter the serialized collision geometry.
pub fn initial_body(entity: &Transform, serialized_body: &Transform) -> Result<Transform> {
    let mut output = compose(entity, serialized_body);
    let q = rotation_quaternion(&output)?;
    set_rotation(&mut output, q);
    Ok(output)
}

/// REL 0x243320..0x243420, called by the keyframed-body update at 0x2425f0.
/// The target is the body's origin, but velocity is measured at its center of
/// mass. Preserve the paired-single dot, cross and fused rotation arithmetic;
/// using origin displacement gives the wrong velocity for an off-center body.
/// `orientation` is the already-normalized target quaternion (xyzw). The caller
/// supplies the reciprocal of the original simulation delta, not render delta.
/// This is the linear part only: angular velocity and activation are separate.
pub fn keyframe_linear_velocity(
    target_origin: [f32; 3],
    orientation: [f32; 4],
    local_center: [f32; 3],
    current_center: [f32; 3],
    inverse_delta: f32,
) -> Result<[f32; 3]> {
    let extend = |v: [f32; 3]| [v[0], v[1], v[2], 0.];
    Ok(keyframe_linear_velocity4(
        extend(target_origin),
        orientation,
        extend(local_center),
        extend(current_center),
        inverse_delta,
    )?[..3]
        .try_into()
        .unwrap())
}

/// All four paired lanes of the same calculation. W is copied by the native
/// motion setter despite not participating in the XYZ write threshold.
pub fn keyframe_linear_velocity4(
    target_origin: [f32; 4],
    orientation: [f32; 4],
    local_center: [f32; 4],
    current_center: [f32; 4],
    inverse_delta: f32,
) -> Result<[f32; 4]> {
    ensure!(
        target_origin
            .iter()
            .chain(orientation.iter())
            .chain(local_center.iter())
            .chain(current_center.iter())
            .all(|v| v.is_finite())
            && inverse_delta.is_finite()
            && inverse_delta > 0.,
        "Invalid keyframed-body velocity input"
    );
    let [x, y, z, w] = orientation;
    let [cx, cy, cz, _] = local_center;
    let dot = z.mul_add(cz, x * cx) + y * cy;
    let cross = [
        y.mul_add(cz, -(cy * z)),
        x.mul_add(cz, -(cx * z)),
        -y.mul_add(cx, -(cy * x)),
    ];
    let cross = [cross[0], -cross[1], cross[2], -cross[1]];
    let factor = w.mul_add(w, -0.5);
    let velocity = std::array::from_fn(|i| {
        let base = orientation[i].mul_add(dot, local_center[i] * factor);
        let rotated_half = cross[i].mul_add(w, base);
        let target_center = (rotated_half + rotated_half) + target_origin[i];
        (target_center - current_center[i]) * inverse_delta
    });
    ensure!(
        velocity.iter().all(|v| v.is_finite()),
        "Nonfinite keyframed-body velocity"
    );
    Ok(velocity)
}

/// REL 0x243500..0x243764: shortest-arc target angular velocity, in world
/// axes. Uses the original acos and repeated quaternion/axis normalization.
/// Activation and the 0.001 per-axis velocity-change test belong to the caller.
pub fn keyframe_angular_velocity(
    target: [f32; 4],
    current: [f32; 4],
    inverse_delta: f32,
) -> Result<[f32; 3]> {
    Ok(
        keyframe_angular_velocity4(target, current, inverse_delta)?[..3]
            .try_into()
            .unwrap(),
    )
}

/// Full four-lane axis normalization/sign/scale from REL 0x2436b4..0x24374c.
pub fn keyframe_angular_velocity4(
    target: [f32; 4],
    current: [f32; 4],
    inverse_delta: f32,
) -> Result<[f32; 4]> {
    ensure!(
        target.iter().chain(current.iter()).all(|v| v.is_finite())
            && inverse_delta.is_finite()
            && inverse_delta > 0.,
        "Invalid keyframed-body angular input"
    );
    let [x, y, z, w] = target;
    let [cx, cy, cz, cw] = current;
    // current.xyz cross target.xyz; retain the native fused subtract signs.
    let cross = [
        cy.mul_add(z, -(y * cz)),
        -cx.mul_add(z, -(x * cz)),
        -cy.mul_add(x, -(y * cx)),
    ];
    let mut delta = [0.; 4];
    for i in 0..3 {
        let difference = -current[i].mul_add(w, -cross[i]);
        delta[i] = target[i].mul_add(cw, difference);
    }
    delta[3] = z.mul_add(cz, x * cx) + w.mul_add(cw, y * cy);
    let length4 = delta[2].mul_add(delta[2], delta[0] * delta[0])
        + delta[3].mul_add(delta[3], delta[1] * delta[1]);
    let normalizer = inverse_root(length4)?;
    delta = delta.map(|v| v * normalizer);
    let length3 = delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1];
    if length3 == 0. {
        return Ok([0.; 4]);
    }
    let root = length3 * inverse_root(length3)?;
    if root <= f32::EPSILON {
        return Ok([0.; 4]);
    }
    let cosine = delta[3].abs();
    let angle = 2.
        * if cosine >= 1. {
            0.
        } else {
            super::native_angle::acos(cosine as f64) as f32
        };
    if angle.abs() <= 0.001 {
        return Ok([0.; 4]);
    }
    let axis_normalizer = inverse_root(length3)?;
    let magnitude = angle * inverse_delta;
    let velocity = std::array::from_fn(|i| {
        let axis = delta[i] * axis_normalizer;
        (if delta[3] < 0. { -axis } else { axis }) * magnitude
    });
    ensure!(
        velocity.iter().all(|v| v.is_finite()),
        "Nonfinite keyframed-body angular velocity"
    );
    Ok(velocity)
}

#[cfg(test)]
mod tests {
    #[test]
    fn entity_position_and_shape_phantom_relocation_preserve_native_lanes() {
        let position = super::entity_position_vector([-0., 2., 3.]);
        assert_eq!(
            position.map(f32::to_bits),
            [0x80000000, 2f32.to_bits(), 3f32.to_bits(), 0]
        );
        let current = std::array::from_fn(|i| f32::from_bits(0x3f000000 + i as u32));
        let raw = [-0., 2., 3., f32::from_bits(0x7fc12345)];
        let moved = super::relocated_shape_phantom(&current, raw);
        assert_eq!(
            moved[..12].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            current[..12]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            moved[12..].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            raw.map(f32::to_bits)
        );
        let moved = super::relocated_shape_phantom(&current, position);
        assert_eq!(moved[15].to_bits(), 0);
    }
    #[test]
    fn relative_and_point_operations_preserve_native_lane_and_fusion_order() {
        use super::*;
        let left = std::array::from_fn(|i| i as f32 + 1.);
        let identity = from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        assert_eq!(
            relative(&left, &identity),
            [1., 5., 9., 9., 2., 6., 10., 10., 3., 7., 11., 12., -86., -254., -422., -437.]
        );
        let mut matrix = identity;
        matrix[0] = 1.0000001192092896;
        matrix[12] = -1.000000238418579;
        let point = [1.0000001192092896, 0., 0., 999.];
        assert_eq!(
            transform_point(&matrix, point)[0].to_bits(),
            (2f32.powi(-46)).to_bits()
        );
        assert_eq!(rotate_vector(&matrix, point)[0] + matrix[12], 0.);
    }
    use super::*;
    const ID: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    #[test]
    fn original_entity_conversion_preserves_every_word() {
        let capture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_entity_conversion.json"
        ))
        .unwrap();
        assert_eq!(capture["complete"], true);
        assert_eq!(capture["mode"], "entity");
        assert_eq!(
            capture["rel_sha256"],
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        let samples = capture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut rotations = std::collections::HashSet::new();
        for (index, sample) in samples.iter().enumerate() {
            let input = sample["entity"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                .collect::<Vec<_>>();
            assert_eq!(input.len(), 12);
            assert!(rotations.insert(input[..9].iter().map(|v| v.to_bits()).collect::<Vec<_>>()));
            let actual = from_entity(
                input[..9].try_into().unwrap(),
                input[9..12].try_into().unwrap(),
            );
            for (lane, (value, expected)) in actual
                .into_iter()
                .zip(sample["result"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(
                    value.to_bits() as u64,
                    expected.as_u64().unwrap(),
                    "sample {index} lane {lane}"
                );
            }
        }
    }
    #[test]
    fn original_keyframe_capture_matches_every_velocity_bit() {
        let capture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_keyframe_velocity.json"
        ))
        .unwrap();
        assert_eq!(capture["complete"], true);
        assert_eq!(capture["mode"], "keyframe");
        assert_eq!(
            capture["rel_sha256"],
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        let samples = capture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        assert!(samples
            .iter()
            .any(|s| s["angular"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_u64().unwrap() & 0x7fffffff != 0)));
        let mut differences = Vec::new();
        for (index, sample) in samples.iter().enumerate() {
            fn vector<const N: usize>(sample: &serde_json::Value, name: &str) -> [f32; N] {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|word| f32::from_bits(u32::try_from(word.as_u64().unwrap()).unwrap()))
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            }
            let inverse = f32::from_bits(sample["inverse_delta_bits"].as_u64().unwrap() as u32);
            let linear = keyframe_linear_velocity(
                vector(sample, "target_origin"),
                vector(sample, "target"),
                vector(sample, "local_center"),
                vector(sample, "current_center"),
                inverse,
            )
            .unwrap();
            let angular = keyframe_angular_velocity(
                vector(sample, "target"),
                vector(sample, "current"),
                inverse,
            )
            .unwrap();
            for (name, actual) in [("linear", linear), ("angular", angular)] {
                for (axis, (a, b)) in actual
                    .into_iter()
                    .zip(vector::<3>(sample, name))
                    .enumerate()
                {
                    if a.to_bits() != b.to_bits() {
                        differences.push(format!(
                            "sample {index} {name}[{axis}]: {a:?} ({:08x}) != {b:?} ({:08x})",
                            a.to_bits(),
                            b.to_bits()
                        ));
                    }
                }
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }
    #[test]
    fn angular_keyframing_selects_short_arc_and_world_axis() {
        let identity = [0., 0., 0., 1.];
        let target = [0.5, 0.5, 0.5, 0.5];
        assert_eq!(
            keyframe_angular_velocity(identity, identity, 60.).unwrap(),
            [0.; 3]
        );
        let a = keyframe_angular_velocity(target, identity, 1.).unwrap();
        let b = keyframe_angular_velocity(target.map(|v| -v), identity, 1.).unwrap();
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-6);
            assert!((a[i] - 1.2091996).abs() < 1e-6);
        }
        // target * inverse(current) is a half-turn about world Z here.
        let a = keyframe_angular_velocity([0., 1., 0., 0.], [1., 0., 0., 0.], 1.).unwrap();
        assert!(a[0].abs() < 1e-6 && a[1].abs() < 1e-6);
        assert!((a[2] - std::f32::consts::PI).abs() < 1e-6);
        assert!(keyframe_angular_velocity([0.; 4], identity, 60.).is_err());
    }
    #[test]
    fn keyframing_uses_rotated_center_of_mass_not_origin_displacement() {
        // Exact unit quaternion: cycles x->y->z->x. A stationary body origin
        // still needs COM motion when its local center is off the rotation axis.
        let q = [0.5, 0.5, 0.5, 0.5];
        assert_eq!(
            keyframe_linear_velocity([10., 20., 30.], q, [2., 3., 4.], [12., 23., 34.], 60.)
                .unwrap(),
            [120., -60., -60.]
        );
        // Compensating translation holds that center still while rotating.
        assert_eq!(
            keyframe_linear_velocity([8., 21., 31.], q, [2., 3., 4.], [12., 23., 34.], 60.)
                .unwrap(),
            [0.; 3]
        );
        assert_eq!(
            keyframe_linear_velocity(
                [11., 20., 30.],
                [0., 0., 0., 1.],
                [2., 3., 4.],
                [12., 23., 34.],
                30.
            )
            .unwrap(),
            [30., 0., 0.]
        );
        for delta in [0., -1., f32::INFINITY, f32::NAN] {
            assert!(keyframe_linear_velocity([0.; 3], q, [0.; 3], [0.; 3], delta).is_err());
        }
    }
    #[test]
    fn captured_retail_rotation_normalization_matches_every_float_bit() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_transform_normalize.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["dol_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut unique = std::collections::HashSet::new();
        for (index, sample) in samples.iter().enumerate() {
            let words = |name: &str| -> Transform {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            };
            let mut result = words("left");
            assert!(unique.insert(result[..12].iter().map(|v| v.to_bits()).collect::<Vec<_>>()));
            let quaternion = rotation_quaternion(&result).unwrap();
            set_rotation(&mut result, quaternion);
            for (lane, (actual, expected)) in result.into_iter().zip(words("result")).enumerate() {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "sample {index} lane {lane}"
                );
            }
        }
    }
    #[test]
    fn captured_retail_compositions_match_every_float_bit() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_transform_compose.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["dol_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut unique = std::collections::HashSet::new();
        for (index, sample) in samples.iter().enumerate() {
            assert!(unique.insert((sample["left"].to_string(), sample["right"].to_string())));
            let words = |name: &str| -> Transform {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            };
            let result = compose(&words("left"), &words("right"));
            for (lane, (actual, expected)) in result.into_iter().zip(words("result")).enumerate() {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "sample {index} lane {lane}"
                );
            }
        }
    }
    #[test]
    fn body_offset_is_rotated_before_entity_translation() {
        let entity = from_entity([0., -1., 0., 1., 0., 0., 0., 0., 1.], [10., 20., 30.]);
        let body = from_entity(ID, [2., 3., 4.]);
        let result = initial_body(&entity, &body).unwrap();
        assert_eq!(&result[12..], [7., 22., 34., 0.]);
        for (a, b) in result[..12].iter().zip(&entity[..12]) {
            assert!((a - b).abs() < 1e-6);
        }
        assert_ne!(compose(&entity, &body), compose(&body, &entity));
    }
    #[test]
    fn quaternion_branches_preserve_half_turn_axes_and_quarter_turn_direction() {
        for q in [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
            [0.5, 0.5, 0.5, 0.5],
            [
                0.,
                0.,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
        ] {
            let mut matrix = from_entity(ID, [3., 4., 5.]);
            set_rotation(&mut matrix, q);
            let recovered = rotation_quaternion(&matrix).unwrap();
            for (a, b) in recovered.into_iter().zip(q) {
                assert!((a - b).abs() < 1e-6, "{recovered:?} != {q:?}");
            }
            let result = initial_body(&from_entity(ID, [0.; 3]), &matrix).unwrap();
            assert_eq!(&result[12..], [3., 4., 5., 0.]);
        }
    }
    #[test]
    fn composition_preserves_fused_rounding() {
        let mut a = from_entity(ID, [0.; 3]);
        let mut b = a;
        a[0] = -1.;
        a[4] = f32::from_bits(0x3f800001);
        b[0] = 1.;
        b[1] = f32::from_bits(0x3f7ffffe);
        assert_eq!(compose(&a, &b)[0].to_bits(), (-2f32.powi(-46)).to_bits());
        a[0] = f32::NAN;
        assert!(initial_body(&a, &b).is_err());
    }
}
