//! One native shape resource per canonical HKX path for the lifetime of a World.
//! Placed phantoms retain separate transforms, listeners and system ownership.
use super::{
    collision::{self, ShapeReference, StaticMesh},
    phantom_objects::Objects,
    shape_bounds::ShapeGraph,
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Resource {
    pub shapes: Arc<Vec<StaticMesh>>,
    pub reference: ShapeReference,
    pub shape_identity: usize,
    pub bounds: Arc<Mutex<ShapeGraph>>,
}
#[derive(Default)]
pub struct Cache {
    entries: HashMap<String, Resource>,
}
impl Cache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Path is the resolved archive filename, after platform aliases have been
    /// resolved. Distinct exact .hkx and .hkx_wii files must not be conflated.
    /// Data imported into a World is immutable. Explicit asset reload must
    /// replace that World; a subsequent component batch is not an asset reload.
    pub fn acquire(&mut self, path: &str, bytes: &[u8], objects: &mut Objects) -> Result<Resource> {
        let key = super::resource_path::canonical(path)?.to_ascii_lowercase();
        if let Some(resource) = self.entries.get(&key) {
            ensure!(
                objects.shapes.contains_key(&resource.shape_identity),
                "Cached phantom shape reference was destroyed"
            );
            return Ok(resource.clone());
        }
        let file = HkxFile::load_bytes(bytes).map_err(anyhow::Error::msg)?;
        let imported = collision::phantom_resource(&file)?;
        let shapes = Arc::new(imported.meshes);
        let first = shapes
            .first()
            .context("Phantom resource has no decoded body shapes")?;
        ensure!(
            shapes
                .iter()
                .all(|part| part.body_offset == first.body_offset
                    && part.bounds_root == first.bounds_root
                    && Arc::ptr_eq(&part.bounds_graph, &first.bounds_graph)),
            "Selected phantom body has inconsistent native shape roots"
        );
        let bounds = Arc::new(Mutex::new((*first.bounds_graph).clone()));
        let shape_identity = objects.allocate_shape_reference(imported.reference.header)?;
        let resource = Resource {
            shapes,
            reference: imported.reference,
            shape_identity,
            bounds,
        };
        self.entries.insert(key, resource.clone());
        Ok(resource)
    }
}
