//! Resolved scene-property adapter for the retail body initialization stage.
//! World admission and motion-mode activation are separate lifecycle stages.
use super::{body_initialization, native_motion_record::MotionRecord};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::BTreeMap;

pub struct Scene<'a> {
    pub component: &'a mut Component,
    pub settings: &'a BTreeMap<String, String>,
    pub transform: super::native_hk_transform::Transform,
    pub mover_material_seen: bool,
}
impl Scene<'_> {
    fn value(&self, name: &str) -> Result<&serde_json::Value> {
        self.component
            .properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.value)
            .with_context(|| format!("Unresolved rigid-body property {name}"))
    }
}
impl body_initialization::Context for Scene<'_> {
    fn float(&mut self, name: &str) -> Result<f32> {
        let value = self
            .value(name)?
            .as_f64()
            .with_context(|| format!("Invalid body float {name}"))? as f32;
        ensure!(value.is_finite(), "Nonfinite body float {name}");
        Ok(value)
    }
    fn point(&mut self, name: &str) -> Result<[f32; 3]> {
        let v = self.value(name)?;
        let mut result = [0.; 3];
        for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
            result[i] = v
                .get(axis)
                .and_then(|v| v.as_f64())
                .with_context(|| format!("Invalid {name}.{axis}"))? as f32;
        }
        ensure!(
            result.iter().all(|v| v.is_finite()),
            "Nonfinite body point {name}"
        );
        Ok(result)
    }
    fn boolean(&mut self, name: &str) -> Result<bool> {
        self.value(name)?
            .as_bool()
            .with_context(|| format!("Invalid body boolean {name}"))
    }
    fn publish_mass(&mut self, value: f32) -> Result<()> {
        let property = self
            .component
            .properties
            .iter_mut()
            .find(|p| p.name == "Mass")
            .context("Unresolved body Mass")?;
        property.value = serde_json::json!(value);
        Ok(())
    }
    fn config_float(&mut self, name: &str, default: &str) -> Result<f32> {
        let value = self
            .settings
            .get(name)
            .map(String::as_str)
            .unwrap_or(default)
            .parse::<f32>()
            .with_context(|| format!("Invalid body setting {name}"))?;
        ensure!(value.is_finite(), "Nonfinite body setting {name}");
        Ok(value)
    }
    fn material_finished(&mut self, _: &mut MotionRecord) -> Result<()> {
        // REL382a3c..a50 resolves Is Mover through37fc4c; a8 is sticky.
        if self.boolean("Is Mover")? {
            self.mover_material_seen = true;
        }
        Ok(())
    }
    fn entity_transform(&mut self) -> Result<super::native_hk_transform::Transform> {
        Ok(self.transform)
    }
    fn placement_finished(&mut self, _: &mut MotionRecord) -> Result<()> {
        // Base PhysicsRigidBodyDynamicComponent virtual38c is37d3b4: blr.
        ensure!(
            self.component.class_name == "PhysicsRigidBodyDynamicComponent",
            "Unresolved derived body placement hook"
        );
        Ok(())
    }
}
