//! Retail e6258 automatic combat/travel/default profile choice. Application
//! combat state and primary-lock identity are explicit caller-owned inputs.
use anyhow::{ensure, Context, Result};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice {
    pub mode: i32,
    pub duration: f32,
}
pub trait Backend {
    fn secondary_lock(&mut self) -> Result<bool>;
    fn boolean(&mut self, offset: usize) -> Result<bool>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn combat_active(&mut self) -> Result<bool>;
    fn actor(&mut self) -> Result<Option<usize>>;
    fn velocity(&mut self, actor: usize) -> Result<[f32; 4]>;
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn word(s: &[u8], off: usize) -> u32 {
    u32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn put_word(s: &mut [u8], off: usize, v: u32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
pub fn choose(s: &[u8], b: &mut impl Backend) -> Result<Choice> {
    ensure!(s.len() >= 0x800, "Automatic camera mode owner too short");
    if f(s, 0x5c0) == f32::MAX && !b.secondary_lock()? && b.boolean(0x7d4)? && b.combat_active()? {
        return Ok(Choice {
            mode: 3,
            duration: b.scalar(0x7d8)?,
        });
    }
    if b.boolean(0x7dc)? {
        if let Some(actor) = b.actor()? {
            let velocity = b.velocity(actor)?;
            // Original rounds X² and Z² separately, then refines frsqrte once.
            // Y, including a large vertical jump velocity, does not contribute.
            let squared = velocity[0] * velocity[0] + velocity[2] * velocity[2];
            let minimum = b.scalar(0x7e0)?;
            ensure!(
                squared.is_finite() && squared >= 0.,
                "Nonfinite automatic camera travel speed requires native compatibility handling"
            );
            let speed = if squared == 0. {
                0.
            } else {
                squared * super::native_hk_transform::inverse_root(squared)?
            };
            if speed >= minimum {
                return Ok(Choice {
                    mode: 2,
                    duration: b.scalar(0x7e4)?,
                });
            }
        }
    }
    Ok(Choice {
        mode: 1,
        duration: 1.5,
    })
}
struct WorldBackend<'a> {
    world: &'a super::assets::World,
    secondary: bool,
    combat: bool,
}
impl Backend for WorldBackend<'_> {
    fn secondary_lock(&mut self) -> Result<bool> {
        Ok(self.secondary)
    }
    fn boolean(&mut self, offset: usize) -> Result<bool> {
        let key = match offset {
            0x7d4 => "FreeCamera_CombatZoom_Active",
            0x7dc => "FreeCamera_TravelZoom_Active",
            _ => anyhow::bail!("Unexpected automatic mode boolean"),
        };
        super::camera_constructor::registered_boolean(key, &self.world.runtime_settings)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(offset, 0x7d8 | 0x7e0 | 0x7e4),
            "Unexpected automatic mode scalar"
        );
        super::camera_constructor::registered_slot_number(offset, &self.world.runtime_settings)
    }
    fn combat_active(&mut self) -> Result<bool> {
        Ok(self.combat)
    }
    fn actor(&mut self) -> Result<Option<usize>> {
        let world = self.world;
        let profile = world
            .camera_profiles
            .extras
            .get(usize::try_from(
                world.camera_profiles.controller.selection.current,
            )?)
            .context("Automatic mode profile absent")?;
        let Some(target) = profile
            .target
            .filter(|id| !world.removed_entities.contains(id))
        else {
            return Ok(None);
        };
        let target_entity = world
            .entities
            .get(target)
            .context("Automatic mode target absent")?;
        // e550c requires an EntityTracker before asking its target for593c.
        if !target_entity
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(None);
        }
        let tracker = world
            .mickey_tracker
            .as_ref()
            .filter(|t| t.owner == target)
            .context("Automatic mode tracker binding unavailable")?;
        ensure!(
            tracker.error.is_none(),
            "Automatic mode tracker update failed"
        );
        if world.removed_entities.contains(&tracker.target) {
            return Ok(None);
        }
        let actor = world
            .entities
            .get(tracker.target)
            .context("Automatic mode tracked entity absent")?;
        if actor
            .components
            .iter()
            .any(|c| c.class_name == "MickeyPlayerControllerComponent")
        {
            ensure!(
                tracker.target == world.player_entity,
                "Automatic mode player controller binding unavailable"
            );
            return Ok(Some(tracker.target));
        }
        ensure!(
            !actor
                .components
                .iter()
                .any(|c| c.class_name.ends_with("PlayerControllerComponent")),
            "Automatic mode player-controller type is not bound"
        );
        Ok(None)
    }
    fn velocity(&mut self, actor: usize) -> Result<[f32; 4]> {
        let world = self.world;
        let entity = world
            .entities
            .get(actor)
            .context("Automatic mode actor absent")?;
        //297198 obtains Physics - Rigid Body; genuinely absent bodies return0.
        if !entity
            .components
            .iter()
            .any(|c| c.class_name.starts_with("PhysicsRigidBody"))
        {
            return Ok([0.; 4]);
        }
        ensure!(
            actor == world.player_entity,
            "Automatic mode actor/body entity binding unavailable"
        );
        let body = world
            .player_physics
            .as_ref()
            .context("Automatic mode rigid-body runtime binding unavailable")?;
        // Body.owner belongs to the collision namespace, not the entity table.
        ensure!(
            world.collision.player_body == Some(body.shape),
            "Automatic mode attached character shape is not registered"
        );
        let pose = world.collision.simulation_pose(body.shape)?;
        Ok([pose.linear[0], pose.linear[1], pose.linear[2], 0.])
    }
}
impl super::assets::World {
    /// Call with the native application's combat byte and actual primary
    /// record identity. Neither is inferred from nearby enemies or UI state.
    pub(super) fn update_automatic_camera_mode(
        &mut self,
        s: &mut [u8],
        secondary_lock: bool,
        combat_active: bool,
    ) -> Result<bool> {
        ensure!(s.len() >= 0x800, "Automatic camera mode owner too short");
        let c = &self.camera_profiles.controller;
        ensure!(
            word(s, 0x14) == c.selection.current as u32 && word(s, 0x15c) == c.mode as u32,
            "Automatic camera mode/profile owner mismatch"
        );
        ensure!(
            !secondary_lock || word(s, 0x200) != 0,
            "Secondary camera lock identity supplied for a null primary record"
        );
        let choice = choose(
            s,
            &mut WorldBackend {
                world: self,
                secondary: secondary_lock,
                combat: combat_active,
            },
        )?;
        if c.mode == choice.mode {
            return Ok(false);
        }
        let extras = &self.camera_profiles.extras;
        let entities = &self.entities;
        let removed = &self.removed_entities;
        let tracker = self.mickey_tracker.as_ref();
        let target = &mut self.camera_target;
        let registry = &mut self.entity_event_registry;
        let mut notify = |_: &super::camera_profiles::Library,
                          selection: &super::camera_profile_selection::State|
         -> Result<()> {
            let profile = extras
                .get(usize::try_from(selection.current)?)
                .context("Automatic selected profile absent")?;
            let resolved =
                super::camera_target::resolve(profile.target, entities, removed, tracker)?;
            target.refresh(resolved, registry, super::camera_feedback_world::LISTENER)
        };
        let c = &mut self.camera_profiles.controller;
        let result = c.select_mode(choice.mode, choice.duration, &mut notify);
        // Mode/selection commit before notification; retain them on failure.
        put_word(s, 0x15c, c.mode as u32);
        let selection = c.selection;
        put_word(s, 0x14, selection.current as u32);
        put_word(s, 0x18, selection.previous as u32);
        for (off, v) in [
            (0x1c, selection.weight),
            (0x20, selection.duration),
            (0x24, selection.elapsed),
        ] {
            put(s, off, v);
        }
        s[0x28] = selection.immediate as u8;
        s[0x646] = self.camera_target.subscribed as u8;
        result
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
        fn event(&mut self, op: &str) -> Result<()> {
            self.events.push(json!({"op":op}));
            ensure!(self.case["fail"] != op, "supplied {op} failure");
            Ok(())
        }
        fn property(&mut self, offset: usize) -> Result<u32> {
            self.events.push(json!({"op":"property","offset":offset}));
            ensure!(
                self.case["fail"].as_u64() != Some(offset as u64),
                "supplied property failure"
            );
            Ok(self.case["properties_bits"][offset.to_string()]
                .as_u64()
                .unwrap() as u32)
        }
    }
    impl Backend for Host<'_> {
        fn secondary_lock(&mut self) -> Result<bool> {
            self.event("primary")?;
            Ok(self.case["secondary"].as_bool().unwrap())
        }
        fn boolean(&mut self, offset: usize) -> Result<bool> {
            Ok(self.property(offset)? != 0)
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            Ok(f32::from_bits(self.property(offset)?))
        }
        fn combat_active(&mut self) -> Result<bool> {
            self.event("combat")?;
            Ok(self.case["combat"].as_bool().unwrap())
        }
        fn actor(&mut self) -> Result<Option<usize>> {
            self.event("actor")?;
            Ok((!self.case["velocity_bits"].is_null()).then_some(7))
        }
        fn velocity(&mut self, actor: usize) -> Result<[f32; 4]> {
            assert_eq!(actor, 7);
            self.event("velocity")?;
            Ok(std::array::from_fn(|i| {
                f32::from_bits(self.case["velocity_bits"][i].as_u64().unwrap() as u32)
            }))
        }
    }
    #[test]
    fn priority_speed_thresholds_arguments_and_failures_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_automatic_mode.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s = vec![0xa5; 0x800];
            put_word(
                &mut s,
                0x5c0,
                case["override_bits"].as_u64().unwrap() as u32,
            );
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = choose(&s, &mut host);
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            assert_eq!(json!(host.events), case["events"], "{case}");
            if let Ok(choice) = result {
                assert_eq!(
                    json!([{"mode":choice.mode,"duration_bits":choice.duration.to_bits()}]),
                    case["calls"],
                    "{case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_chooser_uses_live_body_profiles_and_subscriptions() -> Result<()> {
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
        world.camera_profiles.controller.enabled = [false; 3];
        for (key, value) in [
            ("FreeCamera_CombatZoom_Active", "true"),
            ("FreeCamera_TravelZoom_Active", "true"),
            ("FreeCamera_TravelZoom_MinSpeed", "1.9"),
            ("FreeCamera_TravelZoom_TransitionTime", "0.75"),
            ("FreeCamera_CombatZoom_TransitionTime", "0.5"),
        ] {
            world.runtime_settings.insert(key.into(), value.into());
        }
        let mut s = vec![0; 0x800];
        let c = &world.camera_profiles.controller;
        put_word(&mut s, 0x14, c.selection.current as u32);
        put_word(&mut s, 0x15c, c.mode as u32);
        put(&mut s, 0x5c0, f32::MAX);
        let before = s.clone();
        assert!(world
            .update_automatic_camera_mode(&mut s, false, false)
            .is_err());
        assert_eq!(s, before);
        world.attach_player_body()?;
        let shape = world
            .player_physics
            .as_ref()
            .context("GV player body absent")?
            .shape;
        world
            .collision
            .set_simulation_velocity(shape, [3., 40., 0.])?;
        assert!(world.update_automatic_camera_mode(&mut s, false, false)?);
        assert_eq!(world.camera_profiles.controller.mode, 2);
        assert_eq!(world.camera_profiles.controller.selection.duration, 0.75);
        assert_eq!(
            world.camera_profiles.controller.library.profiles
                [world.camera_profiles.controller.selection.current as usize]
                .name,
            "Travel"
        );
        assert_eq!(
            world.camera_target.resolved_target,
            Some(world.player_entity)
        );
        assert!(world.entity_event_registry.contains_for(
            9,
            super::super::camera_feedback_world::LISTENER,
            Some(world.player_entity)
        )?);
        let before = s.clone();
        let epoch = world.entity_event_registry.epoch();
        assert!(!world.update_automatic_camera_mode(&mut s, false, false)?);
        assert_eq!(s, before);
        assert_eq!(world.entity_event_registry.epoch(), epoch);
        world
            .collision
            .set_simulation_velocity(shape, [0., 40., 0.])?;
        assert!(world.update_automatic_camera_mode(&mut s, false, false)?);
        assert_eq!(world.camera_profiles.controller.mode, 1);
        assert_eq!(world.camera_profiles.controller.selection.duration, 1.5);
        assert!(world.update_automatic_camera_mode(&mut s, false, true)?);
        assert_eq!(world.camera_profiles.controller.mode, 3);
        assert_eq!(world.camera_profiles.controller.selection.duration, 0.5);
        world
            .collision
            .set_simulation_velocity(shape, [3., 0., 0.])?;
        put_word(&mut s, 0x200, 0x224);
        assert!(world.update_automatic_camera_mode(&mut s, true, true)?);
        assert_eq!(world.camera_profiles.controller.mode, 2);
        let actual = word(&s, 0x14);
        put_word(&mut s, 0x14, actual + 1);
        let invalid = s.clone();
        assert!(world
            .update_automatic_camera_mode(&mut s, true, true)
            .is_err());
        assert_eq!(s, invalid);
        put_word(&mut s, 0x14, actual);
        put_word(&mut s, 0x200, 0);
        world
            .entity_event_registry
            .exclude(super::super::camera_feedback_world::LISTENER);
        assert!(world
            .update_automatic_camera_mode(&mut s, false, true)
            .is_err());
        assert_eq!(world.camera_profiles.controller.mode, 3);
        assert_eq!(word(&s, 0x15c), 3);
        assert_eq!(
            word(&s, 0x14),
            world.camera_profiles.controller.selection.current as u32
        );
        assert_eq!(s[0x646], 1);
        assert!(world.camera_target.subscribed);
        assert_eq!(world.camera_pipeline.output()?, published);
        assert_eq!(world.camera_pipeline.publications, publications);
        std::fs::write(
            "target/camera-automatic-mode-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Loaded GV with controlled application/lock flags and Box3D body velocities. Actual profile alias/blend/subscription path; not a gameplay camera frame.",
            "unbound_body_rejected":true,"travel_xz_velocity":[3.,40.,0.],"travel_duration":0.75,"vertical_only_falls_back":true,
            "default_duration":1.5,"combat_duration":0.5,"secondary_lock_blocks_combat":true,"unchanged_preserves_blend_and_registry":true,
            "profile_mismatch_rejected_before_mutation":true,"failed_notification_retains_mode_selection_and_subscription_byte":true,
            "authored_publication_preserved":true}))?,
        )?;
        Ok(())
    }
}
