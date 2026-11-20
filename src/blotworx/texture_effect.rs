//! EM1 NiTextureEffect WORLD_PARALLEL projection, DOL804ba610..804ba8b4.
//! Retain native transpose, single/FMA boundaries and plane-dot order. Other
//! coordinate modes are separate workers, not aliases for this calculation.
use super::{native_matrix as matrix, native_transform_record::Transform};
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Parallel {
    /// Native row-major 3x3 followed by translation, not a NiTransform.
    pub projection: [f32; 12],
    pub plane: [f32; 4],
}
pub fn parallel(world: &Transform, model: &[f32; 12], plane: [f32; 4]) -> Result<Parallel> {
    ensure!(
        world
            .iter()
            .chain(model)
            .chain(&plane)
            .all(|v| v.is_finite())
            && world[12] != 0.,
        "Invalid parallel texture-effect transform"
    );
    let inverse_scale = 1_f32 / world[12];
    let transpose = std::array::from_fn(|i| world[(i % 3) * 3 + i / 3]);
    let rotation =
        matrix::multiply(&model[..9].try_into().unwrap(), &transpose).map(|v| v * inverse_scale);
    let translated = matrix::transform(&rotation, world[9..12].try_into().unwrap());
    let translation: [f32; 3] = std::array::from_fn(|i| model[9 + i] - translated[i]);
    let normal = matrix::transform(
        &world[..9].try_into().unwrap(),
        plane[..3].try_into().unwrap(),
    );
    // SDK PSVECDotProduct805aea70: x*y accumulated over the rounded Y
    // product, then add the separately rounded Z product. Not the matrix dot.
    let dot = normal[0].mul_add(world[9], normal[1] * world[10]) + normal[2] * world[11];
    let constant = world[12].mul_add(plane[3], dot);
    let projection = std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else {
            translation[i - 9]
        }
    });
    ensure!(
        projection
            .iter()
            .chain(&normal)
            .chain([&constant])
            .all(|v| v.is_finite()),
        "Nonfinite parallel texture-effect output"
    );
    Ok(Parallel {
        projection,
        plane: [normal[0], normal[1], normal[2], constant],
    })
}

impl Parallel {
    /// Convert native ST rows to a GL bottom-up render-target lookup. Keep
    /// native arithmetic intact; apply the framebuffer origin change last.
    pub fn framebuffer_receiver(&self) -> [f32; 16] {
        let p = &self.projection;
        [
            p[0],
            -p[3],
            0.,
            0.,
            p[1],
            -p[4],
            0.,
            0.,
            p[2],
            -p[5],
            0.,
            0.,
            p[9],
            1. - p[10],
            0.,
            1.,
        ]
    }
}

/// REL17941c..179478 builds identity and replaces its first row with the
/// world-plane equation (constant4e51e0 is exactly1). The utility texture
/// supplies the band; there is no additional0.5 plane bias here.
pub fn clipping_matrix(plane: [f32; 4]) -> Result<[f32; 12]> {
    ensure!(
        plane.iter().all(|v| v.is_finite()),
        "Nonfinite texture-effect plane"
    );
    Ok([
        plane[0] * 1.,
        plane[1] * 1.,
        plane[2] * 1.,
        (-plane[3]) * 1.,
        0.,
        1.,
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
    ])
}

/// SDK PSMTXConcat805ae0f0. Row-major3x4, rounded X product, fused Y,
/// fused Z, then the native[0,1] pair for columns2/3. This dot order differs
/// from NiMatrix3's y,x,z order; do not share the latter's multiply helper.
pub fn concat(left: &[f32; 12], right: &[f32; 12]) -> Result<[f32; 12]> {
    ensure!(
        left.iter().chain(right).all(|v| v.is_finite()),
        "Nonfinite texture matrix"
    );
    let result = std::array::from_fn(|i| {
        let row = i / 4;
        let column = i % 4;
        let x = left[row * 4] * right[column];
        let xy = left[row * 4 + 1].mul_add(right[4 + column], x);
        let xyz = left[row * 4 + 2].mul_add(right[8 + column], xy);
        if column >= 2 {
            let w = if column == 2 { 0_f32 } else { 1_f32 };
            w.mul_add(left[row * 4 + 3], xyz)
        } else {
            xyz
        }
    });
    ensure!(
        result.iter().all(|v| v.is_finite()),
        "Nonfinite texture matrix product"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_projector_clip_matrix_matches_bits() -> Result<()> {
        let r: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/projector_texture_matrices.json"
        ))?;
        let array = |v: &serde_json::Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
        };
        for sample in r["cases"].as_array().unwrap() {
            let world = clipping_matrix(array(&sample["plane"]).try_into().unwrap())?;
            let local = concat(&world, &array(&sample["model_matrix"]).try_into().unwrap())?;
            for (name, values, key) in [
                ("world", world, "world_clip_matrix"),
                ("local", local, "local_clip_matrix"),
            ] {
                for (i, (a, b)) in values.into_iter().zip(array(&sample[key])).enumerate() {
                    assert_eq!(a.to_bits(), b.to_bits(), "{name} clip matrix lane{i}");
                }
            }
        }
        Ok(())
    }
    #[test]
    fn original_projector_effect_matches_every_float_bit() -> Result<()> {
        let r: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/player_projector_effect.json"
        ))?;
        let array = |v: &serde_json::Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect::<Vec<_>>()
        };
        let effect = &r["effect"];
        let p = parallel(
            &array(&effect["world_transform"]).try_into().unwrap(),
            &array(&effect["model_projection"]).try_into().unwrap(),
            array(&effect["model_plane"]).try_into().unwrap(),
        )?;
        for (i, (a, b)) in p
            .projection
            .into_iter()
            .zip(array(&effect["world_projection"]))
            .enumerate()
        {
            assert_eq!(a.to_bits(), b.to_bits(), "World projection lane{i}");
        }
        for (i, (a, b)) in p
            .plane
            .into_iter()
            .zip(array(&effect["world_plane"]))
            .enumerate()
        {
            assert_eq!(a.to_bits(), b.to_bits(), "World plane lane{i}");
        }
        let mut invalid = array(&effect["world_transform"]);
        invalid[12] = 0.;
        assert!(parallel(
            &invalid.try_into().unwrap(),
            &array(&effect["model_projection"]).try_into().unwrap(),
            p.plane
        )
        .is_err());
        Ok(())
    }
}
