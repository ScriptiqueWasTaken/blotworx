//! Live transition preview using original R3MT/RCLA and the recovered queue.
//! Standalone template shading is provisional: native base-stage composition,
//! object projector overrides, register inheritance and pass ordering remain pending.
use super::{
    assets::World,
    jigsaw_transition::{Host, Target},
};
use crate::render_support::{
    material_preview as material,
    nif::{MaterialBindings, MaterialTexture},
    viewport3d::{DecodedImage, GpuDrawCommand, TextureSampler},
};
use anyhow::{Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
use std::{collections::HashMap, sync::Arc};
pub struct Template {
    pub program: Arc<material::Program>,
    pub animation: Arc<super::material_curve::ColorAnimation>,
    pub texture: String,
}
pub struct Draw {
    pub template: Arc<Template>,
    pub images: Vec<material::MaterialImage>,
    pub bindings: Arc<MaterialBindings>,
}
pub fn templates(files: &[VirtualFile]) -> Result<HashMap<String, Arc<Template>>> {
    let mut result = HashMap::new();
    for name in [
        "GEN_paintToSketch",
        "GEN_paintToSketchOpaque",
        "GEN_sketchToPaint",
        "GEN_sketchToPaintOpaque",
    ] {
        let find = |suffix: &str| {
            files
                .iter()
                .find(|f| {
                    f.path
                        .to_ascii_lowercase()
                        .ends_with(&suffix.to_ascii_lowercase())
                })
                .with_context(|| format!("Missing transition asset {suffix}"))
        };
        let shader = find(&format!("/{name}.r3mt"))?;
        let curve = find(&format!("/{name}.rcla"))?;
        let texture = find(if name.contains("paintToSketch") {
            "/GEN_toSketchTransition_tex.nif_wii"
        } else {
            "/GEN_toPaintTransition_tex.nif_wii"
        })?;
        let program = material::compile_uniform_registers(
            std::str::from_utf8(&shader.data)?.trim_start_matches('\u{feff}'),
            false,
            &crate::tev::Registers::default(),
        )?;
        result.insert(
            name.into(),
            Arc::new(Template {
                program: Arc::new(program),
                animation: Arc::new(super::material_curve::ColorAnimation::parse(
                    std::str::from_utf8(&curve.data)?,
                )?),
                texture: texture.path.clone(),
            }),
        );
    }
    Ok(result)
}
pub fn prepare(
    original: &GpuDrawCommand,
    alpha: u16,
    templates: &HashMap<String, Arc<Template>>,
    files: &[VirtualFile],
    cache: &mut HashMap<String, Option<DecodedImage>>,
) -> Result<[Draw; 2]> {
    let mut states = Vec::new();
    for scalar in [2., 0.] {
        let name = super::jigsaw_material::transition_material(alpha, Some(scalar))?.shader;
        let mut template = templates[name].clone();
        let mut bindings = original
            .material
            .as_ref()
            .context("Missing transition base material")?
            .as_ref()
            .clone();
        if !bindings.textures.iter().any(|t| t.slot == 0) {
            let base = original
                .preview
                .as_ref()
                .and_then(|p| p.as_ref().ok())
                .with_context(|| {
                    format!(
                        "Untextured transition base shader unavailable: {:?}",
                        bindings.shader
                    )
                })?;
            anyhow::ensure!(
                !base.textured && base.register_defaults.is_none(),
                "Untextured transition base needs separate sampled-material composition"
            );
            let mut program = template.program.as_ref().clone();
            let start = base
                .fragment
                .find(crate::tev::EFFECT_HOOK)
                .context("Missing generated base combiner hook")?;
            let main = base
                .fragment
                .find("void main(){")
                .context("Missing generated base body")?;
            let end = base.fragment[main..]
                .find("float alpha=")
                .context("Missing generated base result")?
                + main;
            let function = base.fragment[start..main].replace("bt_combiner", "bt_base_combiner");
            let body = base.fragment[main + 12..end].replace("bt_combiner", "bt_base_combiner");
            let base_function =
                format!("{function}\nvec4 bt_base_surface(){{{body}return result;}}\n");
            let sample = "textureProj(u_tex[0],u_tex_uv[0]*vec3(bw_uv(0),1.0))";
            anyhow::ensure!(
                program.fragment.contains(sample),
                "Unsupported transition diffuse expression"
            );
            program.fragment = program.fragment.replace(sample, "bt_base_surface()");
            let insertion = program
                .fragment
                .find("vec4 bt_combiner(")
                .context("Missing transition combiner")?;
            program.fragment.insert_str(insertion, &base_function);
            program.texture_slots.retain(|&slot| slot != 0);
            program.uv_textures.retain(|&slot| slot != 0);
            template = Arc::new(Template {
                program: Arc::new(program),
                animation: template.animation.clone(),
                texture: template.texture.clone(),
            });
        }
        bindings.shader = Some(name.into());
        bindings.textures.retain(|t| t.slot == 0);
        bindings.textures.push(MaterialTexture {
            shader_id: Some(0),
            slot: 1,
            uv_set: 0,
            path: template.texture.clone(),
            transform: None,
            sampler: TextureSampler {
                wrap_u: glow::REPEAT,
                wrap_v: glow::REPEAT,
                min_filter: glow::LINEAR,
                mag_filter: glow::LINEAR,
            },
        });
        let images = material::images(&template.program, &bindings, files, cache)
            .map_err(anyhow::Error::msg)?;
        states.push(Draw {
            template,
            images,
            bindings: Arc::new(bindings),
        });
    }
    Ok(states.try_into().ok().unwrap())
}
// Whole-model targets use entity indices; jigsaw targets use instance indices.
// Preserve native key -1 without aliasing the two internal namespaces.
const WHOLE_JIGSAW: u64 = 1 << 63;
impl World {
    fn jigsaw_effect_meshes(&self, target: Target) -> Result<Vec<u32>> {
        let index = (target.node & !WHOLE_JIGSAW) as usize;
        let instance = self.jigsaw_instances.get(index).context("Unknown jigsaw effect target")?;
        if target.piece == -1 {
            // Native full-node traversal is mesh based. Visiting pieces here
            // advances shared mesh links multiple times in the same frame.
            let mut meshes = instance.mesh_draws.keys().copied().collect::<Vec<_>>();
            meshes.sort_unstable();
            return Ok(meshes);
        }
        let entity = &self.entities[instance.entity];
        let asset = &self.jigsaws[entity.jigsaw_asset.as_ref().unwrap()][instance.node];
        let piece = asset.pieces.get(usize::try_from(target.piece)?).context("Unknown jigsaw effect piece")?;
        Ok(piece.mesh_links.iter().chain(&piece.secondary_mesh_links).copied().collect())
    }
    /// Scripted whole-body operation. Collision changes at admission; restored
    /// topology waits for the material endpoint, as with piece brush effects.
    pub(super) fn change_whole_jigsaw_paint(&mut self, entity: usize, painted: bool, position: [f32;3]) -> Result<bool> {
        let instances = self.jigsaw_instances.iter().enumerate()
            .filter_map(|(i, instance)| (instance.entity == entity).then_some(i)).collect::<Vec<_>>();
        if instances.is_empty() { return Ok(false); }
        let body = self.entities[entity].components.iter().find(|c|matches!(c.class_name.as_str(),
            "PhysicsRigidBodyDynamicComponent"|"PhysicsRigidBodyStaticComponent"|"PhysicsJigsawComponent"));
        if body.and_then(|c| c.properties.iter().find(|p|p.name=="Is Real"))
            .and_then(|p|p.value.as_bool()) == Some(true) { return Ok(false); }
        let restoring = instances.iter().any(|&i| {
            let instance=&self.jigsaw_instances[i];
            self.entities[entity].jigsaw_states[instance.node].painted.iter().any(|&v|!v)
        });
        if painted && restoring && self.report.jigsaw_overlap_guard_enabled {
            for &i in &instances {
                let instance = &self.jigsaw_instances[i];
                let owner = &self.entities[entity];
                let asset = &self.jigsaws[owner.jigsaw_asset.as_ref().unwrap()][instance.node];
                if instance.collision.contacts_reject_restore(asset, &owner.jigsaw_states[instance.node],
                    &self.collision, position, u32::MAX)? { return Ok(false); }
            }
        }
        for i in instances {
            let target = Target { node: WHOLE_JIGSAW | i as u64, piece: -1 };
            let instance = &self.jigsaw_instances[i];
            let changed = self.entities[entity].jigsaw_states[instance.node].painted.iter().any(|&v|v!=painted);
            let restart = !changed && self.jigsaw_effect_queue.contains(target);
            if !changed && !restart { continue; }
            let owners = self.jigsaw_owners.iter().filter_map(|(&owner, source)|
                (source.instance == i).then_some(owner)).collect::<Vec<_>>();
            let mut queue = std::mem::take(&mut self.jigsaw_effect_queue);
            // A whole-body request supersedes outstanding piece effects.
            for source in self.jigsaw_owners.values().filter(|source|source.instance==i) {
                queue.cancel(Target { node: i as u64, piece: source.piece as i32 });
            }
            if restart { queue.cancel(target); }
            let result = queue.request(target, entity.try_into()?, painted, false, self);
            self.jigsaw_effect_queue = queue;
            result?;
            for owner in owners { self.publish_jigsaw_piece_collision(owner, painted)?; }
            // Render-only pieces have no Box3D owner, but still participate in
            // whole-body paint state and subsequent command admission.
            let node = self.jigsaw_instances[i].node;
            self.entities[entity].jigsaw_states[node].painted.fill(painted);
        }
        if let Some(body) = self.entities[entity].components.iter_mut().find(|c|matches!(c.class_name.as_str(),
            "PhysicsRigidBodyDynamicComponent"|"PhysicsRigidBodyStaticComponent"|"PhysicsJigsawComponent")) {
            if let Some(field) = body.properties.iter_mut().find(|p|p.name=="Is Painted") {
                field.value = painted.into();
            }
        }
        Ok(true)
    }
    /// Live projectile entry point. Pass the controller's current simulation
    /// pose; the render entity pose is not updated until after the tick batch.
    pub fn brush_jigsaw_hit(
        &mut self,
        hit: &super::physics::SweepHit,
        painted: bool,
        player_position: [f32; 3],
    ) -> Result<bool> {
        let Some(source) = self.jigsaw_owners.get(&hit.owner).copied() else {
            return self.brush_model_hit(hit.owner, painted, player_position);
        };
        let instance = &self.jigsaw_instances[source.instance];
        let entity = &self.entities[instance.entity];
        if entity.jigsaw_states[instance.node].painted[source.piece] == painted {
            return Ok(false);
        }
        if painted && self.report.jigsaw_overlap_guard_enabled {
            let asset = &self.jigsaws[entity.jigsaw_asset.as_ref().unwrap()][instance.node];
            let hit_key = match hit.source_triangle {
                Some(triangle) => instance.collision.triangle_key(asset, triangle)?,
                None => instance.collision.convex_key(asset, hit.shape)?,
            }
            .context("Jigsaw projectile lost its native shape key")?;
            let requested_key = if instance.allow_nonoverlapping_painting {
                hit_key
            } else {
                u32::MAX
            };
            // The component owns all its jigsaw nodes, not only the struck
            // piece. Preserve that scope when looking for overlapping bodies.
            for associated in self
                .jigsaw_instances
                .iter()
                .filter(|other| other.entity == instance.entity)
            {
                let asset = &self.jigsaws[entity.jigsaw_asset.as_ref().unwrap()][associated.node];
                if associated.collision.contacts_reject_restore(
                    asset,
                    &entity.jigsaw_states[associated.node],
                    &self.collision,
                    player_position,
                    requested_key,
                )? {
                    return Ok(false);
                }
            }
        }
        self.brush_jigsaw_contact(hit.owner, painted)
    }
    /// Unguarded transition primitive, also used by explicit diagnostics.
    pub fn brush_jigsaw_contact(&mut self, owner: usize, painted: bool) -> Result<bool> {
        let Some(source) = self.jigsaw_owners.get(&owner).copied() else {
            return Ok(false);
        };
        let instance = &self.jigsaw_instances[source.instance];
        if self.entities[instance.entity].jigsaw_states[instance.node].painted[source.piece]
            == painted
        {
            return Ok(false);
        }
        let asset = &self.jigsaws[self.entities[instance.entity]
            .jigsaw_asset
            .as_ref()
            .unwrap()][instance.node];
        let piece = &asset.pieces[source.piece];
        if !piece
            .mesh_links
            .iter()
            .chain(&piece.secondary_mesh_links)
            .any(|mesh| {
                instance.mesh_draws[mesh]
                    .iter()
                    .any(|draw| instance.transition_states.contains_key(draw))
            })
        {
            return self.preview_jigsaw_contact(owner, painted);
        }
        let target = Target {
            node: source.instance as u64,
            piece: source.piece.try_into()?,
        };
        let entity = instance.entity.try_into()?;
        let mut queue = std::mem::take(&mut self.jigsaw_effect_queue);
        let result = queue.request(target, entity, painted, false, self);
        self.jigsaw_effect_queue = queue;
        result?;
        self.publish_jigsaw_piece_collision(owner, painted)?;
        Ok(true)
    }
    /// Keep the split-piece Box3D compatibility body, query filter and source
    /// paint state coherent for both queued gameplay and immediate transitions.
    pub(super) fn publish_jigsaw_piece_collision(
        &mut self,
        owner: usize,
        painted: bool,
    ) -> Result<()> {
        let source = self
            .jigsaw_owners
            .get(&owner)
            .context("Unknown jigsaw collision owner")?;
        let instance = &self.jigsaw_instances[source.instance];
        instance
            .collision
            .set_painted(source.piece, painted, &mut self.collision)?;
        let state = &mut self.entities[instance.entity].jigsaw_states[instance.node];
        state.painted[source.piece] = painted;
        let word = state
            .filter(source.piece, 5)
            .context("Painted solver piece missing")?;
        for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
            self.collision.set_simulation_filter(shape, word)?;
        }
        Ok(())
    }
    pub fn advance_jigsaw_effects(&mut self, dt: f32) -> Result<()> {
        let mut queue = std::mem::take(&mut self.jigsaw_effect_queue);
        let result = queue.update(dt, self);
        self.jigsaw_effect_queue = queue;
        result
    }
}
impl Host for World {
    fn owner_exists(&self, owner: u32) -> bool {
        (owner as usize) < self.entities.len() && !self.removed_entities.contains(&(owner as usize))
    }
    fn prepare(&mut self, target: Target, painted: bool) -> Result<()> {
        if target.piece == -1 && target.node & WHOLE_JIGSAW == 0 {
            return self.prepare_model_effect(target, painted);
        }
        let meshes = self.jigsaw_effect_meshes(target)?;
        let instance = &self.jigsaw_instances[(target.node & !WHOLE_JIGSAW) as usize];
        for mesh in &meshes {
            for &index in &instance.mesh_draws[mesh] {
                if instance.transition_states.contains_key(&index) {
                    self.jigsaw_effect_values
                        .insert(index, if painted { 2. } else { 0. });
                } else {
                    // Explicit loader warning tracks unsupported interior/noise
                    // composition; retain the existing instantaneous endpoint.
                    let state = &instance.render_states[&index][usize::from(!painted)];
                    let draw = &mut self.draws[index];
                    draw.preview = state.preview.clone();
                    draw.material = state.material.clone();
                    draw.material_images = state.material_images.clone();
                    draw.material_registers = None;
                    if painted {
                        self.sketch_draws.remove(&index);
                    } else {
                        self.sketch_draws.insert(index);
                    }
                }
            }
        }
        if target.node & WHOLE_JIGSAW != 0 && !painted {
            self.set_thinned(target, true)?;
        }
        let mut endpoint = painted;
        self.advance(target, 0., &mut endpoint)?;
        Ok(())
    }
    fn set_thinned(&mut self, target: Target, thinned: bool) -> Result<()> {
        if target.piece == -1 {
            let i = target.node & !WHOLE_JIGSAW;
            let instance = &self.jigsaw_instances[i as usize];
            let count = self.entities[instance.entity].jigsaw_states[instance.node].painted.len();
            for piece in 0..count {
                self.set_thinned(Target { node: i, piece: piece.try_into()? }, thinned)?;
            }
            return Ok(());
        }
        let instance = &mut self.jigsaw_instances[target.node as usize];
        let entity = &self.entities[instance.entity];
        let asset = &self.jigsaws[entity.jigsaw_asset.as_ref().unwrap()][instance.node];
        instance.topology.set_thinned(
            &asset.pieces,
            &asset.adjacency,
            target.piece as usize,
            thinned,
            |mesh, visible| {
                for &index in &instance.mesh_draws[&mesh] {
                    if visible {
                        self.hidden_jigsaw_draws.remove(&index);
                    } else {
                        self.hidden_jigsaw_draws.insert(index);
                    }
                }
            },
        )
    }
    fn advance(&mut self, target: Target, step: f32, painted: &mut bool) -> Result<bool> {
        if target.piece == -1 && target.node & WHOLE_JIGSAW == 0 {
            return self.advance_model_effect(target, step, painted);
        }
        let meshes = self.jigsaw_effect_meshes(target)?;
        let instance = &self.jigsaw_instances[(target.node & !WHOLE_JIGSAW) as usize];
        let entity = &self.entities[instance.entity];
        let asset = &self.jigsaws[entity.jigsaw_asset.as_ref().unwrap()][instance.node];
        // Unsupported material compositions use the explicitly reported
        // endpoint fallback; they must not leave an immortal queue entry.
        let mut completed = !meshes.iter().any(|mesh| instance.mesh_draws[mesh].iter()
            .any(|index| self.jigsaw_effect_values.contains_key(index)));
        for mesh in &meshes {
            for &index in &instance.mesh_draws[mesh] {
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
                    let state = &instance.render_states[&index][usize::from(!endpoint)];
                    draw.preview = state.preview.clone();
                    draw.material = state.material.clone();
                    draw.material_images = state.material_images.clone();
                    draw.material_registers = None;
                    if endpoint {
                        self.sketch_draws.remove(&index);
                    } else {
                        self.sketch_draws.insert(index);
                    }
                    self.jigsaw_effect_values.remove(&index);
                } else {
                    let selection = super::jigsaw_material::transition_material(
                        asset.mesh_alpha_flags[mesh],
                        Some(*value),
                    )?;
                    let state = &instance.transition_states[&index][usize::from(*value <= 1.)];
                    let mut registers = state
                        .template
                        .program
                        .register_defaults
                        .context("Missing transition registers")?;
                    state
                        .template
                        .animation
                        .apply_uniforms(selection.curve_time, &mut registers)?;
                    if !draw.preview.as_ref().is_some_and(|p| {
                        p.as_ref()
                            .is_ok_and(|p| Arc::ptr_eq(p, &state.template.program))
                    }) {
                        draw.preview = Some(Ok(state.template.program.clone()));
                        draw.material = Some(state.bindings.clone());
                        draw.material_images = std::sync::Arc::new(state.images.clone());
                    }
                    draw.material_registers = Some(registers);
                }
            }
        }
        if completed {
            for mesh in &meshes {
                for &index in &instance.mesh_draws[mesh] {
                    if instance.transition_states.contains_key(&index) {
                        continue;
                    }
                    let state = &instance.render_states[&index][usize::from(!*painted)];
                    let draw = &mut self.draws[index];
                    draw.preview = state.preview.clone();
                    draw.material = state.material.clone();
                    draw.material_images = state.material_images.clone();
                    draw.material_registers = None;
                    if *painted {
                        self.sketch_draws.remove(&index);
                    } else {
                        self.sketch_draws.insert(index);
                    }
                }
            }
        }
        if completed && *painted && target.node & WHOLE_JIGSAW != 0 {
            self.set_thinned(target, false)?;
        }
        Ok(completed)
    }
}
