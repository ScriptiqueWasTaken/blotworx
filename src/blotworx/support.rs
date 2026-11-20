//! Retail rigid-body support classification, DOL 0x802f60c8..0x802f622c.
//! Inputs are velocities from the downward support solver, not the player's
//! movement velocity. Contact collection and the Havok solver are separate.
use super::physics::V;

// Paired-single ps_mul, ps_madd and ps_sum0: (z*z + x*x) + y*y.
fn dot(a: V, b: V) -> f32 {
    a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    Unsupported = 0,
    Sliding = 1,
    Supported = 2,
}

/// EM1 construction-info+0x60 is initialized to 0.1 at REL 0x379d28,
/// copied to rigid-body+0x3c, then divided by dt at REL 0x23a6e0.
pub fn probe_velocity(up: V, dt: f32) -> V {
    let speed = -0.1f32 / dt;
    up.map(|axis| axis * speed)
}

/// Preserve the retail comparisons and single-precision operation order.
/// The native constant at 0x8067d088 is 0.001 for both component differences
/// and the squared-result-length test; these are deliberately different units.
pub fn classify(requested: V, solved: V, up: V, slope_cos: f32) -> State {
    if (0..3).all(|i| (solved[i] - requested[i]).abs() <= 0.001) {
        return State::Unsupported;
    }
    let length_squared = dot(solved, solved);
    if length_squared < 0.001 {
        return State::Supported;
    }
    let vertical = dot(solved, up);
    let lateral_squared = (-vertical).mul_add(vertical, length_squared);
    let threshold = slope_cos * (length_squared * slope_cos);
    if lateral_squared < threshold {
        State::Sliding
    } else {
        State::Supported
    }
}

/// EM1's override (REL 0x23a944..0x23aa38) retains only solver-active
/// contacts whose upward dot is strictly greater than 0.08. A candidate
/// support result without any such contact is changed to Unsupported.
/// Callers must supply solver activity, not merely geometric proximity.
pub fn retain_active_support(
    candidate: State,
    up: V,
    contacts: impl IntoIterator<Item = (V, bool)>,
) -> State {
    if candidate == State::Unsupported {
        return candidate;
    }
    if contacts
        .into_iter()
        .any(|(normal, active)| active && dot(normal, up) > 0.08)
    {
        candidate
    } else {
        State::Unsupported
    }
}

/// Input from an already filtered, solver-active support contact. Motion type
/// retains the original Havok byte: 4 contributes unconditionally, 5 is fixed,
/// and other types contribute only when the caller requests dynamic velocity.
#[derive(Clone, Copy)]
pub struct SurfaceContact {
    pub normal: V,
    pub distance: f32,
    pub position: V,
    pub center_of_mass: V,
    pub linear_velocity: V,
    pub angular_velocity: V,
    pub motion_type: u8,
}
#[derive(Debug)]
pub struct Surface {
    pub normal: V,
    pub normal_w: f32,
    pub velocity: V,
    pub distance: f32,
    pub dynamic: bool,
}
/// DOL 0x802f668c..0x802f69e4, inherited by EM1's rigid-body vtable
/// (REL relocation 0x4e8ea8). Contact collection and the caller's
/// dynamic-velocity policy are intentionally separate.
pub fn aggregate_surface(
    contacts: &[SurfaceContact],
    include_dynamic_velocity: bool,
    distance_excess: f32,
) -> Option<Surface> {
    aggregate_surface_iter(
        contacts.iter().copied(),
        include_dynamic_velocity,
        distance_excess,
    )
}
/// Streaming form used by collision queries without allocating a contact copy.
pub fn aggregate_surface_iter(
    contacts: impl Iterator<Item = SurfaceContact>,
    include_dynamic_velocity: bool,
    distance_excess: f32,
) -> Option<Surface> {
    let mut count = 0usize;
    let mut normal = [0.; 3];
    let mut velocity = [0.; 3];
    let mut distance = 0.;
    let mut dynamic = false;
    for contact in contacts {
        count += 1;
        for i in 0..3 {
            normal[i] += contact.normal[i];
        }
        distance += contact.distance;
        if contact.motion_type != 4 && contact.motion_type != 5 {
            dynamic = true;
        }
        if contact.motion_type == 4 || (contact.motion_type != 5 && include_dynamic_velocity) {
            let arm =
                std::array::from_fn::<_, 3, _>(|i| contact.position[i] - contact.center_of_mass[i]);
            let omega = contact.angular_velocity;
            let point = [
                omega[1].mul_add(arm[2], -omega[2] * arm[1]),
                omega[2].mul_add(arm[0], -omega[0] * arm[2]),
                omega[0].mul_add(arm[1], -omega[1] * arm[0]),
            ];
            for i in 0..3 {
                velocity[i] += point[i] + contact.linear_velocity[i];
            }
        }
    }
    if count == 0 {
        return None;
    }
    let reciprocal = 1. / count as f32;
    let normalized = normalize_surface_sum([normal[0], normal[1], normal[2], distance]);
    Some(Surface {
        normal: normalized[..3].try_into().unwrap(),
        normal_w: normalized[3],
        velocity: velocity.map(|x| x * reciprocal),
        distance: if dynamic {
            0.01
        } else {
            distance * reciprocal - distance_excess
        },
        dynamic,
    })
}
///802f6910..69c4: XYZ length, one single-refined reciprocal root and
/// four-lane multiply. W retains the normalized contact-distance sum.
fn normalize_surface_sum(value: [f32; 4]) -> [f32; 4] {
    let squared = value[2].mul_add(value[2], value[0] * value[0]) + value[1] * value[1];
    let estimate = if squared == 0. {
        f32::INFINITY
    } else if squared.is_infinite() {
        0.
    } else if squared.is_nan() {
        f32::NAN
    } else {
        super::pose_blend::estimate(squared)
    };
    let correction = (-squared).mul_add(estimate * estimate, 1.);
    let refined = correction.mul_add(0.5 * estimate, estimate);
    let inverse = if refined >= 0. { refined } else { 0. };
    value.map(|v| v * inverse)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_normalization_matches_original_four_lane_tail() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/motion_surface.json"
        ))?;
        for case in fixture["normalization"].as_array().unwrap() {
            let value = std::array::from_fn(|i| {
                f32::from_bits(case["sum_bits"][i].as_u64().unwrap() as u32)
            });
            assert_eq!(
                serde_json::json!(normalize_surface_sum(value).map(f32::to_bits)),
                case["normal_bits"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn surface_combines_contacts_and_angular_point_velocity() {
        let fixed = SurfaceContact {
            normal: [0., 1., 0.],
            distance: 0.2,
            position: [1., 0., 0.],
            center_of_mass: [0.; 3],
            linear_velocity: [2., 0., 0.],
            angular_velocity: [0., 0., 3.],
            motion_type: 5,
        };
        let moving = SurfaceContact {
            normal: [0.6, 0.8, 0.],
            motion_type: 4,
            ..fixed
        };
        let result = aggregate_surface(&[fixed, moving], false, 0.05).unwrap();
        assert_eq!(result.velocity, [1., 1.5, 0.]);
        assert!((result.normal[0] - 0.31622776).abs() < 1e-6);
        assert!((result.distance - 0.15).abs() < 1e-6);
        assert!(!result.dynamic);
        let dynamic = SurfaceContact {
            motion_type: 1,
            ..moving
        };
        let disabled = aggregate_surface(&[fixed, dynamic], false, 0.05).unwrap();
        assert_eq!(disabled.velocity, [0.; 3]);
        assert_eq!(disabled.distance, 0.01);
        assert!(disabled.dynamic);
        assert_eq!(
            aggregate_surface(&[fixed, dynamic], true, 0.05)
                .unwrap()
                .velocity,
            [1., 1.5, 0.]
        );
        assert!(aggregate_surface(&[], true, 0.).is_none());
    }
    #[test]
    fn support_probe_distinguishes_free_motion_floor_and_steep_slide() {
        let down = [0., -3., 0.];
        let up = [0., 1., 0.];
        assert_eq!(classify(down, down, up, 0.8), State::Unsupported);
        assert_eq!(classify(down, [0.; 3], up, 0.8), State::Supported);
        // Projection of downward motion onto a 45-degree slope.
        assert_eq!(classify(down, [1.5, -1.5, 0.], up, 0.8), State::Sliding);
        assert_eq!(classify(down, [1.5, -1.5, 0.], up, 0.6), State::Supported);
    }
    #[test]
    fn component_tolerance_precedes_result_length_test() {
        let up = [0., 1., 0.];
        assert_eq!(classify([0.; 3], [0.001; 3], up, 0.8), State::Unsupported);
        let next = f32::from_bits(0.001f32.to_bits() + 1);
        assert_eq!(classify([0.; 3], [next, 0., 0.], up, 0.8), State::Supported);
        // Comparison is per component, not the length of the difference.
        assert_eq!(classify([0.; 3], [0.0009; 3], up, 0.8), State::Unsupported);
    }
    #[test]
    fn em1_requires_an_active_upward_contact() {
        let up = [0., 1., 0.];
        for candidate in [State::Sliding, State::Supported] {
            assert_eq!(retain_active_support(candidate, up, []), State::Unsupported);
            assert_eq!(
                retain_active_support(candidate, up, [(up, false)]),
                State::Unsupported
            );
            assert_eq!(
                retain_active_support(candidate, up, [([1., 0.08, 0.], true)]),
                State::Unsupported
            );
            let above = f32::from_bits(0.08f32.to_bits() + 1);
            assert_eq!(
                retain_active_support(candidate, up, [([1., above, 0.], true)]),
                candidate
            );
        }
        assert_eq!(
            retain_active_support(State::Unsupported, up, [(up, true)]),
            State::Unsupported
        );
    }
    #[test]
    fn probe_uses_constant_distance_per_step_not_character_speed() {
        for dt in [1. / 30., 1. / 60.] {
            let probe = probe_velocity([0., 1., 0.], dt);
            assert_eq!(probe[0], 0.);
            assert!((probe[1] * dt + 0.1).abs() < 1e-7);
            assert_eq!(probe[2], 0.);
        }
    }
}
