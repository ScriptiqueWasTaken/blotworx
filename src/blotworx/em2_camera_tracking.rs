//! PCf18370 camera-owned MickeyTracker clones and ab3c00 point calculation.
//! World admission is shared with scene entities; motion/facing/update phases
//! remain explicit requirements, not an assumed copy of the player transform.
use super::pc_float::Precision;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Component, SceneFile, ID};
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Binding {
    pub slot: usize,
    pub scene: usize,
    pub entity: u32,
    pub target_scene: usize,
    pub target: u32,
    pub resource: String,
}
pub(super) fn prepare(
    scenes: &mut Vec<SceneFile>,
    names: &mut Vec<String>,
    templates: &super::em2_children::Templates,
    players: [&str; 2],
) -> Result<Vec<Binding>> {
    ensure!(
        scenes.len() == names.len(),
        "Camera tracker scene identities differ"
    );
    let version = scenes
        .first()
        .context("Camera tracker has no source scene")?
        .version
        .clone();
    let scene = scenes.len();
    let mut source = SceneFile::new(vec![], vec![], vec![], ID::new(0), version);
    let mut next = scenes
        .iter()
        .flat_map(|s| &s.objects)
        .flat_map(|e| {
            std::iter::once(e.link_id.to_u32())
                .chain(e.components.iter().map(|c| c.link_id.to_u32()))
        })
        .max()
        .unwrap_or(0)
        .max(templates.maximum_identity());
    let mut bindings = Vec::new();
    for (slot, name) in players.iter().enumerate() {
        let matches = scenes
            .iter()
            .enumerate()
            .flat_map(|(s, scene)| {
                scene
                    .objects
                    .iter()
                    .filter(|e| scene.scene.contains(&e.link_id) && e.name == *name)
                    .map(move |e| (s, e.link_id.to_u32()))
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Camera tracker target {name} is absent or ambiguous"
        );
        let mut spawned = templates.instantiate("MickeyTracker", &mut next)?;
        ensure!(
            spawned.entity.components.iter().all(|c| matches!(
                c.class_name.as_str(),
                "JPSTransformationComponent" | "EntityTrackerComponent" | "InputOutputComponent"
            )),
            "MickeyTracker has an unimplemented additional component lifecycle"
        );
        let mut pose =
            crate::render_support::scene::get_entity_transform(&spawned.entity, &[spawned.entity.clone()]);
        // Native CloneFromDynamicPalette receives an explicit zero position;
        // its null rotation argument does not authorize guessing another pose.
        ensure!(
            pose.rotation == [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] && pose.scale == [1.; 3],
            "Nondefault MickeyTracker source orientation needs native clone validation"
        );
        pose.translation = [0.; 3];
        crate::render_support::scene::set_entity_transform(&mut spawned.entity, pose);
        let entity = spawned.entity.link_id.to_u32();
        source.scene.push(spawned.entity.link_id);
        source.objects.push(spawned.entity);
        bindings.push(Binding {
            slot,
            scene,
            entity,
            target_scene: matches[0].0,
            target: matches[0].1,
            resource: spawned.resource,
        });
    }
    scenes.push(source);
    names.push("<runtime camera trackers>".into());
    Ok(bindings)
}
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Parameters {
    pub buffer_size: usize,
    pub offset: [f32; 3],
    pub special_states: Vec<i32>,
    pub special_offsets: Vec<[f32; 3]>,
}
fn vector(value: &serde_json::Value) -> Result<[f32; 3]> {
    let v = [
        value["x"].as_f64().context("Tracker offset X absent")? as f32,
        value["y"].as_f64().context("Tracker offset Y absent")? as f32,
        value["z"].as_f64().context("Tracker offset Z absent")? as f32,
    ];
    ensure!(
        v.iter().all(|v| v.is_finite()),
        "Nonfinite PC tracker offset"
    );
    Ok(v)
}
impl Parameters {
    pub fn load(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "EntityTrackerComponent",
            "Wrong PC tracker component"
        );
        let get = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("PC tracker property {name} has no reconstructed default"))
        };
        let count = get("BufferSize")?
            .as_i64()
            .context("Invalid PC tracker BufferSize")?;
        ensure!(
            count >= 0 && count <= i32::MAX as i64,
            "Unsupported PC tracker buffer count"
        );
        let special_states = get("SpecialStates")?
            .as_array()
            .context("Invalid PC tracker states")?
            .iter()
            .map(|v| {
                i32::try_from(v.as_i64().context("Invalid PC tracker state")?).map_err(Into::into)
            })
            .collect::<Result<Vec<_>>>()?;
        let special_offsets = get("SpecialStateOffsets")?
            .as_array()
            .context("Invalid PC tracker special offsets")?
            .iter()
            .map(vector)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            special_offsets.len() >= special_states.len(),
            "PC tracker special state lacks an offset"
        );
        Ok(Self {
            buffer_size: count as usize,
            offset: vector(get("TargetOffset")?)?,
            special_states,
            special_offsets,
        })
    }
    pub fn current_point(
        &self,
        position: [f32; 3],
        rotation: [f32; 9],
        motion_state: Option<i32>,
        precision: Precision,
    ) -> Result<([f32; 3], [f32; 3])> {
        let offset = motion_state
            .and_then(|s| self.special_states.iter().position(|state| *state == s))
            .map(|i| self.special_offsets[i])
            .unwrap_or(self.offset);
        super::pc_float::target_point(position, rotation, offset, precision)
    }
}
#[derive(serde::Serialize)]
pub(super) struct Owned {
    pub binding: Binding,
    pub owner: usize,
    pub target: usize,
    pub parameters: Parameters,
    pub reset_complete: bool,
    pub phase: &'static str,
}
impl Owned {
    pub fn new(
        binding: Binding,
        owner: usize,
        target: usize,
        components: &[Component],
    ) -> Result<Self> {
        let component = components
            .iter()
            .find(|c| c.class_name == "EntityTrackerComponent")
            .context("Admitted camera tracker has no tracker component")?;
        Ok(Self {
            binding,
            owner,
            target,
            parameters: Parameters::load(component)?,
            reset_complete: false,
            phase: "waiting for target motion state, tracker Reset/facing and update lifecycle",
        })
    }
}

/// PC96c610 reads column2 of the published transform rotation, not a row.
pub(super) fn forward(rotation: [f32; 9]) -> Result<[f32; 3]> {
    ensure!(
        rotation.iter().all(|v| v.is_finite()),
        "Nonfinite PC target rotation"
    );
    Ok([rotation[2], rotation[5], rotation[8]])
}
/// PC96cb30 -> NiMatrix3::FromEulerAnglesXYZ. The native CRT angle
/// calculation is compared separately from the x87 matrix arithmetic.
pub(super) fn facing(
    direction: [f32; 3],
    roll: f32,
    precision: Precision,
    math: super::pc_float::FacingMath,
) -> Result<[f32; 9]> {
    ensure!(
        direction.iter().all(|v| v.is_finite()) && roll.is_finite(),
        "Nonfinite PC facing input"
    );
    let (yaw, pitch) = super::pc_float::facing_angles(direction, precision, math)?;
    let (sx, cx) = super::pc_float::sincos(pitch, precision)?;
    let (sy, cy) = super::pc_float::sincos(yaw, precision)?;
    let (sz, cz) = super::pc_float::sincos(roll, precision)?;
    let x = [1., 0., 0., 0., cx, sx, 0., -sx, cx];
    let y = [cy, 0., -sy, 0., 1., 0., sy, 0., cy];
    let z = [cz, sz, 0., -sz, cz, 0., 0., 0., 1.];
    let multiply = |a: [f32; 9], b: [f32; 9]| -> Result<[f32; 9]> {
        let mut out = [0.; 9];
        for col in 0..3 {
            let column = [b[col], b[col + 3], b[col + 6]];
            let (_, product) = super::pc_float::target_point([0.; 3], a, column, precision)?;
            for row in 0..3 {
                out[row * 3 + col] = product[row];
            }
        }
        Ok(out)
    };
    multiply(y, multiply(x, z)?)
}

/// Native ab3f10 reset. The host must resolve the real target, publish the
/// translation (including its event semantics), and apply native SetFacing.
/// No default callbacks: missing lifecycle services must remain failures.
pub(super) trait ResetHost {
    fn current_point(&mut self) -> Result<Option<([f32; 3], [f32; 3])>>;
    fn publish_translation(&mut self, point: [f32; 3]) -> Result<()>;
    fn target_forward(&mut self) -> Result<[f32; 3]>;
    fn set_facing(&mut self, forward: [f32; 3], roll: f32) -> Result<()>;
}
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct ResetState {
    pub history: Vec<[f32; 3]>,
    pub head: usize,
    pub translation: [f32; 3],
    pub rotated_offset: [f32; 3],
    pub height: f32,
    pub ground_target: f32,
    pub ground_offset: f32,
    pub eligible: bool,
    pub flags_84_85: [bool; 2],
    pub external_ground: [f32; 3],
    pub flags_c0_c2: [bool; 3],
    pub ground_clock: f32,
    pub flags_11c_11f: [bool; 4],
}
/// PCab35b0 registers these events in this order after unregistering all
/// previous listeners. They are scoped to the selected target handle.
pub(super) const TARGET_EVENTS: [u16; 6] = [0x29, 0x82, 0x8079, 0x11, 0xb2, 0xb3];
#[derive(Clone, Copy)]
pub(super) struct TargetEvent {
    pub id: u16,
    pub payload14: bool,
    pub payload15: bool,
}
/// Native ab3030 reads these fields from a present BasicMotion component.
/// Their source is mandatory; absence is distinct from a default state0.
#[derive(Clone, Copy)]
pub(super) struct GroundMotion {
    pub support_type: i32,
    pub state: i32,
    pub field288: f32,
}
pub(super) fn on_real_ground(motion: Option<GroundMotion>) -> bool {
    let Some(m) = motion else { return false };
    (m.support_type == 1 || m.support_type == 2 || m.state == 0 || m.state == 3 || m.field288 == 0.)
        && m.state != 7
        && m.state != 1
}
/// ab30a0 accepts the transient11c latch even without a motion component.
pub(super) fn on_custom_ground(latch: bool, motion: Option<GroundMotion>, states: &[i32]) -> bool {
    latch || motion.is_some_and(|m| states.contains(&m.state))
}
/// Native motion/ground services remain mandatory; update never invents
/// a grounded result when the physics component is unavailable.
pub(super) trait UpdateHost: ResetHost {
    fn on_real_ground(&mut self) -> Result<bool>;
    fn on_custom_ground(&mut self) -> Result<bool>;
    fn motion_state(&mut self) -> Result<Option<i32>>;
    fn update_ground_plane(&mut self, state: &mut ResetState, point: &mut [f32; 3]) -> Result<()>;
    fn transition_tuning(&mut self) -> Result<(f32, f32)>;
}
#[derive(Debug)]
pub(super) struct UpdateResult {
    pub published: bool,
    pub clear_aligned: bool,
}
impl ResetState {
    /// PCab4c30. ground_mode_property is the explicitly resolved base134
    /// property. Pause preserves one-frame event flags; missing targets clear
    /// the first three, as does a successfully published update.
    pub fn update(
        &mut self,
        paused: bool,
        buffer_size: usize,
        ground_mode_property: bool,
        precision: Precision,
        host: &mut impl UpdateHost,
    ) -> Result<UpdateResult> {
        let mut result = UpdateResult {
            published: false,
            clear_aligned: false,
        };
        if paused {
            return Ok(result);
        }
        if let Some((mut point, offset)) = host.current_point()? {
            ensure!(
                point.iter().chain(&offset).all(|v| v.is_finite()),
                "Nonfinite PC tracker update point"
            );
            self.rotated_offset = offset;
            if buffer_size > 1 && !self.history.is_empty() {
                ensure!(
                    buffer_size == self.history.len() && self.head < buffer_size,
                    "PC tracker history/property mismatch"
                );
                self.history[self.head] = point;
                self.head = (self.head + 1) % buffer_size;
                point = super::pc_float::history_average(&self.history, precision)?;
            }
            let using = (!ground_mode_property || !self.flags_84_85[1]) && self.flags_c0_c2[0];
            if self.flags_c0_c2[1] != using {
                if !self.flags_c0_c2[2] {
                    self.ground_clock = self.height;
                }
                self.flags_c0_c2[2] = true;
            }
            self.flags_c0_c2[1] = using;
            if using {
                self.ground_offset = point[1];
                host.update_ground_plane(self, &mut point)?;
                self.ground_offset -= point[1];
            } else {
                self.eligible = host.on_real_ground()? || host.on_custom_ground()?;
                self.height = point[1];
                self.ground_offset = 0.;
                if self.eligible {
                    if let Some(state) = host.motion_state()? {
                        if state != 1 && !self.flags_11c_11f[3] {
                            self.flags_84_85 = [false; 2];
                            result.clear_aligned = true;
                        }
                    }
                }
            }
            if self.flags_c0_c2[2] {
                let (rate, threshold) = host.transition_tuning()?;
                ensure!(
                    threshold.is_finite(),
                    "Nonfinite PC tracker blend threshold"
                );
                self.ground_clock =
                    super::pc_float::ground_blend(self.height, self.ground_clock, rate, precision)?;
                if threshold > (self.height - self.ground_clock).abs() {
                    self.flags_c0_c2[2] = false;
                }
                point[1] = self.ground_clock;
            }
            host.publish_translation(point)?;
            self.translation = point;
            let direction = host.target_forward()?;
            host.set_facing(direction, 0.)?;
            result.published = true;
        }
        self.flags_11c_11f[..3].fill(false);
        Ok(result)
    }
    /// ab31c0 receives the listener subobject at component+34. Returns
    /// whether field86 must be set; false preserves that field (Reset does
    /// not initialize it). Other unrecognized events are native no-ops.
    pub fn target_event(&mut self, event: TargetEvent) -> Result<bool> {
        let mut set_aligned = false;
        match event.id {
            0x29 => {
                if event.payload14 {
                    self.flags_84_85[1] = true;
                    ensure!(
                        self.ground_target.is_finite() && self.height.is_finite(),
                        "Nonfinite PC tracker event ground state"
                    );
                    if self.eligible && (self.ground_target - self.height).abs() < 0.01 {
                        set_aligned = true;
                    }
                }
                if event.payload15 {
                    self.flags_84_85[0] = true;
                }
                if !self.flags_84_85[0]
                    && !self.flags_11c_11f[1]
                    && !self.flags_11c_11f[2]
                    && !self.flags_11c_11f[3]
                {
                    self.flags_11c_11f[0] = true;
                }
            }
            0x11 => {
                self.flags_11c_11f[2] = true;
                self.flags_11c_11f[0] = false;
            }
            0x82 | 0x8079 => {
                self.flags_11c_11f[1] = true;
                self.flags_11c_11f[0] = false;
            }
            0xb2 => {
                self.flags_84_85[0] = true;
                self.flags_11c_11f[3] = true;
                self.external_ground[1] = f32::MAX;
            }
            0xb3 => {
                self.flags_11c_11f[3] = false;
            }
            _ => {}
        }
        Ok(set_aligned)
    }
    pub fn run(buffer_size: usize, host: &mut impl ResetHost) -> Result<Option<Self>> {
        // A missing handle/entity returns before resetting any tracker fields.
        let Some((point, rotated_offset)) = host.current_point()? else {
            return Ok(None);
        };
        ensure!(
            point.iter().chain(&rotated_offset).all(|v| v.is_finite()),
            "Nonfinite PC tracker reset point"
        );
        let mut history = Vec::new();
        history
            .try_reserve_exact(buffer_size)
            .context("PC tracker history allocation failed")?;
        history.resize(buffer_size, point);
        host.publish_translation(point)?;
        let forward = host.target_forward()?;
        ensure!(
            forward.iter().all(|v| v.is_finite()),
            "Nonfinite PC tracker reset facing"
        );
        host.set_facing(forward, 0.)?;
        Ok(Some(Self {
            history,
            head: 0,
            translation: point,
            rotated_offset,
            height: point[1],
            ground_target: point[1],
            ground_offset: 0.,
            eligible: true,
            flags_84_85: [false; 2],
            external_ground: [f32::MAX; 3],
            flags_c0_c2: [true, true, false],
            ground_clock: 0.,
            flags_11c_11f: [false; 4],
        }))
    }
}

/// f413a0: these are distinct camera vectors. A genuine missing tracker is a
/// native fallback; an admitted but uninitialized tracker is not missing.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(super) struct Working {
    pub translation: [f32; 4],
    pub target: [f32; 4],
    pub rigid: [f32; 4],
    pub origin: [f32; 4],
}
#[derive(Clone, Copy)]
pub(super) struct TrackerSnapshot {
    pub external_ground: f32,
    pub ground_offset: f32,
    pub rigid: Option<[f32; 4]>,
}
pub(super) fn working(
    translation: [f32; 4],
    tracker: Option<TrackerSnapshot>,
    cached_height: f32,
) -> Result<Working> {
    ensure!(
        translation.iter().all(|v| v.is_finite()),
        "Nonfinite PC camera target translation"
    );
    let mut out = Working {
        translation,
        target: translation,
        rigid: translation,
        origin: translation,
    };
    if let Some(tracker) = tracker {
        ensure!(
            tracker.external_ground.is_finite(),
            "Nonfinite PC tracker ground plane"
        );
        if tracker.external_ground >= f32::from_bits(0x7c1071db) {
            ensure!(
                tracker.ground_offset.is_finite(),
                "Nonfinite PC tracker ground offset"
            );
            out.target[1] += tracker.ground_offset;
            // ab4050 returns false without writing if its target is absent.
            if let Some(rigid) = tracker.rigid {
                ensure!(
                    rigid.iter().all(|v| v.is_finite()),
                    "Nonfinite PC rigid target"
                );
                out.rigid = rigid;
            }
        }
    } else {
        ensure!(
            cached_height.is_finite(),
            "Nonfinite PC cached camera height"
        );
        out.target[1] += cached_height;
    }
    ensure!(out.target[1].is_finite(), "PC target height overflow");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn facing_rebuilds_orientation_from_forward_column() -> Result<()> {
        let source = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
        assert_eq!(forward(source)?, [1., 0., 0.]);
        for p in [Precision::Single, Precision::Double, Precision::Extended] {
            let m = facing(
                forward(source)?,
                0.,
                p,
                super::super::pc_float::FacingMath::X87,
            )?;
            assert!((m[2] - 1.).abs() < 1e-6);
            assert!(m[5].abs() < 1e-6);
            assert!(m[8].abs() < 1e-6);
        }
        assert!(facing(
            [f32::NAN, 0., 1.],
            0.,
            Precision::Double,
            super::super::pc_float::FacingMath::X87
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn ground_helpers_distinguish_absence_support_and_air_states() {
        assert!(!on_real_ground(None));
        assert!(on_custom_ground(true, None, &[]));
        assert!(!on_custom_ground(false, None, &[0]));
        assert!(on_real_ground(Some(GroundMotion {
            support_type: 0,
            state: 3,
            field288: 1.
        })));
        assert!(!on_real_ground(Some(GroundMotion {
            support_type: 2,
            state: 1,
            field288: 0.
        })));
        assert!(!on_real_ground(Some(GroundMotion {
            support_type: 2,
            state: 7,
            field288: 0.
        })));
    }
    struct ResetFixture {
        present: bool,
        fail: bool,
        calls: Vec<&'static str>,
    }
    impl ResetHost for ResetFixture {
        fn current_point(&mut self) -> Result<Option<([f32; 3], [f32; 3])>> {
            Ok(self.present.then_some(([1., 2., 3.], [0., 0.7, 0.])))
        }
        fn publish_translation(&mut self, _: [f32; 3]) -> Result<()> {
            self.calls.push("publish");
            ensure!(!self.fail, "publication failed");
            Ok(())
        }
        fn target_forward(&mut self) -> Result<[f32; 3]> {
            self.calls.push("forward");
            Ok([0., 0., 1.])
        }
        fn set_facing(&mut self, f: [f32; 3], r: f32) -> Result<()> {
            assert_eq!(f, [0., 0., 1.]);
            assert_eq!(r, 0.);
            self.calls.push("facing");
            Ok(())
        }
    }
    #[test]
    fn target_events_preserve_pc_ids_and_ground_boundary() -> Result<()> {
        let mut h = ResetFixture {
            present: true,
            fail: false,
            calls: vec![],
        };
        let mut state = ResetState::run(2, &mut h)?.unwrap();
        let event = TargetEvent {
            id: 0x29,
            payload14: true,
            payload15: false,
        };
        assert!(state.target_event(event)?);
        assert!(state.flags_84_85[1]);
        state.ground_target = 0.01;
        state.height = 0.;
        assert!(!state.target_event(event)?);
        let before = state.flags_11c_11f;
        state.target_event(TargetEvent { id: 0x2a, ..event })?;
        assert_eq!(state.flags_11c_11f, before);
        state.target_event(TargetEvent { id: 0xb2, ..event })?;
        assert_eq!(state.external_ground[1], f32::MAX);
        assert!(state.flags_11c_11f[3]);
        state.target_event(TargetEvent { id: 0xb3, ..event })?;
        assert!(!state.flags_11c_11f[3]);
        Ok(())
    }
    #[test]
    fn reset_preserves_missing_target_and_requires_publication_before_facing() -> Result<()> {
        let mut h = ResetFixture {
            present: false,
            fail: false,
            calls: vec![],
        };
        assert!(ResetState::run(2, &mut h)?.is_none());
        assert!(h.calls.is_empty());
        h.present = true;
        h.fail = true;
        assert!(ResetState::run(2, &mut h).is_err());
        assert_eq!(h.calls, ["publish"]);
        h.calls.clear();
        h.fail = false;
        let state = ResetState::run(2, &mut h)?.unwrap();
        assert_eq!(h.calls, ["publish", "forward", "facing"]);
        assert_eq!(state.history, vec![[1., 2., 3.]; 2]);
        assert_eq!(state.external_ground, [f32::MAX; 3]);
        assert_eq!(state.height, 2.);
        Ok(())
    }
    #[test]
    fn tracker_offsets_use_first_matching_state_and_no_entity_scale() -> Result<()> {
        let p = Parameters {
            buffer_size: 2,
            offset: [0., 0.7, 0.],
            special_states: vec![8, 8],
            special_offsets: vec![[-0.02, 1.7, 0.25], [99.; 3]],
        };
        let rotation = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
        for precision in [Precision::Single, Precision::Double, Precision::Extended] {
            assert_eq!(
                p.current_point([1., 2., 3.], rotation, None, precision)?.0,
                [1., 2.7, 3.]
            );
            let (point, offset) = p.current_point([1., 2., 3.], rotation, Some(8), precision)?;
            assert_eq!(offset, [0.25, 1.7, 0.02]);
            assert_eq!(point, [1.25, 3.7, 3.02]);
        }
        Ok(())
    }
    #[test]
    fn native_working_vectors_distinguish_ground_branches_and_missing_rigid_write() -> Result<()> {
        let p = [1., 2., 3., 4.];
        let rigid = [5., 6., 7., 0.];
        let threshold = f32::from_bits(0x7c1071db);
        let a = working(
            p,
            Some(TrackerSnapshot {
                external_ground: threshold,
                ground_offset: 0.7,
                rigid: Some(rigid),
            }),
            99.,
        )?;
        assert_eq!(a.translation, p);
        assert_eq!(a.origin, p);
        assert_eq!(a.target, [1., 2.7, 3., 4.]);
        assert_eq!(a.rigid, rigid);
        assert_eq!(
            working(
                p,
                Some(TrackerSnapshot {
                    external_ground: 1.,
                    ground_offset: 99.,
                    rigid: Some(rigid)
                }),
                99.
            )?
            .target,
            p
        );
        assert_eq!(
            working(
                p,
                Some(TrackerSnapshot {
                    external_ground: threshold,
                    ground_offset: 0.,
                    rigid: None
                }),
                99.
            )?
            .rigid,
            p
        );
        assert_eq!(working(p, None, 0.5)?.target, [1., 2.5, 3., 4.]);
        Ok(())
    }
}
