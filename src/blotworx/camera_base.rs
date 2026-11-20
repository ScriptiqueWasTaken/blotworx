//! Base-profile update, REL1e11f4..1e1768. The derived gameplay
//! generator and camera-phantom publication are mandatory backend operations.
//! This does not substitute an orbit camera for the original generator.
use super::camera_profiles::Controller;
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Information {
    pub transform: [f32; 16], // native0..3f, including padded basis/position lanes
    pub field_of_view: f32,   //40
    pub focus: [f32; 4],      //50..5f; paired copy preserves the fourth lane too
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CachedInformation {
    //1da738 calls ba44, which returns without initializing the transform.
    // Reading it before first publication is not replaced with an identity.
    pub transform: Option<[f32; 16]>,
    pub field_of_view: f32,
    pub focus: [f32; 4],
}
impl From<Information> for CachedInformation {
    fn from(value: Information) -> Self {
        Self {
            transform: Some(value.transform),
            field_of_view: value.field_of_view,
            focus: value.focus,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileState {
    pub kind: i32,
    pub control_reference: Option<usize>, //34/40
    pub motion: [f32; 3],                 // native5c/64/60 -> XYZ
    pub cached: CachedInformation,        //70
}
impl ProfileState {
    pub fn new(kind: i32) -> Self {
        Self {
            kind,
            control_reference: None,
            motion: [0.; 3],
            cached: CachedInformation {
                transform: None,
                field_of_view: 0.,
                focus: [0.; 4],
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inputs {
    pub movement: [f32; 2],         //b8/bc
    pub orientation: [f32; 2],      //c0/c4
    pub dpad_orientation: [f32; 2], //cc/d0, alternative selected by DpadSteering
    pub dpad_steering: bool,        //configuration pointerc8 ->value18
}
/// Base constructor1dfe44..1dfec0, before derived-controller registration.
pub const DPAD_STEERING: (&str, &str) = ("FreeCamera_Input_DpadSteering", "false");
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fov {
    pub current: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub velocity: f32,
}
impl Fov {
    ///1e33d0: positive coefficient changes target/cache without discarding
    ///velocity; nonpositive snaps current and clears spring state, retaining
    ///the old target. Equal positive coefficients keep cached damping bits.
    pub fn set(&mut self, value: f32, coefficient: f32) -> Result<()> {
        ensure!(
            value.is_finite() && coefficient.is_finite(),
            "Nonfinite native FOV setter input"
        );
        if coefficient > 0. {
            self.target = value;
            let mut cache = [0; 8];
            cache[..4].copy_from_slice(&self.stiffness.to_be_bytes());
            cache[4..].copy_from_slice(&self.damping.to_be_bytes());
            super::camera_attributes::coefficient(&mut cache, 0, coefficient);
            self.stiffness = f32::from_be_bytes(cache[..4].try_into().unwrap());
            self.damping = f32::from_be_bytes(cache[4..].try_into().unwrap());
        } else {
            self.current = value;
            self.stiffness = 0.;
            self.damping = 0.;
            self.velocity = 0.;
        }
        Ok(())
    }
    ///1e1664..1e16f4 is a per-update spring. It does NOT multiply by delta.
    /// Error is sampled before integration and reused for both snap tests.
    pub fn advance(&mut self) {
        if self.stiffness > 0. {
            let error = self.target - self.current;
            self.velocity = if error.abs() < f32::from_bits(0x38d1b717) {
                0.
            } else {
                self.velocity
                    + self
                        .stiffness
                        .mul_add(error, -(self.damping * self.velocity))
            };
            self.current += self.velocity;
            if error.abs() < f32::from_bits(0x3c23d70a) {
                self.stiffness = 0.;
                self.damping = 0.;
                self.velocity = 0.;
            }
        }
    }
}
pub trait Backend {
    fn manager_field_of_view(&mut self) -> Result<f32>;
    /// virtual68=d9650. Motion and rotation are sampled BEFORE the profile
    /// clock advances; the generator evaluates its other properties AFTER it.
    fn generate(
        &mut self,
        controller: &mut Controller,
        profiles: &mut [ProfileState],
        information: &mut Information,
        motion: [f32; 4],
        rotation: [f32; 2],
        argument_delta: f32,
    ) -> Result<()>;
    /// Native optional camera phantoms +44 and +48, in that order. The
    /// backend checks actual world membership and must not manufacture it.
    fn publish_phantom(&mut self, slot: usize, position: [f32; 4]) -> Result<()>;
    fn control_point(
        &mut self,
        _controller: &Controller,
        _profiles: &[ProfileState],
    ) -> Result<Option<ControlPoint>> {
        anyhow::bail!("ControlPoint resolution requires its live entity/component owner")
    }
    fn control_point_ready(&mut self, _handle: usize) -> Result<bool> {
        anyhow::bail!("ControlPoint mode requires native31071c")
    }
    fn fallback_profile(&mut self, _controller: &Controller) -> Result<String> {
        anyhow::bail!("ControlPoint fallback requires native virtual58")
    }
    fn select_fallback(
        &mut self,
        _controller: &mut Controller,
        _name: &str,
        _mode: i32,
        _duration: f32,
    ) -> Result<()> {
        anyhow::bail!("ControlPoint fallback requires native profile selection/notification")
    }
    fn update_control_point(
        &mut self,
        _controller: &mut Controller,
        _profiles: &mut [ProfileState],
        _information: &mut Information,
        _handle: usize,
    ) -> Result<bool> {
        anyhow::bail!("ControlPoint update requires native virtual64")
    }
    fn constrain_position(&mut self, _handle: usize, _position: [f32; 4]) -> Result<[f32; 4]> {
        anyhow::bail!("ControlPoint position requires native3107b0")
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    pub handle: usize,
    pub mode: bool,
} //component byte3d
/// The existing caller owns the active controller and its full profile table.
/// An inactive controller must not be updated by this function. Clock delta is
/// nativeTimeOwner+40, independently of the controller's f1 argument.
pub fn update(
    controller: &mut Controller,
    profiles: &mut [ProfileState],
    vertical_invert: bool,
    inputs: Inputs,
    fov: &mut Fov,
    information: &mut Information,
    argument_delta: f32,
    clock_delta: f32,
    backend: &mut impl Backend,
) -> Result<()> {
    ensure!(
        argument_delta.is_finite() && clock_delta.is_finite(),
        "Nonfinite camera update sample"
    );
    if fov.current == 0. {
        fov.current = backend.manager_field_of_view()?;
    }
    let current = usize::try_from(controller.selection.current)?;
    if controller.selection.immediate {
        information.transform=profiles.get(current).context("Camera current profile output missing")?.cached.transform
            .context("Immediate camera profile transform has not been initialized by native output publication")?;
    }
    // Native performs this read even with no movement, before clearing deltas.
    controller.library.evaluate(&controller.selection, 2)?;
    profiles
        .get_mut(current)
        .context("Camera current motion record missing")?
        .motion = [0.; 3];
    if inputs.movement != [0.; 2] {
        let rate = controller.library.evaluate(&controller.selection, 3)? * argument_delta;
        profiles[current].motion = [(-rate) * inputs.movement[0], 0., rate * inputs.movement[1]];
    }
    let orientation = if inputs.dpad_steering {
        inputs.dpad_orientation
    } else {
        inputs.orientation
    };
    let yaw = argument_delta * orientation[0];
    let pitch = (argument_delta * orientation[1]) * if vertical_invert { -1. } else { 1. };
    let [x, y, z] = profiles[current].motion;
    let motion = [x, y, z, 0.];
    let kind = profiles[current].kind;
    controller.selection.advance(clock_delta);
    if kind == 2 {
        match backend.control_point(controller, profiles)? {
            Some(control) if control.mode => {
                if backend.control_point_ready(control.handle)? {
                    regularize(controller, profiles)?;
                    let name = backend.fallback_profile(controller)?;
                    backend.select_fallback(controller, &name, 1, 0.5)?;
                    backend.generate(
                        controller,
                        profiles,
                        information,
                        motion,
                        [yaw, pitch],
                        argument_delta,
                    )?;
                } else {
                    backend.generate(
                        controller,
                        profiles,
                        information,
                        motion,
                        [yaw, pitch],
                        argument_delta,
                    )?;
                    let position = backend.constrain_position(
                        control.handle,
                        information.transform[12..16].try_into().unwrap(),
                    )?;
                    information.transform[12..16].copy_from_slice(&position);
                }
            }
            Some(control) => {
                if !backend.update_control_point(
                    controller,
                    profiles,
                    information,
                    control.handle,
                )? {
                    backend.generate(
                        controller,
                        profiles,
                        information,
                        motion,
                        [yaw, pitch],
                        argument_delta,
                    )?;
                }
            }
            None => {
                regularize(controller, profiles)?;
                backend.generate(
                    controller,
                    profiles,
                    information,
                    motion,
                    [yaw, pitch],
                    argument_delta,
                )?;
            }
        }
    } else {
        backend.generate(
            controller,
            profiles,
            information,
            motion,
            [yaw, pitch],
            argument_delta,
        )?;
    }
    let position = information.transform[12..16].try_into().unwrap();
    backend.publish_phantom(0, position)?;
    backend.publish_phantom(1, position)?;
    fov.advance();
    information.field_of_view = fov.current;
    // The generator may select another profile. Native resolves current again
    // when publishing its final output rather than retaining the entry index.
    let current = usize::try_from(controller.selection.current)?;
    profiles
        .get_mut(current)
        .context("Camera publication profile output missing")?
        .cached = (*information).into();
    Ok(())
}
fn regularize(controller: &Controller, profiles: &mut [ProfileState]) -> Result<()> {
    let current = usize::try_from(controller.selection.current)?;
    let state = profiles
        .get_mut(current)
        .context("ControlPoint fallback profile missing")?;
    state.kind = 1;
    state.control_reference = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info(value: f32) -> Information {
        Information {
            transform: [value; 16],
            field_of_view: 45.,
            focus: [value; 4],
        }
    }
    fn controller() -> Result<Controller> {
        let mut settings = std::collections::BTreeMap::new();
        for field in super::super::camera_profiles::FIELDS {
            settings.insert(format!("FreeCamera_CameraData_Base_{field}"), "2.5".into());
        }
        for field in ["VerticalInvert", "HorizontalInvert", "MaintainPitch"] {
            settings.insert(
                format!("FreeCamera_CameraData_Base_{field}"),
                "false".into(),
            );
        }
        Ok(super::super::camera_catalog::Catalog::load(&settings)?.controller)
    }
    struct Host {
        calls: Vec<&'static str>,
        motion: Option<[f32; 4]>,
        rotation: Option<[f32; 2]>,
        speed: Option<f32>,
        fail: bool,
        select: Option<i32>,
    }
    impl Backend for Host {
        fn manager_field_of_view(&mut self) -> Result<f32> {
            self.calls.push("fov");
            Ok(65.)
        }
        fn generate(
            &mut self,
            c: &mut Controller,
            _: &mut [ProfileState],
            i: &mut Information,
            m: [f32; 4],
            r: [f32; 2],
            dt: f32,
        ) -> Result<()> {
            self.calls.push("generate");
            self.motion = Some(m);
            self.rotation = Some(r);
            self.speed = Some(c.library.evaluate(&c.selection, 3)?);
            assert_eq!(dt, 0.25);
            assert_eq!(c.selection.elapsed, 0.375);
            if self.fail {
                anyhow::bail!("Expected generator failure");
            }
            *i = info(9.);
            if let Some(current) = self.select {
                c.selection.current = current;
            }
            Ok(())
        }
        fn publish_phantom(&mut self, slot: usize, p: [f32; 4]) -> Result<()> {
            assert_eq!(p, [9.; 4]);
            self.calls
                .push(if slot == 0 { "phantom0" } else { "phantom1" });
            Ok(())
        }
    }
    fn host() -> Host {
        Host {
            calls: Vec::new(),
            motion: None,
            rotation: None,
            speed: None,
            fail: false,
            select: None,
        }
    }
    #[test]
    fn input_fov_and_clock_match_original_instruction_results() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_base.json"
        ))?;
        struct Samples {
            motion: [u32; 4],
            rotation: [u32; 2],
        }
        impl Backend for Samples {
            fn manager_field_of_view(&mut self) -> Result<f32> {
                anyhow::bail!("Initialized FOV must not query the manager")
            }
            fn generate(
                &mut self,
                _: &mut Controller,
                _: &mut [ProfileState],
                _: &mut Information,
                m: [f32; 4],
                r: [f32; 2],
                _: f32,
            ) -> Result<()> {
                self.motion = m.map(f32::to_bits);
                self.rotation = r.map(f32::to_bits);
                Ok(())
            }
            fn publish_phantom(&mut self, _: usize, _: [f32; 4]) -> Result<()> {
                Ok(())
            }
        }
        fn values<const N: usize>(value: &serde_json::Value) -> [f32; N] {
            std::array::from_fn(|i| value[i].as_f64().unwrap() as f32)
        }
        assert_eq!(reference["motion"].as_array().unwrap().len(), 108);
        for case in reference["motion"].as_array().unwrap() {
            let mut c = controller()?;
            let property = c.library.profiles[0].properties[3];
            c.library.properties[property].value = case["speed"].as_f64().unwrap() as f32;
            let mut profiles = vec![
                ProfileState {
                    kind: 1,
                    control_reference: None,
                    motion: [7.; 3],
                    cached: info(3.).into()
                };
                c.library.profiles.len()
            ];
            let mut fov = Fov {
                current: 65.,
                ..Fov::default()
            };
            let mut output = info(1.);
            let mut host = Samples {
                motion: [0; 4],
                rotation: [0; 2],
            };
            update(
                &mut c,
                &mut profiles,
                case["invert"].as_bool().unwrap(),
                Inputs {
                    movement: values(&case["movement"]),
                    orientation: values(&case["orientation"]),
                    dpad_orientation: values(&case["pointer"]),
                    dpad_steering: case["enabled"].as_bool().unwrap(),
                },
                &mut fov,
                &mut output,
                case["delta"].as_f64().unwrap() as f32,
                0.,
                &mut host,
            )?;
            assert_eq!(serde_json::to_value(host.motion)?, case["motion"], "{case}");
            assert_eq!(
                serde_json::to_value(host.rotation)?,
                case["rotation"],
                "{case}"
            );
        }
        assert_eq!(reference["fov"].as_array().unwrap().len(), 120);
        for case in reference["fov"].as_array().unwrap() {
            let [current, target, stiffness, damping, velocity] = values(&case["values"]);
            let mut fov = Fov {
                current,
                target,
                stiffness,
                damping,
                velocity,
            };
            fov.advance();
            assert_eq!(
                serde_json::to_value(
                    [
                        fov.current,
                        fov.target,
                        fov.stiffness,
                        fov.damping,
                        fov.velocity
                    ]
                    .map(f32::to_bits)
                )?,
                case["bits"],
                "{case}"
            );
        }
        assert_eq!(reference["clock"].as_array().unwrap().len(), 64);
        for case in reference["clock"].as_array().unwrap() {
            let [weight, duration, elapsed] = values(&case["values"]);
            let mut state = super::super::camera_profile_selection::State {
                weight,
                duration,
                elapsed,
                previous: case["previous"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            state.advance(case["delta"].as_f64().unwrap() as f32);
            assert_eq!(
                serde_json::to_value(
                    [state.weight, state.duration, state.elapsed].map(f32::to_bits)
                )?,
                case["bits"],
                "{case}"
            );
            assert_eq!(
                serde_json::to_value(state.previous as u32)?,
                case["previous_bits"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn update_samples_motion_before_blend_then_publishes_into_final_profile() -> Result<()> {
        let mut c = controller()?;
        c.selection.duration = 1.;
        c.selection.previous = 0;
        c.selection.current = 1;
        c.selection.weight = 0.25;
        let id = c.library.profiles[0].properties[3];
        c.library.properties[id].value = 4.;
        let incoming = c.library.properties.len();
        c.library
            .properties
            .push(super::super::camera_profiles::Property {
                text: "8".into(),
                value: 8.,
                minimum: -f32::MAX,
                maximum: f32::MAX,
            });
        c.library.profiles[1].properties[3] = incoming;
        let mut records = vec![
            ProfileState {
                kind: 1,
                control_reference: None,
                motion: [7.; 3],
                cached: info(3.).into()
            };
            c.library.profiles.len()
        ];
        let mut fov = Fov::default();
        let mut output = info(1.);
        let mut host = host();
        host.select = Some(2);
        update(
            &mut c,
            &mut records,
            true,
            Inputs {
                movement: [0.5, -0.25],
                dpad_steering: true,
                orientation: [99.; 2],
                dpad_orientation: [2., -3.],
            },
            &mut fov,
            &mut output,
            0.25,
            0.25,
            &mut host,
        )?;
        assert_eq!(host.calls, ["fov", "generate", "phantom0", "phantom1"]);
        assert_eq!(host.motion, Some([-0.625, 0., -0.3125, 0.]));
        assert_eq!(host.rotation, Some([0.5, 0.75]));
        assert_eq!(
            host.speed,
            Some(5.5),
            "Generator must see the advanced blend while motion uses the preceding weight"
        );
        assert_eq!(records[1].cached, CachedInformation::from(info(3.)));
        assert_eq!(records[2].cached, CachedInformation::from(output));
        assert_eq!(output.field_of_view, 65.);
        Ok(())
    }
    #[test]
    fn control_point_dispatch_matches_original_branch_fixture() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_control_point.json"
        ))?;
        struct Controls {
            calls: Vec<&'static str>,
            control: Option<ControlPoint>,
            ready: bool,
            handled: bool,
            position: [f32; 4],
        }
        impl Backend for Controls {
            fn manager_field_of_view(&mut self) -> Result<f32> {
                anyhow::bail!("Unexpected FOV query")
            }
            fn generate(
                &mut self,
                _: &mut Controller,
                _: &mut [ProfileState],
                i: &mut Information,
                _: [f32; 4],
                _: [f32; 2],
                _: f32,
            ) -> Result<()> {
                self.calls.push("generate");
                *i = info(9.);
                self.position = [9.; 4];
                Ok(())
            }
            fn publish_phantom(&mut self, slot: usize, p: [f32; 4]) -> Result<()> {
                assert_eq!(p, self.position);
                self.calls
                    .push(if slot == 0 { "phantom0" } else { "phantom1" });
                Ok(())
            }
            fn control_point(
                &mut self,
                c: &Controller,
                _: &[ProfileState],
            ) -> Result<Option<ControlPoint>> {
                assert_eq!(
                    c.selection.elapsed, 0.375,
                    "ControlPoint resolved before the native clock stage"
                );
                self.calls.extend(["clock", "resolve"]);
                Ok(self.control)
            }
            fn control_point_ready(&mut self, h: usize) -> Result<bool> {
                assert_eq!(h, 42);
                self.calls.push("ready");
                Ok(self.ready)
            }
            fn fallback_profile(&mut self, _: &Controller) -> Result<String> {
                self.calls.push("fallback");
                Ok("Default".into())
            }
            fn select_fallback(
                &mut self,
                c: &mut Controller,
                name: &str,
                mode: i32,
                duration: f32,
            ) -> Result<()> {
                self.calls.push("select");
                assert_eq!((mode, duration), (1, 0.5));
                c.selection.select(name, mode, duration, &mut c.library)
            }
            fn update_control_point(
                &mut self,
                _: &mut Controller,
                _: &mut [ProfileState],
                i: &mut Information,
                h: usize,
            ) -> Result<bool> {
                assert_eq!(h, 42);
                self.calls.push("special");
                if self.handled {
                    *i = info(9.);
                    self.position = [9.; 4];
                }
                Ok(self.handled)
            }
            fn constrain_position(&mut self, h: usize, p: [f32; 4]) -> Result<[f32; 4]> {
                assert_eq!(h, 42);
                assert_eq!(p, [9.; 4]);
                self.calls.push("constrain");
                self.position = [1., 2., 3., 4.];
                Ok(self.position)
            }
        }
        assert_eq!(reference["cases"].as_array().unwrap().len(), 16);
        for case in reference["cases"].as_array().unwrap() {
            let mut c = controller()?;
            c.selection.duration = 1.;
            c.selection.previous = 0;
            let mut records = vec![
                ProfileState {
                    kind: 2,
                    control_reference: Some(42),
                    motion: [7.; 3],
                    cached: info(3.).into()
                };
                c.library.profiles.len()
            ];
            let mut fov = Fov {
                current: 65.,
                ..Fov::default()
            };
            let mut output = info(1.);
            let mut host = Controls {
                calls: Vec::new(),
                control: case["present"].as_bool().unwrap().then_some(ControlPoint {
                    handle: 42,
                    mode: case["mode"].as_bool().unwrap(),
                }),
                ready: case["ready"].as_bool().unwrap(),
                handled: case["handled"].as_bool().unwrap(),
                position: [0.; 4],
            };
            update(
                &mut c,
                &mut records,
                false,
                Inputs::default(),
                &mut fov,
                &mut output,
                0.25,
                0.25,
                &mut host,
            )?;
            let published = host.calls.split_off(host.calls.len() - 2);
            assert_eq!(published, ["phantom0", "phantom1"]);
            assert_eq!(serde_json::to_value(host.calls)?, case["calls"], "{case}");
            assert_eq!(records[0].kind as i64, case["kind"].as_i64().unwrap());
            assert_eq!(
                records[0].control_reference.unwrap_or(0) as u64,
                case["reference"].as_u64().unwrap()
            );
            assert_eq!(
                records[c.selection.current as usize].cached,
                CachedInformation::from(output)
            );
            assert_eq!(output.field_of_view, 65.);
        }
        Ok(())
    }
    #[test]
    fn immediate_selection_copies_only_cached_transform_before_generation() -> Result<()> {
        struct Check;
        impl Backend for Check {
            fn manager_field_of_view(&mut self) -> Result<f32> {
                Ok(70.)
            }
            fn generate(
                &mut self,
                _: &mut Controller,
                _: &mut [ProfileState],
                i: &mut Information,
                _: [f32; 4],
                _: [f32; 2],
                _: f32,
            ) -> Result<()> {
                assert_eq!(i.transform, [3.; 16]);
                assert_eq!(i.focus, [1.; 4]);
                assert_eq!(i.field_of_view, 45.);
                Ok(())
            }
            fn publish_phantom(&mut self, _: usize, p: [f32; 4]) -> Result<()> {
                assert_eq!(p, [3.; 4]);
                Ok(())
            }
        }
        let mut c = controller()?;
        c.selection.immediate = true;
        let mut records = vec![
            ProfileState {
                kind: 1,
                control_reference: None,
                motion: [7.; 3],
                cached: info(3.).into()
            };
            c.library.profiles.len()
        ];
        let mut fov = Fov::default();
        let mut output = info(1.);
        update(
            &mut c,
            &mut records,
            false,
            Inputs::default(),
            &mut fov,
            &mut output,
            0.,
            0.,
            &mut Check,
        )?;
        assert_eq!(output.focus, [1.; 4]);
        assert_eq!(output.field_of_view, 70.);
        Ok(())
    }
    #[test]
    fn failed_generator_keeps_input_and_clock_prefix_without_fov_or_output_publication(
    ) -> Result<()> {
        let mut c = controller()?;
        c.selection.duration = 1.;
        c.selection.previous = 0;
        let mut records = vec![
            ProfileState {
                kind: 1,
                control_reference: None,
                motion: [7.; 3],
                cached: info(3.).into()
            };
            c.library.profiles.len()
        ];
        let mut fov = Fov::default();
        let mut output = info(1.);
        let mut host = host();
        host.fail = true;
        assert!(update(
            &mut c,
            &mut records,
            false,
            Inputs::default(),
            &mut fov,
            &mut output,
            0.25,
            0.25,
            &mut host
        )
        .is_err());
        assert_eq!(host.calls, ["fov", "generate"]);
        assert_eq!(records[0].motion, [0.; 3]);
        assert_eq!(records[0].cached, CachedInformation::from(info(3.)));
        assert_eq!(c.selection.elapsed, 0.375);
        assert_eq!(fov.current, 65.);
        Ok(())
    }
    #[test]
    fn fresh_profile_does_not_invent_an_initialized_camera_transform() -> Result<()> {
        let mut c = controller()?;
        c.selection.duration = 1.;
        c.selection.previous = 0;
        let mut records = vec![ProfileState::new(1); c.library.profiles.len()];
        assert_eq!(records[0].cached.transform, None);
        assert_eq!(records[0].cached.focus, [0.; 4]);
        let mut fov = Fov::default();
        let mut output = info(1.);
        let mut first = host();
        update(
            &mut c,
            &mut records,
            false,
            Inputs::default(),
            &mut fov,
            &mut output,
            0.25,
            0.25,
            &mut first,
        )?;
        assert_eq!(records[0].cached.transform, Some([9.; 16]));
        records[0] = ProfileState::new(1);
        c.selection.immediate = true;
        let before = c.selection;
        let before_output = output;
        let mut second = host();
        assert!(update(
            &mut c,
            &mut records,
            false,
            Inputs::default(),
            &mut fov,
            &mut output,
            0.25,
            0.25,
            &mut second
        )
        .unwrap_err()
        .to_string()
        .contains("not been initialized"));
        assert_eq!(c.selection, before);
        assert_eq!(output, before_output);
        assert!(second.calls.is_empty());
        Ok(())
    }
}
