//! Desired-polar stages inside retail e2998. These run before polar preparation,
//! input rotation and placement; they do not replace the generator scheduler.
use anyhow::{ensure, Context, Result};
pub trait RadiusBackend {
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn maximum_distance(&mut self) -> Result<f32>;
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
struct Known<'a>(Option<&'a mut [bool]>);
impl Known<'_> {
    fn require(&self, off: usize, n: usize) -> Result<()> {
        if let Some(known) = self.0.as_ref() {
            ensure!(known.get(off..off+n).is_some_and(|range|range.iter().all(|v|*v)),
            "Desired-radius field {off:x}..{:x} has not been initialized by its native lifecycle",off+n);
        }
        Ok(())
    }
    fn scalar(&self, s: &[u8], off: usize) -> Result<f32> {
        self.require(off, 4)?;
        Ok(f(s, off))
    }
    fn write(&mut self, s: &mut [u8], off: usize, value: f32) {
        put(s, off, value);
        if let Some(known) = self.0.as_mut() {
            known[off..off + 4].fill(true);
        }
    }
    fn coefficient(&mut self, s: &mut [u8], off: usize, value: f32) -> Result<()> {
        let old = self.scalar(s, off)?;
        super::camera_attributes::coefficient(s, off, value);
        if old != value {
            if let Some(known) = self.0.as_mut() {
                known[off..off + 8].fill(true);
            }
        }
        Ok(())
    }
}
fn integrate_radius(s: &mut [u8], target: f32, known: &mut Known<'_>) -> Result<()> {
    let difference = target - known.scalar(s, 0x5a0)?;
    let velocity = if difference.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        let damping = known.scalar(s, 0x60c)?;
        let old = known.scalar(s, 0x610)?;
        let coefficient = known.scalar(s, 0x608)?;
        old + coefficient.mul_add(difference, -(damping * old))
    };
    known.write(s, 0x610, velocity);
    let current = known.scalar(s, 0x5a0)?;
    known.write(s, 0x5a0, current + velocity);
    Ok(())
}
/// e2cd8..e2eb4. FPS mode, hinted distance and ordinary profile MaxDist have
/// distinct paths. Springs are per update, not multiplied by delta again.
pub fn radius(s: &mut [u8], b: &mut impl RadiusBackend) -> Result<()> {
    radius_checked(s, b, &mut Known(None))
}
fn radius_checked(s: &mut [u8], b: &mut impl RadiusBackend, known: &mut Known<'_>) -> Result<()> {
    ensure!(s.len() >= 0x800, "Desired camera radius owner too short");
    known.require(0x63c, 1)?;
    if s[0x63c] != 0 {
        let target = b.scalar(0x650)?;
        let current = known.scalar(s, 0x5a0)?;
        known.write(s, 0x5a0, 0.25f32.mul_add(target - current, current));
        return Ok(());
    }
    let hinted = known.scalar(s, 0x5c0)?;
    if hinted < f32::MAX {
        if known.scalar(s, 0x608)? < f32::MAX {
            integrate_radius(s, hinted, known)?;
        } else {
            let current = known.scalar(s, 0x5a0)?;
            let rate = b.scalar(0x694)?;
            known.write(s, 0x5a0, rate.mul_add(hinted - current, current));
        }
    } else if known.scalar(s, 0x608)? < f32::MAX {
        let target = b.maximum_distance()?;
        integrate_radius(s, target, known)?;
        // The second native evaluation occurs AFTER position/velocity writes.
        // Never reuse the earlier target or roll back that prefix on failure.
        let target = b.maximum_distance()?;
        if (known.scalar(s, 0x5a0)? - target).abs() < f32::from_bits(0x3c23d70a) {
            known.coefficient(s, 0x608, f32::MAX)?;
        }
    } else {
        let rate = b.scalar(0x694)?;
        let target = b.maximum_distance()?;
        let current = known.scalar(s, 0x5a0)?;
        known.write(s, 0x5a0, rate.mul_add(target - current, current));
    }
    Ok(())
}
struct ProfileBackend<'a> {
    catalog: &'a super::camera_catalog::Catalog,
    settings: &'a std::collections::BTreeMap<String, String>,
}
impl RadiusBackend for ProfileBackend<'_> {
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        ensure!(
            matches!(offset, 0x650 | 0x694),
            "Unexpected desired-radius property"
        );
        super::camera_constructor::registered_slot_number(offset, self.settings)
    }
    fn maximum_distance(&mut self) -> Result<f32> {
        let c = &self.catalog.controller;
        c.library.evaluate(&c.selection, 1)
    }
}
pub(super) fn radius_for_profile(
    s: &mut [u8],
    catalog: &super::camera_catalog::Catalog,
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<f32> {
    radius_for_profile_checked(s, None, catalog, settings)
}
pub(super) fn radius_for_profile_checked(
    s: &mut [u8],
    validity: Option<&mut [bool]>,
    catalog: &super::camera_catalog::Catalog,
    settings: &std::collections::BTreeMap<String, String>,
) -> Result<f32> {
    let mut known = Known(validity);
    known.require(0x14, 4)?;
    let selected = i32::from_be_bytes(
        s.get(0x14..0x18)
            .context("Desired-radius profile index absent")?
            .try_into()
            .unwrap(),
    );
    ensure!(
        selected == catalog.controller.selection.current,
        "Desired-radius profile owner mismatch"
    );
    radius_checked(s, &mut ProfileBackend { catalog, settings }, &mut known)?;
    known.scalar(s, 0x5a0)
}
impl super::assets::World {
    pub(super) fn update_camera_desired_radius(&self, s: &mut [u8]) -> Result<f32> {
        radius_for_profile(s, &self.camera_profiles, &self.runtime_settings)
    }
    pub(super) fn advance_owned_camera_desired_radius(&mut self) -> Result<f32> {
        self.camera_pipeline
            .derived
            .as_mut()
            .context("Derived radius owner absent")?
            .advance_desired_radius(&self.camera_profiles, &self.runtime_settings)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
        index: usize,
    }
    impl RadiusBackend for Host<'_> {
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(
                self.case["fail"].as_u64() != Some(offset as u64),
                "supplied scalar failure"
            );
            Ok(f32::from_bits(
                self.case["properties_bits"][offset.to_string()]
                    .as_u64()
                    .unwrap() as u32,
            ))
        }
        fn maximum_distance(&mut self) -> Result<f32> {
            self.events
                .push(json!({"op":"profile","lane":1,"index":self.index}));
            ensure!(
                self.case["fail"] != format!("profile{}", self.index),
                "supplied profile failure"
            );
            let values = self.case["profile_bits"].as_array().unwrap();
            let value = values[self.index.min(values.len() - 1)].as_u64().unwrap();
            self.index += 1;
            Ok(f32::from_bits(value as u32))
        }
    }
    #[test]
    fn radius_modes_springs_release_and_partial_writes_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_desired_radius.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s = vec![0xa5; 0x800];
            for p in case["initial_words"].as_array().unwrap() {
                let off = p[0].as_u64().unwrap() as usize;
                s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
            }
            let mut expected = s.clone();
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let mut host = Host {
                case,
                events: Vec::new(),
                index: 0,
            };
            let result = radius(&mut s, &mut host);
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            assert_eq!(json!(host.events), case["events"], "{case}");
            assert_eq!(s, expected, "{case}");
        }
        Ok(())
    }
    #[test]
    fn owned_radius_requires_only_fields_read_and_marks_actual_writes() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_desired_radius.json"
        ))?;
        let case = &fixture["cases"][0];
        let make = |hint: f32, coefficient: f32| {
            let mut s = vec![0; 0x800];
            put(&mut s, 0x5a0, 5.);
            put(&mut s, 0x5c0, hint);
            put(&mut s, 0x608, coefficient);
            let mut validity = vec![true; 0x800];
            validity[0x60c..0x614].fill(false);
            (s, validity)
        };
        // Inactive coefficients do not consume unknown damping or velocity.
        let (mut s, mut validity) = make(f32::MAX, f32::MAX);
        radius_checked(
            &mut s,
            &mut Host {
                case,
                events: Vec::new(),
                index: 0,
            },
            &mut Known(Some(&mut validity)),
        )?;
        assert!(validity[0x60c..0x614].iter().all(|v| !*v));
        // A tiny hinted error writes velocity0 without reading either old field.
        let (mut s, mut validity) = make(5., 0.05);
        radius_checked(
            &mut s,
            &mut Host {
                case,
                events: Vec::new(),
                index: 0,
            },
            &mut Known(Some(&mut validity)),
        )?;
        assert!(validity[0x610..0x614].iter().all(|v| *v));
        assert!(validity[0x60c..0x610].iter().all(|v| !*v));
        assert_eq!(f(&s, 0x610), 0.);
        // The ordinary default path additionally releases and initializes damping.
        let (mut s, mut validity) = make(f32::MAX, 0.05);
        radius_checked(
            &mut s,
            &mut Host {
                case,
                events: Vec::new(),
                index: 0,
            },
            &mut Known(Some(&mut validity)),
        )?;
        assert_eq!(f(&s, 0x608), f32::MAX);
        assert!(validity[0x60c..0x614].iter().all(|v| *v));
        // A nontrivial error really needs damping; do not substitute zero.
        let (mut s, mut validity) = make(6., 0.05);
        let before = s.clone();
        assert!(radius_checked(
            &mut s,
            &mut Host {
                case,
                events: Vec::new(),
                index: 0
            },
            &mut Known(Some(&mut validity))
        )
        .is_err());
        assert_eq!(s, before);
        assert!(validity[0x60c..0x614].iter().all(|v| !*v));
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_owned_radius_uses_live_hints_springs_and_lua_profiles() -> Result<()> {
        use super::super::{camera_attributes::Setter, camera_clear::Camera};
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
        world.seed_owned_camera_controller(published)?;
        let baseline = world
            .camera_pipeline
            .derived
            .as_ref()
            .unwrap()
            .scalar(0x5a0)?;
        let hinted = baseline + 1.;
        world
            .runtime_settings
            .insert("FreeCamera_Hint_DistanceRate".into(), "0.25".into());
        world
            .camera_pipeline
            .derived
            .as_mut()
            .unwrap()
            .apply_clear_command(Camera::Attribute(
                Setter::Overrides,
                [hinted, -f32::MAX, -f32::MAX, -f32::MAX, -f32::MAX],
                0,
            ))?;
        let interpolated = world.advance_owned_camera_desired_radius()?;
        assert_eq!(
            interpolated.to_bits(),
            0.25f32.mul_add(hinted - baseline, baseline).to_bits()
        );
        world
            .camera_pipeline
            .derived
            .as_mut()
            .unwrap()
            .apply_clear_command(Camera::Attribute(
                Setter::Springs,
                [0.2, -f32::MAX, -f32::MAX, -f32::MAX, -f32::MAX],
                0,
            ))?;
        let old = world
            .camera_pipeline
            .derived
            .as_ref()
            .unwrap()
            .scalar(0x610)?;
        let damping = world
            .camera_pipeline
            .derived
            .as_ref()
            .unwrap()
            .scalar(0x60c)?;
        let expected_velocity = old + 0.2f32.mul_add(hinted - interpolated, -(damping * old));
        let spring = world.advance_owned_camera_desired_radius()?;
        assert_eq!(
            spring.to_bits(),
            (interpolated + expected_velocity).to_bits()
        );
        assert_eq!(
            world
                .camera_pipeline
                .derived
                .as_ref()
                .unwrap()
                .scalar(0x610)?
                .to_bits(),
            expected_velocity.to_bits()
        );
        world.seed_owned_camera_controller(published)?;
        let settled = world.advance_owned_camera_desired_radius()?;
        let owner = world.camera_pipeline.derived.as_ref().unwrap();
        assert_eq!(settled.to_bits(), baseline.to_bits());
        assert_eq!(owner.scalar(0x608)?, f32::MAX);
        assert_eq!(owner.scalar(0x610)?, 0.);
        let custom = baseline + 3.25;
        world.run_callback_fixture(&format!("SetDefaultCameraValues({custom},18,-10)"))?;
        world
            .camera_profiles
            .controller
            .selection
            .advance(super::super::STEP);
        let c = &world.camera_profiles.controller;
        let blended = c.library.evaluate(&c.selection, 1)?;
        assert!(c.enabled[0]);
        ensure!(blended != baseline, "GV profile blend did not advance");
        let after_lua = world.advance_owned_camera_desired_radius()?;
        assert_eq!(
            after_lua.to_bits(),
            0.25f32.mul_add(blended - settled, settled).to_bits()
        );
        assert_eq!(world.camera_pipeline.output()?, published);
        assert_eq!(world.camera_pipeline.publications, publications);
        std::fs::write(
            "target/camera-desired-radius-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Native owned storage seeded from actual GV authored CameraInformation; controlled attribute commands, live settings and Lua profile mutation with explicit profile-clock advance. No full generator scheduling or gameplay frame.",
            "baseline":baseline,"hinted":hinted,"rate_result":interpolated,"spring_result":spring,"spring_velocity":expected_velocity,
            "settled":settled,"released_coefficient":f32::MAX,"custom_requested":custom,"blended_profile_max":blended,"after_lua":after_lua,
            "authored_publication_preserved":true}))?,
        )?;
        Ok(())
    }
}
