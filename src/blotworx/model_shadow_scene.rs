//! Projector presentation through original model bounds and posed meshes.
//! Native query/filter parity and the conditional motion-height adjustment
//! remain explicit compatibility work; no substitute circle/plane is drawn.
use super::{assets::World, physics_filter::QueryFlags, scene_bounds::Sphere};
use crate::render_support::{
    material_preview::Program,
    model_shadow::{Caster, Frame},
    viewport3d::{mat4_mul, GpuDrawCommand},
};
use anyhow::{ensure, Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::{Arc, Weak},
};
#[derive(Default)]
pub struct Runtime {
    bounds: BTreeMap<usize, (String, [u32; 16], u32, Arc<BTreeMap<u32, Sphere>>)>,
    variants: BTreeMap<usize, (Weak<Program>, std::result::Result<Arc<Program>, String>)>,
    pub errors: BTreeMap<String, String>,
    pub casters: usize,
    pub receivers: usize,
    pub ground: Option<[f32; 3]>,
}
impl Runtime {
    fn bounds(
        &mut self,
        world: &World,
        entity: usize,
        draws: &[GpuDrawCommand],
    ) -> Result<Arc<BTreeMap<u32, Sphere>>> {
        let e = &world.entities[entity];
        let h = world
            .entity_bound_hierarchy(entity)?
            .context("Projector bound hierarchy missing")?;
        let rigid = entity != world.player_entity
            && !world
                .actor_playback
                .as_ref()
                .is_some_and(|p| p.entity == entity)
            && !world.model_animation.contains_key(&entity)
            && !world.actors.contains_key(&entity)
            && !world.authored_havok.contains(entity)
            && h.nodes.values().all(|n| !n.needs_runtime_bound);
        let matrix = world.projector_matrix(entity, draws).map(f32::to_bits);
        let asset = e.model_asset.as_deref().unwrap_or("");
        if rigid {
            if let Some((path, old, scale, bounds)) = self.bounds.get(&entity) {
                if path == asset && old == &matrix && *scale == e.authored_scale.to_bits() {
                    return Ok(bounds.clone());
                }
            }
        }
        let bounds = Arc::new(world.projector_bounds(entity, draws)?);
        if rigid {
            self.bounds.insert(
                entity,
                (
                    asset.into(),
                    matrix,
                    e.authored_scale.to_bits(),
                    bounds.clone(),
                ),
            );
        } else {
            self.bounds.remove(&entity);
        }
        Ok(bounds)
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"casters":self.casters,"receiver_draws":self.receivers,"ground":self.ground,"errors":self.errors,
        "native_query_parity_validated":false,"conditional_motion_height_adjustment_connected":false,"original_frame_parity_validated":false})
    }
    fn prepare(
        &mut self,
        world: &mut World,
        draws: &mut [GpuDrawCommand],
        brush: Option<Range<usize>>,
    ) -> Result<Option<Arc<Frame>>> {
        self.errors.clear();
        self.casters = 0;
        self.receivers = 0;
        self.ground = None;
        self.variants
            .retain(|_, (source, _)| source.strong_count() > 0);
        self.bounds
            .retain(|entity, _| !world.removed_entities.contains(entity));
        // REL255884..255958 enrolls the player and attached brush directly.
        // Renderer byte29/RenderSilhouettes control a different main-camera
        // pass; neither selects the offscreen projector's caster list.
        let mut spheres = Vec::new();
        let mut casters = Vec::new();
        let mut caster_entities = BTreeSet::new();
        let mut player_point = None;
        let brush_entity = world.child_entity_by_name(world.player_entity, "magicbrush")?;
        for entity in 0..world.entities.len() {
            if entity != world.player_entity && Some(entity) != brush_entity {
                continue;
            }
            if world.hidden_entities.contains(&entity) || world.removed_entities.contains(&entity) {
                continue;
            }
            let result = (|| -> Result<()> {
                let hierarchy = world
                    .entity_bound_hierarchy(entity)?
                    .context("Caster model hierarchy missing")?;
                let roots: Vec<_> = hierarchy.roots.iter().flatten().copied().collect();
                ensure!(roots.len() == 1, "Caster root selection is ambiguous");
                let Some((_, mesh)) = hierarchy.primary_actor_mesh(roots[0])? else {
                    return Ok(());
                };
                let bounds = self.bounds(world, entity, draws)?;
                if entity == world.player_entity {
                    player_point = Some(
                        bounds
                            .get(&roots[0])
                            .context("Player root bound missing")?
                            .center,
                    );
                }
                let indices = world.entities[entity]
                    .mesh_draws
                    .get(&mesh)
                    .context("Primary caster mesh draws missing")?;
                let visible: Vec<_> = indices
                    .iter()
                    .copied()
                    .filter(|i| {
                        !world.hidden_entity_draws.contains(i)
                            && !world.hidden_jigsaw_draws.contains(i)
                            && !world.removed_draws.contains(i)
                    })
                    .collect();
                if visible.is_empty() {
                    return Ok(());
                }
                spheres.push(*bounds.get(&mesh).context("Caster sphere missing")?);
                for i in visible {
                    let d = &draws[i];
                    casters.push(Caster {
                        vertices: d.v_data.clone(),
                        indices: d.indices.clone(),
                        model: d.trans_mat,
                    });
                }
                caster_entities.insert(entity);
                Ok(())
            })();
            if let Err(error) = result {
                self.errors
                    .insert(world.entities[entity].name.clone(), format!("{error:#}"));
            }
        }
        if let Some(range) = brush {
            if let Some((bound, parts)) = world.brush.shadow_parts(&draws[range])? {
                spheres.push(bound);
                casters.extend(parts);
            }
        }
        if casters.is_empty() {
            return Ok(None);
        }
        let point = player_point.context("Player projector anchor unavailable")?;
        let collision = &world.collision;
        let filter = &mut world.dynamic_runtime_filter;
        let flags = Some(QueryFlags {
            bytes: [1, 1, 1, 0, 1],
            excluded_group: 0,
        });
        let (owners, hit) = filter.query(flags, |shared| {
            let mut owners = BTreeSet::new();
            for candidate in collision.with_player_queries(|| {
                collision.overlap_body_bounds(point.map(|v| v - 0.2), point.map(|v| v + 0.2))
            })? {
                if shared.collision_info(
                    0x00602003,
                    collision.simulation_filter_word(candidate.shape)?,
                ) >> 24
                    != 0
                {
                    owners.insert(candidate.owner);
                }
            }
            let origin = [point[0], point[1] + 0.1, point[2]];
            let mut closest: Option<super::physics::StaticRayHit> = None;
            for hit in
                collision.with_player_queries(|| collision.ray_hits(origin, [0., -10.1, 0.]))?
            {
                if shared.collision_info(0x00602003, collision.simulation_filter_word(hit.shape)?)
                    >> 24
                    == 0
                {
                    continue;
                }
                if closest
                    .as_ref()
                    .is_none_or(|old| hit.fraction < old.fraction)
                {
                    closest = Some(hit);
                }
            }
            if let Some(hit) = &closest {
                owners.insert(hit.owner);
            }
            Ok((owners, closest))
        })?;
        self.ground = hit.as_ref().map(|h| h.position);
        let ground_y = hit.as_ref().map_or(point[1], |h| h.position[1]);
        let direction = super::spherical_lighting::vector_length_and_direction([0., -1., 0.]).1;
        let projection = super::projected_shadow::fit(spheres, direction, 2., 200.)?
            .context("Missing caster spheres")?;
        let model = super::projected_shadow::model_projection(projection.bound.radius)?;
        let plane = [0., 0., 1., ((point[1] - ground_y) + 100.) - 0.5];
        let effect = super::texture_effect::parallel(
            &super::projected_shadow::camera_transform(&projection.camera),
            &model,
            plane,
        )?;
        let mut receivers = BTreeSet::new();
        for owner in owners {
            if let Some(entity) = world.collision_entity(owner) {
                if let Some(index) = world.entities.iter().position(|e| std::ptr::eq(e, entity)) {
                    if !caster_entities.contains(&index) {
                        receivers.insert(index);
                    }
                }
            }
        }
        for entity in receivers {
            let bounds = match self.bounds(world, entity, draws) {
                Ok(b) => b,
                Err(e) => {
                    self.errors.insert(
                        world.entities[entity].name.clone(),
                        format!("Receiver bounds: {e:#}"),
                    );
                    continue;
                }
            };
            for (&mesh, indices) in &world.entities[entity].mesh_draws {
                let Some(bound) = bounds.get(&mesh) else {
                    self.errors.insert(
                        format!("{}:{mesh}", world.entities[entity].name),
                        "Receiver sphere missing".into(),
                    );
                    continue;
                };
                let dx = bound.center[0] - projection.bound.center[0];
                let dz = bound.center[2] - projection.bound.center[2];
                let radius = bound.radius + projection.bound.radius;
                if dx.mul_add(dx, dz * dz) > radius * radius {
                    continue;
                }
                for &i in indices {
                    let Some(Ok(base)) = draws[i].preview.as_ref() else {
                        continue;
                    };
                    let key = Arc::as_ptr(base) as usize;
                    let (_, variant) = self.variants.entry(key).or_insert_with(|| {
                        (
                            Arc::downgrade(base),
                            base.projected_shadow()
                                .map(Arc::new)
                                .map_err(|e| format!("{e:#}")),
                        )
                    });
                    match variant {
                        Ok(program) => {
                            draws[i].preview = Some(Ok(program.clone()));
                            draws[i].projected_shadow = true;
                            self.receivers += 1;
                        }
                        Err(e) => {
                            self.errors.insert(
                                format!("{}:{mesh}", world.entities[entity].name),
                                e.clone(),
                            );
                        }
                    }
                }
            }
        }
        self.casters = casters.len();
        let native = effect.projection;
        let rows = std::array::from_fn(|i| {
            if i % 4 == 3 {
                native[9 + i / 4]
            } else {
                native[(i / 4) * 3 + i % 4]
            }
        });
        Ok(Some(Arc::new(Frame {
            view_projection: mat4_mul(&projection.camera.projection()?, &projection.camera.view()),
            projection: rows,
            clip: super::texture_effect::clipping_matrix(effect.plane)?,
            casters,
        })))
    }
}
impl World {
    fn projector_matrix(&self, entity: usize, draws: &[GpuDrawCommand]) -> [f32; 16] {
        let e = &self.entities[entity];
        e.draws
            .clone()
            .find(|i| !self.model_local_draws.contains(i))
            .and_then(|i| draws.get(i))
            .map(|d| d.trans_mat)
            .unwrap_or(e.transform)
    }
    pub fn prepare_model_shadow(
        &mut self,
        draws: &mut [GpuDrawCommand],
        brush: Option<Range<usize>>,
    ) -> Result<Option<Arc<Frame>>> {
        let mut runtime = std::mem::take(&mut self.model_shadow);
        let result = runtime.prepare(self, draws, brush);
        self.model_shadow = runtime;
        result
    }
    fn projector_bounds(
        &self,
        entity: usize,
        draws: &[GpuDrawCommand],
    ) -> Result<BTreeMap<u32, Sphere>> {
        let e = &self.entities[entity];
        let hierarchy = self
            .entity_bound_hierarchy(entity)?
            .context("Projector model missing")?;
        let matrix = self.projector_matrix(entity, draws);
        let scale = e.authored_scale;
        ensure!(
            scale.is_finite() && scale > 0.,
            "Invalid projector owner scale"
        );
        let parent = epic_mickey_lib_rs::nif_file::NiTransform {
            rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                rows: std::array::from_fn(|r| std::array::from_fn(|c| matrix[c * 4 + r] / scale)),
            },
            translation: epic_mickey_lib_rs::nif_file::Vector3 {
                x: matrix[12],
                y: matrix[13],
                z: matrix[14],
            },
            scale,
        };
        let empty = BTreeMap::new();
        let previous = self.spherical_bounds.get(&entity).unwrap_or(&empty);
        if entity == self.player_entity {
            hierarchy.actor_world_bounds(&parent, previous, &BTreeMap::new())
        } else if let Some(playback) = self.actor_playback.as_ref().filter(|p| p.entity == entity) {
            hierarchy.actor_world_bounds(&parent, previous, &playback.pose.transforms)
        } else if let Some(actor) = self.actors.get(&entity) {
            let empty = BTreeMap::new();
            let poses = actor
                .pose
                .as_ref()
                .map(|p| &p.transforms)
                .or_else(|| self.model_animation.get(&entity).map(|a| &a.poses))
                .unwrap_or(&empty);
            hierarchy.actor_world_bounds(&parent, previous, poses)
        } else if let Some(animation) = self.model_animation.get(&entity) {
            animation.bounds(hierarchy, &parent, previous)
        } else {
            hierarchy.static_world_bounds(Some(&parent), previous)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_projector_uses_model_meshes_independent_of_main_camera_gate() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut world = super::super::assets::load(
            &root,
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut draws = world.draws.clone();
        let result = world.prepare_model_shadow(&mut draws, None);
        std::fs::write(
            "target/model-shadow-scene-probe.json",
            serde_json::to_vec_pretty(&world.model_shadow.summary())?,
        )?;
        let frame = result?.context("Original player projector was not produced")?;
        ensure!(
            !frame.casters.is_empty() && world.model_shadow.receivers > 0,
            "Original caster/receiver admission empty"
        );
        ensure!(
            frame
                .casters
                .iter()
                .all(|c| world
                    .draws
                    .iter()
                    .any(|d| Arc::ptr_eq(&d.v_data, &c.vertices)
                        && Arc::ptr_eq(&d.indices, &c.indices))),
            "Caster did not retain its model geometry"
        );
        ensure!(
            world.model_shadow.errors.is_empty(),
            "Projector errors: {:?}",
            world.model_shadow.errors
        );
        world.render_options.draw_silhouettes = false;
        ensure!(
            world.prepare_model_shadow(&mut draws, None)?.is_some(),
            "Main-camera silhouette gate incorrectly disabled the separate player projector"
        );
        Ok(())
    }
}
