//! Polar position da4bc..da5a4, target point de9a0 and jump pitch e1404.
use super::camera_collision::V;
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn actor_position(&mut self) -> Result<V>;
    fn target_offset(&mut self) -> Result<Option<[f32; 3]>>;
    fn target_position(&mut self) -> Result<Option<V>>;
    fn target_motion_state(&mut self) -> Result<Option<i32>>;
    fn target_forward(&mut self) -> Result<V>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
}
trait Math {
    fn sample(&mut self, kind: &str, args: &[f32]) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, args: &[f32]) -> Result<f32> {
        Ok(match kind {
            "sin" => super::native_angle::sin_cos_medium(args[0] as f64)?.0 as f32,
            "cos" => super::native_angle::sin_cos_medium(args[0] as f64)?.1 as f32,
            "atan2" => super::native_angle::atan2(args[0] as f64, args[1] as f64) as f32,
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
fn root_xz(v: V) -> Result<f32> {
    let squared = v[0] * v[0] + v[2] * v[2];
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Unsupported jump-pitch horizontal length"
    );
    Ok(if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    })
}
/// de9a0. Keep the output writes ordered even when a later property fails.
pub fn target_point(s: &[u8], point: &mut V, b: &mut impl Backend) -> Result<bool> {
    ensure!(s.len() >= 0x584, "Camera target-point owner too short");
    let Some(position) = b.target_position()? else {
        *point = [0.; 4];
        return Ok(false);
    };
    *point = position;
    if b.target_motion_state()? == Some(8) {
        let forward = b.target_forward()?;
        let distance = -f(s, 0x580);
        *point = std::array::from_fn(|i| forward[i].mul_add(distance, point[i]));
        point[1] += b.scalar(0x3c4)?;
    }
    Ok(true)
}
pub fn update(s: &mut [u8], prospective: V, b: &mut impl Backend) -> Result<()> {
    update_with(s, prospective, b, &mut NativeMath)
}
fn update_with(
    s: &mut [u8],
    prospective: V,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x788, "Camera jump-pitch owner too short");
    let mut actor = b.actor_position()?;
    if let Some(offset) = b.target_offset()? {
        actor[1] += offset[1];
    }
    let actor_delta: V = std::array::from_fn(|i| actor[i] - prospective[i]);
    let mut target = [0.; 4];
    target_point(s, &mut target, b)?;
    let target_delta: V = std::array::from_fn(|i| target[i] - prospective[i]);
    let actor_angle = math.sample("atan2", &[actor_delta[1], root_xz(actor_delta)?])?;
    let target_angle = math.sample("atan2", &[target_delta[1], root_xz(target_delta)?])?;
    let difference = super::camera_wrapper::angle_difference(actor_angle, target_angle)?;
    let change = super::camera_wrapper::angle_difference(difference, f(s, 0x764))?.abs();
    let modifier = b.scalar(0x770)?;
    let minimum = b.scalar(0x76c)?;
    let speed = change * modifier;
    let speed = if speed > minimum { speed } else { minimum };
    let airborne = s[0x1be] != 0;
    let rising = if airborne {
        let threshold = (f32::from_bits(0x40490fdb) * b.scalar(0x780)?) / 180.;
        difference > threshold && difference > f(s, 0x764)
    } else {
        false
    };
    let falling = if airborne && !rising {
        let threshold = (f32::from_bits(0x40490fdb) * b.scalar(0x784)?) / 180.;
        difference < threshold && difference < f(s, 0x764)
    } else {
        false
    };
    let scale = b.scalar(if rising {
        0x774
    } else if falling {
        0x77c
    } else {
        0x778
    })?;
    super::camera_attributes::coefficient(s, 0x754, speed * scale);
    let desired = if rising || falling {
        difference - f(s, 0x760)
    } else {
        -f(s, 0x760)
    };
    put(s, 0x764, difference);
    let old = f(s, 0x75c);
    let velocity = if desired.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        old + f(s, 0x754).mul_add(desired, -(f(s, 0x758) * old))
    };
    put(s, 0x75c, velocity);
    put(s, 0x760, f(s, 0x760) + velocity);
    Ok(())
}
/// Construct the real polar position first. Jump pitch updates only history/
/// orientation offset; it never replaces or moves the prospective position.
pub fn position_stage(
    s: &mut [u8],
    point: &mut V,
    scratch_w: f32,
    b: &mut impl Backend,
) -> Result<()> {
    position_with(s, point, scratch_w, b, &mut NativeMath)
}
fn position_with(
    s: &mut [u8],
    point: &mut V,
    scratch_w: f32,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x764, "Polar position owner too short");
    let phi = f32::from_bits(0x3fc90fdb) - f(s, 0x358);
    let sp = math.sample("sin", &[phi])?;
    let sy = math.sample("sin", &[f(s, 0x354)])?;
    let x = sy * (f(s, 0x350) * sp);
    let y = f(s, 0x350) * math.sample("cos", &[phi])?;
    let sp = math.sample("sin", &[phi])?;
    let cy = math.sample("cos", &[f(s, 0x354)])?;
    let offset = [x, y, cy * (f(s, 0x350) * sp), scratch_w];
    *point = std::array::from_fn(|i| f(s, 0x2d0 + i * 4) + offset[i]);
    if s[0x750] != 0 && (s[0x1be] != 0 || f(s, 0x760).abs() > f32::from_bits(0x3a83126f)) {
        update_with(s, *point, b, math)?;
    } else {
        put(s, 0x760, 0.);
    }
    Ok(())
}
struct WorldBackend<'a> {
    world: &'a super::assets::World,
    player: &'a super::physics::Player,
}
impl WorldBackend<'_> {
    fn target(&self) -> Result<Option<usize>> {
        let world = self.world;
        let profile = world
            .camera_profiles
            .extras
            .get(usize::try_from(
                world.camera_profiles.controller.selection.current,
            )?)
            .context("Jump-pitch profile absent")?;
        Ok(profile
            .target
            .filter(|id| !world.removed_entities.contains(id)))
    }
}
impl Backend for WorldBackend<'_> {
    fn actor_position(&mut self) -> Result<V> {
        let target = self
            .target()?
            .context("Jump pitch requires a tracked actor")?;
        let tracker = self
            .world
            .mickey_tracker
            .as_ref()
            .filter(|t| t.owner == target)
            .context("Jump-pitch tracked controller is unavailable")?;
        ensure!(tracker.error.is_none(), "Jump-pitch tracker update failed");
        ensure!(
            tracker.target == self.world.player_entity
                && !self.world.removed_entities.contains(&tracker.target),
            "Jump-pitch actor controller is not bound"
        );
        let entity = self
            .world
            .entities
            .get(tracker.target)
            .context("Jump-pitch actor entity missing")?;
        Ok(super::native_hk_transform::entity_position_vector(
            entity.transform[12..15].try_into().unwrap(),
        ))
    }
    fn target_offset(&mut self) -> Result<Option<[f32; 3]>> {
        let Some(target) = self.target()? else {
            return Ok(None);
        };
        let tracker = self
            .world
            .mickey_tracker
            .as_ref()
            .filter(|t| t.owner == target)
            .context("Jump-pitch TargetOffset getter is unavailable")?;
        Ok(Some(tracker.parameters.offset))
    }
    fn target_position(&mut self) -> Result<Option<V>> {
        let Some(target) = self.target()? else {
            return Ok(None);
        };
        let entity = self
            .world
            .entities
            .get(target)
            .context("Jump-pitch target missing")?;
        ensure!(
            entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSTransformationComponent"),
            "Jump-pitch target transform unavailable"
        );
        Ok(Some(super::native_hk_transform::entity_position_vector(
            entity.transform[12..15].try_into().unwrap(),
        )))
    }
    fn target_motion_state(&mut self) -> Result<Option<i32>> {
        let target = self
            .target()?
            .context("Target-point motion lookup without a target")?;
        let entity = &self.world.entities[target];
        if entity
            .components
            .iter()
            .any(|c| c.class_name == "MickeyPlayerMotionControllerComponent")
        {
            ensure!(
                target == self.world.player_entity,
                "Target-point motion actor binding unavailable"
            );
            return Ok(Some(self.player.motion_state()));
        }
        ensure!(
            !entity
                .components
                .iter()
                .any(|c| c.class_name.ends_with("MotionControllerComponent")),
            "Target-point motion controller binding unavailable"
        );
        Ok(None)
    }
    fn target_forward(&mut self) -> Result<V> {
        let target = self
            .target()?
            .context("Target-point facing without a target")?;
        let r = self.world.entities[target].runtime_rotation;
        Ok([r[2], r[5], r[8], 0.])
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(
                offset,
                0x3c4 | 0x76c | 0x770 | 0x774 | 0x778 | 0x77c | 0x780 | 0x784
            ),
            "Unexpected jump-pitch property"
        );
        super::camera_constructor::registered_slot_number(offset, &self.world.runtime_settings)
    }
}
impl super::assets::World {
    pub(super) fn construct_camera_polar_position(
        &self,
        s: &mut [u8],
        point: &mut V,
        player: &super::physics::Player,
        scratch_w: f32,
    ) -> Result<()> {
        let selected = i32::from_be_bytes(
            s.get(0x14..0x18)
                .context("Polar profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Polar position/profile owner mismatch"
        );
        position_stage(
            s,
            point,
            scratch_w,
            &mut WorldBackend {
                world: self,
                player,
            },
        )
    }
    /// Ordinary generator continuation after input/screen-steering/filter
    /// stage. Earlier acquisition/modes and application publication stay outside.
    pub(super) fn complete_camera_post_input(
        &self,
        s: &mut [u8],
        information: &mut super::camera_base::Information,
        player: &super::physics::Player,
        gates: super::camera_yaw_pass::Gates,
        delta: f32,
        scratch_w: f32,
        entries: &mut Vec<super::camera_distance::LookThrough>,
    ) -> Result<()> {
        self.apply_camera_rotation_input(s, *information, gates)?;
        let mut prospective = [0.; 4];
        self.construct_camera_polar_position(s, &mut prospective, player, scratch_w)?;
        self.camera_complete_generated_placement(
            s,
            information,
            &mut prospective,
            gates,
            delta,
            scratch_w,
            entries,
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
    impl Host<'_> {
        fn event(&mut self, name: &str) -> Result<()> {
            self.events.push(json!({"op":name}));
            ensure!(self.case["fail"] != name, "supplied {name} failure");
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn actor_position(&mut self) -> Result<V> {
            self.event("actor")?;
            Ok(serde_json::from_value(self.case["actor"].clone())?)
        }
        fn target_offset(&mut self) -> Result<Option<[f32; 3]>> {
            self.event("offset")?;
            Ok(if self.case["target"].is_null() {
                None
            } else {
                Some(serde_json::from_value(self.case["offset"].clone())?)
            })
        }
        fn target_position(&mut self) -> Result<Option<V>> {
            self.event("target")?;
            Ok(if self.case["target"].is_null() {
                None
            } else {
                Some(serde_json::from_value(self.case["target"].clone())?)
            })
        }
        fn target_motion_state(&mut self) -> Result<Option<i32>> {
            self.event("motion")?;
            Ok(self.case["motion"].as_i64().map(|v| v as i32))
        }
        fn target_forward(&mut self) -> Result<V> {
            self.event("forward")?;
            Ok(serde_json::from_value(self.case["forward"].clone())?)
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
        fn sample(&mut self, kind: &str, args: &[f32]) -> Result<f32> {
            let call = self
                .calls
                .next()
                .context("Unexpected jump-pitch math call")?;
            assert_eq!(call["kind"], kind);
            assert_eq!(
                json!(args.iter().map(|v| v.to_bits()).collect::<Vec<_>>()),
                call["args_bits"]
            );
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
    fn jump_springs_target_point_and_position_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_jump_pitch.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut expected = initial(case);
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            for direct in [false, true] {
                let mut s = initial(case);
                let mut point: V = serde_json::from_value(case["point"].clone())?;
                let mut host = Host {
                    case,
                    events: Vec::new(),
                };
                let mut math = Replay {
                    calls: case["math"].as_array().unwrap().iter(),
                };
                let result = match case["mode"].as_str().unwrap() {
                    "point" => target_point(&s, &mut point, &mut host).map(Some),
                    "worker" => (if direct {
                        update(&mut s, point, &mut host)
                    } else {
                        update_with(&mut s, point, &mut host, &mut math)
                    })
                    .map(|_| None),
                    "position" => (if direct {
                        position_stage(
                            &mut s,
                            &mut point,
                            case["scratch_w"].as_f64().unwrap() as f32,
                            &mut host,
                        )
                    } else {
                        position_with(
                            &mut s,
                            &mut point,
                            case["scratch_w"].as_f64().unwrap() as f32,
                            &mut host,
                            &mut math,
                        )
                    })
                    .map(|_| None),
                    _ => unreachable!(),
                };
                assert_eq!(result.is_err(), !case["error"].is_null());
                if let Ok(returned) = result {
                    assert_eq!(json!(returned), case["returned"]);
                }
                assert_eq!(
                    json!(point.map(f32::to_bits)),
                    case["point_bits"],
                    "direct={direct} {case}"
                );
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
                    "Jump-pitch differences {differences:?}; direct={direct} {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_post_input_composes_rotation_jump_and_real_placement() -> Result<()> {
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
        let count = world.camera_pipeline.publications;
        let player =
            super::super::physics::Player::new(world.report.spawn, world.report.motion.clone());
        let target = world
            .mickey_tracker
            .as_ref()
            .context("GV tracker missing")?
            .state
            .position;
        let mut frames = Vec::new();
        let mut rows = Vec::new();
        for enabled in [false, true] {
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
                (0x334, 0.2),
                (0x338, 0.2),
                (0x350, 5.),
                (0x354, 0.4),
                (0x358, -0.4),
                (0x360, 4.),
                (0x364, 0.4),
                (0x368, -0.4),
                (0x384, 1.),
                (0x388, 1.),
                (0x454, 1.),
                (0x488, f32::MAX),
                (0x580, 0.35),
                (0x5a0, 5.),
                (0x5a4, 0.4),
                (0x5a8, -0.4),
                (0x5c4, f32::MAX),
                (0x75c, 0.01),
                (0x760, 0.05),
            ] {
                put(&mut s, off, value);
            }
            s[0x331] = 1;
            s[0x7f2] = 1;
            s[0x1be] = 1;
            s[0x750] = enabled as u8;
            for off in [0x2d0, 0x2e0, 0x2f0, 0x300] {
                for i in 0..3 {
                    put(&mut s, off + i * 4, target[i]);
                }
            }
            put(&mut s, 0x17c, 320.);
            put(&mut s, 0x180, 240.);
            put(&mut s, 0x198, 11.);
            let mut commands = [-0.002, 0.003];
            world.filter_camera_commands(&mut s, &mut commands, [640, 480], super::super::STEP)?;
            assert_eq!(f(&s, 0x198), 0.);
            let smoothing = super::super::camera_constructor::registered_slot_number(
                0x178,
                &world.runtime_settings,
            )?;
            assert_eq!(f(&s, 0x170).to_bits(), (smoothing * commands[0]).to_bits());
            assert_eq!(f(&s, 0x174).to_bits(), (smoothing * commands[1]).to_bits());
            let mut information = original;
            let mut entries = Vec::new();
            world.complete_camera_post_input(
                &mut s,
                &mut information,
                &player,
                super::super::camera_yaw_pass::Gates {
                    target_moved: false,
                    input_hold_elapsed: false,
                },
                super::super::STEP,
                0.,
                &mut entries,
            )?;
            assert!(entries.is_empty());
            assert!(information.transform.iter().all(|v| v.is_finite()));
            assert_eq!(
                information.field_of_view.to_bits(),
                original.field_of_view.to_bits()
            );
            assert_eq!(
                information.focus.map(f32::to_bits),
                original.focus.map(f32::to_bits)
            );
            super::super::nif_camera::Frame::from_information(
                information,
                world.camera_lens.sample(information.field_of_view),
            )?
            .projection()?;
            if enabled {
                ensure!(
                    f(&s, 0x760) != 0.,
                    "Jump-pitch source/worker was not consumed by post-input placement"
                );
            } else {
                assert_eq!(f(&s, 0x760), 0.);
            }
            rows.push(json!({"jump_enabled":enabled,"jump_offset":f(&s,0x760),"difference_history":f(&s,0x764),"transform":information.transform}));
            frames.push(information);
        }
        assert_eq!(
            frames[0].transform[12..15],
            frames[1].transform[12..15],
            "Jump pitch must not change placement position"
        );
        assert_ne!(
            frames[0].transform[..12],
            frames[1].transform[..12],
            "Final orientation did not consume jump pitch"
        );
        // The same World bridge must honor configuration and the native bit8
        // override before touching screen-border properties.
        world
            .runtime_settings
            .insert("PlayerMouseSteering".into(), "false".into());
        let mut input = vec![0; 0x800];
        put(&mut input, 0x198, 11.);
        put(&mut input, 0x17c, f32::NAN);
        let mut commands = [0.; 2];
        world.filter_camera_commands(&mut input, &mut commands, [640, 480], super::super::STEP)?;
        assert_eq!(f(&input, 0x198), 0.);
        assert_eq!(commands, [0.; 2]);
        input[0x634..0x638].copy_from_slice(&8u32.to_be_bytes());
        world
            .runtime_settings
            .insert("PlayerMouseSteering".into(), "not-a-boolean".into());
        world
            .runtime_settings
            .insert("FreeCamera_ScreenSteer_BorderX".into(), "50".into());
        world
            .runtime_settings
            .insert("FreeCamera_ScreenSteer_BorderY".into(), "50".into());
        world
            .runtime_settings
            .insert("FreeCamera_ScreenSteer_BorderPushX".into(), "2".into());
        world
            .runtime_settings
            .insert("FreeCamera_ScreenSteer_BorderPushY".into(), "0.5".into());
        let delay = super::super::camera_constructor::registered_slot_number(
            0x6cc,
            &world.runtime_settings,
        )?;
        put(&mut input, 0x198, delay + 0.5);
        put(&mut input, 0x17c, 25.);
        put(&mut input, 0x180, 25.);
        world.filter_camera_commands(&mut input, &mut commands, [640, 480], super::super::STEP)?;
        assert_eq!(
            commands.map(f32::to_bits),
            [0.005f32.to_bits(), (-0.00125f32).to_bits()]
        );
        assert_eq!(world.camera_pipeline.output()?, original);
        assert_eq!(world.camera_pipeline.publications, count);
        std::fs::write(
            "target/camera-post-input-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Controlled initialized commands through screen steering/filtering, actual GV target/actor data, live settings, rotation, jump offset, obstruction distance and orientation. Incoming authored camera remains selected; not full generator scheduling or a gameplay camera frame.",
            "cases":rows,"screen_steering_disabled_reset":true,"screen_steering_mask_override":true,
            "live_border_push_commands":commands,"authored_publication_preserved":world.nif_camera.is_some()}))?,
        )?;
        Ok(())
    }
}
