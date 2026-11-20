//! Native model-to-texture silhouette projector, separate from bicone shadows.
//! REL25d834 fits the camera to ordered caster spheres. REL25daa0 builds the
//! parallel receiver transform; the raster image itself uses a perspective
//! camera at distance100, with5percent padding. GPU integration is separate.
use super::{nif_camera::Frame, scene_bounds::Sphere};
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Projection {
    pub bound: Sphere,
    pub camera: Frame,
    pub receiver: [f32; 16],
}

fn unit(v: [f32; 3]) -> Result<[f32; 3]> {
    let (length, direction) = super::spherical_lighting::vector_length_and_direction(v);
    ensure!(
        length > 1e-6 && length.is_finite(),
        "Degenerate shadow projector basis"
    );
    Ok(direction)
}

/// Direction is already normalized by native25dc38. Preserve it: normalizing
/// again changes eye/frustum rounding. Near/far belong to the retained camera.
pub fn fit(
    casters: impl IntoIterator<Item = Sphere>,
    direction: [f32; 3],
    near: f32,
    far: f32,
) -> Result<Option<Projection>> {
    ensure!(
        direction.iter().all(|v| v.is_finite()),
        "Nonfinite shadow direction"
    );
    let mut casters = casters.into_iter();
    let Some(mut bound) = casters.next() else {
        return Ok(None);
    };
    // Unlike NiNode's child-bound update, this loop starts with the first
    // sphere unconditionally, then calls NiBound::Merge for each later caster.
    for caster in casters {
        bound = bound.merge(caster)?;
    }
    ensure!(
        bound.center.iter().all(|v| v.is_finite()) && bound.radius.is_finite() && bound.radius > 0.,
        "Invalid shadow caster bound"
    );
    let eye = std::array::from_fn(|i| bound.center[i] - direction[i] * 100.);
    let backward = unit(std::array::from_fn(|i| eye[i] - bound.center[i]))?;
    // Preserve native25d94c's auxiliary-axis choices and comparison/tie order.
    let [x, y, z] = direction.map(|v| v * v);
    let auxiliary = if x < y {
        if z >= x {
            [0., 0., 1.]
        } else {
            [1., 0., 0.]
        }
    } else if z >= y {
        [0., 1., 0.]
    } else {
        [0., 0., 1.]
    };
    let right = unit(super::decal_geometry::cross(auxiliary, backward))?;
    let up = unit(super::decal_geometry::cross(backward, right))?;
    let (distance, _) =
        super::spherical_lighting::vector_length_and_direction(std::array::from_fn(|i| {
            bound.center[i] - eye[i]
        }));
    ensure!(
        distance.is_finite() && distance > 0.,
        "Invalid shadow camera distance"
    );
    let half = (1_f32 + 0.05_f32) * (bound.radius / distance);
    let camera = Frame {
        eye,
        left: right.map(|v| -v),
        up,
        direction: backward.map(|v| -v),
        frustum: [-half, half, half, -half, near, far],
        orthographic: false,
        native_lens: None,
    };
    camera.projection()?;
    // Native NiTextureEffect is parallel/ST, not perspective/STQ. Native
    // texture rows are top-down; a GL framebuffer texture needs the Y flip.
    let model = model_projection(bound.radius)?;
    let effect = super::texture_effect::parallel(&camera_transform(&camera), &model, [0.; 4])?;
    let receiver = effect.framebuffer_receiver();
    Ok(Some(Projection {
        bound,
        camera,
        receiver,
    }))
}

pub fn camera_transform(camera: &Frame) -> super::native_transform_record::Transform {
    let c = camera;
    [
        c.left[0],
        c.up[0],
        c.direction[0],
        c.left[1],
        c.up[1],
        c.direction[1],
        c.left[2],
        c.up[2],
        c.direction[2],
        c.eye[0],
        c.eye[1],
        c.eye[2],
        1.,
    ]
}
pub fn model_projection(radius: f32) -> Result<[f32; 12]> {
    ensure!(
        radius.is_finite() && radius > 0.,
        "Invalid shadow projection radius"
    );
    let denominator = (2_f32 * (1_f32 + 0.05_f32)) * radius;
    let scale = 1_f32 / -denominator;
    Ok([scale, 0., 0., 0., scale, 0., 0., 0., 1., 0.5, 0.5, 0.])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_projector_model_projection_matches_bits() -> Result<()> {
        let r: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/player_projector_effect.json"
        ))?;
        let radius = r["effect_context"]["merged_bound"][3].as_f64().unwrap() as f32;
        let model = model_projection(radius)?;
        for (i, (a, b)) in model
            .into_iter()
            .zip(r["effect"]["model_projection"].as_array().unwrap())
            .enumerate()
        {
            assert_eq!(
                a.to_bits(),
                (b.as_f64().unwrap() as f32).to_bits(),
                "Model projection lane{i}"
            );
        }
        Ok(())
    }
    #[test]
    fn stopped_original_projector_camera_matches_bits() -> Result<()> {
        let r: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/player_projector_camera.json"
        ))?;
        let array = |value: &serde_json::Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
        };
        let spheres = r["casters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let b = array(&c["bound"]);
                Sphere {
                    center: b[..3].try_into().unwrap(),
                    radius: b[3],
                }
            })
            .collect::<Vec<_>>();
        let direction = array(&r["direction"]).try_into().unwrap();
        let f = array(&r["frustum_before"]);
        let p = fit(spheres, direction, f[4], f[5])?.unwrap();
        let expected = array(&r["merged_bound"]);
        let center: [f32; 3] = expected[..3].try_into().unwrap();
        assert_eq!(p.bound.center.map(f32::to_bits), center.map(f32::to_bits));
        assert_eq!(p.bound.radius.to_bits(), expected[3].to_bits());
        let c = p.camera;
        let matrix = [
            c.left[0],
            c.up[0],
            c.direction[0],
            c.left[1],
            c.up[1],
            c.direction[1],
            c.left[2],
            c.up[2],
            c.direction[2],
            c.eye[0],
            c.eye[1],
            c.eye[2],
            1.,
        ];
        for (i, (a, b)) in matrix
            .into_iter()
            .zip(array(&r["camera_world_transform"]))
            .enumerate()
        {
            assert_eq!(a.to_bits(), b.to_bits(), "Camera world lane {i}");
        }
        for (i, (a, b)) in c.frustum.into_iter().zip(array(&r["frustum"])).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "Frustum lane {i}");
        }
        Ok(())
    }
}
