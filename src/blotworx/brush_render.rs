//! Rigid brush attachment and authored hose resources. Particle playback lives
//! in brush_particles; gameplay firing and visibility are still incomplete.
use crate::render_support::{
    material_preview, scene,
    viewport3d::{DecodedImage, GpuDrawCommand},
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::NifFile;
use epic_mickey_lib_rs::{hkx_file::HkxBone, packfile::VirtualFile};
use std::{collections::HashMap, path::Path, sync::Arc};

/// Cooked materials sometimes keep a root-relative texture beginning with a
/// slash. That slash is part of the game's virtual asset name, not a host
/// filesystem root. Resolve it inside the selected extracted game directory.
fn loose_texture(root: &Path, path: &str) -> Result<Option<(Vec<u8>, u32, u32)>> {
    let relative =
        super::resource_path::canonical(path.replace('\\', "/").trim_start_matches('/'))?;
    let root = root.canonicalize()?;
    let mut current = root.clone();
    for part in relative.split('/') {
        let requested = current.join(part);
        let candidate = if requested.try_exists()? {
            requested
        } else {
            let mut found = None;
            for entry in std::fs::read_dir(&current)? {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(part)
                {
                    ensure!(found.is_none(), "Ambiguous loose texture casing: {path}");
                    found = Some(entry.path());
                }
            }
            let Some(found) = found else { return Ok(None) };
            found
        };
        let resolved = candidate.canonicalize()?;
        ensure!(
            resolved.starts_with(&root),
            "Loose texture escapes game root: {path}"
        );
        current = resolved;
    }
    ensure!(
        current.is_file(),
        "Loose texture is not a regular file: {path}"
    );
    Ok(crate::render_support::nif::decode_image_bytes(&std::fs::read(
        &current,
    )?))
}

/// Compose a named object's authored ancestor chain, retaining rotations and
/// scales. The action requests `paintbrush` (REL 0xa62a0); the retail NIF spells
/// its node `PaintBrush`. Ambiguous names/parents must not silently pick a node.
pub(super) fn named_transform(nif: &NifFile, name: &str) -> Result<Option<[f32; 16]>> {
    let mut matches = nif
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            b.as_av_object()
                .is_some_and(|a| a.base.name.eq_ignore_ascii_case(name))
        })
        .map(|(i, _)| i);
    let Some(index) = matches.next() else {
        return Ok(None);
    };
    ensure!(
        matches.next().is_none(),
        "Ambiguous brush source node {name}"
    );
    Ok(Some(node_transform(nif, index)?))
}
/// Indexed particle systems need their own transform even when names repeat.
pub(super) fn node_transform(nif: &NifFile, mut index: usize) -> Result<[f32; 16]> {
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        ensure!(seen.insert(index), "Cycle in brush source hierarchy");
        chain.push(index);
        let mut parents = nif
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.children().contains(&Some(index as u32)))
            .map(|(i, _)| i);
        let Some(parent) = parents.next() else { break };
        ensure!(
            parents.next().is_none(),
            "Multiple parents for brush source node"
        );
        index = parent;
    }
    let mut result = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    for index in chain.into_iter().rev() {
        let t = &nif.blocks[index]
            .as_av_object()
            .context("Non-transform brush ancestor")?
            .transform;
        let mut local = [0.; 16];
        local[15] = 1.;
        for row in 0..3 {
            for col in 0..3 {
                local[col * 4 + row] = t.rotation.rows[row][col] * t.scale;
            }
        }
        local[12..15].copy_from_slice(&[t.translation.x, t.translation.y, t.translation.z]);
        ensure!(
            local.iter().all(|v| v.is_finite()),
            "Nonfinite brush source transform"
        );
        result = super::skin::matrix_product(&result, &local);
    }
    Ok(result)
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use epic_mickey_lib_rs::nif_file::{NiNodeData, NiObject};
    fn identity() -> [f32; 16] {
        [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ]
    }
    fn fixture() -> NifFile {
        let mut root = NiNodeData::default();
        root.base.base.name = "root".into();
        root.children = vec![Some(1)];
        root.base.transform.scale = 2.;
        root.base.transform.rotation.rows = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
        root.base.transform.translation.x = 10.;
        root.base.transform.translation.y = 20.;
        root.base.transform.translation.z = 30.;
        let mut source = NiNodeData::default();
        source.base.base.name = "PaintBrush".into();
        source.base.transform.scale = 0.5;
        source.base.transform.rotation.rows = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        source.base.transform.translation.x = 1.;
        source.base.transform.translation.y = 2.;
        source.base.transform.translation.z = 3.;
        NifFile {
            blocks: vec![NiObject::NiNode(root), NiObject::NiNode(source)],
            ..Default::default()
        }
    }
    #[test]
    fn source_composes_authored_parents_and_animated_attachment() -> Result<()> {
        let nif = fixture();
        let local = named_transform(&nif, "paintbrush")?.unwrap();
        assert_eq!(&local[12..15], &[16., 24., 28.]);
        let mut bone = identity();
        bone[12] = 3.;
        let mut actor = identity();
        actor[13] = 5.;
        let mut brush = Brush {
            casts_shadow: false,
            shadow_primary: None,
            shadow_draws: vec![],
            lighting: None,
            local_scale: 1.,
            bone: 0,
            local: identity(),
            emitter_local: Some(local),
            draws: vec![],
            warnings: vec![],
            hoses: vec![],
        };
        assert_eq!(
            brush.emitter_position(&[bone], actor, [100.; 3])?,
            [19., 29., 28.]
        );
        brush.emitter_local = None;
        assert_eq!(brush.emitter_position(&[], actor, [100.; 3])?, [100.; 3]);
        Ok(())
    }
    #[test]
    fn missing_source_and_invalid_hierarchies_are_distinct() -> Result<()> {
        let mut nif = fixture();
        assert!(named_transform(&nif, "missing")?.is_none());
        if let NiObject::NiNode(node) = &mut nif.blocks[1] {
            node.children = vec![Some(0)];
        }
        assert!(named_transform(&nif, "paintbrush").is_err());
        let mut nif = fixture();
        nif.blocks.push(nif.blocks[1].clone());
        assert!(named_transform(&nif, "paintbrush").is_err());
        let mut nif = fixture();
        nif.blocks.push(nif.blocks[0].clone());
        assert!(named_transform(&nif, "paintbrush").is_err());
        Ok(())
    }
}

pub struct Brush {
    shadow_draws: Vec<usize>,
    shadow_primary: Option<u32>,
    casts_shadow: bool,
    lighting: Option<Lighting>,
    local_scale: f32,
    pub hoses: Vec<super::brush_particles::Hose>,
    pub bone: usize,
    pub local: [f32; 16],
    pub emitter_local: Option<[f32; 16]>,
    pub draws: Vec<GpuDrawCommand>,
    pub warnings: Vec<String>,
}
struct Lighting {
    hierarchy: super::scene_bounds::Hierarchy,
    previous: std::collections::BTreeMap<u32, super::scene_bounds::Sphere>,
    properties: super::spherical_lighting::PropertyStore,
    property: u64,
    root: u32,
    materials: Vec<(
        usize,
        u8,
        std::result::Result<
            (
                Arc<material_preview::Program>,
                Vec<material_preview::MaterialImage>,
            ),
            String,
        >,
    )>,
}
impl Brush {
    fn attachment(&self, pose: &[[f32; 16]], actor: [f32; 16]) -> Result<[f32; 16]> {
        let bone = pose
            .get(self.bone)
            .context("Animated pose lacks brush attachment bone")?;
        let mut socket = super::skin::matrix_product(&actor, bone);
        // Bone placement is in the scaled character's world space. The brush
        // asset already carries its model scale; inheriting the actor's scale
        // again makes the brush ten times too small with stock Mickey.
        for col in 0..3 {
            let length = (0..3)
                .map(|row| socket[col * 4 + row].powi(2))
                .sum::<f32>()
                .sqrt();
            ensure!(
                length.is_finite() && length > 0.,
                "Invalid brush socket basis"
            );
            for row in 0..3 {
                socket[col * 4 + row] /= length;
            }
        }
        Ok(super::skin::matrix_product(&socket, &self.local))
    }
    pub fn load(
        assets: &super::brush_assets::Assets,
        files: &[VirtualFile],
        root_path: &Path,
        bones: &[HkxBone],
        ctx: &egui::Context,
    ) -> Result<Self> {
        let root = &assets.definitions[&assets.root];
        let component = root
            .components
            .iter()
            .find(|c| c.class_name == "JPSTransformationComponent")
            .context("Brush has no attachment transform")?;
        let property = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("Missing brush transform property {name}"))
        };
        let name = property("Bone Attach Name")?
            .as_str()
            .context("Invalid brush attachment bone")?;
        // Retail palette spells SKEL_Weapon; the shipped rig spells
        // SKEL_weapon. Retain the authored name but resolve case-insensitively.
        let mut matching = bones
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name.eq_ignore_ascii_case(name));
        let bone = matching
            .next()
            .with_context(|| format!("Mickey skeleton lacks brush bone {name}"))?
            .0;
        ensure!(matching.next().is_none(), "Ambiguous brush bone {name}");
        ensure!(
            property("Bone Attach Rotate Mode")?.as_i64() == Some(0),
            "Brush attachment rotation mode is not implemented"
        );
        let vector = |name: &str| -> Result<[f32; 3]> {
            let value = property(name)?;
            let mut result = [0.; 3];
            for (i, key) in ["x", "y", "z"].iter().enumerate() {
                result[i] = value[*key]
                    .as_f64()
                    .with_context(|| format!("Invalid brush {name}.{key}"))?
                    as f32;
                ensure!(result[i].is_finite(), "Non-finite brush {name}.{key}");
            }
            Ok(result)
        };
        // Stock brush has a zero offset. Nonzero offset's coordinate space
        // needs executable validation before accepting modified content.
        ensure!(
            vector("Bone Attach Offset")? == [0.; 3],
            "Nonzero brush bone offset is not implemented"
        );
        let translation = vector("Local Translation")?;
        let scale = property("Local Scale")?
            .as_f64()
            .context("Invalid brush local scale")? as f32;
        ensure!(scale.is_finite(), "Non-finite brush scale");
        let rotation = &property("Local Rotation")?["m"];
        let mut local = [0.; 16];
        local[15] = 1.;
        for row in 0..3 {
            for col in 0..3 {
                let value = rotation[row][col]
                    .as_f64()
                    .context("Invalid brush local rotation")? as f32;
                ensure!(value.is_finite(), "Non-finite brush rotation");
                local[col * 4 + row] = value * scale;
            }
        }
        local[12..15].copy_from_slice(&translation);
        let path = root.model.as_deref().context("Brush has no model")?;
        let source = NifFile::load_bytes(
            assets
                .models
                .get(path)
                .context("Missing brush model bytes")?,
        )
        .map_err(anyhow::Error::msg)?;
        let emitter_local = named_transform(&source, "paintbrush")?;
        let mut models = HashMap::new();
        let cache = scene::ensure_nif_cached(path, files, &mut models);
        let meshes = &models[&cache];
        ensure!(
            !meshes.is_empty(),
            "Brush model has no decoded geometry: {path}"
        );
        let sources = material_preview::native_sources(ctx, files);
        let mut textures = HashMap::new();
        let mut draws = Vec::new();
        let mut warnings = Vec::new();
        let mut lighting_materials = Vec::new();
        let placeholder = Arc::new(vec![0; super::spherical_lighting::SIZE.pow(2) * 4]);
        for mesh in meshes {
            let tex = if let Some(name) = mesh.texture_name.as_ref() {
                if !textures.contains_key(name) {
                    let packed = crate::render_support::nif::find_pak_texture(files, name)
                        .and_then(|file| crate::render_support::nif::decode_image_bytes(&file.data));
                    let decoded = match packed {
                        Some(image) => Some(image),
                        None => loose_texture(root_path, name)?,
                    };
                    textures.insert(
                        name.clone(),
                        decoded.map(|(rgba, width, height)| DecodedImage {
                            rgba: Arc::new(rgba),
                            width,
                            height,
                        }),
                    );
                }
                textures
                    .get(name)
                    .and_then(|image| image.as_ref())
                    .map(|i| (i.rgba.clone(), i.width, i.height))
            } else {
                None
            };
            ensure!(
                mesh.texture_name.is_none() || tex.is_some(),
                "Missing brush texture {:?}",
                mesh.texture_name
            );
            let mut images = Vec::new();
            let prepared = material_preview::prepare(&mesh.material, &sources, false);
            if let Ok(program) = &prepared {
                if let Some(slot) = program.first_runtime_sh_slot(&mesh.material) {
                    let slot = u8::try_from(slot).context("Invalid brush SH texture slot")?;
                    let runtime = material_preview::MaterialImage {
                        slot,
                        rgba: placeholder.clone(),
                        width: 16,
                        height: 16,
                        sampler: Default::default(),
                        matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                    };
                    let images = material_preview::images_with_runtime_sh(
                        program,
                        &mesh.material,
                        files,
                        &mut textures,
                        Some(&runtime),
                    );
                    lighting_materials.push((
                        draws.len(),
                        slot,
                        images.map(|images| (program.clone(), images)),
                    ));
                }
            }
            let program = prepared.and_then(|program| {
                images = material_preview::images(&program, &mesh.material, files, &mut textures)?;
                Ok(program)
            });
            // Same explicit material diagnostic as the rest of the native
            // viewport. Unsupported retail lighting must remain visible.
            if let Err(error) = &program {
                warnings.push(format!("Brush material: {error}"));
            }
            draws.push(GpuDrawCommand {
                shader_time:None,source_attributes: mesh.source_attributes.clone(),
                extra_uvs: mesh.extra_uvs.clone(),
                projection_bound: None,
                projected_shadow: false,
                skybox: false,
                material_registers: None,
                material_indirect: None,
                material_images: Arc::new(images),
                baked: false,
                baked_error: None,
                preview: Some(program),
                uv_transform: mesh.uv_transform,
                material: Some(mesh.material.clone()),
                sampler: mesh.sampler,
                v_data: mesh.v_data.clone(),
                indices: mesh.indices.clone(),
                trans_mat: local,
                color: [1.; 4],
                tex_bytes: tex,
                aabb_min: mesh.aabb_min,
                aabb_max: mesh.aabb_max,
                light_group: Arc::from(""),
            });
        }
        let mut hoses = Vec::new();
        for name in &assets.streams {
            let model = assets.definitions[name]
                .model
                .as_ref()
                .context("Missing hose model")?;
            let hose = super::brush_particles::Hose::load(&assets.models[model], files, ctx)
                .with_context(|| format!("Hose {name}"))?;
            warnings.extend(hose.warnings.iter().cloned());
            hoses.push(hose);
        }
        let lighting = if lighting_materials.is_empty() {
            None
        } else {
            let hierarchy = super::scene_bounds::Hierarchy::load(&source)?;
            let roots: Vec<_> = hierarchy.roots.iter().flatten().copied().collect();
            ensure!(roots.len() == 1, "Brush lighting requires one model root");
            let root = roots[0];
            // Original runtime capture gives MagicBrush its own property and
            // model-root bound, distinct from Mickey's lighting property.
            let mut properties = super::spherical_lighting::PropertyStore::default();
            let property = properties.allocate(super::spherical_lighting::BoundOwner {
                entity: 0,
                node: root,
            })?;
            for (index, _, _) in &lighting_materials {
                properties.attach((0, meshes[*index].mesh_index as u32), property)?;
            }
            Some(Lighting {
                hierarchy,
                previous: Default::default(),
                properties,
                property,
                root,
                materials: lighting_materials,
            })
        };
        let shadow_primary = lighting
            .as_ref()
            .map(|l| l.hierarchy.primary_actor_mesh(l.root))
            .transpose()?
            .flatten()
            .map(|(_, mesh)| mesh);
        let shadow_draws = meshes
            .iter()
            .enumerate()
            .filter_map(|(i, m)| (Some(m.mesh_index as u32) == shadow_primary).then_some(i))
            .collect();
        let casts_shadow = root
            .components
            .iter()
            .flat_map(|c| &c.properties)
            .find(|p| {
                p.name
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    == "SpecialRendering"
            })
            .and_then(|p| p.value.as_i64())
            == Some(2);
        Ok(Self {
            casts_shadow,
            shadow_draws,
            shadow_primary,
            lighting,
            local_scale: scale,
            bone,
            local,
            emitter_local,
            draws,
            warnings,
            hoses,
        })
    }
    pub(super) fn shadow_parts(
        &self,
        draws: &[GpuDrawCommand],
    ) -> Result<
        Option<(
            super::scene_bounds::Sphere,
            Vec<crate::render_support::model_shadow::Caster>,
        )>,
    > {
        if !self.casts_shadow {
            return Ok(None);
        }
        let lighting = self
            .lighting
            .as_ref()
            .context("Attached brush model bounds unavailable")?;
        let node = self
            .shadow_primary
            .context("Attached brush primary mesh unavailable")?;
        let bound = *lighting
            .previous
            .get(&node)
            .context("Attached brush bound not updated")?;
        ensure!(
            draws.len() == self.draws.len(),
            "Attached brush render range mismatch"
        );
        let casters = self
            .shadow_draws
            .iter()
            .map(|&i| {
                let d = &draws[i];
                crate::render_support::model_shadow::Caster {
                    vertices: d.v_data.clone(),
                    indices: d.indices.clone(),
                    model: d.trans_mat,
                }
            })
            .collect();
        Ok(Some((bound, casters)))
    }
    /// Apply after rigid attachment so the brush samples at its own animated
    /// model bound, not at Mickey's position or the brush emission point.
    pub fn apply_lighting(
        &mut self,
        draws: &mut [GpuDrawCommand],
        view: &[f32; 16],
        lights: &[super::spherical_lighting::RegisteredLight],
    ) -> Result<()> {
        ensure!(
            draws.len() == self.draws.len(),
            "Attached brush draw range does not match its model"
        );
        let Some(lighting) = &mut self.lighting else {
            return Ok(());
        };
        let pixels = (|| -> Result<Arc<Vec<u8>>> {
            let matrix = draws
                .first()
                .context("Missing attached brush draws")?
                .trans_mat;
            ensure!(
                self.local_scale.is_finite() && self.local_scale > 0.,
                "Invalid brush lighting scale"
            );
            let transform = epic_mickey_lib_rs::nif_file::NiTransform {
                rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                    rows: std::array::from_fn(|r| {
                        std::array::from_fn(|c| matrix[c * 4 + r] / self.local_scale)
                    }),
                },
                translation: epic_mickey_lib_rs::nif_file::Vector3 {
                    x: matrix[12],
                    y: matrix[13],
                    z: matrix[14],
                },
                scale: self.local_scale,
            };
            let bounds = lighting
                .hierarchy
                .static_world_bounds(Some(&transform), &lighting.previous)?;
            let bound = *bounds
                .get(&lighting.root)
                .context("Missing brush root bound")?;
            ensure!(
                !lights.iter().any(|light| light.raycast),
                "Brush SH raycast visibility requires runtime integration"
            );
            let pixels = lighting.properties.update_image(
                lighting.property,
                bound,
                lights.iter().map(|l| (&l.light, 1.)),
                view,
            )?;
            lighting.previous = bounds;
            Ok(pixels)
        })()
        .map_err(|error| format!("Brush lighting: {error:#}"));
        for (index, slot, material) in &lighting.materials {
            let draw = &mut draws[*index];
            let prepared = pixels.as_ref().map_err(Clone::clone).and_then(|pixels| {
                let (program, templates) = material.as_ref().map_err(Clone::clone)?;
                let mut images = templates.clone();
                if let Some(image) = images.iter_mut().find(|image| image.slot == *slot) {
                    image.rgba = pixels.clone();
                }
                Ok((program.clone(), images))
            });
            match prepared {
                Ok((program, images)) => {
                    draw.preview = Some(Ok(program));
                    draw.material_images = Arc::new(images);
                }
                Err(error) => draw.preview = Some(Err(error)),
            }
        }
        Ok(())
    }
    /// REL 0xa82cc: stream/projectile source is the named object on MagicBrush;
    /// absent object falls back to the owning entity position. The caller
    /// supplies the animated attachment rather than an authored bind pose.
    pub fn emitter_transform(&self, pose: &[[f32; 16]], actor: [f32; 16]) -> Result<[f32; 16]> {
        let local = self
            .emitter_local
            .context("Missing brush effect attachment node")?;
        Ok(super::skin::matrix_product(
            &self.attachment(pose, actor)?,
            &local,
        ))
    }
    pub fn emitter_position(
        &self,
        pose: &[[f32; 16]],
        actor: [f32; 16],
        owner_position: [f32; 3],
    ) -> Result<[f32; 3]> {
        let Some(local) = self.emitter_local else {
            return Ok(owner_position);
        };
        let attachment = self.attachment(pose, actor)?;
        let matrix = super::skin::matrix_product(&attachment, &local);
        let point = [matrix[12], matrix[13], matrix[14]];
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "Nonfinite animated brush source"
        );
        Ok(point)
    }
    pub fn append(
        &self,
        pose: &[[f32; 16]],
        actor: [f32; 16],
        draws: &mut Vec<GpuDrawCommand>,
    ) -> Result<()> {
        let transform = self.attachment(pose, actor)?;
        ensure!(
            transform.iter().all(|v| v.is_finite()),
            "Non-finite animated brush transform"
        );
        draws.extend(self.draws.iter().cloned().map(|mut draw| {
            draw.trans_mat = transform;
            draw
        }));
        Ok(())
    }
}
