//! Runtime light ownership for the enhanced renderer. Editor catalogs include
//! unplaced prototypes; this registry admits constructed, active entities only.
use super::{
    assets::{RuntimeEntity, World},
    spherical_lighting::{self, LightKind},
};
use crate::render_support::viewport3d::GpuLight;
use anyhow::{Context, Result};
#[derive(Default, serde::Serialize)]
pub struct Registry {
    scanned: usize,
    owners: Vec<(usize, usize)>,
    pub admitted: usize,
    pub disabled: usize,
    pub baked_only: usize,
}
impl Registry {
    fn refresh(&mut self, entities: &[RuntimeEntity]) {
        if entities.len() < self.scanned {
            self.owners.clear();
            self.scanned = 0;
        }
        for (owner, e) in entities.iter().enumerate().skip(self.scanned) {
            for (index, c) in e.components.iter().enumerate() {
                if c.class_name == "JPSLightComponent" {
                    self.owners.push((owner, index));
                }
            }
        }
        self.scanned = entities.len();
    }
}
impl World {
    pub(super) fn enhanced_scene_lights(&mut self, style: u32) -> Result<Vec<GpuLight>> {
        self.enhanced_light_registry.refresh(&self.entities);
        let mut lights = Vec::new();
        let mut disabled = 0;
        let mut baked_only = 0;
        for &(owner, index) in &self.enhanced_light_registry.owners {
            if self.removed_entities.contains(&owner)
                || !self.entity_scene_active(owner)
                || self
                    .component_control
                    .disabled
                    .get(&owner)
                    .and_then(|f| f.get(&index))
                    .copied()
                    .unwrap_or(false)
            {
                disabled += 1;
                continue;
            }
            let e = &self.entities[owner];
            let c = &e.components[index];
            let baking = e
                .components
                .iter()
                .find(|c| c.class_name == "JPSLightBakingComponent");
            let prop = |name: &str| {
                c.properties
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| &p.value)
            };
            let bake = |name: &str| {
                baking.and_then(|c| {
                    c.properties
                        .iter()
                        .find(|p| p.name == name)
                        .map(|p| &p.value)
                })
            };
            if style == 1 && bake("GenerateRuntimeLight").and_then(|v| v.as_bool()) == Some(false) {
                baked_only += 1;
                continue;
            }
            let yes = serde_json::Value::Bool(true);
            let no = serde_json::Value::Bool(false);
            let zero = serde_json::json!(0.);
            let (light, _) = spherical_lighting::authored(
                prop,
                |name| match name {
                    "GenerateRuntimeLight" => Some(&yes),
                    "LightIsSubtractive" | "RuntimeLightRayCast" => bake(name).or(Some(&no)),
                    "Min Falloff Range" | "Max Falloff Range" => bake(name).or(Some(&zero)),
                    _ => bake(name),
                },
                e.transform[12..15].try_into().unwrap(),
                std::array::from_fn(|r| std::array::from_fn(|c| e.runtime_rotation[r * 3 + c])),
            )
            .with_context(|| format!("Enhanced light {}:{}", e.scene, e.name))?
            .context("Enhanced light admission failed")?;
            let mut gpu = GpuLight::default();
            gpu.position = light.position;
            gpu.direction = light.direction;
            gpu.light_type = match light.kind {
                LightKind::Ambient => 0,
                LightKind::Directional => 1,
                LightKind::Point => 2,
                LightKind::Spot => 3,
            };
            gpu.dimmer = light.dimmer;
            gpu.ambient = light.ambient.map(|v| v / 255.);
            gpu.diffuse = light.diffuse.map(|v| v / 255.);
            gpu.attenuation = light.attenuation;
            gpu.range = light.range_end;
            gpu.range_start = light.range_start;
            gpu.spot_inner = light.spot_inner_cos;
            gpu.spot_outer = light.spot_outer_cos;
            gpu.spot_exponent = light.spot_exponent;
            gpu.light_group = prop("Light Group")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .into();
            lights.push(gpu);
        }
        self.enhanced_light_registry.admitted = lights.len();
        self.enhanced_light_registry.disabled = disabled;
        self.enhanced_light_registry.baked_only = baked_only;
        Ok(lights)
    }
}
