//! EM1 DOL 0x802ba898 motion-state step. The batch solver, activation and
//! scheduling remain separate. Limits are decoded hkUFloat8 table values;
//! the angular limit must not be replaced with a host radians/sec conversion.
use super::native_hk_transform::{inverse_root, set_rotation, Transform};
use anyhow::{ensure, Result};

#[derive(Clone, Debug)]
pub struct MotionState {
    pub transform: Transform,
    pub previous_center: [f32; 4],
    pub center: [f32; 4],
    pub previous_orientation: [f32; 4],
    pub orientation: [f32; 4],
    pub local_center: [f32; 4],
    pub delta_angle: [f32; 4],
}
impl MotionState {
    /// DOL802bec9c -> 8014d060/801660b4/8014db00. Unlike rigid-body
    /// placement this uses the owner's unscaled pose alone and the raw
    /// quaternion conversion, without composing the serialized body pose.
    /// DOL80659fa8 and8065d8c8 provide zero swept/local-center lanes.
    pub fn shape_phantom_initial(rotation: [f32; 9], position: [f32; 3]) -> Result<Self> {
        ensure!(
            rotation
                .iter()
                .chain(position.iter())
                .all(|v| v.is_finite()),
            "Invalid shape phantom owner pose"
        );
        let mut transform = super::native_hk_transform::from_entity(rotation, position);
        let orientation = super::native_hk_transform::rotation_quaternion_raw(&transform)?;
        set_rotation(&mut transform, orientation);
        let center = super::native_hk_transform::entity_position_vector(position);
        Ok(Self {
            transform,
            previous_center: center,
            center,
            previous_orientation: orientation,
            orientation,
            local_center: [0.; 4],
            delta_angle: [0.; 4],
        })
    }
    /// DOL 8014eaa8, motion virtual +0x34. Shift both swept endpoints using
    /// the CURRENT rotation, not each endpoint's quaternion. Preserve pose,
    /// orientations and delta angle. Body shape/radius refresh is separate.
    pub fn set_local_center(&mut self, local: [f32; 4]) -> Result<()> {
        ensure!(
            local
                .iter()
                .chain(self.local_center.iter())
                .chain(self.transform.iter())
                .chain(self.previous_center.iter())
                .chain(self.center.iter())
                .all(|v| v.is_finite()),
            "Invalid center-of-mass update input"
        );
        let delta: [f32; 3] = std::array::from_fn(|i| local[i] - self.local_center[i]);
        let offset: [f32; 4] = std::array::from_fn(|i| {
            if i == 3 {
                0.
            } else {
                self.transform[8 + i].mul_add(
                    delta[2],
                    self.transform[4 + i].mul_add(delta[1], self.transform[i] * delta[0]),
                )
            }
        });
        let previous: [f32; 4] = std::array::from_fn(|i| self.previous_center[i] + offset[i]);
        let current: [f32; 4] = std::array::from_fn(|i| self.center[i] + offset[i]);
        ensure!(
            previous.iter().chain(current.iter()).all(|v| v.is_finite()),
            "Center-of-mass update overflow"
        );
        self.local_center = local;
        self.previous_center = previous;
        self.center = current;
        Ok(())
    }
    /// DOL 0x8014e750, reached from motion's setTransform slot +0x44.
    /// Warp both endpoints to the new pose, preserve the previous base time,
    /// clear inverse delta and delta angle. It neither zeros velocities nor
    /// normalizes the converted quaternion a second time.
    pub fn set_transform(&mut self, transform: Transform) -> Result<()> {
        ensure!(
            self.local_center.iter().all(|v| v.is_finite()) && self.previous_center[3].is_finite(),
            "Invalid motion-state warp input"
        );
        let q = super::native_hk_transform::rotation_quaternion_raw(&transform)?;
        let mut center: [f32; 4] = std::array::from_fn(|i| {
            let rotated = transform[8 + i].mul_add(
                self.local_center[2],
                transform[4 + i].mul_add(self.local_center[1], transform[i] * self.local_center[0]),
            );
            transform[12 + i] + rotated
        });
        ensure!(
            center.iter().all(|v| v.is_finite()),
            "Nonfinite motion-state warp result"
        );
        center[3] = 0.;
        self.transform = transform;
        self.orientation = q;
        self.previous_orientation = q;
        let base_time = self.previous_center[3];
        self.center = center;
        self.previous_center = center;
        self.previous_center[3] = base_time;
        self.delta_angle = [0.; 4];
        Ok(())
    }
}

/// Velocity limiting from DOL802ba898, shared by the native
/// swept-state reference step and Box3D's pre-position compatibility hook.
/// Angular limits retain the native half-step/scaled-angle calculation.
pub(super) fn limit_velocity(
    linear: [f32; 4],
    angular: [f32; 4],
    delta: f32,
    limits: [f32; 2],
) -> Result<([f32; 4], [f32; 4], [f32; 4], f32)> {
    ensure!(
        delta.is_finite()
            && delta >= 0.
            && limits.iter().all(|v| v.is_finite() && *v >= 0.)
            && linear.iter().chain(angular.iter()).all(|v| v.is_finite()),
        "Invalid velocity limit input"
    );
    let mut lv = linear;
    let mut av = angular;
    let length3 = |v: [f32; 4]| v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    let speed2 = length3(lv);
    if speed2 > limits[0] * limits[0] {
        let scale = limits[0] * inverse_root(speed2)?;
        lv = lv.map(|v| v * scale);
    }
    let half_delta = 0.5 * delta;
    let mut rotation = av.map(|v| v * half_delta);
    let mut angle2 = f32::from_bits(0x3ecf817a) * length3(rotation);
    let max_angle = 0.9f32.min(delta * limits[1]);
    let maximum2 = max_angle * max_angle;
    if angle2 > maximum2 {
        let factor = max_angle * inverse_root(angle2)?;
        rotation = rotation.map(|v| v * factor);
        av = av.map(|v| v * factor);
        angle2 = maximum2;
    }
    ensure!(
        lv.iter()
            .chain(av.iter())
            .chain(rotation.iter())
            .all(|v| v.is_finite())
            && angle2.is_finite(),
        "Nonfinite limited velocity"
    );
    Ok((lv, av, rotation, angle2))
}

/// Step valid, finite motion inputs. The retail corrupt-velocity fallback is
/// not reproduced here: callers must diagnose invalid input explicitly.
/// W lanes are preserved/calculated as in the original paired instructions.
pub fn step(
    state: &mut MotionState,
    linear: &mut [f32; 4],
    angular: &mut [f32; 4],
    start: f32,
    delta: f32,
    inverse_delta: f32,
    limits: [f32; 2],
) -> Result<()> {
    ensure!(
        start.is_finite()
            && delta.is_finite()
            && delta >= 0.
            && inverse_delta.is_finite()
            && inverse_delta >= 0.
            && limits.iter().all(|v| v.is_finite() && *v >= 0.)
            && linear
                .iter()
                .chain(angular.iter())
                .chain(state.center.iter())
                .chain(state.orientation.iter())
                .chain(state.local_center.iter())
                .all(|v| v.is_finite()),
        "Invalid native motion-step input"
    );
    // Stage results so validation failure cannot publish a partially stepped body.
    let mut next = state.clone();
    let mut lv = *linear;
    let mut av = *angular;
    next.previous_center = next.center;
    next.previous_center[3] = start;
    let (limited_linear, limited_angular, mut rotation, angle2) =
        limit_velocity(lv, av, delta, limits)?;
    lv = limited_linear;
    av = limited_angular;
    for i in 0..4 {
        next.center[i] = lv[i].mul_add(delta, next.center[i]);
    }
    next.center[3] = inverse_delta;
    let current = next.orientation;
    next.previous_orientation = current;
    let angle4 = angle2 * angle2;
    let w = -f32::from_bits(0x3f52acb8).mul_add(angle2, -1.);
    let w = -f32::from_bits(0x3e05a965).mul_add(angle4, -w);
    rotation[3] = -angle4.mul_add(f32::from_bits(0x3d35e52a) * angle2, -w);
    let [x, y, z, w] = rotation;
    let [cx, cy, cz, cw] = current;
    let cross = [
        y.mul_add(cz, -(cy * z)),
        -x.mul_add(cz, -(cx * z)),
        -y.mul_add(cx, -(cy * x)),
    ];
    let mut q = [0.; 4];
    for i in 0..3 {
        q[i] = rotation[i].mul_add(cw, current[i].mul_add(w, cross[i]));
    }
    q[3] = w.mul_add(cw, -(z.mul_add(cz, x * cx) + y * cy));
    let squared = q[2].mul_add(q[2], q[0] * q[0]) + q[3].mul_add(q[3], q[1] * q[1]);
    let factor = inverse_root(squared)?;
    next.orientation = q.map(|v| v * factor);
    next.delta_angle = rotation.map(|v| v + v);
    next.delta_angle[3] = if angle2 == 0. {
        0.
    } else {
        (angle2 * inverse_root(angle2)?) * f32::from_bits(0x40490fdb)
    };
    set_rotation(&mut next.transform, next.orientation);
    for i in 0..4 {
        let shift = next.transform[8 + i].mul_add(
            next.local_center[2],
            next.transform[4 + i].mul_add(
                next.local_center[1],
                next.transform[i] * next.local_center[0],
            ),
        );
        next.transform[12 + i] = next.center[i] - shift;
    }
    ensure!(
        next.transform
            .iter()
            .chain(next.orientation.iter())
            .chain(next.delta_angle.iter())
            .all(|v| v.is_finite()),
        "Nonfinite native motion-step result"
    );
    *state = next;
    *linear = lv;
    *angular = av;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_original_placement_matches_every_state_word() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_warp.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["dol_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut distinct = std::collections::HashSet::new();
        for (index, sample) in samples.iter().enumerate() {
            let words = |name: &str| -> Vec<f32> {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect()
            };
            assert!(distinct.insert(sample["transform"].to_string()));
            let raw = words("state");
            assert_eq!(raw.len(), 44);
            let mut state = MotionState {
                transform: raw[..16].try_into().unwrap(),
                previous_center: raw[16..20].try_into().unwrap(),
                center: raw[20..24].try_into().unwrap(),
                previous_orientation: raw[24..28].try_into().unwrap(),
                orientation: raw[28..32].try_into().unwrap(),
                local_center: raw[32..36].try_into().unwrap(),
                delta_angle: raw[36..40].try_into().unwrap(),
            };
            state
                .set_transform(words("transform").try_into().unwrap())
                .unwrap();
            let actual = state
                .transform
                .iter()
                .chain(&state.previous_center)
                .chain(&state.center)
                .chain(&state.previous_orientation)
                .chain(&state.orientation)
                .chain(&state.local_center)
                .chain(&state.delta_angle)
                .chain(&raw[40..])
                .copied()
                .collect::<Vec<_>>();
            let expected = words("result");
            assert_eq!(actual.len(), expected.len());
            for (lane, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(a.to_bits(), b.to_bits(), "placement {index} lane {lane}");
            }
        }
    }
    #[test]
    fn placement_preserves_base_time_and_warps_both_endpoints() {
        let mut state = identity();
        state.previous_center[3] = 7.;
        state.center[3] = 2.;
        state.local_center = [1., 2., 3., 4.];
        state.delta_angle = [9.; 4];
        let transform = super::super::native_hk_transform::from_entity(
            [0., 0., 1., 1., 0., 0., 0., 1., 0.],
            [10., 20., 30.],
        );
        state.set_transform(transform).unwrap();
        assert_eq!(state.center, [13., 21., 32., 0.]);
        assert_eq!(state.previous_center, [13., 21., 32., 7.]);
        assert_eq!(state.transform, transform);
        assert_eq!(state.local_center, [1., 2., 3., 4.]);
        assert_eq!(state.previous_orientation, state.orientation);
        assert_eq!(state.delta_angle, [0.; 4]);
    }
    #[test]
    fn captured_original_steps_match_every_output_bit() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_step.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["dol_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut mismatches = Vec::new();
        let mut distinct = std::collections::HashSet::new();
        for (index, sample) in samples.iter().enumerate() {
            assert!(distinct.insert((sample["linear"].to_string(), sample["angular"].to_string())));
            let words = |name: &str| -> Vec<f32> {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect()
            };
            let raw = words("state");
            let info = words("step");
            let mut state = MotionState {
                transform: raw[..16].try_into().unwrap(),
                previous_center: raw[16..20].try_into().unwrap(),
                center: raw[20..24].try_into().unwrap(),
                previous_orientation: raw[24..28].try_into().unwrap(),
                orientation: raw[28..32].try_into().unwrap(),
                local_center: raw[32..36].try_into().unwrap(),
                delta_angle: raw[36..40].try_into().unwrap(),
            };
            let encoded = raw[43].to_bits();
            let limits = [
                super::super::native_motion_limits::decode((encoded >> 24) as u8),
                super::super::native_motion_limits::decode((encoded >> 16) as u8),
            ];
            assert_eq!(
                limits.map(f32::to_bits).as_slice(),
                words("limits")
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
            let mut linear = words("linear").try_into().unwrap();
            let mut angular = words("angular").try_into().unwrap();
            step(
                &mut state,
                &mut linear,
                &mut angular,
                info[0],
                info[2],
                info[3],
                limits,
            )
            .unwrap();
            let actual = state
                .transform
                .iter()
                .chain(&state.previous_center)
                .chain(&state.center)
                .chain(&state.previous_orientation)
                .chain(&state.orientation)
                .chain(&state.local_center)
                .chain(&state.delta_angle)
                .chain(&raw[40..])
                .copied()
                .collect::<Vec<_>>();
            for (name, values) in [
                ("result", actual),
                ("result_linear", linear.to_vec()),
                ("result_angular", angular.to_vec()),
            ] {
                let expected = words(name);
                assert_eq!(values.len(), expected.len());
                for (lane, (a, b)) in values.iter().zip(expected).enumerate() {
                    if a.to_bits() != b.to_bits() {
                        mismatches.push(format!(
                            "sample {index} {name}[{lane}]: {:08x} != {:08x}",
                            a.to_bits(),
                            b.to_bits()
                        ));
                    }
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }
    fn identity() -> MotionState {
        MotionState {
            transform: super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0.; 3],
            ),
            previous_center: [0.; 4],
            center: [0.; 4],
            previous_orientation: [0., 0., 0., 1.],
            orientation: [0., 0., 0., 1.],
            local_center: [0.; 4],
            delta_angle: [0.; 4],
        }
    }
    #[test]
    fn stationary_rotation_and_linear_limit_preserve_swept_endpoints() {
        let mut state = identity();
        state.center = [10., 20., 30., 0.];
        state.local_center = [1., 2., 3., 0.];
        let mut linear = [10., 0., 0., 0.];
        let mut angular = [0.; 4];
        step(
            &mut state,
            &mut linear,
            &mut angular,
            2.,
            0.5,
            2.,
            [2., 10.],
        )
        .unwrap();
        assert_eq!(state.previous_center, [10., 20., 30., 2.]);
        assert!((linear[0] - 2.).abs() < 1e-6);
        assert_eq!(state.center, [11., 20., 30., 2.]);
        assert_eq!(&state.orientation[..3], [0.; 3]);
        assert!((state.orientation[3] - 1.).abs() < 1e-7);
        assert_eq!(state.delta_angle, [0.; 4]);
        assert_eq!(&state.transform[12..], [10., 18., 27., 2.]);
    }
    #[test]
    fn invalid_orientation_does_not_partially_advance_body() {
        let mut state = identity();
        state.orientation = [0.; 4];
        let original = state.center;
        let mut linear = [10., 0., 0., 0.];
        let mut angular = [0.; 4];
        assert!(step(&mut state, &mut linear, &mut angular, 0., 1., 1., [2., 10.]).is_err());
        assert_eq!(state.center, original);
        assert_eq!(linear, [10., 0., 0., 0.]);
    }
}
