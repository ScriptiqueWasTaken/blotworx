//! Normal brush ray selection, joining recovered Cursor3D policy to live data.
//! Box3D intersection/arrival order and phantom AABB parity remain unverified;
//! alternate scripted rays, radius casts and cursor-model rendering are separate.
use super::{
    assets::World,
    brush::Ink,
    cursor,
    physics::{SphereCandidate, V},
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeSet;
const MASK: u32 = 0x69005b; // REL73d54..73d80
pub struct Runtime {
    state: cursor::TargetState<usize>,
    point: V,
    normal: V,
    queries: u64,
    phantom_size: f32,
    near_distance: f32,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            state: cursor::TargetState {
                hit: false,
                hit_normal: [0.; 3],
                entity: None,
            },
            point: [0.; 3],
            normal: [0., 1., 0.],
            queries: 0,
            phantom_size: 0.,
            near_distance: 0.,
        }
    }
}
struct Bindings<'a> {
    world: &'a World,
    near_bodies: BTreeSet<usize>,
}
impl cursor::TargetWorld<(SphereCandidate, u32), usize> for Bindings<'_> {
    fn collision_filter(&mut self, hit: &cursor::CursorHit<(SphereCandidate, u32)>) -> Result<u32> {
        Ok(hit.collidable.1)
    }
    fn rejection(&mut self, hit: &(SphereCandidate, u32)) -> Result<u32> {
        Ok(
            if self
                .near_bodies
                .contains(&self.world.collision.query_body_identity(hit.0.shape)?)
            {
                0x01000000
            } else {
                0
            },
        )
    }
    fn is_live(&mut self, handle: &usize) -> Result<bool> {
        Ok(*handle < self.world.entities.len() && !self.world.removed_entities.contains(handle))
    }
    fn candidate_target(&mut self, hit: &(SphereCandidate, u32)) -> Result<Option<usize>> {
        let Some(entity) = self.world.collision_entity(hit.0.owner) else {
            return Ok(None);
        };
        if !entity
            .components
            .iter()
            .any(|c| c.class_name == "TargetableComponent")
        {
            return Ok(None);
        }
        self.world
            .entities
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entity))
            .map(Some)
            .context("Cursor entity is outside the loaded world")
    }
}
impl World {
    pub(super) fn brush_cursor_summary(&self) -> serde_json::Value {
        let state = &self.brush_cursor;
        serde_json::json!({"queries":state.queries,"hit":state.state.hit,"point":state.point,
            "hit_normal":state.state.hit_normal,"target_entity":state.state.entity,
            "target_name":state.state.entity.and_then(|index|self.entities.get(index)).map(|entity|&entity.name),
            "near_camera_distance":state.near_distance,"brush_phantom_half_extent":state.phantom_size,
            "scope":"Normal brush ray; Box3D narrowphase/phantom parity, scripted rays and cursor-model rendering remain unverified"})
    }
    /// RELaa8e0..aa904: paint permits ordinary surfaces, thinner rejects them;
    /// near-camera geometry is enabled, vertical fallback disabled, ray radius0.
    pub(super) fn brush_cursor_target(
        &mut self,
        ray: cursor::Ray,
        camera_position: V,
        owner: V,
        range: f32,
        ink: Ink,
    ) -> Result<V> {
        ensure!(
            range.is_finite() && range > 0.,
            "Invalid brush cursor range"
        );
        ensure!(
            ray.origin
                .iter()
                .chain(&ray.direction)
                .chain(&camera_position)
                .chain(&owner)
                .all(|v| v.is_finite()),
            "Invalid brush cursor vectors"
        );
        let setting = |name: &str, default: f32| -> Result<f32> {
            let value = self
                .runtime_settings
                .get(name)
                .map(|s| s.parse::<f32>().with_context(|| format!("Invalid {name}")))
                .transpose()?
                .unwrap_or(default);
            ensure!(value.is_finite(), "Nonfinite {name}");
            Ok(value)
        };
        let phantom_size = setting("MickeyAbilities_Brush_PhantomSize", 0.15)?; // RELa73b0..a746c
        let near_distance = setting("Cursor_Detection_AimBehind_MinDistance", 0.)?;
        let end = cursor::preferred_endpoint(owner, range, ray.origin, ray.direction);
        ensure!(
            end.iter().all(|v| v.is_finite()),
            "Invalid brush cursor endpoint"
        );
        let mut filter = self.dynamic_runtime_filter.clone();
        let mut runtime = std::mem::take(&mut self.brush_cursor);
        let result = filter.query(
            Some(super::physics_filter::QueryFlags {
                bytes: [1, 0, 1, 0, 0],
                excluded_group: 0,
            }),
            |view| -> Result<V> {
                let mut near_bodies = BTreeSet::new();
                if phantom_size > 0. {
                    let lower = owner.map(|x| x - phantom_size);
                    let upper = owner.map(|x| x + phantom_size);
                    for candidate in self
                        .collision
                        .with_player_queries(|| self.collision.overlap_body_bounds(lower, upper))?
                    {
                        near_bodies.insert(candidate.shape);
                    }
                }
                let raw = self.collision.with_player_queries(|| {
                    self.collision
                        .ray_hits(ray.origin, super::physics::sub(end, ray.origin))
                })?;
                let mut hits = Vec::new();
                hits.try_reserve(raw.len())?;
                for hit in raw {
                    // Match the world query before resolving child/material data.
                    if view.collision_info(MASK, self.collision.simulation_filter_word(hit.shape)?)
                        >> 24
                        == 0
                    {
                        continue;
                    }
                    let candidate = SphereCandidate {
                        shape: hit.shape,
                        owner: hit.owner,
                        source_triangle: hit.source_triangle,
                    };
                    let metadata = self.collision_shape_metadata(candidate)?;
                    if metadata
                        .child_filter
                        .is_some_and(|word| view.collision_info(MASK, word) >> 24 == 0)
                    {
                        continue;
                    }
                    let mut keys = [u32::MAX; 8];
                    keys[0] = metadata.key;
                    hits.push(cursor::CursorHit {
                        fraction: hit.fraction,
                        normal: hit.normal,
                        collidable: (candidate, metadata.material),
                        shape_keys: keys,
                    });
                }
                super::cast_contacts::sort_fractions(&mut hits, |hit| hit.fraction)?;
                let query = cursor::RaySelection {
                    start: ray.origin,
                    end,
                    reject_ordinary: ink == Ink::Thinner,
                    use_hit_normal: false,
                    owner: Some((owner, range)),
                    near_camera: Some(cursor::NearCamera {
                        position: camera_position,
                        threshold: near_distance,
                    }),
                };
                let mut bindings = Bindings {
                    world: self,
                    near_bodies,
                };
                let hit = cursor::select_ray_hits(
                    &hits,
                    &query,
                    &mut bindings,
                    &mut runtime.state,
                    &mut runtime.point,
                    &mut runtime.normal,
                )?;
                // REL73ed8..73efc replaces the candidate with the preferred endpoint
                // when no ray hit is accepted. Model placement is not performed here.
                if !hit {
                    runtime.point = end;
                }
                runtime.queries += 1;
                runtime.phantom_size = phantom_size;
                runtime.near_distance = near_distance;
                Ok(runtime.point)
            },
        );
        // No script callbacks run during collection. Commit the query view's
        // native retained fields and cleared mode30 even on a policy error.
        self.dynamic_runtime_filter = filter;
        self.brush_cursor = runtime;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_brush_cursor_ignores_player_shell_selects_npc_and_clears_stale_target() -> Result<()>
    {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        for tick in 1..=3 {
            world.advance_models(tick as f32 * super::super::STEP)?;
            world.advance_dynamic_physics(super::super::STEP)?;
        }
        let entity = world
            .entities
            .iter()
            .position(|e| e.name == "gremlinvillage_zonea.SpatterMelee 02")
            .context("Spatter absent")?;
        let body = world.character_physics[&entity]
            .as_ref()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let position = world.collision.simulation_pose(body.shape)?.position;
        let owner = [position[0] - 3., position[1], position[2]];
        let ray = cursor::Ray {
            origin: [owner[0], owner[1] + 0.15, owner[2]],
            direction: [1., 0., 0.],
        };
        for ink in [Ink::Paint, Ink::Thinner] {
            let point = world.brush_cursor_target(ray, ray.origin, owner, 6., ink)?;
            ensure!(
                world.brush_cursor.state.hit,
                "Original NPC was not selected: {point:?}"
            );
            assert_eq!(world.brush_cursor.state.entity, Some(entity));
            assert!(point[0] > owner[0] + 2. && point[0] < owner[0] + 4.);
            assert_eq!(world.brush_cursor.near_distance, 10.);
            assert_eq!(world.brush_cursor.phantom_size, 0.15);
            assert_eq!(world.dynamic_runtime_filter.mode_30, 0);
        }
        // An empty query resets the weak target but retains the last hit normal.
        let normal = world.brush_cursor.state.hit_normal;
        let owner = [1000.; 3];
        let ray = cursor::Ray {
            origin: owner,
            direction: [0., 1., 0.],
        };
        world.brush_cursor_target(ray, ray.origin, owner, 6., Ink::Paint)?;
        assert!(!world.brush_cursor.state.hit);
        assert_eq!(world.brush_cursor.state.entity, None);
        assert_eq!(world.brush_cursor.state.hit_normal, normal);
        let queries = world.brush_cursor.queries;
        world
            .runtime_settings
            .insert("MickeyAbilities_Brush_PhantomSize".into(), "NaN".into());
        assert!(world
            .brush_cursor_target(ray, ray.origin, owner, 6., Ink::Paint)
            .is_err());
        assert_eq!(world.brush_cursor.queries, queries);
        Ok(())
    }
}
