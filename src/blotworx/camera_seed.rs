//! Retail e5558: initialize gameplay-camera state from an outgoing view.
//! This is the handoff's initialization stage, not the default generator.
use super::{
    camera_base::Information,
    camera_collision::V,
    camera_wrapper::{self, Angles},
};
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn target_position(&mut self) -> Result<V>;
    fn profile(&mut self, lane: usize) -> Result<f32>;
    fn correction(&mut self, position: V, target: V, angles: Angles) -> Result<f32>;
    fn cache(&mut self, state: &[u8], information: Information) -> Result<()>;
    fn clear_overrides(&mut self, state: &mut [u8]) -> Result<()>;
}
trait Math {
    fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64> {
        Ok(match kind {
            "atan2" => super::native_angle::atan2(args[0], args[1]),
            "acos" => super::native_angle::acos(args[0]),
            _ => unreachable!(),
        })
    }
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn root(v: V) -> Result<f32> {
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Invalid camera handoff vector length"
    );
    Ok(if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    })
}
fn pitch(v: V, length: f32, math: &mut impl Math) -> Result<f32> {
    let ratio = v[1] / length;
    ensure!(
        !ratio.is_nan(),
        "Degenerate camera handoff vector produces unsupported NaN pitch"
    );
    let angle = if ratio.abs() >= 1. {
        if ratio > 0. {
            0.
        } else {
            f32::from_bits(0x40490fdb)
        }
    } else {
        math.sample("acos", &[ratio as f64])? as f32
    };
    Ok(f32::from_bits(0x3fc90fdb) - angle)
}
pub fn initialize(
    state: &mut [u8],
    information: Information,
    backend: &mut impl Backend,
) -> Result<()> {
    initialize_with(state, information, backend, &mut NativeMath)
}
fn initialize_with(
    s: &mut [u8],
    information: Information,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Camera handoff owner too short");
    ensure!(
        information.transform.iter().all(|v| v.is_finite()),
        "Nonfinite outgoing camera transform"
    );
    let position: V = information.transform[12..16].try_into().unwrap();
    let target = b.target_position()?;
    ensure!(
        target[..3].iter().all(|v| v.is_finite()),
        "Nonfinite camera handoff target"
    );
    let forward: V = std::array::from_fn(|i| -information.transform[8 + i]);
    let forward_length = root(forward)?;
    let forward_yaw = math.sample("atan2", &[forward[0] as f64, forward[2] as f64])? as f32;
    let forward_pitch = pitch(forward, forward_length, math)?;
    let difference: V = std::array::from_fn(|i| position[i] - target[i]);
    let distance = root(difference)?;
    put(s, 0x350, distance);
    put(
        s,
        0x354,
        math.sample("atan2", &[difference[0] as f64, difference[2] as f64])? as f32,
    );
    put(s, 0x358, pitch(difference, distance, math)?);
    if difference[0].abs() + difference[2].abs() < f32::from_bits(0x3a83126f) {
        put(
            s,
            0x354,
            math.sample("atan2", &[forward[0] as f64, forward[2] as f64])? as f32,
        );
    }
    // Include the existing padded polar lane; e57e0 copies it unchanged.
    s.copy_within(0x350..0x360, 0x360);
    s.copy_within(0x350..0x360, 0x5a0);
    let maximum = b.profile(1)?;
    ensure!(maximum.is_finite(), "Nonfinite camera profile maximum");
    put(s, 0x5a0, maximum);
    let placement = b.profile(7)?;
    ensure!(placement.is_finite(), "Nonfinite camera profile placement");
    let placement = (f32::from_bits(0x40490fdb) * placement) / 180.;
    put(s, 0x5a8, placement);
    put(
        s,
        0x380,
        camera_wrapper::angle_difference(f(s, 0x358), placement)?,
    );
    let fraction = f(s, 0x350) / maximum;
    put(s, 0x384, fraction);
    put(
        s,
        0x5b4,
        camera_wrapper::angle_difference(f(s, 0x354), forward_yaw)?,
    );
    put(
        s,
        0x5b8,
        camera_wrapper::angle_difference(f(s, 0x358), forward_pitch)?,
    );
    let look = b.profile(6)?;
    ensure!(look.is_finite(), "Nonfinite camera profile look pitch");
    let look = (f32::from_bits(0x40490fdb) * look) / 180.;
    let placement = b.profile(7)?;
    ensure!(placement.is_finite(), "Nonfinite camera profile placement");
    let placement = (f32::from_bits(0x40490fdb) * placement) / 180.;
    let correction = b.correction(position, target, Angles { placement, look })?;
    ensure!(
        correction.is_finite(),
        "Nonfinite camera handoff pitch correction"
    );
    put(s, 0x5b8, f(s, 0x5b8) - correction);
    for off in (0x370..0x380).step_by(4) {
        put(s, off, 0.);
    }
    b.cache(s, information)?;
    b.clear_overrides(s)
}
struct WorldBackend<'a>(&'a mut super::assets::World);
impl Backend for WorldBackend<'_> {
    fn target_position(&mut self) -> Result<V> {
        let current = usize::try_from(self.0.camera_profiles.controller.selection.current)?;
        let target = self
            .0
            .camera_profiles
            .extras
            .get(current)
            .context("Camera handoff profile missing")?
            .target
            .filter(|i| *i < self.0.entities.len() && !self.0.removed_entities.contains(i))
            .context("Camera handoff target missing; null transform semantics unverified")?;
        let entity = &self.0.entities[target];
        ensure!(
            entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSTransformationComponent"),
            "Camera handoff target has no transformation component"
        );
        Ok(super::native_hk_transform::entity_position_vector(
            entity.transform[12..15].try_into().unwrap(),
        ))
    }
    fn profile(&mut self, lane: usize) -> Result<f32> {
        let c = &self.0.camera_profiles.controller;
        c.library.evaluate(&c.selection, lane)
    }
    fn correction(&mut self, position: V, target: V, angles: Angles) -> Result<f32> {
        let limits = camera_wrapper::Limits::sample(|key| {
            super::camera_constructor::registered_number(key, &self.0.runtime_settings)
        })?;
        camera_wrapper::correction_for_positions(position, target, angles, Some(limits))
    }
    fn cache(&mut self, state: &[u8], information: Information) -> Result<()> {
        let current = u32::from_be_bytes(state[0x14..0x18].try_into().unwrap()) as usize;
        let profile = self
            .0
            .camera_pipeline
            .profiles
            .get_mut(current)
            .context("Camera handoff cache profile missing")?;
        profile.cached = information.into();
        Ok(())
    }
    fn clear_overrides(&mut self, state: &mut [u8]) -> Result<()> {
        super::camera_attributes::apply(
            state,
            super::camera_attributes::Setter::Overrides,
            [f32::MAX; 5],
            0,
        )
    }
}
impl super::assets::World {
    pub(super) fn seed_camera_controller(
        &mut self,
        state: &mut [u8],
        information: Information,
    ) -> Result<()> {
        initialize(state, information, &mut WorldBackend(self))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
        cached: Option<Information>,
    }
    impl Backend for Host<'_> {
        fn target_position(&mut self) -> Result<V> {
            self.events
                .extend([json!({"op":"resolve"}), json!({"op":"target"})]);
            Ok(serde_json::from_value(self.case["target"].clone())?)
        }
        fn profile(&mut self, lane: usize) -> Result<f32> {
            self.events.push(json!({"op":"profile","lane":lane}));
            ensure!(
                self.case["fail"].as_u64() != Some(lane as u64),
                "supplied profile failure"
            );
            Ok(self.case["profile"][lane.to_string()].as_f64().unwrap() as f32)
        }
        fn correction(&mut self, position: V, target: V, angles: Angles) -> Result<f32> {
            self.events.push(json!({"op":"correction","placement_bits":angles.placement.to_bits(),"look_bits":angles.look.to_bits(),
                "position_bits":position.map(f32::to_bits),"target_bits":target.map(f32::to_bits)}));
            Ok(self.case["correction"].as_f64().unwrap() as f32)
        }
        fn cache(&mut self, _: &[u8], info: Information) -> Result<()> {
            self.events.push(json!({"op":"cache"}));
            self.cached = Some(info);
            Ok(())
        }
        fn clear_overrides(&mut self, _: &mut [u8]) -> Result<()> {
            self.events
                .push(json!({"op":"overrides","bits":([f32::MAX.to_bits();5])}));
            Ok(())
        }
    }
    struct TraceMath<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Math for TraceMath<'_> {
        fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64> {
            let call = self.calls.next().context("Unexpected camera math call")?;
            assert_eq!(call["kind"], kind);
            let expected: Vec<f64> = serde_json::from_value(call["args"].clone())?;
            assert_eq!(
                args.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                expected.into_iter().map(f64::to_bits).collect::<Vec<_>>()
            );
            Ok(call["value"].as_f64().unwrap())
        }
    }
    #[test]
    fn outgoing_view_initialization_matches_original_polar_writes_and_order() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_seed.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut actual = [0xa5; 0x800];
            actual[0x14..0x18].copy_from_slice(&0u32.to_be_bytes());
            actual[0x2c..0x30].copy_from_slice(&0x74000000u32.to_be_bytes());
            actual[4..8].copy_from_slice(&0x77000000u32.to_be_bytes());
            let mut expected = actual;
            for pair in case["changed"].as_array().unwrap() {
                expected[pair[0].as_u64().unwrap() as usize] = pair[1].as_u64().unwrap() as u8;
            }
            let mut matrix = [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.,
            ];
            matrix[8..12].copy_from_slice(&serde_json::from_value::<V>(case["forward"].clone())?);
            matrix[12..16].copy_from_slice(&serde_json::from_value::<V>(case["position"].clone())?);
            let info = Information {
                transform: matrix,
                field_of_view: 63.,
                focus: [1., 2., 3., -0.],
            };
            let mut host = Host {
                case,
                events: Vec::new(),
                cached: None,
            };
            let mut math = TraceMath {
                calls: case["math"].as_array().unwrap().iter(),
            };
            let result = initialize_with(&mut actual, info, &mut host, &mut math);
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            assert!(math.calls.next().is_none());
            assert_eq!(host.events, case["events"].as_array().unwrap().as_slice());
            let differences = actual
                .iter()
                .zip(expected)
                .enumerate()
                .filter(|(_, (a, b))| **a != *b)
                .map(|(i, (a, b))| (i, *a, b))
                .collect::<Vec<_>>();
            assert!(
                differences.is_empty(),
                "Camera seed differences: {differences:?}; {case}"
            );
            if let Some(cached) = host.cached {
                assert_eq!(
                    cached.transform.map(f32::to_bits),
                    info.transform.map(f32::to_bits)
                );
                assert_eq!(cached.focus.map(f32::to_bits), info.focus.map(f32::to_bits));
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_outgoing_view_initializes_the_owned_gameplay_camera() -> Result<()> {
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
        let source = world.camera_pipeline.output()?;
        let publications = world.camera_pipeline.publications;
        world.seed_owned_camera_controller(source)?;
        let current = usize::try_from(world.camera_profiles.controller.selection.current)?;
        let cached = world.camera_pipeline.profiles[current]
            .cached
            .transform
            .context("Initializer didn't seed profile cache")?;
        assert_eq!(cached.map(f32::to_bits), source.transform.map(f32::to_bits));
        let owner = world.camera_pipeline.derived.as_ref().unwrap();
        for off in [
            0x350, 0x354, 0x358, 0x5a0, 0x5a8, 0x380, 0x384, 0x5b4, 0x5b8,
        ] {
            assert!(owner.scalar(off)?.is_finite(), "Uninitialized {off:x}");
        }
        assert!(
            world.nif_camera.is_some(),
            "Initialization selected the unfinished default generator"
        );
        assert_eq!(
            world.camera_pipeline.publications, publications,
            "Initialization published a generated view"
        );
        std::fs::write(
            "target/camera-seed-owned.json",
            serde_json::to_vec_pretty(&json!({
            "scope":"Original GV outgoing authored view initializes the real owned gameplay-camera state. Does not select the default camera, execute its generator or finish ClearAllCameraAttributesTimed.",
            "information":{"transform":source.transform,"field_of_view":source.field_of_view,"focus_bits":source.focus.map(f32::to_bits)},
            "selected_profile":current,"fields":(0x350..0x388).step_by(4).chain((0x5a0..0x5ac).step_by(4)).chain([0x5b4,0x5b8])
                .map(|off|Ok((format!("{off:x}"),owner.scalar(off)?.to_bits()))).collect::<Result<std::collections::BTreeMap<_,_>>>()?,
            "camera_pipeline":world.camera_pipeline.summary()}))?,
        )?;
        Ok(())
    }
}
