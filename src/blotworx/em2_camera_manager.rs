//! PC CameraManager aff0b0 defaults and source DefaultCamera ownership.
//! Default-pass pose generation, camera transitions and renderer publication
//! are separate receivers; source lens properties alone do not replace them.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Point3, Property, SceneFile, ID};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Binding {
    pub slot: usize,
    pub scene: usize,
    pub entity: u32,
    pub resource: String,
}

pub(super) fn prepare(scenes: &mut Vec<SceneFile>, names: &mut Vec<String>, templates: &super::em2_children::Templates) -> Result<Vec<Binding>> {
    ensure!(scenes.len() == names.len(), "Default camera source identities differ");
    let version = scenes.first().context("Default camera has no source scene")?.version.clone();
    let mut next = scenes.iter().flat_map(|s| &s.objects).flat_map(|e| {
        std::iter::once(e.link_id.to_u32()).chain(e.components.iter().map(|c| c.link_id.to_u32()))
    }).max().unwrap_or(0).max(templates.maximum_identity());
    let scene = scenes.len();
    let mut source = SceneFile::new(vec![], vec![], vec![], ID::new(0), version);
    let mut bindings = Vec::new();
    for slot in 0..2 {
        let mut spawn = templates.instantiate("DefaultCamera", &mut next)?;
        ensure!(spawn.entity.components.iter().all(|c| matches!(c.class_name.as_str(), "JPSTransformationComponent" | "JPSCameraComponent")), "DefaultCamera has an unsupported component lifecycle");
        ensure!(spawn.entity.components.iter().filter(|c| c.class_name == "JPSCameraComponent").count() == 1, "DefaultCamera source camera component is absent or ambiguous");
        let transform = spawn.entity.components.iter_mut().find(|c| c.class_name == "JPSTransformationComponent").context("DefaultCamera source transform is absent")?;
        // Initialize receives an explicit zero translation and null rotation.
        // Keep all other source properties, including its authored zero scale.
        let zero = Point3::new(0., 0., 0.).to_dict();
        if let Some(p) = transform.properties.iter_mut().find(|p| p.name == "Translation") { p.value = zero; }
        else { transform.properties.push(Property::new("Point3".into(), "Translation".into(), false, false, false, zero)); }
        let entity = spawn.entity.link_id.to_u32();
        source.scene.push(spawn.entity.link_id);
        source.objects.push(spawn.entity);
        bindings.push(Binding { slot, scene, entity, resource: spawn.resource });
    }
    scenes.push(source);
    names.push("<runtime default cameras>".into());
    Ok(bindings)
}

fn config_key(name: &str) -> u32 {
    let mut crc = !(name.len() as u32);
    for byte in name.bytes() {
        crc ^= byte.to_ascii_lowercase() as u32;
        for _ in 0..8 { crc = (crc >> 1) ^ (0u32.wrapping_sub(crc & 1) & 0xedb88320); }
    }
    !crc
}
fn number(settings: &BTreeMap<String, String>, hash: u32, fallback: &str) -> Result<f32> {
    let mut matches = settings.iter().filter(|(key, _)| config_key(key) == hash);
    let value = matches.next().map(|(_, value)| value.as_str()).unwrap_or(fallback);
    ensure!(matches.next().is_none(), "Ambiguous PC camera configuration hash {hash:08x}");
    super::config_value::decimal_float(value).map_err(anyhow::Error::msg)
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Lens {
    pub default_fov: f32,
    pub current_fov: f32,
    pub field_10: f32,
    pub aspect: Option<f32>,
    pub near: f32,
    pub far: f32,
}
impl Lens {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        let default_fov = number(settings, 0x7aecadc4, "90.0")?;
        Ok(Self { default_fov, current_fov: default_fov, field_10: number(settings, 0x23ca4a5e, "0.5")?, aspect: None,
            near: number(settings, 0xe021b053, "4.0")?, far: number(settings, 0x780a3266, "90.0")? })
    }
    pub fn renderer_aspect(&mut self, aspect: f32) -> Result<()> {
        ensure!(aspect.is_finite() && aspect > 0., "Invalid default-camera renderer aspect");
        self.aspect = Some(aspect); Ok(())
    }
    pub fn properties(&self) -> Result<[(&'static str, f32); 4]> {
        Ok([("Aspect Ratio", self.aspect.context("Default camera is waiting for renderer aspect")?),
            ("Field of View", self.default_fov), ("Near Clipping Plane", self.near), ("Far Clipping Plane", self.far)])
    }
}

pub(super) struct Manager {
    pub bindings: Vec<Binding>,
    pub owners: [usize; 2],
    pub lens: Lens,
    pub property_publications: u64,
}
impl Manager {
    pub fn new(bindings: Vec<Binding>, owners: [usize; 2], settings: &BTreeMap<String, String>) -> Result<Self> {
        ensure!(bindings.len() == 2 && bindings.iter().enumerate().all(|(i, b)| i == b.slot) && owners[0] != owners[1], "Invalid default-camera ownership");
        Ok(Self { bindings, owners, lens: Lens::load(settings)?, property_publications: 0 })
    }
    pub fn publish(&mut self, aspect: f32, entities: &mut [super::assets::RuntimeEntity]) -> Result<()> {
        if self.lens.aspect == Some(aspect) && self.property_publications != 0 { return Ok(()); }
        self.lens.renderer_aspect(aspect)?;
        let properties = self.lens.properties()?;
        // Both source instances must validate before changing either instance.
        for owner in self.owners {
            let camera = entities.get(owner).context("Default camera owner is absent")?.components.iter().find(|c| c.class_name == "JPSCameraComponent").context("Default camera component is absent")?;
            ensure!(properties.iter().all(|(name, _)| camera.properties.iter().filter(|p| p.name == *name).count() == 1), "Default camera lens properties are absent or ambiguous");
        }
        for owner in self.owners {
            let camera = entities[owner].components.iter_mut().find(|c| c.class_name == "JPSCameraComponent").unwrap();
            for (name, value) in properties { camera.properties.iter_mut().find(|p| p.name == name).unwrap().value = value.into(); }
        }
        self.property_publications += 1; Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"sources":self.bindings,"owners":self.owners,"lens":self.lens,"property_publications":self.property_publications,
            "scope":"Two real DefaultCamera source clones and PC lens/default property publication; default-pass view generation, transitions, cursor/ability/tracker reset receivers remain incomplete"})
    }
}
