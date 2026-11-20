//! Prepared original sketch templates. Runtime placement must instantiate
//! these component graphs, not substitute a mesh or a generic rigid body.
use crate::render_support::{
    material_preview, scene,
    viewport3d::{DecodedImage, GpuDrawCommand},
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{packfile::VirtualFile, scene_file::SceneFile};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub block: u32,
    pub source_indices: Option<Vec<u32>>,
    pub mesh_to_model: [[f32; 4]; 4],
    pub runtime_sh_slot: Option<u8>,
    draw: GpuDrawCommand,
}
impl Mesh {
    pub(super) fn prepared_draw(&self) -> GpuDrawCommand {
        self.draw.clone()
    }
    pub fn instantiate_draw(
        &self,
        transform: [f32; 16],
        lighting: Option<material_preview::MaterialImage>,
    ) -> Result<GpuDrawCommand> {
        ensure!(
            transform.iter().all(|v| v.is_finite()),
            "Invalid sketch instance transform"
        );
        let mut draw = self.draw.clone();
        draw.trans_mat = transform;
        match (self.runtime_sh_slot, lighting) {
            (Some(slot), Some(mut image)) => {
                ensure!(
                    image.slot == slot
                        && image.width > 0
                        && image.height > 0
                        && u64::from(image.width) * u64::from(image.height) * 4
                            == image.rgba.len() as u64,
                    "Invalid sketch instance lighting image"
                );
                let bindings = draw.material.as_ref().context("Sketch material missing")?;
                if let Some(binding) = bindings
                    .textures
                    .iter()
                    .rev()
                    .find(|t| t.slot == u32::from(slot))
                {
                    image.sampler = binding.sampler;
                    image.matrix = binding
                        .transform
                        .and_then(|t| t.matrix())
                        .unwrap_or([1., 0., 0., 0., 1., 0., 0., 0., 1.]);
                } else if let Some(Some(sampler)) = draw
                    .preview
                    .as_ref()
                    .and_then(|p| p.as_ref().ok())
                    .and_then(|p| p.template_samplers.get(slot as usize))
                {
                    image.sampler = *sampler;
                }
                Arc::make_mut(&mut draw.material_images).push(image);
            }
            (None, None) => {}
            _ => anyhow::bail!("Sketch instance must supply exactly its required lighting map"),
        }
        Ok(draw)
    }
}
pub struct Model {
    pub meshes: Vec<Mesh>,
    pub hierarchy: super::scene_bounds::Hierarchy,
    pub skins: Vec<super::skin::Skin>,
}
impl Model {
    pub fn apply_pose(
        &self,
        pose: &[epic_mickey_lib_rs::hkx_file::HkxBone],
        indices: &std::collections::HashMap<u32, Vec<usize>>,
        draws: &mut [GpuDrawCommand],
    ) -> Result<()> {
        let world = super::skin::model_pose(pose, &[])?;
        for mesh in &self.meshes {
            let skin = self
                .skins
                .iter()
                .find(|skin| skin.block == mesh.block as usize)
                .context("Sketch mesh has no decoded skin")?;
            let palette = super::skin::palette_from_model_pose(skin, &world)?;
            let sources = mesh
                .source_indices
                .as_ref()
                .context("Sketch mesh lacks source vertex mapping")?;
            let (vertices,min,max)=super::skin::render_skinned_pose(skin,&palette,&mesh.draw.v_data,&mesh.positions,&mesh.normals,sources,mesh.mesh_to_model)?;
            let vertices = Arc::new(vertices);
            for &index in indices
                .get(&mesh.block)
                .context("Sketch runtime draw mapping missing")?
            {
                let draw = draws
                    .get_mut(index)
                    .context("Sketch runtime draw missing")?;
                draw.v_data = vertices.clone();
                draw.aabb_min = min;
                draw.aabb_max = max;
            }
        }
        Ok(())
    }
}

pub struct Template {
    pub animation: Result<Option<Animation>, String>,
    pub graph: super::brush_assets::Assets,
    /// Canonical resource identities; bytes are shared by the library.
    pub resources: Vec<String>,
}
pub struct Animation {
    pub contract: super::behavior::CharacterAssets,
    pub graph: super::behavior::Document,
    pub resources: super::animation_resources::Resources,
}
pub struct Actor {
    pub prepared: super::generator_plan::Prepared,
    pub playback: super::havok_playback::Playback,
}
impl Animation {
    pub fn instantiate(&mut self) -> Result<Actor> {
        let mut prepared = super::generator_plan::prepare(
            &self.graph,
            super::behavior_state::State::new(&self.graph)?,
            0,
        )?;
        let playback = super::havok_playback::Playback::initialize(
            &self.graph,
            &mut prepared,
            &mut self.resources,
        )?;
        Ok(Actor { prepared, playback })
    }
    pub fn advance(&mut self, actor: &mut Actor, ticks: u64) -> Result<()> {
        actor.playback.advance_character(
            &self.graph,
            &mut actor.prepared,
            &mut self.resources,
            ticks,
        )
    }
    pub fn send_event(&mut self, actor: &mut Actor, name: &str) -> Result<()> {
        actor.playback.queue_named_event(&self.graph, name)
    }
}
#[derive(Default)]
pub struct Library {
    pub models: BTreeMap<String, Model>,
    pub templates: BTreeMap<String, Template>,
    pub resources: BTreeMap<String, std::sync::Arc<[u8]>>,
}
impl Library {
    /// Release the import-only dependency cache after every configured
    /// template has prepared its animation/resources. Runtime instantiation
    /// uses each template's parsed animation state and decoded model data.
    pub fn release_source_bytes(&mut self) {
        self.resources.clear();
    }

    /// Prepare immutable root-model resources while packfile data is present.
    /// Instances keep their own pose, draws and lighting state. Child particle
    /// graphs remain separate from these ordinary mesh resources.
    pub fn prepare_rendering(
        &mut self,
        files: &[VirtualFile],
        sources: &material_preview::Sources,
        textures: &mut std::collections::HashMap<String, Option<DecodedImage>>,
    ) -> Result<()> {
        let paths = self
            .templates
            .values()
            .filter_map(|template| {
                template.graph.definitions[&template.graph.root]
                    .model
                    .clone()
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut cache = std::collections::HashMap::new();
        for path in paths {
            let identity = path.replace('\\', "/").to_ascii_lowercase();
            if self.models.contains_key(&identity) {
                continue;
            }
            let file = crate::wii_mesh::resolve_model(files, &path)
                .with_context(|| format!("Missing sketch model {path}"))?;
            let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
                .map_err(anyhow::Error::msg)?;
            let hierarchy = super::scene_bounds::Hierarchy::load(&nif)?;
            let skins = super::skin::read_nif(&file.data)?;
            let key = scene::ensure_nif_cached(&path, files, &mut cache);
            let mut meshes = Vec::new();
            for mesh in &cache[&key] {
                ensure!(
                    !nif.block_type_indices
                        .get(mesh.mesh_index)
                        .and_then(|&kind| nif.block_types.get(kind as usize))
                        .is_some_and(|kind| kind.contains("ParticleSystem")),
                    "Sketch root {path} requires particle instantiation"
                );
                let tex = mesh.texture_name.as_ref().and_then(|name| {
                    textures
                        .entry(name.clone())
                        .or_insert_with(|| {
                            crate::render_support::nif::find_pak_texture(files, name)
                                .and_then(|file| {
                                    crate::render_support::nif::decode_image_bytes(&file.data)
                                })
                                .map(|(rgba, width, height)| DecodedImage {
                                    rgba: Arc::new(rgba),
                                    width,
                                    height,
                                })
                        })
                        .as_ref()
                        .map(|image| (image.rgba.clone(), image.width, image.height))
                });
                let program = material_preview::prepare(&mesh.material, sources, false)
                    .map_err(anyhow::Error::msg)
                    .with_context(|| format!("Sketch material {path}:{}", mesh.mesh_index))?;
                let runtime_sh_slot = program
                    .first_runtime_sh_slot(&mesh.material)
                    .map(u8::try_from)
                    .transpose()?;
                let mut immutable_program = (*program).clone();
                immutable_program
                    .texture_slots
                    .retain(|slot| Some(*slot) != runtime_sh_slot);
                let images =
                    material_preview::images(&immutable_program, &mesh.material, files, textures)
                        .map_err(anyhow::Error::msg)?;
                let raw = match nif.blocks.get(mesh.mesh_index) {
                    Some(epic_mickey_lib_rs::nif_file::NiObject::NiMesh(raw)) => raw,
                    _ => anyhow::bail!(
                        "Sketch mesh is not a supported Wii NiMesh: {path}:{}",
                        mesh.mesh_index
                    ),
                };
                let decoded = crate::wii_mesh::extract(&nif, raw)?;
                ensure!(
                    mesh.source_indices.as_ref() == Some(&decoded.source_indices),
                    "Sketch source/render vertex maps differ"
                );
                meshes.push(Mesh {
                    positions: decoded.mesh.positions,
                    normals: decoded.mesh.normals.context("Sketch mesh lacks normals")?,
                    block: mesh.mesh_index as u32,
                    source_indices: mesh.source_indices.clone(),
                    mesh_to_model: mesh.mesh_to_model,
                    runtime_sh_slot,
                    draw: GpuDrawCommand {
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
                        preview: Some(Ok(program)),
                        uv_transform: mesh.uv_transform,
                        material: Some(mesh.material.clone()),
                        sampler: mesh.sampler,
                        v_data: mesh.v_data.clone(),
                        indices: mesh.indices.clone(),
                        trans_mat: [
                            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                        ],
                        color: [1.; 4],
                        tex_bytes: tex,
                        aabb_min: mesh.aabb_min,
                        aabb_max: mesh.aabb_max,
                        light_group: std::sync::Arc::from(""),
                    },
                });
            }
            ensure!(
                !meshes.is_empty(),
                "Sketch model has no decoded meshes: {path}"
            );
            self.models.insert(
                identity,
                Model {
                    meshes,
                    hierarchy,
                    skins,
                },
            );
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        name: &str,
        scenes: &[(String, SceneFile)],
        files: &[VirtualFile],
    ) -> Result<()> {
        let key = name.to_ascii_lowercase();
        if self.templates.contains_key(&key) {
            return Ok(());
        }
        let find = |path: &str| -> Result<&VirtualFile> {
            files
                .iter()
                .find(|file| {
                    let candidate = file.path.replace('\\', "/").to_ascii_lowercase();
                    let requested = path.replace('\\', "/").to_ascii_lowercase();
                    candidate == requested || candidate == format!("{requested}_wii")
                })
                .with_context(|| format!("Missing sketch dependency {path}"))
        };
        let graph =
            super::brush_assets::Assets::effect(name, scenes, |path| Ok(find(path)?.data.clone()))?;
        let mut paths = Vec::new();
        let mut pending = BTreeMap::new();
        for definition in graph.definitions.values() {
            for component in &definition.components {
                for property in &component.properties {
                    if !matches!(
                        property.name.as_str(),
                        "RB Hull File Path"
                            | "Behavior File Path"
                            | "Animation List Path"
                            | "KFM File Path"
                            | "Lua File Path"
                    ) {
                        continue;
                    }
                    for path in super::actor_scripts::authored_paths(&property.value).with_context(
                        || {
                            format!(
                                "Sketch {} {}.{}",
                                definition.name, component.class_name, property.name
                            )
                        },
                    )? {
                        let file = find(&path)?;
                        let identity = file.path.replace('\\', "/").to_ascii_lowercase();
                        if !paths.contains(&identity) {
                            paths.push(identity.clone());
                        }
                        if !self.resources.contains_key(&identity)
                            && !pending.contains_key(&identity)
                        {
                            pending.insert(
                                identity,
                                std::sync::Arc::<[u8]>::from(file.data.as_slice()),
                            );
                        }
                    }
                }
            }
        }
        ensure!(
            graph.definitions.contains_key(&graph.root),
            "Sketch root definition missing"
        );
        let animation = graph.definitions[&graph.root]
            .components
            .iter()
            .find(|c| c.class_name == "AnimationComponent")
            .map(|component| -> Result<Animation> {
                let string = |name: &str| {
                    component
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .and_then(|p| p.value.as_str())
                        .with_context(|| format!("Sketch {key} missing animation {name}"))
                };
                let (contract, behavior) = super::behavior::character_assets(
                    string("Behavior File Path")?,
                    string("CharacterName")?,
                    |path| find(path).ok().map(|file| file.data.as_slice()),
                )?;
                let resources = super::animation_resources::Resources::load(&contract, |path| {
                    find(path).ok().map(|file| file.data.clone())
                })?;
                Ok(Animation {
                    contract,
                    graph: behavior,
                    resources,
                })
            })
            .transpose()
            .map_err(|error| format!("{error:#}"));
        // Commit only after every required direct dependency resolves.
        self.resources.extend(pending);
        self.templates.insert(
            key,
            Template {
                graph,
                resources: paths,
                animation,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_sketch_graphs_keep_animation_physics_and_scripts() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let library = &world.sketch_templates;
        for name in ["paintanvil", "painttv", "paintwatch"] {
            let template = library.templates.get(name).with_context(|| {
                format!(
                    "Missing {name}: {:?}",
                    world
                        .report
                        .unsupported
                        .iter()
                        .filter(|e| e.starts_with("Sketch template"))
                        .collect::<Vec<_>>()
                )
            })?;
            let root = &template.graph.definitions[&template.graph.root];
            ensure!(!root.components.is_empty(), "Sketch lost its components");
            if let Some(model) = &root.model {
                ensure!(
                    template.graph.models.is_empty(),
                    "Decoded sketch NIF payload was retained unnecessarily"
                );
                let rendering = library
                    .models
                    .get(&model.replace('\\', "/").to_ascii_lowercase())
                    .with_context(|| {
                        format!(
                            "Sketch rendering missing for {model}: {:?}",
                            world
                                .report
                                .unsupported
                                .iter()
                                .filter(|e| e.starts_with("Sketch rendering"))
                                .collect::<Vec<_>>()
                        )
                    })?;
                ensure!(
                    !rendering.meshes.is_empty() && !rendering.hierarchy.nodes.is_empty(),
                    "Sketch lost mesh/hierarchy data"
                );
                for mesh in &rendering.meshes {
                    ensure!(!mesh.draw.indices.is_empty(), "Empty sketch mesh");
                    ensure!(
                        mesh.draw.preview.as_ref().is_some_and(Result::is_ok),
                        "Sketch material not prepared"
                    );
                    let copy = mesh.draw.clone();
                    assert!(
                        Arc::ptr_eq(&copy.v_data, &mesh.draw.v_data),
                        "Instance duplicated immutable mesh vertices"
                    );
                }
            }
            ensure!(
                template
                    .resources
                    .iter()
                    .all(|path| library.resources.contains_key(path)),
                "Sketch dependency was not retained"
            );
        }
        let anvil = &library.templates["paintanvil"].graph;
        let root = &anvil.definitions[&anvil.root];
        assert_eq!(
            root.model.as_deref(),
            Some("SkinnedProps/anvil/anvilBlue.nif")
        );
        assert!(root
            .components
            .iter()
            .any(|c| c.class_name == "AnimationComponent"));
        assert!(root
            .components
            .iter()
            .any(|c| c.class_name == "PhysicsRigidBodyDynamicComponent"));
        assert!(root
            .components
            .iter()
            .any(|c| c.class_name == "InputOutputComponent"));
        let lifetime = root
            .components
            .iter()
            .find(|c| c.class_name == "LifetimeComponent")
            .unwrap();
        assert_eq!(
            lifetime
                .properties
                .iter()
                .find(|p| p.name == "Lifetime")
                .unwrap()
                .value
                .as_f64(),
            Some(7.)
        );
        assert!(library
            .resources
            .keys()
            .any(|p| p.ends_with("sketches.lua")));
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files; sketch character selection/schema compatibility remains incomplete"]
    fn original_anvil_animation_resources_sample_every_clip() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let animation = world
            .sketch_templates
            .templates
            .get_mut("paintanvil")
            .context("Anvil template missing")?
            .animation
            .as_mut()
            .map_err(|error| anyhow::anyhow!(error.clone()))?
            .as_mut()
            .context("Anvil animation resources missing")?;
        assert!(!animation.resources.rig.bones.is_empty());
        assert!(!animation.contract.clips.is_empty());
        for name in animation.contract.clips.keys() {
            let bones = animation.resources.rig.bones.len();
            let clip = animation.resources.clip(name)?;
            let pose = clip.sample_tracks(bones, clip.info.duration * 0.5)?;
            assert_eq!(pose.len(), bones);
            assert!(
                pose.iter().any(Option::is_some),
                "Anvil clip has no bound tracks: {name}"
            );
        }
        let mut first = animation.instantiate()?;
        let mut second = animation.instantiate()?;
        let root = animation.graph.single("hkbBehaviorGraph")?["rootGenerator"]["ref"]
            .as_str()
            .context("Anvil graph root missing")?
            .to_owned();
        assert_eq!(first.prepared.machines[&root].current, 0);
        assert_eq!(second.prepared.machines[&root].current, 0);
        let model = &world.sketch_templates.models["skinnedprops/anvil/anvilblue.nif"];
        let mut draws = model
            .meshes
            .iter()
            .map(|mesh| mesh.draw.clone())
            .collect::<Vec<_>>();
        let indices = model
            .meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| (mesh.block, vec![i]))
            .collect();
        model.apply_pose(&first.playback.pose, &indices, &mut draws)?;
        let spawn = draws
            .iter()
            .map(|draw| draw.v_data.clone())
            .collect::<Vec<_>>();
        animation.advance(&mut first, 30)?;
        model.apply_pose(&first.playback.pose, &indices, &mut draws)?;
        assert!(
            draws
                .iter()
                .zip(&spawn)
                .any(|(draw, initial)| draw.v_data.as_slice() != initial.as_slice()),
            "Anvil pose never changed rendered vertices"
        );
        assert_eq!(
            first.prepared.machines[&root].current, 1,
            "Spawn did not transition automatically to idle-air"
        );
        assert_eq!(
            second.prepared.machines[&root].current, 0,
            "Actor instances shared state"
        );
        animation.send_event(&mut first, "EVENT_Start_Land")?;
        animation.advance(&mut first, 1)?;
        assert_eq!(
            first.prepared.machines[&root].current, 2,
            "Landing event did not reach anvil behavior"
        );
        animation.advance(&mut first, 45)?;
        assert_eq!(
            first.prepared.machines[&root].current, 3,
            "Landing clip did not transition to grounded idle"
        );
        animation.send_event(&mut first, "EVENT_Start_Death")?;
        animation.advance(&mut first, 1)?;
        assert_eq!(
            first.prepared.machines[&root].current, 4,
            "Death event did not reach exit state"
        );
        Ok(())
    }
}
