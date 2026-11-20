//! Geometry compatibility for the verified 32-bit PC Havok 2012.1 ABI.
//! Keeps body/part identities, authored point order, radius and welding data.
//! This decodes source resources; it does not implement body lifecycle or dynamics.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct Part {
    pub shape: usize,
    pub subpart: usize,
    pub vertices: Vec<[f32; 3]>,
    /// Empty for a convex point cloud; never fabricate a triangle fan.
    pub indices: Vec<u32>,
    pub radius: f32,
    pub convex: bool,
    pub key_bits: Option<u8>,
    pub list_children: Vec<u32>,
    pub filter: u32,
    pub welding: Option<Arc<super::shape_welding::Welding>>,
    pub welding_offset: usize,
    pub mopp: Option<Arc<super::mopp_query::OwnedCode>>,
}
pub struct Body {
    pub offset: usize,
    pub name: Option<String>,
    pub transform: [f32; 16],
    pub motion_type: u8,
    pub filter: u32,
    pub friction: f32,
    pub restitution: f32,
    /// Preserve the original ABI payload for subsequent motion admission.
    pub motion_record: Arc<[u8]>,
    pub motion: super::havok_2012_motion::Motion,
    pub parts: Vec<Part>,
}
/// Native component selection retains the system's ordered handles and
/// auxiliary objects independently of the decoded collision geometry.
pub struct Resource {
    pub bodies: Arc<Vec<Body>>,
    pub system: Option<Arc<super::physics_system::Imported>>,
    pub system_count: usize,
}
struct View<'a> {
    data: &'a [u8],
    section: usize,
    classes: HashMap<usize, &'a str>,
    pointers: HashMap<usize, usize>,
}
impl<'a> View<'a> {
    fn new(file: &'a HkxFile) -> Result<Self> {
        ensure!(
            file.header.pointer_size == 4
                && file.header.endianness == 1
                && file.header.padding_option == 0
                && file.header.base_format == 1
                && file.header.contents_version == "hk_2012.1.0-r1",
            "Unsupported Havok 2012 geometry ABI"
        );
        let section = file
            .sections
            .iter()
            .position(|s| s.section_tag == "__data__")
            .context("Missing physics data section")?;
        let data = file
            .section_data
            .get(section)
            .context("Missing physics section bytes")?;
        let mut pointers = HashMap::new();
        for fixup in file
            .local_fixups
            .get(section)
            .context("Missing local fixups")?
        {
            ensure!(
                pointers
                    .insert(fixup.src_offset as usize, fixup.dst_offset as usize)
                    .is_none(),
                "Duplicate local pointer"
            );
        }
        for fixup in file
            .global_fixups
            .get(section)
            .context("Missing global fixups")?
        {
            ensure!(
                fixup.dst_section as usize == section,
                "Cross-section physics pointers need admission"
            );
            ensure!(
                pointers
                    .insert(fixup.src_offset as usize, fixup.dst_offset as usize)
                    .is_none(),
                "Duplicate global pointer"
            );
        }
        let mut classes = HashMap::new();
        for fixup in file
            .virtual_fixups
            .get(section)
            .context("Missing virtual fixups")?
        {
            let bytes = file
                .section_data
                .get(fixup.section_index as usize)
                .and_then(|b| b.get(fixup.class_name_offset as usize..))
                .context("Invalid physics class pointer")?;
            let end = bytes
                .iter()
                .position(|&b| b == 0)
                .context("Unterminated physics class")?;
            let name = std::str::from_utf8(&bytes[..end])?;
            ensure!(
                classes.insert(fixup.src_offset as usize, name).is_none(),
                "Duplicate class identity"
            );
        }
        Ok(Self {
            data,
            section,
            classes,
            pointers,
        })
    }
    fn bytes(&self, at: usize, size: usize) -> Result<&'a [u8]> {
        self.data
            .get(at..at.checked_add(size).context("Physics range overflow")?)
            .context("Truncated physics record")
    }
    fn word(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into()?))
    }
    fn half(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at, 2)?.try_into()?))
    }
    fn float(&self, at: usize) -> Result<f32> {
        let value = f32::from_bits(self.word(at)?);
        ensure!(value.is_finite(), "Nonfinite physics float at {at:#x}");
        Ok(value)
    }
    fn vector<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut value = [0.; N];
        for (i, v) in value.iter_mut().enumerate() {
            *v = self.float(at + i * 4)?;
        }
        Ok(value)
    }
    fn pointer(&self, at: usize) -> Result<Option<usize>> {
        if let Some(&target) = self.pointers.get(&at) {
            self.bytes(target, 1)?;
            return Ok(Some(target));
        }
        ensure!(
            self.word(at)? == 0,
            "Unrelocated physics pointer at {at:#x}"
        );
        Ok(None)
    }
    fn required(&self, at: usize) -> Result<usize> {
        self.pointer(at)?
            .with_context(|| format!("Missing physics pointer at {at:#x}"))
    }
    fn class(&self, at: usize) -> Result<&str> {
        self.classes
            .get(&at)
            .copied()
            .with_context(|| format!("Missing shape class at {at:#x}"))
    }
    fn array(&self, at: usize, count: usize, stride: usize) -> Result<()> {
        self.bytes(
            at,
            count
                .checked_mul(stride)
                .context("Physics array overflow")?,
        )?;
        Ok(())
    }
}
fn identity() -> [f32; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}
pub fn point(matrix: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|a| {
        matrix[a] * p[0] + matrix[4 + a] * p[1] + matrix[8 + a] * p[2] + matrix[12 + a]
    })
}
pub(super) fn compose(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let r = i % 4;
        let c = i / 4;
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}
fn placed(vertices: Vec<[f32; 3]>, matrix: &[f32; 16]) -> Result<Vec<[f32; 3]>> {
    let vertices = vertices
        .into_iter()
        .map(|v| point(matrix, v))
        .collect::<Vec<_>>();
    ensure!(
        vertices.iter().flatten().all(|v| v.is_finite()),
        "Collision placement overflow"
    );
    Ok(vertices)
}
/// PC/Wii-U hkpExtendedMeshShape::getChildShape, subpart Qs at +40/+50/+60.
/// Scalar arithmetic is native f32; original-platform rounding parity is pending.
fn subpart_pose(view: &View<'_>, at: usize) -> Result<[f32; 16]> {
    let translation = view.vector::<4>(at + 0x40)?;
    let [x, y, z, w] = view.vector::<4>(at + 0x50)?;
    let scale = view.vector::<4>(at + 0x60)?;
    ensure!(
        (x * x + y * y + z * z + w * w - 1.).abs() < 0.001,
        "Non-unit triangle subpart rotation"
    );
    ensure!(
        scale[..3].iter().all(|v| *v > 0.),
        "Unsupported reflected/zero subpart scale"
    );
    Ok([
        (1. - 2. * (y * y + z * z)) * scale[0],
        2. * (x * y + z * w) * scale[0],
        2. * (x * z - y * w) * scale[0],
        0.,
        2. * (x * y - z * w) * scale[1],
        (1. - 2. * (x * x + z * z)) * scale[1],
        2. * (y * z + x * w) * scale[1],
        0.,
        2. * (x * z + y * w) * scale[2],
        2. * (y * z - x * w) * scale[2],
        (1. - 2. * (x * x + y * y)) * scale[2],
        0.,
        translation[0],
        translation[1],
        translation[2],
        1.,
    ])
}
struct ShapeContext<'a> {
    view: &'a View<'a>,
    codes: super::mopp_query::CodeCache<'a>,
    active: HashSet<usize>,
}
impl ShapeContext<'_> {
    fn shape(
        &mut self,
        at: usize,
        matrix: [f32; 16],
        list_children: Vec<u32>,
        filter: u32,
        mopp: Option<Arc<super::mopp_query::OwnedCode>>,
        out: &mut Vec<Part>,
    ) -> Result<()> {
        ensure!(self.active.insert(at), "Cyclic physics shape graph");
        let view = self.view;
        match view.class(at)? {
            "hkpMoppBvTreeShape" => {
                let code = view.required(at + 0x14)?;
                ensure!(
                    view.class(code)? == "hkpMoppCode",
                    "Invalid MOPP code class"
                );
                let code = self.codes.get(view.section, code)?;
                self.shape(
                    view.required(at + 0x34)?,
                    matrix,
                    list_children,
                    filter,
                    Some(code),
                    out,
                )?;
            }
            "hkpConvexTranslateShape" => {
                let mut local = identity();
                local[12..15].copy_from_slice(&view.vector::<3>(at + 0x20)?);
                self.shape(
                    view.required(at + 0x18)?,
                    compose(&matrix, &local),
                    list_children,
                    filter,
                    mopp,
                    out,
                )?;
            }
            "hkpListShape" => {
                ensure!(
                    view.half(at + 0x26)? == 0,
                    "Disabled PC list children need runtime admission"
                );
                let count = view.word(at + 0x1c)? as usize;
                let children = view.required(at + 0x18)?;
                view.array(children, count, 16)?;
                for i in 0..count {
                    let mut path = list_children.clone();
                    path.push(u32::try_from(i)?);
                    self.shape(
                        view.required(children + i * 16)?,
                        matrix,
                        path,
                        view.word(children + i * 16 + 4)?,
                        mopp.clone(),
                        out,
                    )?;
                }
            }
            "hkpConvexVerticesShape" | "hkpBoxShape" => {
                let hull = if view.class(at)? == "hkpConvexVerticesShape" {
                    super::character_hull::decode_endian(
                        view.data,
                        at,
                        view.required(at + 0x40)?,
                        true,
                    )?
                } else {
                    let half = view.vector::<3>(at + 0x20)?;
                    ensure!(half.iter().all(|v| *v >= 0.), "Negative box extent");
                    super::character_hull::ConvexHull {
                        radius: view.float(at + 0x10)?,
                        vertices: (0..8)
                            .map(|i| {
                                std::array::from_fn(|a| {
                                    if i & (1 << a) == 0 {
                                        -half[a]
                                    } else {
                                        half[a]
                                    }
                                })
                            })
                            .collect(),
                    }
                };
                ensure!(hull.radius >= 0., "Negative convex radius");
                out.push(Part {
                    shape: at,
                    subpart: 0,
                    vertices: placed(hull.vertices, &matrix)?,
                    indices: vec![],
                    radius: hull.radius,
                    convex: true,
                    key_bits: None,
                    list_children,
                    filter,
                    welding: None,
                    welding_offset: 0,
                    mopp,
                });
            }
            "hkpStorageExtendedMeshShape" => {
                self.triangles(at, matrix, list_children, mopp, out)?
            }
            class => anyhow::bail!("Unsupported Havok 2012 shape {class} at {at:#x}"),
        }
        self.active.remove(&at);
        Ok(())
    }
    fn triangles(
        &self,
        at: usize,
        matrix: [f32; 16],
        list_children: Vec<u32>,
        mopp: Option<Arc<super::mopp_query::OwnedCode>>,
        out: &mut Vec<Part>,
    ) -> Result<()> {
        let v = self.view;
        // Native 2012.1 getChildShape: bits+b4, triangles+b8/bc,
        // welding+d0/d4, mode+dc, default filter+e0, radius+e8.
        let count = v.word(at + 0xbc)? as usize;
        ensure!(
            count > 0 && count == v.word(at + 0xf4)? as usize,
            "Triangle subpart/storage counts differ"
        );
        ensure!(
            v.word(at + 0xc8)? == 0,
            "Extended mesh shape subparts need explicit decoding"
        );
        let key_bits = u8::try_from(v.word(at + 0xb4)?)?;
        ensure!(
            (1..=31).contains(&key_bits),
            "Invalid PC shape key bit count"
        );
        let parts = v.required(at + 0xb8)?;
        let storages = v.required(at + 0xf0)?;
        v.array(parts, count, 0x70)?;
        v.array(storages, count, 4)?;
        let radius = v.float(at + 0xe8)?;
        ensure!(radius >= 0., "Negative triangle radius");
        let word_count = v.word(at + 0xd4)? as usize;
        let words = if word_count == 0 {
            Vec::new()
        } else {
            let start = v.required(at + 0xd0)?;
            v.array(start, word_count, 2)?;
            (0..word_count)
                .map(|i| v.half(start + i * 2))
                .collect::<Result<Vec<_>>>()?
        };
        let mode = v.bytes(at + 0xdc, 1)?[0];
        ensure!(
            matches!(mode, 0 | 4 | 5 | 6),
            "Unsupported welding mode {mode}"
        );
        let welding = Arc::new(super::shape_welding::Welding { mode, words });
        for subpart in 0..count {
            // Single-subpart native queries use the embedded copy at +20.
            let p = if count == 1 {
                at + 0x20
            } else {
                parts + subpart * 0x70
            };
            let storage = v.required(storages + subpart * 4)?;
            ensure!(
                v.class(storage)? == "hkpStorageExtendedMeshShapeMeshSubpartStorage",
                "Invalid triangle storage class"
            );
            ensure!(
                v.pointer(p + 4)?.is_none(),
                "Per-triangle PC material indices need decoding"
            );
            ensure!(
                v.vector::<4>(p + 0x30)? == [0.; 4],
                "Extruded PC triangles need shape admission"
            );
            let triangles = v.word(p + 0x14)? as usize;
            let vertex_count = v.word(p + 0x1c)? as usize;
            ensure!(
                vertex_count == v.word(storage + 0xc)? as usize,
                "Triangle vertex counts differ"
            );
            let vertex_start = v.required(storage + 8)?;
            let vertex_stride = v.half(p + 0x24)? as usize;
            ensure!(vertex_stride >= 12, "Invalid vertex stride");
            v.array(vertex_start, vertex_count, vertex_stride)?;
            let vertices = (0..vertex_count)
                .map(|i| v.vector::<3>(vertex_start + i * vertex_stride))
                .collect::<Result<Vec<_>>>()?;
            let index_stride = v.half(p + 0x2c)? as usize;
            let kind = v.bytes(p + 0x2e, 1)?[0];
            let (width, field) = match kind {
                1 => (1, 0x14),
                2 => (2, 0x20),
                3 => (4, 0x2c),
                _ => anyhow::bail!("Unsupported PC index type {kind}"),
            };
            ensure!(index_stride >= 3 * width, "Invalid triangle index stride");
            let start = v.required(storage + field)?;
            let stored_count = v.word(storage + field + 4)? as usize;
            v.array(start, stored_count, width)?;
            ensure!(
                triangles
                    .checked_mul(index_stride)
                    .is_some_and(|bytes| bytes <= stored_count * width),
                "Truncated triangle indices"
            );
            let flip = v.bytes(p + 0x2f, 1)?[0] as usize;
            ensure!(flip <= 1, "Invalid triangle flip mask");
            let mut indices = Vec::with_capacity(
                triangles
                    .checked_mul(3)
                    .context("Triangle index allocation overflow")?,
            );
            for triangle in 0..triangles {
                let first = start + triangle * index_stride;
                let swapped = triangle & flip;
                for lane in [0, swapped + 1, (swapped ^ 1) + 1] {
                    let at = first + lane * width;
                    let index = match width {
                        1 => v.bytes(at, 1)?[0] as u32,
                        2 => v.half(at)? as u32,
                        _ => v.word(at)?,
                    };
                    ensure!(
                        (index as usize) < vertex_count,
                        "Triangle index exceeds vertices"
                    );
                    indices.push(index);
                }
            }
            let welding_offset = v.word(p + 0x28)? as usize;
            if !welding.words.is_empty() {
                ensure!(
                    welding_offset
                        .checked_add(triangles)
                        .is_some_and(|end| end <= welding.words.len()),
                    "Truncated triangle welding"
                );
            }
            let pose = compose(&matrix, &subpart_pose(v, p)?);
            out.push(Part {
                shape: at,
                subpart,
                vertices: placed(vertices, &pose)?,
                indices,
                radius,
                convex: false,
                key_bits: Some(key_bits),
                list_children: list_children.clone(),
                filter: v.word(at + 0xe0)?,
                welding: Some(welding.clone()),
                welding_offset,
                mopp: mopp.clone(),
            });
        }
        Ok(())
    }
}
pub fn load(file: &HkxFile) -> Result<Vec<Body>> {
    let view = View::new(file)?;
    let mut offsets = view
        .classes
        .iter()
        .filter_map(|(&at, &name)| (name == "hkpRigidBody").then_some(at))
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    ensure!(
        !offsets.is_empty(),
        "No rigid bodies in PC physics resource"
    );
    decode_bodies(file, &view, offsets)
}
/// EM2 component loaders clone the first system returned by the selected
/// hkpPhysicsData variant. Virtual-fixup/file order is not body-array order.
pub fn load_selected(file: &HkxFile) -> Result<Resource> {
    let view = View::new(file)?;
    let systems = super::physics_system::import(file)?;
    let system_count = systems.len();
    let selected = super::physics_system::select_first(file, &systems)?;
    let system = systems
        .into_iter()
        .find(|system| Some(system.offset) == selected)
        .map(Arc::new);
    let offsets = system
        .as_ref()
        .map(|system| {
            system
                .system
                .bodies
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut unique = HashSet::new();
    ensure!(
        offsets.iter().all(|offset| unique.insert(*offset)),
        "Aliased body slots require component clone/initialization compatibility"
    );
    let bodies = Arc::new(decode_bodies(file, &view, offsets)?);
    Ok(Resource {
        bodies,
        system,
        system_count,
    })
}
fn decode_bodies(file: &HkxFile, view: &View<'_>, offsets: Vec<usize>) -> Result<Vec<Body>> {
    let mut context = ShapeContext {
        view,
        codes: super::mopp_query::CodeCache::new(file),
        active: HashSet::new(),
    };
    let mut bodies = Vec::new();
    for offset in offsets {
        ensure!(
            view.class(offset)? == "hkpRigidBody",
            "System slot does not reference a rigid body"
        );
        let transform = view.vector::<16>(offset + 0xf0)?;
        ensure!(transform[15] == 1., "Invalid body transform padding");
        let motion_type = view.bytes(offset + 0xe8, 1)?[0];
        ensure!((1..=7).contains(&motion_type), "Invalid body motion type");
        let filter = view.word(offset + 0x2c)?;
        let name = view
            .pointer(offset + 0x78)?
            .map(|at| -> Result<String> {
                let tail = &view.data[at..];
                let end = tail
                    .iter()
                    .position(|&b| b == 0)
                    .context("Unterminated body name")?;
                Ok(std::str::from_utf8(&tail[..end])?.into())
            })
            .transpose()?;
        let mut parts = Vec::new();
        context.shape(
            view.required(offset + 0x10)?,
            identity(),
            vec![],
            filter,
            None,
            &mut parts,
        )?;
        ensure!(!parts.is_empty(), "Rigid body has no collision parts");
        bodies.push(Body {
            offset,
            name,
            transform,
            motion_type,
            filter,
            friction: view.float(offset + 0x90)?,
            restitution: view.float(offset + 0x94)?,
            motion_record: Arc::from(view.bytes(offset + 0xe0, 0x120)?),
            motion: super::havok_2012_motion::decode(view.bytes(offset + 0xe0, 0x120)?)
                .with_context(|| format!("Body {offset:#x} motion"))?,
            parts,
        });
    }
    Ok(bodies)
}

#[cfg(test)]
mod pc_geometry_tests {
    use super::*;
    use epic_mickey_lib_rs::hkx_file::{
        HkxGlobalFixup, HkxHeader, HkxLocalFixup, HkxSectionHeader, HkxVirtualFixup,
    };
    fn fixture() -> HkxFile {
        let mut data = vec![0; 0xa00];
        let word =
            |data: &mut [u8], at: usize, n: u32| data[at..at + 4].copy_from_slice(&n.to_le_bytes());
        let float =
            |data: &mut [u8], at: usize, f: f32| data[at..at + 4].copy_from_slice(&f.to_le_bytes());
        data[0x1e8] = 5;
        for (i, v) in identity().iter().enumerate() {
            float(&mut data, 0x1f0 + i * 4, *v);
        }
        float(&mut data, 0x190, 0.4);
        for (at, value) in [
            (0x4b4, 12),
            (0x4bc, 1),
            (0x4f4, 1),
            (0x4d4, 1),
            (0x434, 1),
            (0x43c, 3),
            (0x72c, 3),
            (0x744, 3),
        ] {
            word(&mut data, at, value);
        }
        // Embedded subpart is authoritative for a single-part mesh.
        // The non-authoritative array copy intentionally disagrees.
        word(&mut data, 0x614, 999);
        data[0x444..0x446].copy_from_slice(&16u16.to_le_bytes());
        data[0x44c..0x44e].copy_from_slice(&6u16.to_le_bytes());
        data[0x44e] = 2;
        for (i, f) in [2f32, 3., 4., 0., 0., 0., 0., 1., 2., 3., 4., 1.]
            .iter()
            .enumerate()
        {
            float(&mut data, 0x460 + i * 4, *f);
        }
        float(&mut data, 0x4e8, 0.05);
        data[0x4dc] = 5;
        for (i, p) in [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]]
            .iter()
            .enumerate()
        {
            for (a, f) in p.iter().enumerate() {
                float(&mut data, 0x800 + i * 16 + a * 4, *f);
            }
        }
        for (i, n) in [2u16, 0, 1].iter().enumerate() {
            data[0x900 + i * 2..0x902 + i * 2].copy_from_slice(&n.to_le_bytes());
        }
        data[0x950..0x952].copy_from_slice(&0x1234u16.to_le_bytes());
        let mut names = Vec::new();
        let mut virtuals = Vec::new();
        for (at, class) in [
            (0x100, "hkpRigidBody"),
            (0x400, "hkpStorageExtendedMeshShape"),
            (0x720, "hkpStorageExtendedMeshShapeMeshSubpartStorage"),
        ] {
            let name = names.len() as u32;
            names.extend_from_slice(class.as_bytes());
            names.push(0);
            virtuals.push(HkxVirtualFixup {
                src_offset: at,
                section_index: 1,
                class_name_offset: name,
            });
        }
        HkxFile {
            header: HkxHeader {
                pointer_size: 4,
                endianness: 1,
                padding_option: 0,
                base_format: 1,
                contents_version: "hk_2012.1.0-r1".into(),
                ..Default::default()
            },
            sections: vec![HkxSectionHeader {
                section_tag: "__data__".into(),
                ..Default::default()
            }],
            section_data: vec![data, names],
            local_fixups: vec![vec![
                (0x4b8, 0x600),
                (0x4f0, 0x700),
                (0x4d0, 0x950),
                (0x728, 0x800),
                (0x740, 0x900),
            ]
            .into_iter()
            .map(|(s, d)| HkxLocalFixup {
                src_offset: s,
                dst_offset: d,
            })
            .collect()],
            global_fixups: vec![vec![(0x110, 0x400), (0x700, 0x720)]
                .into_iter()
                .map(|(s, d)| HkxGlobalFixup {
                    src_offset: s,
                    dst_section: 0,
                    dst_offset: d,
                })
                .collect()],
            virtual_fixups: vec![virtuals],
            ..Default::default()
        }
    }
    #[test]
    fn embedded_subpart_transform_indices_and_welding_are_preserved() -> Result<()> {
        let file = fixture();
        let bodies = load(&file)?;
        let part = &bodies[0].parts[0];
        assert_eq!(part.vertices, [[2., 3., 4.], [4., 3., 4.], [2., 6., 4.]]);
        assert_eq!(part.indices, [2, 0, 1]);
        assert_eq!(part.welding.as_ref().unwrap().words, [0x1234]);
        assert_eq!(part.radius, 0.05);
        assert_eq!(bodies[0].motion_type, 5);
        Ok(())
    }
    #[test]
    fn invalid_indices_and_unimplemented_extrusion_fail() {
        let mut file = fixture();
        file.section_data[0][0x900..0x902].copy_from_slice(&3u16.to_le_bytes());
        assert!(load(&file).is_err());
        let mut file = fixture();
        file.section_data[0][0x450..0x454].copy_from_slice(&1f32.to_le_bytes());
        assert!(load(&file).is_err());
    }
    #[test]
    fn component_selection_uses_authored_order_without_decoding_unused_shapes() -> Result<()> {
        let mut file = fixture();
        file.header.contents_section_index=0;
        file.header.contents_section_offset=0;
        file.section_data[0].resize(0x1100, 0);
        let original = file.section_data[0][0x100..0x300].to_vec();
        file.section_data[0][0xc00..0xe00].copy_from_slice(&original);
        file.section_data[0][0xf00..0x1100].copy_from_slice(&original);
        for (at, value) in [(4, 1u32), (0x60, 2), (0xa0c, 2), (0xa8c, 1)] {
            file.section_data[0][at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        file.section_data[0][0xa40] = 1;
        for (at, name) in [
            (0, "hkRootLevelContainer"),
            (0x50, "hkpPhysicsData"),
            (0xa00, "hkpPhysicsSystem"),
            (0xa80, "hkpPhysicsSystem"),
            (0xc00, "hkpRigidBody"),
            (0xf00, "hkpRigidBody"),
            (0xe00, "UnsupportedUnusedShape"),
        ] {
            let offset = file.section_data[1].len() as u32;
            file.section_data[1].extend_from_slice(name.as_bytes());
            file.section_data[1].push(0);
            file.virtual_fixups[0].push(HkxVirtualFixup {
                src_offset: at,
                section_index: 1,
                class_name_offset: offset,
            });
        }
        for (src, dst) in [(0, 0x20), (0x5c, 0x80), (0xa08, 0xb00), (0xa88, 0xb20)] {
            file.local_fixups[0].push(HkxLocalFixup {
                src_offset: src,
                dst_offset: dst,
            });
        }
        for (src, dst) in [
            (0x28, 0x50),
            (0x80, 0xa00),
            (0x84, 0xa80),
            (0xb00, 0xc00),
            (0xb04, 0x100),
            (0xb20, 0xf00),
            (0xc10, 0x400),
            (0xf10, 0xe00),
        ] {
            file.global_fixups[0].push(HkxGlobalFixup {
                src_offset: src,
                dst_section: 0,
                dst_offset: dst,
            });
        }
        let selected = load_selected(&file)?;
        assert_eq!(selected.system_count, 2);
        assert_eq!(selected.system.as_ref().unwrap().offset, 0xa00);
        assert_eq!(
            selected.bodies.iter().map(|b| b.offset).collect::<Vec<_>>(),
            [0xc00, 0x100]
        );
        assert!(
            load(&file).is_err(),
            "The all-object survey must encounter the unsupported unused shape"
        );
        file.section_data[0][0x60..0x64].copy_from_slice(&0u32.to_le_bytes());
        let empty = load_selected(&file)?;
        assert!(empty.system.is_none());
        assert!(empty.bodies.is_empty());
        Ok(())
    }
}
