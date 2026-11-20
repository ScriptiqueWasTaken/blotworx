//! Source catalog from loaded exact AIPathDatabaseComponent instances. This
//! supplies definition data, not native manager/service admission or execution.
use super::{
    ai_biped_catalog::{Catalog, Database},
    assets::World,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
fn integer(component: &Component, name: &str) -> Result<i32> {
    let mut properties = component.properties.iter().filter(|p| p.name == name);
    let property = properties.next();
    ensure!(
        properties.next().is_none(),
        "Ambiguous navigation database {name}"
    );
    property.map_or(Ok(0), |p| {
        i32::try_from(
            p.value
                .as_i64()
                .with_context(|| format!("Invalid navigation database {name}"))?,
        )
        .with_context(|| format!("Navigation database {name} exceeds native word domain"))
    })
}
impl World {
    pub(super) fn biped_navigation_catalog(&self) -> Result<Catalog> {
        let mut databases = Vec::new();
        for entity in &self.entities {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "AIPathDatabaseComponent")
            {
                //187fd0/187f48: zero movement is excluded; other values are
                //not clamped to a guessed enumeration. Preserve input order.
                if integer(component, "PathData Movement Type")? == 0 {
                    continue;
                }
                let mut properties = component
                    .properties
                    .iter()
                    .filter(|p| p.name == "PathData HPD Filename");
                let property = properties.next();
                ensure!(
                    properties.next().is_none(),
                    "Ambiguous navigation database HPD filename"
                );
                databases.push(Database {
                    radius: integer(component, "PathData Radius")?,
                    height: integer(component, "PathData Height")?,
                    path: property
                        .map(|p| {
                            p.value
                                .as_str()
                                .map(|s| s.as_bytes().to_vec())
                                .context("Invalid navigation database HPD filename")
                        })
                        .transpose()?,
                });
            }
        }
        Catalog::new(&databases)
    }
}
