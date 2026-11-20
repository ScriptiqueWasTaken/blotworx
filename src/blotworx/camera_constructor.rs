//! Derived d18e0..d6ad0 scalar initialization and typed property ownership.
//! Native pointer identities are metadata, never relocated Wii addresses.
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};
#[derive(Deserialize)]
struct Property {
    default: String,
}
#[derive(Deserialize)]
struct Copy {
    kind: String,
    offsets: Vec<usize>,
    key: String,
}
#[derive(Deserialize)]
struct Layout {
    literal_bytes: BTreeMap<usize, u8>,
    property_slots: BTreeMap<usize, String>,
    names: BTreeMap<usize, String>,
    spring_slots: Vec<usize>,
    properties: BTreeMap<String, Property>,
    copies: Vec<Copy>,
}
static LAYOUT: LazyLock<Layout> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/camera_constructor_layout.json"
    ))
    .expect("Validated camera constructor layout")
});
fn text<'a>(key: &str, settings: &'a BTreeMap<String, String>) -> Result<&'a str> {
    Ok(settings.get(key).map(String::as_str).unwrap_or(
        LAYOUT
            .properties
            .get(key)
            .context("Unregistered constructor property")?
            .default
            .as_str(),
    ))
}
fn number(key: &str, settings: &BTreeMap<String, String>) -> Result<f32> {
    let text = text(key, settings)?;
    let v = match text {
        "true" => 1.,
        "false" => 0.,
        _ => text
            .strip_suffix('f')
            .unwrap_or(text)
            .parse::<f64>()
            .with_context(|| {
                format!("Unsupported native camera constructor number {key}: {text}")
            })? as f32,
    };
    ensure!(v.is_finite(), "Nonfinite camera constructor property {key}");
    Ok(v)
}
/// The gameplay controller registers properties before its pitch wrapper.
/// Preserve an existing registration's default when both share a key.
pub(super) fn registered_number(key: &str, settings: &BTreeMap<String, String>) -> Result<f32> {
    if LAYOUT.properties.contains_key(key) {
        return number(key, settings);
    }
    let default = super::camera_wrapper::LIMIT_REGISTRATIONS
        .iter()
        .find(|entry| entry.0 == key)
        .context("Unregistered gameplay/wrapper camera property")?
        .1;
    let text = settings.get(key).map(String::as_str).unwrap_or(default);
    let value = text
        .strip_suffix('f')
        .unwrap_or(text)
        .parse::<f64>()
        .with_context(|| format!("Invalid wrapper camera property {key}"))? as f32;
    ensure!(value.is_finite(), "Nonfinite wrapper camera property {key}");
    Ok(value)
}
pub(super) fn registered_slot_number(
    offset: usize,
    settings: &BTreeMap<String, String>,
) -> Result<f32> {
    registered_number(
        LAYOUT
            .property_slots
            .get(&offset)
            .with_context(|| format!("Unregistered native camera property slot {offset:x}"))?,
        settings,
    )
}
pub(super) fn registered_boolean(key: &str, settings: &BTreeMap<String, String>) -> Result<bool> {
    let raw = if key == super::camera_base::DPAD_STEERING.0 {
        settings
            .get(key)
            .map(String::as_str)
            .unwrap_or(super::camera_base::DPAD_STEERING.1)
    } else {
        text(key, settings)?
    };
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => {
            let value = raw
                .strip_suffix('f')
                .unwrap_or(raw)
                .parse::<f64>()
                .with_context(|| format!("Invalid camera boolean {key}"))?;
            ensure!(
                value.is_finite()
                    && value.trunc() >= i32::MIN as f64
                    && value.trunc() <= i32::MAX as f64,
                "Unsupported camera boolean conversion {key}"
            );
            Ok(value.trunc() as i32 != 0)
        }
    }
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn word(s: &mut [u8], off: usize, v: u32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn initialize_raw(
    s: &mut [u8],
    settings: &BTreeMap<String, String>,
    unscaled_counter: u64,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Camera constructor allocation too short");
    for (&i, &byte) in &LAYOUT.literal_bytes {
        s[i] = byte;
    }
    // These helpers intentionally retain damping if the old coefficient is
    // already MAX. Input allocation contents are not silently overwritten.
    for &off in &LAYOUT.spring_slots {
        super::camera_attributes::coefficient(s, off, f32::MAX);
    }
    for copy in &LAYOUT.copies {
        let value = number(&copy.key, settings)?;
        for &off in &copy.offsets {
            match copy.kind.as_str() {
                "float" => put(s, off, value),
                "tan" => put(
                    s,
                    off,
                    super::native_angle::tan_medium(value as f64)? as f32,
                ),
                "integer" | "boolean" => {
                    let truncated = (value as f64).trunc();
                    ensure!(
                        (i32::MIN as f64..=i32::MAX as f64).contains(&truncated),
                        "Camera constructor integer conversion unsupported"
                    );
                    let v = truncated as i32;
                    if copy.kind == "boolean" {
                        s[off] = (v != 0) as u8;
                    } else {
                        word(s, off, v as u32);
                    }
                }
                "spring" => {
                    // These cache pairs have explicit +zero initialization before
                    // the property read. Equal values (including -zero) retain
                    // those bits; the constructor's inlined helper is conditional.
                    s[off..off + 8].fill(0);
                    super::camera_attributes::coefficient(s, off, value);
                }
                _ => anyhow::bail!("Unknown native constructor copy operation"),
            }
        }
    }
    //14bb9c with duration0 copies the current UNSCALED counter (+28), with
    // a nonzero marker when that counter is zero. No wall-clock substitution.
    let timestamp = if unscaled_counter == 0 {
        1
    } else {
        unscaled_counter
    };
    for off in [0x470, 0x480] {
        s[off..off + 8].copy_from_slice(&timestamp.to_be_bytes());
    }
    Ok(())
}
pub struct Owner {
    storage: Box<[u8; 0x800]>,
    known: Box<[bool; 0x800]>,
}
impl Owner {
    /// e2cd8..e2eb4 after earlier e2998 acquisition/mode work. Require only
    /// fields consumed on the selected native branch. Inactive spring damping
    /// can legitimately remain uninitialized after construction.
    pub(super) fn advance_desired_radius(
        &mut self,
        catalog: &super::camera_catalog::Catalog,
        settings: &BTreeMap<String, String>,
    ) -> Result<f32> {
        self.sync_selection(&catalog.controller);
        super::camera_desired_polar::radius_for_profile_checked(
            &mut self.storage[..],
            Some(&mut self.known[..]),
            catalog,
            settings,
        )
    }
    /// d9860 follows target preparation. Require the lifecycle-owned target
    /// history and clocks before reading them; do not invent cold-start values.
    pub(super) fn advance_generator_timing(
        &mut self,
        input: [f32; 2],
        delta: f32,
        actor: impl FnOnce() -> Result<[f32; 2]>,
    ) -> Result<super::camera_generator_timing::Activity> {
        for (off, n) in [(0x2d0, 12), (0x2e0, 12), (0x444, 20)] {
            self.require(off, n)?;
        }
        let result =
            super::camera_generator_timing::advance(&mut self.storage[..], input, delta, actor);
        // Actor lookup failures occur after the camera-input/target clocks.
        self.mark(0x444, 12);
        if let Ok(activity) = &result {
            self.mark(0x450, 8);
            if !activity.actor_input_active {
                self.mark(0x424, 4);
            }
        }
        result
    }
    pub(super) fn generator_gate_snapshot(
        &self,
        target_moved: bool,
        settings: &BTreeMap<String, String>,
    ) -> Result<super::camera_yaw_pass::Gates> {
        self.require(0x44c, 4)?;
        super::camera_generator_timing::snapshot(
            &self.storage[..],
            target_moved,
            registered_number("FreeCamera_Input_HoldWait", settings)?,
        )
    }
    pub(super) fn apply_clear_command(
        &mut self,
        command: super::camera_clear::Camera,
    ) -> Result<()> {
        use super::{
            camera_attributes::{self, Setter},
            camera_clear::Camera,
        };
        match command {
            Camera::Attribute(setter, values, selector) => {
                ensure!(
                    values.iter().all(|v| v.is_finite()),
                    "Nonfinite owned camera attribute input"
                );
                let mut writes = Vec::new();
                let mut add = |off, n| writes.push((off, n));
                match setter {
                    Setter::Overrides => {
                        for (i, off) in [0x5c0, 0x5c4, 0x5c8, 0x5d8, 0x5d4].into_iter().enumerate()
                        {
                            if values[i] > -f32::MAX {
                                add(off, 4);
                                if i == 1 {
                                    for off in [0x390, 0x43c, 0x5f8] {
                                        add(off, 4);
                                    }
                                }
                            }
                        }
                    }
                    Setter::Springs => {
                        for (i, off) in [0x608, 0x5f0, 0x5fc, 0x620, 0x614].into_iter().enumerate()
                        {
                            if values[i] > -f32::MAX && self.scalar(off)? != values[i] {
                                add(off, 8);
                            }
                        }
                    }
                    Setter::Offsets => {
                        for i in 0..3 {
                            if values[i] > -f32::MAX {
                                add(0x5e0 + i * 4, 4);
                            }
                        }
                    }
                    Setter::Lookahead => {
                        add(0x6e8, 1);
                        for (i, off) in [0x70c, 0x710].into_iter().enumerate() {
                            if values[i] > -f32::MAX {
                                add(off, 4);
                            }
                        }
                    }
                    Setter::Ledge => {
                        for (i, off) in [0x6ac, 0x6b0].into_iter().enumerate() {
                            if values[i] > -f32::MAX {
                                add(off, 4);
                            }
                        }
                    }
                    Setter::AutoCenter => {
                        add(0x714, 1);
                        if values[0] > -f32::MAX {
                            add(0x728, 4);
                        }
                    }
                    Setter::AutoLevel => add(0x738, 1),
                    Setter::JumpPitch => add(0x750, 1),
                    Setter::TargetScalar => {
                        if values[0] > -f32::MAX {
                            add(if selector != 0 { 0x214 } else { 0x234 }, 4);
                        }
                    }
                }
                camera_attributes::apply(&mut self.storage[..], setter, values, selector)?;
                for (off, n) in writes {
                    self.mark(off, n);
                }
                Ok(())
            }
            Camera::ClearControl(mask) => {
                self.require(0x634, 4)?;
                let value =
                    u32::from_be_bytes(self.storage[0x634..0x638].try_into().unwrap()) & !mask;
                word(&mut self.storage[..], 0x634, value);
                self.mark(0x634, 4);
                Ok(())
            }
            Camera::ClearPrimaryOverride => {
                self.require(0x204, 4)?;
                ensure!(u32::from_be_bytes(self.storage[0x204..0x208].try_into().unwrap())==0,
                    "Clearing a live primary camera override requires native e5ad8 lookahead notification");
                for off in [0x204, 0x208, 0x20c] {
                    word(&mut self.storage[..], off, 0);
                    self.mark(off, 4);
                }
                for off in [0x21c, 0x214] {
                    put(&mut self.storage[..], off, f32::MAX);
                    self.mark(off, 4);
                }
                for off in [0x218, 0x211, 0x210] {
                    self.storage[off] = 0;
                    self.mark(off, 1);
                }
                put(&mut self.storage[..], 0x220, 0.);
                self.mark(0x220, 4);
                Ok(())
            }
        }
    }
    pub fn new(
        settings: &BTreeMap<String, String>,
        catalog: &super::camera_catalog::Catalog,
        unscaled_counter: u64,
    ) -> Result<Self> {
        // Rust backing bytes are not a claim about the native allocator. The
        // validity map controls which scalar values may actually be consumed.
        let mut storage = Box::new([0; 0x800]);
        initialize_raw(&mut storage[..], settings, unscaled_counter)?;
        let mut owner = Self {
            storage,
            known: Box::new([false; 0x800]),
        };
        for &i in LAYOUT.literal_bytes.keys() {
            owner.known[i] = true;
        }
        for &off in &LAYOUT.spring_slots {
            owner.mark(off, 4);
        }
        for copy in &LAYOUT.copies {
            for &off in &copy.offsets {
                owner.mark(
                    off,
                    if copy.kind == "boolean" {
                        1
                    } else if copy.kind == "spring" {
                        8
                    } else {
                        4
                    },
                );
            }
        }
        for off in [0x470, 0x480] {
            owner.mark(off, 8);
        }
        owner.sync_selection(&catalog.controller);
        Ok(owner)
    }
    fn mark(&mut self, off: usize, n: usize) {
        self.known[off..off + n].fill(true);
    }
    fn require(&self, off: usize, n: usize) -> Result<()> {
        ensure!(
            off.checked_add(n)
                .is_some_and(|end| end <= self.known.len())
                && self.known[off..off + n].iter().all(|v| *v),
            "Camera owner field {off:x}..{:x} has not been initialized by its native lifecycle",
            off + n
        );
        Ok(())
    }
    pub fn scalar(&self, off: usize) -> Result<f32> {
        self.require(off, 4)?;
        Ok(f32::from_be_bytes(
            self.storage[off..off + 4].try_into().unwrap(),
        ))
    }
    pub fn property_text<'a>(
        &self,
        off: usize,
        settings: &'a BTreeMap<String, String>,
    ) -> Result<&'a str> {
        text(
            LAYOUT
                .property_slots
                .get(&off)
                .with_context(|| format!("Unbound native camera property slot {off:x}"))?,
            settings,
        )
    }
    fn sync_selection(&mut self, c: &super::camera_profiles::Controller) {
        let s = c.selection;
        for (off, value) in [
            (0x14, s.current as u32),
            (0x18, s.previous as u32),
            (0x15c, c.mode as u32),
        ] {
            word(&mut self.storage[..], off, value);
            self.mark(off, 4);
        }
        for (off, value) in [(0x1c, s.weight), (0x20, s.duration), (0x24, s.elapsed)] {
            put(&mut self.storage[..], off, value);
            self.mark(off, 4);
        }
        self.storage[0x28] = s.immediate as u8;
        self.mark(0x28, 1);
        for i in 0..3 {
            self.storage[0x160 + i] = c.enabled[i] as u8;
            self.mark(0x160 + i, 1);
        }
    }
    fn reset_readiness(&self) -> Result<()> {
        for (off, n) in [(0x14, 4), (0x15c, 4), (0x280, 1), (0x350, 16), (0x648, 4)] {
            self.require(off, n)?;
        }
        Ok(())
    }
    fn reset_complete(&mut self) {
        for (off, n) in [
            (0x2d0, 32),
            (0x634, 4),
            (0x384, 8),
            (0x1bc, 1),
            (0x28c, 12),
            (0x4ac, 1),
            (0x4b0, 4),
            (0x4b4, 1),
            (0x5a8, 4),
            (0x358, 4),
            (0x360, 16),
            (0x5a0, 4),
            (0x370, 20),
            (0x760, 8),
            (0x73c, 4),
            (0x5b0, 16),
            (0x1be, 2),
            (0x5c0, 12),
            (0x5d8, 4),
            (0x5d4, 4),
            (0x390, 4),
            (0x43c, 4),
            (0x5f8, 4),
            (0x14, 21),
            (0x646, 1),
            (0x7f1, 1),
            (0x200, 4),
        ] {
            self.mark(off, n);
        }
        for base in [0x204, 0x224] {
            for (off, n) in [
                (0, 12),
                (0x18, 4),
                (0x10, 4),
                (0x14, 1),
                (0xc, 2),
                (0x1c, 4),
            ] {
                self.mark(base + off, n);
            }
        }
        for off in [
            0x3ac, 0x3b8, 0x3d0, 0x3e4, 0x3f4, 0x404, 0x414, 0x254, 0x708, 0x734, 0x75c, 0x684,
        ] {
            self.mark(off, 4);
        }
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"scalar_bytes_initialized":self.known.iter().filter(|v|**v).count(),"property_bindings":LAYOUT.property_slots.len(),
            "registered_properties":LAYOUT.properties.len(),"construction_names":LAYOUT.names,
            "has_unfiltered_target":self.require(0x310,16).is_ok(),"reset_inputs_ready":self.reset_readiness().is_ok(),
            "conditional_spring_damping_unresolved":LAYOUT.spring_slots.iter().filter(|&&off|self.require(off+4,4).is_err()).count(),
            "named_resource_request":"FreeCamera_Lock_IndicatorTemplateName","generator_connected":false})
    }
}
impl super::assets::World {
    ///e5558 seeds the gameplay controller during c038. Preparing the owner
    ///does not select it or claim the missing default generator is available.
    pub(super) fn seed_owned_camera_controller(
        &mut self,
        information: super::camera_base::Information,
    ) -> Result<()> {
        let mut owner = self
            .camera_pipeline
            .derived
            .take()
            .context("Derived camera owner absent")?;
        owner.sync_selection(&self.camera_profiles.controller);
        let result = owner
            .require(0x35c, 4)
            .and_then(|_| self.seed_camera_controller(&mut owner.storage[..], information));
        if result.is_ok() {
            for (off, n) in [
                (0x350, 32),
                (0x370, 24),
                (0x5a0, 16),
                (0x5b4, 8),
                (0x5c0, 12),
                (0x5d8, 4),
                (0x5d4, 4),
                (0x390, 4),
                (0x43c, 4),
                (0x5f8, 4),
            ] {
                owner.mark(off, n);
            }
        }
        self.camera_pipeline.derived = Some(owner);
        result
    }
    ///70878..7091c only. The complete API must also execute its player/input/
    ///guardian effects and real default-camera handoff before returning success.
    pub(super) fn clear_camera_scalar_prefix(&mut self, duration: f32) -> Result<()> {
        let owner = self
            .camera_pipeline
            .derived
            .as_mut()
            .context("Derived camera owner missing")?;
        super::camera_clear::prefix(duration, |command| owner.apply_clear_command(command))
    }
    ///10a54 resolves EntityTrackerComponent, not an arbitrary scene entity.
    pub(super) fn restore_camera_tracker_ground_plane(&mut self) -> Result<()> {
        let current = usize::try_from(self.camera_profiles.controller.selection.current)?;
        let target = self
            .camera_profiles
            .extras
            .get(current)
            .context("Camera profile missing")?
            .target;
        let Some(target) =
            target.filter(|i| *i < self.entities.len() && !self.removed_entities.contains(i))
        else {
            return Ok(());
        };
        if !self.entities[target]
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(());
        };
        let tracker = self
            .mickey_tracker
            .as_mut()
            .filter(|t| t.owner == target)
            .context("Camera target tracker is not bound")?;
        tracker.parameters.ground_on = true;
        let component = self.entities[target]
            .components
            .iter_mut()
            .find(|c| c.class_name == "EntityTrackerComponent")
            .unwrap();
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "GroundPlaneOn")
            .context("Tracker reflected GroundPlaneOn is missing")?
            .value = serde_json::json!(true);
        Ok(())
    }
    pub(super) fn mirror_camera_target_acquisition(
        &mut self,
        frame: super::camera_generator_target::Frame,
    ) {
        if let Some(owner) = &mut self.camera_pipeline.derived {
            owner.sync_selection(&self.camera_profiles.controller);
            for (off, v) in [
                (0x300, frame.published),
                (0x2f0, frame.adjusted),
                (0x310, frame.unfiltered),
            ] {
                for i in 0..4 {
                    put(&mut owner.storage[..], off + 4 * i, v[i]);
                }
                owner.mark(off, 16);
            }
        }
    }
    pub(super) fn begin_owned_camera_target_tick(&mut self) -> Result<()> {
        let owner = self
            .camera_pipeline
            .derived
            .as_mut()
            .context("Derived camera owner absent during acquisition")?;
        owner.require(0x300, 16)?;
        owner.storage.copy_within(0x300..0x310, 0x2d0);
        owner.mark(0x2d0, 16);
        owner.storage[0x1be] = self.camera_feedback.airborne as u8;
        owner.mark(0x1be, 1);
        owner.storage[0x1bc] = self.camera_feedback.spin_follow_active as u8;
        owner.mark(0x1bc, 1);
        Ok(())
    }
    pub(super) fn reset_owned_camera_controller(&mut self) -> Result<()> {
        let mut owner = self
            .camera_pipeline
            .derived
            .take()
            .context("Derived camera owner absent")?;
        owner.sync_selection(&self.camera_profiles.controller);
        let result = owner
            .reset_readiness()
            .and_then(|_| self.reset_camera_controller(&mut owner.storage[..]));
        if result.is_ok() {
            owner.reset_complete();
        }
        // On errors earlier raw writes remain, but unknown bytes are not
        // optimistically declared initialized from a partially run callback.
        self.camera_pipeline.derived = Some(owner);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructor_copies_sentinels_and_retained_allocation_match_original() -> Result<()> {
        let d: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_constructor.json"
        ))?;
        for c in d["cases"].as_array().unwrap() {
            let seed = c["seed"].as_u64().unwrap() as u32;
            let pattern = if seed <= 255 {
                vec![seed as u8]
            } else {
                seed.to_be_bytes().to_vec()
            };
            let mut s = (0..0x800)
                .map(|i| pattern[i % pattern.len()])
                .collect::<Vec<_>>();
            let mut expected = s.clone();
            for (off, v) in c["scalar_bytes"].as_object().unwrap() {
                expected[off.parse::<usize>()?] = v.as_u64().unwrap() as u8;
            }
            let settings = serde_json::from_value(c["settings"].clone())?;
            initialize_raw(&mut s, &settings, 0)?;
            for i in 0x140..0x800 {
                let opaque = c["property_slots"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .chain(c["opaque_slots"].as_object().unwrap().keys())
                    .any(|k| {
                        let off = k.parse::<usize>().unwrap();
                        i >= off && i < off + 4
                    });
                if !opaque {
                    assert_eq!(
                        s[i], expected[i],
                        "Seed{seed:x}, offset{i:x}, settings{settings:?}"
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn native_tan_kernel_matches_original_double_instructions() -> Result<()> {
        let d: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/native_tan.json"
        ))?;
        for c in d["cases"].as_array().unwrap() {
            let x = f64::from_bits(c["x_bits"].as_u64().unwrap());
            let y = f64::from_bits(c["y_bits"].as_u64().unwrap());
            let mode = c["mode"].as_i64().unwrap() as i32;
            assert_eq!(
                super::super::native_angle::kernel_tan(x, y, mode).to_bits(),
                c["output_bits"].as_u64().unwrap(),
                "tan({x},{y},{mode})"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_owns_constructor_state_and_resets_without_a_fake_pose() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .camera_pipeline
            .derived
            .as_ref()
            .context("World has no constructor owner")?;
        assert_eq!(owner.scalar(0x350)?, 0.);
        assert_eq!(
            owner.scalar(0x648)?,
            number("CameraFieldOfView", &world.runtime_settings)?
        );
        assert_eq!(
            owner.scalar(0x360)?,
            number(
                "FreeCamera_MickeyHideCameraDistance",
                &world.runtime_settings
            )?
        );
        assert!(owner.scalar(0x310).is_err());
        assert!(owner.scalar(0x5f4).is_err());
        owner.reset_readiness()?;
        assert_eq!(
            owner.property_text(0x2b0, &world.runtime_settings)?,
            text("FreeCamera_Thinned_Collides", &world.runtime_settings)?
        );
        let before = world.camera_pipeline.application;
        let publications = world.camera_pipeline.publications;
        world.reset_owned_camera_controller()?;
        let owner = world.camera_pipeline.derived.as_ref().unwrap();
        assert!(owner.scalar(0x310).is_err());
        assert_eq!(
            owner.scalar(0x350)?,
            0.,
            "Reset must not replace radius with a guessed eye distance"
        );
        assert_eq!(
            owner.scalar(0x5a0)?,
            world
                .camera_profiles
                .controller
                .library
                .evaluate(&world.camera_profiles.controller.selection, 1)?
        );
        assert_eq!(world.camera_pipeline.application, before);
        assert_eq!(world.camera_pipeline.publications, publications);
        assert_eq!(
            owner.scalar(0x2d0)?.to_bits(),
            world.entities[world.camera_profiles.extras
                [world.camera_profiles.controller.selection.current as usize]
                .target
                .unwrap()]
            .transform[12]
                .to_bits()
        );
        world.select_nif_camera(None, None, 1, 0.)?;
        let player =
            super::super::physics::Player::new(world.report.spawn, world.report.motion.clone());
        world.update_gameplay_camera_targets(&player);
        ensure!(
            world.gameplay_camera_targets.error.is_none(),
            "Target prefix failed: {:?}",
            world.gameplay_camera_targets.error
        );
        let frame = world
            .gameplay_camera_targets
            .frame
            .context("No acquired target frame")?;
        let owner = world.camera_pipeline.derived.as_ref().unwrap();
        for (off, v) in [
            (0x300, frame.published),
            (0x2f0, frame.adjusted),
            (0x310, frame.unfiltered),
            (0x2d0, frame.published),
        ] {
            for i in 0..4 {
                assert_eq!(owner.scalar(off + i * 4)?.to_bits(), v[i].to_bits());
            }
        }
        let owner = world.camera_pipeline.derived.as_mut().unwrap();
        let idle_before = owner.scalar(0x44c)?;
        let activity =
            owner.advance_generator_timing([0.; 2], super::super::STEP, || Ok([0.; 2]))?;
        assert!(!activity.actor_input_active);
        assert_eq!(
            owner.scalar(0x44c)?.to_bits(),
            (idle_before + super::super::STEP).to_bits()
        );
        assert_eq!(owner.scalar(0x450)?, 0.);
        assert_eq!(owner.scalar(0x424)?, 0.);
        let gates =
            owner.generator_gate_snapshot(activity.target_moved, &world.runtime_settings)?;
        assert_eq!(
            gates.input_hold_elapsed,
            owner.scalar(0x44c)?
                >= registered_number("FreeCamera_Input_HoldWait", &world.runtime_settings)?
        );
        let idle_before = owner.scalar(0x44c)?;
        let actor_timers = [
            owner.scalar(0x450)?.to_bits(),
            owner.scalar(0x454)?.to_bits(),
        ];
        assert!(owner
            .advance_generator_timing([0.; 2], super::super::STEP, || anyhow::bail!(
                "controlled actor lookup failure"
            ))
            .is_err());
        assert_eq!(
            owner.scalar(0x44c)?.to_bits(),
            (idle_before + super::super::STEP).to_bits()
        );
        assert_eq!(
            [
                owner.scalar(0x450)?.to_bits(),
                owner.scalar(0x454)?.to_bits()
            ],
            actor_timers
        );
        assert_eq!(world.camera_pipeline.application, before);
        assert_eq!(world.camera_pipeline.publications, publications);
        Ok(())
    }
}
