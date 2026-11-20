//! Bind decoded jigsaw geometry to the existing Box3D collision world.
//! Native material filters and stimulus eligibility remain separate policy.
use super::{
    jigsaw::Asset,
    jigsaw_state::PaintState,
    physics::{point, Collision, Triangle},
};
use anyhow::{ensure, Result};

/// Authored triangle identity before Box3D welding/grouping. Subpart order is
/// the original extended-mesh order, independent of per-piece installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriangleSource {
    pub subpart: usize,
    pub triangle: usize,
}
pub struct TriangleRange {
    pub source: std::ops::Range<usize>,
    pub subpart: usize,
    pub first_triangle: usize,
}
pub struct Binding {
    pub owners: Vec<Option<usize>>,
    triangle_sources: Vec<TriangleRange>,
    convex_sources: Vec<(usize, usize)>,
    piece_shapes: Vec<(usize, usize)>,
}
impl Binding {
    /// Contact phase of the restoration guard. Uses the current paint state,
    /// including erased geometry, without changing the mover's contact cache.
    /// The native early triangle-ray phase is not reproduced here yet.
    pub fn contacts_reject_restore(
        &self,
        asset: &Asset,
        state: &PaintState,
        world: &Collision,
        position: [f32; 3],
        requested_key: u32,
    ) -> Result<bool> {
        for &(piece, shape) in &self.piece_shapes {
            // All four runtime palette entries have bit 16 clear; bit 15 is
            // exactly the painted state, independent of the rigid-body layer.
            let painted = *state
                .painted
                .get(piece)
                .ok_or_else(|| anyhow::anyhow!("Missing jigsaw piece state"))?;
            if painted {
                continue;
            }
            for contact in world.shape_penetrations(shape, position)? {
                let key = match contact.source_triangle {
                    Some(triangle) => self.triangle_key(asset, triangle)?,
                    None => self.convex_key(asset, shape)?,
                }
                .ok_or_else(|| {
                    anyhow::anyhow!("Jigsaw overlap contact lost its native shape key")
                })?;
                if super::jigsaw_overlap::contact_rejects_restore(
                    requested_key,
                    key,
                    -contact.depth,
                    Some(0),
                ) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    pub fn convex_key(&self, asset: &Asset, shape: usize) -> Result<Option<u32>> {
        let Ok(index) = self
            .convex_sources
            .binary_search_by_key(&shape, |&(shape, _)| shape)
        else {
            return Ok(None);
        };
        let part = &asset.collision.convex[self.convex_sources[index].1];
        let key = super::collision::ShapeKeys::Triangles {
            bits: asset.collision.key_bits,
            subpart: u32::try_from(part.subpart)?,
        }
        .key(Some(part.child))?;
        Ok(Some(key | 0x80000000))
    }
    /// Runtime palette replaces serialized material pointers during native
    /// external-buffer binding (REL 0x249a08). The immutable asset is retained.
    pub fn runtime_triangle_filter(
        &self,
        asset: &Asset,
        state: &PaintState,
        index: usize,
        motion_type: u8,
    ) -> Result<Option<u32>> {
        let Some(source) = self.triangle_source(index) else {
            return Ok(None);
        };
        let part = &asset.collision.parts[source.subpart];
        let filter = state
            .filter(part.piece, motion_type)
            .ok_or_else(|| anyhow::anyhow!("Missing jigsaw piece state"))?;
        Ok(Some(
            super::jigsaw_state::RUNTIME_MATERIALS
                [super::jigsaw_state::material_index(filter) as usize],
        ))
    }
    /// Serialized table lookup only; runtime initialization replaces these
    /// values. Original jigsaws may serialize a zero default.
    pub fn serialized_triangle_filter(
        &self,
        asset: &Asset,
        index: usize,
        material_index: u8,
    ) -> Result<Option<u32>> {
        let Some(source) = self.triangle_source(index) else {
            return Ok(None);
        };
        let part = &asset.collision.parts[source.subpart];
        part.materials.selected(material_index).map(Some)
    }
    /// Recover a native triangle key from an all-hit query's source index.
    /// The per-subpart serialized triangle offset is not part of this key.
    pub fn triangle_key(&self, asset: &Asset, index: usize) -> Result<Option<u32>> {
        let Some(source) = self.triangle_source(index) else {
            return Ok(None);
        };
        super::collision::ShapeKeys::Triangles {
            bits: asset.collision.key_bits,
            subpart: u32::try_from(source.subpart)?,
        }
        .key(Some(source.triangle))
        .map(Some)
    }
    pub fn triangle_source(&self, index: usize) -> Option<TriangleSource> {
        let entry = self.triangle_sources.get(
            self.triangle_sources
                .partition_point(|r| r.source.end <= index),
        )?;
        entry.source.contains(&index).then(|| TriangleSource {
            subpart: entry.subpart,
            triangle: entry.first_triangle + index - entry.source.start,
        })
    }
    /// The caller supplies the collision-to-world transform. This module must
    /// not infer it from render mesh transforms or duplicate geometry per mesh.
    pub fn install(
        asset: &Asset,
        state: &PaintState,
        transform: &[f32; 16],
        world: &mut Collision,
        mut allocate_owner: impl FnMut(usize) -> usize,
    ) -> Result<Self> {
        ensure!(
            state.painted.len() == asset.pieces.len(),
            "Jigsaw state/geometry count mismatch"
        );
        ensure!(
            transform.iter().all(|v| v.is_finite()),
            "Invalid jigsaw world transform"
        );
        let mut owners = vec![None; asset.pieces.len()];
        let mut triangle_sources = Vec::new();
        let mut convex_sources = Vec::new();
        let mut piece_shapes = Vec::new();
        for piece in 0..asset.pieces.len() {
            let mut triangles = Vec::new();
            let start = world.triangle_count();
            for (subpart, part) in asset
                .collision
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.piece == piece)
            {
                let vertices = &asset.collision.vertices[part.vertex_stream];
                let mut source_start = start + triangles.len();
                let mut first_triangle = 0;
                for (triangle, face) in part.indices.chunks_exact(3).enumerate() {
                    if part.unreachable_triangles.binary_search(&triangle).is_ok() {
                        if start + triangles.len() > source_start {
                            triangle_sources.push(TriangleRange {
                                source: source_start..start + triangles.len(),
                                subpart,
                                first_triangle,
                            });
                        }
                        first_triangle = triangle + 1;
                        source_start = start + triangles.len();
                        continue;
                    }
                    triangles.push(Triangle {
                        p: std::array::from_fn(|i| point(transform, vertices[face[i] as usize])),
                    });
                }
                if triangles.len() + start > source_start {
                    triangle_sources.push(TriangleRange {
                        source: source_start..start + triangles.len(),
                        subpart,
                        first_triangle,
                    });
                }
            }
            let convex = asset
                .collision
                .convex
                .iter()
                .enumerate()
                .filter(|(_, p)| p.piece == piece)
                .collect::<Vec<_>>();
            if triangles.is_empty() && convex.is_empty() {
                continue;
            }
            let owner = allocate_owner(piece);
            if !triangles.is_empty() {
                world.add_static_triangles(&triangles, asset.collision.radius, owner)?;
            }
            for (source, part) in convex {
                let vertices = part
                    .vertices
                    .iter()
                    .map(|&v| point(transform, v))
                    .collect::<Vec<_>>();
                let shape = world.add_static_convex(&vertices, part.radius, owner)?;
                convex_sources.push((shape, source));
            }
            world.set_owner_solid(owner, state.painted[piece])?;
            piece_shapes.extend(world.owner_shapes(owner).map(|shape| (piece, shape)));
            owners[piece] = Some(owner);
        }
        Ok(Self {
            owners,
            triangle_sources,
            convex_sources,
            piece_shapes,
        })
    }
    pub fn set_painted(&self, piece: usize, painted: bool, world: &mut Collision) -> Result<()> {
        ensure!(piece < self.owners.len(), "Invalid jigsaw collision piece");
        if let Some(owner) = self.owners[piece] {
            world.set_owner_solid(owner, painted)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn triangle_identity_handles_gaps_and_reordered_subparts() {
        let binding = Binding {
            owners: vec![],
            convex_sources: vec![],
            piece_shapes: vec![],
            triangle_sources: vec![
                TriangleRange {
                    source: 5..8,
                    subpart: 3,
                    first_triangle: 0,
                },
                TriangleRange {
                    source: 8..10,
                    subpart: 1,
                    first_triangle: 2,
                },
                TriangleRange {
                    source: 15..17,
                    subpart: 4,
                    first_triangle: 0,
                },
            ],
        };
        for index in [0, 4, 10, 14, 17, usize::MAX] {
            assert_eq!(binding.triangle_source(index), None);
        }
        assert_eq!(
            binding.triangle_source(7),
            Some(TriangleSource {
                subpart: 3,
                triangle: 2
            })
        );
        assert_eq!(
            binding.triangle_source(8),
            Some(TriangleSource {
                subpart: 1,
                triangle: 2
            })
        );
        assert_eq!(
            binding.triangle_source(16),
            Some(TriangleSource {
                subpart: 4,
                triangle: 1
            })
        );
    }
    #[test]
    #[ignore = "Requires BLOTWORX_TEST_DATA pointing to original game files"]
    fn skull_island_retains_unreachable_invalid_face_without_fetching_vertices() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = crate::pack_io::read_filtered(
            std::fs::read(root.join("packfiles/potw_zonel.pak"))?,
            |_, path| {
                path.eq_ignore_ascii_case(
                    "Environments/ThePark/AdventureLand/Pirates/Zones/P_ZoneL_Toon14.nif_wii",
                )
            },
        )?;
        let file = pak
            .pak
            .files
            .iter()
            .find(|f| !f.data.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Skull Island jigsaw absent"))?;
        let assets = super::super::jigsaw::load(&file.data)?;
        let asset = &assets[0];
        let invalid = &asset.collision.parts[48];
        assert_eq!(invalid.indices, [191, 191, 192]);
        assert_eq!(invalid.unreachable_triangles, [0]);
        assert_eq!(asset.collision.vertices[invalid.vertex_stream].len(), 171);
        let mut world = Collision::new(vec![])?;
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let state = PaintState::initial(asset.pieces.len(), true, &[], &[]);
        let binding = Binding::install(asset, &state, &identity, &mut world, |piece| piece + 1)?;
        for index in 0..world.triangle_count() {
            let source = binding
                .triangle_source(index)
                .ok_or_else(|| anyhow::anyhow!("Lost triangle identity"))?;
            assert_ne!(source.subpart, 48);
            let part = &asset.collision.parts[source.subpart];
            assert_eq!(
                world.triangles[index].p,
                std::array::from_fn(|i| asset.collision.vertices[part.vertex_stream]
                    [part.indices[source.triangle * 3 + i] as usize])
            );
        }
        assert_eq!(
            world.triangle_count(),
            asset
                .collision
                .parts
                .iter()
                .map(|p| p.indices.len() / 3 - p.unreachable_triangles.len())
                .sum::<usize>()
        );
        Ok(())
    }
    #[test]
    #[ignore = "Requires BLOTWORX_TEST_DATA pointing to original game files"]
    fn original_jigsaw_geometry_binds_and_survives_erasure() -> Result<()> {
        use super::super::physics::{add, cross, mul, sub, unit};
        use epic_mickey_lib_rs::packfile::Packfile;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut assets_checked = 0;
        for file in pak.files.iter().filter(|f| {
            let p = f.path.to_ascii_lowercase();
            p.ends_with("gv_zonea_toon01.nif_wii") || p.ends_with("gv_zonea_toon02.nif_wii")
        }) {
            for asset in super::super::jigsaw::load(&file.data)? {
                let mut world = Collision::new(vec![Triangle {
                    p: [[1000., 0., 0.], [1001., 0., 0.], [1000., 0., 1.]],
                }])?;
                let identity = [
                    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                ];
                let state = PaintState::initial(asset.pieces.len(), true, &[], &[]);
                let binding =
                    Binding::install(&asset, &state, &identity, &mut world, |piece| piece + 1)?;
                assert!(binding.owners.iter().all(Option::is_some));
                assert_eq!(binding.convex_sources.len(), asset.collision.convex.len());
                for &(shape, source) in &binding.convex_sources {
                    let part = &asset.collision.convex[source];
                    let key = binding.convex_key(&asset, shape)?.unwrap();
                    let mask = u32::MAX << (32 - asset.collision.key_bits);
                    assert!(asset.pieces[part.piece]
                        .collision_keys
                        .contains(&(key & mask)));
                    assert_eq!(key & !mask, part.child as u32);
                }
                assert_eq!(binding.convex_key(&asset, usize::MAX)?, None);
                let mut hits = 0;
                for part in &asset.collision.parts {
                    let v = &asset.collision.vertices[part.vertex_stream];
                    let face = &part.indices[..3];
                    let a = v[face[0] as usize];
                    let b = v[face[1] as usize];
                    let c = v[face[2] as usize];
                    let n = unit(cross(sub(b, a), sub(c, a)));
                    let center = mul(add(add(a, b), c), 1. / 3.);
                    let origin = add(center, mul(n, 0.5));
                    let delta = mul(n, -1.);
                    if world.ray(origin, delta, 1.).is_some() {
                        hits += 1;
                    }
                    let ray_hits = world.ray_hits(origin, delta)?;
                    for hit in &ray_hits {
                        if let Some(source) =
                            hit.source_triangle.and_then(|i| binding.triangle_source(i))
                        {
                            let authored = &asset.collision.parts[source.subpart];
                            assert_eq!(binding.owners[authored.piece], Some(hit.owner));
                            assert!(source.triangle < authored.indices.len() / 3);
                            let key = binding
                                .triangle_key(&asset, hit.source_triangle.unwrap())?
                                .unwrap();
                            let subpart_mask = u32::MAX << (32 - asset.collision.key_bits);
                            assert!(
                                asset.pieces[authored.piece]
                                    .collision_keys
                                    .contains(&(key & subpart_mask)),
                                "Ray key does not belong to original piece key table"
                            );
                            for index in 0..4 {
                                assert_eq!(
                                    binding.serialized_triangle_filter(
                                        &asset,
                                        hit.source_triangle.unwrap(),
                                        index
                                    )?,
                                    Some(authored.materials.selected(index)?)
                                );
                            }
                            for painted in [false, true] {
                                let state =
                                    PaintState::initial(asset.pieces.len(), painted, &[], &[]);
                                for motion in [2, 4, 5] {
                                    assert_eq!(
                                        binding.runtime_triangle_filter(
                                            &asset,
                                            &state,
                                            hit.source_triangle.unwrap(),
                                            motion
                                        )?,
                                        state.filter(authored.piece, motion)
                                    );
                                }
                            }
                        } else if hit.triangle.is_some() && hit.owner != 0 {
                            anyhow::bail!("Jigsaw ray lost its authored triangle identity");
                        }
                    }
                    let before = ray_hits.len();
                    for piece in 0..asset.pieces.len() {
                        binding.set_painted(piece, false, &mut world)?;
                    }
                    assert!(
                        world.ray(origin, delta, 1.).is_none(),
                        "Erased shape still blocks ray"
                    );
                    assert_eq!(
                        world.ray_hits(origin, delta)?.len(),
                        before,
                        "Erasure lost paint targets"
                    );
                    assert!(
                        world.sweep_sphere(origin, delta, 0.025)?.is_some(),
                        "Erased geometry cannot be repainted"
                    );
                    for piece in 0..asset.pieces.len() {
                        binding.set_painted(piece, true, &mut world)?;
                    }
                }
                assert!(hits > 0);
                assets_checked += 1;
                println!(
                    "{}: {} pieces bound, {} triangle-part probes",
                    file.path,
                    binding.owners.len(),
                    asset.collision.parts.len()
                );
            }
        }
        assert_eq!(assets_checked, 2);
        Ok(())
    }
}
