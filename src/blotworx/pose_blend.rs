//! Retail transform-track blending: DOL 0x801e8e14 (dense) and
//! 0x80204928 (sparse), called by generator-output blend 0x80204494.
//! Missing tracks stay missing until the graph fills the reference pose.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::ffi_anim::FfiTransform;

/// Positive finite f32 input to the Gekko reciprocal-square-root estimate.
/// The table contains hardware lookup coefficients, also documented by
/// Dolphin's Common/FloatUtils.cpp frsqrte_expected table. Special double/NaN
/// cases are unnecessary here: normalization validates the f32 length first.
pub(super) fn estimate(value: f32) -> f32 {
    estimate_double(value) as f32
}
/// Gekko fmuls with a retained-double frsqrte estimate in both A/C slots.
/// Only C is rounded to25 significant bits before the single result. This is
/// NOT the square of an already narrowed estimate, nor a double fmul.
#[inline]
pub(super) fn square_estimate(estimate: f64) -> f32 {
    let bits = estimate.to_bits();
    let operand = f64::from_bits(((bits >> 28) + ((bits >> 27) & 1)) << 28);
    (estimate * operand) as f32
}
/// frsqrte leaves a double-precision estimate in the register. Callers without
/// an intervening frsp must retain it until their first single operation.
pub(super) fn estimate_double(value: f32) -> f64 {
    const COEFFICIENTS: [(i64, i64); 32] = [
        (0x1a7e800, -0x568),
        (0x17cb800, -0x4f3),
        (0x1552800, -0x48d),
        (0x130c000, -0x435),
        (0x10f2000, -0x3e7),
        (0x0eff000, -0x3a2),
        (0x0d2e000, -0x365),
        (0x0b7c000, -0x32e),
        (0x09e5000, -0x2fc),
        (0x0867000, -0x2d0),
        (0x06ff000, -0x2a8),
        (0x05ab800, -0x283),
        (0x046a000, -0x261),
        (0x0339800, -0x243),
        (0x0218800, -0x226),
        (0x0105800, -0x20b),
        (0x3ffa000, -0x7a4),
        (0x3c29000, -0x700),
        (0x38aa000, -0x670),
        (0x3572000, -0x5f2),
        (0x3279000, -0x584),
        (0x2fb7000, -0x524),
        (0x2d26000, -0x4cc),
        (0x2ac0000, -0x47e),
        (0x2881000, -0x43a),
        (0x2665000, -0x3fa),
        (0x2468000, -0x3c2),
        (0x2287000, -0x38e),
        (0x20c1000, -0x35e),
        (0x1f12000, -0x332),
        (0x1d79000, -0x30a),
        (0x1bf4000, -0x2e6),
    ];
    let bits = (value as f64).to_bits(); // Even f32 subnormals are normal doubles.
    let exponent = ((bits >> 52) & 2047) as i32 - 1023;
    let fraction = ((bits & ((1u64 << 52) - 1)) >> 37) as usize;
    let bank = if exponent.rem_euclid(2) == 0 { 16 } else { 0 };
    let (base, slope) = COEFFICIENTS[bank + (fraction >> 11)];
    let mantissa = (base + slope * (fraction & 2047) as i64) as u64;
    let output_exponent = (1022 - exponent.div_euclid(2)) as u64;
    f64::from_bits((output_exponent << 52) | (mantissa << 26))
}

fn paired_dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[2].mul_add(b[2], a[0] * b[0]) + a[3].mul_add(b[3], a[1] * b[1])
}

/// Normal (non-additive), unmasked blender accumulation. DOL 0x801f7504
/// accumulates per-bone weights; 0x801f7ee0 -> 0x8014c924 divides translation
/// and scale by those sums, then normalizes quaternion sums once.
pub fn weighted_tracks(
    samples: &[(&[Option<FfiTransform>], f32)],
) -> Result<Vec<Option<FfiTransform>>> {
    let count = samples.first().context("No weighted pose inputs")?.0.len();
    let mut output: Vec<Option<FfiTransform>> = vec![None; count];
    let mut totals = vec![0f32; count];
    for &(tracks, weight) in samples {
        ensure!(
            tracks.len() == count && weight.is_finite() && weight >= 0.,
            "Invalid normal blender input"
        );
        if weight == 0. {
            continue;
        }
        for ((out, total), track) in output.iter_mut().zip(&mut totals).zip(tracks) {
            let Some(track) = track else {
                continue;
            };
            ensure!(
                track
                    .translation
                    .iter()
                    .chain(&track.rotation)
                    .chain(&track.scale)
                    .all(|x| x.is_finite()),
                "Non-finite weighted pose"
            );
            let out = out.get_or_insert(FfiTransform {
                translation: [0.; 3],
                rotation: [0.; 4],
                scale: [0.; 3],
            });
            let signed = if paired_dot(out.rotation, track.rotation) < 0. {
                -weight
            } else {
                weight
            };
            for i in 0..3 {
                out.translation[i] = track.translation[i].mul_add(weight, out.translation[i]);
                out.scale[i] = track.scale[i].mul_add(weight, out.scale[i]);
            }
            for i in 0..4 {
                out.rotation[i] = track.rotation[i].mul_add(signed, out.rotation[i]);
            }
            *total += weight;
        }
    }
    for (out, total) in output.iter_mut().zip(totals) {
        let Some(out) = out else {
            continue;
        };
        ensure!(
            total.is_finite() && total > 0.,
            "Invalid weighted pose normalization"
        );
        let reciprocal = 1. / total;
        for i in 0..3 {
            out.translation[i] *= reciprocal;
            out.scale[i] *= reciprocal;
        }
        let length = paired_dot(out.rotation, out.rotation);
        ensure!(
            length.is_finite() && length > 0.,
            "Degenerate weighted quaternion"
        );
        let reciprocal = estimate(length);
        let correction = (-length).mul_add(reciprocal * reciprocal, 1.);
        let factor = correction.mul_add(0.5 * reciprocal, reciprocal);
        for v in &mut out.rotation {
            *v *= factor;
        }
        ensure!(
            out.translation
                .iter()
                .chain(&out.scale)
                .all(|v| v.is_finite()),
            "Weighted pose overflow"
        );
    }
    Ok(output)
}

/// Clip echo accumulation (0x801fac50), followed by one normalization
/// (0x801fad74). Repeated pairwise nlerps change the result for multiple echoes.
pub fn echo_tracks(
    samples: impl IntoIterator<Item = (Vec<Option<FfiTransform>>, f32)>,
) -> Result<Vec<Option<FfiTransform>>> {
    let mut output: Vec<Option<FfiTransform>> = Vec::new();
    let mut initialized = false;
    for (tracks, weight) in samples {
        ensure!(weight.is_finite(), "Invalid echo weight");
        if !initialized {
            output.resize(tracks.len(), None);
            initialized = true;
        }
        ensure!(tracks.len() == output.len(), "Echo binding changed");
        for (out, track) in output.iter_mut().zip(tracks) {
            let Some(track) = track else {
                continue;
            };
            ensure!(
                track
                    .translation
                    .iter()
                    .chain(&track.rotation)
                    .chain(&track.scale)
                    .all(|v| v.is_finite()),
                "Non-finite echo pose"
            );
            let out = out.get_or_insert(FfiTransform {
                translation: [0.; 3],
                rotation: [0.; 4],
                scale: [0.; 3],
            });
            let signed = if paired_dot(out.rotation, track.rotation) < 0. {
                -weight
            } else {
                weight
            };
            for i in 0..3 {
                out.translation[i] = track.translation[i].mul_add(weight, out.translation[i]);
                out.scale[i] = track.scale[i].mul_add(weight, out.scale[i]);
            }
            for i in 0..4 {
                out.rotation[i] = track.rotation[i].mul_add(signed, out.rotation[i]);
            }
        }
    }
    for out in output.iter_mut().flatten() {
        let length = paired_dot(out.rotation, out.rotation);
        ensure!(length.is_finite(), "Echo quaternion overflow");
        if length < f32::EPSILON {
            out.rotation = [0., 0., 0., 1.];
        } else {
            let reciprocal = estimate(length);
            let correction = (-length).mul_add(reciprocal * reciprocal, 1.);
            let factor = correction.mul_add(0.5 * reciprocal, reciprocal);
            for v in &mut out.rotation {
                *v *= factor;
            }
        }
        // The FFI exposes three scale lanes; fourth-lane parity remains part
        // of the original echo-pose capture validation.
        let length = out.scale[2].mul_add(out.scale[2], out.scale[0] * out.scale[0])
            + out.scale[1] * out.scale[1];
        if length < f32::EPSILON {
            out.scale = [1.; 3];
        }
        ensure!(
            out.translation
                .iter()
                .chain(&out.scale)
                .all(|v| v.is_finite()),
            "Echo pose overflow"
        );
    }
    Ok(output)
}

/// Weight belongs to the second argument. Keep multiplication/FMA ordering
/// from the paired-single implementation rather than using a vector slerp.
pub fn transform(a: &FfiTransform, b: &FfiTransform, weight: f32) -> Result<FfiTransform> {
    ensure!(
        weight.is_finite()
            && [a, b].into_iter().all(|t| t
                .translation
                .iter()
                .chain(&t.rotation)
                .chain(&t.scale)
                .all(|v| v.is_finite())),
        "Non-finite transform blend input"
    );
    let complement = 1. - weight;
    let blend3 =
        |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| weight.mul_add(b[i], complement * a[i]));
    let sign = if paired_dot(a.rotation, b.rotation) < 0. {
        -1.
    } else {
        1.
    };
    let mut rotation =
        std::array::from_fn(|i| weight.mul_add(b.rotation[i] * sign, complement * a.rotation[i]));
    let length = paired_dot(rotation, rotation);
    ensure!(length.is_finite(), "Quaternion blend overflow");
    if length < f32::EPSILON {
        rotation = [0., 0., 0., 1.];
    } else {
        let reciprocal = estimate(length);
        let correction = (-length).mul_add(reciprocal * reciprocal, 1.);
        let factor = correction.mul_add(0.5 * reciprocal, reciprocal);
        for v in &mut rotation {
            *v *= factor;
        }
    }
    let output = FfiTransform {
        translation: blend3(a.translation, b.translation),
        rotation,
        scale: blend3(a.scale, b.scale),
    };
    ensure!(
        output
            .translation
            .iter()
            .chain(&output.scale)
            .all(|v| v.is_finite()),
        "Transform blend overflow"
    );
    Ok(output)
}

/// 0x802046ac: a track present in only one input is copied at full weight;
/// blending it with a reference bone here would change partial-pose semantics.
pub fn tracks(
    a: &[Option<FfiTransform>],
    b: &[Option<FfiTransform>],
    weight: f32,
) -> Result<Vec<Option<FfiTransform>>> {
    ensure!(
        a.len() == b.len() && weight.is_finite(),
        "Incompatible pose blend inputs"
    );
    a.iter()
        .zip(b)
        .map(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => transform(a, b, weight).map(Some),
            (Some(a), None) => Ok(Some(*a)),
            (None, Some(b)) => Ok(Some(*b)),
            (None, None) => Ok(None),
        })
        .collect()
}

/// Final reference-pose fill after transform composition, before skinning.
pub fn resolve(
    rig: &epic_mickey_lib_rs::hkx_file::HkxSkeleton,
    tracks: &[Option<FfiTransform>],
) -> Result<Vec<epic_mickey_lib_rs::hkx_file::HkxBone>> {
    ensure!(
        rig.bones.len() == tracks.len(),
        "Pose/skeleton size differs"
    );
    let mut bones = rig.bones.clone();
    for (bone, track) in bones.iter_mut().zip(tracks) {
        if let Some(t) = track {
            bone.local_translation = t.translation;
            bone.local_rotation = t.rotation;
            bone.local_scale = t.scale;
        }
    }
    Ok(bones)
}

#[cfg(test)]
mod tests {
    #[test]
    fn weighted_tracks_normalize_per_present_bone_and_align_quaternions() -> anyhow::Result<()> {
        use super::*;
        let t = |x, q| FfiTransform {
            translation: [x, 0., 0.],
            rotation: q,
            scale: [1.; 3],
        };
        let a = vec![
            Some(t(2., [0., 0., 0., 1.])),
            Some(t(7., [0., 0., 0., 1.])),
            None,
        ];
        let b = vec![Some(t(8., [0., 0., 0., -1.])), None, None];
        let c = vec![Some(t(14., [0., 0., 0., 1.])), None, None];
        let output = weighted_tracks(&[(&a, 2.), (&b, 1.), (&c, 1.)])?;
        assert_eq!(output[0].unwrap().translation, [6.5, 0., 0.]);
        assert!((output[0].unwrap().rotation[3] - 1.).abs() < 0.000001);
        assert_eq!(output[1].unwrap().translation, [7., 0., 0.]);
        assert_eq!(output[1].unwrap().scale, [1.; 3]);
        assert!(output[2].is_none());
        assert!(weighted_tracks(&[(&a, 0.)])?.iter().all(Option::is_none));
        assert!(weighted_tracks(&[(&a, -1.)]).is_err());
        Ok(())
    }
    use super::*;
    #[test]
    fn echoes_accumulate_before_normalizing_and_keep_missing_tracks() -> Result<()> {
        let result = echo_tracks([
            (vec![Some(pose([0., 0., 0., 1.], 2.)), None], 0.25),
            (vec![Some(pose([0., 0., 0., -1.], 6.)), None], 0.5),
            (vec![Some(pose([0., 0., 0., 1.], 10.)), None], 0.25),
        ])?;
        let output = result[0].unwrap();
        assert_eq!(output.translation, [6., 0., 0.]);
        assert_eq!(output.scale, [1.; 3]);
        assert!(output.rotation[3] > 0.9999998);
        assert!(result[1].is_none());
        Ok(())
    }
    fn pose(q: [f32; 4], x: f32) -> FfiTransform {
        FfiTransform {
            translation: [x, 0., 0.],
            rotation: q,
            scale: [1.; 3],
        }
    }
    #[test]
    fn dense_pose_matches_retail_capture_bit_for_bit() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/pose_blend.json"
        ))?;
        let decode = |v: &serde_json::Value| -> FfiTransform {
            let f = |i: usize| f32::from_bits(v[i].as_u64().unwrap() as u32);
            FfiTransform {
                translation: [f(0), f(1), f(2)],
                rotation: [f(4), f(5), f(6), f(7)],
                scale: [f(8), f(9), f(10)],
            }
        };
        for (sample_index, sample) in fixture["samples"].as_array().unwrap().iter().enumerate() {
            let weight = sample["weight"].as_f64().unwrap() as f32;
            let sources = sample["source"].as_array().unwrap();
            let from: Vec<_> = sources.iter().map(|v| Some(decode(v))).collect();
            let to: Vec<_> = sample["target"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| Some(decode(v)))
                .collect();
            let clock = super::super::transition_timing::BlendingClock {
                time: weight,
                remaining: 1. - weight,
            };
            let attributes = super::super::animation_attributes::Track {
                values: vec![],
                on_fraction: 0.,
            };
            let composed = clock
                .output(
                    &super::super::generator_output::Output {
                        transforms: from,
                        attributes: attributes.clone(),
                    },
                    &super::super::generator_output::Output {
                        transforms: to,
                        attributes,
                    },
                    false,
                    0.,
                    1,
                )?
                .transforms;
            for (bone, a) in sources.iter().enumerate() {
                let result = transform(&decode(a), &decode(&sample["target"][bone]), weight)?;
                let expected = decode(&sample["result"][bone]);
                for (lane, (actual, expected)) in result
                    .translation
                    .iter()
                    .chain(&result.rotation)
                    .chain(&result.scale)
                    .zip(
                        expected
                            .translation
                            .iter()
                            .chain(&expected.rotation)
                            .chain(&expected.scale),
                    )
                    .enumerate()
                {
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "sample {sample_index}, bone {bone}, lane {lane}: {actual} vs {expected}"
                    );
                }
                let output = composed[bone].unwrap();
                assert_eq!(
                    output.translation.map(f32::to_bits),
                    expected.translation.map(f32::to_bits)
                );
                assert_eq!(
                    output.rotation.map(f32::to_bits),
                    expected.rotation.map(f32::to_bits)
                );
                assert_eq!(
                    output.scale.map(f32::to_bits),
                    expected.scale.map(f32::to_bits)
                );
            }
        }
        Ok(())
    }
    #[test]
    fn hemisphere_and_sparse_tracks_preserve_semantics() -> Result<()> {
        let a = pose([0., 0., 0., 1.], 2.);
        let b = pose([0., 0., 0., -1.], 6.);
        let blended = transform(&a, &b, 0.25)?;
        assert_eq!(blended.translation, [3., 0., 0.]);
        assert!(blended.rotation[3] > 0.9999998);
        let result = tracks(
            &[Some(a), None, Some(a), None],
            &[Some(b), Some(b), None, None],
            0.25,
        )?;
        assert_eq!(result[1].unwrap().translation, b.translation);
        assert_eq!(result[2].unwrap().translation, a.translation);
        assert!(result[3].is_none());
        assert_eq!(
            transform(&pose([0.; 4], 0.), &pose([0.; 4], 0.), 0.5)?.rotation,
            [0., 0., 0., 1.]
        );
        Ok(())
    }
}
