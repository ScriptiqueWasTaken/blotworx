//! Camera-dependent NiBillboardNode presentation using the Gamebryo 2.6
//! face modes. EM1 instruction/FPSCR and culling-phase parity remain unverified.
//! Never write camera-facing transforms into simulation/serialized objects.
use super::native_matrix::{Matrix, multiply, transform};
use anyhow::{Context, Result, ensure};
type V = [f32; 3];
fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn transpose(r: Matrix) -> Matrix {
    std::array::from_fn(|i| r[(i % 3) * 3 + i / 3])
}
fn columns(x: V, y: V, z: V) -> Matrix {
    [x[0], y[0], z[0], x[1], y[1], z[1], x[2], y[2], z[2]]
}
/// SDK face modes0..4; the input is the freshly propagated world rotation.
/// Return identity effect at the original center/up-axis degeneracies.
fn face(
    mode: u16,
    rotation: Matrix,
    position: V,
    scale: f32,
    eye: V,
    right: V,
    up: V,
) -> Result<Matrix> {
    ensure!(mode <= 4, "Unsupported NiBillboardNode mode {mode}");
    ensure!(
        rotation
            .iter()
            .chain(position.iter())
            .chain(eye.iter())
            .chain(right.iter())
            .chain(up.iter())
            .all(|v| v.is_finite()),
        "Nonfinite billboard transform/camera"
    );
    let inverse = transpose(rotation);
    if mode == 1 {
        ensure!(scale != 0., "Singular billboard scale");
        let local =
            transform(&inverse, std::array::from_fn(|i| eye[i] - position[i])).map(|x| x / scale);
        let length = (local[0] * local[0] + local[2] * local[2]).sqrt();
        if length < 1e-12 {
            return Ok(rotation);
        }
        let x = local[0] / length;
        let z = local[2] / length;
        return Ok(multiply(&rotation, &[z, 0., x, 0., 1., 0., -x, 0., z]));
    }
    let mut r = right;
    let mut u = up;
    let mut d = cross(right, up);
    if mode >= 3 {
        let at = std::array::from_fn(|i| eye[i] - position[i]);
        let squared = dot(at, at);
        if squared < 0.001 {
            return Ok(rotation);
        }
        let at = at.map(|x| x / squared.sqrt());
        let cosine = dot(at, d);
        if cosine < 0.999999 {
            let axis = cross(at, d);
            let length = dot(axis, axis).sqrt();
            let axis = if length > 1e-6 {
                axis.map(|x| x / length)
            } else {
                [0.; 3]
            };
            let angle = if cosine > -1. {
                if cosine < 1. {
                    (cosine as f64).acos() as f32
                } else {
                    0.
                }
            } else {
                std::f32::consts::PI
            };
            let (sin, cos) = super::native_angle::sin_cos_medium(angle as f64)?;
            let turn = super::native_matrix::rotation(axis, sin as f32, cos as f32);
            r = transform(&turn, r);
            u = transform(&turn, u);
            d = at;
        }
    }
    let r = transform(&inverse, r);
    let u = transform(&inverse, u);
    let d = transform(&inverse, d);
    let facing = if mode == 0 || mode == 3 {
        let length = (r[1] * r[1] + u[1] * u[1]).sqrt();
        if length > 1e-6 {
            let c = u[1] / length;
            let s = -r[1] / length;
            columns(
                std::array::from_fn(|i| c * r[i] + s * u[i]),
                std::array::from_fn(|i| -s * r[i] + c * u[i]),
                d,
            )
        } else {
            columns(r.map(|v| -v), u.map(|v| -v), d)
        }
    } else {
        columns(r, u, d)
    };
    Ok(multiply(&rotation, &facing))
}
fn record_matrix(record: &super::native_transform_record::Transform) -> [f32; 16] {
    std::array::from_fn(|i| {
        let row = i % 4;
        let col = i / 4;
        if row < 3 && col < 3 {
            record[row * 3 + col] * record[12]
        } else if col == 3 && row < 3 {
            record[9 + row]
        } else if i == 15 {
            1.
        } else {
            0.
        }
    })
}
pub(super) fn camera_delta(
    mode: u16,
    before: [f32; 16],
    scale: f32,
    eye: V,
    right: V,
    up: V,
) -> Result<[f32; 16]> {
    ensure!(
        scale.is_finite() && scale != 0.,
        "Invalid billboard world scale"
    );
    let mut record = [0.; 13];
    for row in 0..3 {
        for col in 0..3 {
            record[row * 3 + col] = before[col * 4 + row] / scale;
        }
    }
    record[9..12].copy_from_slice(&before[12..15]);
    record[12] = scale;
    let rotation = record[..9].try_into()?;
    let after = face(
        mode,
        rotation,
        before[12..15].try_into()?,
        scale,
        eye,
        right,
        up,
    )?;
    let mut changed = record;
    changed[..9].copy_from_slice(&after);
    Ok(super::skin::matrix_product(
        &record_matrix(&changed),
        &record_matrix(&super::native_transform_record::inverse(&record)),
    ))
}
impl super::assets::World {
    /// Runs after animated model poses and before lighting/culling/submission.
    /// Accumulated ancestor deltas preserve mesh offsets and nested billboards.
    pub(super) fn apply_model_camera_transforms(
        &self,
        draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
        eye: V,
        right: V,
        up: V,
    ) -> Result<usize> {
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let mut count = 0;
        for (owner, entity) in self.entities.iter().enumerate() {
            if !self.entity_scene_active(owner) || self.removed_entities.contains(&owner) {
                continue;
            }
            let mut parent = entity.transform;
            if entity
                .components
                .iter()
                .any(|c| c.class_name == "SkyboxComponent")
            {
                for index in entity.draws.clone() {
                    for axis in 0..3 {
                        draws[index].trans_mat[12 + axis] +=
                            eye[axis] - entity.transform[12 + axis];
                    }
                }
                parent[12..15].copy_from_slice(&eye);
            }
            let Some(graph) = entity
                .model_asset
                .as_ref()
                .and_then(|key| self.model_bounds.get(key))
            else {
                continue;
            };
            if !graph.nodes.values().any(|n| n.billboard_mode.is_some()) {
                continue;
            }
            let base;
            let frames =
                if let Some(actor) = self.actor_playback.as_ref().filter(|a| a.entity == owner) {
                    &actor.world
                } else if let Some(actor) = self.actors.get(&owner).filter(|a| a.pose.is_some()) {
                    &actor.world
                } else if let Some(animation) = self.model_animation.get(&owner) {
                    &animation.world
                } else {
                    base = graph.base_world_transforms(None)?;
                    &base
                };
            let mut pending: Vec<_> = graph
                .roots
                .iter()
                .rev()
                .flatten()
                .map(|&id| (id, identity, false))
                .collect();
            while let Some((id, mut delta, mut affected)) = pending.pop() {
                let node = &graph.nodes[&id];
                if let Some(mode) = node.billboard_mode {
                    let world = frames
                        .get(&id)
                        .context("Billboard animated world transform missing")?;
                    let before = super::skin::matrix_product(
                        &delta,
                        &super::skin::matrix_product(&parent, &super::nif_runtime::matrix(world)),
                    );
                    let local = camera_delta(
                        mode,
                        before,
                        entity.authored_scale * world.scale,
                        eye,
                        right,
                        up,
                    )?;
                    delta = super::skin::matrix_product(&local, &delta);
                    affected = true;
                    count += 1;
                }
                if affected {
                    if let Some(indices) = entity.mesh_draws.get(&id) {
                        for &index in indices {
                            draws[index].trans_mat =
                                super::skin::matrix_product(&delta, &draws[index].trans_mat);
                        }
                    }
                }
                pending.extend(
                    node.children
                        .iter()
                        .rev()
                        .flatten()
                        .map(|&child| (child, delta, affected)),
                );
            }
        }
        Ok(count)
    }
}
#[cfg(debug_assertions)]
pub(super) fn validate() -> Result<()> {
    let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    let r = face(
        1,
        identity,
        [0.; 3],
        1.,
        [10., 3., 0.],
        [1., 0., 0.],
        [0., 1., 0.],
    )?;
    ensure!(
        (r[2] - 1.).abs() < 1e-6 && r[5] == 0. && r[8].abs() < 1e-6,
        "Upright billboard not facing eye"
    );
    for mode in [0, 2] {
        let r = face(
            mode,
            identity,
            [0.; 3],
            1.,
            [0., 0., 10.],
            [0., 1., 0.],
            [-1., 0., 0.],
        )?;
        ensure!(
            r.iter().all(|x| x.is_finite()),
            "Invalid camera facing frame"
        );
    }
    for mode in [3, 4] {
        let r = face(
            mode,
            identity,
            [0.; 3],
            1.,
            [10., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
        )?;
        ensure!(
            (r[2] - 1.).abs() < 1e-5 && r[5].abs() < 1e-5 && r[8].abs() < 1e-5,
            "Center-facing mode did not point at camera"
        );
        ensure!(
            face(
                mode,
                identity,
                [0.; 3],
                1.,
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.]
            )? == identity,
            "Near-center SDK guard changed pose"
        );
    }
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let world = super::assets::load(
        std::path::Path::new(&root),
        "Levels/GV_ZoneA_Start.level",
        &egui::Context::default(),
    )?;
    let mut a = world.draws.clone();
    world.apply_model_poses(&mut a)?;
    let original: Vec<_> = a.iter().map(|d| d.trans_mat).collect();
    let count =
        world.apply_model_camera_transforms(&mut a, [25., 10., 15.], [1., 0., 0.], [0., 1., 0.])?;
    ensure!(count > 0, "No original scene billboards parsed");
    let changed = a
        .iter()
        .zip(&original)
        .filter(|(d, m)| d.trans_mat != **m)
        .count();
    ensure!(changed > 0, "Billboards never reached draws");
    let mut b = world.draws.clone();
    world.apply_model_poses(&mut b)?;
    world.apply_model_camera_transforms(&mut b, [25., 10., 15.], [1., 0., 0.], [0., 1., 0.])?;
    ensure!(
        a.iter().zip(&b).all(|(a, b)| a.trans_mat == b.trans_mat),
        "Camera-facing transforms accumulated between presentations"
    );
    println!(
        "PASS: {count} original GV billboard nodes connected, {changed} camera-dependent draws; repeat presentation stable (SDK behavior, EM1 arithmetic parity unverified)"
    );
    Ok(())
}
