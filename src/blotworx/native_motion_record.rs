//! Original 32-bit, big-endian motion storage. Pointer bytes remain opaque
//! identity data; they are never dereferenced or executed by the native engine.
//! This isolates transfer semantics from concrete inertia-model construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionRecord(pub [u8; 0x120]);
pub struct RestoredMotion {
    pub motion: MotionRecord,
    pub collision_quality: u8,
}
impl MotionRecord {
    /// Native hkMotion's stored inverse-mass vector (+c0..cc), after scene
    /// restrictions. Keep raw XYZ, including individually zeroed axes; this
    /// is solver storage, not the virtual inertia getter's clamped reciprocal.
    pub fn solver_inverse_mass(&self) -> anyhow::Result<([f32; 3], f32)> {
        anyhow::ensure!(
            matches!(self.motion_type(), 2 | 3 | 6 | 7),
            "Unresolved dynamic inertia mode {}",
            self.motion_type()
        );
        let values = self.floats::<4>(0xc0);
        anyhow::ensure!(
            values.iter().all(|v| v.is_finite() && *v >= 0.),
            "Invalid stored inverse mass/inertia"
        );
        Ok(([values[0], values[1], values[2]], values[3]))
    }
    pub fn motion_type(&self) -> u8 {
        self.0[8]
    }
    /// DOL 0x802d7c6c..0x802d7c9c: construct keyframed motion in-place,
    /// then transfer the previous motion's kinematics. Constructor defaults
    /// overwritten by the transfer need not be calculated twice.
    /// `gravity_factor_word` is the current word at original 0x806ba500; the
    /// constructor stores its high halfword as gravity factor, so the caller supplies it rather
    /// than assuming the original executable's initial value never changes.
    /// This stops BEFORE installing retained dynamic motion and quality,
    /// refreshing the shape or changing any world/island membership.
    pub fn prepare_keyframed(&self, gravity_factor_word: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=7).contains(&self.motion_type()),
            "Invalid source motion type"
        );
        let mut result = self.clone();
        // Original vtable is retained as opaque type identity, never executed.
        result.0[..4].copy_from_slice(&0x806ba3d8u32.to_be_bytes());
        result.0[6..8].copy_from_slice(&1u16.to_be_bytes());
        result.0[8] = 4;
        result.0[0xa..0xe].fill(0);
        result.0[0xc0..0xd0].fill(0);
        result.0[0xf0..0x11e].fill(0);
        result.0[0x11e..].copy_from_slice(&((gravity_factor_word >> 16) as u16).to_be_bytes());
        Ok(result)
    }
    fn floats<const N: usize>(&self, offset: usize) -> [f32; N] {
        std::array::from_fn(|i| {
            f32::from_be_bytes(
                self.0[offset + 4 * i..offset + 4 * i + 4]
                    .try_into()
                    .unwrap(),
            )
        })
    }
    fn write_floats(&mut self, offset: usize, values: &[f32]) {
        for (i, value) in values.iter().enumerate() {
            self.0[offset + 4 * i..offset + 4 * i + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
    pub fn swept_state(&self) -> super::native_motion_step::MotionState {
        super::native_motion_step::MotionState {
            transform: self.floats(0x10),
            previous_center: self.floats(0x50),
            center: self.floats(0x60),
            previous_orientation: self.floats(0x70),
            orientation: self.floats(0x80),
            local_center: self.floats(0x90),
            delta_angle: self.floats(0xa0),
        }
    }
    pub fn linear_velocity(&self) -> [f32; 4] {
        self.floats(0xd0)
    }
    pub fn angular_velocity(&self) -> [f32; 4] {
        self.floats(0xe0)
    }
    /// DOL 802bbfd0: zero inverse mass reports zero, not infinity.
    pub fn mass(&self) -> f32 {
        let inverse = self.floats::<1>(0xcc)[0];
        if inverse == 0. {
            0.
        } else {
            1. / inverse
        }
    }
    /// REL 382d04..382e40, AFTER placement and the component's +38c hook.
    /// Zero means retain the imported field, including for Gravity Factor.
    /// Read/write each property in order; failures retain earlier writes.
    pub fn initialize_motion_tuning(
        &mut self,
        mut property: impl FnMut(&str) -> anyhow::Result<f32>,
    ) -> anyhow::Result<()> {
        for (index, name) in [
            "Linear Damping",
            "Angular Damping",
            "Gravity Factor",
            "Max Linear Velocity",
            "Max Angular Velocity",
        ]
        .into_iter()
        .enumerate()
        {
            let value = property(name)?;
            anyhow::ensure!(value.is_finite(), "Nonfinite motion property {name}");
            if value == 0. {
                continue;
            }
            match index {
                0 => self.write_floats(0xb4, &[value]),
                1 => self.write_floats(0xb8, &[value]),
                // Native srawi/sth keeps the high float halfword, no rounding.
                2 => self.0[0x11e..0x120]
                    .copy_from_slice(&((value.to_bits() >> 16) as u16).to_be_bytes()),
                3 | 4 => self.0[0xbc + index - 3] = super::native_motion_limits::encode(value)?,
                _ => unreachable!(),
            }
        }
        Ok(())
    }
    /// Contiguous loader stage REL 382a78..382c00, BEFORE entity placement.
    /// `center` is None when Should Change Center Of Mass is false; otherwise
    /// the Point3 conversion (23b970) supplies positive zero in W.
    /// Property publication is synchronous: subsequent bodies read the updated
    /// component mass. Failure retains preceding native side effects.
    pub fn initialize_mass_properties(
        &mut self,
        center: Option<[f32; 3]>,
        query: impl FnOnce(&Self) -> anyhow::Result<[[f32; 4]; 2]>,
        mass: impl FnOnce() -> anyhow::Result<f32>,
        publish_mass: impl FnOnce(f32, &Self) -> anyhow::Result<()>,
        rotation_restriction: impl FnOnce() -> anyhow::Result<[f32; 3]>,
    ) -> anyhow::Result<()> {
        if let Some([x, y, z]) = center {
            self.set_center_with_bounds([x, y, z, 0.], query)?;
        }
        if let Some(value) = self.initialize_mass(mass()?)? {
            publish_mass(value, self)?;
        }
        self.restrict_rotation(rotation_restriction()?)
    }
    /// REL 382afc..382bac loader stage. A nonpositive override publishes the
    /// existing positive mass back to the component (returned to the caller).
    /// Positive overrides rescale the LOCAL inertia through its virtual
    /// getter/setter, then set mass. Do not simplify to inverse-inertia/ratio:
    /// that loses the original reciprocal rounding and getter clamps.
    pub fn initialize_mass(&mut self, requested: f32) -> anyhow::Result<Option<f32>> {
        use super::body_mass_properties::{self, Change, CharacterZero};
        match body_mass_properties::initialize(
            self.motion_type(),
            self.floats::<4>(0xc0),
            requested,
            CharacterZero::Reciprocal,
        )? {
            Change::Unchanged => Ok(None),
            Change::Publish(value) => Ok(Some(value)),
            Change::Updated(values) => {
                self.write_floats(0xc0, &values);
                Ok(None)
            }
        }
    }
    pub fn set_linear_velocity(&mut self, value: [f32; 4]) {
        self.write_floats(0xd0, &value);
    }
    /// REL 382bd4..382c00, after mass initialization. The Point3 property is
    /// an axis mask, not a velocity: any nonzero axis clears the corresponding
    /// inverse inertia directly, with no motion-type guard or velocity write.
    pub fn restrict_rotation(&mut self, axes: [f32; 3]) -> anyhow::Result<()> {
        anyhow::ensure!(
            axes.iter().all(|v| v.is_finite()),
            "Nonfinite rotation restriction"
        );
        for (axis, value) in axes.into_iter().enumerate() {
            if value != 0. {
                self.write_floats(0xc0 + axis * 4, &[0.]);
            }
        }
        Ok(())
    }
    pub fn set_angular_velocity(&mut self, value: [f32; 4]) {
        self.write_floats(0xe0, &value);
    }
    pub fn set_swept_state(&mut self, state: &super::native_motion_step::MotionState) {
        self.write_floats(0x10, &state.transform);
        self.write_floats(0x50, &state.previous_center);
        self.write_floats(0x60, &state.center);
        self.write_floats(0x70, &state.previous_orientation);
        self.write_floats(0x80, &state.orientation);
        self.write_floats(0x90, &state.local_center);
        self.write_floats(0xa0, &state.delta_angle);
    }
    pub fn set_transform(
        &mut self,
        transform: super::native_hk_transform::Transform,
    ) -> anyhow::Result<()> {
        let mut state = self.swept_state();
        state.set_transform(transform)?;
        self.set_swept_state(&state);
        Ok(())
    }
    /// Only the motion virtual setter. The body owner must also query its
    /// shape bounds and recompute object radius (DOL 802b7704..802b7840).
    pub fn set_local_center(&mut self, local: [f32; 4]) -> anyhow::Result<()> {
        let mut state = self.swept_state();
        state.set_local_center(local)?;
        self.set_swept_state(&state);
        Ok(())
    }
    /// DOL 802b7704: motion setter first, then the shape's identity-space AABB
    /// query with zero tolerance, then radius publication. The query must use
    /// actual shape semantics, not rendered mesh bounds. Failure after the
    /// first stage retains that change, so callers must not blindly retry.
    pub fn set_center_with_bounds(
        &mut self,
        local: [f32; 4],
        query: impl FnOnce(&Self) -> anyhow::Result<[[f32; 4]; 2]>,
    ) -> anyhow::Result<()> {
        self.set_local_center(local)?;
        let bounds = query(self)?;
        let radius = center_radius(self.floats(0x90), bounds)?;
        self.write_floats(0xb0, &[radius]);
        Ok(())
    }
    /// Run the recovered type-4 integrator against this record, preserving
    /// inertia, damping, bookkeeping and opaque identity fields. The world
    /// owner still must perform collision and ordered publication separately.
    pub fn step_keyframed(
        &mut self,
        start: f32,
        delta: f32,
        inverse_delta: f32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.motion_type() == 4,
            "Keyframed step requires motion type 4"
        );
        let mut state = self.swept_state();
        let mut linear = self.floats(0xd0);
        let mut angular = self.floats(0xe0);
        let limits = [
            super::native_motion_limits::decode(self.0[0xbc]),
            super::native_motion_limits::decode(self.0[0xbd]),
        ];
        super::native_motion_step::step(
            &mut state,
            &mut linear,
            &mut angular,
            start,
            delta,
            inverse_delta,
            limits,
        )?;
        self.set_swept_state(&state);
        self.write_floats(0xd0, &linear);
        self.write_floats(0xe0, &angular);
        Ok(())
    }
    /// DOL 0x802d7900..0x802d796c: apply current kinematics to saved inertia,
    /// reset the two 16-bit counters, and restore the saved collision quality.
    /// Resource release, shape refresh and world callbacks are caller-owned;
    /// this returns the replacement state without publishing it prematurely.
    pub fn restore_dynamic(&self, saved: &Self) -> anyhow::Result<RestoredMotion> {
        anyhow::ensure!(
            matches!(self.motion_type(), 4 | 5),
            "Dynamic restore requires fixed/keyframed current motion"
        );
        anyhow::ensure!(
            matches!(saved.motion_type(), 1 | 2 | 3 | 6 | 7),
            "Saved motion is not dynamic"
        );
        let mut motion = saved.clone();
        motion.transfer_kinematics_from(self);
        motion.0[0xa..0xe].fill(0);
        // lhz current+0x11c followed by stb: keep the low byte.
        Ok(RestoredMotion {
            motion,
            collision_quality: self.0[0x11d],
        })
    }
    /// DOL 0x802bc0ac, virtual slot +0x6c for base, keyframed and inertia motion.
    /// Keep destination type, inverse inertia/mass and saved-motion identity.
    /// This is not a whole-motion replacement or a world/island operation.
    pub fn transfer_kinematics_from(&mut self, source: &Self) {
        self.0[0x10..0xbf].copy_from_slice(&source.0[0x10..0xbf]);
        self.0[0xd0..0xf0].copy_from_slice(&source.0[0xd0..0xf0]);
        self.0[9] = source.0[9];
    }
}

/// DOL 802b7770..802b7840 compares distances to the two AABB endpoints.
/// It does not synthesize a furthest corner from independent axis maxima.
fn center_radius(center: [f32; 4], bounds: [[f32; 4]; 2]) -> anyhow::Result<f32> {
    anyhow::ensure!(
        center[..3]
            .iter()
            .chain(bounds[0][..3].iter())
            .chain(bounds[1][..3].iter())
            .all(|v| v.is_finite()),
        "Nonfinite center/radius bounds"
    );
    anyhow::ensure!(
        (0..3).all(|i| bounds[0][i] <= bounds[1][i]),
        "Inverted shape bounds"
    );
    let distance = |corner: [f32; 4]| {
        let d: [f32; 3] = std::array::from_fn(|i| corner[i] - center[i]);
        d[2].mul_add(d[2], d[0] * d[0]) + d[1] * d[1]
    };
    let lower = distance(bounds[0]);
    let upper = distance(bounds[1]);
    let squared = if upper > lower { upper } else { lower };
    anyhow::ensure!(squared.is_finite(), "Shape radius overflow");
    // At zero, frsqrte/its refinement is unordered; original fsel chooses
    // its zero fallback before multiplication. Positive path has one Newton step.
    Ok(if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loader_tuning_preserves_zero_overrides_and_original_encodings() {
        let mut record = MotionRecord([0x55; 0x120]);
        let original = record.clone();
        record.initialize_motion_tuning(|_| Ok(-0.)).unwrap();
        assert_eq!(record, original);
        let mut order = Vec::new();
        record
            .initialize_motion_tuning(|name| {
                order.push(name.to_owned());
                Ok(match name {
                    "Linear Damping" => -0.5,
                    "Angular Damping" => 0.25,
                    "Gravity Factor" => f32::from_bits(0x3f81ffff),
                    "Max Linear Velocity" => 0.011,
                    "Max Angular Velocity" => f32::MAX,
                    _ => unreachable!(),
                })
            })
            .unwrap();
        assert_eq!(
            order,
            [
                "Linear Damping",
                "Angular Damping",
                "Gravity Factor",
                "Max Linear Velocity",
                "Max Angular Velocity"
            ]
        );
        assert_eq!(record.floats::<2>(0xb4), [-0.5, 0.25]);
        assert_eq!(&record.0[0x11e..], [0x3f, 0x81]);
        assert_eq!(&record.0[0xbc..0xbe], [2, 255]);
        let mut expected = original;
        expected.write_floats(0xb4, &[-0.5, 0.25]);
        expected.0[0x11e..].copy_from_slice(&[0x3f, 0x81]);
        expected.0[0xbc..0xbe].copy_from_slice(&[2, 255]);
        assert_eq!(record, expected);
        assert!(record
            .initialize_motion_tuning(|name| if name == "Linear Damping" {
                Ok(0.125)
            } else {
                anyhow::bail!("Property unavailable")
            })
            .is_err());
        assert_eq!(record.floats::<2>(0xb4), [0.125, 0.25]);
    }
    #[test]
    fn loader_mass_properties_preserve_order_shared_writeback_and_failure() {
        use std::cell::{Cell, RefCell};
        let make = |mass: f32| {
            let mut record = MotionRecord([0; 0x120]);
            record.0[8] = 3;
            record
                .set_transform(super::super::native_hk_transform::from_entity(
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                    [5., 6., 7.],
                ))
                .unwrap();
            record.write_floats(0xc0, &[1., 2., 4., 1. / mass]);
            record
        };
        let mut first = make(8.);
        let mut second = make(16.);
        let mass = Cell::new(0.);
        let events = RefCell::new(Vec::new());
        first
            .initialize_mass_properties(
                Some([1., 2., 3.]),
                |current| {
                    events.borrow_mut().push("bounds");
                    assert_eq!(current.swept_state().local_center, [1., 2., 3., 0.]);
                    assert_eq!(current.mass(), 8.);
                    Ok([[-1., -1., -1., 0.], [1., 1., 1., 0.]])
                },
                || {
                    events.borrow_mut().push("read mass");
                    Ok(mass.get())
                },
                |value, current| {
                    events.borrow_mut().push("publish mass");
                    mass.set(value);
                    assert_eq!(current.floats::<3>(0xc0), [1., 2., 4.]);
                    Ok(())
                },
                || {
                    events.borrow_mut().push("read restriction");
                    Ok([1., 0., 0.])
                },
            )
            .unwrap();
        assert_eq!(
            *events.borrow(),
            ["bounds", "read mass", "publish mass", "read restriction"]
        );
        assert_eq!(first.floats::<3>(0xc0), [0., 2., 4.]);
        second
            .initialize_mass_properties(
                None,
                |_| panic!("Disabled center must not query"),
                || Ok(mass.get()),
                |_, _| panic!("Positive mass must not publish"),
                || Ok([0.; 3]),
            )
            .unwrap();
        assert_eq!(second.mass(), 8.);
        assert_eq!(second.floats::<3>(0xc0), [2., 4., 8.]);
        // Failed property publication stops before restriction, retaining COM.
        let mut failed = make(4.);
        assert!(failed
            .initialize_mass_properties(
                Some([2., 0., 0.]),
                |_| Ok([[-1.; 4], [1.; 4]]),
                || Ok(0.),
                |_, _| anyhow::bail!("Property write failed"),
                || panic!("Restriction must follow successful publication")
            )
            .is_err());
        assert_eq!(failed.swept_state().local_center, [2., 0., 0., 0.]);
        assert_eq!(failed.floats::<3>(0xc0), [1., 2., 4.]);
    }
    #[test]
    fn body_center_radius_uses_original_endpoint_rule_and_query_order() {
        let mut record = MotionRecord([0; 0x120]);
        record.0[8] = 3;
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        record.set_transform(transform).unwrap();
        record
            .set_center_with_bounds([-2., 1., 0., 7.], |current| {
                assert_eq!(current.floats::<4>(0x90), [-2., 1., 0., 7.]);
                assert_eq!(current.floats::<1>(0xb0), [0.]);
                Ok([[-2., -1., 0., 100.], [2., 1., 0., 200.]])
            })
            .unwrap();
        let radius = record.floats::<1>(0xb0)[0];
        assert!((radius - 4.).abs() < 0.000001);
        assert!(
            radius < 20f32.sqrt(),
            "Do not substitute farthest-corner bounds"
        );
        assert_eq!(
            center_radius([1., 2., 3., 9.], [[1., 2., 3., 0.]; 2])
                .unwrap()
                .to_bits(),
            0
        );
        assert!(record
            .set_center_with_bounds([0.; 4], |_| anyhow::bail!("Shape query failed"))
            .is_err());
        assert_eq!(record.floats::<4>(0x90), [0.; 4]);
        assert_eq!(record.floats::<1>(0xb0)[0].to_bits(), radius.to_bits());
    }
    #[test]
    fn center_change_uses_current_rotation_for_both_endpoints_without_warping() {
        let mut record = MotionRecord([0; 0x120]);
        record.0[8] = 3;
        let transform = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [11., 12., 13.],
        );
        let state = super::super::native_motion_step::MotionState {
            transform,
            previous_center: [10., 20., 30., 2.],
            center: [40., 50., 60., 30.],
            previous_orientation: [0., 0., 0., 1.],
            orientation: [0., 0., 1., 0.],
            local_center: [1., 2., 3., 9.],
            delta_angle: [4., 5., 6., 7.],
        };
        record.set_swept_state(&state);
        record.set_linear_velocity([1., 2., 3., 4.]);
        record.set_angular_velocity([5., 6., 7., 8.]);
        let before = record.clone();
        record.set_local_center([3., 5., 7., 8.]).unwrap();
        let after = record.swept_state();
        assert_eq!(after.previous_center, [7., 22., 34., 2.]);
        assert_eq!(after.center, [37., 52., 64., 30.]);
        assert_eq!(after.local_center, [3., 5., 7., 8.]);
        let mut expected = before.clone();
        expected.write_floats(0x50, &after.previous_center);
        expected.write_floats(0x60, &after.center);
        expected.write_floats(0x90, &after.local_center);
        assert_eq!(record, expected);
        let before_error = record.clone();
        assert!(record.set_local_center([f32::NAN, 0., 0., 0.]).is_err());
        assert_eq!(record, before_error);
    }
    #[test]
    fn rotation_restriction_follows_mass_and_survives_motion_round_trip() {
        let mut record = MotionRecord([0x5a; 0x120]);
        record.0[8] = 3;
        record.write_floats(0xc0, &[0.5, 0.25, 0.125, 0.25]);
        record.set_angular_velocity([3., 4., 5., 6.]);
        record.initialize_mass(8.).unwrap();
        let before = record.clone();
        record.restrict_rotation([1., -0., -2.]).unwrap();
        let mut expected = before.clone();
        expected.0[0xc0..0xc4].fill(0);
        expected.0[0xc8..0xcc].fill(0);
        assert_eq!(record, expected);
        assert_eq!(record.mass(), 8.);
        assert_eq!(record.angular_velocity(), [3., 4., 5., 6.]);
        let mut owner = super::super::body_motion::BodyMotion::new(record, 6, None).unwrap();
        owner.replace(4, 0x3f800000).unwrap();
        owner.replace(1, 0x3f800000).unwrap();
        assert_eq!(owner.current.floats::<4>(0xc0), [0., 0.125, 0., 0.125]);
        assert_eq!(owner.current.angular_velocity(), [3., 4., 5., 6.]);
    }
    #[test]
    fn rotation_restriction_has_no_mode_guard_and_rejects_bad_input_before_writes() {
        for mode in [2, 3, 4, 5, 6, 7] {
            let mut record = MotionRecord([0; 0x120]);
            record.0[8] = mode;
            record.write_floats(0xc0, &[0.5, 0.25, 0.125, 0.25]);
            let before = record.clone();
            assert!(record.restrict_rotation([1., f32::NAN, 0.]).is_err());
            assert_eq!(record, before);
            record.restrict_rotation([0., 0., -0.]).unwrap();
            assert_eq!(record, before);
            record
                .restrict_rotation([0., f32::MIN_POSITIVE, 0.])
                .unwrap();
            assert_eq!(record.floats::<4>(0xc0), [0.5, 0., 0.125, 0.25]);
        }
    }
    #[test]
    fn loader_mass_rescales_each_concrete_inertia_model_and_only_its_storage() {
        for mode in [2, 3, 6, 7] {
            let mut record = MotionRecord([0x5a; 0x120]);
            record.0[8] = mode;
            record.write_floats(0xc0, &[0.5, 0.25, 0.125, 0.25]);
            let before = record.clone();
            assert_eq!(record.mass(), 4.);
            assert_eq!(record.initialize_mass(8.).unwrap(), None);
            let expected = if matches!(mode, 3 | 6) {
                [0.25, 0.125, 0.0625, 0.125]
            } else {
                [0.25, 0.25, 0.25, 0.125]
            };
            assert_eq!(record.floats::<4>(0xc0), expected);
            assert_eq!(&record.0[..0xc0], &before.0[..0xc0]);
            assert_eq!(&record.0[0xd0..], &before.0[0xd0..]);
            assert_eq!(record.mass(), 8.);
        }
    }
    #[test]
    fn loader_mass_preserves_getter_clamps_and_property_writeback_branch() {
        let epsilon = f32::from_bits(0x34000000);
        for mode in [2, 3, 6, 7] {
            let mut record = MotionRecord([0; 0x120]);
            record.0[8] = mode;
            record.write_floats(0xc0, &[0., 0., 0., 0.25]);
            let before = record.clone();
            assert_eq!(record.initialize_mass(-1.).unwrap(), Some(4.));
            assert_eq!(record, before);
            record.initialize_mass(8.).unwrap();
            assert_eq!(
                record.floats::<3>(0xc0),
                [if mode == 7 { 0. } else { epsilon * 0.5 }; 3]
            );
        }
        for mode in [4, 5] {
            let mut record = MotionRecord([0; 0x120]);
            record.0[8] = mode;
            let before = record.clone();
            assert_eq!(record.initialize_mass(8.).unwrap(), None);
            assert_eq!(record.initialize_mass(0.).unwrap(), None);
            assert_eq!(record, before);
        }
        let mut record = MotionRecord([0; 0x120]);
        record.0[8] = 1;
        let before = record.clone();
        assert!(record.initialize_mass(8.).is_err());
        assert_eq!(record, before);
        assert!(record.initialize_mass(f32::NAN).is_err());
        assert_eq!(record, before);
    }
    #[test]
    fn keyframed_preparation_matches_captured_motion_records() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_transfer.json"
        ))
        .unwrap();
        let read = |v: &serde_json::Value| {
            let bytes: Vec<u8> = v
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|v| (v.as_u64().unwrap() as u32).to_be_bytes())
                .collect();
            MotionRecord(bytes.try_into().unwrap())
        };
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        for sample in samples {
            let source = read(&sample["source"]);
            let prepared = source.prepare_keyframed(0x3f800000).unwrap();
            assert_eq!(prepared, read(&sample["result"]));
            assert_eq!(
                source.prepare_keyframed(0x12345678).unwrap().0[0x11e..],
                [0x12, 0x34]
            );
        }
        assert!(MotionRecord([0; 0x120])
            .prepare_keyframed(0x3f800000)
            .is_err());
    }
    #[test]
    fn restoring_keeps_current_pose_and_saved_inertia() {
        let mut current = MotionRecord([0x55; 0x120]);
        current.0[8] = 4;
        current.0[0x11c..0x11e].copy_from_slice(&[0x12, 0x34]);
        let mut saved = MotionRecord([0xa5; 0x120]);
        saved.0[8] = 3;
        let restored = current.restore_dynamic(&saved).unwrap();
        assert_eq!(restored.collision_quality, 0x34);
        assert_eq!(restored.motion.motion_type(), 3);
        assert_eq!(&restored.motion.0[0x10..0xbf], &current.0[0x10..0xbf]);
        assert_eq!(&restored.motion.0[0xd0..0xf0], &current.0[0xd0..0xf0]);
        assert_eq!(&restored.motion.0[0xc0..0xd0], &saved.0[0xc0..0xd0]);
        assert_eq!(&restored.motion.0[0xa..0xe], &[0; 4]);
        assert_eq!(&restored.motion.0[0xe..0x10], &[0xa5; 2]);
        assert_eq!(saved.0[0xa], 0xa5, "Input snapshot must stay immutable");
        saved.0[8] = 5;
        assert!(current.restore_dynamic(&saved).is_err());
    }
    #[test]
    fn record_steps_match_original_state_and_preserve_other_fields() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_step.json"
        ))
        .unwrap();
        let read = |v: &serde_json::Value| -> Vec<f32> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                .collect()
        };
        assert_eq!(fixture["complete"], true);
        for sample in fixture["samples"].as_array().unwrap() {
            let mut record = MotionRecord([0xa5; 0x120]);
            record.0[8] = 4;
            record.write_floats(0x10, &read(&sample["state"]));
            record.write_floats(0xd0, &read(&sample["linear"]));
            record.write_floats(0xe0, &read(&sample["angular"]));
            let previous = record.clone();
            let step = read(&sample["step"]);
            record.step_keyframed(step[0], step[2], step[3]).unwrap();
            let expected = read(&sample["result"]);
            for (i, value) in expected.iter().enumerate() {
                assert_eq!(
                    record.floats::<1>(0x10 + i * 4)[0].to_bits(),
                    value.to_bits(),
                    "state word {i}"
                );
            }
            assert_eq!(
                record.floats::<4>(0xd0).map(f32::to_bits),
                read(&sample["result_linear"])
                    .try_into()
                    .map(|v: [f32; 4]| v.map(f32::to_bits))
                    .unwrap()
            );
            assert_eq!(
                record.floats::<4>(0xe0).map(f32::to_bits),
                read(&sample["result_angular"])
                    .try_into()
                    .map(|v: [f32; 4]| v.map(f32::to_bits))
                    .unwrap()
            );
            assert_eq!(&record.0[..0x10], &previous.0[..0x10]);
            assert_eq!(&record.0[0xc0..0xd0], &previous.0[0xc0..0xd0]);
            assert_eq!(&record.0[0xf0..], &previous.0[0xf0..]);
            let before = record.clone();
            assert!(record.step_keyframed(0., f32::NAN, 30.).is_err());
            assert_eq!(record, before);
        }
    }
    #[test]
    fn original_motion_transfers_match_every_byte() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_transfer.json"
        ))
        .unwrap();
        assert_eq!(fixture["complete"], true);
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let read = |value: &serde_json::Value| {
            let bytes: Vec<u8> = value
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|v| (v.as_u64().unwrap() as u32).to_be_bytes())
                .collect();
            MotionRecord(bytes.try_into().unwrap())
        };
        let mut changed = 0;
        for sample in samples {
            let source = read(&sample["source"]);
            let mut destination = read(&sample["destination"]);
            let previous = destination.clone();
            destination.transfer_kinematics_from(&source);
            assert_eq!(destination, read(&sample["result"]));
            changed += usize::from(destination != previous);
        }
        assert_eq!(
            changed, 8,
            "Reference must exercise actual destination changes"
        );
    }
    #[test]
    fn transfer_preserves_inertia_type_and_saved_identity() {
        let source = MotionRecord(std::array::from_fn(|i| i as u8));
        let mut saved = MotionRecord([0xa5; 0x120]);
        saved.transfer_kinematics_from(&source);
        assert_eq!(&saved.0[0xc0..0xd0], &[0xa5; 16]);
        assert_eq!(saved.0[8], 0xa5);
        assert_eq!(&saved.0[0xf0..], &[0xa5; 0x30]);
        assert_eq!(&saved.0[0x10..0x20], &source.0[0x10..0x20]);
        assert_eq!(&saved.0[0xd0..0xf0], &source.0[0xd0..0xf0]);
        assert_eq!(saved.0[0xbf], 0xa5);
        assert_eq!(saved.0[9], 9);
    }
}
