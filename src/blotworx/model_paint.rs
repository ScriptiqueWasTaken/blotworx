//! Whole-node toon preview connected to scene rigid-body flags. Native body
//! filtering, stimulus/event dispatch and material stage composition still
//! require fidelity validation, as they do for the existing jigsaw preview.
use super::{assets::World, jigsaw_transition::Target};
use crate::render_support::{
    material_preview as material,
    viewport3d::{DecodedImage, GpuDrawCommand},
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub struct Model {
    pub painted: bool,
    pub invulnerable: bool,
    pub owners: Vec<usize>,
    pub states: BTreeMap<usize, [GpuDrawCommand; 2]>,
    pub transitions: BTreeMap<usize, std::result::Result<[super::jigsaw_effects::Draw; 2], String>>,
    pub alpha: BTreeMap<usize, u16>,
}
pub fn initialize(
    world: &mut World,
    files: &[VirtualFile],
    shaders: &super::sketch_render::Shaders,
    sources: &material::Sources,
    textures: &mut HashMap<String, Option<DecodedImage>>,
    templates: &mut HashMap<String, Arc<super::jigsaw_effects::Template>>,
    static_only: bool,
) -> Result<()> {
    let range = 0..world.entities.len();
    initialize_range(world, range, files, shaders, sources, textures, templates, static_only)
}
pub(super) fn initialize_range(
    world: &mut World,
    range: std::ops::Range<usize>,
    files: &[VirtualFile],
    shaders: &super::sketch_render::Shaders,
    sources: &material::Sources,
    textures: &mut HashMap<String, Option<DecodedImage>>,
    templates: &mut HashMap<String, Arc<super::jigsaw_effects::Template>>,
    static_only: bool,
) -> Result<()> {
    ensure!(
        range.start <= range.end && range.end <= world.entities.len(),
        "Invalid model paint initialization range"
    );
    for entity in range {
        let owner = &world.entities[entity];
        if owner.jigsaw_asset.is_some() || world.model_paint.contains_key(&entity) {
            continue;
        }
        let body_class = if owner
            .components
            .iter()
            .any(|c| c.class_name == "PhysicsRigidBodyDynamicComponent")
        {
            if static_only {
                continue;
            }
            "PhysicsRigidBodyDynamicComponent"
        } else {
            "PhysicsRigidBodyStaticComponent"
        };
        let components = owner
            .components
            .iter()
            .filter(|c| c.class_name == body_class)
            .collect::<Vec<_>>();
        if components.is_empty() {
            continue;
        }
        ensure!(
            components.len() == 1,
            "Whole-model paint requires unambiguous body on {}",
            owner.name
        );
        let component = components[0];
        let boolean = |name: &str| -> Result<bool> {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.value.as_bool())
                .with_context(|| format!("{}: missing resolved body {name}", owner.name))
        };
        // Native whole-body gate 37e624 is !IsReal. Invulnerability belongs
        // to hit admission, not whether an initially thinned model is visible.
        if boolean("Is Real")? {
            continue;
        }
        let painted = boolean("Is Painted")?;
        let invulnerable = boolean("InvulnerableToPaintThinner")?;
        let mut owners = world
            .report
            .collision_parts
            .iter()
            .filter(|p| p.scene == owner.scene && p.entity_id == owner.id)
            .map(|p| p.owner)
            .collect::<Vec<_>>();
        owners.extend(
            world
                .dynamic_runtime_sources
                .iter()
                .filter(|(source, _)| world.dynamic_bodies[**source].entity == entity)
                .map(|(_, owner)| *owner),
        );
        let indices = owner
            .mesh_draws
            .values()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if indices.is_empty() {
            continue;
        }
        if templates.is_empty() {
            *templates = super::jigsaw_effects::templates(files)?;
        }
        let mut model = Model {
            painted,
            invulnerable,
            owners,
            states: BTreeMap::new(),
            transitions: BTreeMap::new(),
            alpha: BTreeMap::new(),
        };
        for index in indices {
            let original = world.draws[index].clone();
            let bindings = original
                .material
                .as_ref()
                .context("Whole-model draw lacks material")?;
            let alpha = bindings.legacy_alpha_flags.unwrap_or(0);
            let mut sketch = original.clone();
            let mut bindings = bindings.as_ref().clone();
            bindings.shader = Some(super::jigsaw_material::sketch_shader(false, alpha).into());
            let program = if bindings.textures.iter().any(|t| t.slot == 0) {
                shaders.program(bindings.shader.as_ref().unwrap())?
            } else {
                material::prepare(&bindings, sources, false).map_err(anyhow::Error::msg)?
            };
            sketch.material_images = std::sync::Arc::new(
                material::images(&program, &bindings, files, textures)
                    .map_err(anyhow::Error::msg)?,
            );
            sketch.preview = Some(Ok(program));
            sketch.material = Some(Arc::new(bindings));
            sketch.material_registers = None;
            // Transition preparation is independent of initial state loading.
            // Retain the authored endpoint and timing if a visual transition
            // cannot be composed. Collision and level events still advance.
            let transition =
                super::jigsaw_effects::prepare(&original, alpha, templates, files, textures)
                    .with_context(|| format!("Whole-model transition {} draw {index}", owner.name))
                    .map_err(|error| format!("{error:#}"));
            if let Err(error) = &transition {
                world.report.unsupported.push(error.clone());
            }
            model.transitions.insert(index, transition);
            model.alpha.insert(index, alpha);
            if !painted {
                world.draws[index] = sketch.clone();
                world.sketch_draws.insert(index);
            }
            model.states.insert(index, [original, sketch]);
        }
        for &body in &model.owners {
            world.collision.set_owner_solid(body, painted)?;
            world.model_paint_owners.insert(body, entity);
        }
        world
            .report
            .geometry
            .push(serde_json::json!({"kind":"whole_model_toon_preview",
            "entity":owner.name,"painted":painted,"invulnerable":invulnerable,
            "body_parts":model.owners.len(),"draws":model.states.len(),
            "native_filter_and_event_parity":false}));
        world.model_paint.insert(entity, model);
    }
    let limitation = "Whole toon models retain authored paint state and accept brush transitions, including admitted single-body dynamic models. Native child filtering, restoration queries, stimulus thresholds, state events/persistence and material composition remain incomplete.";
    if !world.model_paint.is_empty() && !world.report.unsupported.iter().any(|text| text == limitation) {
        world.report.unsupported.push(limitation.into());
    }
    Ok(())
}
impl World {
    ///37d2e0 initializes false, then queries ShouldSavePaintState. The native
    ///ID is the placed entity link ID; unsupported duplicate/clone identities
    ///must not silently share one persistence record.
    fn body_paint_save_key(&self, entity: usize) -> Result<Option<u32>> {
        let owner = self
            .entities
            .get(entity)
            .context("Unknown painted instance")?;
        let class = if owner
            .components
            .iter()
            .any(|component| component.class_name == "PhysicsRigidBodyDynamicComponent")
        {
            "PhysicsRigidBodyDynamicComponent"
        } else {
            "PhysicsRigidBodyStaticComponent"
        };
        let components = owner
            .components
            .iter()
            .filter(|component| component.class_name == class)
            .collect::<Vec<_>>();
        ensure!(
            components.len() == 1,
            "Ambiguous painted instance component"
        );
        let saved = components[0]
            .properties
            .iter()
            .find(|property| property.name == "ShouldSavePaintState")
            .map(|property| {
                property
                    .value
                    .as_bool()
                    .context("Invalid ShouldSavePaintState")
            })
            .transpose()?
            .unwrap_or(false);
        if !saved {
            return Ok(None);
        }
        ensure!(
            self.entities
                .iter()
                .enumerate()
                .filter(|(index, candidate)| candidate.id == owner.id
                    && !self.removed_entities.contains(index))
                .count()
                == 1,
            "Native save identity for duplicate/cloned instance {} ({:#x}) is not implemented",
            owner.name,
            owner.id
        );
        Ok(Some(owner.id))
    }
    pub fn brush_model_hit(
        &mut self,
        owner: usize,
        painted: bool,
        player_position: [f32; 3],
    ) -> Result<bool> {
        let Some(entity) = self.model_paint_owners.get(&owner).copied() else {
            return Ok(false);
        };
        self.change_whole_model_paint(entity,painted,player_position,true)
    }
    /// Scripted body commands use the same material/collision path, but the
    /// native 37e624 admission is !IsReal, without hit invulnerability.
    pub(super) fn change_whole_model_paint(
        &mut self,entity:usize,painted:bool,player_position:[f32;3],hit:bool,
    )->Result<bool> {
        if self.entities[entity].components.iter().any(|component| {
            matches!(component.class_name.as_str(),
                "PhysicsRigidBodyStaticComponent" | "PhysicsRigidBodyDynamicComponent")
                && component.properties.iter().any(|field| {
                    field.name == "Is Real" && field.value.as_bool() == Some(true)
                })
        }) {
            return Ok(false);
        }
        let model = &self.model_paint[&entity];
        let target=Target{node:entity as u64,piece:-1};
        let changed=model.painted!=painted;
        let restart=!hit && !changed && self.jigsaw_effect_queue.contains(target);
        if (hit && model.invulnerable) || (!changed && !restart) {
            return Ok(false);
        }
        let save_key = self.body_paint_save_key(entity)?;
        // Bind the same compatibility contact phase used by jigsaw bodies.
        // Whole-body scope is key -1; no child-key restriction is applied.
        // Native broadphase/ray/filter parity remains unverified.
        if changed && painted && self.report.jigsaw_overlap_guard_enabled {
            for &owner in &model.owners {
                for shape in self.collision.owner_shapes(owner) {
                    for contact in self.collision.shape_penetrations(shape, player_position)? {
                        // The model is non-real and currently unpainted; the
                        // predicate only consumes these two filter flags.
                        if super::jigsaw_overlap::contact_rejects_restore(
                            u32::MAX,
                            u32::MAX,
                            -contact.depth,
                            Some(0),
                        ) {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        let mut queue = std::mem::take(&mut self.jigsaw_effect_queue);
        if restart {queue.cancel(target);}
        let result = queue.request(
            Target {
                node: entity as u64,
                piece: -1,
            },
            entity.try_into()?,
            painted,
            false,
            self,
        );
        self.jigsaw_effect_queue = queue;
        result?;
        self.paint_admitted_dynamic_model(entity, painted)?;
        let model = self.model_paint.get_mut(&entity).unwrap();
        model.painted = painted;
        for &owner in &model.owners {
            for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
                if let Some(word) = self.collision.simulation_filter(shape) {
                    let word = super::body_filter::change_layer(
                        word,
                        word & 31,
                        u8::from(painted),
                        u8::from(word & 0x10000 != 0),
                    );
                    self.collision.set_simulation_filter(shape, word)?;
                }
            }
            self.collision.set_owner_solid(owner, painted)?;
        }
        if let Some(key) = save_key {
            self.instance_changes.set_painted(key, u8::from(painted))?;
        }
        if let Some(body) = self.entities[entity].components.iter_mut().find(|c|matches!(c.class_name.as_str(),
            "PhysicsRigidBodyDynamicComponent"|"PhysicsRigidBodyStaticComponent")) {
            if let Some(field) = body.properties.iter_mut().find(|p|p.name=="Is Painted") {
                field.value = painted.into();
            }
        }
        Ok(true)
    }
    pub fn prepare_model_effect(&mut self, target: Target, painted: bool) -> Result<()> {
        let model = self
            .model_paint
            .get(&usize::try_from(target.node)?)
            .context("Unknown whole-model effect target")?;
        for &index in model.states.keys() {
            self.jigsaw_effect_values
                .insert(index, if painted { 2. } else { 0. });
        }
        let mut endpoint = painted;
        self.advance_model_effect(target, 0., &mut endpoint)?;
        Ok(())
    }
    pub fn advance_model_effect(
        &mut self,
        target: Target,
        step: f32,
        painted: &mut bool,
    ) -> Result<bool> {
        let entity = usize::try_from(target.node)?;
        let model = self
            .model_paint
            .get(&entity)
            .context("Unknown whole-model effect target")?;
        let mut completed = false;
        for (&index, states) in &model.states {
            let Some(value) = self.jigsaw_effect_values.get_mut(&index) else {
                continue;
            };
            let next = super::jigsaw_transition::advance_effect(
                *value,
                step,
                self.report.jigsaw_time_scale,
            )?;
            *value = next.value;
            completed = next.completion.is_some();
            let draw = &mut self.draws[index];
            if let Some(endpoint) = next.completion {
                *painted = endpoint;
                let state = &states[usize::from(!endpoint)];
                draw.preview = state.preview.clone();
                draw.material = state.material.clone();
                draw.material_images = state.material_images.clone();
                draw.material_registers = state.material_registers;
                if endpoint {
                    self.sketch_draws.remove(&index);
                } else {
                    self.sketch_draws.insert(index);
                }
                self.jigsaw_effect_values.remove(&index);
            } else if let Ok(transition) = &model.transitions[&index] {
                let selection =
                    super::jigsaw_material::transition_material(model.alpha[&index], Some(*value))?;
                let state = &transition[usize::from(*value <= 1.)];
                let mut registers = state
                    .template
                    .program
                    .register_defaults
                    .context("Missing model transition registers")?;
                state
                    .template
                    .animation
                    .apply_uniforms(selection.curve_time, &mut registers)?;
                draw.preview = Some(Ok(state.template.program.clone()));
                draw.material = Some(state.bindings.clone());
                draw.material_images = std::sync::Arc::new(state.images.clone());
                draw.material_registers = Some(registers);
            }
        }
        if completed {
            let output = if *painted { "OnFullyPainted" } else { "OnFullySketched" };
            self.emit_entity_output(entity, output, self.player_entity);
        }
        Ok(completed)
    }
}
