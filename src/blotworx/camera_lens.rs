//! EM1 controller FOV and application-owned JPS projection. Source camera
//! transforms do not own the application's clip planes or renderer aspect.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use serde::Serialize;
use std::collections::BTreeMap;

const PI: f32 = f32::from_bits(0x40490fdb);
const TWO_PI: f32 = f32::from_bits(0x40c90fdb);
const RADIANS: f32 = f32::from_bits(0x3c8efa35);
fn sine(angle: f32) -> Result<f32> {
    Ok(super::native_angle::sin_cos_medium(f64::from(angle))?.0 as f32)
}
fn clamped_asin(value: f32) -> f32 {
    if value <= -1. {
        -std::f32::consts::FRAC_PI_2
    } else if value >= 1. {
        std::f32::consts::FRAC_PI_2
    } else {
        super::native_angle::asin(f64::from(value)) as f32
    }
}
/// REL1e4700..1e4748, including clamp and single-precision conversion.
pub fn source_fov(right: f32) -> Result<f32> {
    ensure!(right.is_finite(), "Nonfinite source camera extent");
    Ok((2. * clamped_asin(right)) / RADIANS)
}
/// REL347b10. Sine/asin are deliberate; this is not a tan/atan lens.
pub fn convert_aspect(source: f32, target: f32, fov: f32) -> Result<f32> {
    ensure!(
        [source, target, fov].iter().all(|v| v.is_finite()) && source > 0. && target > 0.,
        "Invalid camera aspect/FOV"
    );
    let angle = (PI * (fov * 0.5)) / 180.;
    let ratio = target * (1. / source);
    let angle = clamped_asin(ratio * sine(angle)?);
    Ok(2. * ((180. * angle) / PI))
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Properties {
    pub renderer_aspect: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub width: f32,
    pub orthographic: bool,
}
impl Properties {
    /// REL347cf4..347dc0, before NiCamera builds the matrix.
    pub fn frustum(self, fov: f32) -> Result<[f32; 6]> {
        ensure!(
            [
                self.renderer_aspect,
                self.aspect,
                self.near,
                self.far,
                self.width
            ]
            .iter()
            .all(|v| v.is_finite())
                && self.aspect > 0.,
            "Invalid application camera properties"
        );
        let converted = convert_aspect(1.2, self.renderer_aspect, fov)?;
        let right = if self.orthographic {
            self.width * 0.5
        } else {
            sine((TWO_PI * (converted * 0.5)) / 360.)?
        };
        let top = right / self.aspect;
        let result = [-right, right, top, -top, self.near, self.far];
        super::camera_projection::matrix(result, self.orthographic)?;
        Ok(result)
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Sample {
    pub field_of_view: f32,
    pub properties: Properties,
    pub letterbox: bool,
}
impl Sample {
    /// REL1d9f10..1d9f2c. Lens ownership remains the destination's current
    /// application; only the controller's scalar FOV is interpolated.
    pub fn blend(self, destination: Self, weight: f32) -> Self {
        Self {
            field_of_view: weight.mul_add(
                destination.field_of_view - self.field_of_view,
                self.field_of_view,
            ),
            ..destination
        }
    }
    /// RELad24..addc adjusts the published component FOV AFTER controller
    /// blending. Keep controller FOV raw so sampling cannot compound it.
    pub fn published_field_of_view(self) -> Result<f32> {
        if self.letterbox {
            convert_aspect(
                self.properties.renderer_aspect,
                f32::from_bits(0x3fe38e39),
                self.field_of_view,
            )
        } else {
            Ok(self.field_of_view)
        }
    }
    pub fn frustum(self) -> Result<[f32; 6]> {
        self.properties.frustum(self.published_field_of_view()?)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Application {
    pub properties: Properties,
    pub field_of_view: f32,
    pub renderer_supplied: bool,
    pub native_entity_lifecycle_complete: bool,
    pub letterbox: bool,
}
impl Application {
    pub fn load(component: &Component, settings: &BTreeMap<String, String>) -> Result<Self> {
        ensure!(
            component.class_name == "JPSCameraComponent",
            "Application requires the default camera component"
        );
        let number = |name: &str| -> Result<f32> {
            Ok(component
                .properties
                .iter()
                .find(|property| property.name == name)
                .and_then(|property| property.value.as_f64())
                .with_context(|| format!("Default camera lacks {name}"))? as f32)
        };
        let flag = |name: &str| {
            component
                .properties
                .iter()
                .find(|property| property.name == name)
                .and_then(|property| property.value.as_bool())
                .with_context(|| format!("Default camera lacks {name}"))
        };
        let setting = |name: &str, default: f32| -> Result<f32> {
            settings
                .get(name)
                .map(|text| {
                    text.parse::<f32>()
                        .with_context(|| format!("Invalid {name}"))
                })
                .transpose()
                .map(|v| v.unwrap_or(default))
        };
        // 1dadf8..1daea4 loads these values, then1dac2c publishes them
        // into the active camera. Before a renderer is supplied retain the
        // actual prefab aspect; do not guess a host viewport resolution.
        let aspect = number("Aspect Ratio")?;
        let result = Self {
            properties: Properties {
                renderer_aspect: aspect,
                aspect,
                near: setting("CameraNearClipPlane", 4.)?,
                far: setting("CameraFarClipPlane", 90.)?,
                width: number("Orthographic Frustum Width")?,
                orthographic: flag("Orthographic")?,
            },
            field_of_view: setting("CameraFieldOfView", 90.)?,
            renderer_supplied: false,
            native_entity_lifecycle_complete: false,
            letterbox: false,
        };
        result.sample(result.field_of_view).frustum()?;
        Ok(result)
    }
    pub fn set_renderer_aspect(&mut self, aspect: f32) -> Result<()> {
        ensure!(
            aspect.is_finite() && aspect > 0.,
            "Renderer camera aspect must be positive and finite"
        );
        self.properties.aspect = aspect;
        self.properties.renderer_aspect = aspect;
        self.renderer_supplied = true;
        Ok(())
    }
    /// RELbc74 compares first;bc14 toggles only when the renderer's wide
    /// predicate is false. DOL8048f164 uses strict aspect>1.6f32. In
    /// particular, a wide resize does not automatically clear an active mode.
    pub fn set_letterbox(&mut self, requested: bool) -> Result<()> {
        if self.letterbox == requested {
            return Ok(());
        }
        ensure!(
            self.renderer_supplied,
            "SetLetterbox needs the initialized renderer aspect"
        );
        if self.properties.renderer_aspect <= f32::from_bits(0x3fcccccd) {
            self.letterbox = !self.letterbox;
        }
        Ok(())
    }
    /// REL57d4..5820 forces the UI's independent cinematic bars to full
    /// progress and1/8 screen height while this mode is active. Normal HUD
    /// cinematic-bar requests/lifecycle remain a separate UI owner.
    pub fn forced_letterbox_rectangles(&self) -> Option<[[f32; 4]; 2]> {
        self.letterbox
            .then_some([[0., 0., 1., 0.125], [0., 0.875, 1., 0.125]])
    }
    pub fn sample(&self, field_of_view: f32) -> Sample {
        Sample {
            field_of_view,
            properties: self.properties,
            letterbox: self.letterbox,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_lens.json"
        ))
        .unwrap()
    }
    fn application(aspect: f32) -> Application {
        Application {
            properties: Properties {
                renderer_aspect: aspect,
                aspect,
                near: 0.25,
                far: 400.,
                width: 0.,
                orthographic: false,
            },
            field_of_view: 60.,
            renderer_supplied: true,
            native_entity_lifecycle_complete: false,
            letterbox: false,
        }
    }
    #[test]
    fn letterbox_gate_matches_strict_wide_predicate_and_idempotent_native_toggle() -> Result<()> {
        let threshold = f32::from_bits(0x3fcccccd);
        for (aspect, allowed) in [
            (4. / 3., true),
            (threshold, true),
            (f32::from_bits(threshold.to_bits() + 1), false),
            (16. / 9., false),
        ] {
            let mut app = application(aspect);
            app.set_letterbox(true)?;
            assert_eq!(app.letterbox, allowed);
            app.set_letterbox(true)?;
            assert_eq!(app.letterbox, allowed, "Repeated request toggled mode");
            app.set_letterbox(false)?;
            assert!(!app.letterbox);
        }
        let mut app = application(4. / 3.);
        app.renderer_supplied = false;
        app.set_letterbox(false)?;
        assert!(app.set_letterbox(true).is_err());
        app.renderer_supplied = true;
        app.set_letterbox(true)?;
        assert_eq!(
            app.forced_letterbox_rectangles(),
            Some([[0., 0., 1., 0.125], [0., 0.875, 1., 0.125]])
        );
        app.set_renderer_aspect(16. / 9.)?;
        app.set_letterbox(false)?;
        assert!(
            app.letterbox,
            "Wide renderer must decline a toggle in either direction"
        );
        app.set_renderer_aspect(4. / 3.)?;
        app.set_letterbox(false)?;
        assert_eq!(app.forced_letterbox_rectangles(), None);
        Ok(())
    }
    #[test]
    fn letterbox_publishes_after_blending_without_compounding_and_matches_original_bits(
    ) -> Result<()> {
        for case in reference()["letterbox"].as_array().unwrap() {
            let get = |key: &str| case[key].as_f64().unwrap() as f32;
            let mut app = application(get("renderer_aspect"));
            app.set_letterbox(true)?;
            let source = app.sample(get("source_fov"));
            let destination = app.sample(get("destination_fov"));
            let blend = source.blend(destination, get("weight"));
            assert_eq!(
                blend.field_of_view.to_bits(),
                case["raw_fov_bits"].as_u64().unwrap() as u32
            );
            assert_eq!(
                blend.published_field_of_view()?.to_bits(),
                case["published_fov_bits"].as_u64().unwrap() as u32
            );
            let expected: [u32; 6] =
                std::array::from_fn(|i| case["bits"][i].as_u64().unwrap() as u32);
            assert_eq!(blend.frustum()?.map(f32::to_bits), expected, "{case}");
            assert_eq!(
                blend.frustum()?.map(f32::to_bits),
                expected,
                "Repeated publication compounded FOV"
            );
        }
        Ok(())
    }
    #[test]
    fn source_fov_matches_original_instruction_bits() -> Result<()> {
        for case in reference()["source_fov"].as_array().unwrap() {
            assert_eq!(
                source_fov(case["right"].as_f64().unwrap() as f32)?.to_bits(),
                case["bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn asin_port_matches_original_calls_at_camera_precision() {
        for case in reference()["asin"].as_array().unwrap() {
            assert_eq!(
                (super::super::native_angle::asin(case["input"].as_f64().unwrap()) as f32)
                    .to_bits(),
                case["bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
    }
    #[test]
    fn aspect_conversion_matches_original_instruction_bits() -> Result<()> {
        for case in reference()["conversions"].as_array().unwrap() {
            let get = |key: &str| case[key].as_f64().unwrap() as f32;
            assert_eq!(
                convert_aspect(get("source"), get("target"), get("fov"))?.to_bits(),
                case["bits"].as_u64().unwrap() as u32,
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn application_frustum_matches_original_projection_setup_bits() -> Result<()> {
        for case in reference()["projections"].as_array().unwrap() {
            let get = |key: &str| case[key].as_f64().unwrap() as f32;
            let properties = Properties {
                renderer_aspect: get("renderer_aspect"),
                aspect: get("aspect"),
                near: get("near"),
                far: get("far"),
                width: get("width"),
                orthographic: case["orthographic"].as_bool().unwrap(),
            };
            let expected: [u32; 6] =
                std::array::from_fn(|i| case["bits"][i].as_u64().unwrap() as u32);
            assert_eq!(
                properties.frustum(get("fov"))?.map(f32::to_bits),
                expected,
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn blend_uses_fused_scalar_fov_instead_of_interpolating_extents() -> Result<()> {
        let properties = Properties {
            renderer_aspect: 16. / 9.,
            aspect: 16. / 9.,
            near: 0.25,
            far: 400.,
            width: 0.,
            orthographic: false,
        };
        let source = Sample {
            field_of_view: 40.,
            properties,
            letterbox: false,
        };
        let destination = Sample {
            field_of_view: 100.,
            properties,
            letterbox: false,
        };
        let blend = source.blend(destination, 0.37);
        assert_eq!(
            blend.field_of_view.to_bits(),
            0.37f32.mul_add(60., 40.).to_bits()
        );
        let extents = 0.37f32.mul_add(
            destination.frustum()?[1] - source.frustum()?[1],
            source.frustum()?[1],
        );
        assert_ne!(blend.frustum()?[1].to_bits(), extents.to_bits());
        assert!(source_fov(f32::NAN).is_err());
        assert!(convert_aspect(0., 1., 60.).is_err());
        assert!(Properties {
            near: 0.,
            ..properties
        }
        .frustum(60.)
        .is_err());
        Ok(())
    }
}
