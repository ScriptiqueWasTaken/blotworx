//! Concrete reflected-property services for190a8c/190e44 construction.
//! Registry layout/global scale are supplied by their owning runtime; no
//! construction here publishes an initialized navigation-agent binding.
use super::{
    ai_navigation_entity::{Host, Owned, Registry},
    assets::World,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

impl World {
    pub(super) fn construct_navigation_entity(
        &self,
        identity: usize,
        name: Option<&[u8]>,
        user: [usize; 2],
        registry: &Registry,
        unit_scale: Option<f32>,
    ) -> Result<Owned> {
        Owned::new(
            identity,
            name,
            user,
            registry,
            &mut Binding {
                world: self,
                physics_component: None,
                unit_scale,
            },
        )
    }
}
struct Binding<'a> {
    world: &'a World,
    physics_component: Option<usize>,
    unit_scale: Option<f32>,
}
impl Binding<'_> {
    fn component_index(&self, owner: usize, class: &str) -> Result<usize> {
        let actor = self
            .world
            .entities
            .get(owner)
            .context("Navigation constructor gameplay owner absent")?;
        let mut matches = actor
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| c.class_name == class);
        let (index, _) = matches
            .next()
            .with_context(|| format!("Navigation constructor requires {class}"))?;
        ensure!(
            matches.next().is_none(),
            "Navigation constructor requires native {class} slot arbitration"
        );
        Ok(index)
    }
    fn physics(&self, owner: usize) -> Result<&Component> {
        self.world
            .entities
            .get(owner)
            .and_then(|e| e.components.get(self.physics_component?))
            .context("Navigation constructor lost its retained physics component")
    }
}
fn selector(component: &Component, name: &str) -> Result<i64> {
    let mut matches = component.properties.iter().filter(|p| p.name == name);
    let value = matches.next();
    ensure!(
        matches.next().is_none(),
        "Ambiguous navigation constructor property {name}"
    );
    value.map_or(Ok(1), |p| {
        p.value
            .as_i64()
            .with_context(|| format!("Invalid navigation {name}"))
    })
}
impl Host for Binding<'_> {
    fn radius(&mut self, entity: &mut Owned) -> Result<f32> {
        let owner = entity.entity.user_data[0];
        self.physics_component =
            Some(self.component_index(owner, "PhysicsRigidBodyCharacterComponent")?);
        super::motion_ground::radius(
            selector(self.physics(owner)?, "Entity Radius")?,
            &self.world.runtime_settings,
        )
    }
    fn height(&mut self, entity: &mut Owned) -> Result<f32> {
        super::character_physics::height(
            selector(self.physics(entity.entity.user_data[0])?, "Entity Height")?,
            &self.world.runtime_settings,
        )
    }
    fn maximum_speed(&mut self, entity: &mut Owned) -> Result<f32> {
        let owner = entity.entity.user_data[0];
        let component = self.component_index(owner, "BasicMotionControllerComponent")?;
        let mut matches = self.world.entities[owner].components[component]
            .properties
            .iter()
            .filter(|p| p.name == "Max Move Speed");
        let value = matches.next();
        ensure!(
            matches.next().is_none(),
            "Ambiguous navigation Max Move Speed"
        );
        value.map_or(Ok(0.), |p| {
            p.value
                .as_f64()
                .map(|v| v as f32)
                .context("Invalid navigation Max Move Speed")
        })
    }
    fn unit_scale(&mut self, _: &mut Owned) -> Result<Option<f32>> {
        Ok(self.unit_scale)
    }
}
