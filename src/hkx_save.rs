use anyhow::{Context, Result};
use epic_mickey_lib_rs::hkx_file::{HkxBone, HkxFile};

/// Havok 7's 32-bit simple mesh has a 24-byte shape base. The generic
/// library decoder assumes an older 8-byte base and misses these arrays.
pub fn collision_meshes(file: &HkxFile) -> Vec<epic_mickey_lib_rs::hkx_file::HkxCollisionMesh> {
    let mut meshes = Vec::new();
    let mut has_convex = false;
    if file.header.pointer_size == 4 && file.header.contents_version.starts_with("Havok-7.") {
        for (section, fixups) in file.virtual_fixups.iter().enumerate() {
            let Some(data) = file.section_data.get(section) else {
                continue;
            };
            let Some(locals) = file.local_fixups.get(section) else {
                continue;
            };
            let read = |offset: usize| -> Option<u32> {
                let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
                Some(if file.header.endianness == 0 {
                    u32::from_be_bytes(bytes)
                } else {
                    u32::from_le_bytes(bytes)
                })
            };
            for fixup in fixups {
                let name = file
                    .section_data
                    .get(fixup.section_index as usize)
                    .and_then(|b| b.get(fixup.class_name_offset as usize..))
                    .and_then(|b| b.split(|v| *v == 0).next());
                if name == Some(b"hkpConvexVerticesShape".as_slice()) {
                    has_convex = true;
                    let start = fixup.src_offset as usize;
                    let decode = || -> Option<epic_mickey_lib_rs::hkx_file::HkxCollisionMesh> {
                        let vertices = locals
                            .iter()
                            .find(|f| f.src_offset as usize == start + 0x40)?
                            .dst_offset as usize;
                        let groups = read(start + 0x44)? as usize;
                        let count = read(start + 0x4c)? as usize;
                        if count < 4
                            || count > 65536
                            || groups != (count + 3) / 4
                            || vertices.checked_add(groups.checked_mul(48)?)? > data.len()
                        {
                            return None;
                        }
                        // hkFourTransposedPoints stores XXXX YYYY ZZZZ, not XYZ rows.
                        let mut positions = Vec::with_capacity(count);
                        for index in 0..count {
                            let base = vertices + (index / 4) * 48 + (index % 4) * 4;
                            let point = [
                                f32::from_bits(read(base)?),
                                f32::from_bits(read(base + 16)?),
                                f32::from_bits(read(base + 32)?),
                            ];
                            if !point.iter().all(|v| v.is_finite()) {
                                return None;
                            }
                            positions.push(point);
                        }
                        let indices = convex_hull_indices(&positions)?;
                        Some(epic_mickey_lib_rs::hkx_file::HkxCollisionMesh {
                            name: format!("Convex collision {}", fixup.src_offset),
                            vertices: positions,
                            indices,
                        })
                    };
                    if let Some(mesh) = decode() {
                        meshes.push(mesh);
                    }
                    continue;
                }
                if name != Some(b"hkpSimpleMeshShape".as_slice()) {
                    continue;
                }
                let start = fixup.src_offset as usize;
                let decode = || -> Option<epic_mickey_lib_rs::hkx_file::HkxCollisionMesh> {
                    let vertices = locals
                        .iter()
                        .find(|f| f.src_offset as usize == start + 24)?
                        .dst_offset as usize;
                    let triangles = locals
                        .iter()
                        .find(|f| f.src_offset as usize == start + 36)?
                        .dst_offset as usize;
                    let nv = read(start + 28)? as usize;
                    let nt = read(start + 40)? as usize;
                    if nv > 1_000_000
                        || nt > 1_000_000
                        || vertices.checked_add(nv * 16)? > data.len()
                        || triangles.checked_add(nt * 16)? > data.len()
                    {
                        return None;
                    }
                    let mut positions = Vec::with_capacity(nv);
                    let mut indices = Vec::with_capacity(nt * 3);
                    for i in 0..nv {
                        let p = [
                            f32::from_bits(read(vertices + i * 16)?),
                            f32::from_bits(read(vertices + i * 16 + 4)?),
                            f32::from_bits(read(vertices + i * 16 + 8)?),
                        ];
                        if !p.iter().all(|v| v.is_finite()) {
                            return None;
                        }
                        positions.push(p);
                    }
                    for i in 0..nt {
                        for a in 0..3 {
                            let index = read(triangles + i * 16 + a * 4)?;
                            if index as usize >= nv {
                                return None;
                            }
                            indices.push(index);
                        }
                    }
                    Some(epic_mickey_lib_rs::hkx_file::HkxCollisionMesh {
                        name: format!("Collision_{}", fixup.src_offset),
                        vertices: positions,
                        indices,
                    })
                };
                if let Some(mesh) = decode() {
                    meshes.push(mesh);
                }
            }
        }
    }
    if meshes.is_empty() && !has_convex {
        file.parse_collisions()
    } else {
        meshes
    }
}

pub struct OriginalHkx {
    pub bytes: Vec<u8>,
    baseline: Vec<u8>,
}
impl OriginalHkx {
    pub fn capture(bytes: &[u8], file: &HkxFile) -> Self {
        Self {
            bytes: bytes.into(),
            baseline: file.to_bytes(),
        }
    }
    pub fn save(&self, file: &HkxFile) -> Result<Vec<u8>> {
        let original = HkxFile::load_bytes(&self.bytes).map_err(|e| anyhow::anyhow!(e))?;
        anyhow::ensure!(
            file.header.pointer_size == original.header.pointer_size
                && file.header.endianness == original.header.endianness
                && file.header.contents_version == original.header.contents_version,
            "Havok output must retain the game's platform and SDK version"
        );
        let current = file.to_bytes();
        anyhow::ensure!(
            current.len() == self.baseline.len() && current.len() == self.bytes.len(),
            "Havok editing must preserve the original Wii section layout"
        );
        let mut out = self.bytes.clone();
        for (i, (a, b)) in self.baseline.iter().zip(&current).enumerate() {
            if a != b {
                out[i] = *b;
            }
        }
        let reparsed = HkxFile::load_bytes(&out).map_err(|e| anyhow::anyhow!(e))?;
        anyhow::ensure!(
            reparsed.to_bytes() == current,
            "Havok round-trip validation failed"
        );
        Ok(out)
    }
}

/// Locate the reference pose through Havok pointer fixups, never byte-pattern guesses.
pub fn set_bone(file: &mut HkxFile, index: usize, bone: &HkxBone) -> Result<()> {
    anyhow::ensure!(
        file.header.pointer_size == 4,
        "Reference pose editing requires 32-bit Havok"
    );
    let si = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("No data section")?;
    let ci = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__classnames__")
        .context("No class section")?;
    let classes = &file.section_data[ci];
    let offsets: Vec<_> = file.virtual_fixups[si]
        .iter()
        .filter(|f| {
            classes
                .get(f.class_name_offset as usize..)
                .is_some_and(|v| v.starts_with(b"hkaSkeleton\0"))
        })
        .map(|f| f.src_offset as usize)
        .collect();
    anyhow::ensure!(
        offsets.len() == 1,
        "Expected one skeleton with a verified virtual fixup"
    );
    let off = offsets[0];
    let data = &file.section_data[si];
    let le = file.header.endianness == 1;
    let read = |offset: usize| -> Result<usize> {
        let b = data
            .get(offset..offset + 4)
            .context("Truncated skeleton")?
            .try_into()?;
        Ok(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        } as usize)
    };
    let count = read(off + 28)?;
    anyhow::ensure!(
        index < count && read(off + 16)? == count && read(off + 40)? == count,
        "Inconsistent skeleton arrays"
    );
    let fixup = |source: usize| -> Result<usize> {
        file.local_fixups[si]
            .iter()
            .find(|f| f.src_offset as usize == source)
            .map(|f| f.dst_offset as usize)
            .context("Missing skeleton pointer fixup")
    };
    let parent = fixup(off + 12)? + index * 2;
    let pose = fixup(off + 36)? + index * 48;
    let name = fixup(fixup(off + 24)? + index * 8)?;
    let tail = data.get(name..).context("Invalid bone name pointer")?;
    let length = tail
        .iter()
        .position(|b| *b == 0)
        .context("Unterminated bone name")?;
    anyhow::ensure!(
        bone.name.is_ascii() && !bone.name.contains('\0') && bone.name.len() == length,
        "Bone names must retain their original ASCII byte length"
    );
    anyhow::ensure!(
        parent + 2 <= data.len() && pose + 48 <= data.len(),
        "Pose is outside data section"
    );
    anyhow::ensure!(
        bone.local_translation
            .iter()
            .chain(&bone.local_rotation)
            .chain(&bone.local_scale)
            .all(|f| f.is_finite()),
        "Pose values must be finite"
    );
    let norm = bone.local_rotation.iter().map(|v| v * v).sum::<f32>();
    anyhow::ensure!(
        (norm - 1.0).abs() < 0.01,
        "Rotation must be a normalized quaternion"
    );
    let old_parent = if le {
        i16::from_le_bytes(data[parent..parent + 2].try_into()?)
    } else {
        i16::from_be_bytes(data[parent..parent + 2].try_into()?)
    };
    anyhow::ensure!(
        old_parent == bone.parent_index,
        "Changing the rig hierarchy is not supported by this pose operation"
    );
    let data = &mut file.section_data[si];
    data[name..name + length].copy_from_slice(bone.name.as_bytes());
    for (start, values) in [
        (pose, bone.local_translation.as_slice()),
        (pose + 16, bone.local_rotation.as_slice()),
        (pose + 32, bone.local_scale.as_slice()),
    ] {
        for (i, value) in values.iter().enumerate() {
            data[start + i * 4..start + i * 4 + 4].copy_from_slice(&if le {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            });
        }
    }
    Ok(())
}

/// Incremental convex hull for authoring/preview. Interior points and duplicated
/// SIMD padding are ignored; faces are oriented away from a fixed interior point.
pub fn convex_hull_indices(points: &[[f32; 3]]) -> Option<Vec<u32>> {
    if points.len() < 4 || points.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let p: Vec<[f64; 3]> = points.iter().map(|p| p.map(f64::from)).collect();
    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.iter().zip(b).map(|(a, b)| a * b).sum()
    }
    let a = 0;
    let b = (1..p.len()).max_by(|&i, &j| {
        dot(sub(p[i], p[a]), sub(p[i], p[a])).total_cmp(&dot(sub(p[j], p[a]), sub(p[j], p[a])))
    })?;
    let edge = sub(p[b], p[a]);
    let extent = dot(edge, edge).sqrt();
    if extent == 0. {
        return None;
    }
    let eps = extent * 1e-7;
    let c = (0..p.len()).max_by(|&i, &j| {
        let x = cross(edge, sub(p[i], p[a]));
        let y = cross(edge, sub(p[j], p[a]));
        dot(x, x).total_cmp(&dot(y, y))
    })?;
    let normal = cross(edge, sub(p[c], p[a]));
    let len = dot(normal, normal).sqrt();
    if len <= eps * extent {
        return None;
    }
    let d = (0..p.len()).max_by(|&i, &j| {
        dot(normal, sub(p[i], p[a]))
            .abs()
            .total_cmp(&dot(normal, sub(p[j], p[a])).abs())
    })?;
    if dot(normal, sub(p[d], p[a])).abs() <= eps * len {
        return None;
    }
    let center = [0, 1, 2].map(|i| (p[a][i] + p[b][i] + p[c][i] + p[d][i]) / 4.);
    let orient = |mut f: [usize; 3]| {
        if dot(
            cross(sub(p[f[1]], p[f[0]]), sub(p[f[2]], p[f[0]])),
            sub(center, p[f[0]]),
        ) > 0.
        {
            f.swap(1, 2);
        }
        f
    };
    let mut faces = vec![
        orient([a, b, c]),
        orient([a, d, b]),
        orient([a, c, d]),
        orient([b, d, c]),
    ];
    for index in 0..p.len() {
        if [a, b, c, d].contains(&index) {
            continue;
        }
        let mut horizon = std::collections::BTreeMap::new();
        faces.retain(|face| {
            let n = cross(sub(p[face[1]], p[face[0]]), sub(p[face[2]], p[face[0]]));
            if dot(n, sub(p[index], p[face[0]])) <= eps * dot(n, n).sqrt() {
                return true;
            }
            for (u, v) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
                let key = (u.min(v), u.max(v));
                if horizon.remove(&key).is_none() {
                    horizon.insert(key, (u, v));
                }
            }
            false
        });
        for (_, (u, v)) in horizon {
            faces.push(orient([u, v, index]));
        }
    }
    Some(faces.into_iter().flatten().map(|i| i as u32).collect())
}
#[cfg(test)]
mod convex_tests {
    use super::*;
    #[test]
    fn hull_closes_cube_ignores_interior_points_and_rejects_flat_input() {
        let mut points = Vec::new();
        for x in [-1., 1.] {
            for y in [-1., 1.] {
                for z in [-1., 1.] {
                    points.push([x, y, z]);
                }
            }
        }
        points.push([0.; 3]);
        points.push(points[0]);
        let indices = convex_hull_indices(&points).unwrap();
        assert_eq!(indices.len(), 36);
        let mut edges = std::collections::BTreeMap::new();
        for t in indices.chunks_exact(3) {
            assert!(!t.contains(&8));
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|n| *n == 2));
        assert!(
            convex_hull_indices(&[[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]])
                .is_none()
        );
    }
    #[test]
    #[ignore = "Requires local retail packfiles; set EM1_PACKFILES"]
    fn retail_portal_convex_geometry_is_closed_and_finite() {
        let root = std::path::PathBuf::from(std::env::var("EM1_PACKFILES").unwrap());
        let pack = crate::pack_io::read_filtered(
            std::fs::read(root.join("2d_castaway01_shared.pak")).unwrap(),
            |_, p| p.ends_with("GEN_Portal_Frame_static_hull.hkx_wii"),
        )
        .unwrap();
        let data = &pack
            .pak
            .files
            .iter()
            .find(|f| f.path.ends_with("GEN_Portal_Frame_static_hull.hkx_wii"))
            .unwrap()
            .data;
        let file = HkxFile::load_bytes(data).unwrap();
        let meshes = collision_meshes(&file);
        assert_eq!(meshes.len(), 3);
        for mesh in meshes {
            assert!(mesh.name.starts_with("Convex collision"));
            assert!(mesh.vertices.iter().flatten().all(|v| v.is_finite()));
            let mut edges = std::collections::BTreeMap::new();
            for t in mesh.indices.chunks_exact(3) {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
            assert!(edges.values().all(|n| *n == 2));
            println!(
                "{}: {} vertices / {} triangles",
                mesh.name,
                mesh.vertices.len(),
                mesh.indices.len() / 3
            );
        }
    }
}
