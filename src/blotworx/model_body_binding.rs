//! Named rigid-body/model-node binding shared by source admission and render
//! writeback. EM2 Initialize selects a model node before trying animation bones.
//! Unresolved animation fallback stays explicit; no body is picked arbitrarily.
use super::{scene_bounds::Hierarchy, scene_static_collision::Owner};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    nif_file::{NiTransform, NifFile},
    packfile::VirtualFile,
    scene_file::SceneFile,
};
use std::{collections::HashMap, sync::Arc};

pub struct DrawSource {
    pub scene: usize,
    pub entity: u32,
    pub path: Option<Vec<u32>>,
    pub original_transform: [f32; 16],
}
pub struct Body {
    pub source: usize,
    pub node: Option<u32>,
    pub initial_world: [f32; 16],
    pub entity_scale: f32,
}
struct Attachment {
    draw: usize,
    binding: usize,
    original: [f32; 16],
}
pub struct State {
    pub bodies: Vec<Body>,
    attachments: Vec<Attachment>,
    pub report: serde_json::Value,
}
fn key(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}
fn transform(matrix: &[f32; 16]) -> Result<NiTransform> {
    use epic_mickey_lib_rs::nif_file::{Matrix3, Vector3};
    ensure!(
        matrix.iter().all(|v| v.is_finite()),
        "Nonfinite model owner transform"
    );
    let scales: [f32; 3] = std::array::from_fn(|c| {
        (0..3)
            .map(|r| matrix[c * 4 + r].powi(2))
            .sum::<f32>()
            .sqrt()
    });
    ensure!(
        scales[0] > 0.
            && scales
                .iter()
                .all(|s| (*s - scales[0]).abs() <= scales[0] * 1e-5),
        "Nonuniform or zero model owner scale needs physics compatibility"
    );
    let determinant = matrix[0] * (matrix[5] * matrix[10] - matrix[9] * matrix[6])
        - matrix[4] * (matrix[1] * matrix[10] - matrix[9] * matrix[2])
        + matrix[8] * (matrix[1] * matrix[6] - matrix[5] * matrix[2]);
    let scale = if determinant < 0. {
        -scales[0]
    } else {
        scales[0]
    };
    Ok(NiTransform {
        rotation: Matrix3 {
            rows: std::array::from_fn(|r| std::array::from_fn(|c| matrix[c * 4 + r] / scale)),
        },
        translation: Vector3 {
            x: matrix[12],
            y: matrix[13],
            z: matrix[14],
        },
        scale,
    })
}
fn rigid(frame: &NiTransform) -> [f32; 16] {
    let mut out = [0.; 16];
    for c in 0..3 {
        for r in 0..3 {
            out[c * 4 + r] = frame.rotation.rows[r][c];
        }
    }
    out[12..15].copy_from_slice(&[
        frame.translation.x,
        frame.translation.y,
        frame.translation.z,
    ]);
    out[15] = 1.;
    out
}
/// Rigid pose delta, retaining model scale in the original draw transform.
pub(super) fn delta(initial: &[f32; 16], current: &[f32; 16]) -> Result<[f32; 16]> {
    ensure!(
        initial.iter().chain(current).all(|v| v.is_finite()),
        "Nonfinite model body pose"
    );
    let mut inverse = [0.; 16];
    inverse[15] = 1.;
    for c in 0..3 {
        for r in 0..3 {
            inverse[c * 4 + r] = initial[r * 4 + c];
        }
    }
    for r in 0..3 {
        inverse[12 + r] = -(0..3)
            .map(|c| inverse[c * 4 + r] * initial[12 + c])
            .sum::<f32>();
    }
    Ok(super::havok_2012_geometry::compose(current, &inverse))
}
impl State {
    pub fn prepare(
        owners: &[Owner],
        scenes: &[SceneFile],
        files: &[VirtualFile],
        draws: &[DrawSource],
    ) -> Self {
        let catalog = files
            .iter()
            .map(|file| (key(&file.path), file))
            .collect::<HashMap<_, _>>();
        let mut cache = HashMap::<String, Result<Arc<Hierarchy>, String>>::new();
        let mut bodies = Vec::new();
        let mut rows = Vec::new();
        let mut errors = Vec::new();
        for (source, owner) in owners.iter().enumerate() {
            let result = (|| -> Result<Body> {
                let scene = scenes
                    .get(owner.scene)
                    .context("Body scene identity missing")?;
                let entity = scene
                    .objects
                    .iter()
                    .find(|e| e.link_id.to_u32() == owner.entity_link)
                    .context("Body entity identity missing")?;
                let parent = transform(&owner.entity_transform)?;
                let path =
                    crate::render_support::scene::find_entity_nif_path_with_master(entity, &scene.objects);
                let mut node = None;
                let mut bound_frame = None;
                if let Some(path) = path {
                    let graph = cache.entry(key(&path)).or_insert_with(|| {
                        (|| -> Result<_> {
                            let file = catalog
                                .get(&key(&path))
                                .with_context(|| format!("Missing binding model {path}"))?;
                            let nif =
                                NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
                            Ok(Arc::new(Hierarchy::load(&nif)?))
                        })()
                        .map_err(|e| format!("{e:#}"))
                    });
                    let graph = graph.as_ref().map_err(|e| anyhow::anyhow!(e.clone()))?;
                    if let Some(name) = owner.resource[owner.body]
                        .name
                        .as_deref()
                        .filter(|s| !s.is_empty())
                    {
                        for root in graph.roots.iter().flatten() {
                            if let Some(found) = graph.find_named_node(*root, name)? {
                                node = Some(found);
                                break;
                            }
                        }
                        if let Some(node) = node {
                            let frames = graph.base_world_transforms(Some(&parent))?;
                            bound_frame = Some(rigid(
                                frames.get(&node).context("Bound model frame missing")?,
                            ));
                        }
                    }
                }
                if node.is_none()
                    && owner.resource[owner.body]
                        .name
                        .as_deref()
                        .is_some_and(|s| !s.is_empty())
                {
                    let components =
                        crate::render_support::scene::get_effective_components(entity, &scene.objects);
                    ensure!(
                        !components
                            .iter()
                            .any(|c| c.class_name == "AnimationComponent"),
                        "Named body needs animation-bone fallback resolution"
                    );
                }
                let initial_world = bound_frame.unwrap_or_else(|| {
                    // Serialized body translation scales with its entity; rotation does not.
                    let full = super::havok_2012_geometry::compose(
                        &owner.entity_transform,
                        &owner.resource[owner.body].transform,
                    );
                    let mut rigid = full;
                    for c in 0..3 {
                        for r in 0..3 {
                            rigid[c * 4 + r] /= parent.scale;
                        }
                    }
                    rigid
                });
                Ok(Body {
                    source,
                    node,
                    initial_world,
                    entity_scale: parent.scale,
                })
            })();
            match result {
                Ok(body)=>{rows.push(serde_json::json!({"entity":owner.entity,"body":owner.resource[owner.body].name,
                    "target_node":body.node,"target_kind":if body.node.is_some(){"model node"}else{"entity"},
                    "initial_world":body.initial_world,"start_hidden":owner.start_hidden}));bodies.push(body);},
                Err(error)=>errors.push(serde_json::json!({"entity":owner.entity,"body":owner.resource[owner.body].name,"error":format!("{error:#}")})),
            }
        }
        let mut attachments = Vec::new();
        for (draw, mesh) in draws.iter().enumerate() {
            let mut candidates = Vec::new();
            for (index, body) in bodies.iter().enumerate() {
                let owner = &owners[body.source];
                if owner.scene != mesh.scene || owner.entity_link != mesh.entity {
                    continue;
                }
                if let Some(node) = body.node {
                    if let Some(depth) = mesh
                        .path
                        .as_ref()
                        .and_then(|p| p.iter().position(|&id| id == node))
                    {
                        candidates.push((depth + 1, index));
                    }
                } else {
                    candidates.push((0, index));
                }
            }
            candidates.sort_by_key(|&(depth, _)| depth);
            if let Some(&(depth, binding)) = candidates.last() {
                if candidates.iter().filter(|(d, _)| *d == depth).count() != 1 {
                    errors.push(serde_json::json!({"draw":draw,"error":"Several bodies bind the same render subtree"}));
                    continue;
                }
                attachments.push(Attachment {
                    draw,
                    binding,
                    original: mesh.original_transform,
                });
            }
        }
        let report = serde_json::json!({"bodies":rows,"draw_attachments":attachments.len(),"errors":errors,
            "simulation_writeback_active":false,"original_transform_arithmetic_parity_verified":false});
        Self {
            bodies,
            attachments,
            report,
        }
    }
    /// Publish absolute body poses. The deepest bound ancestor owns a draw;
    /// this handles independent children without applying parent motion twice.
    pub fn apply(
        &self,
        poses: &HashMap<usize, [f32; 16]>,
        draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
    ) -> Result<()> {
        ensure!(
            poses
                .keys()
                .all(|source| self.bodies.iter().any(|body| body.source == *source)),
            "Pose targets an unresolved model body"
        );
        let mut updates = Vec::new();
        for attachment in &self.attachments {
            let body = &self.bodies[attachment.binding];
            let Some(current) = poses.get(&body.source) else {
                continue;
            };
            ensure!(attachment.draw < draws.len(), "Bound render draw missing");
            let matrix = super::havok_2012_geometry::compose(
                &delta(&body.initial_world, current)?,
                &attachment.original,
            );
            ensure!(
                matrix.iter().all(|v| v.is_finite()),
                "Model body writeback overflow"
            );
            updates.push((attachment.draw, matrix));
        }
        for (draw, matrix) in updates {
            draws[draw].trans_mat = matrix;
        }
        Ok(())
    }
    pub fn debug_lines(&self, owners: &[Owner]) -> Vec<crate::render_support::viewport3d::GpuLineSegment> {
        use crate::render_support::viewport3d::GpuLineSegment;
        let mut lines = Vec::new();
        for body in &self.bodies {
            let owner = &owners[body.source];
            if owner.start_hidden {
                continue;
            }
            for part in &owner.resource[owner.body].parts {
                let points = part
                    .vertices
                    .iter()
                    .map(|p| {
                        super::havok_2012_geometry::point(
                            &body.initial_world,
                            p.map(|v| v * body.entity_scale),
                        )
                    })
                    .collect::<Vec<_>>();
                if part.convex {
                    for p in points {
                        for axis in 0..3 {
                            let mut a = p;
                            let mut b = p;
                            a[axis] -= 0.02;
                            b[axis] += 0.02;
                            lines.push(GpuLineSegment {
                                start: a,
                                end: b,
                                color: [0.95, 0.2, 0.85, 0.8],
                            });
                        }
                    }
                } else {
                    for ids in part.indices.chunks_exact(3) {
                        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                            lines.push(GpuLineSegment {
                                start: points[ids[a] as usize],
                                end: points[ids[b] as usize],
                                color: [0.95, 0.2, 0.85, 0.6],
                            });
                        }
                    }
                }
            }
        }
        lines
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_uniform_owner_scale_stays_out_of_rigid_rotation() -> Result<()> {
        let matrix = [
            -2., 0., 0., 0., 0., -2., 0., 0., 0., 0., -2., 0., 3., 4., 5., 1.,
        ];
        let frame = transform(&matrix)?;
        assert_eq!(frame.scale, -2.);
        assert_eq!(
            rigid(&frame),
            [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 3., 4., 5., 1.]
        );
        let mut invalid = matrix;
        invalid[0] = -3.;
        assert!(transform(&invalid).is_err());
        Ok(())
    }
    #[test]
    fn body_delta_keeps_model_scale_and_pivots_separate() -> Result<()> {
        let initial = [
            0., 0., -1., 0., 0., 1., 0., 0., 1., 0., 0., 0., 4., 5., 6., 1.,
        ];
        let mut current = initial;
        current[12] += 3.;
        current[14] -= 2.;
        let change = delta(&initial, &current)?;
        let model = [
            2., 0., 0., 0., 0., 2., 0., 0., 0., 0., 2., 0., 10., 20., 30., 1.,
        ];
        let moved = super::super::havok_2012_geometry::compose(&change, &model);
        assert_eq!(&moved[..12], &model[..12]);
        assert_eq!(&moved[12..], [13., 20., 28., 1.]);
        assert!(delta(&initial, &[f32::NAN; 16]).is_err());
        Ok(())
    }
}
