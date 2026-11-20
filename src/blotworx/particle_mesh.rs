//! EM1 rigid face-surface emission, DOL804dbc6c/804dc6dc/804df288.
//! Source arrays retain authored triangle order, including hidden geometry.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
use std::sync::Arc;
type V = [f32; 3];
#[derive(Clone)]
pub(super) struct Surface {
    pub node: u32,
    pub transform: [f32; 16],
    positions: Arc<Vec<V>>,
    normals: Arc<Vec<V>>,
    indices: Arc<Vec<u32>>,
    strip: bool,
}
impl Surface {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"node":self.node,"vertices":self.positions.len(),
        "triangles":if self.strip {self.indices.len()-2}else{self.indices.len()/3},"triangle_strip":self.strip})
    }
    pub fn load(nif: &NifFile, node: u32) -> Result<Self> {
        let Some(NiObject::NiMesh(mesh)) = nif.blocks.get(node as usize) else {
            anyhow::bail!("Mesh emitter source {node} is not NiMesh")
        };
        ensure!(
            mesh.modifiers.iter().all(Option::is_none),
            "Skinned/modified particle mesh source {node} requires deformation support"
        );
        ensure!(
            mesh.submesh_count == 1 && matches!(mesh.primitive_type, 0 | 1),
            "Particle mesh source {node} needs native multi-submesh/primitive sampling"
        );
        ensure!(
            mesh.stream_refs
                .iter()
                .any(|r| r.semantics.iter().any(|s| s.name == "POSITION")),
            "Particle mesh source {node} has no authored POSITION stream"
        );
        ensure!(
            mesh.stream_refs
                .iter()
                .any(|r| r.semantics.iter().any(|s| s.name == "INDEX")),
            "Particle mesh source {node} has no index stream"
        );
        ensure!(
            mesh.stream_refs
                .iter()
                .all(|r| !r.semantics.iter().any(|s| s.name == "DISPLAYLIST")
                    || r.semantics.len() == 1),
            "Particle mesh source {node} mixes DISPLAYLIST with CPU semantics"
        );
        //805370d4..37154 explicitly locks INDEX for CPU face sampling.
        //Wii meshes may ALSO carry DISPLAYLIST for rendering. The renderer's
        //GX decoder must not reorder/weld the CPU emitter vertices/triangles.
        let mut cpu_mesh = mesh.clone();
        cpu_mesh
            .stream_refs
            .retain(|r| !r.semantics.iter().any(|s| s.name == "DISPLAYLIST"));
        let (geometry,_)=epic_mickey_lib_rs::nif_file::extract_mesh_with_source_indices(nif,&cpu_mesh)
            .context("Cannot decode authored particle mesh source CPU streams; preview geometry is not permitted")?;
        let normals = geometry
            .normals
            .context("Particle mesh source has no authored normals")?;
        let strip = mesh.primitive_type == 1;
        ensure!(
            geometry.indices.len() >= 3 && (strip || geometry.indices.len() % 3 == 0),
            "Invalid particle mesh triangle stream"
        );
        ensure!(
            normals.len() == geometry.positions.len()
                && geometry
                    .positions
                    .iter()
                    .chain(&normals)
                    .flatten()
                    .all(|v| v.is_finite()),
            "Invalid particle mesh positions/normals"
        );
        ensure!(
            geometry
                .indices
                .iter()
                .all(|&i| (i as usize) < normals.len()),
            "Particle mesh index outside vertex stream"
        );
        Ok(Self {
            node,
            transform: super::brush_render::node_transform(nif, node as usize)?,
            positions: Arc::new(geometry.positions),
            normals: Arc::new(normals),
            indices: Arc::new(geometry.indices),
            strip,
        })
    }
    pub fn sample(
        &self,
        variation: [f32; 2],
        speed: f32,
        emission: u32,
        mut random: impl FnMut() -> u32,
    ) -> Result<(V, V)> {
        ensure!(
            matches!(emission, 1 | 3),
            "Unsupported native face emission mode {emission}"
        );
        let count = if self.strip {
            self.indices.len() - 2
        } else {
            self.indices.len() / 3
        };
        let triangle = choose(count, &mut random)?;
        let indices = triangle_indices(&self.indices, self.strip, triangle);
        let vertices = std::array::from_fn(|i| self.positions[indices[i] as usize]);
        let normals = std::array::from_fn(|i| self.normals[indices[i] as usize]);
        // The retail face path perturbs normals before sampling the surface.
        let velocity = normal_velocity(
            normals,
            variation,
            speed,
            [unit(random())?, unit(random())?],
        );
        let position = if emission == 1 {
            face_center(vertices)
        } else {
            face_position(vertices, [unit(random())?, unit(random())?])
        };
        ensure!(
            position.iter().chain(&velocity).all(|v| v.is_finite()),
            "Invalid mesh-emitter sample"
        );
        Ok((position, velocity))
    }
}
fn triangle_indices(indices: &[u32], strip: bool, mut triangle: usize) -> [u32; 3] {
    let count = if strip {
        indices.len() - 2
    } else {
        indices.len() / 3
    };
    let mut result = [0; 3];
    // DOL804dee64..dee7c advances through at most SIX candidates, wrapping
    // without consuming more RNG. This is the retail bound, not a preview cap.
    for _ in 0..6 {
        let start = if strip { triangle } else { triangle * 3 };
        result = indices[start..start + 3].try_into().unwrap();
        if strip && triangle & 1 != 0 {
            result.swap(0, 1);
        }
        if result[0] != result[1] && result[0] != result[2] && result[1] != result[2] {
            break;
        }
        triangle = (triangle + 1) % count;
    }
    result
}
pub(super) fn unit(raw: u32) -> Result<f32> {
    ensure!(raw <= 32767, "Particle mesh RNG must be 15-bit");
    Ok(raw as f32 / 32767.)
}
pub(super) fn choose(count: usize, mut random: impl FnMut() -> u32) -> Result<usize> {
    ensure!(count > 0, "No mesh-emitter choices");
    let index = (count as f32 * unit(random())?) as usize;
    Ok(if index == count { count - 1 } else { index })
}
fn squared(v: V) -> f32 {
    v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1]
}
fn inverse_length(s: f32) -> f32 {
    let r = super::pose_blend::estimate_double(s);
    let bits = r.to_bits();
    let rounded = f64::from_bits(((bits >> 28) + ((bits >> 27) & 1)) << 28);
    let square = (r * rounded) as f32;
    let half = (r * 0.5) as f32;
    (-s).mul_add(square, 3.) * half
}
fn normalize(v: V) -> V {
    let inverse = inverse_length(squared(v));
    v.map(|x| x * inverse)
}
fn length(v: V) -> f32 {
    let s = squared(v);
    if s == 0. {
        0.
    } else {
        s * inverse_length(s)
    }
}
fn normal_velocity(normals: [V; 3], variation: [f32; 2], speed: f32, samples: [f32; 2]) -> V {
    let mean = std::array::from_fn(|i| {
        ((normals[0][i] + normals[1][i]) + normals[2][i]) * f32::from_bits(0x3eaaaaab)
    });
    let mut direction = if squared(mean) > f32::from_bits(0x358637bd) {
        normalize(mean)
    } else {
        [0.; 3]
    };
    direction[0] = variation[0].mul_add(2. * samples[0] - 1., direction[0]);
    direction[2] = variation[1].mul_add(2. * samples[1] - 1., direction[2]);
    let direction = if length(direction) > f32::from_bits(0x358637bd) {
        normalize(direction)
    } else {
        [0.; 3]
    };
    direction.map(|v| v * speed)
}
fn face_position(vertices: [V; 3], samples: [f32; 2]) -> V {
    let value = samples[0];
    // NiFastSqrt: frsp estimate followed by two single Newton steps.
    let root = if value < f32::from_bits(0x012355e6) {
        0.
    } else {
        let mut r = super::pose_blend::estimate(value);
        let half = 0.5 * value;
        for _ in 0..2 {
            r *= (-r).mul_add(half * r, 1.5);
        }
        value * r
    };
    std::array::from_fn(|i| {
        let d1 = vertices[1][i] - vertices[0][i];
        let d2 = vertices[2][i] - vertices[0][i];
        (vertices[0][i] + root * (samples[1] * d2 - d1)) + d1
    })
}
///804dc960..804dc998, including the original three vector helpers.
fn face_center(vertices: [V; 3]) -> V {
    std::array::from_fn(|i| {
        ((vertices[0][i] + vertices[1][i]) + vertices[2][i]) * f32::from_bits(0x3eaaaaab)
    })
}
#[cfg(any(test, debug_assertions))]
pub fn validate_original_source_streams() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
        std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak"),
    )?);
    let file = pak
        .files
        .iter()
        .find(|f| {
            f.path
                .eq_ignore_ascii_case("Effects/_Shared/fx_Hopper_Knockback_Dust.nif_wii")
        })
        .context("Original CPU source mesh resource absent")?;
    let load = || NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg);
    let original = load()?;
    let source = Surface::load(&original, 42)?;
    ensure!(
        source.positions.len() == 42 && source.strip && source.indices.len() - 2 == 118,
        "Original CPU mesh topology changed"
    );
    for (kind, expected) in [
        (0, "no authored POSITION"),
        (1, "no index stream"),
        (2, "mixes DISPLAYLIST"),
    ] {
        let mut malformed = load()?;
        let Some(NiObject::NiMesh(mesh)) = malformed.blocks.get_mut(42) else {
            anyhow::bail!("Original source mesh absent")
        };
        let position = mesh
            .stream_refs
            .iter()
            .flat_map(|r| &r.semantics)
            .find(|s| s.name == "POSITION")
            .context("Source POSITION semantic absent")?
            .clone();
        match kind {
            0 => {
                for r in &mut mesh.stream_refs {
                    r.semantics.retain(|s| s.name != "POSITION");
                }
            }
            1 => {
                for r in &mut mesh.stream_refs {
                    r.semantics.retain(|s| s.name != "INDEX");
                }
            }
            _ => mesh
                .stream_refs
                .iter_mut()
                .find(|r| r.semantics.iter().any(|s| s.name == "DISPLAYLIST"))
                .context("Original DISPLAYLIST absent")?
                .semantics
                .push(position),
        }
        let error = Surface::load(&malformed, 42)
            .err()
            .context("Malformed particle source produced geometry")?;
        ensure!(
            error.to_string().contains(expected),
            "Unexpected malformed source diagnostic: {error:#}"
        );
    }
    println!("PASS: original CPU emitter mesh retained; missing POSITION/INDEX and mixed GPU semantics rejected without preview geometry");
    Ok(())
}
#[cfg(any(test, debug_assertions))]
pub fn validate_center() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/particle_mesh_center.json"
    ))?;
    let cases = fixture["cases"]
        .as_array()
        .context("Missing original face-center cases")?;
    ensure!(cases.len() == 18, "Incomplete original face-center corpus");
    for row in cases {
        let vertices: [V; 3] = serde_json::from_value(row["vertices"].clone())?;
        let normals: [V; 3] = serde_json::from_value(row["normals"].clone())?;
        let variation: [f32; 2] = serde_json::from_value(row["variation"].clone())?;
        let samples: [u32; 2] = serde_json::from_value(row["samples"].clone())?;
        let position = face_center(vertices);
        let velocity = normal_velocity(
            normals,
            variation,
            row["speed"].as_f64().context("Missing source speed")? as f32,
            [unit(samples[0])?, unit(samples[1])?],
        );
        ensure!(
            serde_json::json!(position.map(f32::to_bits)) == row["position_bits"],
            "Native face-center position mismatch"
        );
        ensure!(
            serde_json::json!(velocity.map(f32::to_bits)) == row["velocity_bits"],
            "Native face-center velocity mismatch"
        );
    }
    let surface = Surface {
        node: 0,
        transform: [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
        positions: Arc::new(vec![[0., 0., 0.], [3., 0., 0.], [0., 3., 0.]]),
        normals: Arc::new(vec![[0., 0., 1.]; 3]),
        indices: Arc::new(vec![0, 1, 2]),
        strip: false,
    };
    for (mode, expected) in [(1, 3), (3, 5)] {
        let mut calls = 0;
        let result = surface.sample([0.; 2], 1., mode, || {
            calls += 1;
            16384
        })?;
        ensure!(
            calls == expected,
            "Face emission consumed the wrong number of random draws"
        );
        if mode == 1 {
            ensure!(
                result.0 == [1., 1., 0.],
                "Face-center sampling used a random surface point"
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_face_arithmetic_matches_retail_instruction_cases() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_mesh_reference.json"
        ))?;
        for row in fixture["cases"].as_array().unwrap() {
            let vertices: [V; 3] = serde_json::from_value(row["vertices"].clone())?;
            let normals: [V; 3] = serde_json::from_value(row["normals"].clone())?;
            let variation: [f32; 2] = serde_json::from_value(row["variation"].clone())?;
            let samples: [u32; 4] = serde_json::from_value(row["samples"].clone())?;
            let velocity = normal_velocity(
                normals,
                variation,
                row["speed"].as_f64().unwrap() as f32,
                [unit(samples[0])?, unit(samples[1])?],
            );
            let position = face_position(vertices, [unit(samples[2])?, unit(samples[3])?]);
            assert_eq!(
                serde_json::json!(position.map(f32::to_bits)),
                row["position_bits"],
                "Position: {row}"
            );
            assert_eq!(
                serde_json::json!(velocity.map(f32::to_bits)),
                row["velocity_bits"],
                "Velocity: {row}"
            );
        }
        Ok(())
    }
    #[test]
    fn endpoint_choice_is_clamped_without_resampling() -> Result<()> {
        assert_eq!(choose(3, || 32767)?, 2);
        assert_eq!(choose(3, || 0)?, 0);
        assert!(unit(32768).is_err());
        assert!(choose(0, || panic!()).is_err());
        Ok(())
    }
    #[test]
    fn strips_preserve_winding_degenerates_and_retail_scan_bound() {
        assert_eq!(triangle_indices(&[4, 5, 6, 7], true, 1), [6, 5, 7]);
        assert_eq!(triangle_indices(&[0, 0, 1, 1, 2, 3], true, 0), [2, 1, 3]);
        assert_eq!(
            triangle_indices(&[0, 0, 0, 0, 0, 0, 0, 0, 1, 2], true, 0),
            [0, 0, 0]
        );
        assert_eq!(triangle_indices(&[0, 0, 1, 2, 3, 4], false, 0), [2, 3, 4]);
    }
}
