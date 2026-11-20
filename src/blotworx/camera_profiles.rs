//! Scalar camera-profile storage/evaluation, REL1df744,1e320c,1e3280.
//! Construction of all native profiles, target handles and the generated
//! camera still belong to the controller integration, not this scalar table.
use super::camera_defaults;
use super::camera_profile_selection::{self, State};
use anyhow::{ensure, Context, Result};

pub const FIELDS: [&str; 10] = [
    "MinDist",
    "MaxDist",
    "WalkSpeed",
    "RunSpeed",
    "PitchAngle",
    "PitchSpeed",
    "LookPitch",
    "PlacementPitch",
    "TargetHeightOffset",
    "TargetRadius",
];

/// REL146fc0 uses %g (six significant digits) for an unqualified %1% float.
/// Nonfinite property formatting/atof behavior needs separate native coverage.
pub fn format_scalar(value: f32) -> Result<String> {
    ensure!(
        value.is_finite(),
        "Nonfinite camera property formatting is not reconstructed"
    );
    let promoted = f64::from(value);
    let scientific = format!("{promoted:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .context("Missing float exponent")?;
    let exponent: i32 = exponent.parse()?;
    let trim = |text: String| {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text
        }
    };
    if exponent < -4 || exponent >= 6 {
        Ok(format!(
            "{}e{}{:02}",
            trim(mantissa.to_owned()),
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        ))
    } else {
        Ok(trim(format!(
            "{:.*}",
            (5 - exponent).max(0) as usize,
            promoted
        )))
    }
}

#[derive(Clone, Debug)]
pub struct Property {
    pub text: String,
    pub value: f32,
    pub minimum: f32,
    pub maximum: f32,
}
impl Property {
    pub fn write_float(&mut self, value: f32) -> Result<()> {
        let text = format_scalar(value)?;
        // REL14aeb0 uses atof (double), then frsp before applying bounds.
        let parsed = text.parse::<f64>()? as f32;
        let bounded = if parsed > self.minimum {
            parsed
        } else {
            self.minimum
        };
        let bounded = if bounded < self.maximum {
            bounded
        } else {
            self.maximum
        };
        self.text = text;
        self.value = bounded;
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub properties: [usize; 10],
}
#[derive(Default, Debug)]
pub struct Library {
    pub properties: Vec<Property>,
    pub profiles: Vec<Profile>,
}
impl Library {
    pub fn find(&self, name: &str) -> Option<usize> {
        self.profiles
            .iter()
            .position(|profile| profile.name == name)
    }
    /// Native1df744 prefers an explicitly found configuration property; absent
    /// overrides retain the base property pointer, not a value copy.
    pub fn derive(
        &mut self,
        name: &str,
        base: usize,
        overrides: [Option<usize>; 10],
    ) -> Result<usize> {
        ensure!(self.find(name).is_none(), "Duplicate camera profile {name}");
        let base = self
            .profiles
            .get(base)
            .context("Camera base profile absent")?;
        let properties = std::array::from_fn(|i| overrides[i].unwrap_or(base.properties[i]));
        ensure!(
            properties.iter().all(|&id| id < self.properties.len()),
            "Camera profile property absent"
        );
        let index = self.profiles.len();
        self.profiles.push(Profile {
            name: name.into(),
            properties,
        });
        Ok(index)
    }
    fn property(&self, profile: i32, lane: usize) -> Result<&Property> {
        let profile = usize::try_from(profile).context("Invalid camera profile index")?;
        let id = *self
            .profiles
            .get(profile)
            .context("Camera profile absent")?
            .properties
            .get(lane)
            .context("Invalid camera scalar lane")?;
        self.properties
            .get(id)
            .context("Camera scalar property absent")
    }
    /// Native1e320c multiplies incoming separately, then fuses outgoing*weight
    /// plus that rounded product. Reassociating this changes observable bits.
    pub fn evaluate(&self, state: &State, lane: usize) -> Result<f32> {
        let incoming = self.property(state.current, lane)?.value;
        if state.previous == -1 {
            return Ok(incoming);
        }
        let outgoing = self.property(state.previous, lane)?.value;
        let incoming = state.weight * incoming;
        Ok((1. - state.weight).mul_add(outgoing, incoming))
    }
    /// Native1e3280 finds Intermediate once, then evaluates/formats/writes each
    /// lane in order. Shared properties make these interleaved writes observable.
    pub fn capture_intermediate(&mut self, state: &State) -> Result<usize> {
        let index = self
            .find("Intermediate")
            .context("Intermediate camera profile absent")?;
        for lane in 0..10 {
            let value = self.evaluate(state, lane)?;
            let id = self.profiles[index].properties[lane];
            self.properties
                .get_mut(id)
                .context("Intermediate property absent")?
                .write_float(value)?;
        }
        Ok(index)
    }
}
impl camera_profile_selection::Backend for Library {
    fn find_profile(&mut self, name: &str) -> Result<i32> {
        i32::try_from(
            self.find(name)
                .with_context(|| format!("Camera profile {name} absent"))?,
        )
        .map_err(Into::into)
    }
    fn snapshot_interrupted(&mut self, state: &State) -> Result<i32> {
        i32::try_from(self.capture_intermediate(state)?).map_err(Into::into)
    }
}

/// The scalar profile portion of the native gameplay controller. Creation of
/// its catalog/mode/names must come from the native initialization path.
/// Notification remains mandatory: selecting a profile cannot silently skip
/// the target subscription lifecycle at e52e0.
pub struct Controller {
    pub library: Library,
    pub selection: State,
    pub mode: i32,
    pub mode_zero_name: String, // native14c; constructor d4a50..d4a6c: Near
    /// Native order: Default, Combat, Travel.
    pub originals: [String; 3],
    pub customs: [String; 3],
    pub enabled: [bool; 3],
}
impl Controller {
    /// dbe30: clamp the native mode, store it before virtual5c selection, and
    /// do nothing when unchanged. Mode2 is Travel; mode3 is Combat.
    pub fn select_mode(
        &mut self,
        mode: i32,
        duration: f32,
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<bool> {
        let mode = mode.clamp(0, 3);
        if self.mode == mode {
            return Ok(false);
        }
        self.mode = mode;
        let name = match mode {
            0 => self.mode_zero_name.clone(),
            1 => self.originals[0].clone(),
            2 => self.originals[2].clone(),
            _ => self.originals[1].clone(),
        };
        self.reapply(&name, 1, duration, notify)?;
        Ok(true)
    }
    /// e46a8 selects the current mode's name through derived virtual5c;
    /// it does not call dbe30 or early-return because the mode is unchanged.
    pub(super) fn reapply_mode(
        &mut self,
        blend: i32,
        duration: f32,
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<()> {
        let name = match self.mode {
            0 => self.mode_zero_name.clone(),
            1 => self.originals[0].clone(),
            2 => self.originals[2].clone(),
            3 => self.originals[1].clone(),
            _ => anyhow::bail!("Native camera reset mode is outside its name table"),
        };
        self.reapply(&name, blend, duration, notify)
    }
    fn reapply(
        &mut self,
        name: &str,
        mode: i32,
        duration: f32,
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<()> {
        let aliases = std::array::from_fn(|i| camera_defaults::Alias {
            original: &self.originals[i],
            custom: &self.customs[i],
            enabled: self.enabled[i],
        });
        let selected = camera_defaults::resolve_alias(name, aliases).to_owned();
        self.selection
            .select(&selected, mode, duration, &mut self.library)?;
        notify(&self.library, &self.selection)
    }
    /// RELe4a30/e4c90/e4ef0 clear only the selected alias-enable byte.
    /// Retained custom property values survive reset. Reapplication uses the
    /// remaining aliases and preserves the ordinary interrupted blend path.
    pub fn reset_values(
        &mut self,
        kind: camera_defaults::Kind,
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<()> {
        self.enabled[kind.index()] = false;
        if self.mode == kind.mode() {
            let name = self.originals[kind.index()].clone();
            self.reapply(&name, 1, 0.5, notify)?;
        }
        Ok(())
    }
    pub fn set_defaults(
        &mut self,
        values: [f32; 3],
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<()> {
        self.set_values(camera_defaults::Kind::Default, values, notify)
    }
    pub fn set_values(
        &mut self,
        kind: camera_defaults::Kind,
        values: [f32; 3],
        notify: &mut impl FnMut(&Library, &State) -> Result<()>,
    ) -> Result<()> {
        struct Adapter<'a, F> {
            controller: &'a mut Controller,
            notify: &'a mut F,
            kind: camera_defaults::Kind,
        }
        impl<F: FnMut(&Library, &State) -> Result<()>> camera_defaults::Backend for Adapter<'_, F> {
            fn enable_custom(&mut self) -> Result<()> {
                self.controller.enabled[self.kind.index()] = true;
                Ok(())
            }
            fn custom_profile(&mut self) -> Result<usize> {
                self.controller
                    .library
                    .find(&self.controller.customs[self.kind.index()])
                    .context("Custom camera profile absent")
            }
            fn set_formatted_float(
                &mut self,
                profile: usize,
                property: camera_defaults::Property,
                value: f32,
            ) -> Result<()> {
                let lane = match property {
                    camera_defaults::Property::MaxDistance => 1,
                    camera_defaults::Property::PlacementPitch => 7,
                    camera_defaults::Property::LookPitch => 6,
                };
                let library = &mut self.controller.library;
                let id = library
                    .profiles
                    .get(profile)
                    .context("Camera profile absent")?
                    .properties[lane];
                library
                    .properties
                    .get_mut(id)
                    .context("Camera property absent")?
                    .write_float(value)
            }
            fn controller_mode(&mut self) -> Result<i32> {
                Ok(self.controller.mode)
            }
            fn selected_name(&mut self) -> Result<String> {
                Ok(self.controller.originals[self.kind.index()].clone())
            }
            fn reapply_profile(&mut self, name: &str, mode: i32, duration: f32) -> Result<()> {
                self.controller.reapply(name, mode, duration, self.notify)
            }
        }
        camera_defaults::set_kind(
            kind,
            values,
            &mut Adapter {
                controller: self,
                notify,
                kind,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn property(value: f32) -> Property {
        Property {
            text: value.to_string(),
            value,
            minimum: -f32::MAX,
            maximum: f32::MAX,
        }
    }
    fn library() -> Library {
        Library {
            properties: (0..30).map(|id| property(id as f32)).collect(),
            profiles: vec![
                Profile {
                    name: "From".into(),
                    properties: std::array::from_fn(|i| i),
                },
                Profile {
                    name: "To".into(),
                    properties: std::array::from_fn(|i| 10 + i),
                },
                Profile {
                    name: "Intermediate".into(),
                    properties: std::array::from_fn(|i| 20 + i),
                },
            ],
        }
    }
    #[test]
    fn general_format_rounds_six_digits_and_switches_at_rounded_exponent() -> Result<()> {
        for (value, expected) in [
            (6.25, "6.25"),
            (18., "18"),
            (-10., "-10"),
            (0., "0"),
            (-0., "-0"),
            (1.2345678, "1.23457"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (999999.5, "1e+06"),
            (1234567., "1.23457e+06"),
            (f32::MAX, "3.40282e+38"),
            (f32::from_bits(1), "1.4013e-45"),
        ] {
            assert_eq!(format_scalar(value)?, expected, "{value}");
        }
        assert!(format_scalar(f32::INFINITY).is_err());
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn general_format_matches_host_crt_for_finite_f32_samples() -> Result<()> {
        // A secondary formatting oracle, not a claim of original Wii MSL
        // runtime validation. The native format string itself is verified.
        #[link(name = "legacy_stdio_definitions")]
        unsafe extern "C" {
            fn snprintf(
                buffer: *mut std::ffi::c_char,
                length: usize,
                format: *const std::ffi::c_char,
                ...
            ) -> i32;
        }
        let mut seed = 0x72a491e3_u32;
        for _ in 0..4096 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let value = f32::from_bits(seed);
            if !value.is_finite() {
                continue;
            }
            let mut buffer = [0_i8; 128];
            let length = unsafe {
                snprintf(
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    b"%g\0".as_ptr().cast(),
                    f64::from(value),
                )
            };
            ensure!(
                length >= 0 && (length as usize) < buffer.len(),
                "CRT formatting failed"
            );
            let expected = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_str()?;
            assert_eq!(format_scalar(value)?, expected, "input bits {seed:08x}");
        }
        Ok(())
    }
    #[test]
    fn inherited_properties_share_ownership_but_explicit_overrides_are_independent() -> Result<()> {
        let mut lib = library();
        let mut overrides = [None; 10];
        overrides[1] = Some(19);
        let derived = lib.derive("CustomDefault", 0, overrides)?;
        lib.properties[lib.profiles[derived].properties[0]].write_float(99.)?;
        assert_eq!(lib.properties[lib.profiles[0].properties[0]].value, 99.);
        lib.properties[lib.profiles[derived].properties[1]].write_float(88.)?;
        assert_eq!(lib.properties[lib.profiles[0].properties[1]].value, 1.);
        Ok(())
    }
    #[test]
    fn interruption_captures_ten_current_blended_values_and_selects_real_profile() -> Result<()> {
        let mut lib = library();
        let mut state = State {
            current: 1,
            previous: 0,
            weight: 0.25,
            ..State::default()
        };
        assert_eq!(lib.evaluate(&state, 0)?, 2.5);
        state.select("To", 1, 0.5, &mut lib)?;
        assert_eq!((state.current, state.previous), (1, 2));
        assert_eq!(lib.properties[20].value, 2.5);
        assert_eq!(lib.properties[29].value, 11.5);
        assert_eq!(state.weight, 0.);
        assert_eq!(lib.evaluate(&state, 0)?, 2.5);
        state.advance(0.1);
        assert!(lib.evaluate(&state, 0)? > 2.5);
        Ok(())
    }
    #[test]
    fn snapshot_writes_are_interleaved_with_reads_and_apply_property_bounds() -> Result<()> {
        let mut lib = library();
        lib.profiles[2].properties[0] = 1;
        lib.properties[1].maximum = 0.5;
        let state = State {
            current: 1,
            previous: 0,
            weight: 0.25,
            ..State::default()
        };
        lib.capture_intermediate(&state)?;
        assert_eq!(lib.properties[1].text, "2.5");
        assert_eq!(lib.properties[1].value, 0.5);
        assert_eq!(
            lib.properties[21].value, 3.125,
            "Lane1 did not observe lane0's shared-property write"
        );
        Ok(())
    }
    #[test]
    fn native_fused_order_retains_separately_rounded_incoming_product() -> Result<()> {
        let mut lib = library();
        lib.properties[0].value = f32::from_bits(0xc0019e7b);
        lib.properties[10].value = f32::from_bits(0x40bfa2ec);
        let state = State {
            current: 1,
            previous: 0,
            weight: f32::from_bits(0x3efc984a),
            ..State::default()
        };
        assert_eq!(lib.evaluate(&state, 0)?.to_bits(), 0x3ff6d4e7);
        // A conventional outgoing + weight*(incoming-outgoing) gives e9.
        Ok(())
    }
    fn controller() -> Result<Controller> {
        let mut library = library();
        library.derive("CustomDefault", 0, [None; 10])?;
        Ok(Controller {
            library,
            selection: State::default(),
            mode: 1,
            mode_zero_name: "Near".into(),
            originals: ["From".into(), "Combat".into(), "Travel".into()],
            customs: [
                "CustomDefault".into(),
                "CustomCombat".into(),
                "CustomTravel".into(),
            ],
            enabled: [false; 3],
        })
    }
    #[test]
    fn default_update_uses_real_shared_properties_alias_selection_and_notification() -> Result<()> {
        let mut c = controller()?;
        let mut calls = 0;
        c.set_defaults([6.25, 18., -10.], &mut |library, state| {
            calls += 1;
            assert_eq!((state.current, state.previous, state.duration), (3, 0, 0.5));
            assert_eq!(library.evaluate(state, 1)?, 6.25);
            assert_eq!(library.evaluate(state, 7)?, 18.);
            assert_eq!(library.evaluate(state, 6)?, -10.);
            Ok(())
        })?;
        assert_eq!(calls, 1);
        assert!(c.enabled[0]);
        assert_eq!(
            c.library.properties[c.library.profiles[0].properties[1]].text,
            "6.25"
        );
        Ok(())
    }
    #[test]
    fn resets_match_original_mode_gates_flags_and_tail_call_arguments() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_reset.json"
        ))?;
        assert_eq!(reference["cases"].as_array().unwrap().len(), 36);
        for case in reference["cases"].as_array().unwrap() {
            let kind = match case["kind"].as_str().unwrap() {
                "Default" => camera_defaults::Kind::Default,
                "Combat" => camera_defaults::Kind::Combat,
                _ => camera_defaults::Kind::Travel,
            };
            let mut c = controller()?;
            for name in ["Combat", "Travel", "CustomCombat", "CustomTravel"] {
                c.library.derive(name, 1, [None; 10])?;
            }
            c.mode = case["mode"].as_i64().unwrap() as i32;
            c.enabled = [case["initial_enabled"].as_bool().unwrap(); 3];
            let custom = c.library.find(&c.customs[kind.index()]).unwrap();
            c.selection.current = custom as i32;
            let original = c.library.find(&c.originals[kind.index()]).unwrap();
            let properties = c.library.profiles[custom]
                .properties
                .map(|id| c.library.properties[id].value.to_bits());
            let previous = c.selection;
            let mut calls = Vec::new();
            c.reset_values(kind, &mut |_, state| {
                calls.push(*state);
                Ok(())
            })?;
            assert_eq!(serde_json::to_value(c.enabled)?, case["enabled"], "{case}");
            assert_eq!(
                calls.len(),
                case["calls"].as_array().unwrap().len(),
                "{case}"
            );
            if let Some(state) = calls.first() {
                let native = &case["calls"][0];
                assert_eq!(
                    native["name_offset"].as_u64().unwrap(),
                    0x14c + kind.mode() as u64 * 4
                );
                assert_eq!(native["mode"], 1);
                assert_eq!(state.current, original as i32);
                assert_eq!(state.previous, custom as i32);
                assert_eq!(state.elapsed, 0.);
                assert_eq!(
                    state.duration.to_bits() as u64,
                    native["duration_bits"].as_u64().unwrap()
                );
            } else {
                assert_eq!(c.selection, previous);
            }
            assert_eq!(
                c.library.profiles[custom]
                    .properties
                    .map(|id| c.library.properties[id].value.to_bits()),
                properties,
                "Reset must retain custom property contents"
            );
        }
        Ok(())
    }
    #[test]
    fn reset_retains_other_aliases_and_clears_before_selection_or_notification_failure(
    ) -> Result<()> {
        let mut c = controller()?;
        c.library.derive("CustomCombat", 1, [None; 10])?;
        c.originals[1] = c.originals[0].clone();
        c.enabled = [true, true, true];
        let combat = c.library.find("CustomCombat").unwrap();
        c.reset_values(camera_defaults::Kind::Default, &mut |_, _| Ok(()))?;
        assert_eq!(c.enabled, [false, true, true]);
        assert_eq!(c.selection.current, combat as i32);
        c.enabled[0] = true;
        c.originals[0] = "MissingOriginal".into();
        assert!(c
            .reset_values(camera_defaults::Kind::Default, &mut |_, _| Ok(()))
            .is_err());
        assert!(!c.enabled[0]);
        c.originals[0] = "From".into();
        c.enabled[0] = true;
        c.enabled[1] = false;
        assert!(c
            .reset_values(camera_defaults::Kind::Default, &mut |_, _| anyhow::bail!(
                "Expected target failure"
            ))
            .is_err());
        assert!(!c.enabled[0]);
        assert_eq!(c.selection.current, 0);
        assert_eq!(c.selection.duration, 0.5);
        Ok(())
    }
    #[test]
    fn combat_and_travel_update_own_profiles_and_only_reapply_in_matching_mode() -> Result<()> {
        use camera_defaults::Kind;
        for kind in [Kind::Combat, Kind::Travel] {
            for mode in [1, 2, 3] {
                let mut c = controller()?;
                // Independent explicit property overrides, as in retail ConfigFiles.ini.
                for name in ["CustomCombat", "CustomTravel"] {
                    let mut overrides = [None; 10];
                    for lane in [1, 7, 6] {
                        let id = c.library.properties.len();
                        c.library.properties.push(property(0.));
                        overrides[lane] = Some(id);
                    }
                    c.library.derive(name, 0, overrides)?;
                }
                c.customs[1] = "CustomCombat".into();
                c.customs[2] = "CustomTravel".into();
                c.originals[1] = "Combat".into();
                c.originals[2] = "Travel".into();
                c.mode = mode;
                let previous = c.selection;
                let mut calls = 0;
                c.set_values(kind, [8.25, 21., -12.], &mut |_, _| {
                    calls += 1;
                    Ok(())
                })?;
                assert!(c.enabled[kind.index()]);
                assert!(!c.enabled[0]);
                assert_eq!(calls, usize::from(mode == kind.mode()));
                let profile = c.library.find(&c.customs[kind.index()]).unwrap();
                for (lane, value) in [(1, 8.25), (7, 21.), (6, -12.)] {
                    assert_eq!(
                        c.library.properties[c.library.profiles[profile].properties[lane]].value,
                        value
                    );
                }
                if mode == kind.mode() {
                    assert_eq!(c.selection.current, profile as i32);
                    assert_eq!(c.selection.duration, 0.5);
                } else {
                    assert_eq!(c.selection.current, previous.current);
                    assert_eq!(c.selection.previous, previous.previous);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn notification_failure_keeps_committed_properties_and_transition() -> Result<()> {
        let mut c = controller()?;
        assert!(c
            .set_defaults([6.25, 18., -10.], &mut |_, _| anyhow::bail!(
                "Native target subscriptions unavailable"
            ))
            .is_err());
        assert!(c.enabled[0]);
        assert_eq!((c.selection.current, c.selection.previous), (3, 0));
        assert_eq!(c.library.properties[1].value, 6.25);
        Ok(())
    }
    #[test]
    fn automatic_modes_select_native_travel_combat_slots_and_keep_aliases() -> Result<()> {
        let mut settings = std::collections::BTreeMap::new();
        for field in FIELDS {
            settings.insert(format!("FreeCamera_CameraData_Base_{field}"), "1".into());
        }
        for field in ["VerticalInvert", "HorizontalInvert", "MaintainPitch"] {
            settings.insert(
                format!("FreeCamera_CameraData_Base_{field}"),
                "false".into(),
            );
        }
        let mut c = super::super::camera_catalog::Catalog::load(&settings)?.controller;
        c.enabled = [true, true, true];
        let mut calls = 0;
        for (mode, name, duration) in [
            (2, "CustomTravel", 2.5),
            (3, "CustomCombat", 1.),
            (1, "CustomDefault", 1.5),
        ] {
            assert!(c.select_mode(mode, duration, &mut |library, state| {
                calls += 1;
                assert_eq!(library.profiles[state.current as usize].name, name);
                assert_eq!(state.duration, duration);
                assert_eq!(state.elapsed, 0.);
                Ok(())
            })?);
            assert_eq!(c.mode, mode);
            let prior = c.selection;
            assert!(!c.select_mode(mode, 9., &mut |_, _| anyhow::bail!(
                "Unchanged mode must not notify"
            ))?);
            assert_eq!(
                c.selection, prior,
                "Unchanged mode must not restart an active blend"
            );
        }
        assert_eq!(calls, 3);
        assert!(c
            .select_mode(9, 0.75, &mut |_, _| anyhow::bail!(
                "Expected notification failure"
            ))
            .is_err());
        assert_eq!(
            c.mode, 3,
            "Native commits mode before selection/notification"
        );
        assert_eq!(
            c.library.profiles[c.selection.current as usize].name,
            "CustomCombat"
        );
        assert_eq!(c.selection.duration, 0.75);
        assert!(c.select_mode(-2, 1., &mut |_, _| Ok(()))?);
        assert_eq!(c.mode, 0);
        assert_eq!(
            c.library.profiles[c.selection.current as usize].name,
            "Near"
        );
        assert!(!c.select_mode(0, 1., &mut |_, _| anyhow::bail!(
            "Unchanged mode0 must return early"
        ))?);
        Ok(())
    }
    #[test]
    fn mode_clamp_and_callback_protocol_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_mode.json"
        ))?;
        let mut settings = std::collections::BTreeMap::new();
        for field in FIELDS {
            settings.insert(format!("FreeCamera_CameraData_Base_{field}"), "1".into());
        }
        for field in ["VerticalInvert", "HorizontalInvert", "MaintainPitch"] {
            settings.insert(
                format!("FreeCamera_CameraData_Base_{field}"),
                "false".into(),
            );
        }
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 54);
        for case in fixture["cases"].as_array().unwrap() {
            let mut c = super::super::camera_catalog::Catalog::load(&settings)?.controller;
            c.mode = case["previous"].as_i64().unwrap() as i32;
            let previous = c.selection;
            let mut calls = Vec::new();
            let changed = c.select_mode(
                case["requested"].as_i64().unwrap() as i32,
                0.7123,
                &mut |library, state| {
                    calls.push(library.profiles[state.current as usize].name.clone());
                    assert_eq!(state.duration.to_bits(), 0.7123f32.to_bits());
                    Ok(())
                },
            )?;
            assert_eq!(c.mode, case["mode"].as_i64().unwrap() as i32, "{case}");
            let expected = case["calls"].as_array().unwrap();
            assert_eq!(calls.len(), expected.len(), "{case}");
            assert_eq!(changed, !expected.is_empty());
            if let Some(call) = expected.first() {
                let slot = (call["name_offset"].as_u64().unwrap() as usize - 0x14c) / 4;
                assert_eq!(calls[0], fixture["names"][slot].as_str().unwrap());
                assert_eq!(call["selection_mode"], 1);
                assert_eq!(call["stored_mode"].as_u64().unwrap(), c.mode as u64);
            } else {
                assert_eq!(
                    c.selection, previous,
                    "Unchanged mode must preserve every transition field"
                );
            }
        }
        Ok(())
    }
}
