//! Verified front end of Basic AI Controller relocation/input updates.
//! Angular/magnitude smoothing and full controller lifecycle remain incomplete.

/// REL3001fc calls DOL805aea00 (squared XYZ length), then checks inclusive
/// bounds from REL4c6b18/1c. W is not part of this test.
pub fn valid_facing(value: [f32; 4]) -> bool {
    let xy = value[0] * value[0];
    let yy = value[1] * value[1];
    let squared = value[2].mul_add(value[2], xy) + yy;
    squared >= 0.9f32 && squared <= 1.1f32
}

#[derive(Debug, PartialEq)]
pub struct Prepared {
    /// Stored at controller +7c/+80 regardless of the effective-input gate.
    pub stored: [f32; 2],
    /// Stack +48/+4c: zeroed when controller +214's low two bits are not 3.
    pub effective: [f32; 2],
    /// Controller +170 suppresses the authored smoothing mode (+360).
    pub smoothing_mode: u32,
}

/// REL306d84..306e2c. Parameters retain register/field identities because
/// their complete gameplay meanings are not yet established.
pub fn prepare(
    f1: f32,
    f2: f32,
    scale_364: f32,
    flags_214: u32,
    state_170: u32,
    mode_360: u32,
) -> Prepared {
    let stored = [-f1 * scale_364, f2 * scale_364];
    Prepared {
        stored,
        effective: if flags_214 & 3 == 3 { stored } else { [0.0; 2] },
        smoothing_mode: if state_170 == 0 { mode_360 } else { 0 },
    }
}

/// REL1efed0: a paused owner gets zero; its clock selector chooses the two
/// manager delta fields. Do not substitute render-frame delta here.
pub fn owner_delta(paused_41: u8, clock_40: u8, manager_40: f32, manager_44: f32) -> f32 {
    if paused_41 != 0 {
        0.0
    } else if clock_40 != 0 {
        manager_44
    } else {
        manager_40
    }
}

/// REL147c44, for the bounded original atan2 domain. Inclusive endpoints
/// retain their sign; do not substitute remainder or clamp.
fn angle_difference(target: f32, current: f32) -> f32 {
    let mut difference = target - current;
    while difference > std::f32::consts::PI {
        difference -= std::f32::consts::TAU;
    }
    while difference < -std::f32::consts::PI {
        difference += std::f32::consts::TAU;
    }
    difference
}

/// REL306efc..307140: native polar filter, including separate single-precision
/// normalization, angle-rate and magnitude-rate operations.
pub fn polar(
    current: [f32; 2],
    target: [f32; 2],
    property_260: f32,
    property_268: f32,
    delta: f32,
) -> mlua::Result<[f32; 2]> {
    let length = |v: [f32; 2]| super::lua_vector::Vector4([v[0], 0.0, v[1], 0.0]).length2();
    let old_length = length(current)?;
    let new_length = length(target)?;
    let normalize = |v: [f32; 2], length: f32| {
        let inverse = 1.0f32 / length;
        v.map(|x| x * inverse)
    };
    let direction = if new_length == 0.0 && old_length == 0.0 {
        [0.0; 2]
    } else if new_length == 0.0 {
        normalize(current, old_length)
    } else if old_length == 0.0 {
        normalize(target, new_length)
    } else {
        let from = normalize(current, old_length);
        let to = normalize(target, new_length);
        let old_angle = super::native_angle::atan2(from[1] as f64, from[0] as f64) as f32;
        let new_angle = super::native_angle::atan2(to[1] as f64, to[0] as f64) as f32;
        let difference = angle_difference(new_angle, old_angle);
        let sign = if difference > 0.0 {
            1.0
        } else if difference < 0.0 {
            -1.0
        } else {
            0.0
        };
        let mut step = (property_268 * delta) * sign;
        if step.abs() > difference.abs() {
            step = difference;
        }
        let angle = old_angle + step;
        let (sin, cos) =
            super::native_angle::sin_cos_medium(angle as f64).map_err(mlua::Error::external)?;
        [cos as f32, sin as f32]
    };
    let difference = new_length - old_length;
    let sign = if difference > 0.0 {
        1.0
    } else if difference < 0.0 {
        -1.0
    } else {
        0.0
    };
    let mut step = (property_260 * delta) * sign;
    if step.abs() > difference.abs() {
        step = difference;
    }
    let magnitude = old_length + step;
    Ok(direction.map(|v| v * magnitude))
}

/// REL306e34..306ef8: mode 1 bounds the magnitude of the 2D change by
/// property(+258) * owner delta, using the original sqrt arithmetic.
pub fn smooth(
    prepared: &Prepared,
    current: [f32; 2],
    property_258: f32,
    owner_delta: f32,
) -> mlua::Result<[f32; 2]> {
    match prepared.smoothing_mode {
        1 => {
            let difference = [
                prepared.effective[0] - current[0],
                prepared.effective[1] - current[1],
            ];
            let distance =
                super::lua_vector::Vector4([difference[0], 0.0, difference[1], 0.0]).length2()?;
            let mut direction = difference;
            if distance != 0.0 {
                let inverse = 1.0f32 / distance;
                direction = direction.map(|v| v * inverse);
            }
            let mut step = property_258 * owner_delta;
            if step > distance {
                step = distance;
            }
            Ok(std::array::from_fn(|i| current[i] + direction[i] * step))
        }
        2 => Err(mlua::Error::external(
            "Original AI angular/magnitude smoothing is not implemented",
        )),
        _ => Ok(prepared.effective),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_polar_filter_preserves_zero_cases_and_wrap_endpoints() -> mlua::Result<()> {
        assert_eq!(
            angle_difference(std::f32::consts::PI, 0.0),
            std::f32::consts::PI
        );
        assert_eq!(
            angle_difference(-std::f32::consts::PI, 0.0),
            -std::f32::consts::PI
        );
        assert_eq!(polar([0.0; 2], [0.0; 2], 1.0, 1.0, 0.1)?, [0.0; 2]);
        let start = polar([0.0; 2], [0.0, 1.0], 3.0, 0.0, 0.1)?;
        assert!((start[1] - 0.3).abs() < 0.000001 && start[0] == 0.0);
        let turn = polar([1.0, 0.0], [0.0, 0.5], 30.0, 4.5, 0.1)?;
        assert!((turn[0] - 0.45022354).abs() < 0.000001);
        assert!((turn[1] - 0.21748277).abs() < 0.000001);
        let release = polar([0.0, 1.0], [0.0; 2], 3.0, 4.5, 0.1)?;
        assert!((release[1] - 0.7).abs() < 0.000001 && release[0] == 0.0);
        Ok(())
    }
    #[test]
    fn owner_clock_pause_and_linear_smoothing_follow_native_updates() -> mlua::Result<()> {
        assert_eq!(owner_delta(1, 1, 0.25, 0.5), 0.0);
        assert_eq!(owner_delta(0, 0, 0.25, 0.5), 0.25);
        assert_eq!(owner_delta(0, 1, 0.25, 0.5), 0.5);
        let request = prepare(-3.0, 4.0, 1.0, 3, 0, 1);
        let moving = smooth(&request, [0.0; 2], 2.0, 0.5)?;
        assert!((moving[0] - 0.6).abs() < 0.000001);
        assert!((moving[1] - 0.8).abs() < 0.000001);
        assert_eq!(smooth(&request, [0.0; 2], 2.0, 0.0)?, [0.0; 2]);
        let snapped = smooth(&request, [0.0; 2], 100.0, 1.0)?;
        assert!((snapped[0] - 3.0).abs() < 0.00001 && (snapped[1] - 4.0).abs() < 0.00001);
        assert_eq!(smooth(&request, [3.0, 4.0], 1.0, 1.0)?, [3.0, 4.0]);
        let negative = smooth(&request, [0.0; 2], -2.0, 0.5)?;
        assert!(negative[0] < 0.0 && negative[1] < 0.0);
        assert!(smooth(&prepare(0.0, 1.0, 1.0, 3, 0, 2), [0.0; 2], 1.0, 1.0).is_err());
        Ok(())
    }
    #[test]
    fn facing_checks_squared_xyz_and_ignores_w() {
        assert!(valid_facing([1.0, 0.0, 0.0, f32::NAN]));
        assert!(!valid_facing([0.0; 4]));
        assert!(!valid_facing([0.8, 0.0, 0.0, 0.0]));
        assert!(!valid_facing([1.2, 0.0, 0.0, 0.0]));
        assert!(!valid_facing([f32::NAN, 0.0, 0.0, 0.0]));
        assert!(!valid_facing([f32::INFINITY, 0.0, 0.0, 0.0]));
    }
    #[test]
    fn request_storage_survives_effective_input_gate() {
        let gated = prepare(0.25, 0.5, 2.0, 0x102, 0, 2);
        assert_eq!(
            gated,
            Prepared {
                stored: [-0.5, 1.0],
                effective: [0.0; 2],
                smoothing_mode: 2
            }
        );
        let active = prepare(0.25, 0.5, 2.0, 0x103, 1, 2);
        assert_eq!(
            active,
            Prepared {
                stored: [-0.5, 1.0],
                effective: [-0.5, 1.0],
                smoothing_mode: 0
            }
        );
    }
}
