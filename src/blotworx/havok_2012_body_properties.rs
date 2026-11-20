//! Authored EM2 body-property snapshot, not a complete component lifecycle.
//! Scalar JPSProperty constructors default to zero (reference 3969670);
//! point properties default to NiPoint3::ZERO (3972997). No asset is rewritten.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
#[derive(serde::Serialize)]
pub struct Settings {
    pub mass: f32,
    pub rotation: [f32; 3],
    pub tuning: [f32; 5],
}
const TUNING: [&str; 5] = [
    "Linear Damping",
    "Angular Damping",
    "Gravity Factor",
    "Max Linear Velocity",
    "Max Angular Velocity",
];
impl Settings {
    pub fn read(component: &Component) -> Result<Self> {
        let value = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
        };
        let scalar = |name: &str| -> Result<f32> {
            let Some(value) = value(name) else {
                return Ok(0.);
            };
            let number = value
                .as_f64()
                .with_context(|| format!("Invalid scalar body property {name}"))?
                as f32;
            ensure!(number.is_finite(), "Nonfinite body property {name}");
            Ok(number)
        };
        let mut rotation = [0.; 3];
        if let Some(value) = value("Restrict Rotation") {
            for (index, key) in ["x", "y", "z"].iter().enumerate() {
                rotation[index] = value
                    .get(*key)
                    .and_then(|v| v.as_f64())
                    .context("Invalid Restrict Rotation point")?
                    as f32;
            }
            ensure!(
                rotation.iter().all(|v| v.is_finite()),
                "Nonfinite rotation restriction"
            );
        }
        let mut tuning = [0.; 5];
        for (index, name) in TUNING.iter().enumerate() {
            tuning[index] = scalar(name)?;
        }
        Ok(Self {
            mass: scalar("Mass")?,
            rotation,
            tuning,
        })
    }
    /// Independent value stages only. COM/bounds, velocity/material setup,
    /// placement hooks and world activation still surround these operations.
    /// Re-read live settings when those hooks are connected; this startup
    /// snapshot is not proof that a full native constructor has executed.
    pub fn prepare(
        &self,
        source: &super::havok_2012_motion::Motion,
        live_mass: &mut f32,
    ) -> Result<super::havok_2012_motion::Motion> {
        let mut motion = source.clone();
        if let Some(published) = motion.initialize_mass(*live_mass)? {
            *live_mass = published;
        }
        motion.restrict_rotation(self.rotation)?;
        motion.initialize_tuning(|name| {
            Ok(self.tuning[TUNING
                .iter()
                .position(|key| *key == name)
                .context("Unknown motion tuning property")?])
        })?;
        Ok(motion)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_defaults_and_mass_publication_reach_subsequent_bodies() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{Property, ID};
        let mut component = Component::new(
            "PhysicsRigidBodyDynamicComponent".into(),
            "Body".into(),
            ID::from_u32(0),
            ID::from_u32(1),
            ID::from_u32(0),
            vec![],
        );
        let settings = Settings::read(&component)?;
        assert_eq!(settings.mass, 0.);
        assert_eq!(settings.tuning, [0.; 5]);
        let mut bytes = [0; 0x120];
        bytes[8] = 3;
        for i in [0, 5, 10, 15] {
            bytes[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&1f32.to_le_bytes());
        }
        for (i, value) in [2f32, 4., 8., 0.5].into_iter().enumerate() {
            bytes[0xc0 + i * 4..0xc4 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        let first = super::super::havok_2012_motion::decode(&bytes)?;
        let mut live_mass = settings.mass;
        let prepared = settings.prepare(&first, &mut live_mass)?;
        assert_eq!(live_mass, 2.);
        assert_eq!(prepared.inverse_inertia_mass, first.inverse_inertia_mass);
        let mut second = first.clone();
        second.inverse_inertia_mass[3] = 0.25;
        assert_eq!(
            settings
                .prepare(&second, &mut live_mass)?
                .inverse_inertia_mass,
            [4., 8., 16., 0.5]
        );
        assert_eq!(second.inverse_inertia_mass, [2., 4., 8., 0.25]);
        component.properties.push(Property::new(
            "Float".into(),
            "Mass".into(),
            false,
            false,
            false,
            "bad".into(),
        ));
        assert!(Settings::read(&component).is_err());
        Ok(())
    }
}
