//! Diagnostic geometric candidates from the actual presented world draws.
//! Alpha/discard/blending are deliberately not inferred by a collision ray.
use super::{
    assets::World,
    physics::{cross, dot, point, sub, V},
};
use crate::render_support::viewport3d::GpuDrawCommand;
/// Compact record of the exact post-animation data submitted for one entity.
/// Recomputed bounds expose stale or mismatched presentation metadata.
pub fn entity_geometry(world: &World, draws: &[GpuDrawCommand], name: &str) -> serde_json::Value {
    let Some((entity, runtime)) = world
        .entities
        .iter()
        .enumerate()
        .find(|(_, entity)| entity.name == name)
    else {
        return serde_json::json!({"entity":name,"error":"Entity not loaded"});
    };
    let mut records = Vec::new();
    for (&mesh, indices) in &runtime.mesh_draws {
        for &index in indices {
            let Some(draw) = draws.get(index) else {
                continue;
            };
            let mut local_min = [f32::INFINITY; 3];
            let mut local_max = [f32::NEG_INFINITY; 3];
            let mut world_min = local_min;
            let mut world_max = local_max;
            for vertex in draw.v_data.chunks_exact(12) {
                let local = [vertex[0], vertex[1], vertex[2]];
                let p = point(&draw.trans_mat, local);
                for axis in 0..3 {
                    local_min[axis] = local_min[axis].min(local[axis]);
                    local_max[axis] = local_max[axis].max(local[axis]);
                    world_min[axis] = world_min[axis].min(p[axis]);
                    world_max[axis] = world_max[axis].max(p[axis]);
                }
            }
            records.push(serde_json::json!({"draw":index,"mesh":mesh,"vertices":draw.v_data.len()/12,
                "indices":draw.indices.len(),"maximum_index":draw.indices.iter().max(),
                "transform":draw.trans_mat,"affine":super::render_transform::is_affine(&draw.trans_mat),"local_min":local_min,"local_max":local_max,
                "world_min":world_min,"world_max":world_max,"reported_min":draw.aabb_min,"reported_max":draw.aabb_max,
                "shader":draw.material.as_ref().and_then(|m|m.shader.as_deref()),
                "shader_error":draw.preview.as_ref().and_then(|p|p.as_ref().err()),
                "projection_bound":draw.projection_bound,
                "pinned_projection":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.pinned_projection),
                "required_images":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|&p.texture_slots),
                "bound_images":draw.material_images.iter().map(|image|(image.slot,image.width,image.height)).collect::<Vec<_>>(),
                "uv_channels":draw.extra_uvs.as_ref().map(|channels|channels.keys().copied().collect::<Vec<_>>()),
                "pass_constants":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.pass_constants),
                "registers":draw.material_registers,"hidden":world.hidden_entity_draws.contains(&index)
                    ||world.hidden_jigsaw_draws.contains(&index)||world.removed_draws.contains(&index)}));
        }
    }
    serde_json::json!({"entity":name,"runtime_index":entity,"entity_transform":runtime.transform,
        "entity_affine":super::render_transform::is_affine(&runtime.transform),"draws":records})
}
pub fn candidates(
    world: &World,
    draws: &[GpuDrawCommand],
    origin: V,
    direction: V,
) -> serde_json::Value {
    let mut hits = Vec::new();
    for (index, draw) in draws.iter().enumerate() {
        if world.hidden_jigsaw_draws.contains(&index)
            || world.hidden_entity_draws.contains(&index)
            || world.removed_draws.contains(&index)
        {
            continue;
        }
        let mut closest = f32::INFINITY;
        let mut face = None;
        for (triangle, indices) in draw.indices.chunks_exact(3).enumerate() {
            let get = |i: u32| -> Option<V> {
                let at = (i as usize).checked_mul(12)?;
                let v = draw.v_data.get(at..at + 3)?;
                Some(point(&draw.trans_mat, [v[0], v[1], v[2]]))
            };
            let (Some(a), Some(b), Some(c)) = (get(indices[0]), get(indices[1]), get(indices[2]))
            else {
                continue;
            };
            let e1 = sub(b, a);
            let e2 = sub(c, a);
            let p = cross(direction, e2);
            let determinant = dot(e1, p);
            if determinant == 0. {
                continue;
            }
            let inverse = 1. / determinant;
            let offset = sub(origin, a);
            let u = dot(offset, p) * inverse;
            if !(0.0..=1.).contains(&u) {
                continue;
            }
            let q = cross(offset, e1);
            let v = dot(direction, q) * inverse;
            if v < 0. || u + v > 1. {
                continue;
            }
            let distance = dot(e2, q) * inverse;
            if distance > 0. && distance < closest {
                closest = distance;
                face = Some(triangle);
            }
        }
        if let Some(triangle) = face {
            let owner = world
                .entities
                .iter()
                .enumerate()
                .find(|(_, entity)| entity.draws.contains(&index));
            let mesh = owner.and_then(|(_, entity)| {
                entity
                    .mesh_draws
                    .iter()
                    .find(|(_, indices)| indices.contains(&index))
                    .map(|(&mesh, _)| mesh)
            });
            hits.push(serde_json::json!({"draw":index,"distance":closest,"triangle":triangle,"mesh":mesh,
                "entity":owner.map(|(_,entity)|&entity.name),"asset":owner.and_then(|(_,entity)|entity.model_asset.as_ref()),
                "shader":draw.material.as_ref().and_then(|m|m.shader.as_deref()),
                "shader_error":draw.preview.as_ref().and_then(|p|p.as_ref().err()),
                "projection_bound":draw.projection_bound,
                "pinned_projection":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.pinned_projection),
                "required_images":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|&p.texture_slots),
                "bound_images":draw.material_images.iter().map(|image|(image.slot,image.width,image.height)).collect::<Vec<_>>(),
                "required_uv_images":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|&p.uv_textures),
                "uv_sets":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.uv_sets),
                "pass_constants":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.pass_constants),
                "textures":draw.material.as_ref().map(|m|m.textures.iter().map(|t|&t.path).collect::<Vec<_>>())}));
        }
    }
    hits.sort_by(|a, b| {
        a["distance"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["distance"].as_f64().unwrap())
    });
    serde_json::json!({"scope":"Post-animation visible-entity geometry; alpha/discard and GPU culling are not evaluated",
        "origin":origin,"direction":direction,"candidates":hits})
}

#[cfg(test)]
mod material_inventory_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 data in BLOTWORX_TEST_DATA"]
    fn original_material_states_and_projection_anchors() -> anyhow::Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut draws = world.draws.clone();
        world.apply_spherical_lighting(
            &mut draws,
            &[
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
        );
        let mut records = Vec::new();
        let mut unresolved = std::collections::BTreeMap::new();
        let mut native_assigned = 0;
        for (entity, runtime) in world.entities.iter().enumerate() {
            for (&mesh, indices) in &runtime.mesh_draws {
                for &index in indices {
                    let draw = &draws[index];
                    let Some(bindings) = &draw.material else {
                        continue;
                    };
                    if bindings
                        .shader
                        .as_deref()
                        .is_some_and(|s| !s.contains("Glint"))
                    {
                        continue;
                    }
                    if bindings.shader.is_none() && draw.preview.as_ref().is_some_and(|p| p.is_ok())
                    {
                        native_assigned += 1;
                    }
                    if draw.projection_bound.is_none()
                        && draw
                            .preview
                            .as_ref()
                            .and_then(|p| p.as_ref().ok())
                            .is_some_and(|p| p.pinned_projection)
                    {
                        if let Some(hierarchy) = world.entity_bound_hierarchy(entity)? {
                            unresolved
                                .entry(runtime.model_asset.clone().unwrap())
                                .or_insert(serde_json::to_value(hierarchy)?);
                        }
                    }
                    let native = bindings.shader.is_none().then(|| {
                        crate::render_support::wii_default::compile_particle(bindings)
                            .map(|p| p.fragment)
                            .map_err(|e| format!("{e:#}"))
                    });
                    records.push(serde_json::json!({"entity":entity,"name":runtime.name,"asset":runtime.model_asset,"mesh":mesh,"draw":index,
                        "material":format!("{bindings:#?}"),"native_unlit":native,"hidden":world.hidden_entities.contains(&entity)||world.hidden_entity_draws.contains(&index),
                        "shader":bindings.shader,"projection_bound":draw.projection_bound,
                        "pinned_projection":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|p.pinned_projection),
                        "required_images":draw.preview.as_ref().and_then(|p|p.as_ref().ok()).map(|p|&p.texture_slots),
                        "images":draw.material_images.iter().map(|i|(i.slot,i.width,i.height)).collect::<Vec<_>>(),
                        "shader_error":draw.preview.as_ref().and_then(|p|p.as_ref().err())}));
                }
            }
        }
        anyhow::ensure!(!records.is_empty(), "No original materials found");
        anyhow::ensure!(
            native_assigned > 0,
            "Original unlit mesh materials were not connected"
        );
        anyhow::ensure!(
            records
                .iter()
                .filter(|record| record["shader"] == "GEN_Glint")
                .all(|record| !record["projection_bound"].is_null()),
            "Original glint projection anchors remain missing"
        );
        std::fs::write(
            "target/original-material-inventory.json",
            serde_json::to_vec_pretty(&records)?,
        )?;
        std::fs::write(
            "target/original-projection-hierarchies.json",
            serde_json::to_vec_pretty(&unresolved)?,
        )?;
        Ok(())
    }
}
