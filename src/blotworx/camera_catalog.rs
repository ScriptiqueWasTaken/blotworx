//! Native profile initialization: base1df7f8 and derived d49b0..d4bf0.
//! Values come from the imported configuration; no game paths are embedded.
use super::{
    camera_profile_selection::State,
    camera_profiles::{Controller, Library, Profile, Property, FIELDS},
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub const INITIAL_PROFILES: [(&str, i32); 13] = [
    ("Base", 1),
    ("Intermediate", 1),
    ("ControlPoint", 2),
    ("Rolling", 1),
    ("Missile", 1),
    ("Strafing", 1),
    ("Near", 1),
    ("Default", 1),
    ("Travel", 1),
    ("Far", 1),
    ("CustomDefault", 1),
    ("CustomCombat", 1),
    ("CustomTravel", 1),
];
const FLAGS: [&str; 3] = ["VerticalInvert", "HorizontalInvert", "MaintainPitch"];
#[derive(Clone, Debug)]
pub struct Flag {
    pub text: String,
    pub value: bool,
}
#[derive(Clone, Debug)]
pub struct Extras {
    /// Native profile+38 starts null, independently of transform parenting.
    pub target: Option<usize>,
    pub frame_yaw: f32,
    pub frame_pitch: f32,
    pub flags: [usize; 3],
    pub kind: i32,
    /// Profile+48 is initialized once from MaxDist at1df538..1df544.
    pub cached_distance: f32,
}
pub struct Catalog {
    pub controller: Controller,
    pub extras: Vec<Extras>,
    pub flags: Vec<Flag>,
    pub scalar_keys: BTreeMap<String, usize>,
    pub flag_keys: BTreeMap<String, usize>,
}
fn key(profile: &str, field: &str) -> String {
    format!("FreeCamera_CameraData_{profile}_{field}")
}
fn number(text: &str) -> Result<f32> {
    // Supported cooked decimal syntax. Do not silently emulate malformed atof
    // prefixes, nonfinite payloads or arbitrary ConfigProperty vector syntax.
    let value = text
        .parse::<f64>()
        .context("Unsupported camera numeric configuration")? as f32;
    ensure!(value.is_finite(), "Nonfinite camera configuration");
    Ok(value)
}
fn flag(text: &str) -> Result<bool> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => {
            let value = number(text)?.trunc();
            ensure!(
                value >= i32::MIN as f32 && (value as f64) <= i32::MAX as f64,
                "Camera boolean integer conversion unsupported"
            );
            Ok(value as i32 != 0)
        }
    }
}
impl Catalog {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        let mut library = Library::default();
        let mut scalar_keys = BTreeMap::new();
        let mut base = [0; 10];
        for (lane, field) in FIELDS.iter().enumerate() {
            let key = key("Base", field);
            let text = settings
                .get(&key)
                .with_context(|| format!("Missing camera property {key}"))?;
            base[lane] = library.properties.len();
            scalar_keys.insert(key, base[lane]);
            library.properties.push(Property {
                text: text.clone(),
                value: number(text)?,
                minimum: -f32::MAX,
                maximum: f32::MAX,
            });
        }
        library.profiles.push(Profile {
            name: "Base".into(),
            properties: base,
        });
        let mut flags = Vec::new();
        let mut flag_keys = BTreeMap::new();
        let mut base_flags = [0; 3];
        for (lane, field) in FLAGS.iter().enumerate() {
            let key = key("Base", field);
            let text = settings
                .get(&key)
                .with_context(|| format!("Missing camera property {key}"))?;
            base_flags[lane] = flags.len();
            flag_keys.insert(key, flags.len());
            flags.push(Flag {
                text: text.clone(),
                value: flag(text)?,
            });
        }
        let base_extra = Extras {
            target: None,
            frame_yaw: settings
                .get(&key("Base", "FrameYaw"))
                .map(|s| number(s))
                .transpose()?
                .unwrap_or(0.),
            frame_pitch: settings
                .get(&key("Base", "FramePitch"))
                .map(|s| number(s))
                .transpose()?
                .unwrap_or(0.),
            flags: base_flags,
            kind: 1,
            cached_distance: library.properties[base[1]].value,
        };
        let mut result = Self {
            controller: Controller {
                library,
                selection: State::default(),
                mode: 1,
                mode_zero_name: "Near".into(),
                originals: ["Default".into(), "Far".into(), "Travel".into()],
                customs: [
                    "CustomDefault".into(),
                    "CustomCombat".into(),
                    "CustomTravel".into(),
                ],
                enabled: [false; 3],
            },
            extras: vec![base_extra],
            flags,
            scalar_keys,
            flag_keys,
        };
        for (name, kind) in INITIAL_PROFILES.into_iter().skip(1) {
            result.register(settings, name, kind)?;
        }
        Ok(result)
    }
    /// Base target setter1e0ad8: all profiles or only the current profile.
    /// The derived owner refreshes its subscription after these writes.
    pub fn assign_target(&mut self, target: Option<usize>, all: bool) -> Result<()> {
        if all {
            for profile in &mut self.extras {
                profile.target = target;
            }
        } else {
            self.extras
                .get_mut(usize::try_from(self.controller.selection.current)?)
                .context("Camera current target profile missing")?
                .target = target;
        }
        Ok(())
    }
    /// 1e0c04 reuses an existing profile without changing its original kind.
    /// Every newly registered profile uses the first/Base profile as fallback.
    pub fn register(
        &mut self,
        settings: &BTreeMap<String, String>,
        name: &str,
        kind: i32,
    ) -> Result<usize> {
        if let Some(index) = self.controller.library.find(name) {
            return Ok(index);
        }
        let mut overrides = [None; 10];
        for (lane, field) in FIELDS.iter().enumerate() {
            let key = key(name, field);
            let Some(text) = settings.get(&key) else {
                continue;
            };
            let id = if let Some(&id) = self.scalar_keys.get(&key) {
                id
            } else {
                let property = Property {
                    text: text.clone(),
                    value: number(text)?,
                    minimum: -f32::MAX,
                    maximum: f32::MAX,
                };
                let id = self.controller.library.properties.len();
                self.controller.library.properties.push(property);
                self.scalar_keys.insert(key, id);
                id
            };
            overrides[lane] = Some(id);
        }
        let mut extra = self.extras[0].clone();
        extra.kind = kind;
        extra.target = None;
        // 1df6f4 reads framing values once, falling back by value (not pointer).
        if let Some(text) = settings.get(&key(name, "FrameYaw")) {
            extra.frame_yaw = number(text)?;
        }
        if let Some(text) = settings.get(&key(name, "FramePitch")) {
            extra.frame_pitch = number(text)?;
        }
        for (lane, field) in FLAGS.iter().enumerate() {
            let key = key(name, field);
            let Some(text) = settings.get(&key) else {
                continue;
            };
            extra.flags[lane] = if let Some(&id) = self.flag_keys.get(&key) {
                id
            } else {
                let value = flag(text)?;
                let id = self.flags.len();
                self.flags.push(Flag {
                    text: text.clone(),
                    value,
                });
                self.flag_keys.insert(key, id);
                id
            };
        }
        let index = self.controller.library.derive(name, 0, overrides)?;
        extra.cached_distance = self.controller.library.properties
            [self.controller.library.profiles[index].properties[1]]
            .value;
        ensure!(
            index == self.extras.len(),
            "Camera profile/extras indices diverged"
        );
        self.extras.push(extra);
        Ok(index)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> BTreeMap<String, String> {
        let mut settings = BTreeMap::new();
        for (i, field) in FIELDS.iter().enumerate() {
            settings.insert(key("Base", field), (i + 1).to_string());
        }
        for field in FLAGS {
            settings.insert(key("Base", field), "false".into());
        }
        settings
    }
    #[test]
    fn native_catalog_order_kinds_names_and_sharing() -> Result<()> {
        let mut s = settings();
        s.insert(key("Default", "MaxDist"), "4.5".into());
        s.insert(key("Default", "FrameYaw"), "15".into());
        s.insert(key("Travel", "VerticalInvert"), "true".into());
        let mut c = Catalog::load(&s)?;
        assert_eq!(
            c.controller
                .library
                .profiles
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            INITIAL_PROFILES.map(|p| p.0)
        );
        assert_eq!(c.controller.originals, ["Default", "Far", "Travel"]);
        assert_eq!(c.controller.selection, State::default());
        assert_eq!(c.controller.library.properties.len(), 11);
        assert_eq!(c.flags.len(), 4);
        assert_eq!(c.extras[2].kind, 2);
        assert_eq!(c.extras[7].cached_distance, 4.5);
        assert_eq!(c.extras[7].frame_yaw, 15.);
        assert_eq!(
            c.extras[8].frame_yaw, 0.,
            "Travel incorrectly inherited Default framing"
        );
        assert!(c.flags[c.extras[8].flags[0]].value);
        assert!(!c.flags[c.extras[7].flags[0]].value);
        assert_eq!(
            c.controller.library.profiles[8].properties[1],
            c.controller.library.profiles[0].properties[1]
        );
        assert_eq!(c.register(&s, "ControlPoint", 1)?, 2);
        assert_eq!(c.extras[2].kind, 2);
        Ok(())
    }
    #[test]
    fn missing_or_malformed_required_values_are_errors() -> Result<()> {
        let mut s = settings();
        s.remove(&key("Base", "MinDist"));
        assert!(Catalog::load(&s).is_err());
        s.insert(key("Base", "MinDist"), "not-a-number".into());
        assert!(Catalog::load(&s).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original ConfigFiles.ini in BLOTWORX_TEST_DATA"]
    fn original_cooked_camera_section_populates_native_catalog() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let text = std::fs::read_to_string(std::path::Path::new(&root).join("ConfigFiles.ini"))?;
        // Fixture-only extraction; production uses the application's existing
        // section-aware configuration parser before passing its settings map.
        let mut section = "";
        let mut settings = BTreeMap::new();
        for line in text.lines().map(str::trim) {
            if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = s;
                continue;
            }
            if section == "FreeCamera_CameraData" {
                if let Some((name, value)) = line.split_once('=') {
                    settings.insert(
                        format!("{section}_{}", name.trim()),
                        value.trim().to_owned(),
                    );
                }
            }
        }
        let c = Catalog::load(&settings)?;
        let value = |name: &str, lane: usize| {
            let p = c.controller.library.find(name).unwrap();
            c.controller.library.properties[c.controller.library.profiles[p].properties[lane]].value
        };
        assert_eq!(value("Base", 1), 5.5);
        assert_eq!(value("Default", 1), 4.5);
        assert_eq!(value("Default", 8), 4.4);
        assert_eq!(value("CustomDefault", 7), 10.);
        assert_eq!(value("ControlPoint", 0), 0.1);
        assert_eq!(value("Far", 1), 8.);
        assert_eq!(c.controller.library.profiles.len(), 13);
        Ok(())
    }
}
