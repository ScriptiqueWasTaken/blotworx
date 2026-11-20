//! d9e94..d9f34: choose retained/default yaw and obtain AutoLevel pitch.
//! Tracker resolution, screen steering and later input stages remain ordered
//! caller responsibilities; this is not a shortcut around those stages.
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn dpad_steering(&mut self) -> Result<bool>;
    fn pitch(&mut self, state: &mut [u8]) -> Result<f32>;
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
pub fn prepare(
    s: &mut [u8],
    gates: super::camera_yaw_pass::Gates,
    b: &mut impl Backend,
) -> Result<()> {
    ensure!(s.len() >= 0x640, "Camera polar owner too short");
    let retain_previous = word(s, 0x200) == 0
        && (s[0x63c] != 0
            || (b.dpad_steering()?
                && !gates.input_hold_elapsed
                && word(s, 0x62c) == 0
                && f(s, 0x5c4) == f32::MAX));
    put(s, 0x350, f(s, 0x5a0));
    put(s, 0x354, f(s, if retain_previous { 0x364 } else { 0x5a4 }));
    let pitch = b.pitch(s)?;
    put(s, 0x358, pitch);
    if retain_previous {
        put(s, 0x5a4, f(s, 0x364));
    }
    Ok(())
}
struct WorldBackend<'a, S> {
    world: &'a super::assets::World,
    source: &'a mut S,
    actor: Option<usize>,
}
impl<S: super::camera_auto_level::MotionSource> super::camera_auto_level::Backend
    for WorldBackend<'_, S>
{
    fn actor(&mut self) -> Result<bool> {
        let world = self.world;
        let profile = world
            .camera_profiles
            .extras
            .get(usize::try_from(
                world.camera_profiles.controller.selection.current,
            )?)
            .context("AutoLevel profile missing")?;
        self.actor = None;
        let Some(target) = profile
            .target
            .filter(|id| !world.removed_entities.contains(id))
        else {
            return Ok(false);
        };
        let entity = world
            .entities
            .get(target)
            .context("AutoLevel target missing")?;
        if !entity
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(false);
        }
        let tracker = world
            .mickey_tracker
            .as_ref()
            .filter(|t| t.owner == target)
            .context("AutoLevel tracker binding unavailable")?;
        ensure!(tracker.error.is_none(), "AutoLevel tracker update failed");
        if world.removed_entities.contains(&tracker.target) {
            return Ok(false);
        }
        // e550c resolves the tracker's controller through593c. Do not treat
        // another actor's missing binding as a successful null lookup.
        ensure!(
            tracker.target == world.player_entity,
            "AutoLevel tracked actor controller binding unavailable"
        );
        self.actor = Some(tracker.target);
        Ok(true)
    }
    fn motion(&mut self) -> Result<bool> {
        self.source
            .motion(self.actor.context("AutoLevel actor not acquired")?)
    }
    fn motion_vector(&mut self) -> Result<super::camera_collision::V> {
        self.source
            .motion_vector(self.actor.context("AutoLevel actor not acquired")?)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(offset, 0x744 | 0x748 | 0x74c),
            "Unexpected AutoLevel property"
        );
        super::camera_constructor::registered_slot_number(offset, &self.world.runtime_settings)
    }
}
impl<S: super::camera_auto_level::MotionSource> Backend for WorldBackend<'_, S> {
    fn dpad_steering(&mut self) -> Result<bool> {
        super::camera_constructor::registered_boolean(
            super::camera_base::DPAD_STEERING.0,
            &self.world.runtime_settings,
        )
    }
    fn pitch(&mut self, s: &mut [u8]) -> Result<f32> {
        super::camera_auto_level::pitch(s, self)
    }
}
impl super::assets::World {
    pub(super) fn prepare_player_camera_polar(
        &self,
        s: &mut [u8],
        gates: super::camera_yaw_pass::Gates,
        player: &super::physics::Player,
    ) -> Result<()> {
        struct Source<'a> {
            player: &'a super::physics::Player,
            actor: usize,
            has_motion: bool,
        }
        impl super::camera_auto_level::MotionSource for Source<'_> {
            fn motion(&mut self, actor: usize) -> Result<bool> {
                ensure!(
                    actor == self.actor,
                    "AutoLevel player motion owner mismatch"
                );
                Ok(self.has_motion)
            }
            fn motion_vector(&mut self, actor: usize) -> Result<super::camera_collision::V> {
                ensure!(
                    actor == self.actor,
                    "AutoLevel player surface owner mismatch"
                );
                self.player
                    .motion_surface_normal
                    .context("Player motion surface has not been published by character support")
            }
        }
        let has_motion = self.entities[self.player_entity]
            .components
            .iter()
            .any(|c| c.class_name == "MickeyPlayerMotionControllerComponent");
        self.prepare_camera_polar(
            s,
            gates,
            &mut Source {
                player,
                actor: self.player_entity,
                has_motion,
            },
        )
    }
    pub(super) fn prepare_camera_polar(
        &self,
        s: &mut [u8],
        gates: super::camera_yaw_pass::Gates,
        source: &mut impl super::camera_auto_level::MotionSource,
    ) -> Result<()> {
        let selected = i32::from_be_bytes(
            s.get(0x14..0x18)
                .context("Polar profile index missing")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Polar profile owner mismatch"
        );
        prepare(
            s,
            gates,
            &mut WorldBackend {
                world: self,
                source,
                actor: None,
            },
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
        fn pitch(&mut self, s: &mut [u8]) -> Result<f32> {
            self.events.push(json!({"op":"pitch","radius_bits":f(s,0x350).to_bits(),"yaw_bits":f(s,0x354).to_bits()}));
            // Supplied helper writes test aliasing and subsequent reloads.
            put(s, 0x364, f(s, 0x364) + 0.125);
            put(s, 0x73c, 0.25);
            ensure!(self.case["fail"] != "pitch", "supplied pitch failure");
            Ok(0.3125)
        }
    }
    #[test]
    fn polar_selection_and_partial_writes_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_polar_prepare.json"
        ))?;
        for case in fixture["polar"].as_array().unwrap() {
            let initial = || {
                let mut s = vec![0xa5; 0x800];
                for p in case["initial_words"].as_array().unwrap() {
                    let off = p[0].as_u64().unwrap() as usize;
                    s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
                }
                s
            };
            let mut s = initial();
            let mut expected = s.clone();
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = prepare(
                &mut s,
                super::super::camera_yaw_pass::Gates {
                    target_moved: false,
                    input_hold_elapsed: case["hold"].as_bool().unwrap(),
                },
                &mut host,
            );
            assert_eq!(result.is_err(), !case["error"].is_null());
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
                "Polar byte differences {differences:?}: {case}"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_polar_uses_live_properties_and_real_tracker_resolution() -> Result<()> {
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
        let publication = world.camera_pipeline.output()?;
        let count = world.camera_pipeline.publications;
        let tracker = world
            .mickey_tracker
            .as_ref()
            .context("GV tracker missing")?
            .owner;
        struct UnusedMotion;
        impl super::super::camera_auto_level::MotionSource for UnusedMotion {
            fn motion(&mut self, _: usize) -> Result<bool> {
                anyhow::bail!("Unexpected motion lookup in disabled/absent-actor branch")
            }
            fn motion_vector(&mut self, _: usize) -> Result<super::super::camera_collision::V> {
                anyhow::bail!("Unexpected motion vector read")
            }
        }
        let key = super::super::camera_base::DPAD_STEERING.0;
        let prior = world.runtime_settings.get(key).cloned();
        let mut rows = Vec::new();
        for tracked in [false, true] {
            let target = if tracked {
                tracker
            } else {
                world.player_entity
            };
            world.camera_profiles.assign_target(Some(target), false)?;
            for dpad in [false, true] {
                world.runtime_settings.insert(key.into(), dpad.to_string());
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
                    (0x5a0, 5.),
                    (0x5a4, 0.7),
                    (0x364, 0.4),
                    (0x5c4, f32::MAX),
                    (0x73c, 0.2),
                    (0x5a8, 0.2),
                ] {
                    put(&mut s, off, value);
                }
                s[0x738] = (!tracked) as u8;
                let coefficient = super::super::camera_constructor::registered_slot_number(
                    0x744,
                    &world.runtime_settings,
                )?;
                let history = coefficient.mul_add(-0.2, 0.2);
                let scale = super::super::camera_constructor::registered_slot_number(
                    if history > 0. { 0x748 } else { 0x74c },
                    &world.runtime_settings,
                )?;
                world.prepare_camera_polar(
                    &mut s,
                    super::super::camera_yaw_pass::Gates {
                        target_moved: false,
                        input_hold_elapsed: false,
                    },
                    &mut UnusedMotion,
                )?;
                assert_eq!(f(&s, 0x350), 5.);
                assert_eq!(f(&s, 0x354), if dpad { 0.4 } else { 0.7 });
                assert_eq!(
                    f(&s, 0x358).to_bits(),
                    history.mul_add(scale, 0.2).to_bits()
                );
                assert_eq!(f(&s, 0x73c).to_bits(), history.to_bits());
                rows.push(json!({"tracked_actor":tracked,"dpad_steering":dpad,"polar":[f(&s,0x350),f(&s,0x354),f(&s,0x358)],"slope_history":f(&s,0x73c)}));
            }
        }
        world.camera_profiles.assign_target(Some(tracker), false)?;
        if let Some(value) = prior {
            world.runtime_settings.insert(key.into(), value);
        } else {
            world.runtime_settings.remove(key);
        }
        assert_eq!(world.camera_pipeline.output()?, publication);
        assert_eq!(world.camera_pipeline.publications, count);
        std::fs::write(
            "target/camera-polar-prepare-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Controlled polar inputs through actual GV target/tracker resolution and live settings. Disabled or genuinely absent actor branches; no supplied motion vector, full generator or gameplay parity claim.",
            "cases":rows,"authored_camera_preserved":world.nif_camera.is_some()}))?,
        )?;
        Ok(())
    }
}
