//! Snap de384, auto-center de528, movement steering dde80 and their
//! da3b8..da4bc caller. The original spring coefficients are per update.
use super::camera_collision::V;
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn profile(&mut self, lane: usize) -> Result<f32>;
    fn target_forward(&mut self) -> Result<Option<[f32; 3]>>;
}
trait Math {
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32>;
    fn cos(&mut self, angle: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
        Ok(super::native_angle::atan2(y as f64, x as f64) as f32)
    }
    fn cos(&mut self, angle: f32) -> Result<f32> {
        Ok(super::native_angle::sin_cos_medium(angle as f64)?.1 as f32)
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn word(s: &[u8], off: usize) -> u32 {
    u32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn sign(value: f32) -> f32 {
    if value > 0. {
        1.
    } else if value < 0. {
        -1.
    } else {
        0.
    }
}
fn angle(target: f32, current: f32) -> Result<f32> {
    super::camera_wrapper::angle_difference(target, current)
}
pub fn snap(s: &mut [u8], current_yaw: f32, b: &mut impl Backend) -> Result<f32> {
    ensure!(s.len() >= 0x65c, "Camera snap owner too short");
    if s[0xdc] != 0 || (f(s, 0x44c) == 0. && s[0xde] == 0) {
        s[0xdd] = 0;
    }
    if s[0xdd] == 0 {
        return Ok(0.);
    }
    let difference = angle(f(s, 0xf0), current_yaw)?;
    if f(s, 0x460) == 0. {
        put(s, 0x460, difference);
    }
    if sign(f(s, 0x460)) != sign(difference) {
        s[0xdd] = 0;
        return Ok(difference);
    }
    let old = f(s, 0x374);
    put(s, 0x414, old);
    let velocity = if difference.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        old + f(s, 0x40c).mul_add(difference, -(f(s, 0x410) * old))
    };
    put(s, 0x414, velocity);
    put(s, 0x374, velocity);
    if velocity.abs() < f32::from_bits(0x3a83126f) {
        s[0xdd] = 0;
        return Ok(0.);
    }
    if f(s, 0x428) >= 0. && f(s, 0x450) - f(s, 0x424) > f(s, 0x428) {
        let padding = b.scalar(0x658)?;
        s[0x464] = 0;
        put(s, 0xf0, f(s, 0x414).mul_add(padding, f(s, 0x354)));
    }
    Ok(f(s, 0x374))
}
pub fn center(s: &mut [u8], b: &mut impl Backend) -> Result<f32> {
    center_with(s, b, &mut NativeMath)
}
fn center_with(s: &mut [u8], b: &mut impl Backend, math: &mut impl Math) -> Result<f32> {
    ensure!(s.len() >= 0x738, "Auto-center owner too short");
    let Some(forward) = b.target_forward()? else {
        return Ok(f(s, 0x354));
    };
    let heading = math.atan2(forward[0], forward[2])?;
    let difference = angle(f32::from_bits(0x40490fdb) + heading, f(s, 0x354))?;
    let tolerance = (f32::from_bits(0x40490fdb) * b.scalar(0x724)?) / 180.;
    if difference.abs() < tolerance {
        put(s, 0x734, 0.);
        return Ok(f(s, 0x354));
    }
    let coefficient = if f(s, 0x728) < f32::MAX {
        f(s, 0x728)
    } else {
        b.scalar(0x720)?
    };
    super::camera_attributes::coefficient(s, 0x72c, coefficient);
    let old = f(s, 0x734);
    let velocity = if difference.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        old + f(s, 0x72c).mul_add(difference, -(f(s, 0x730) * old))
    };
    put(s, 0x734, velocity);
    put(s, 0x354, f(s, 0x354) + velocity);
    Ok(f(s, 0x354))
}
pub fn movement(s: &mut [u8], target: V, b: &mut impl Backend) -> Result<f32> {
    movement_with(s, target, b, &mut NativeMath)
}
fn movement_with(
    s: &mut [u8],
    target: V,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<f32> {
    ensure!(s.len() >= 0x3bc, "Camera movement-yaw owner too short");
    let direction = math.atan2(target[0] - f(s, 0x2e0), target[2] - f(s, 0x2e8))?;
    let alignment = 1. - math.cos(angle(direction, f(s, 0x354))?)?.abs();
    let change = angle(f(s, 0x354), f(s, 0x364))?;
    let lower = f32::from_bits(0xbe20d97c);
    let upper = f32::from_bits(0x3e20d97c);
    let change = if change > lower { change } else { lower };
    let change = if change < upper { change } else { upper };
    let maximum = b.profile(1)?;
    let ratio = f(s, 0x360) / maximum;
    let airborne = s[0x1be] != 0;
    let (property, cache, velocity) = if airborne {
        (0x3a0, 0x3b0, 0x3b8)
    } else {
        (0x39c, 0x3a4, 0x3ac)
    };
    let coefficient = b.scalar(property)?;
    super::camera_attributes::coefficient(s, cache, coefficient);
    let desired = b.scalar(if airborne { 0x398 } else { 0x394 })?;
    let difference = desired - f(s, 0x390);
    let old = f(s, velocity);
    let next = if difference.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        old + f(s, cache).mul_add(difference, -(f(s, cache + 4) * old))
    };
    put(s, velocity, next);
    put(s, 0x390, f(s, 0x390) + next);
    Ok(ratio * (f(s, 0x390) * (alignment * change)))
}
pub fn update(s: &mut [u8], target_moved: bool, b: &mut impl Backend) -> Result<f32> {
    update_with(s, target_moved, b, &mut NativeMath)
}
fn update_with(
    s: &mut [u8],
    target_moved: bool,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<f32> {
    ensure!(s.len() >= 0x738, "Camera yaw-stage owner too short");
    if f(s, 0x5c4) == f32::MAX && word(s, 0x200) == 0 {
        if s[0xdd] != 0 {
            let delta = snap(s, f(s, 0x354), b)?;
            put(s, 0x354, f(s, 0x354) + delta);
        } else if s[0x714] != 0 {
            let wait = b.scalar(0x71c)?;
            if f(s, 0x448) >= wait {
                center_with(s, b, math)?;
            } else {
                put(s, 0x734, 0.);
            }
        }
    }
    // Re-read DD: finishing snap may admit movement steering in this call.
    if word(s, 0x200) == 0
        && s[0xdd] == 0
        && f(s, 0x5c4) == f32::MAX
        && word(s, 0x62c) == 0
        && s[0x63c] == 0
        && s[0x645] == 0
        && word(s, 0x634) & 4 == 0
        && target_moved
    {
        let target = std::array::from_fn(|i| f(s, 0x2d0 + i * 4));
        let delta = movement_with(s, target, b, math)?;
        put(s, 0x354, f(s, 0x354) + delta);
    }
    Ok(f(s, 0x354))
}
struct WorldBackend<'a>(&'a super::assets::World);
impl Backend for WorldBackend<'_> {
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(
                offset,
                0x394 | 0x398 | 0x39c | 0x3a0 | 0x658 | 0x71c | 0x720 | 0x724
            ),
            "Unexpected yaw property"
        );
        super::camera_constructor::registered_slot_number(offset, &self.0.runtime_settings)
    }
    fn profile(&mut self, lane: usize) -> Result<f32> {
        ensure!(lane == 1, "Unexpected yaw profile lane");
        let c = &self.0.camera_profiles.controller;
        c.library.evaluate(&c.selection, lane)
    }
    fn target_forward(&mut self) -> Result<Option<[f32; 3]>> {
        let world = self.0;
        let profile = world
            .camera_profiles
            .extras
            .get(usize::try_from(
                world.camera_profiles.controller.selection.current,
            )?)
            .context("Auto-center profile missing")?;
        let Some(id) = profile
            .target
            .filter(|id| !world.removed_entities.contains(id))
        else {
            return Ok(None);
        };
        let entity = world
            .entities
            .get(id)
            .context("Auto-center target missing")?;
        ensure!(
            entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSTransformationComponent"),
            "Auto-center target transform unavailable"
        );
        //297110(axis2) takes indices2/5/8 from the unscaled world rotation.
        Ok(Some([
            entity.runtime_rotation[2],
            entity.runtime_rotation[5],
            entity.runtime_rotation[8],
        ]))
    }
}
impl super::assets::World {
    pub(super) fn apply_camera_yaw_control(
        &self,
        state: &mut [u8],
        target_moved: bool,
    ) -> Result<f32> {
        let selected = i32::from_be_bytes(
            state
                .get(0x14..0x18)
                .context("Yaw profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Yaw profile owner mismatch"
        );
        update(state, target_moved, &mut WorldBackend(self))
    }
    /// Contiguous rotation stages after input ramp/screen-steering/filter. Preparing the
    /// resulting polar state does not select or publish the default camera.
    pub(super) fn apply_camera_rotation_input(
        &self,
        state: &mut [u8],
        information: super::camera_base::Information,
        gates: super::camera_yaw_pass::Gates,
    ) -> Result<(super::camera_pitch::PitchFrame, f32)> {
        let pitch = self.apply_camera_pitch_input(state, information, gates)?;
        let yaw = self.apply_camera_yaw_control(state, gates.target_moved)?;
        Ok((pitch, yaw))
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
        fn profile(&mut self, lane: usize) -> Result<f32> {
            self.events.push(json!({"op":"profile","lane":lane}));
            ensure!(self.case["fail"] != "profile", "supplied profile failure");
            Ok(self.case["profile"].as_f64().unwrap() as f32)
        }
        fn target_forward(&mut self) -> Result<Option<[f32; 3]>> {
            self.events.push(json!({"op":"target"}));
            ensure!(self.case["fail"] != "target", "supplied target failure");
            Ok(if self.case["target"].is_null() {
                None
            } else {
                Some(serde_json::from_value(self.case["target"].clone())?)
            })
        }
    }
    struct Replay<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Replay<'_> {
        fn call(&mut self, kind: &str, args: &[f32]) -> Result<f32> {
            let call = self
                .calls
                .next()
                .context("Unexpected yaw-control math call")?;
            assert_eq!(call["kind"], kind);
            assert_eq!(
                json!(args.iter().map(|v| v.to_bits()).collect::<Vec<_>>()),
                call["args_bits"]
            );
            Ok(f32::from_bits(call["result_bits"].as_u64().unwrap() as u32))
        }
    }
    impl Math for Replay<'_> {
        fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
            self.call("atan2", &[y, x])
        }
        fn cos(&mut self, angle: f32) -> Result<f32> {
            self.call("cos", &[angle])
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
    fn yaw_workers_and_ordered_caller_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_yaw_control.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut expected = initial(case);
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let target: V = serde_json::from_value(case["input_target"].clone())?;
            let yaw = f32::from_bits(case["yaw_bits"].as_u64().unwrap() as u32);
            for direct in [false, true] {
                let mut state = initial(case);
                let mut host = Host {
                    case,
                    events: Vec::new(),
                };
                let mut math = Replay {
                    calls: case["math"].as_array().unwrap().iter(),
                };
                let result = match case["mode"].as_str().unwrap() {
                    "snap" => snap(&mut state, yaw, &mut host),
                    "center" => {
                        if direct {
                            center(&mut state, &mut host)
                        } else {
                            center_with(&mut state, &mut host, &mut math)
                        }
                    }
                    "movement" => {
                        if direct {
                            movement(&mut state, target, &mut host)
                        } else {
                            movement_with(&mut state, target, &mut host, &mut math)
                        }
                    }
                    "stage" => {
                        if direct {
                            update(&mut state, case["moved"].as_bool().unwrap(), &mut host)
                        } else {
                            update_with(
                                &mut state,
                                case["moved"].as_bool().unwrap(),
                                &mut host,
                                &mut math,
                            )
                        }
                    }
                    _ => unreachable!(),
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
                let differences = state
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| (i, *a, *b))
                    .collect::<Vec<_>>();
                assert!(
                    differences.is_empty(),
                    "Yaw-control differences {differences:?}; direct={direct} {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_rotation_stages_use_live_facing_and_keep_publication() -> Result<()> {
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
        let count = world.camera_pipeline.publications;
        let make_state = |world: &super::super::assets::World| -> Result<Vec<u8>> {
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
                (0xf0, 0.2),
                (0x354, 0.4),
                (0x364, -0.2),
                (0x360, 5.),
                (0x358, 0.2),
                (0x368, 0.2),
                (0x5a8, 0.2),
                (0x488, f32::MAX),
                (0x5c4, f32::MAX),
                (0x728, f32::MAX),
                (0x448, 2.),
                (0x44c, 0.2),
                (0x428, -1.),
                (0x40c, 0.2),
                (0x410, 0.8),
            ] {
                put(&mut s, off, value);
            }
            s[0xde] = 1;
            let p = world
                .mickey_tracker
                .as_ref()
                .context("GV tracker missing")?
                .state
                .position;
            for i in 0..3 {
                put(&mut s, 0x2d0 + i * 4, p[i]);
                put(&mut s, 0x2e0 + i * 4, p[i] - if i != 1 { 0.5 } else { 0. });
            }
            Ok(s)
        };
        let mut rows = Vec::new();
        let mut s = make_state(&world)?;
        s[0xdd] = 1;
        put(&mut s, 0x460, 0.2);
        let yaw = world.apply_camera_yaw_control(&mut s, true)?;
        assert_eq!(s[0xdd], 0);
        ensure!(
            f(&s, 0x390) > 0.,
            "Completing snap did not admit movement steering"
        );
        rows.push(json!({"branch":"snap_then_movement","yaw":yaw,"movement_gain":f(&s,0x390)}));
        world
            .camera_profiles
            .assign_target(Some(world.player_entity), false)?;
        world
            .runtime_settings
            .insert("FreeCamera_AutoCenter_Wait".into(), "0".into());
        world
            .runtime_settings
            .insert("FreeCamera_AutoCenter_Speed".into(), "0.25".into());
        world
            .runtime_settings
            .insert("FreeCamera_AutoCenter_NeutralRange".into(), "0".into());
        let rotation = world.entities[world.player_entity].runtime_rotation;
        let heading =
            super::super::native_angle::atan2(rotation[2] as f64, rotation[8] as f64) as f32;
        let old = f32::from_bits(0x40490fdb) + heading - 0.5;
        let mut s = make_state(&world)?;
        s[0x714] = 1;
        put(&mut s, 0x354, old);
        let yaw = world.apply_camera_yaw_control(&mut s, false)?;
        assert_eq!(f(&s, 0x72c), 0.25);
        ensure!(yaw > old, "Auto-center did not use actual target facing");
        rows.push(json!({"branch":"live_target_center","yaw":yaw,"previous_yaw":old,"coefficient":f(&s,0x72c)}));
        world.camera_profiles.assign_target(None, false)?;
        let old = f(&s, 0x354);
        put(&mut s, 0x734, 0.123);
        assert_eq!(
            world.apply_camera_yaw_control(&mut s, false)?.to_bits(),
            old.to_bits()
        );
        assert_eq!(f(&s, 0x734), 0.123);
        world
            .runtime_settings
            .insert("FreeCamera_Limits_CameraMaxPitch".into(), "1".into());
        world
            .runtime_settings
            .insert("FreeCamera_Limits_CameraMinPitch".into(), "-1".into());
        let mut s = make_state(&world)?;
        put(&mut s, 0x174, 0.02);
        let (pitch, yaw) = world.apply_camera_rotation_input(
            &mut s,
            published,
            super::super::camera_yaw_pass::Gates {
                target_moved: false,
                input_hold_elapsed: false,
            },
        )?;
        assert_eq!(f(&s, 0x358).to_bits(), (0.2f32 + 0.02).to_bits());
        assert_eq!(yaw, 0.4);
        rows.push(json!({"branch":"combined_rotation","pitch_adjustment":pitch.adjustment,"pitch":f(&s,0x358),"yaw":yaw}));
        assert_eq!(world.camera_pipeline.output()?, published);
        assert_eq!(world.camera_pipeline.publications, count);
        std::fs::write(
            "target/camera-yaw-control-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Controlled initialized rotation inputs through actual GV facing, profile and live settings. Snap completion/movement ordering and combined pitch/yaw; not full generator or gameplay camera publication.",
            "cases":rows,"authored_publication_preserved":world.nif_camera.is_some()}))?,
        )?;
        Ok(())
    }
}
