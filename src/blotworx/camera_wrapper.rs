//! Camera wrapper1e41c4 and target-relative pitch correction d1318/d154c.
//! Active-controller ownership and the derived gameplay generator are separate.
use super::{camera_base::Information, camera_profiles::Controller};
use anyhow::{ensure, Context, Result};

#[derive(Debug, Default)]
pub struct State {
    pub child: Option<usize>,
    pub duration: f32,
    pub elapsed: f32,
    pub cached: Option<Information>,
    pub refresh: bool,
}
pub trait Backend {
    fn update_child(&mut self, child: usize, cached: &mut Information, delta: f32) -> Result<()>;
    fn derive(&mut self, information: &mut Information, weight: f32) -> Result<()>;
    fn promote_child(&mut self, child: &mut Option<usize>) -> Result<()>;
}
impl State {
    ///1e4334 stores the supplied child directly; its caller owns acquisition.
    pub fn initialize(
        &mut self,
        child: Option<usize>,
        duration: f32,
        information: Option<Information>,
    ) {
        self.child = child;
        self.duration = duration;
        self.elapsed = 0.;
        self.refresh = information.is_none();
        if let Some(info) = information {
            self.cached = Some(info);
        }
    }
    pub fn request_refresh(&mut self) {
        self.refresh = true;
    }
    pub fn complete(&self) -> bool {
        self.elapsed >= self.duration
    }
    pub fn update(
        &mut self,
        information: &mut Information,
        delta: f32,
        backend: &mut impl Backend,
    ) -> Result<()> {
        let weight = self.advance_weight(delta);
        if self.refresh {
            self.cached = Some(*information);
            self.refresh = false;
        }
        let child = self
            .child
            .context("Camera wrapper has no acquired child controller")?;
        let cached = self
            .cached
            .as_mut()
            .context("Camera wrapper transform has not been initialized")?;
        // The child mutates owned cache directly. Its partial writes survive
        // errors; caller output is copied only after the child succeeds.
        backend.update_child(child, cached, delta)?;
        *information = *cached;
        backend.derive(information, weight)?;
        //1db12c is AFTER derived output, preserving this update's endpoint.
        backend.promote_child(&mut self.child)
    }
    pub fn advance_weight(&mut self, delta: f32) -> f32 {
        if self.duration < 0. {
            self.elapsed = self.duration - 1.;
            if self.duration < -1. {
                1.
            } else {
                -self.duration
            }
        } else {
            self.elapsed += delta;
            if self.elapsed < self.duration {
                self.elapsed / self.duration
            } else {
                1.
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Angles {
    pub placement: f32,
    pub look: f32,
}
#[derive(Debug, Default)]
pub struct AngleState {
    pub placement: Option<f32>,
    pub look: Option<f32>,
}
impl AngleState {
    ///e5a5c multiplies by pi THEN divides by180, with separate f32 rounding.
    pub fn sample(&mut self, controller: &Controller) -> Result<Angles> {
        let pi = f32::from_bits(0x40490fdb);
        let placement = (pi * controller.library.evaluate(&controller.selection, 7)?) / 180.;
        self.placement = Some(placement);
        let look = (pi * controller.library.evaluate(&controller.selection, 6)?) / 180.;
        self.look = Some(look);
        Ok(Angles { placement, look })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub camera_min: f32,
    pub camera_max: f32,
    pub look_min: f32,
    pub look_max: f32,
}
/// d0d24's requests, not permission to replace an existing ConfigProperty.
/// The gameplay controller can have registered a shared key before its wrapper.
pub const LIMIT_REGISTRATIONS: [(&str, &str); 4] = [
    ("FreeCamera_Limits_CameraMinPitch", "-0.7"),
    ("FreeCamera_Limits_CameraMaxPitch", "1.5"),
    ("FreeCamera_Limits_LookMinPitch", "-0.8123"),
    ("FreeCamera_Limits_LookMaxPitch", "1.5123"),
];
impl Limits {
    /// Read the live shared property owner; do not resolve constructor defaults
    /// independently or freeze mutable registry values in the wrapper.
    pub fn sample(mut property: impl FnMut(&str) -> Result<f32>) -> Result<Self> {
        let mut values = [0.; 4];
        for (i, (name, _)) in LIMIT_REGISTRATIONS.iter().enumerate() {
            values[i] = property(name)?;
            ensure!(values[i].is_finite(), "Nonfinite camera pitch limit {name}");
        }
        Ok(Self {
            camera_min: values[0],
            camera_max: values[1],
            look_min: values[2],
            look_max: values[3],
        })
    }
}
///147c44 preserves both pi endpoints. Do not replace its repeated rounded
/// additions/subtractions with a host modulo or normalize to a half-open range.
pub fn angle_difference(left: f32, right: f32) -> Result<f32> {
    let mut angle = left - right;
    ensure!(angle.is_finite(), "Nonfinite native angular difference");
    let tau = f32::from_bits(0x40c90fdb);
    let pi = f32::from_bits(0x40490fdb);
    while angle > pi {
        let next = angle - tau;
        ensure!(
            next < angle,
            "Native angular reduction cannot represent subtraction"
        );
        angle = next;
    }
    while angle < -pi {
        let next = angle + tau;
        ensure!(
            next > angle,
            "Native angular reduction cannot represent addition"
        );
        angle = next;
    }
    Ok(angle)
}
fn root(squared: f32) -> Result<f32> {
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Unsupported camera square root"
    );
    if squared == 0. {
        Ok(0.)
    } else {
        Ok(squared * super::native_hk_transform::inverse_root(squared)?)
    }
}
fn horizontal_length(value: [f32; 4]) -> Result<f32> {
    root(value[0] * value[0] + value[2] * value[2])
}
fn vector_length(value: [f32; 4]) -> Result<f32> {
    root(value[2].mul_add(value[2], value[0] * value[0]) + value[1] * value[1])
}
/// d1634..16d0, after the original atan2 of camera-position minus target.
pub fn desired_correction(observed: f32, angles: Angles, limits: Option<Limits>) -> Result<f32> {
    let Some(limits) = limits else { return Ok(0.) };
    let difference = angle_difference(observed, angles.placement)?;
    let ratio = if difference > 0. {
        difference / (limits.camera_max - angles.placement)
    } else {
        difference / (angles.placement - limits.camera_min)
    };
    // Native compares the lower bound first. Its not-less/not-greater branches
    // preserve an unordered ratio; do not turn0/0 into a guessed endpoint.
    let ratio = if ratio < -1. {
        -1.
    } else if ratio > 1. {
        1.
    } else {
        ratio
    };
    let delta = if ratio > 0. {
        angle_difference(limits.look_min, angles.look)?
    } else {
        angle_difference(angles.look, limits.look_max)?
    };
    Ok(observed + ratio.mul_add(delta, angles.look))
}
/// d154c's vector entry. Keep the horizontal products separately rounded;
/// the four-lane Havok length used by other camera stages is different.
pub(super) fn correction_for_positions(
    position: [f32; 4],
    target: [f32; 4],
    angles: Angles,
    limits: Option<Limits>,
) -> Result<f32> {
    if limits.is_none() {
        return Ok(0.);
    }
    let away = std::array::from_fn(|i| position[i] - target[i]);
    let observed =
        super::native_angle::atan2(f64::from(away[1]), f64::from(horizontal_length(away)?)) as f32;
    desired_correction(observed, angles, limits)
}
/// d1318. The generic wrapper weight is intentionally not used by this
/// particular derived stage. Source code applies its full pitch correction.
pub fn correct(
    information: &mut Information,
    target: Option<[f32; 3]>,
    angles: Angles,
    limits: Limits,
) -> Result<()> {
    let Some(target) = target else { return Ok(()) };
    let position: [f32; 4] = information.transform[12..16].try_into().unwrap();
    let target = super::native_hk_transform::entity_position_vector(target);
    let toward = std::array::from_fn(|i| target[i] - position[i]);
    if vector_length(toward)? <= f32::from_bits(0x38d1b717) {
        return Ok(());
    }
    let mut correction = correction_for_positions(position, target, angles, Some(limits))?;
    let direction: [f32; 4] = information.transform[8..12].try_into().unwrap();
    let pitch = super::native_angle::atan2(
        f64::from(direction[1]),
        f64::from(horizontal_length(direction)?),
    ) as f32;
    if pitch + correction > limits.look_max {
        correction = angle_difference(limits.look_max, pitch)?;
    }
    if pitch + correction < limits.look_min {
        correction = angle_difference(limits.look_min, pitch)?;
    }
    //1daefc returns the NEGATED first basis column, including its padded lane.
    //8014cdd4 forms an axis-angle quaternion;8014bdd4 forms its matrix.
    let axis: [f32; 4] = std::array::from_fn(|i| -information.transform[i]);
    let half = 0.5f32 * correction;
    let (sin, cos) = super::native_angle::sin_cos_medium(f64::from(half))?;
    let sin = sin as f32;
    let cos = cos as f32;
    let quaternion = [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos];
    let mut rotation = [0.; 16];
    super::native_hk_transform::set_rotation(&mut rotation, quaternion);
    //8014996c updates only three rotation columns. Translation, lens and
    //focus remain intact; this is not full transform composition.
    let previous = information.transform;
    for column in 0..3 {
        let vector = previous[column * 4..column * 4 + 4].try_into().unwrap();
        information.transform[column * 4..column * 4 + 4].copy_from_slice(
            &super::native_hk_transform::rotate_vector(&rotation, vector),
        );
    }
    Ok(())
}

#[derive(Default)]
pub struct Gameplay {
    pub wrapper: State,
    pub angles: AngleState,
    pub target: Option<usize>,
}
pub struct BaseUpdate<'a> {
    pub controller: &'a mut Controller,
    pub profiles: &'a mut [super::camera_base::ProfileState],
    pub inputs: super::camera_base::Inputs,
    pub fov: &'a mut super::camera_base::Fov,
    pub vertical_invert: bool,
    pub clock_delta: f32,
}
pub trait GameplayBackend: super::camera_base::Backend {
    /// Resolve the wrapper's actual acquired child, not a guessed base pointer.
    fn validate_base_child(&mut self, child: usize) -> Result<()>;
    fn target_position(&mut self, target: usize) -> Result<Option<[f32; 3]>>;
    fn pitch_limits(&mut self) -> Result<Limits>;
    fn promote_base_child(&mut self, child: &mut Option<usize>) -> Result<()>;
}
impl Gameplay {
    /// d12b0 ->1e41c4 ->1e11f4 ->derived generator ->d1318 ->promotion.
    /// World may enable this chain only once its required generator, registry,
    /// target, physics and ownership backend is real. No preview-orbit adapter.
    pub fn update(
        &mut self,
        base: BaseUpdate<'_>,
        information: &mut Information,
        delta: f32,
        backend: &mut impl GameplayBackend,
    ) -> Result<()> {
        let angles = self.angles.sample(base.controller)?;
        struct Chain<'a, 'b, B> {
            base: BaseUpdate<'a>,
            backend: &'b mut B,
            angles: Angles,
            target: Option<usize>,
        }
        impl<B: GameplayBackend> Backend for Chain<'_, '_, B> {
            fn update_child(
                &mut self,
                child: usize,
                information: &mut Information,
                delta: f32,
            ) -> Result<()> {
                self.backend.validate_base_child(child)?;
                super::camera_base::update(
                    &mut *self.base.controller,
                    &mut *self.base.profiles,
                    self.base.vertical_invert,
                    self.base.inputs,
                    &mut *self.base.fov,
                    information,
                    delta,
                    self.base.clock_delta,
                    &mut *self.backend,
                )
            }
            fn derive(&mut self, information: &mut Information, _weight: f32) -> Result<()> {
                let Some(target) = self.target else {
                    return Ok(());
                };
                let Some(position) = self.backend.target_position(target)? else {
                    return Ok(());
                };
                let limits = self.backend.pitch_limits()?;
                correct(information, Some(position), self.angles, limits)
            }
            fn promote_child(&mut self, child: &mut Option<usize>) -> Result<()> {
                self.backend.promote_base_child(child)
            }
        }
        self.wrapper.update(
            information,
            delta,
            &mut Chain {
                base,
                backend,
                angles,
                target: self.target,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info(value: f32) -> Information {
        Information {
            transform: [value; 16],
            field_of_view: value + 45.,
            focus: [value; 4],
        }
    }
    struct Host {
        calls: Vec<&'static str>,
        seen: Option<Information>,
        weight: f32,
        fail: bool,
    }
    impl Backend for Host {
        fn update_child(&mut self, child: usize, cache: &mut Information, dt: f32) -> Result<()> {
            assert_eq!(child, 7);
            assert_eq!(dt, 0.25);
            self.calls.push("child");
            self.seen = Some(*cache);
            *cache = info(9.);
            if self.fail {
                anyhow::bail!("Expected child failure");
            }
            Ok(())
        }
        fn derive(&mut self, output: &mut Information, weight: f32) -> Result<()> {
            self.calls.push("derive");
            assert_eq!(*output, info(9.));
            self.weight = weight;
            output.focus = [17.; 4];
            Ok(())
        }
        fn promote_child(&mut self, child: &mut Option<usize>) -> Result<()> {
            self.calls.push("promote");
            *child = Some(11);
            Ok(())
        }
    }
    fn host() -> Host {
        Host {
            calls: Vec::new(),
            seen: None,
            weight: 0.,
            fail: false,
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
    #[test]
    fn wrapper_scalars_and_copy_lifecycle_match_original_instructions() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_wrapper.json"
        ))?;
        assert_eq!(reference["weight"].as_array().unwrap().len(), 63);
        for case in reference["weight"].as_array().unwrap() {
            let mut state = State {
                duration: case["duration"].as_f64().unwrap() as f32,
                elapsed: case["elapsed"].as_f64().unwrap() as f32,
                ..Default::default()
            };
            let weight = state.advance_weight(case["delta"].as_f64().unwrap() as f32);
            assert_eq!(
                u64::from(weight.to_bits()),
                case["weight_bits"].as_u64().unwrap(),
                "{case}"
            );
            assert_eq!(
                u64::from(state.elapsed.to_bits()),
                case["elapsed_bits"].as_u64().unwrap(),
                "{case}"
            );
        }
        assert_eq!(reference["lifecycle"].as_array().unwrap().len(), 2);
        for case in reference["lifecycle"].as_array().unwrap() {
            let mut state = State::default();
            state.initialize(Some(7), 1., Some(info(2.)));
            if case["refresh"].as_bool().unwrap() {
                state.request_refresh();
            }
            let mut incoming = info(3.);
            let mut host = host();
            state.update(&mut incoming, 0.25, &mut host)?;
            assert_eq!(serde_json::to_value(host.calls)?, case["calls"]);
            assert_eq!(
                serde_json::to_value(host.seen.unwrap().transform.map(f32::to_bits))?,
                case["seen_transform"]
            );
            assert_eq!(
                serde_json::to_value(state.cached.unwrap().transform.map(f32::to_bits))?,
                case["cache_transform"]
            );
            assert_eq!(
                serde_json::to_value(incoming.focus.map(f32::to_bits))?,
                case["output_focus"]
            );
            assert_eq!(state.child.unwrap() as u64, case["child"].as_u64().unwrap());
        }
        assert_eq!(reference["pitch"].as_array().unwrap().len(), 64);
        for case in reference["pitch"].as_array().unwrap() {
            let values: [f32; 4] =
                std::array::from_fn(|i| case["limits"][i].as_f64().unwrap() as f32);
            let value = desired_correction(
                case["observed"].as_f64().unwrap() as f32,
                Angles {
                    placement: case["placement"].as_f64().unwrap() as f32,
                    look: case["look"].as_f64().unwrap() as f32,
                },
                Some(Limits {
                    camera_min: values[0],
                    camera_max: values[1],
                    look_min: values[2],
                    look_max: values[3],
                }),
            )?;
            if case["is_nan"].as_bool().unwrap() {
                assert!(value.is_nan(), "{case}");
            } else {
                assert_eq!(
                    u64::from(value.to_bits()),
                    case["bits"].as_u64().unwrap(),
                    "{case}"
                );
            }
        }
        assert_eq!(reference["roots"].as_array().unwrap().len(), 5);
        for case in reference["roots"].as_array().unwrap() {
            let vector = std::array::from_fn(|i| case["vector"][i].as_f64().unwrap() as f32);
            assert_eq!(
                u64::from(horizontal_length(vector)?.to_bits()),
                case["bits"].as_u64().unwrap(),
                "{case}"
            );
        }
        assert_eq!(reference["degrees"].as_array().unwrap().len(), 36);
        for case in reference["degrees"].as_array().unwrap() {
            let mut controller = controller()?;
            for (lane, name) in [(7, "placement"), (6, "look")] {
                let id = controller.library.profiles[0].properties[lane];
                controller.library.properties[id].value = case[name].as_f64().unwrap() as f32;
            }
            let angles = AngleState::default().sample(&controller)?;
            assert_eq!(
                serde_json::to_value([angles.placement.to_bits(), angles.look.to_bits()])?,
                case["bits"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn profile_angle_failure_keeps_first_write_and_live_limits_are_resampled() -> Result<()> {
        let mut controller = controller()?;
        let index = controller.library.profiles[0].properties[7];
        controller.library.properties[index].value = 18.;
        controller.library.profiles[0].properties[6] = usize::MAX;
        let mut state = AngleState {
            placement: None,
            look: Some(7.),
        };
        assert!(state.sample(&controller).is_err());
        assert_eq!(
            state.placement,
            Some((f32::from_bits(0x40490fdb) * 18.) / 180.)
        );
        assert_eq!(state.look, Some(7.));
        let mut values = LIMIT_REGISTRATIONS.map(|(_, value)| value.parse::<f32>().unwrap());
        let first = Limits::sample(|name| {
            Ok(values[LIMIT_REGISTRATIONS
                .iter()
                .position(|entry| entry.0 == name)
                .unwrap()])
        })?;
        values[1] = 0.7123;
        let second = Limits::sample(|name| {
            Ok(values[LIMIT_REGISTRATIONS
                .iter()
                .position(|entry| entry.0 == name)
                .unwrap()])
        })?;
        assert_eq!(first.camera_max, 1.5);
        assert_eq!(second.camera_max, 0.7123);
        assert_eq!(
            desired_correction(
                0.,
                Angles {
                    placement: 0.,
                    look: 0.
                },
                None
            )?,
            0.
        );
        Ok(())
    }
    #[test]
    fn wrapper_refresh_child_copy_derived_output_and_promotion_keep_native_order() -> Result<()> {
        let mut state = State::default();
        state.initialize(Some(7), 1., None);
        let mut incoming = info(3.);
        let mut host = host();
        state.update(&mut incoming, 0.25, &mut host)?;
        assert_eq!(host.calls, ["child", "derive", "promote"]);
        assert_eq!(host.seen, Some(info(3.)));
        assert_eq!(host.weight, 0.25);
        assert_eq!(state.child, Some(11));
        assert_eq!(state.cached, Some(info(9.)));
        assert_eq!(incoming.focus, [17.; 4]);
        assert!(!state.refresh);
        assert!(!state.complete());
        Ok(())
    }
    #[test]
    fn child_failure_preserves_cache_clock_and_refresh_without_publishing_output() -> Result<()> {
        let mut state = State::default();
        state.initialize(Some(7), 1., Some(info(2.)));
        state.request_refresh();
        let mut incoming = info(3.);
        let mut host = host();
        host.fail = true;
        assert!(state.update(&mut incoming, 0.25, &mut host).is_err());
        assert_eq!(host.calls, ["child"]);
        assert_eq!(state.cached, Some(info(9.)));
        assert_eq!(incoming, info(3.));
        assert_eq!(state.elapsed, 0.25);
        assert!(!state.refresh);
        assert_eq!(state.child, Some(7));
        Ok(())
    }
    #[test]
    fn correction_requires_target_and_preserves_position_lens_and_focus() -> Result<()> {
        let limits = Limits {
            camera_min: -0.7,
            camera_max: 1.5,
            look_min: -0.8123,
            look_max: 1.5123,
        };
        let mut information = Information {
            transform: super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0., 3., -5.],
            ),
            field_of_view: 65.,
            focus: [7.; 4],
        };
        let before = information;
        let angles = Angles {
            placement: 0.2,
            look: -0.1,
        };
        correct(&mut information, None, angles, limits)?;
        assert_eq!(information, before);
        correct(&mut information, Some([0., 0., 0.]), angles, limits)?;
        assert_ne!(information.transform[..12], before.transform[..12]);
        assert_eq!(information.transform[12..], before.transform[12..]);
        assert_eq!(information.field_of_view, before.field_of_view);
        assert_eq!(information.focus, before.focus);
        Ok(())
    }
    #[test]
    fn composed_gameplay_chain_runs_base_then_pitch_without_rewriting_child_cache() -> Result<()> {
        struct Pipeline {
            calls: Vec<&'static str>,
            generated: Information,
        }
        impl super::super::camera_base::Backend for Pipeline {
            fn manager_field_of_view(&mut self) -> Result<f32> {
                self.calls.push("fov");
                Ok(65.)
            }
            fn generate(
                &mut self,
                c: &mut Controller,
                _: &mut [super::super::camera_base::ProfileState],
                i: &mut Information,
                _: [f32; 4],
                _: [f32; 2],
                delta: f32,
            ) -> Result<()> {
                self.calls.push("generate");
                assert_eq!(delta, 0.25);
                assert_eq!(c.selection.elapsed, 0.375);
                *i = self.generated;
                Ok(())
            }
            fn publish_phantom(&mut self, slot: usize, p: [f32; 4]) -> Result<()> {
                assert_eq!(p, [0., 3., -5., 0.]);
                self.calls
                    .push(if slot == 0 { "phantom0" } else { "phantom1" });
                Ok(())
            }
        }
        impl GameplayBackend for Pipeline {
            fn validate_base_child(&mut self, child: usize) -> Result<()> {
                assert_eq!(child, 7);
                self.calls.push("validate");
                Ok(())
            }
            fn target_position(&mut self, target: usize) -> Result<Option<[f32; 3]>> {
                assert_eq!(target, 99);
                self.calls.push("target");
                Ok(Some([0.; 3]))
            }
            fn pitch_limits(&mut self) -> Result<Limits> {
                self.calls.push("limits");
                Ok(Limits {
                    camera_min: -0.7,
                    camera_max: 1.5,
                    look_min: -0.8123,
                    look_max: 1.5123,
                })
            }
            fn promote_base_child(&mut self, child: &mut Option<usize>) -> Result<()> {
                self.calls.push("promote");
                *child = Some(11);
                Ok(())
            }
        }
        let mut controller = controller()?;
        controller.selection.duration = 1.;
        controller.selection.previous = 0;
        let mut profiles = vec![
            super::super::camera_base::ProfileState::new(1);
            controller.library.profiles.len()
        ];
        let mut fov = super::super::camera_base::Fov::default();
        let generated = Information {
            transform: super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0., 3., -5.],
            ),
            field_of_view: 99.,
            focus: [7.; 4],
        };
        let mut output = generated;
        let mut host = Pipeline {
            calls: Vec::new(),
            generated,
        };
        let mut state = Gameplay {
            target: Some(99),
            ..Default::default()
        };
        state.wrapper.initialize(Some(7), 1., None);
        state.update(
            BaseUpdate {
                controller: &mut controller,
                profiles: &mut profiles,
                inputs: Default::default(),
                fov: &mut fov,
                vertical_invert: false,
                clock_delta: 0.25,
            },
            &mut output,
            0.25,
            &mut host,
        )?;
        assert_eq!(
            host.calls,
            [
                "validate", "fov", "generate", "phantom0", "phantom1", "target", "limits",
                "promote"
            ]
        );
        let raw = Information {
            field_of_view: 65.,
            ..generated
        };
        assert_eq!(state.wrapper.cached, Some(raw));
        assert_eq!(
            profiles[0].cached,
            super::super::camera_base::CachedInformation::from(raw)
        );
        assert_ne!(output.transform[..12], raw.transform[..12]);
        assert_eq!(output.transform[12..], raw.transform[12..]);
        assert_eq!(output.field_of_view, 65.);
        assert_eq!(output.focus, [7.; 4]);
        assert_eq!(state.wrapper.child, Some(11));
        Ok(())
    }
}
