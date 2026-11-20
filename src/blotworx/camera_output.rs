//! Retail daa04..dab88: final placement, orientation dispatch and history.
//! Requires real initialized caller state; not construction or full d9650.
use super::{camera_base::Information, camera_collision::V};
use anyhow::{ensure, Context, Result};
pub trait Backend {
    /// None is the actual absent current-profile entity. An unavailable vector
    /// getter is an error, never a made-up zero TargetOffset.
    fn target_offset(&mut self) -> Result<Option<[f32; 3]>>;
    fn orient(&mut self, state: &[u8], information: &mut Information, unfiltered: V) -> Result<()>;
}
trait Math {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
        let (sin, cos) = super::native_angle::sin_cos_medium(value as f64)?;
        Ok(if kind == "sin" {
            sin as f32
        } else {
            cos as f32
        })
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn vector(s: &[u8], off: usize) -> V {
    std::array::from_fn(|i| f(s, off + 4 * i))
}
/// e2508 consumes Y from the TargetOffset getter's XYZ destination. It uses
/// cos(pi/2+pitch), including negative offsets/radii; no positivity clamp.
fn restrict(s: &mut [u8], b: &mut impl Backend, math: &mut impl Math) -> Result<bool> {
    let pitch = f(s, 0x358);
    ensure!(pitch.is_finite(), "Nonfinite native camera placement pitch");
    if pitch > f32::from_bits(0xba83126f) {
        return Ok(false);
    }
    let Some(offset) = b.target_offset()? else {
        return Ok(false);
    };
    ensure!(
        offset[1].is_finite(),
        "Nonfinite native camera TargetOffset Y"
    );
    let cosine = math.sample("cos", f32::from_bits(0x3fc90fdb) + pitch)?;
    let limit = offset[1] / cosine;
    // Native comparisons preserve +/-infinity here. NaN payload/FPSCR behavior
    // is not established, so do not invent a finite fallback in that case.
    ensure!(
        !limit.is_nan(),
        "Native camera TargetOffset/cosine produced unsupported NaN"
    );
    if limit < f(s, 0x350) {
        put(s, 0x350, limit);
        Ok(true)
    } else {
        Ok(false)
    }
}
/// Caller supplies the original scratch W carried into the XYZ polar writes.
/// It is not legal to replace this with an invented homogeneous coordinate.
pub fn finish(
    s: &mut [u8],
    information: &mut Information,
    delta: f32,
    scratch_w: f32,
    b: &mut impl Backend,
) -> Result<()> {
    finish_with(s, information, delta, scratch_w, b, &mut NativeMath)
}
fn finish_with(
    s: &mut [u8],
    information: &mut Information,
    delta: f32,
    scratch_w: f32,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x7f4, "Native camera output owner too short");
    ensure!(delta.is_finite(), "Nonfinite native camera output delta");
    if s[0x331] != 0 {
        let current = f(s, 0x350);
        let previous = f(s, 0x360);
        ensure!(
            [current, previous].into_iter().all(f32::is_finite),
            "Nonfinite delayed camera radius"
        );
        if current > previous {
            let elapsed = f(s, 0x334) + delta;
            put(s, 0x334, elapsed);
            let duration = f(s, 0x338);
            ensure!(
                elapsed.is_finite() && duration.is_finite(),
                "Nonfinite camera distance delay"
            );
            put(
                s,
                0x350,
                if elapsed >= duration {
                    f32::from_bits(0x3d4ccccd).mul_add(current - previous, previous)
                } else {
                    previous
                },
            );
        } else {
            put(s, 0x334, 0.);
        }
    }
    let gate = f(s, 0x4f4);
    ensure!(gate.is_finite(), "Nonfinite camera radius restriction gate");
    s[0x7f3] = if gate <= 0. {
        restrict(s, b, math)? as u8
    } else {
        0
    };
    let phi = f32::from_bits(0x3fc90fdb) - f(s, 0x358);
    let sp = math.sample("sin", phi)?;
    let sy = math.sample("sin", f(s, 0x354))?;
    let x = sy * (f(s, 0x350) * sp);
    let y = f(s, 0x350) * math.sample("cos", phi)?;
    let sp = math.sample("sin", phi)?;
    let cy = math.sample("cos", f(s, 0x354))?;
    let z = cy * (f(s, 0x350) * sp);
    let target = vector(s, 0x2d0);
    let offset = [x, y, z, scratch_w];
    for i in 0..4 {
        information.transform[12 + i] = target[i] + offset[i];
    }
    b.orient(s, information, vector(s, 0x2f0))?;
    s.copy_within(0x2d0..0x2e0, 0x2e0);
    s.copy_within(0x350..0x360, 0x360);
    if s[0x1be] == 0 {
        s[0x7f2] = 0;
    }
    Ok(())
}
struct WorldBackend<'a>(&'a super::assets::World);
impl Backend for WorldBackend<'_> {
    fn target_offset(&mut self) -> Result<Option<[f32; 3]>> {
        let world = self.0;
        let current = usize::try_from(world.camera_profiles.controller.selection.current)?;
        let profile = world
            .camera_profiles
            .extras
            .get(current)
            .context("Camera output current profile missing")?;
        let Some(owner) = profile
            .target
            .filter(|i| *i < world.entities.len() && !world.removed_entities.contains(i))
        else {
            return Ok(None);
        };
        if !world.entities[owner]
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(None);
        }
        // The verified target in the default controller is the actual tracker.
        // Other entity getter/default adapters still require reconstruction.
        let tracker = world
            .mickey_tracker
            .as_ref()
            .filter(|t| t.owner == owner)
            .context("Camera TargetOffset getter is not bound for this profile target")?;
        Ok(Some(tracker.parameters.offset))
    }
    fn orient(&mut self, s: &[u8], information: &mut Information, unfiltered: V) -> Result<()> {
        self.0.camera_orient_information(s, information, unfiltered)
    }
}
impl super::assets::World {
    /// Joins final placement with the real orientation backend and history.
    /// The manager must supply native scalar ownership and schedule publication.
    pub(super) fn camera_finish_placement(
        &self,
        s: &mut [u8],
        information: &mut Information,
        delta: f32,
        scratch_w: f32,
    ) -> Result<()> {
        ensure!(s.len() >= 0x7f4, "Native camera output owner too short");
        ensure!(
            i32::from_be_bytes(s[0x14..0x18].try_into().unwrap())
                == self.camera_profiles.controller.selection.current,
            "Camera output scalar/profile owner mismatch"
        );
        finish(s, information, delta, scratch_w, &mut WorldBackend(self))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Host<'a> {
        case: &'a serde_json::Value,
        events: Vec<String>,
        snapshots: Vec<Vec<u32>>,
    }
    impl Backend for Host<'_> {
        fn target_offset(&mut self) -> Result<Option<[f32; 3]>> {
            self.events.push("target".into());
            if !self.case["target"].as_bool().unwrap() {
                return Ok(None);
            }
            self.events.push("offset".into());
            ensure!(self.case["fail"] != "offset", "Fixture offset failure");
            Ok(Some(std::array::from_fn(|i| {
                self.case["offset"][i].as_f64().unwrap() as f32
            })))
        }
        fn orient(&mut self, _: &[u8], information: &mut Information, unfiltered: V) -> Result<()> {
            assert_eq!(unfiltered, [4., 5., 6., 11.]);
            self.events.push("orient".into());
            self.snapshots
                .push(information.transform.map(f32::to_bits).to_vec());
            ensure!(self.case["fail"] != "orient", "Fixture orientation failure");
            information.transform[..4].copy_from_slice(&[0.5, 0.25, 0.75, -0.]);
            Ok(())
        }
    }
    struct Replay<'a> {
        calls: &'a [serde_json::Value],
        next: usize,
    }
    impl Math for Replay<'_> {
        fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
            let c = &self.calls[self.next];
            self.next += 1;
            assert_eq!(c["kind"], kind);
            assert_eq!(c["input_bits"].as_u64().unwrap(), (value as f64).to_bits());
            Ok(f32::from_bits(c["output_bits"].as_u64().unwrap() as u32))
        }
    }
    #[test]
    fn production_trig_matches_supplied_output_reference_leaves() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_output.json"
        ))?;
        let mut math = NativeMath;
        for c in fixture["cases"].as_array().unwrap() {
            for call in c["math_calls"].as_array().unwrap() {
                let input = f64::from_bits(call["input_bits"].as_u64().unwrap());
                assert_eq!(input, (input as f32) as f64);
                let kind = call["kind"].as_str().unwrap();
                let answer = math.sample(kind, input as f32)?;
                assert_eq!(
                    answer.to_bits(),
                    call["output_bits"].as_u64().unwrap() as u32,
                    "{kind}({input})"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn final_placement_history_and_failure_prefixes_match_original() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_output.json"
        ))?;
        for (n, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut s = vec![0x33; 0x800];
            s[0x331] = c["delay"].as_bool().unwrap() as u8;
            s[0x1be] = c["airborne"].as_bool().unwrap() as u8;
            s[0x7f2] = 1;
            for (off, values) in [
                (
                    0x334,
                    vec![
                        c["elapsed"].as_f64().unwrap() as f32,
                        c["duration"].as_f64().unwrap() as f32,
                    ],
                ),
                (
                    0x350,
                    vec![
                        c["current"].as_f64().unwrap() as f32,
                        0.4,
                        c["pitch"].as_f64().unwrap() as f32,
                        19.,
                    ],
                ),
                (
                    0x360,
                    vec![c["previous"].as_f64().unwrap() as f32, -0.3, 0.2, 23.],
                ),
                (0x2d0, vec![2., 3., 4., 7.]),
                (0x2e0, vec![-2., -3., -4., -7.]),
                (0x2f0, vec![4., 5., 6., 11.]),
                (0x4f4, vec![c["gate"].as_f64().unwrap() as f32]),
            ] {
                for (i, v) in values.into_iter().enumerate() {
                    put(&mut s, off + 4 * i, v);
                }
            }
            let mut expected = s.clone();
            for (off, v) in c["changes"].as_object().unwrap() {
                expected[off.parse::<usize>()?] = v.as_u64().unwrap() as u8;
            }
            let mut information = Information {
                transform: [
                    1., 0., 0., 13., 0., 1., 0., 17., 0., 0., 1., 19., -5., -6., -7., 23.,
                ],
                field_of_view: 55.,
                focus: [1., 2., 3., 4.],
            };
            let mut b = Host {
                case: c,
                events: vec![],
                snapshots: vec![],
            };
            let mut math = Replay {
                calls: c["math_calls"].as_array().unwrap(),
                next: 0,
            };
            let result = finish_with(
                &mut s,
                &mut information,
                c["delta"].as_f64().unwrap() as f32,
                29.,
                &mut b,
                &mut math,
            );
            assert_eq!(
                result.is_err(),
                !c["stopped"].is_null(),
                "Case {n}: {result:?}"
            );
            assert_eq!(s, expected, "Retained state case {n}");
            assert_eq!(
                serde_json::json!(information.transform.map(f32::to_bits)),
                c["output_bits"],
                "Output case {n}"
            );
            assert_eq!(serde_json::json!(b.events), c["events"], "Events case {n}");
            assert_eq!(
                serde_json::json!(b.snapshots),
                c["snapshots"],
                "Snapshots case {n}"
            );
            assert_eq!(math.next, math.calls.len());
            assert_eq!(information.field_of_view, 55.);
            assert_eq!(information.focus, [1., 2., 3., 4.]);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_final_placement_uses_tracker_offset_real_orientation_and_preserves_publication(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        for tick in 1..=6 {
            world
                .advance_models_with_delta(tick as f32 * super::super::STEP, super::super::STEP)?;
        }
        let original = world.camera_pipeline.output()?;
        let publications = world.camera_pipeline.publications;
        let tracker = world
            .mickey_tracker
            .as_ref()
            .context("Original MickeyTracker absent")?;
        let owner = tracker.owner;
        let position = tracker.state.position;
        let offset_y = tracker.parameters.offset[1];
        ensure!(offset_y > 0., "Original tracker offset not positive");
        world.camera_profiles.assign_target(Some(owner), false)?;
        let mut outputs = vec![];
        let mut bypass_state = None;
        for gate in [0., 1.] {
            let mut s = vec![0; 0x800];
            s[0x14..0x18].copy_from_slice(
                &world
                    .camera_profiles
                    .controller
                    .selection
                    .current
                    .to_be_bytes(),
            );
            s[0x331] = 1;
            s[0x7f2] = 1;
            for (off, v) in [
                (0x334, 0.2),
                (0x338, 0.2),
                (0x350, 5.),
                (0x354, 0.4),
                (0x358, -0.4),
                (0x360, 4.),
                (0x384, 1.),
                (0x4f4, gate),
            ] {
                put(&mut s, off, v);
            }
            for off in [0x2d0, 0x2f0, 0x300] {
                for i in 0..3 {
                    put(&mut s, off + 4 * i, position[i]);
                }
            }
            if gate == 1. {
                bypass_state = Some(s.clone());
            }
            let mut information = original;
            world.camera_finish_placement(&mut s, &mut information, super::super::STEP, 0.)?;
            assert_eq!(s[0x7f3], (gate == 0.) as u8);
            assert_eq!(s[0x7f2], 0);
            assert_eq!(&s[0x360..0x370], &s[0x350..0x360]);
            assert_eq!(&s[0x2e0..0x2f0], &s[0x2d0..0x2e0]);
            assert!(information.transform.iter().all(|v| v.is_finite()));
            let direction = &information.transform[8..11];
            let squared = direction.iter().map(|v| v * v).sum::<f32>();
            assert!(
                (squared - 1.).abs() < 0.000002,
                "Direction must be normalized by real orientation"
            );
            assert_eq!(
                information.field_of_view.to_bits(),
                original.field_of_view.to_bits()
            );
            assert_eq!(
                information.focus.map(f32::to_bits),
                original.focus.map(f32::to_bits)
            );
            let frame = super::super::nif_camera::Frame::from_information(
                information,
                world.camera_lens.sample(information.field_of_view),
            )?;
            assert!(frame.view().iter().all(|v| v.is_finite()));
            assert!(frame.projection()?.iter().all(|v| v.is_finite()));
            outputs.push((f(&s, 0x350), information));
        }
        assert!(
            outputs[0].0 < outputs[1].0,
            "Actual TargetOffset must reduce downward placement radius"
        );
        assert_ne!(
            outputs[0].1.transform[12..15],
            outputs[1].1.transform[12..15]
        );
        world
            .camera_profiles
            .assign_target(Some(world.player_entity), false)?;
        let mut s = bypass_state.unwrap();
        put(&mut s, 0x4f4, 0.);
        let mut direct = original;
        world.camera_finish_placement(&mut s, &mut direct, super::super::STEP, 0.)?;
        assert_eq!(
            s[0x7f3], 0,
            "10a54 declines an entity without EntityTrackerComponent"
        );
        assert_eq!(
            direct.transform.map(f32::to_bits),
            outputs[1].1.transform.map(f32::to_bits)
        );
        // Exercise the combined placement pass through its real World query,
        // property and orientation adapters. These are explicit fixture inputs,
        // not a fabricated complete d9650 update or a selected gameplay view.
        for (off, v) in [
            (0x350, 5.),
            (0x384, 1.),
            (0x388, 1.),
            (0x444, 1.),
            (0x44c, 1.),
            (0x488, f32::MAX),
            (0x580, 0.35),
        ] {
            put(&mut s, off, v);
        }
        let phi = f32::from_bits(0x3fc90fdb) - f(&s, 0x358);
        let (sin_phi, cos_phi) = super::super::native_angle::sin_cos_medium(phi as f64)?;
        let (sin_yaw, cos_yaw) = super::super::native_angle::sin_cos_medium(f(&s, 0x354) as f64)?;
        let radius = f(&s, 0x350);
        let offset = [
            sin_yaw as f32 * (radius * sin_phi as f32),
            radius * cos_phi as f32,
            cos_yaw as f32 * (radius * sin_phi as f32),
            0.,
        ];
        let prospective: V = std::array::from_fn(|i| f(&s, 0x2d0 + i * 4) + offset[i]);
        // Controlled inputs run through the actual timing stage: unchanged
        // target history, zero actor axes and no camera rotation input. These
        // are explicit fixture inputs, not guessed values from body velocity.
        s.copy_within(0x2d0..0x2e0, 0x2e0);
        put(&mut s, 0x450, 0.);
        put(&mut s, 0x454, 0.);
        put(&mut s, 0x44c, 0.);
        let activity = super::super::camera_generator_timing::advance(
            &mut s,
            [0.; 2],
            super::super::STEP,
            || Ok([0.; 2]),
        )?;
        let key = super::super::camera_base::DPAD_STEERING.0;
        let gates = super::super::camera_generator_timing::snapshot(
            &s,
            activity.target_moved,
            super::super::camera_constructor::registered_number(
                "FreeCamera_Input_HoldWait",
                &world.runtime_settings,
            )?,
        )?;
        let input_hold_elapsed = gates.input_hold_elapsed;
        let prior = world.runtime_settings.get(key).cloned();
        let mut rows = Vec::new();
        for enabled in [false, true] {
            world
                .runtime_settings
                .insert(key.into(), enabled.to_string());
            let mut state = s.clone();
            let mut candidate = prospective;
            let mut combined = original;
            let mut entries = Vec::new();
            world.camera_complete_generated_placement(
                &mut state,
                &mut combined,
                &mut candidate,
                gates,
                super::super::STEP,
                0.,
                &mut entries,
            )?;
            assert!(entries.is_empty());
            assert!(combined.transform.iter().all(|v| v.is_finite()));
            assert_eq!(
                combined.field_of_view.to_bits(),
                original.field_of_view.to_bits()
            );
            assert_eq!(
                combined.focus.map(f32::to_bits),
                original.focus.map(f32::to_bits)
            );
            if enabled && !input_hold_elapsed {
                assert_eq!(
                    candidate.map(f32::to_bits),
                    prospective.map(f32::to_bits),
                    "Held DpadSteering did not preserve the preceding prospective view"
                );
            }
            rows.push(serde_json::json!({"dpad_steering":enabled,"prospective_after_yaw":candidate,"transform":combined.transform,"distance_fraction":f(&state,0x384)}));
        }
        if let Some(value) = prior {
            world.runtime_settings.insert(key.into(), value);
        } else {
            world.runtime_settings.remove(key);
        }
        std::fs::write(
            "target/camera-placement-pass-owned.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Controlled initialized inputs/snapshots for yaw gates and combined final placement through real GV World geometry, live DpadSteering settings and orientation. Full generator/handoff remains disconnected.",
            "input_hold_elapsed":input_hold_elapsed,"prospective_before_yaw":prospective,"cases":rows,"selected_authored_camera_preserved":world.nif_camera.is_some()}))?,
        )?;
        assert_eq!(world.camera_pipeline.output()?, original);
        assert_eq!(world.camera_pipeline.publications, publications);
        Ok(())
    }
}
