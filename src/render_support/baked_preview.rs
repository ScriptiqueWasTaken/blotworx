use std::{collections::HashMap, sync::Arc};
#[derive(Default)]
pub struct BakedPreview {
    streams: HashMap<(u32, Vec<u32>), Arc<Vec<u8>>>,
    vertices: HashMap<(u32, usize), (Arc<Vec<f32>>, Result<Arc<Vec<f32>>, String>)>,
    pub error: Option<String>,
    paths: HashMap<u32, Vec<Vec<u32>>>,
    checked: std::collections::HashSet<(u32, usize)>,
    pub warnings: std::collections::BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_lighting_uses_colour_indices_and_keeps_shared_mesh_unchanged() {
        let mut bytes = b"BKLT0001".to_vec();
        for word in [44u32, 1, 1, 2] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend([255, 0, 32, 128, 0, 64, 255, 255]);
        for word in [1u32, 1, 0, 42, 16] {
            bytes.extend(word.to_be_bytes());
        }
        let mut preview = BakedPreview::default();
        preview.load(&bytes);
        assert!(preview.error.is_none());
        let mesh = super::super::scene::CachedNifMesh {
            source_attributes: None,
            extra_uvs: None,
            mesh_index: 0,
            source_indices: None,
            mesh_to_model: [[0.; 4]; 4],
            hierarchy_path: Some(vec![0]),
            colour_indices: Some(vec![1, 0]),
            colour_count: Some(2),
            uv_transform: None,
            material: Default::default(),
            sampler: Default::default(),
            positions: vec![],
            normals: None,
            uvs: None,
            indices: Arc::new(vec![]),
            texture_name: None,
            aabb_min: [0.; 3],
            aabb_max: [0.; 3],
            v_data: Arc::new(vec![1.; 24]),
        };
        let first = preview.apply(42, &mesh).unwrap().unwrap();
        assert_eq!(&first[8..12], &[0., 64. / 255., 1., 1.]);
        assert_eq!(&first[20..24], &[1., 0., 32. / 255., 128. / 255.]);
        assert!(mesh.v_data.iter().all(|v| *v == 1.));
        assert!(Arc::ptr_eq(
            &first,
            &preview.apply(42, &mesh).unwrap().unwrap()
        ));
        assert!(preview.apply(43, &mesh).is_none());
        preview.load(&bytes);
        assert!(!Arc::ptr_eq(
            &first,
            &preview.apply(42, &mesh).unwrap().unwrap()
        ));
    }
}
impl BakedPreview {
    pub fn load(&mut self, data: &[u8]) {
        *self = Self::default();
        match crate::cooked_lighting::Lighting::load(data) {
            Ok(lighting) => {
                for entity in &lighting.entities {
                    for mesh in &entity.meshes {
                        let colours = Arc::new(lighting.colours(mesh).to_vec());
                        for path in &mesh.groups {
                            self.paths.entry(entity.id).or_default().push(path.clone());
                            self.streams
                                .insert((entity.id, path.clone()), colours.clone());
                        }
                    }
                }
            }
            Err(e) => self.error = Some(format!("Baked lighting: {e:#}")),
        }
    }
    pub fn validate_entity(&mut self, entity: u32, meshes: &[super::scene::CachedNifMesh]) {
        let Some(first) = meshes.first() else {
            return;
        };
        if !self
            .checked
            .insert((entity, Arc::as_ptr(&first.v_data) as usize))
        {
            return;
        }
        if let Some(paths) = self.paths.get(&entity) {
            for path in paths {
                if !meshes
                    .iter()
                    .any(|m| m.hierarchy_path.as_ref() == Some(path))
                {
                    self.warnings.insert(format!(
                        "Entity {entity}: baked mesh path {path:?} is absent from its model"
                    ));
                }
            }
        }
    }
    pub fn apply(
        &mut self,
        entity: u32,
        mesh: &super::scene::CachedNifMesh,
    ) -> Option<Result<Arc<Vec<f32>>, String>> {
        let path = mesh.hierarchy_path.as_ref()?;
        let colours = self.streams.get(&(entity, path.clone()))?;
        let key = (entity, Arc::as_ptr(&mesh.v_data) as usize);
        let entry = self.vertices.entry(key).or_insert_with(|| {
            let result = (|| {
                let indices = mesh
                    .colour_indices
                    .as_ref()
                    .ok_or("Baked colour mapping unavailable for this mesh")?;
                if mesh.colour_count != Some(colours.len() / 4)
                    || indices.len() != mesh.v_data.len() / 12
                    || indices.iter().any(|i| *i as usize >= colours.len() / 4)
                {
                    return Err("Baked lighting no longer matches this mesh".into());
                }
                let mut vertices = mesh.v_data.as_ref().clone();
                for (v, &index) in vertices.chunks_exact_mut(12).zip(indices) {
                    for channel in 0..4 {
                        v[8 + channel] = colours[index as usize * 4 + channel] as f32 / 255.;
                    }
                }
                Ok(Arc::new(vertices))
            })();
            (mesh.v_data.clone(), result)
        });
        Some(entry.1.clone())
    }
}
