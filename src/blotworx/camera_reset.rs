//! Native derived-camera reset e44a4..e47e4. This is not its constructor.
use super::camera_collision::V;
use anyhow::{ensure, Result};
pub trait Backend {
    fn target_position(&mut self, state: &[u8]) -> Result<V>;
    fn clear_breadcrumbs(&mut self, state: &mut [u8]) -> Result<()>;
    fn profile_value(&mut self, state: &[u8], lane: usize) -> Result<f32>;
    fn tracker(&mut self, state: &[u8]) -> Result<Option<usize>>;
    fn reset_tracker(&mut self, state: &mut [u8], tracker: usize) -> Result<()>;
    fn select_mode_profile(&mut self, state: &mut [u8], blend: i32, duration: f32) -> Result<()>;
    fn set_fov(&mut self, state: &mut [u8], text: &str) -> Result<()>;
    fn disable_2d(&mut self, state: &mut [u8]) -> Result<()>;
    fn restore_lock(&mut self, state: &mut [u8]) -> Result<()>;
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn word(s: &mut [u8], off: usize, value: u32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn copy(s: &mut [u8], from: usize, to: usize, n: usize) {
    s.copy_within(from..from + n, to);
}
fn profile(s: &[u8], b: &mut impl Backend, lane: usize) -> Result<f32> {
    let v = b.profile_value(s, lane)?;
    ensure!(v.is_finite(), "Nonfinite native reset profile value {lane}");
    Ok(v)
}
pub fn reset(s: &mut [u8], b: &mut impl Backend) -> Result<()> {
    ensure!(s.len() >= 0x7f2, "Native reset owner too short");
    word(s, 0x62c, 2);
    let position = b.target_position(s)?;
    for i in 0..4 {
        put(s, 0x2d0 + 4 * i, position[i]);
    }
    copy(s, 0x2d0, 0x2e0, 16);
    word(s, 0x634, 0);
    put(s, 0x384, 1.);
    put(s, 0x388, 1.);
    s[0x1bc] = 0;
    b.clear_breadcrumbs(s)?;
    for off in [0x28c, 0x290, 0x294] {
        word(s, off, 0);
    }
    s[0x4ac] = 0;
    word(s, 0x4b0, 0);
    s[0x4b4] = 0;
    let pi = f32::from_bits(0x40490fdb);
    let pitch = (pi * profile(s, b, 7)?) / 180.;
    put(s, 0x5a8, pitch);
    put(s, 0x358, pitch);
    let maximum = profile(s, b, 1)?;
    // Native copies radius/yaw BEFORE writing the desired distance. Do not
    // replace this with a guessed initialized radius or a new identity view.
    copy(s, 0x350, 0x360, 16);
    put(s, 0x5a0, maximum);
    for off in (0x370..=0x380).step_by(4) {
        put(s, off, 0.);
    }
    for off in [0x760, 0x764, 0x73c, 0x5b0, 0x5b4, 0x5b8, 0x5bc] {
        put(s, off, 0.);
    }
    let look = profile(s, b, 6)?;
    let placement = profile(s, b, 7)?;
    s[0x1be] = 0;
    s[0x1bf] = 0;
    put(s, 0x5b8, (pi * (placement + look)) / 180.);
    super::camera_attributes::apply(
        s,
        super::camera_attributes::Setter::Overrides,
        [f32::MAX; 5],
        0,
    )?;
    if let Some(tracker) = b.tracker(s)? {
        b.reset_tracker(s, tracker)?;
    }
    b.select_mode_profile(s, 1, 0.5)?;
    let text = super::camera_profiles::format_scalar(f(s, 0x648))?;
    b.set_fov(s, &text)?;
    b.disable_2d(s)?;
    s[0x7f1] = 0;
    b.restore_lock(s)?;
    for base in [0x224, 0x204] {
        for off in [base, base + 4, base + 8] {
            word(s, off, 0);
        }
        put(s, base + 0x18, f32::MAX);
        put(s, base + 0x10, f32::MAX);
        s[base + 0x14] = 0;
        s[base + 0xd] = 0;
        s[base + 0xc] = 0;
        put(s, base + 0x1c, 0.);
    }
    word(s, 0x200, 0);
    for off in [
        0x3ac, 0x3b8, 0x3d0, 0x3e4, 0x3f4, 0x404, 0x414, 0x254, 0x708, 0x734, 0x75c, 0x684,
    ] {
        put(s, off, 0.);
    }
    Ok(())
}
struct WorldBackend<'a>(&'a mut super::assets::World);
impl WorldBackend<'_> {
    fn target(&self) -> Result<Option<usize>> {
        use anyhow::Context;
        let i = usize::try_from(self.0.camera_profiles.controller.selection.current)?;
        Ok(self
            .0
            .camera_profiles
            .extras
            .get(i)
            .context("Reset camera profile missing")?
            .target
            .filter(|i| *i < self.0.entities.len() && !self.0.removed_entities.contains(i)))
    }
}
impl Backend for WorldBackend<'_> {
    fn target_position(&mut self, _: &[u8]) -> Result<V> {
        use anyhow::Context;
        let owner = self.target()?.context(
            "Native camera reset target absent; null transformation lookup is not reconstructed",
        )?;
        let entity = &self.0.entities[owner];
        ensure!(
            entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSTransformationComponent"),
            "Reset target has no transformation owner"
        );
        Ok(super::native_hk_transform::entity_position_vector(
            entity.transform[12..15].try_into().unwrap(),
        ))
    }
    fn clear_breadcrumbs(&mut self, s: &mut [u8]) -> Result<()> {
        self.0.camera_feedback.spin_follow_active = s[0x1bc] != 0;
        self.0.gameplay_camera_targets.breadcrumbs.clear();
        if let Some(frame) = &mut self.0.gameplay_camera_targets.frame {
            frame.published = std::array::from_fn(|i| f(s, 0x2d0 + 4 * i));
        }
        Ok(())
    }
    fn profile_value(&mut self, _: &[u8], lane: usize) -> Result<f32> {
        let c = &self.0.camera_profiles.controller;
        c.library.evaluate(&c.selection, lane)
    }
    fn tracker(&mut self, s: &[u8]) -> Result<Option<usize>> {
        self.0.camera_feedback.airborne = s[0x1be] != 0;
        self.0.camera_feedback.spin_follow_active = s[0x1bc] != 0;
        let Some(owner) = self.target()? else {
            return Ok(None);
        };
        if !self.0.entities[owner]
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(None);
        }
        ensure!(
            self.0
                .mickey_tracker
                .as_ref()
                .is_some_and(|t| t.owner == owner),
            "Reset tracker runtime binding is absent"
        );
        Ok(Some(owner))
    }
    fn reset_tracker(&mut self, _: &mut [u8], tracker: usize) -> Result<()> {
        ensure!(
            self.0
                .mickey_tracker
                .as_ref()
                .is_some_and(|t| t.owner == tracker),
            "Reset tracker changed during lookup"
        );
        self.0.reset_mickey_tracker()
    }
    fn select_mode_profile(&mut self, s: &mut [u8], blend: i32, duration: f32) -> Result<()> {
        use anyhow::Context;
        let world = &mut self.0;
        let extras = &world.camera_profiles.extras;
        let entities = &world.entities;
        let removed = &world.removed_entities;
        let tracker = world.mickey_tracker.as_ref();
        let target = &mut world.camera_target;
        let registry = &mut world.entity_event_registry;
        let mut notify = |_: &super::camera_profiles::Library,
                          selection: &super::camera_profile_selection::State|
         -> Result<()> {
            let profile = extras
                .get(usize::try_from(selection.current)?)
                .context("Reset selected profile missing")?;
            let resolved =
                super::camera_target::resolve(profile.target, entities, removed, tracker)?;
            target.refresh(resolved, registry, super::camera_feedback_world::LISTENER)
        };
        let c = &mut world.camera_profiles.controller;
        let result = c.reapply_mode(blend, duration, &mut notify);
        let selection = c.selection;
        // Selection mutates before target notification. Preserve those writes
        // even if target resolution/registration fails after selection.
        word(s, 0x14, selection.current as u32);
        word(s, 0x18, selection.previous as u32);
        for (off, v) in [
            (0x1c, selection.weight),
            (0x20, selection.duration),
            (0x24, selection.elapsed),
        ] {
            put(s, off, v);
        }
        s[0x28] = selection.immediate as u8;
        s[0x646] = world.camera_target.subscribed as u8;
        result
    }
    fn set_fov(&mut self, _: &mut [u8], text: &str) -> Result<()> {
        self.0
            .runtime_settings
            .insert("CameraFieldOfView".into(), text.into());
        Ok(())
    }
    fn disable_2d(&mut self, _: &mut [u8]) -> Result<()> {
        self.0
            .runtime_settings
            .insert("FreeCamera_2DMode_Active".into(), "false".into());
        Ok(())
    }
    fn restore_lock(&mut self, s: &mut [u8]) -> Result<()> {
        if s[0x280] == 0 {
            return Ok(());
        }
        s[0x280] = 0; // dcd1c occurs before tracker/motion restoration.
        anyhow::bail!("Native camera lock exit requires motion-speed restoration and dcd94..dcdb8 controller integration")
    }
}
impl super::assets::World {
    pub(super) fn reset_camera_controller(&mut self, state: &mut [u8]) -> Result<()> {
        ensure!(state.len() >= 0x7f2, "Native reset owner too short");
        let c = &self.camera_profiles.controller;
        ensure!(
            u32::from_be_bytes(state[0x14..0x18].try_into().unwrap()) == c.selection.current as u32
                && u32::from_be_bytes(state[0x15c..0x160].try_into().unwrap()) == c.mode as u32,
            "Native camera scalar owner/profile selection mismatch"
        );
        reset(state, &mut WorldBackend(self))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Host<'a> {
        case: &'a serde_json::Value,
        events: Vec<String>,
        snapshots: Vec<serde_json::Value>,
        fov: Option<String>,
    }
    impl Host<'_> {
        fn event(&mut self, name: &str, s: &[u8]) -> Result<()> {
            self.events.push(name.into());
            let flags: serde_json::Map<String, serde_json::Value> =
                [0x1bc, 0x1be, 0x1bf, 0x4ac, 0x4b4, 0x7f1]
                    .into_iter()
                    .map(|o| (o.to_string(), serde_json::json!(s[o])))
                    .collect();
            let words: serde_json::Map<String, serde_json::Value> = [
                0x62c, 0x634, 0x384, 0x388, 0x5a0, 0x5a8, 0x5b8, 0x370, 0x390,
            ]
            .into_iter()
            .map(|o| {
                (
                    o.to_string(),
                    serde_json::json!(u32::from_be_bytes(s[o..o + 4].try_into().unwrap())),
                )
            })
            .collect();
            self.snapshots
                .push(serde_json::json!({"name":name,"flags":flags,"words":words}));
            if self.case["fail"].as_str() == Some(name) {
                anyhow::bail!("Fixture failure at {name}");
            }
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn target_position(&mut self, s: &[u8]) -> Result<V> {
            self.event("position", s)?;
            Ok([2., 3., 4., 0.])
        }
        fn clear_breadcrumbs(&mut self, _: &mut [u8]) -> Result<()> {
            Ok(())
        }
        fn profile_value(&mut self, s: &[u8], lane: usize) -> Result<f32> {
            self.event(&format!("profile:{lane}"), s)?;
            Ok(f32::from_bits(
                self.case["values"][lane.to_string()].as_u64().unwrap() as u32,
            ))
        }
        fn tracker(&mut self, s: &[u8]) -> Result<Option<usize>> {
            self.event("tracker", s)?;
            Ok(self.case["tracker"].as_bool().unwrap().then_some(7))
        }
        fn reset_tracker(&mut self, s: &mut [u8], tracker: usize) -> Result<()> {
            assert_eq!(tracker, 7);
            self.event("reset_tracker", s)
        }
        fn select_mode_profile(&mut self, s: &mut [u8], blend: i32, duration: f32) -> Result<()> {
            assert_eq!(blend, 1);
            assert_eq!(duration, 0.5);
            self.event("select", s)?;
            word(s, 0x14, self.case["mode"].as_u64().unwrap() as u32 + 2);
            word(s, 0x18, 0);
            for (o, v) in [(0x1c, 0.), (0x20, 0.5), (0x24, 0.)] {
                put(s, o, v);
            }
            s[0x28] = 0;
            Ok(())
        }
        fn set_fov(&mut self, s: &mut [u8], text: &str) -> Result<()> {
            self.fov = Some(text.into());
            self.event("fov", s)
        }
        fn disable_2d(&mut self, s: &mut [u8]) -> Result<()> {
            self.event("disable_2d", s)
        }
        fn restore_lock(&mut self, s: &mut [u8]) -> Result<()> {
            self.event("lock", s)
        }
    }
    #[test]
    fn reset_prefixes_callback_order_and_raw_writes_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_controller_reset.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut state = vec![0x33; 0x800];
            word(&mut state, 0x14, 0);
            word(&mut state, 0x15c, case["mode"].as_u64().unwrap() as u32);
            word(&mut state, 0x280, 0);
            put(&mut state, 0x350, 4.5);
            put(&mut state, 0x354, -0.3);
            word(&mut state, 0x648, case["fov_bits"].as_u64().unwrap() as u32);
            word(
                &mut state,
                0x28c,
                case["initial_head"].as_u64().unwrap() as u32,
            );
            let mut expected = state.clone();
            for (off, value) in case["changes"].as_object().unwrap() {
                expected[off.parse::<usize>()?] = value.as_u64().unwrap() as u8;
            }
            let mut host = Host {
                case,
                events: Vec::new(),
                snapshots: Vec::new(),
                fov: None,
            };
            let result = reset(&mut state, &mut host);
            assert_eq!(
                result.is_err(),
                !case["fail"].is_null(),
                "Error case {index}"
            );
            let mismatches: Vec<_> = state
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| (format!("{i:x}"), *a, *b))
                .collect();
            assert!(mismatches.is_empty(), "State case {index}: {mismatches:?}");
            assert_eq!(
                host.events,
                serde_json::from_value::<Vec<String>>(case["events"].clone())?,
                "Calls case {index}"
            );
            assert_eq!(
                host.snapshots,
                *case["snapshots"].as_array().unwrap(),
                "Observed state case {index}"
            );
            assert_eq!(
                host.fov,
                case["fov_text"].as_str().map(str::to_owned),
                "FOV text case {index}"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_reset_updates_real_profiles_tracker_and_configuration_without_taking_over_view(
    ) -> Result<()> {
        use anyhow::Context;
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
        let information = world.camera_pipeline.output()?;
        let publications = world.camera_pipeline.publications;
        for mode in 0..=3 {
            world.camera_profiles.controller.mode = mode;
            world.camera_profiles.controller.enabled = [true; 3];
            let c = &world.camera_profiles.controller;
            let expected = if mode == 0 {
                c.mode_zero_name.clone()
            } else {
                c.customs[[
                    super::super::camera_defaults::Kind::Default,
                    super::super::camera_defaults::Kind::Combat,
                    super::super::camera_defaults::Kind::Travel,
                ]
                .into_iter()
                .find(|k| k.mode() == mode)
                .unwrap()
                .index()]
                .clone()
            };
            let maximum = c.library.evaluate(&c.selection, 1)?;
            let old_position = world
                .mickey_tracker
                .as_ref()
                .map(|t| world.entities[t.owner].transform[12..15].to_vec())
                .context("Original tracker missing")?;
            world
                .gameplay_camera_targets
                .breadcrumbs
                .push_back([1., 2., 3., 0.]);
            let mut state = vec![0; 0x800];
            word(&mut state, 0x14, c.selection.current as u32);
            word(&mut state, 0x15c, mode as u32);
            put(&mut state, 0x350, 4.5);
            put(&mut state, 0x354, -0.3);
            put(&mut state, 0x648, 55.1234);
            state[0x646] = world.camera_target.subscribed as u8;
            let epoch = world.entity_event_registry.epoch();
            world.reset_camera_controller(&mut state)?;
            let c = &world.camera_profiles.controller;
            assert_eq!(
                c.library.profiles[usize::try_from(c.selection.current)?].name,
                expected
            );
            assert_eq!(c.selection.duration, 0.5);
            assert_eq!(c.selection.weight, 0.);
            assert!(!c.selection.immediate);
            assert_eq!(world.entity_event_registry.epoch(), epoch + 1);
            assert_eq!(
                world.camera_target.resolved_target,
                Some(world.player_entity)
            );
            assert_eq!(state[0x646], world.camera_target.subscribed as u8);
            assert_eq!(f(&state, 0x5a0).to_bits(), maximum.to_bits());
            assert_eq!(
                [f(&state, 0x2d0), f(&state, 0x2d4), f(&state, 0x2d8)].as_slice(),
                old_position.as_slice()
            );
            assert_eq!(
                f(&state, 0x350),
                4.5,
                "Reset must not fabricate its current radius"
            );
            assert!(world.gameplay_camera_targets.breadcrumbs.is_empty());
            assert_eq!(world.runtime_settings["FreeCamera_2DMode_Active"], "false");
            assert_eq!(
                world.runtime_settings["CameraFieldOfView"],
                super::super::camera_profiles::format_scalar(55.1234)?
            );
            let tracker = world.mickey_tracker.as_ref().unwrap();
            assert_eq!(tracker.state.history.len(), tracker.parameters.buffer_size);
        }
        // Tracker failure occurs after native feedback flags have cleared,
        // so both raw and typed owners must retain those clears on error.
        let current = world.camera_profiles.controller.selection.current;
        let mode = world.camera_profiles.controller.mode;
        let held_tracker = world.mickey_tracker.take();
        world.camera_feedback.airborne = true;
        world.camera_feedback.spin_follow_active = true;
        let mut failed = vec![0; 0x800];
        word(&mut failed, 0x14, current as u32);
        word(&mut failed, 0x15c, mode as u32);
        put(&mut failed, 0x350, 4.5);
        put(&mut failed, 0x354, -0.3);
        put(&mut failed, 0x648, 55.1234);
        failed[0x1bc] = 1;
        failed[0x1be] = 1;
        failed[0x646] = world.camera_target.subscribed as u8;
        assert!(world.reset_camera_controller(&mut failed).is_err());
        assert!(!world.camera_feedback.airborne && !world.camera_feedback.spin_follow_active);
        assert_eq!(failed[0x1bc], 0);
        assert_eq!(failed[0x1be], 0);
        assert_eq!(world.camera_profiles.controller.selection.current, current);
        world.mickey_tracker = held_tracker;
        // Selecting a legitimately null target clears the subscription byte
        // in both owners, without treating that branch as a missing runtime.
        let default = world
            .camera_profiles
            .controller
            .library
            .find("CustomDefault")
            .unwrap();
        world.camera_profiles.extras[default].target = None;
        world.camera_profiles.controller.mode = 1;
        let mut null = vec![0; 0x800];
        word(&mut null, 0x14, current as u32);
        word(&mut null, 0x15c, 1);
        put(&mut null, 0x350, 4.5);
        put(&mut null, 0x354, -0.3);
        put(&mut null, 0x648, 55.1234);
        null[0x646] = world.camera_target.subscribed as u8;
        world.reset_camera_controller(&mut null)?;
        assert!(!world.camera_target.subscribed);
        assert_eq!(null[0x646], 0);
        // A new selected tracker without its runtime must fail before
        // deregistering the old target. Selection writes still survive.
        let current = usize::try_from(world.camera_profiles.controller.selection.current)?;
        world.camera_profiles.extras[current].target = Some(world.player_entity);
        world.camera_profiles.controller.mode = 3;
        let held_tracker = world.mickey_tracker.take();
        let mut state = vec![0x33; 0x800];
        word(&mut state, 0x14, current as u32);
        word(&mut state, 0x15c, 3);
        state[0x646] = world.camera_target.subscribed as u8;
        put(&mut state, 0x350, 4.5);
        put(&mut state, 0x354, -0.3);
        put(&mut state, 0x648, 55.1234);
        state[0x280] = 0;
        world
            .runtime_settings
            .insert("CameraFieldOfView".into(), "77.777".into());
        let epoch = world.entity_event_registry.epoch();
        let error = world.reset_camera_controller(&mut state).unwrap_err();
        assert!(format!("{error:#}").contains("runtime binding is absent"));
        let selected = world.camera_profiles.controller.selection.current;
        assert_eq!(
            u32::from_be_bytes(state[0x14..0x18].try_into().unwrap()),
            selected as u32
        );
        assert_eq!(
            world.camera_profiles.controller.library.profiles[usize::try_from(selected)?].name,
            "CustomCombat"
        );
        assert_eq!(world.entity_event_registry.epoch(), epoch);
        assert_eq!(state[0x646], world.camera_target.subscribed as u8);
        assert_eq!(
            state[0x7f1], 0x33,
            "Tail must not run after selection notification failure"
        );
        assert_eq!(world.runtime_settings["CameraFieldOfView"], "77.777");
        world.mickey_tracker = held_tracker;
        assert_eq!(world.camera_pipeline.publications, publications);
        assert_eq!(world.camera_pipeline.output()?, information);
        Ok(())
    }
}
