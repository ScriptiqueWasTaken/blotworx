//! Retail camera attribute leaves. Full controller initialization, attributes
//! from entities and ClearAllCameraAttributesTimed orchestration remain separate.
//! This is native controller storage, never a change to the game's asset format.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setter {
    Overrides,
    Springs,
    Offsets,
    Lookahead,
    Ledge,
    AutoCenter,
    AutoLevel,
    JumpPitch,
    TargetScalar,
}

fn write_float(record: &mut [u8], offset: usize, value: f32) {
    record[offset..offset + 4].copy_from_slice(&value.to_bits().to_be_bytes());
}
fn conditional_float(record: &mut [u8], offset: usize, value: f32) {
    // fcmpu followed by ble: -FLT_MAX means leave unchanged. +FLT_MAX is
    // a stored default-selection sentinel, not a number to clamp to a range.
    if value <= -f32::MAX {
        return;
    }
    write_float(record, offset, value);
}
fn float(record: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32::from_be_bytes(
        record[offset..offset + 4].try_into().unwrap(),
    ))
}
/// d6c0c and its inlined copies at e2680. An equal coefficient preserves
/// the cached damping and does not rewrite even the coefficient's sign bit.
pub(super) fn coefficient(record: &mut [u8], offset: usize, value: f32) {
    if float(record, offset) == value {
        return;
    }
    write_float(record, offset, value);
    // Native negative-coefficient fsel loads DOL806a9700 = +infinity.
    let root = if value < 0. {
        f32::INFINITY
    } else if value == 0. {
        value
    } else {
        let estimate = super::pose_blend::estimate(value);
        let correction = (-value).mul_add(estimate * estimate, 1.);
        let refined = correction.mul_add(0.5 * estimate, estimate);
        // Native fsel replaces negative/unordered refinement with zero.
        value * if refined >= 0. { refined } else { 0. }
    };
    write_float(record, offset + 4, 2. * root);
}
/// d27d8..d2894: initialize ONLY the observed motion-storage segment of an
/// existing native allocation. Previous coefficients are inputs, because
/// d6c0c preserves cached damping when the incoming value is unchanged.
/// This is not a fabricated complete gameplay-controller constructor.
pub fn initialize_motion_storage(record: &mut [u8]) -> Result<()> {
    ensure!(record.len() >= 0x62c, "Camera motion owner is too short");
    for offset in (0x5a0..0x5c0).step_by(4) {
        write_float(record, offset, 0.);
    }
    for offset in (0x5c0..0x5e0).step_by(4) {
        write_float(record, offset, f32::MAX);
    }
    for offset in (0x5e0..0x5f0).step_by(4) {
        write_float(record, offset, 0.);
    }
    for offset in [0x5f0, 0x5fc, 0x608, 0x614, 0x620] {
        write_float(record, offset + 8, 0.);
        coefficient(record, offset, f32::MAX);
    }
    Ok(())
}
/// Update caller-owned native controller storage, preserving every other byte.
/// The caller must supply actual initialized controller state; this module
/// deliberately does not fabricate a zero-filled gameplay camera constructor.
pub fn apply(record: &mut [u8], setter: Setter, values: [f32; 5], selector: u8) -> Result<()> {
    ensure!(
        record.len() >= 0x751,
        "Camera attribute owner is shorter than its native fields"
    );
    match setter {
        Setter::Overrides => {
            for (lane, offset) in [0x5c0, 0x5c4, 0x5c8, 0x5d8, 0x5d4].into_iter().enumerate() {
                conditional_float(record, offset, values[lane]);
                if lane == 1 && !(values[lane] <= -f32::MAX) {
                    for offset in [0x390, 0x43c, 0x5f8] {
                        write_float(record, offset, 0.);
                    }
                }
            }
        }
        Setter::Springs => {
            ensure!(
                values.iter().all(|v| v.is_finite()),
                "Nonfinite camera spring coefficients require native special-value coverage"
            );
            // Lane order differs from memory order. Velocity is deliberately
            // untouched; this setter updates only coefficient/damping pairs.
            for (lane, offset) in [0x608, 0x5f0, 0x5fc, 0x620, 0x614].into_iter().enumerate() {
                if !(values[lane] <= -f32::MAX) {
                    coefficient(record, offset, values[lane]);
                }
            }
        }
        Setter::Offsets => {
            for lane in 0..3 {
                conditional_float(record, 0x5e0 + lane * 4, values[lane]);
            }
        }
        Setter::Lookahead => {
            record[0x6e8] = selector;
            conditional_float(record, 0x70c, values[0]);
            conditional_float(record, 0x710, values[1]);
        }
        Setter::Ledge => {
            conditional_float(record, 0x6ac, values[0]);
            conditional_float(record, 0x6b0, values[1]);
        }
        Setter::AutoCenter => {
            record[0x714] = selector;
            conditional_float(record, 0x728, values[0]);
        }
        Setter::AutoLevel => record[0x738] = selector,
        Setter::JumpPitch => record[0x750] = selector,
        Setter::TargetScalar => {
            conditional_float(record, if selector != 0 { 0x214 } else { 0x234 }, values[0])
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribute_leaves_match_original_instruction_writes() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_attributes.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let setter = match case["function"].as_str().unwrap() {
                "overrides" => Setter::Overrides,
                "offsets" => Setter::Offsets,
                "lookahead" => Setter::Lookahead,
                "ledge" => Setter::Ledge,
                "autocenter" => Setter::AutoCenter,
                "autolevel" => Setter::AutoLevel,
                "jump_pitch" => Setter::JumpPitch,
                "target_scalar" => Setter::TargetScalar,
                _ => panic!("Unknown reference setter"),
            };
            let mut expected = [0xa5; 0x800];
            for pair in case["changed"].as_array().unwrap() {
                expected[pair[0].as_u64().unwrap() as usize] = pair[1].as_u64().unwrap() as u8;
            }
            let mut actual = [0xa5; 0x800];
            apply(
                &mut actual,
                setter,
                serde_json::from_value(case["values"].clone())?,
                case["selector"].as_u64().unwrap() as u8,
            )?;
            let differences: Vec<_> = actual
                .iter()
                .zip(expected)
                .enumerate()
                .filter(|(_, (a, b))| **a != *b)
                .map(|(i, (a, b))| (i, *a, b))
                .collect();
            assert!(
                differences.is_empty(),
                "Differences {differences:?}; {case}"
            );
        }
        Ok(())
    }
    #[test]
    fn short_owner_is_rejected_before_any_partial_write() {
        let mut bytes = [0x37; 0x750];
        assert!(apply(&mut bytes, Setter::Overrides, [1.; 5], 1).is_err());
        assert!(bytes.iter().all(|b| *b == 0x37));
    }
    #[test]
    fn motion_construction_and_springs_match_original_instruction_writes() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_springs.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut actual = [0xa5; 0x800];
            for offset in [0x5f0, 0x5fc, 0x608, 0x614, 0x620] {
                write_float(
                    &mut actual,
                    offset,
                    case["previous"].as_f64().unwrap() as f32,
                );
                write_float(&mut actual, offset + 4, 9.);
                write_float(&mut actual, offset + 8, 7.);
            }
            let mut expected = actual;
            for pair in case["changed"].as_array().unwrap() {
                expected[pair[0].as_u64().unwrap() as usize] = pair[1].as_u64().unwrap() as u8;
            }
            match case["kind"].as_str().unwrap() {
                "construction_segment" => initialize_motion_storage(&mut actual)?,
                "setters" => apply(
                    &mut actual,
                    Setter::Springs,
                    serde_json::from_value(case["values"].clone())?,
                    0,
                )?,
                _ => panic!("Unknown camera spring reference"),
            }
            assert_eq!(actual, expected, "{case}");
        }
        Ok(())
    }
}
