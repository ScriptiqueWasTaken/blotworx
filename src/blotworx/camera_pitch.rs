//! deadc..deff0: input/automatic pitch, native spring caches and snap history.
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn dpad_steering(&mut self) -> Result<bool>;
    fn profile(&mut self, lane: usize) -> Result<f32>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
}
trait Math {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
        Ok(match kind {
            "sin" => super::native_angle::sin_cos_medium(value as f64)?.0 as f32,
            "cos" => super::native_angle::sin_cos_medium(value as f64)?.1 as f32,
            "asin" => super::native_angle::asin(value as f64) as f32,
            _ => unreachable!(),
        })
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
#[derive(Clone, Copy, Debug)]
pub struct PitchFrame {
    pub horizontal_change: f32,
    pub automatic_allowed: bool,
    pub adjustment: f32,
}
/// da23c..da3b8, after input/screen-steering/filter. CameraInformation
/// is the incoming view, not a newly generated orbit position.
pub fn input_stage(
    s: &mut [u8],
    information: super::camera_base::Information,
    gates: super::camera_yaw_pass::Gates,
    b: &mut impl Backend,
) -> Result<PitchFrame> {
    input_stage_with(s, information, gates, b, &mut NativeMath)
}
fn input_stage_with(
    s: &mut [u8],
    information: super::camera_base::Information,
    gates: super::camera_yaw_pass::Gates,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<PitchFrame> {
    ensure!(s.len() >= 0x648, "Camera pitch input-stage owner too short");
    let cos = math.sample("cos", f(s, 0x368))?;
    let dz = f(s, 0x2d8) - information.transform[14];
    let dx = f(s, 0x2d0) - information.transform[12];
    let old_horizontal = cos * f(s, 0x360);
    let dpad = b.dpad_steering()?;
    let squared = dx * dx + dz * dz;
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Unsupported camera horizontal distance"
    );
    let distance = if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    };
    let horizontal_change = distance - old_horizontal;
    let eligible = gates.target_moved || f(s, 0x4f4) > 0.;
    let automatic_allowed = eligible
        && (if dpad {
            gates.input_hold_elapsed
        } else {
            s[0x63c] == 0
        })
        && s[0x645] == 0;
    let adjustment = adjust_with(
        s,
        f(s, 0x174),
        horizontal_change,
        automatic_allowed,
        b,
        math,
    )?;
    put(s, 0x358, f(s, 0x358) + adjustment);
    Ok(PitchFrame {
        horizontal_change,
        automatic_allowed,
        adjustment,
    })
}
pub fn adjust(
    s: &mut [u8],
    input: f32,
    horizontal_change: f32,
    allow_auto: bool,
    b: &mut impl Backend,
) -> Result<f32> {
    adjust_with(s, input, horizontal_change, allow_auto, b, &mut NativeMath)
}
fn adjust_with(
    s: &mut [u8],
    input: f32,
    horizontal_change: f32,
    allow_auto: bool,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<f32> {
    ensure!(s.len() >= 0x638, "Camera pitch owner too short");
    let base_pitch = f(s, 0x5a8);
    let snap = s[0x464] != 0;
    let spin = s[0x1bc] != 0;
    let automatic = snap
        || ((allow_auto || spin)
            && s[0x1c0] == 0
            && u32::from_be_bytes(s[0x634..0x638].try_into().unwrap()) & 2 == 0);
    if automatic {
        let error = f(s, 0x380);
        if error > 0. {
            let maximum_step = if snap || spin {
                f32::MAX
            } else if horizontal_change > 0. {
                let previous_pitch = f(s, 0x368);
                let maximum_distance = b.profile(1)?;
                let sin = math.sample("sin", f32::from_bits(0x40490fdb) - previous_pitch)?;
                let ratio = (horizontal_change * sin) / maximum_distance;
                // Preserve bge's !LT behavior, including unordered ratios.
                let angle = if !(-1. < ratio) {
                    -f32::from_bits(0x3fc90fdb)
                } else if !(ratio < 1.) {
                    f32::from_bits(0x3fc90fdb)
                } else {
                    math.sample("asin", ratio)?
                };
                let half_pi = f32::from_bits(0x3fc90fdb);
                let candidate = half_pi - (angle + (half_pi - previous_pitch));
                let default_pitch = (f32::from_bits(0x40490fdb) * b.profile(7)?) / 180.;
                let desired = if candidate > default_pitch {
                    candidate
                } else {
                    default_pitch
                };
                (desired - previous_pitch).abs()
            } else {
                0.
            };
            let coefficient = if snap { f(s, 0x40c) } else { b.scalar(0x408)? };
            super::camera_attributes::coefficient(s, 0x3fc, coefficient);
            let old = f(s, 0x378);
            put(s, 0x404, old);
            let next = if (-error).abs() < f32::from_bits(0x38d1b717) {
                0.
            } else {
                old + f(s, 0x3fc).mul_add(-error, -(f(s, 0x400) * old))
            };
            put(s, 0x404, next);
            let limited = if next < -maximum_step {
                -maximum_step
            } else if next > maximum_step {
                maximum_step
            } else {
                next
            };
            put(s, 0x378, limited);
        } else {
            // The negative snap path updates3fc/400, then uses3ec/3f0 below.
            // This asymmetry is in the retail instructions, not a typo here.
            if snap {
                super::camera_attributes::coefficient(s, 0x3fc, f(s, 0x40c));
            } else {
                let coefficient = b.scalar(0x3f8)?;
                super::camera_attributes::coefficient(s, 0x3ec, coefficient);
            }
            let old = f(s, 0x378);
            put(s, 0x3f4, old);
            let next = if (-error).abs() < f32::from_bits(0x38d1b717) {
                0.
            } else {
                old + f(s, 0x3ec).mul_add(-error, -(f(s, 0x3f0) * old))
            };
            put(s, 0x3f4, next);
            put(s, 0x378, next);
        }
        let velocity = f(s, 0x378);
        let updated = f(s, 0x380) + velocity;
        put(s, 0x380, updated);
        let tiny = f32::from_bits(0x3a83126f);
        if input.abs() > f32::from_bits(0x3c23d70a)
            || s[0x1be] != 0
            || (s[0x464] != 0 && velocity.abs() < tiny && updated.abs() < tiny)
        {
            s[0x464] = 0;
        }
        put(s, 0x488, f32::MAX);
    } else if input.abs() > f32::from_bits(0x3a83126f) {
        put(s, 0x378, 0.);
        put(s, 0x380, f(s, 0x380) + input);
        let maximum = b.scalar(0x58c)?;
        let over_max = f(s, 0x380) + base_pitch > maximum && input > 0.;
        let under_min = if over_max {
            false
        } else {
            let minimum = b.scalar(0x590)?;
            f(s, 0x380) + base_pitch < minimum && input < 0.
        };
        // Native adds then subtracts on rejection. Reassigning the old value
        // would remove its observable single-precision rounding.
        if over_max || under_min {
            put(s, 0x380, f(s, 0x380) - input);
        }
        put(s, 0x488, f32::MAX);
    } else if f(s, 0x488) < f32::MAX {
        let angle = super::camera_wrapper::angle_difference(f(s, 0x488), f(s, 0x358))?;
        put(s, 0x380, angle);
    }
    s[0x1c0] = 0;
    Ok(f(s, 0x380))
}
struct WorldBackend<'a>(&'a super::assets::World);
impl Backend for WorldBackend<'_> {
    fn dpad_steering(&mut self) -> Result<bool> {
        super::camera_constructor::registered_boolean(
            super::camera_base::DPAD_STEERING.0,
            &self.0.runtime_settings,
        )
    }
    fn profile(&mut self, lane: usize) -> Result<f32> {
        ensure!(
            matches!(lane, 1 | 7),
            "Unexpected camera pitch profile lane"
        );
        let controller = &self.0.camera_profiles.controller;
        controller.library.evaluate(&controller.selection, lane)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(offset, 0x3f8 | 0x408 | 0x58c | 0x590),
            "Unexpected camera pitch property"
        );
        super::camera_constructor::registered_slot_number(offset, &self.0.runtime_settings)
    }
}
impl super::assets::World {
    pub(super) fn apply_camera_pitch_input(
        &self,
        state: &mut [u8],
        information: super::camera_base::Information,
        gates: super::camera_yaw_pass::Gates,
    ) -> Result<PitchFrame> {
        let selected = i32::from_be_bytes(
            state
                .get(0x14..0x18)
                .context("Pitch input profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Pitch input profile owner mismatch"
        );
        input_stage(state, information, gates, &mut WorldBackend(self))
    }
    pub(super) fn adjust_camera_pitch(
        &self,
        state: &mut [u8],
        input: f32,
        horizontal_change: f32,
        allow_auto: bool,
    ) -> Result<f32> {
        let selected = i32::from_be_bytes(
            state
                .get(0x14..0x18)
                .context("Pitch profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Pitch profile owner mismatch"
        );
        adjust(
            state,
            input,
            horizontal_change,
            allow_auto,
            &mut WorldBackend(self),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
    }
    impl Backend for Host<'_> {
        fn dpad_steering(&mut self) -> Result<bool> {
            self.events.push(json!({"op":"dpad"}));
            ensure!(self.case["fail"] != "dpad", "supplied Dpad failure");
            Ok(self.case["dpad"].as_bool().unwrap())
        }
        fn profile(&mut self, lane: usize) -> Result<f32> {
            self.events.push(json!({"op":"profile","lane":lane}));
            ensure!(
                self.case["fail"] != format!("profile{lane}"),
                "supplied profile failure"
            );
            Ok(self.case["profiles"][lane.to_string()].as_f64().unwrap() as f32)
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(
                self.case["fail"].as_u64() != Some(offset as u64),
                "supplied scalar failure"
            );
            Ok(self.case["properties"][offset.to_string()]
                .as_f64()
                .unwrap() as f32)
        }
    }
    struct Replay<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Math for Replay<'_> {
        fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
            let call = self.calls.next().context("Unexpected pitch math call")?;
            assert_eq!(call["kind"], kind);
            assert_eq!(call["input_bits"].as_u64().unwrap(), value.to_bits() as u64);
            Ok(f32::from_bits(call["result_bits"].as_u64().unwrap() as u32))
        }
    }
    fn initial(case: &Value) -> Vec<u8> {
        let mut s = vec![0xa5; 0x800];
        for p in case["initial_words"].as_array().unwrap() {
            let off = p[0].as_u64().unwrap() as usize;
            s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
        }
        s
    }
    #[test]
    fn pitch_spring_snap_limits_and_failures_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_pitch.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut expected = initial(case);
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let input = f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
            let horizontal = f32::from_bits(case["horizontal_bits"].as_u64().unwrap() as u32);
            for direct in [false, true] {
                let mut s = initial(case);
                let mut host = Host {
                    case,
                    events: Vec::new(),
                };
                let mut math = Replay {
                    calls: case["math"].as_array().unwrap().iter(),
                };
                let result = if case["pass"].as_bool().unwrap() {
                    let mut transform = [0.; 16];
                    for i in 0..4 {
                        transform[12 + i] = case["position"][i].as_f64().unwrap() as f32;
                    }
                    let information = super::super::camera_base::Information {
                        transform,
                        field_of_view: 60.,
                        focus: [0.; 4],
                    };
                    let gates = super::super::camera_yaw_pass::Gates {
                        target_moved: case["moved"].as_bool().unwrap(),
                        input_hold_elapsed: case["hold"].as_bool().unwrap(),
                    };
                    let result = if direct {
                        input_stage(&mut s, information, gates, &mut host)
                    } else {
                        input_stage_with(&mut s, information, gates, &mut host, &mut math)
                    };
                    if let Ok(frame) = &result {
                        assert_eq!(
                            json!(frame.horizontal_change.to_bits()),
                            case["pass_args"]["horizontal_bits"]
                        );
                        assert_eq!(json!(frame.automatic_allowed), case["pass_args"]["allow"]);
                    }
                    result.map(|frame| frame.adjustment)
                } else if direct {
                    adjust(
                        &mut s,
                        input,
                        horizontal,
                        case["allow"].as_bool().unwrap(),
                        &mut host,
                    )
                } else {
                    adjust_with(
                        &mut s,
                        input,
                        horizontal,
                        case["allow"].as_bool().unwrap(),
                        &mut host,
                        &mut math,
                    )
                };
                assert_eq!(result.is_err(), !case["error"].is_null());
                if let Ok(value) = result {
                    assert_eq!(
                        json!(value.to_bits()),
                        case["result_bits"],
                        "direct={direct} {case}"
                    );
                }
                if !direct {
                    assert!(math.calls.next().is_none());
                }
                assert_eq!(json!(host.events), case["events"]);
                let differences = s
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| (i, *a, *b))
                    .collect::<Vec<_>>();
                assert!(
                    differences.is_empty(),
                    "Pitch differences {differences:?}; direct={direct} {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_pitch_stage_uses_live_profiles_and_config_without_publishing() -> Result<()> {
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
        let published = world.camera_pipeline.output()?;
        let publications = world.camera_pipeline.publications;
        let make_state = |world: &super::super::assets::World, error: f32| -> Result<Vec<u8>> {
            let mut s = vec![0; 0x800];
            s[0x14..0x18].copy_from_slice(
                &world
                    .camera_profiles
                    .controller
                    .selection
                    .current
                    .to_be_bytes(),
            );
            for (off, value) in [
                (0x5a8, 0.2),
                (0x380, error),
                (0x368, 0.2),
                (0x358, 0.2),
                (0x360, 5.),
                (0x488, f32::MAX),
            ] {
                put(&mut s, off, value);
            }
            let target = world
                .mickey_tracker
                .as_ref()
                .context("GV camera target tracker missing")?
                .state
                .position;
            for i in 0..3 {
                put(&mut s, 0x2d0 + i * 4, target[i]);
            }
            Ok(s)
        };
        let mut rows = Vec::new();
        for coefficient in [0.1f32, 0.25] {
            world.runtime_settings.insert(
                "FreeCamera_Spring_RemovePitch_Down".into(),
                coefficient.to_string(),
            );
            let mut s = make_state(&world, 0.3)?;
            let result = world.adjust_camera_pitch(&mut s, 0., 0.3, true)?;
            assert_eq!(f(&s, 0x3fc).to_bits(), coefficient.to_bits());
            ensure!(
                result.is_finite() && result >= 0. && result <= 0.3,
                "Original GV pitch geometry produced invalid correction"
            );
            rows.push(json!({"branch":"automatic_positive","coefficient":coefficient,"adjustment":result,"velocity":f(&s,0x378)}));
        }
        world
            .runtime_settings
            .insert("FreeCamera_Spring_RemovePitch_Up".into(), "0.36".into());
        let mut s = make_state(&world, -0.3)?;
        let result = world.adjust_camera_pitch(&mut s, 0., 0., true)?;
        assert_eq!(f(&s, 0x3ec).to_bits(), 0.36f32.to_bits());
        assert_eq!(
            result.to_bits(),
            (-0.3 + 0.36f32.mul_add(0.3, -0.)).to_bits()
        );
        rows.push(json!({"branch":"automatic_negative","coefficient":0.36,"adjustment":result}));
        world.runtime_settings.insert(
            super::super::camera_base::DPAD_STEERING.0.into(),
            "false".into(),
        );
        world
            .runtime_settings
            .insert("FreeCamera_Limits_CameraMaxPitch".into(), "1".into());
        world
            .runtime_settings
            .insert("FreeCamera_Limits_CameraMinPitch".into(), "-1".into());
        let mut s = make_state(&world, 0.)?;
        put(&mut s, 0x174, 0.02);
        let frame = world.apply_camera_pitch_input(
            &mut s,
            published,
            super::super::camera_yaw_pass::Gates {
                target_moved: false,
                input_hold_elapsed: false,
            },
        )?;
        assert!(!frame.automatic_allowed);
        assert!(frame.horizontal_change.is_finite());
        assert_eq!(frame.adjustment.to_bits(), 0.02f32.to_bits());
        assert_eq!(f(&s, 0x358).to_bits(), (0.2f32 + 0.02).to_bits());
        rows.push(json!({"branch":"manual_input_stage","horizontal_change":frame.horizontal_change,"adjustment":frame.adjustment,"pitch":f(&s,0x358)}));
        s[0x14..0x18].copy_from_slice(
            &(world.camera_profiles.controller.selection.current + 1).to_be_bytes(),
        );
        let before = s.clone();
        assert!(world.adjust_camera_pitch(&mut s, 0., 0., true).is_err());
        assert_eq!(s, before);
        assert_eq!(world.camera_pipeline.output()?, published);
        assert_eq!(world.camera_pipeline.publications, publications);
        std::fs::write(
            "target/camera-pitch-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Controlled pitch inputs through actual GV profile evaluation, live settings and incoming authored camera information. Not the complete generator, selected gameplay output or timed handoff.",
            "cases":rows,"authored_publication_preserved":world.nif_camera.is_some()}))?,
        )?;
        Ok(())
    }
}
