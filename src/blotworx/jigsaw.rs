//! EM1 version-4 JPSJigsawNode payloads. Structural decoding from original assets.
//! Piece link sets and adjacency are validated; field/runtime semantics remain
//! provisional until checked against the REL. Opaque suffixes are never discarded.
use anyhow::{ensure, Context, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Piece {
    pub adjacency_start: u16,
    pub adjacency_count: u32,
    pub mesh_links: Vec<u32>,
    pub secondary_mesh_links: Vec<u32>,
    pub collision_keys: Vec<u32>,
    pub position: [f32; 3],
}
#[derive(Debug)]
pub struct Jigsaw<'a> {
    pub pieces: Vec<Piece>,
    pub adjacency: Vec<u16>,
    /// Eight as-yet unidentified bytes followed by twelve serialized floats.
    pub collision_metadata: &'a [u8],
    /// Embedded Havok packfile; borrowed, not duplicated in the asset cache.
    pub havok: &'a [u8],
    pub undecoded_suffix: &'a [u8],
}
struct Reader<'a> {
    data: &'a [u8],
    little: bool,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.data.len(), "Truncated JPSJigsawNode");
        let (a, b) = self.data.split_at(n);
        self.data = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?.try_into()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?.try_into()?;
        Ok(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn array32(&mut self, n: usize) -> Result<Vec<u32>> {
        ensure!(n <= self.data.len() / 4, "Jigsaw array exceeds block");
        (0..n).map(|_| self.u32()).collect()
    }
    fn skip_array32(&mut self) -> Result<()> {
        let n = self.u32()? as usize;
        ensure!(n <= self.data.len() / 4, "Jigsaw base array exceeds block");
        self.take(n * 4)?;
        Ok(())
    }
}
/// Decode the complete serialized node, including its ordinary NiNode prefix.
/// Only the observed EM1 v4 layout is accepted. Unknown versions are errors.
pub fn read_node(bytes: &[u8], little: bool) -> Result<Jigsaw<'_>> {
    let mut r = Reader {
        data: bytes,
        little,
    };
    r.u32()?;
    r.skip_array32()?;
    r.u32()?; // name, extra data, controller
    r.take(54)?;
    r.skip_array32()?;
    r.u32()?; // AV flags/transform/properties/collision
    r.skip_array32()?;
    r.skip_array32()?; // children, effects
    ensure!(r.u32()? == 4, "Unsupported jigsaw node version");
    let count = r.u32()? as usize;
    ensure!(count <= r.data.len() / 32, "Invalid jigsaw piece count");
    let mut pieces = Vec::with_capacity(count);
    for index in 0..count {
        ensure!(r.u32()? == 4, "Unsupported jigsaw piece version at {index}");
        let adjacency_count = r.u32()?;
        let adjacency_start = r.u16()?;
        let n = r.u16()? as usize;
        let mesh_links = r.array32(n)?;
        let n = r.u16()? as usize;
        let secondary_mesh_links = r.array32(n)?;
        let n = r.u32()? as usize;
        let collision_keys = r.array32(n)?;
        let position = [
            f32::from_bits(r.u32()?),
            f32::from_bits(r.u32()?),
            f32::from_bits(r.u32()?),
        ];
        ensure!(
            position.iter().all(|f| f.is_finite()),
            "Invalid jigsaw piece position"
        );
        pieces.push(Piece {
            adjacency_start,
            adjacency_count,
            mesh_links,
            secondary_mesh_links,
            collision_keys,
            position,
        });
    }
    let n = r.u32()? as usize;
    ensure!(n <= r.data.len() / 2, "Invalid jigsaw adjacency count");
    let adjacency = (0..n).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
    for (index, piece) in pieces.iter().enumerate() {
        let end = (piece.adjacency_start as usize)
            .checked_add(piece.adjacency_count as usize)
            .context("Jigsaw adjacency overflow")?;
        ensure!(
            end <= adjacency.len(),
            "Jigsaw piece {index} adjacency range exceeds array"
        );
    }
    ensure!(
        adjacency.iter().all(|&i| (i as usize) < count),
        "Jigsaw adjacency references absent piece"
    );
    let collision_metadata = r.take(56)?;
    let n = r.u32()? as usize;
    let havok = r.take(n)?;
    ensure!(
        havok.starts_with(&[0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10]),
        "Missing embedded jigsaw Havok signature"
    );
    Ok(Jigsaw {
        pieces,
        adjacency,
        collision_metadata,
        havok,
        undecoded_suffix: r.data,
    })
}

/// NIF streams which supply the external buffers of hkpExtendedMeshShape.
/// The two reserved arrays have only been observed empty; their element layout
/// is deliberately not guessed. Asset-supported semantics await REL validation.
#[derive(Debug, Serialize)]
pub struct Bindings {
    pub vertex_streams: Vec<u32>,
    pub index_streams: Vec<u32>,
    pub vertex_regions: Vec<u16>,
    pub triangle_vertex_streams: Vec<u32>,
    pub triangle_index_streams: Vec<u32>,
    pub triangle_pieces: Vec<u32>,
    pub convex_pieces: Vec<u32>,
}
pub fn read_bindings(bytes: &[u8], little: bool, pieces: &[Piece]) -> Result<Bindings> {
    let mut r = Reader {
        data: bytes,
        little,
    };
    fn array(r: &mut Reader<'_>) -> Result<Vec<u32>> {
        let n = r.u32()? as usize;
        r.array32(n)
    }
    let vertex_streams = array(&mut r)?;
    let index_streams = array(&mut r)?;
    let n = r.u32()? as usize;
    ensure!(n <= r.data.len() / 2, "Truncated jigsaw vertex regions");
    let vertex_regions = (0..n).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
    ensure!(
        r.u32()? == 0 && r.u32()? == 0,
        "Nonempty reserved jigsaw binding arrays need decoding"
    );
    let triangle_vertex_streams = array(&mut r)?;
    let triangle_index_streams = array(&mut r)?;
    let triangle_pieces = array(&mut r)?;
    let convex_pieces = array(&mut r)?;
    ensure!(r.data.is_empty(), "Unparsed jigsaw stream bindings");
    ensure!(
        vertex_streams.len() == vertex_regions.len(),
        "Jigsaw vertex region count mismatch"
    );
    ensure!(
        triangle_vertex_streams.len() == triangle_pieces.len()
            && triangle_index_streams.len() == triangle_pieces.len(),
        "Jigsaw triangle binding count mismatch"
    );
    ensure!(
        triangle_vertex_streams
            .iter()
            .all(|&i| (i as usize) < vertex_streams.len()),
        "Missing jigsaw vertex stream"
    );
    ensure!(
        triangle_index_streams
            .iter()
            .all(|&i| (i as usize) < index_streams.len()),
        "Missing jigsaw index stream"
    );
    ensure!(
        triangle_pieces
            .iter()
            .chain(&convex_pieces)
            .all(|&i| (i as usize) < pieces.len()),
        "Missing jigsaw collision owner"
    );
    Ok(Bindings {
        vertex_streams,
        index_streams,
        vertex_regions,
        triangle_vertex_streams,
        triangle_index_streams,
        triangle_pieces,
        convex_pieces,
    })
}
impl Bindings {
    /// Cross-check independent forward/reverse ownership tables using the
    /// embedded Havok shape's actual numBitsForSubpartIndex value.
    pub fn validate_keys(&self, pieces: &[Piece], bits: u32) -> Result<()> {
        ensure!(
            (1..=31).contains(&bits),
            "Invalid jigsaw shape-key bit count"
        );
        let mut expected = vec![Vec::new(); pieces.len()];
        for (kind, owners) in [
            (0u32, &self.triangle_pieces),
            (0x80000000, &self.convex_pieces),
        ] {
            for (index, &owner) in owners.iter().enumerate() {
                ensure!(
                    index < (1usize << (bits - 1)),
                    "Jigsaw subpart key overflow"
                );
                expected
                    .get_mut(owner as usize)
                    .context("Missing jigsaw owner")?
                    .push(kind | ((index as u32) << (32 - bits)));
            }
        }
        for (i, (keys, piece)) in expected.iter_mut().zip(pieces).enumerate() {
            keys.sort_unstable();
            let mut actual = piece.collision_keys.clone();
            actual.sort_unstable();
            ensure!(
                *keys == actual,
                "Jigsaw piece {i} collision ownership tables disagree"
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct TrianglePart {
    pub materials: CollisionMaterials,
    pub piece: usize,
    pub vertex_stream: usize,
    pub triangle_offset: u32,
    pub indices: Vec<u32>,
    /// Invalid serialized faces proven absent from every MOPP terminal.
    /// Preserve their indices and native triangle numbering, but never fetch
    /// their vertices or submit them to Box3D.
    pub unreachable_triangles: Vec<usize>,
}
#[derive(Debug)]
pub struct CollisionGeometry {
    pub key_bits: u8,
    pub convex: Vec<ConvexPart>,
    pub vertices: Vec<Vec<[f32; 3]>>,
    pub parts: Vec<TrianglePart>,
    pub radius: f32,
}
#[derive(Debug)]
pub struct ConvexPart {
    pub subpart: usize,
    pub materials: CollisionMaterials,
    pub piece: usize,
    pub child: usize,
    pub vertices: Vec<[f32; 3]>,
    pub radius: f32,
}
#[derive(Debug, Clone)]
pub enum CollisionMaterials {
    Default(u32),
    Table(std::sync::Arc<[u32]>),
}
impl CollisionMaterials {
    pub fn selected(&self, index: u8) -> Result<u32> {
        match self {
            Self::Default(word) => Ok(*word),
            Self::Table(words) => words
                .get(index as usize)
                .copied()
                .context("Jigsaw material selection outside authored table"),
        }
    }
}
/// Resolve external triangle buffers and embedded convex children.
/// Currently accepts identity transforms on convex subparts; reject others
/// instead of silently discarding an authored transform.
pub fn collision_geometry(
    nif: &epic_mickey_lib_rs::nif_file::NifFile,
    hkx: &epic_mickey_lib_rs::hkx_file::HkxFile,
    b: &Bindings,
) -> Result<CollisionGeometry> {
    use epic_mickey_lib_rs::nif_file::NiObject;
    ensure!(
        hkx.header.pointer_size == 4
            && hkx.header.endianness == 0
            && hkx.header.contents_version.starts_with("Havok-7.0"),
        "Unsupported jigsaw Havok layout"
    );
    let section = usize::try_from(hkx.header.contents_section_index)?;
    let data = hkx
        .section_data
        .get(section)
        .context("Missing jigsaw Havok section")?;
    let word = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            data.get(at..at + 4)
                .context("Truncated jigsaw collision")?
                .try_into()?,
        ))
    };
    let global = |at: usize| -> Result<usize> {
        let p = hkx.global_fixups[section]
            .iter()
            .find(|p| p.src_offset as usize == at)
            .context("Missing jigsaw shape pointer")?;
        ensure!(
            p.dst_section as usize == section,
            "Cross-section jigsaw shape"
        );
        Ok(p.dst_offset as usize)
    };
    let local = |at: usize| -> Result<usize> {
        Ok(hkx.local_fixups[section]
            .iter()
            .find(|p| p.src_offset as usize == at)
            .context("Missing jigsaw array pointer")?
            .dst_offset as usize)
    };
    ensure!(
        hkx.root_class_name == "hkpMoppBvTreeShape",
        "Expected jigsaw MOPP root"
    );
    let shape = global(usize::try_from(hkx.header.contents_section_offset)? + 0x34)?;
    let fix = hkx.virtual_fixups[section]
        .iter()
        .find(|p| p.src_offset as usize == shape)
        .context("Missing jigsaw child class")?;
    let name = hkx.section_data[fix.section_index as usize]
        .get(fix.class_name_offset as usize..)
        .context("Missing jigsaw class name")?
        .split(|v| *v == 0)
        .next()
        .unwrap_or_default();
    ensure!(
        name == b"hkpExtendedMeshShape",
        "Unsupported jigsaw MOPP child"
    );
    let materials = |part: usize| -> Result<CollisionMaterials> {
        let pointer = |at: usize| -> Result<Option<usize>> {
            if let Some(p) = hkx.local_fixups[section]
                .iter()
                .find(|p| p.src_offset as usize == at)
            {
                return Ok(Some(p.dst_offset as usize));
            }
            if hkx.global_fixups[section]
                .iter()
                .any(|p| p.src_offset as usize == at)
            {
                return global(at).map(Some);
            }
            ensure!(word(at)? == 0, "Unresolved jigsaw material pointer");
            Ok(None)
        };
        // DOL 0x802704ec: missing index storage uses the shape default.
        if pointer(part + 4)?.is_none() {
            return Ok(CollisionMaterials::Default(word(shape + 0xdc)?));
        }
        let base = pointer(part + 12)?.context("Missing jigsaw collision materials")?;
        let stride = i16::from_be_bytes(
            data.get(part + 2..part + 4)
                .context("Missing material stride")?
                .try_into()?,
        );
        let count = u16::from_be_bytes(
            data.get(part + 10..part + 12)
                .context("Missing material count")?
                .try_into()?,
        );
        ensure!(count > 0, "Empty jigsaw collision material table");
        let filters = (0..count)
            .map(|index| {
                let at = usize::try_from(base as i64 + i64::from(index) * i64::from(stride))
                    .context("Jigsaw material address underflow")?;
                word(at)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(CollisionMaterials::Table(filters.into()))
    };
    let count = word(shape + 0xb8)? as usize;
    let key_bits = u8::try_from(word(shape + 0xb0)?)?;
    ensure!(
        (1..=31).contains(&key_bits),
        "Invalid jigsaw shape-key layout"
    );
    ensure!(
        count == b.triangle_pieces.len() && word(shape + 0xc4)? as usize == b.convex_pieces.len(),
        "Jigsaw Havok ownership count mismatch"
    );
    // Havok serializes empty hkArrays without a pointer fixup. Convex-only
    // jigsaws legitimately have no triangle subparts to dereference.
    let parts = if count == 0 { 0 } else { local(shape + 0xb4)? };
    let radius = f32::from_bits(word(shape + 0xe4)?);
    ensure!(
        radius.is_finite() && radius >= 0.,
        "Invalid jigsaw triangle radius"
    );
    let scale = [
        f32::from_bits(word(shape + 0x80)?),
        f32::from_bits(word(shape + 0x84)?),
        f32::from_bits(word(shape + 0x88)?),
    ];
    ensure!(
        scale.iter().all(|v| v.is_finite() && *v > 0.),
        "Invalid jigsaw collision scale"
    );
    let stream = |link: u32| match nif.blocks.get(link as usize) {
        Some(NiObject::NiDataStream(d)) => Ok(d),
        _ => anyhow::bail!("Jigsaw link {link} is not a data stream"),
    };
    let mut vertices = Vec::with_capacity(b.vertex_streams.len());
    for (i, &link) in b.vertex_streams.iter().enumerate() {
        let ds = stream(link)?;
        ensure!(
            ds.component_formats == [0x00030437],
            "Unsupported jigsaw position format"
        );
        let region = ds
            .regions
            .get(b.vertex_regions[i] as usize)
            .context("Missing jigsaw vertex region")?;
        let start = region.start_index as usize * 12;
        let bytes = ds
            .buffer
            .get(start..start + region.count as usize * 12)
            .context("Truncated jigsaw positions")?;
        let mut points = Vec::with_capacity(region.count as usize);
        for row in bytes.chunks_exact(12) {
            let p = std::array::from_fn(|axis| {
                let bytes = row[axis * 4..axis * 4 + 4].try_into().unwrap();
                (if nif.source_is_little_endian {
                    f32::from_le_bytes(bytes)
                } else {
                    f32::from_be_bytes(bytes)
                }) * scale[axis]
            });
            ensure!(p.iter().all(|v| v.is_finite()), "Nonfinite jigsaw vertex");
            points.push(p);
        }
        vertices.push(points);
    }
    let mut output = Vec::with_capacity(count);
    let mut terminal_keys = None;
    for i in 0..count {
        let part = parts + i * 0x60;
        ensure!(
            data.get(part + 0x24..part + 0x44)
                .is_some_and(|v| v.iter().all(|b| *b == 0))
                && data
                    .get(part + 0x50..part + 0x60)
                    .is_some_and(|v| v.iter().all(|b| *b == 0)),
            "Transformed jigsaw triangle subpart {i} needs decoding: {:02x?} / {:02x?}",
            data.get(part + 0x24..part + 0x44),
            data.get(part + 0x50..part + 0x60)
        );
        let vertex_stream = b.triangle_vertex_streams[i] as usize;
        ensure!(
            word(part + 0x1c)? == 12
                && word(part + 0x20)? as usize == vertices[vertex_stream].len(),
            "Jigsaw vertex layout mismatch"
        );
        let ds = stream(b.index_streams[b.triangle_index_streams[i] as usize])?;
        let kind = *data.get(part + 0x48).context("Missing jigsaw index type")?;
        let (width, format) = match kind {
            1 => (1, 0x00010105),
            2 => (2, 0x00010215),
            3 => (4, 0x00010425),
            _ => anyhow::bail!("Unsupported jigsaw index type"),
        };
        ensure!(
            ds.component_formats == [format],
            "Jigsaw index format mismatch"
        );
        ensure!(
            ds.regions.len() == 1 && ds.regions[0].start_index == 0,
            "Unsupported jigsaw index regions"
        );
        let stride = word(part + 0x44)? as usize;
        let triangles = word(part + 0x14)? as usize;
        ensure!(
            stride >= width * 3 && triangles <= ds.buffer.len() / stride,
            "Jigsaw index buffer too short"
        );
        let mut indices = Vec::with_capacity(triangles * 3);
        let mut unreachable_triangles = Vec::new();
        for (triangle, row) in ds.buffer.chunks_exact(stride).take(triangles).enumerate() {
            let start = indices.len();
            for axis in 0..3 {
                let bytes = &row[axis * width..(axis + 1) * width];
                let index = match width {
                    1 => bytes[0] as u32,
                    2 => {
                        if nif.source_is_little_endian {
                            u16::from_le_bytes(bytes.try_into()?) as u32
                        } else {
                            u16::from_be_bytes(bytes.try_into()?) as u32
                        }
                    }
                    _ => {
                        if nif.source_is_little_endian {
                            u32::from_le_bytes(bytes.try_into()?)
                        } else {
                            u32::from_be_bytes(bytes.try_into()?)
                        }
                    }
                };
                indices.push(index);
            }
            if indices[start..]
                .iter()
                .any(|&index| index as usize >= vertices[vertex_stream].len())
            {
                if terminal_keys.is_none() {
                    let root = usize::try_from(hkx.header.contents_section_offset)?;
                    let pointer = root + 0x14;
                    let code = if hkx.local_fixups[section]
                        .iter()
                        .any(|p| p.src_offset as usize == pointer)
                    {
                        local(pointer)?
                    } else {
                        global(pointer)?
                    };
                    let code = super::mopp_query::Code::import(hkx, section, code)?;
                    terminal_keys = Some(super::mopp_query::terminal_keys(code.bytes)?);
                }
                let key = super::collision::ShapeKeys::Triangles {
                    bits: key_bits,
                    subpart: u32::try_from(i)?,
                }
                .key(Some(triangle))?;
                ensure!(!terminal_keys.as_ref().unwrap().contains(&key),
                    "Jigsaw triangle index out of range: active subpart {i}, triangle {triangle}, key {key:#x}");
                unreachable_triangles.push(triangle);
            }
        }
        output.push(TrianglePart {
            materials: materials(part)?,
            piece: b.triangle_pieces[i] as usize,
            vertex_stream,
            triangle_offset: word(part + 0x4c)?,
            indices,
            unreachable_triangles,
        });
    }
    let convex_count = word(shape + 0xc4)? as usize;
    let mut convex = Vec::new();
    if convex_count > 0 {
        let convex_parts = local(shape + 0xc0)?;
        ensure!(
            scale == [1., 1., 1.],
            "Scaled jigsaw convex subparts need validation"
        );
        for i in 0..convex_count {
            let part = convex_parts + i * 0x40;
            // Embedded hkClassMember reflection: childShapes +0x14,
            // rotation +0x20 (quaternion), translation +0x30 (vector4).
            let q = [
                word(part + 0x20)?,
                word(part + 0x24)?,
                word(part + 0x28)?,
                word(part + 0x2c)?,
            ]
            .map(f32::from_bits);
            let t =
                [word(part + 0x30)?, word(part + 0x34)?, word(part + 0x38)?].map(f32::from_bits);
            ensure!(
                q == [0., 0., 0., 1.] && t == [0., 0., 0.],
                "Transformed jigsaw convex subpart needs validation"
            );
            let count = word(part + 0x18)? as usize;
            let children = if count == 0 { 0 } else { local(part + 0x14)? };
            let materials = materials(part)?;
            ensure!(
                count <= data.len() / 16,
                "Invalid jigsaw convex child count"
            );
            for child in 0..count {
                let leaf = global(children + child * 16)?;
                let fix = hkx.virtual_fixups[section]
                    .iter()
                    .find(|p| p.src_offset as usize == leaf)
                    .context("Missing jigsaw convex class")?;
                let name = hkx.section_data[fix.section_index as usize]
                    .get(fix.class_name_offset as usize..)
                    .context("Missing convex class name")?
                    .split(|v| *v == 0)
                    .next()
                    .unwrap_or_default();
                ensure!(
                    name == b"hkpConvexVerticesShape",
                    "Unsupported jigsaw convex child"
                );
                let hull = super::character_hull::decode(data, leaf, local(leaf + 0x40)?)?;
                convex.push(ConvexPart {
                    subpart: i,
                    materials: materials.clone(),
                    piece: b.convex_pieces[i] as usize,
                    child,
                    vertices: hull.vertices,
                    radius: hull.radius,
                });
            }
        }
    }
    Ok(CollisionGeometry {
        key_bits,
        vertices,
        parts: output,
        radius,
        convex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 82]; // empty NiNode prefix
        let u = |b: &mut Vec<u8>, v: u32| b.extend(v.to_be_bytes());
        u(&mut b, 4);
        u(&mut b, 1);
        u(&mut b, 4);
        u(&mut b, 0);
        b.extend(0u16.to_be_bytes());
        b.extend(1u16.to_be_bytes());
        u(&mut b, 15);
        b.extend(1u16.to_be_bytes());
        u(&mut b, 1);
        u(&mut b, 1);
        u(&mut b, 0x80000000);
        for f in [1f32, 2., 3.] {
            u(&mut b, f.to_bits());
        }
        u(&mut b, 0);
        b.extend([0; 56]);
        u(&mut b, 8);
        b.extend([0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10]);
        b.extend([9, 8, 7]);
        b
    }
    #[test]
    fn mixed_width_links_and_borrowed_payload() {
        let b = fixture();
        let j = read_node(&b, false).unwrap();
        assert_eq!(j.pieces[0].mesh_links, [15]);
        assert_eq!(j.pieces[0].secondary_mesh_links, [1]);
        assert_eq!(j.pieces[0].collision_keys, [0x80000000]);
        assert_eq!(j.pieces[0].position, [1., 2., 3.]);
        assert_eq!(j.undecoded_suffix, [9, 8, 7]);
        assert_eq!(j.collision_metadata.len(), 56);
        assert_eq!(j.havok.as_ptr(), b[b.len() - 11..].as_ptr());
    }
    #[test]
    fn rejects_piece_adjacency_outside_table() {
        let mut b = fixture();
        // First piece has zero neighbors. A declared neighbor without a table entry is invalid.
        b[94..98].copy_from_slice(&1u32.to_be_bytes());
        assert!(read_node(&b, false)
            .unwrap_err()
            .to_string()
            .contains("adjacency range"));
    }
    #[test]
    fn odd_region_count_and_independent_ownership_tables() {
        let node = fixture();
        let j = read_node(&node, false).unwrap();
        let mut b = Vec::new();
        let array = |b: &mut Vec<u8>, values: &[u32]| {
            b.extend((values.len() as u32).to_be_bytes());
            for v in values {
                b.extend(v.to_be_bytes());
            }
        };
        array(&mut b, &[2, 3, 4]);
        array(&mut b, &[]);
        b.extend(3u32.to_be_bytes());
        for _ in 0..3 {
            b.extend(0u16.to_be_bytes());
        }
        array(&mut b, &[]);
        array(&mut b, &[]);
        array(&mut b, &[]);
        array(&mut b, &[]);
        array(&mut b, &[]);
        array(&mut b, &[0]);
        let bindings = read_bindings(&b, false, &j.pieces).unwrap();
        assert_eq!(bindings.vertex_regions, [0, 0, 0]);
        bindings.validate_keys(&j.pieces, 12).unwrap();
        let mut pieces = j.pieces;
        pieces[0].collision_keys[0] = 0;
        assert!(bindings.validate_keys(&pieces, 12).is_err());
        b.push(0);
        assert!(read_bindings(&b, false, &pieces).is_err());
    }
    #[test]
    fn rejects_truncated_payload_and_unknown_versions() {
        let mut b = fixture();
        assert!(read_node(&b[..b.len() - 4], false).is_err());
        b[85] = 5;
        assert!(read_node(&b, false).is_err());
    }
}

/// Borrow serialized blocks: custom node tails must not disappear behind NiNode aliases.
pub fn raw_blocks(data: &[u8]) -> Result<Vec<&[u8]>> {
    struct Reader<'a> {
        data: &'a [u8],
        little: bool,
    }
    impl<'a> Reader<'a> {
        fn take(&mut self, n: usize) -> Result<&'a [u8]> {
            anyhow::ensure!(n <= self.data.len(), "Truncated NIF");
            let (head, tail) = self.data.split_at(n);
            self.data = tail;
            Ok(head)
        }
        fn u32(&mut self) -> Result<u32> {
            let b = self.take(4)?.try_into()?;
            Ok(if self.little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        }
        fn u16(&mut self) -> Result<u16> {
            let b = self.take(2)?.try_into()?;
            Ok(if self.little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            })
        }
        fn string(&mut self) -> Result<()> {
            let n = self.u32()? as usize;
            self.take(n)?;
            Ok(())
        }
    }
    let start = data
        .iter()
        .position(|&b| b == b'\n')
        .context("Missing NIF header")?
        + 1;
    let mut r = Reader {
        data: &data[start..],
        little: true,
    };
    anyhow::ensure!(r.u32()? == 0x14060500, "Expected EM1 NIF");
    let little = r.take(1)?[0] != 0;
    r.u32()?;
    let count = r.u32()? as usize;
    r.little = little;
    for _ in 0..r.u16()? {
        r.string()?;
    }
    anyhow::ensure!(count <= r.data.len() / 6, "Invalid block count");
    r.take(count * 2)?;
    let sizes = (0..count).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
    let strings = r.u32()?;
    r.u32()?;
    for _ in 0..strings {
        r.string()?;
    }
    for _ in 1..r.u32()? {
        r.u32()?;
    }
    sizes.into_iter().map(|n| r.take(n as usize)).collect()
}

/// Owned runtime cache. Serialized NIF/Havok byte copies are not retained.
#[derive(Debug)]
pub struct Asset {
    pub mesh_alpha_flags: std::collections::HashMap<u32, u16>,
    pub collision_root_identity: bool,
    pub node: usize,
    pub pieces: Vec<Piece>,
    pub adjacency: Vec<u16>,
    pub bindings: Bindings,
    pub collision: CollisionGeometry,
}
impl Asset {
    /// Owned geometry and routing payload used by memory diagnostics. Shared
    /// material tables and allocator metadata are excluded.
    pub fn owned_payload_bytes(&self) -> usize {
        let pieces = self
            .pieces
            .iter()
            .map(|piece| {
                piece.mesh_links.capacity() * std::mem::size_of::<u32>()
                    + piece.secondary_mesh_links.capacity() * std::mem::size_of::<u32>()
                    + piece.collision_keys.capacity() * std::mem::size_of::<u32>()
            })
            .sum::<usize>();
        let bindings = self.bindings.vertex_streams.capacity() * std::mem::size_of::<u32>()
            + self.bindings.index_streams.capacity() * std::mem::size_of::<u32>()
            + self.bindings.vertex_regions.capacity() * std::mem::size_of::<u16>()
            + self.bindings.triangle_vertex_streams.capacity() * std::mem::size_of::<u32>()
            + self.bindings.triangle_index_streams.capacity() * std::mem::size_of::<u32>()
            + self.bindings.triangle_pieces.capacity() * std::mem::size_of::<u32>()
            + self.bindings.convex_pieces.capacity() * std::mem::size_of::<u32>();
        let collision_vertices = self
            .collision
            .vertices
            .iter()
            .map(|vertices| vertices.capacity() * std::mem::size_of::<[f32; 3]>())
            .sum::<usize>();
        let convex_vertices = self
            .collision
            .convex
            .iter()
            .map(|part| part.vertices.capacity() * std::mem::size_of::<[f32; 3]>())
            .sum::<usize>();
        let triangle_parts = self
            .collision
            .parts
            .iter()
            .map(|part| {
                part.indices.capacity() * std::mem::size_of::<u32>()
                    + part.unreachable_triangles.capacity() * std::mem::size_of::<usize>()
            })
            .sum::<usize>();
        self.pieces.capacity() * std::mem::size_of::<Piece>()
            + pieces
            + self.adjacency.capacity() * std::mem::size_of::<u16>()
            + self.mesh_alpha_flags.capacity() * std::mem::size_of::<(u32, u16)>()
            + bindings
            + collision_vertices
            + convex_vertices
            + self.collision.convex.capacity() * std::mem::size_of::<ConvexPart>()
            + triangle_parts
            + self.collision.parts.capacity() * std::mem::size_of::<TrianglePart>()
    }
}
pub fn load(data: &[u8]) -> Result<Vec<Asset>> {
    use epic_mickey_lib_rs::{
        hkx_file::HkxFile,
        nif_file::{NiObject, NifFile},
    };
    let nif = NifFile::load_bytes(data).map_err(anyhow::Error::msg)?;
    let blocks = raw_blocks(data)?;
    let mut assets = Vec::new();
    for (index, &kind) in nif.block_type_indices.iter().enumerate() {
        if nif.block_types[kind as usize] != "JPSJigsawNode" {
            continue;
        }
        let j = read_node(blocks[index], nif.source_is_little_endian)?;
        let mut mesh_alpha_flags = std::collections::HashMap::new();
        for piece in &j.pieces {
            for &link in piece.mesh_links.iter().chain(&piece.secondary_mesh_links) {
                ensure!(
                    matches!(nif.blocks.get(link as usize), Some(NiObject::NiMesh(_))),
                    "Jigsaw piece references non-mesh block {link}"
                );
                let av = nif.blocks[link as usize]
                    .as_av_object()
                    .context("Jigsaw mesh has no AV data")?;
                let flags = av
                    .property_links
                    .iter()
                    .flatten()
                    .find_map(|&p| match nif.blocks.get(p as usize) {
                        Some(NiObject::NiAlphaProperty(a)) => Some(a.flags),
                        _ => None,
                    })
                    .unwrap_or(0);
                mesh_alpha_flags.insert(link, flags);
            }
        }
        let bindings = read_bindings(j.undecoded_suffix, nif.source_is_little_endian, &j.pieces)?;
        let hkx = HkxFile::load_bytes(j.havok).map_err(anyhow::Error::msg)?;
        let collision = collision_geometry(&nif, &hkx, &bindings)?;
        let section = usize::try_from(hkx.header.contents_section_index)?;
        let root = u32::try_from(hkx.header.contents_section_offset)?;
        let shape = hkx.global_fixups[section]
            .iter()
            .find(|f| f.src_offset == root + 0x34)
            .context("Missing jigsaw MOPP child")?;
        let at = shape.dst_offset as usize + 0xb0;
        let bits = u32::from_be_bytes(
            hkx.section_data[section]
                .get(at..at + 4)
                .context("Missing jigsaw key layout")?
                .try_into()?,
        );
        bindings.validate_keys(&j.pieces, bits)?;
        let av = nif.blocks[index]
            .as_av_object()
            .context("Jigsaw node has no transform")?;
        let t = &av.transform;
        let collision_root_identity = nif.roots.contains(&Some(index as u32))
            && t.scale == 1.
            && t.translation.x == 0.
            && t.translation.y == 0.
            && t.translation.z == 0.
            && t.rotation.rows == [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        assets.push(Asset {
            mesh_alpha_flags,
            collision_root_identity,
            node: index,
            pieces: j.pieces,
            adjacency: j.adjacency,
            bindings,
            collision,
        });
    }
    ensure!(
        !assets.is_empty(),
        "Jigsaw component references a NIF without JPSJigsawNode"
    );
    Ok(assets)
}
