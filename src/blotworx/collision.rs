//! Havok 7.0 Wii static collision: extended triangle meshes and convex list children.
//! Follow pointer fixups from rigid body -> MOPP -> extended mesh -> storage.
//! Stored index arrays include padding: triangle stride comes from the subpart.
use super::physics::point;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;

enum ConvexPlacement {
    Translation([f32; 3]),
    Transform(super::native_hk_transform::Transform),
}
impl ConvexPlacement {
    fn apply(&self, vertex: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Translation(value) => std::array::from_fn(|axis| vertex[axis] + value[axis]),
            Self::Transform(value) => super::native_hk_transform::transform_point(
                value,
                [vertex[0], vertex[1], vertex[2], 0.],
            )[..3]
                .try_into()
                .unwrap(),
        }
    }
}
/// Runtime collidable inputs, not necessarily the serialized body's values.
pub struct FilterBody {
    pub shape_type: u32,
    pub filter: u32,
    pub has_jigsaw_component: bool,
}

/// REL 0x24317c. Collection lookup is lazy and may return None when the root
/// does not expose a collection. A supported collection returning zero is a
/// real filter value, not an instruction to fall back to the body filter.
pub fn resolve_filter(
    body: Option<FilterBody>,
    key: u32,
    collection_filter: impl FnOnce(u32) -> Result<Option<u32>>,
) -> Result<u32> {
    let Some(body) = body else {
        return Ok(0);
    };
    if key == u32::MAX {
        return Ok(body.filter);
    }
    let use_collection = match body.shape_type {
        0xa => body.has_jigsaw_component,
        8 | 0x1f => true,
        _ => false,
    };
    if use_collection {
        Ok(collection_filter(key)?.unwrap_or(body.filter))
    } else {
        Ok(body.filter)
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub enum ShapeKeys {
    Simple,
    List { child: u32 },
    Triangles { bits: u8, subpart: u32 },
}
impl ShapeKeys {
    /// EM1 DOL 0x8026fdcc encodes; 0x8026ff40 / 0x802704ec decode.
    /// Bit 31 distinguishes shape subparts from triangle subparts.
    pub fn key(&self, triangle: Option<usize>) -> Result<u32> {
        match *self {
            Self::Simple => {
                ensure!(triangle.is_none(), "Simple shape has no triangle key");
                Ok(u32::MAX)
            }
            Self::List { child } => {
                ensure!(
                    triangle.is_none() && child != u32::MAX,
                    "Invalid list shape key"
                );
                Ok(child)
            }
            Self::Triangles { bits, subpart } => {
                ensure!(
                    (1..=31).contains(&bits),
                    "Invalid Havok subpart bit count {bits}"
                );
                let triangle = u32::try_from(triangle.context("Missing source triangle")?)?;
                let shift = 32 - bits as u32;
                ensure!(
                    subpart < (1u32 << (bits - 1)) && triangle < (1u32 << shift),
                    "Havok shape key overflow"
                );
                Ok((subpart << shift) | triangle)
            }
        }
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub enum ChildFilters {
    Uniform(u32),
    Triangles(Vec<u32>),
}
impl ChildFilters {
    pub fn at(&self, triangle: Option<usize>) -> Result<u32> {
        match self {
            Self::Uniform(v) => Ok(*v),
            Self::Triangles(values) => values
                .get(triangle.context("Missing filter triangle")?)
                .copied()
                .context("Filter triangle out of range"),
        }
    }
}
pub struct StaticMesh {
    pub mopp: Option<std::sync::Arc<super::mopp_query::OwnedCode>>,
    /// Stable within this HKX; combine with asset and placed entity identity.
    pub body_offset: usize,
    pub material: super::body_material::Coefficients,
    pub welding: Option<std::sync::Arc<super::shape_welding::Welding>>,
    pub welding_offset: usize,
    /// One immutable authored graph per HKX, shared by every body/mesh part.
    /// A placed physics system owns its mutable cache copy, not each mesh.
    pub bounds_graph: std::sync::Arc<super::shape_bounds::ShapeGraph>,
    pub bounds_root: usize,
    pub motion: std::sync::Arc<super::body_motion::ImportedMotion>,
    pub properties: std::sync::Arc<[super::body_properties::Entry]>,
    /// hkpEntity name (+0x74). Retail uses this to find a scene-graph binding;
    /// absence must remain distinct from an empty authored name.
    pub body_name: Option<String>,
    /// Serialized collidable filter, before scene/runtime overrides. EM1's
    /// 0x24317c reads collidable+0x1c; rigid body's collidable starts at +0x10.
    pub body_filter_info: u32,
    pub shape_keys: ShapeKeys,
    pub child_filters: ChildFilters,
    pub convex: bool,
    pub motion_type: u8,
    /// Preserve all lanes: native setters and activation caches copy hkVector4.
    pub linear_velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
    /// hkMotionState +0xac/+0xad (body +0x19c): original hkUFloat8 bytes.
    pub encoded_motion_limits: [u8; 2],
    /// Motion swept-transform center of mass in body-local coordinates (+0x170).
    /// Keep separate from world-baked vertices: keyframing rotates this offset.
    pub local_center_of_mass: [f32; 4],
    /// Previous swept COM's W lane; setTransform preserves this base timestamp.
    pub swept_base_time: f32,
    pub subpart_index: usize,
    pub body_transform: [f32; 16],
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub triangle_radius: f32,
}
impl StaticMesh {
    /// Clone only mutable auxiliary state; immutable geometry/property inputs
    /// remain shared. Does not allocate/install a physical body or listener.
    pub fn instantiate_auxiliary(&self) -> Result<super::character_runtime_storage::BodyAuxiliary> {
        Ok(super::character_runtime_storage::BodyAuxiliary {
            properties: super::body_properties::Properties::from_entries(self.properties.to_vec())?,
            listeners: Default::default(),
            user_data: None,
        })
    }
    /// Bind the owning body's runtime welding result, including regenerated
    /// words. Geometry remains in the original body-local frame.
    pub fn prepared_triangle_contact(
        &self,
        triangle: usize,
        prepared: &super::shape_welding::Prepared,
    ) -> Result<TriangleContactShape> {
        use super::shape_welding::Prepared;
        let support = self.triangle_support(triangle)?;
        let (mode, word) = match prepared {
            Prepared::Authored(source) => {
                ensure!(
                    self.welding
                        .as_ref()
                        .is_some_and(|w| std::sync::Arc::ptr_eq(w, source)),
                    "Prepared welding belongs to another mesh"
                );
                ensure!(
                    source.mode == 5,
                    "Unprepared authored welding mode {}",
                    source.mode
                );
                (
                    source.mode,
                    source
                        .subpart(self.welding_offset, self.indices.len() / 3)?
                        .context("Prepared authored welding has no words")?[triangle],
                )
            }
            Prepared::TwoSided(words) => (5, words.word(self.subpart_index, triangle)?),
        };
        Ok(TriangleContactShape {
            support,
            radius: self.triangle_radius,
            mode,
            word,
        })
    }
    /// Bind serialized welding to the original subpart-local triangle index.
    /// Use placed-body prepared welding instead after runtime regeneration.
    /// Missing words are an error, not an assumed smooth edge.
    pub fn authored_triangle_contact(&self, triangle: usize) -> Result<TriangleContactShape> {
        let support = self.triangle_support(triangle)?;
        let welding = self
            .welding
            .as_ref()
            .context("Triangle has no authored welding metadata")?;
        ensure!(
            matches!(welding.mode, 0 | 4 | 5 | 6),
            "Unsupported authored welding mode {}",
            welding.mode
        );
        let word = if welding.mode == 6 {
            0
        } else {
            welding
                .subpart(self.welding_offset, self.indices.len() / 3)?
                .context("Triangle welding requires runtime preparation")?[triangle]
        };
        Ok(TriangleContactShape {
            support,
            radius: self.triangle_radius,
            mode: welding.mode,
            word,
        })
    }
    /// Original extended-mesh child triangle in this mesh's coordinate frame.
    /// Import currently rejects nonzero subpart extrusion; do not use this
    /// zero-extrusion binding after broadening that import contract.
    pub fn triangle_support(&self, triangle: usize) -> Result<super::native_gsk::TriangleSupport> {
        ensure!(
            !self.convex && matches!(self.shape_keys, ShapeKeys::Triangles { .. }),
            "Native triangle support requires an extended-mesh triangle"
        );
        let start = triangle
            .checked_mul(3)
            .context("Triangle support index overflow")?;
        let end = start
            .checked_add(3)
            .context("Triangle support index overflow")?;
        let indices = self
            .indices
            .get(start..end)
            .context("Triangle support index out of range")?;
        let mut vertices = [[0.; 4]; 3];
        for i in 0..3 {
            vertices[i][..3].copy_from_slice(
                self.vertices
                    .get(indices[i] as usize)
                    .context("Triangle support vertex out of range")?,
            );
        }
        Ok(super::native_gsk::TriangleSupport {
            vertices,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        })
    }
}
pub struct TriangleContactShape {
    pub support: super::native_gsk::TriangleSupport,
    pub radius: f32,
    pub mode: u8,
    pub word: u16,
}
impl TriangleContactShape {
    pub fn query(
        &mut self,
        features: &mut super::native_gsk::Features,
        opposing: &mut dyn super::native_gsk::SupportShape,
        opposing_transform: &super::native_hk_transform::Transform,
        triangle_transform: &super::native_hk_transform::Transform,
        opposing_radius: f32,
        tolerance: f32,
        penetration_enabled: bool,
        difference_scratch: &mut [[f32; 4]; 4],
        initial_retry_padding: Option<f32>,
        table: &super::shape_welding::ContactTable,
    ) -> Result<super::shape_welding::ClosestQuery> {
        super::shape_welding::query_triangle_contact(
            features,
            opposing,
            &mut self.support,
            opposing_transform,
            triangle_transform,
            opposing_radius,
            self.radius,
            tolerance,
            penetration_enabled,
            difference_scratch,
            initial_retry_padding,
            self.mode,
            self.word,
            table,
        )
    }
}
pub fn meshes(file: &HkxFile) -> Result<Vec<StaticMesh>> {
    decode_meshes(file, true, None)
}
/// Retain original body-local vertices for runtime rigid bodies. The serialized
/// placement stays in body_transform and must be applied once by the body owner.
pub fn body_local_meshes(file: &HkxFile) -> Result<Vec<StaticMesh>> {
    decode_meshes(file, false, None)
}
/// A serialized address, distinct from the imported ShapeGraph node index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeReference {
    pub section: usize,
    pub offset: usize,
    pub header: super::hk_reference::Header,
}
pub struct PhantomResource {
    pub meshes: Vec<StaticMesh>,
    pub reference: ShapeReference,
}
/// Read the selected body's actual collidable shape pointer (+0x10). The
/// finish-loaded reference header belongs to that shape, not the graph node.
fn phantom_shape_reference(file: &HkxFile, body: usize) -> Result<ShapeReference> {
    ensure!(
        file.header.pointer_size == 4 && file.header.endianness == 0,
        "Phantom header requires big-endian 32-bit data"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Missing phantom data section")?;
    let slot = body
        .checked_add(0x10)
        .context("Phantom body pointer offset overflow")?;
    let mut pointers = file.global_fixups[section]
        .iter()
        .filter(|fix| fix.src_offset as usize == slot);
    let pointer = pointers
        .next()
        .context("Missing phantom body shape pointer")?;
    ensure!(
        pointers.next().is_none(),
        "Ambiguous phantom body shape pointer"
    );
    let section = pointer.dst_section as usize;
    let root = pointer.dst_offset as usize;
    let data = file
        .section_data
        .get(section)
        .context("Invalid phantom shape section")?;
    let end = root
        .checked_add(8)
        .context("Phantom shape header offset overflow")?;
    let bytes = data
        .get(root..end)
        .context("Truncated phantom shape reference header")?;
    Ok(ShapeReference {
        section,
        offset: root,
        header: super::hk_reference::Header {
            memory_size: u16::from_be_bytes(bytes[4..6].try_into()?),
            references: i16::from_be_bytes(bytes[6..8].try_into()?),
        },
    })
}
/// REL377f70..377fe0 selects the first system's first rigid-body shape.
/// Multiple physics-data containers need named-variant lookup, not an arbitrary
/// virtual-fixup ordering choice. Keep that unsupported case explicit.
pub fn phantom_resource(file: &HkxFile) -> Result<PhantomResource> {
    ensure!(
        file.header.pointer_size == 4
            && file.header.endianness == 0
            && file.header.contents_version.starts_with("Havok-7.0"),
        "Phantom resource requires 32-bit big-endian Havok 7.0"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Missing phantom data section")?;
    let data = &file.section_data[section];
    let class = |offset: usize| -> Option<&str> {
        let fix = file.virtual_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == offset)?;
        let bytes = file
            .section_data
            .get(fix.section_index as usize)?
            .get(fix.class_name_offset as usize..)?;
        std::str::from_utf8(bytes.split(|b| *b == 0).next()?).ok()
    };
    let containers: Vec<_> = file.virtual_fixups[section]
        .iter()
        .filter(|f| class(f.src_offset as usize) == Some("hkpPhysicsData"))
        .collect();
    ensure!(containers.len()==1,"Phantom requires an unambiguous hkpPhysicsData container; named-variant selection needs support");
    let word = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            data.get(at..at.checked_add(4).context("Phantom offset overflow")?)
                .context("Truncated phantom resource")?
                .try_into()?,
        ))
    };
    let pointer = |at: usize| -> Result<usize> {
        if let Some(f) = file.local_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
        {
            return Ok(f.dst_offset as usize);
        }
        let f = file.global_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
            .with_context(|| format!("Missing phantom resource pointer {at:#x}"))?;
        ensure!(
            f.dst_section as usize == section,
            "Cross-section phantom resource pointer needs support"
        );
        Ok(f.dst_offset as usize)
    };
    let container = containers[0].src_offset as usize;
    let system_count = word(container + 0x10)? as i32;
    ensure!(system_count > 0, "Phantom physics data has no systems");
    let systems = pointer(container + 0xc)?;
    let system = pointer(systems)?;
    ensure!(
        class(system) == Some("hkpPhysicsSystem"),
        "Invalid first phantom physics system"
    );
    let body_count = word(system + 0xc)? as i32;
    ensure!(body_count > 0, "First phantom physics system has no bodies");
    let bodies = pointer(system + 8)?;
    let body = pointer(bodies)?;
    ensure!(
        class(body) == Some("hkpRigidBody"),
        "Invalid first phantom rigid body"
    );
    let reference = phantom_shape_reference(file, body)?;
    let meshes = decode_meshes(file, false, Some(body))?;
    Ok(PhantomResource { meshes, reference })
}
fn decode_meshes(
    file: &HkxFile,
    bake_body_transform: bool,
    selected_body: Option<usize>,
) -> Result<Vec<StaticMesh>> {
    ensure!(
        file.header.pointer_size == 4
            && file.header.endianness == 0
            && file.header.contents_version.starts_with("Havok-7.0"),
        "Blotworx static collision currently requires 32-bit big-endian Havok 7.0"
    );
    let section = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("No Havok data section")?;
    let data = &file.section_data[section];
    let u32_at = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(
            data.get(at..at + 4)
                .context("Truncated collision structure")?
                .try_into()?,
        ))
    };
    let local = |at: usize| -> Result<usize> {
        file.local_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
            .map(|f| f.dst_offset as usize)
            .context(format!("Missing local pointer at {at:#x}"))
    };
    let global = |at: usize| -> Result<usize> {
        let f = file.global_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
            .context(format!("Missing shape pointer at {at:#x}"))?;
        ensure!(
            f.dst_section as usize == section,
            "Cross-section collision shape unsupported"
        );
        Ok(f.dst_offset as usize)
    };
    let pointer = |at: usize| -> Result<Option<usize>> {
        if let Some(f) = file.local_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)
        {
            return Ok(Some(f.dst_offset as usize));
        }
        if file.global_fixups[section]
            .iter()
            .any(|f| f.src_offset as usize == at)
        {
            return global(at).map(Some);
        }
        ensure!(u32_at(at)? == 0, "Unresolved collision pointer at {at:#x}");
        Ok(None)
    };
    let class = |at: usize| -> Option<&str> {
        let f = file.virtual_fixups[section]
            .iter()
            .find(|f| f.src_offset as usize == at)?;
        let bytes = file
            .section_data
            .get(f.section_index as usize)?
            .get(f.class_name_offset as usize..)?
            .split(|b| *b == 0)
            .next()?;
        std::str::from_utf8(bytes).ok()
    };
    let bodies = file.virtual_fixups[section]
        .iter()
        .filter(|f| class(f.src_offset as usize) == Some("hkpRigidBody"))
        .filter(|f| selected_body.is_none_or(|selected| f.src_offset as usize == selected))
        .map(|f| f.src_offset as usize)
        .collect::<Vec<_>>();
    let roots = bodies
        .iter()
        .map(|&body| global(body + 0x10).map(|shape| (section, shape)))
        .collect::<Result<Vec<_>>>()?;
    let (bounds_graph, bounds_roots) = super::shape_bounds_import::import(file, &roots)
        .context("Importing rigid-body bounds graph")?;
    let bounds_graph = std::sync::Arc::new(bounds_graph);
    let mut output = vec![];
    let mut welding_by_shape = std::collections::HashMap::new();
    let mut mopp_codes = super::mopp_query::CodeCache::new(file);
    let empty_properties: std::sync::Arc<[super::body_properties::Entry]> =
        std::sync::Arc::from([]);
    let mut property_arrays = std::collections::BTreeMap::new();
    for (body, bounds_root) in bodies.into_iter().zip(bounds_roots) {
        let property_count = u32_at(body + 0x7c)? as usize;
        ensure!(
            property_count <= i32::MAX as usize,
            "Invalid Havok body property count"
        );
        let properties = if property_count == 0 {
            empty_properties.clone()
        } else {
            let at =
                pointer(body + 0x78)?.context("Nonempty body properties have no array pointer")?;
            let cached = property_arrays.entry((at, property_count)).or_insert(None);
            if cached.is_none() {
                *cached = Some(std::sync::Arc::<[super::body_properties::Entry]>::from(
                    super::body_properties::decode(data, at, property_count)?,
                ));
            }
            cached.as_ref().unwrap().clone()
        };
        let material = super::body_material::Coefficients::decode(data, body)?;
        let body_name = pointer(body + 0x74)?
            .map(|at| -> Result<String> {
                let bytes = data.get(at..).context("Body name pointer out of range")?;
                let end = bytes
                    .iter()
                    .position(|b| *b == 0)
                    .context("Unterminated rigid-body name")?;
                Ok(std::str::from_utf8(&bytes[..end])
                    .context("Invalid rigid-body name encoding")?
                    .to_owned())
            })
            .transpose()?;
        let body_filter_info = u32_at(body + 0x2c)?;
        // Retail hkClassMember reflection: entity.motion +0xe0;
        // motion.type +8, linearVelocity +0xd0, angularVelocity +0xe0.
        let motion_type = *data
            .get(body + 0xe8)
            .context("Missing rigid-body motion type")?;
        let read_motion = |at: usize| -> Result<super::native_motion_record::MotionRecord> {
            let end = at
                .checked_add(0x120)
                .context("Motion record offset overflow")?;
            Ok(super::native_motion_record::MotionRecord(
                data.get(at..end)
                    .context("Truncated Havok motion record")?
                    .try_into()?,
            ))
        };
        let saved = if matches!(motion_type, 4 | 5) {
            pointer(body + 0x1f8)?.map(read_motion).transpose()?
        } else {
            None
        };
        let motion = std::sync::Arc::new(super::body_motion::ImportedMotion {
            current: read_motion(body + 0xe0)?,
            quality: *data.get(body + 0x2a).context("Missing collision quality")?,
            saved,
        });
        motion.validate().context("Invalid imported body motion")?;
        let vector = |offset: usize| -> Result<[f32; 4]> {
            let values = (0..4)
                .map(|i| u32_at(body + offset + i * 4).map(f32::from_bits))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                values.iter().all(|v| v.is_finite()),
                "Nonfinite rigid-body vector at {offset:#x}"
            );
            Ok(values.try_into().unwrap())
        };
        let linear_velocity = vector(0x1b0)?;
        let angular_velocity = vector(0x1c0)?;
        let encoded_motion_limits = data
            .get(body + 0x19c..body + 0x19e)
            .context("Missing rigid-body motion limits")?
            .try_into()
            .unwrap();
        let local_center_of_mass = vector(0x170)?;
        let swept_base_time = f32::from_bits(u32_at(body + 0x13c)?);
        ensure!(
            local_center_of_mass[3].is_finite() && swept_base_time.is_finite(),
            "Nonfinite rigid-body swept metadata"
        );
        let matrix: Vec<f32> = (0..16)
            .map(|i| u32_at(body + 0xf0 + i * 4).map(f32::from_bits))
            .collect::<Result<_>>()?;
        let matrix: [f32; 16] = matrix.try_into().unwrap();
        ensure!(
            matrix.iter().all(|v| v.is_finite()) && (matrix[15] - 1.).abs() < 0.001,
            "Invalid rigid-body transform"
        );
        let mut shape = global(body + 0x10)?;
        let mut translations = Vec::new();
        let mut wrappers = std::collections::HashSet::new();
        while matches!(
            class(shape),
            Some("hkpConvexTranslateShape" | "hkpConvexTransformShape")
        ) {
            ensure!(wrappers.insert(shape), "Cyclic convex placement wrapper");
            let placement = if class(shape) == Some("hkpConvexTransformShape") {
                ConvexPlacement::Transform(super::native_hk_transform::decode_shape_transform(
                    data, shape,
                )?)
            } else {
                let translation = std::array::from_fn::<_, 3, _>(|axis| {
                    f32::from_bits(u32_at(shape + 0x20 + axis * 4).unwrap_or(f32::NAN.to_bits()))
                });
                ensure!(
                    translation.iter().all(|value| value.is_finite()),
                    "Invalid convex translation"
                );
                ConvexPlacement::Translation(translation)
            };
            translations.push(placement);
            shape = global(shape + 0x18)?;
        }
        let place = |p| {
            if bake_body_transform {
                point(&matrix, p)
            } else {
                p
            }
        };
        if matches!(
            class(shape),
            Some("hkpConvexVerticesShape" | "hkpBoxShape" | "hkpCylinderShape")
        ) {
            let hull = match class(shape) {
                Some("hkpBoxShape") => decode_box(data, shape)?,
                Some("hkpCylinderShape") => decode_cylinder(data, shape)?,
                _ => super::character_hull::decode(data, shape, local(shape + 0x40)?)?,
            };
            output.push(StaticMesh {
                bounds_graph: bounds_graph.clone(),
                bounds_root,
                material,
                welding: None,
                welding_offset: 0,
                mopp: None,
                body_offset: body,
                motion: motion.clone(),
                properties: properties.clone(),
                body_name: body_name.clone(),
                body_filter_info,
                convex: true,
                motion_type,
                linear_velocity,
                angular_velocity,
                shape_keys: ShapeKeys::Simple,
                child_filters: ChildFilters::Uniform(body_filter_info),
                subpart_index: 0,
                body_transform: matrix,
                local_center_of_mass,
                encoded_motion_limits,
                swept_base_time,
                vertices: hull
                    .vertices
                    .into_iter()
                    .map(|mut vertex| {
                        for placement in translations.iter().rev() {
                            vertex = placement.apply(vertex);
                        }
                        place(vertex)
                    })
                    .collect(),
                indices: vec![],
                triangle_radius: hull.radius,
            });
            continue;
        }
        ensure!(
            class(shape) == Some("hkpMoppBvTreeShape"),
            "Unsupported static root shape {:?}",
            class(shape)
        );
        let code = pointer(shape + 0x14)?.context("MOPP shape missing code")?;
        ensure!(
            class(code) == Some("hkpMoppCode"),
            "Invalid MOPP code class"
        );
        let mopp = mopp_codes.get(section, code)?;
        let shape = global(shape + 0x34)?;
        if class(shape) == Some("hkpListShape") {
            // EM1 hkClassMember records 0x66ed30..0x66eda8: childInfo +0x18,
            // numDisabledChildren +0x26, enabledChildren +0x50. ChildInfo is
            // 16 bytes: shape +0, filter +4, shapeSize +8, numChildShapes +12.
            let count = u32_at(shape + 0x1c)? as usize;
            let children = local(shape + 0x18)?;
            let disabled = u16::from_be_bytes(
                data.get(shape + 0x26..shape + 0x28)
                    .context("Truncated list shape")?
                    .try_into()?,
            );
            ensure!(
                disabled == 0,
                "Disabled list children require runtime shape-key support"
            );
            ensure!(
                count > 0
                    && children
                        .checked_add(count.checked_mul(16).context("List child count overflow")?)
                        .is_some_and(|end| end <= data.len()),
                "Invalid list children array"
            );
            for child in 0..count {
                let info = children + child * 16;
                let mut leaf = global(info)?;
                let mut translations = Vec::new();
                let mut wrappers = std::collections::HashSet::new();
                while matches!(
                    class(leaf),
                    Some("hkpConvexTranslateShape" | "hkpConvexTransformShape")
                ) {
                    ensure!(wrappers.insert(leaf), "Cyclic list child placement wrapper");
                    let placement = if class(leaf) == Some("hkpConvexTransformShape") {
                        ConvexPlacement::Transform(
                            super::native_hk_transform::decode_shape_transform(data, leaf)?,
                        )
                    } else {
                        let translation = std::array::from_fn::<_, 3, _>(|axis| {
                            f32::from_bits(
                                u32_at(leaf + 0x20 + axis * 4).unwrap_or(f32::NAN.to_bits()),
                            )
                        });
                        ensure!(
                            translation.iter().all(|v| v.is_finite()),
                            "Invalid list child translation"
                        );
                        ConvexPlacement::Translation(translation)
                    };
                    translations.push(placement);
                    leaf = global(leaf + 0x18)?;
                }
                ensure!(
                    matches!(
                        class(leaf),
                        Some("hkpConvexVerticesShape" | "hkpBoxShape" | "hkpCylinderShape")
                    ),
                    "Unsupported list child {:?} at {leaf:#x}",
                    class(leaf)
                );
                ensure!(
                    u32_at(info + 4)? == 0,
                    "Non-default list child collision filter requires filter routing"
                );
                let mut hull = match class(leaf) {
                    Some("hkpBoxShape") => decode_box(data, leaf)?,
                    Some("hkpCylinderShape") => decode_cylinder(data, leaf)?,
                    _ => super::character_hull::decode(data, leaf, local(leaf + 0x40)?)?,
                };
                for vertex in &mut hull.vertices {
                    for placement in translations.iter().rev() {
                        *vertex = placement.apply(*vertex);
                    }
                }
                output.push(StaticMesh {
                    bounds_graph: bounds_graph.clone(),
                    bounds_root,
                    material,
                    welding: None,
                    welding_offset: 0,
                    mopp: Some(mopp.clone()),
                    body_offset: body,
                    motion: motion.clone(),
                    properties: properties.clone(),
                    body_name: body_name.clone(),
                    body_filter_info,
                    convex: true,
                    motion_type,
                    linear_velocity,
                    angular_velocity,
                    shape_keys: ShapeKeys::List {
                        child: u32::try_from(child)?,
                    },
                    child_filters: ChildFilters::Uniform(u32_at(info + 4)?),
                    subpart_index: child,
                    body_transform: matrix,
                    local_center_of_mass,
                    encoded_motion_limits,
                    swept_base_time,
                    vertices: hull.vertices.into_iter().map(place).collect(),
                    indices: vec![],
                    triangle_radius: hull.radius,
                });
            }
            continue;
        }
        ensure!(
            class(shape) == Some("hkpStorageExtendedMeshShape"),
            "Unsupported MOPP child {:?} at {shape:#x} (body {body:#x})",
            class(shape)
        );
        let count = u32_at(shape + 0xb8)? as usize;
        let welding = if let Some(value) = welding_by_shape.get(&shape) {
            std::sync::Arc::clone(value)
        } else {
            let value =
                std::sync::Arc::new(super::shape_welding::Welding::decode(data, shape, |at| {
                    pointer(at)?.context("Missing mesh welding pointer")
                })?);
            welding_by_shape.insert(shape, value.clone());
            value
        };
        let key_bits = u8::try_from(u32_at(shape + 0xb0)?)?;
        let default_filter = u32_at(shape + 0xdc)?;
        // EM1's own hkClassMember reflection records triangleRadius at 0xe4.
        let triangle_radius = f32::from_bits(u32_at(shape + 0xe4)?);
        ensure!(
            triangle_radius.is_finite() && triangle_radius >= 0.,
            "Invalid extended mesh triangle radius"
        );
        ensure!(
            count > 0 && count <= 1024 && count == u32_at(shape + 0xf4)? as usize,
            "Mismatched triangle subparts / storage"
        );
        let parts = local(shape + 0xb4)?;
        let storages = local(shape + 0xf0)?;
        let scale: [f32; 3] =
            std::array::from_fn(|i| f32::from_bits(u32_at(shape + 0x80 + i * 4).unwrap_or(0)));
        ensure!(
            scale.iter().all(|v| v.is_finite() && *v > 0.),
            "Invalid mesh shape scale"
        );
        for i in 0..count {
            let part = parts + i * 0x60;
            let storage = global(storages + i * 4)?;
            ensure!(
                class(storage) == Some("hkpStorageExtendedMeshShapeMeshSubpartStorage"),
                "Unexpected mesh storage"
            );
            // +4c is the supported welding offset (802701a4), not a transform.
            ensure!(
                data.get(part + 0x24..part + 0x44)
                    .is_some_and(|b| b.iter().all(|v| *v == 0))
                    && data
                        .get(part + 0x50..part + 0x60)
                        .is_some_and(|b| b.iter().all(|v| *v == 0)),
                "Transformed or extended triangle subparts are not implemented"
            );
            let triangles = u32_at(part + 0x14)? as usize;
            ensure!(triangles <= 1_000_000, "Invalid collision triangle count");
            let shape_keys = ShapeKeys::Triangles {
                bits: key_bits,
                subpart: u32::try_from(i)?,
            };
            shape_keys.key(Some(triangles.saturating_sub(1)))?;
            // DOL 0x802704ec / 0x8027058c. The serialized subpart stores
            // signed material stride +2, index pointer +4, index stride +8,
            // material count +a, and material pointer +c.
            let child_filters = if let Some(index_base) = pointer(part + 4)? {
                let width = match data[part + 1] {
                    1 => 1,
                    2 => 2,
                    v => anyhow::bail!("Unsupported material index type {v}"),
                };
                let stride = u16::from_be_bytes(data[part + 8..part + 10].try_into()?) as usize;
                let material_count =
                    u16::from_be_bytes(data[part + 10..part + 12].try_into()?) as usize;
                let material_stride = i16::from_be_bytes(data[part + 2..part + 4].try_into()?);
                ensure!(material_count > 0, "Empty collision material table");
                let base = pointer(part + 12)?.context("Missing collision materials")?;
                let mut filters = Vec::with_capacity(triangles);
                for triangle in 0..triangles {
                    let at = index_base
                        .checked_add(
                            triangle
                                .checked_mul(stride)
                                .context("Material index overflow")?,
                        )
                        .context("Material index overflow")?;
                    let bytes = data
                        .get(at..at + width)
                        .context("Truncated collision material indices")?;
                    let index = if width == 1 {
                        bytes[0] as usize
                    } else {
                        u16::from_be_bytes(bytes.try_into()?) as usize
                    };
                    ensure!(
                        index < material_count,
                        "Collision material index out of range"
                    );
                    let address =
                        usize::try_from(base as i64 + index as i64 * material_stride as i64)
                            .context("Collision material address underflow")?;
                    filters.push(u32_at(address)?);
                }
                if filters.iter().all(|v| Some(v) == filters.first()) {
                    ChildFilters::Uniform(filters.first().copied().unwrap_or(default_filter))
                } else {
                    ChildFilters::Triangles(filters)
                }
            } else {
                ChildFilters::Uniform(default_filter)
            };
            let vertex_count = u32_at(storage + 0xc)? as usize;
            ensure!(
                triangles <= 1_000_000
                    && vertex_count <= 1_000_000
                    && vertex_count == u32_at(part + 0x20)? as usize,
                "Invalid collision counts"
            );
            let stride = u32_at(part + 0x44)? as usize;
            let kind = *data.get(part + 0x48).context("Missing index type")?;
            let (width, pointer, count_offset) = match kind {
                1 => (1, 0x14, 0x18),
                2 => (2, 0x20, 0x24),
                3 => (4, 0x2c, 0x30),
                _ => anyhow::bail!("Unsupported triangle index type {kind}"),
            };
            ensure!(
                stride >= width * 3 && stride <= 64,
                "Invalid triangle stride"
            );
            let start = local(storage + pointer)?;
            let array_size = u32_at(storage + count_offset)? as usize;
            ensure!(
                triangles
                    .checked_mul(stride)
                    .is_some_and(|n| n <= array_size * width),
                "Index buffer does not cover all padded triangles"
            );
            let vertices_at = local(storage + 8)?;
            let mut vertices = Vec::with_capacity(vertex_count);
            for v in 0..vertex_count {
                let mut p = [0.; 3];
                for a in 0..3 {
                    p[a] = f32::from_bits(u32_at(vertices_at + v * 16 + a * 4)?) * scale[a];
                }
                ensure!(
                    p.iter().all(|v| v.is_finite()),
                    "Nonfinite collision vertex"
                );
                vertices.push(place(p));
            }
            let mut indices = Vec::with_capacity(triangles * 3);
            for t in 0..triangles {
                for a in 0..3 {
                    let at = start + t * stride + a * width;
                    let bytes = data.get(at..at + width).context("Truncated index buffer")?;
                    let index = match width {
                        1 => bytes[0] as u32,
                        2 => u16::from_be_bytes(bytes.try_into()?) as u32,
                        _ => u32::from_be_bytes(bytes.try_into()?),
                    };
                    ensure!(
                        (index as usize) < vertex_count,
                        "Collision index out of bounds"
                    );
                    indices.push(index);
                }
            }
            output.push(StaticMesh {
                bounds_graph: bounds_graph.clone(),
                bounds_root,
                material,
                mopp: Some(mopp.clone()),
                welding: Some(welding.clone()),
                welding_offset: {
                    let offset = u32_at(part + 0x4c)? as usize;
                    welding.subpart(offset, indices.len() / 3)?;
                    offset
                },
                body_offset: body,
                motion: motion.clone(),
                properties: properties.clone(),
                body_name: body_name.clone(),
                body_filter_info,
                convex: false,
                shape_keys,
                child_filters,
                motion_type,
                linear_velocity,
                angular_velocity,
                subpart_index: i,
                body_transform: matrix,
                local_center_of_mass,
                encoded_motion_limits,
                swept_base_time,
                triangle_radius,
                vertices,
                indices,
            });
        }
    }
    ensure!(
        !output.is_empty(),
        "No supported rigid-body collision found"
    );
    Ok(output)
}
/// Retail hkClassMember at DOL file offset 0x66f700 declares halfExtents
/// (vector4) at +0x20. The inherited convex radius remains at +0x10.
/// Keep the core corners and radius separate; expanding corners by the radius
/// would turn the rounded Minkowski boundary into a different shape.
fn decode_box(data: &[u8], shape: usize) -> Result<super::character_hull::ConvexHull> {
    let super::shape_bounds::BoxShape { half, radius } =
        super::shape_bounds::BoxShape::decode(data, shape)?;
    let vertices = (0..8)
        .map(|corner| {
            std::array::from_fn(|axis| {
                if corner & (1 << axis) == 0 {
                    -half[axis]
                } else {
                    half[axis]
                }
            })
        })
        .collect();
    Ok(super::character_hull::ConvexHull { vertices, radius })
}
/// Retail hkpCylinderShape +14 radius, +20/+30 endpoints (DOL reflection
/// table at file 0x66fd28). The current backend uses a circumscribed four-sided
/// approximation; exact Havok support/tessellation remains unfinished.
fn decode_cylinder(data: &[u8], shape: usize) -> Result<super::character_hull::ConvexHull> {
    let cylinder = super::shape_bounds::CylinderShape::decode(data, shape)?;
    Ok(super::character_hull::ConvexHull {
        vertices: cylinder.vertices(4)?,
        radius: cylinder.convex_radius,
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn nested_convex_placement_applies_inner_transform_before_outer_translation() {
        let inner = ConvexPlacement::Transform(super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        ));
        let outer = ConvexPlacement::Translation([5., -3., 2.]);
        assert_eq!(outer.apply(inner.apply([1., 2., 3.])), [13., 18., 35.]);
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_clockwork_transformed_collision_keeps_bounds_and_vertices_together(
    ) -> anyhow::Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let pak = crate::pack_io::read_filtered(
            std::fs::read(
                std::path::Path::new(&root).join("packfiles/sw_clockworkboss_shared.pak"),
            )?,
            |_, path| {
                path.to_ascii_lowercase()
                    .ends_with("/sw_clockwork_backwall_static_simple.hkx_wii")
            },
        )?;
        let asset = pak
            .pak
            .files
            .iter()
            .find(|file| !file.data.is_empty())
            .context("Missing clockwork collision")?;
        let file = HkxFile::load_bytes(&asset.data).map_err(anyhow::Error::msg)?;
        let local = body_local_meshes(&file)?;
        let baked = meshes(&file)?;
        ensure!(
            !local.is_empty() && local.len() == baked.len(),
            "Missing collision parts"
        );
        for (local, baked) in local.iter().zip(&baked) {
            let mut graph = (*local.bounds_graph).clone();
            let bounds = graph.aabb(local.bounds_root, &local.body_transform, 0.)?;
            assert_eq!(
                local.triangle_radius.to_bits(),
                baked.triangle_radius.to_bits()
            );
            for (local_vertex, vertex) in local.vertices.iter().zip(&baked.vertices) {
                assert_eq!(point(&local.body_transform, *local_vertex), *vertex);
                for axis in 0..3 {
                    ensure!(
                        vertex[axis] >= bounds[0][axis] - 0.0001
                            && vertex[axis] <= bounds[1][axis] + 0.0001,
                        "Transformed collision vertex escapes authored bounds"
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn box_core_and_radius_remain_separate() -> anyhow::Result<()> {
        let mut data = vec![0u8; 0x30];
        for (at, value) in [(0x10, 0.05f32), (0x20, 1.), (0x24, 2.), (0x28, 3.)] {
            data[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let hull = decode_box(&data, 0)?;
        assert_eq!(hull.radius, 0.05);
        assert_eq!(hull.vertices.len(), 8);
        assert!(hull.vertices.contains(&[-1., -2., -3.]));
        assert!(hull.vertices.contains(&[1., 2., 3.]));
        assert!(decode_box(&data[..0x2f], 0).is_err());
        data[0x20..0x24].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(decode_box(&data, 0).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_wheel_platform_collision() -> anyhow::Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let path = std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak");
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(path)?);
        let mut report = Vec::new();
        for name in [
            "gv_zonea_platform01_dynamic_mesh.hkx",
            "gv_zonea_wheel01_dynamic_hull.hkx",
        ] {
            let file = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().contains(name))
                .context("Missing wheel/platform collision asset")?;
            let hkx = HkxFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            let geometry = meshes(&hkx).with_context(|| file.path.clone())?;
            ensure!(!geometry.is_empty(), "Empty wheel/platform collision");
            let model_name = if name.contains("platform") {
                "/gv_zonea_platform01.nif_wii"
            } else {
                "/gv_zonea_wheel01.nif_wii"
            };
            let model = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(model_name))
                .context("Missing model for body binding audit")?;
            let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&model.data)
                .map_err(anyhow::Error::msg)?;
            let hierarchy = super::super::scene_bounds::Hierarchy::load(&nif)?;
            let bindings=geometry.iter().map(|part|->anyhow::Result<serde_json::Value> {
                let matches=if let Some(name)=part.body_name.as_deref().filter(|s|!s.is_empty()) {
                    hierarchy.roots.iter().flatten().map(|root|hierarchy.find_named_node(*root,name))
                        .collect::<anyhow::Result<Vec<_>>>()?
                } else {vec![]};
                ensure!(matches.iter().all(Option::is_none),"Wheel/platform unexpectedly binds to a model node");
                Ok(serde_json::json!({"body":part.body_offset,"name":part.body_name,"model_roots":matches}))
            }).collect::<anyhow::Result<Vec<_>>>()?;
            report.push(serde_json::json!({"binding_model":model.path,"bindings":bindings,
                "model_nodes":hierarchy.nodes.iter().map(|(id,n)|serde_json::json!({"id":id,"name":n.name,"class":n.class_name})).collect::<Vec<_>>()}));
            report.push(serde_json::json!({"asset":file.path,"parts":geometry.iter().map(|m|
                serde_json::json!({"body":m.body_offset,"name":m.body_name,"motion_type":m.motion_type,"filter":m.body_filter_info,
                    "convex":m.convex,"vertices":m.vertices.len(),"triangles":m.indices.len()/3,
                    "shape_keys":m.shape_keys,"body_transform":m.body_transform,"radius":m.triangle_radius})).collect::<Vec<_>>()}));
        }
        std::fs::write(
            "target/original-wheel-platform-collision.json",
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("Decoded wheel and platform collision resources without changing motion type");
        Ok(())
    }
    use super::*;
    use epic_mickey_lib_rs::hkx_file::*;
    #[test]
    fn filter_resolution_preserves_jigsaw_guard_and_zero_child_filters() -> Result<()> {
        let body = |shape_type, has_jigsaw_component| {
            Some(FilterBody {
                shape_type,
                filter: 0x8000,
                has_jigsaw_component,
            })
        };
        let forbidden = |_| -> Result<Option<u32>> {
            panic!("Collection should not be queried");
        };
        assert_eq!(resolve_filter(None, 0, forbidden)?, 0);
        assert_eq!(
            resolve_filter(body(0xa, true), u32::MAX, forbidden)?,
            0x8000
        );
        assert_eq!(resolve_filter(body(0xa, false), 5, forbidden)?, 0x8000);
        assert_eq!(resolve_filter(body(4, true), 5, forbidden)?, 0x8000);
        for shape_type in [8, 0xa, 0x1f] {
            assert_eq!(
                resolve_filter(body(shape_type, true), 5, |key| {
                    assert_eq!(key, 5);
                    Ok(Some(0))
                })?,
                0
            );
            assert_eq!(
                resolve_filter(body(shape_type, true), 5, |_| Ok(None))?,
                0x8000
            );
            assert!(resolve_filter(body(shape_type, true), 5, |_| anyhow::bail!(
                "Unsupported collection"
            ))
            .is_err());
        }
        Ok(())
    }
    #[test]
    fn shape_keys_reserve_the_high_bit_and_check_both_index_fields() {
        let keys = ShapeKeys::Triangles {
            bits: 12,
            subpart: 7,
        };
        assert_eq!(keys.key(Some(123)).unwrap(), 0x0070007b);
        assert!(keys.key(Some(1 << 20)).is_err());
        assert!(ShapeKeys::Triangles {
            bits: 12,
            subpart: 2048
        }
        .key(Some(0))
        .is_err());
        assert!(ShapeKeys::Triangles {
            bits: 0,
            subpart: 0
        }
        .key(Some(0))
        .is_err());
        assert_eq!(ShapeKeys::List { child: 5 }.key(None).unwrap(), 5);
        assert_eq!(ShapeKeys::Simple.key(None).unwrap(), u32::MAX);
        assert!(ShapeKeys::Simple.key(Some(0)).is_err());
    }
    #[test]
    fn triangle_material_filters_follow_authored_indices_and_default() {
        let mut file = fixture();
        file.section_data[0][0x3dc..0x3e0].copy_from_slice(&0x12340000u32.to_be_bytes());
        assert_eq!(
            meshes(&file).unwrap()[0].child_filters.at(Some(0)).unwrap(),
            0x12340000
        );
        let data = &mut file.section_data[0];
        data[0x481] = 1;
        data[0x482..0x484].copy_from_slice(&8i16.to_be_bytes());
        data[0x488..0x48a].copy_from_slice(&1u16.to_be_bytes());
        data[0x48a..0x48c].copy_from_slice(&2u16.to_be_bytes());
        data[0x680..0x682].copy_from_slice(&[1, 0]);
        data[0x690..0x694].copy_from_slice(&0x8000u32.to_be_bytes());
        data[0x698..0x69c].copy_from_slice(&0x10000u32.to_be_bytes());
        file.local_fixups[0].extend([(0x484, 0x680), (0x48c, 0x690)].map(
            |(src_offset, dst_offset)| HkxLocalFixup {
                src_offset,
                dst_offset,
            },
        ));
        let parts = meshes(&file).unwrap();
        assert_eq!(parts[0].child_filters.at(Some(0)).unwrap(), 0x10000);
        assert_eq!(parts[0].child_filters.at(Some(1)).unwrap(), 0x8000);
        file.section_data[0][0x680] = 2;
        assert!(meshes(&file).is_err());
    }
    #[test]
    #[ignore = "requires BLOTWORX_COMPOUND_HKX from an imported game"]
    fn imported_portal_compound_keeps_both_convex_children() -> Result<()> {
        let path =
            std::env::var_os("BLOTWORX_COMPOUND_HKX").context("Set BLOTWORX_COMPOUND_HKX")?;
        let bytes = std::fs::read(path)?;
        let file = HkxFile::load_bytes(&bytes).map_err(anyhow::Error::msg)?;
        let parts = meshes(&file)?;
        ensure!(
            parts.len() == 2 && parts.iter().all(|p| p.convex && p.indices.is_empty()),
            "Expected portal's two convex children"
        );
        ensure!(
            parts[0].subpart_index == 0 && parts[1].subpart_index == 1,
            "Child identity lost"
        );
        ensure!(
            parts.iter().all(|p| p.body_offset == 0x2c0),
            "Body identity lost"
        );
        Ok(())
    }
    fn fixture() -> HkxFile {
        let mut data = vec![0u8; 0x700];
        let mut write =
            |at: usize, value: u32| data[at..at + 4].copy_from_slice(&value.to_be_bytes());
        for i in [0, 5, 10, 15] {
            write(0xf0 + i * 4, 1f32.to_bits());
        }
        write(0xe8, 5 << 24);
        write(0x2c, 0x12348000);
        write(0x1b0, 2f32.to_bits());
        write(0x1c4, 3f32.to_bits());
        write(0x120, 10f32.to_bits());
        write(0x124, 2f32.to_bits());
        write(0x2bc, 1f32.to_bits());
        write(0x2c4, 1);
        // One subpart with two triangles, each padded by a deliberately invalid fourth index.
        write(0x300 + 0xb8, 1);
        write(0x300 + 0xb0, 12);
        write(0x300 + 0xf4, 1);
        write(0x300 + 0xe4, 0.05f32.to_bits());
        for a in 0..4 {
            write(0x380 + a * 4, 1f32.to_bits());
        }
        write(0x480 + 0x14, 2);
        write(0x480 + 0x20, 4);
        write(0x480 + 0x44, 16);
        write(0x480 + 0x48, 0x03000000);
        write(0x500 + 0xc, 4);
        write(0x500 + 0x30, 8);
        for (i, p) in [[0f32, 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]
            .iter()
            .enumerate()
        {
            for a in 0..3 {
                write(0x600 + i * 16 + a * 4, p[a].to_bits());
            }
        }
        for (i, v) in [0, 1, 2, u32::MAX, 0, 2, 3, u32::MAX]
            .into_iter()
            .enumerate()
        {
            write(0x650 + i * 4, v);
        }
        let mut names = vec![];
        let mut finish = vec![];
        for (offset, name) in [
            (0, "hkpRigidBody"),
            (0x250, "hkpMoppBvTreeShape"),
            (0x300, "hkpStorageExtendedMeshShape"),
            (0x500, "hkpStorageExtendedMeshShapeMeshSubpartStorage"),
            (0x2a0, "hkpMoppCode"),
        ] {
            finish.push(HkxVirtualFixup {
                src_offset: offset,
                section_index: 1,
                class_name_offset: names.len() as u32,
            });
            names.extend(name.as_bytes());
            names.push(0);
        }
        HkxFile {
            header: HkxHeader {
                endianness: 0,
                contents_version: "Havok-7.0.0-r1".into(),
                ..Default::default()
            },
            sections: vec![
                HkxSectionHeader::default(),
                HkxSectionHeader {
                    section_tag: "__classnames__".into(),
                    ..Default::default()
                },
            ],
            class_names: vec![],
            local_fixups: vec![
                [
                    (0x3b4, 0x480),
                    (0x3f0, 0x460),
                    (0x508, 0x600),
                    (0x52c, 0x650),
                    (0x2c0, 0x2e0),
                ]
                .into_iter()
                .map(|(src_offset, dst_offset)| HkxLocalFixup {
                    src_offset,
                    dst_offset,
                })
                .collect(),
                vec![],
            ],
            global_fixups: vec![
                [
                    (0x10, 0x250),
                    (0x284, 0x300),
                    (0x460, 0x500),
                    (0x264, 0x2a0),
                ]
                .into_iter()
                .map(|(src_offset, dst_offset)| HkxGlobalFixup {
                    src_offset,
                    dst_offset,
                    dst_section: 0,
                })
                .collect(),
                vec![],
            ],
            virtual_fixups: vec![finish, vec![]],
            section_data: vec![data, names],
            root_class_name: "hkpRigidBody".into(),
        }
    }
    #[test]
    fn respects_stride_and_body_transform() {
        let m = meshes(&fixture()).unwrap();
        assert_eq!(m[0].indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(m[0].vertices[0], [10., 2., 0.]);
        assert_eq!(m[0].vertices[2], [11., 2., 1.]);
        assert_eq!(m[0].triangle_radius, 0.05);
        assert_eq!(m[0].motion_type, 5);
        assert_eq!(m[0].body_filter_info, 0x12348000);
        assert_eq!(m[0].linear_velocity, [2., 0., 0., 0.]);
        assert_eq!(m[0].angular_velocity, [0., 3., 0., 0.]);
        assert_eq!(m[0].body_offset, 0);
        assert_eq!(m[0].body_name, None);
        assert_eq!(m[0].subpart_index, 0);
        assert_eq!(&m[0].body_transform[12..15], &[10., 2., 0.]);
    }
    #[test]
    fn phantom_shape_reference_header_preserves_zero_size_and_signed_count() -> Result<()> {
        let mut file = fixture();
        file.section_data[0][0x254..0x258].copy_from_slice(&[0, 0, 0xff, 0xfe]);
        assert_eq!(
            phantom_shape_reference(&file, 0)?.header,
            super::super::hk_reference::Header {
                memory_size: 0,
                references: -2
            }
        );
        file.section_data[0][0x254..0x258].copy_from_slice(&[1, 0x70, 0, 3]);
        assert_eq!(
            phantom_shape_reference(&file, 0)?.header,
            super::super::hk_reference::Header {
                memory_size: 0x170,
                references: 3
            }
        );
        assert!(phantom_shape_reference(&file, usize::MAX).is_err());
        assert!(phantom_shape_reference(&file, file.section_data[0].len() - 4).is_err());
        Ok(())
    }
    #[test]
    fn phantom_shape_reference_follows_body_pointer_not_bounds_graph_index() -> Result<()> {
        let mut file = fixture();
        let parts = body_local_meshes(&file)?;
        let graph_index = parts[0].bounds_root;
        assert_ne!(graph_index, 0x250);
        file.section_data[0][graph_index + 4..graph_index + 8]
            .copy_from_slice(&[0xab, 0xcd, 0x12, 0x34]);
        file.section_data[0][0x254..0x258].copy_from_slice(&[1, 0x70, 0, 3]);
        let reference = phantom_shape_reference(&file, parts[0].body_offset)?;
        assert_eq!((reference.section, reference.offset), (0, 0x250));
        assert_eq!(
            reference.header,
            super::super::hk_reference::Header {
                memory_size: 0x170,
                references: 3
            }
        );
        file.global_fixups[0].push(HkxGlobalFixup {
            src_offset: 0x10,
            dst_offset: 0x300,
            dst_section: 0,
        });
        assert!(phantom_shape_reference(&file, 0).is_err());
        file.global_fixups[0].pop();
        file.global_fixups[0]
            .iter_mut()
            .find(|fix| fix.src_offset == 0x10)
            .unwrap()
            .dst_section = 99;
        assert!(phantom_shape_reference(&file, 0).is_err());
        Ok(())
    }
    #[test]
    fn nonzero_welding_offset_imports_and_binds_original_triangles() {
        use super::super::native_gsk::SupportShape;
        let mut file = fixture();
        let words = file.section_data[0].len();
        file.section_data[0].extend_from_slice(&[0xaa, 0xbb, 0x12, 0x34, 0x56, 0x78]);
        file.local_fixups[0].push(HkxLocalFixup {
            src_offset: 0x3cc,
            dst_offset: words as u32,
        });
        file.section_data[0][0x3d0..0x3d4].copy_from_slice(&3u32.to_be_bytes());
        file.section_data[0][0x3d8] = 5;
        file.section_data[0][0x4cc..0x4d0].copy_from_slice(&1u32.to_be_bytes());
        let parts = body_local_meshes(&file).unwrap();
        let part = &parts[0];
        assert_eq!(part.welding_offset, 1);
        assert_eq!(
            part.welding
                .as_ref()
                .unwrap()
                .subpart(1, 2)
                .unwrap()
                .unwrap(),
            [0x1234, 0x5678]
        );
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let layout = super::super::dynamic_body::layout(&parts, &identity).unwrap();
        let mut world = super::super::physics::Collision::new(Vec::new()).unwrap();
        world
            .add_static_triangles(
                &[super::super::physics::Triangle {
                    p: [[0., -100., 0.], [1., -100., 0.], [0., -100., 1.]],
                }],
                0.,
                99,
            )
            .unwrap();
        let start = world.triangle_count();
        let motion = layout[0].instantiate_motion(&parts).unwrap();
        let auxiliary = part.instantiate_auxiliary().unwrap();
        let placed = layout[0]
            .place_initialized_query_body(&parts, motion, auxiliary, &mut world, 7)
            .unwrap();
        let installed_shape = placed.shapes[0];
        for triangle in 0..2 {
            let bound = part.authored_triangle_contact(triangle).unwrap();
            let installed = placed
                .contact_triangle(&parts, installed_shape, start + triangle)
                .unwrap()
                .unwrap();
            assert_eq!(installed.support.vertices, bound.support.vertices);
            assert_eq!(installed.radius, bound.radius);
            assert_eq!(installed.mode, bound.mode);
            assert_eq!(installed.word, bound.word);
            assert_eq!(bound.mode, 5);
            assert_eq!(bound.word, [0x1234, 0x5678][triangle]);
            assert_eq!(bound.radius, part.triangle_radius);
            let mut shape = bound.support;
            for id in 0..3 {
                let vertex = shape.vertex(id).unwrap();
                assert_eq!(
                    &vertex[..3],
                    &part.vertices[part.indices[triangle * 3 + id as usize] as usize]
                );
                assert_eq!(vertex[3].to_bits(), 0x3f000000 | id as u32);
            }
            assert_eq!(shape.sphere_count().unwrap(), 3);
        }
        assert!(part.triangle_support(2).is_err());
        assert!(part.authored_triangle_contact(2).is_err());
        assert!(part.triangle_support(usize::MAX).is_err());
        assert!(placed
            .contact_triangle(&parts, installed_shape, start - 1)
            .is_err());
        assert!(placed
            .contact_triangle(&parts, installed_shape, start + 2)
            .is_err());
        assert!(placed.contact_triangle(&parts, usize::MAX, start).is_err());
        file.section_data[0][0x4cc..0x4d0].copy_from_slice(&2u32.to_be_bytes());
        assert!(
            body_local_meshes(&file).is_err(),
            "Offset must remain bounds checked"
        );
    }
    #[test]
    fn resolves_saved_motion_and_keeps_placement_ownership_independent() {
        let mut file = fixture();
        let mut saved = file.section_data[0][0xe0..0x200].to_vec();
        saved[8] = 3;
        let offset = file.section_data[0].len();
        file.section_data[0].extend_from_slice(&saved);
        file.local_fixups[0].push(HkxLocalFixup {
            src_offset: 0x1f8,
            dst_offset: offset as u32,
        });
        file.section_data[0][0x2a] = 6;
        let parts = body_local_meshes(&file).unwrap();
        let mut first = parts[0].motion.instantiate().unwrap();
        let second = parts[0].motion.instantiate().unwrap();
        assert!(first.has_stored_dynamic() && second.has_stored_dynamic());
        first.replace(1, 0x3f800000).unwrap();
        assert_eq!(first.current.motion_type(), 3);
        assert_eq!(second.current.motion_type(), 5);
        assert!(second.has_stored_dynamic());
        assert_eq!(parts[0].motion.current.motion_type(), 5);
        assert_eq!(parts[0].motion.quality, 6);
        file.section_data[0].truncate(offset + 0x100);
        assert!(body_local_meshes(&file).is_err());
    }
    #[test]
    fn retains_velocity_w_lanes_and_rejects_nonfinite_metadata() {
        let mut file = fixture();
        file.section_data[0][0x1bc..0x1c0].copy_from_slice(&7.5f32.to_be_bytes());
        file.section_data[0][0x1cc..0x1d0].copy_from_slice(&(-3.25f32).to_be_bytes());
        for parts in [meshes(&file).unwrap(), body_local_meshes(&file).unwrap()] {
            for part in parts {
                assert_eq!(part.linear_velocity, [2., 0., 0., 7.5]);
                assert_eq!(part.angular_velocity, [0., 3., 0., -3.25]);
            }
        }
        file.section_data[0][0x1cc..0x1d0].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(body_local_meshes(&file).is_err());
    }
    #[test]
    fn retains_encoded_motion_limits_for_every_body_part() {
        let mut file = fixture();
        file.section_data[0][0x19c..0x19e].copy_from_slice(&[7, 128]);
        for parts in [meshes(&file).unwrap(), body_local_meshes(&file).unwrap()] {
            assert!(!parts.is_empty());
            assert!(parts.iter().all(|p| p.encoded_motion_limits == [7, 128]));
        }
    }
    #[test]
    fn preserves_local_center_of_mass_without_applying_body_translation() {
        let mut file = fixture();
        let center = [-0.75f32, 2.5, 0.125, 1.];
        for (i, value) in center.into_iter().enumerate() {
            file.section_data[0][0x170 + i * 4..0x174 + i * 4]
                .copy_from_slice(&value.to_be_bytes());
        }
        file.section_data[0][0x13c..0x140].copy_from_slice(&7.25f32.to_be_bytes());
        let parts = meshes(&file).unwrap();
        assert!(parts.iter().all(|p| p.local_center_of_mass == center));
        assert!(parts.iter().all(|p| p.swept_base_time == 7.25));
        assert_eq!(parts[0].vertices[0], [10., 2., 0.]);
        file.section_data[0][0x170..0x174].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(meshes(&file)
            .err()
            .unwrap()
            .to_string()
            .contains("Nonfinite rigid-body vector"));
    }
    #[test]
    fn body_local_geometry_preserves_placement_as_separate_metadata() {
        let file = fixture();
        let baked = meshes(&file).unwrap();
        let local = body_local_meshes(&file).unwrap();
        assert_eq!(local.len(), baked.len());
        assert_eq!(local[0].vertices[0], [0.; 3]);
        for (a, b) in local.iter().zip(&baked) {
            assert_eq!(a.body_transform, b.body_transform);
            assert_eq!(a.indices, b.indices);
            assert_eq!(a.body_offset, b.body_offset);
            for (&p, &expected) in a.vertices.iter().zip(&b.vertices) {
                assert_eq!(point(&a.body_transform, p), expected);
            }
        }
    }
    #[test]
    fn body_binding_name_preserves_empty_absent_and_malformed_states() {
        let mut f = fixture();
        let at = f.section_data[0].len() as u32;
        f.section_data[0].extend_from_slice(b"PlatformNode\0");
        f.local_fixups[0].push(HkxLocalFixup {
            src_offset: 0x74,
            dst_offset: at,
        });
        assert_eq!(
            meshes(&f).unwrap()[0].body_name.as_deref(),
            Some("PlatformNode")
        );
        f.section_data[0][at as usize] = 0;
        assert_eq!(meshes(&f).unwrap()[0].body_name.as_deref(), Some(""));
        f.section_data[0][at as usize] = b'P';
        f.section_data[0].pop();
        assert!(meshes(&f).is_err());
    }
    #[test]
    fn shared_shape_keeps_distinct_rigid_body_identity() {
        let mut f = fixture();
        let original_body = f.section_data[0][..0x200].to_vec();
        f.section_data[0].resize(0x900, 0);
        f.section_data[0][0x700..0x900].copy_from_slice(&original_body);
        f.section_data[0][0x820..0x824].copy_from_slice(&20f32.to_bits().to_be_bytes());
        let mut second = f.virtual_fixups[0][0].clone();
        second.src_offset = 0x700;
        f.virtual_fixups[0].push(second);
        f.global_fixups[0].push(HkxGlobalFixup {
            src_offset: 0x710,
            dst_offset: 0x250,
            dst_section: 0,
        });
        let result = meshes(&f).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!((result[0].body_offset, result[1].body_offset), (0, 0x700));
        assert_eq!(result[0].indices, result[1].indices);
        assert_eq!(result[0].vertices[0], [10., 2., 0.]);
        assert_eq!(result[1].vertices[0], [20., 2., 0.]);
    }
    #[test]
    fn rejects_truncated_index_storage() {
        let mut f = fixture();
        f.section_data[0][0x530..0x534].copy_from_slice(&4u32.to_be_bytes());
        assert!(meshes(&f).is_err());
    }
    #[test]
    fn rejects_invalid_triangle_radius() {
        for value in [-0.01f32, f32::NAN, f32::INFINITY] {
            let mut f = fixture();
            f.section_data[0][0x3e4..0x3e8].copy_from_slice(&value.to_be_bytes());
            assert!(meshes(&f).is_err());
        }
    }
}
