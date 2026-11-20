// nif_file.rs — NetImmerse / Gamebryo NIF 3D model format parser & serializer

use crate::file_manipulator::EndianType;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

// ─────────────────────────────────────────────────────────────────────────────
// Basic Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Matrix3 {
    pub rows: [[f32; 3]; 3],
}

impl Default for Matrix3 {
    fn default() -> Self {
        Self {
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NiTransform {
    pub translation: Vector3,
    pub rotation: Matrix3,
    pub scale: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Object Structures
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiObjectNETData {
    pub name: String,
    pub extra_data_links: Vec<Option<u32>>,
    pub controller_link: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiAVObjectData {
    pub base: NiObjectNETData,
    pub flags: u16,
    pub transform: NiTransform,
    pub property_links: Vec<Option<u32>>,
    pub collision_object_link: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiNodeData {
    pub base: NiAVObjectData,
    pub children: Vec<Option<u32>>,
    pub effects: Vec<Option<u32>>,
}

/// NiNode prefix followed by the authored sorting enum. Preserve the wire value;
/// the original runtime maps values >= 2 to INHERIT when loading.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiSortAdjustNodeData {
    pub base: NiNodeData,
    pub sorting_mode: u32,
    pub legacy_accumulator: Option<u32>,
}

/// Modern NiCamera wire layout, including obsolete stream fields retained
/// for lossless editing. Runtime interpretation of ortho is nonzero => true.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiCameraData {
    pub base: NiAVObjectData,
    pub obsolete_flags: u16,
    pub frustum: [f32; 6], // left, right, top, bottom, near, far
    pub orthographic: u8,
    pub viewport: [f32; 4], // left, right, top, bottom
    pub lod_adjust: f32,
    pub scene_link: Option<u32>,
    pub screen_polygons: Vec<Option<u32>>,
    pub screen_textures: Vec<Option<u32>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiMaterialPropertyData {
    pub base: NiObjectNETData,
    pub ambient: Vector3,
    pub diffuse: Vector3,
    pub specular: Vector3,
    pub emissive: Vector3,
    pub shininess: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiAlphaPropertyData {
    pub base: NiObjectNETData,
    pub flags: u16,
    pub threshold: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiSourceTextureData {
    pub base: NiObjectNETData,
    pub use_external: u8,
    pub file_name: Option<String>,
    pub pixel_data_link: Option<u32>,
    pub pixel_layout: u32,
    pub use_mipmaps: u32,
    pub alpha_format: u32,
    pub is_static: u8,
    pub direct_render: u8,
    pub persist_render_data: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialEntry {
    pub name: String,
    pub extra_data: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiRenderObjectData {
    pub base: NiAVObjectData,
    pub materials: Vec<MaterialEntry>,
    pub active_material: i32,
    pub needs_update_default: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticData {
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiDataStreamRef {
    pub stream_link: u32,
    pub is_per_instance: u8,
    pub submesh_region_indices: Vec<u16>,
    pub semantics: Vec<SemanticData>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiMeshData {
    pub base: NiRenderObjectData,
    pub primitive_type: u32,
    /// Exact EM1 words between primitive type and submesh count. Their
    /// meanings are not inferred, including the word formerly read as float.
    #[serde(default)]
    pub em1_fields: Vec<u32>,
    pub submesh_count: u16,
    pub instancing_enabled: u8,
    pub bound_center: Vector3,
    pub bound_radius: f32,
    pub stream_refs: Vec<NiDataStreamRef>,
    pub modifiers: Vec<Option<u32>>,
    pub hw_skin_flag: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RotationKey {
    pub initial_axis: f32,
    pub rotation_speed: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiPSParticleSystemData {
    pub base: NiMeshData,
    pub simulator_link: Option<u32>,
    pub generator_link: Option<u32>,
    pub emitter_links: Vec<Option<u32>>,
    pub spawner_links: Vec<Option<u32>>,
    pub bound_updater_link: Option<u32>,
    pub max_num_particles: u32,
    pub flag1: u8,
    pub flag2: u8,
    pub flag3: u8,
    pub flag4: u8,
    pub flag5: u8,
    pub normal_method: u32,
    pub normal_direction: Vector3,
    pub up_method: u32,
    pub up_direction: Vector3,
    pub living_spawner_link: Option<u32>,
    pub rotations: Vec<RotationKey>,
    pub unknown_has_float: u16,
    pub unknown_float: Option<f32>,
    /// Subclass payload following the known particle-system prefix. Retain it
    /// verbatim so typed base decoding never drops custom EM1 particle fields.
    #[serde(default)]
    pub extension: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiDataStreamRegion {
    pub start_index: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiDataStreamData {
    pub size_in_bytes: u32,
    pub cloning_behavior: u32,
    pub regions: Vec<NiDataStreamRegion>,
    pub component_formats: Vec<u32>,
    pub buffer: Vec<u8>,
    pub streamable: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiPixelFormatData {
    pub format: u32,
    pub bits_per_pixel: u8,
    pub renderer_hint: u32,
    pub extra_data: u32,
    pub flags: u8,
    pub tiling: u32,
    pub srgb_space: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MipLevel {
    pub width: u32,
    pub height: u32,
    pub offset_in_bytes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiWiiUPersistentSrcTextureRendererData {
    pub pixel_format: NiPixelFormatData,
    pub palette_link: Option<u32>,
    pub mip_levels: Vec<MipLevel>,
    pub face_size: u32,
    pub pad_offset: u32,
    pub face_count: u32,
    pub target_platform: u32,
    pub pixel_stride: u32,
    pub raw_pixels: Vec<u8>,
    pub gx2_format: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiStringExtraDataData {
    pub base: NiObjectNETData,
    pub string_data: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiIntegerExtraDataData {
    pub base: NiObjectNETData,
    pub value: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiFloatExtraDataData {
    pub base: NiObjectNETData,
    pub value: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NiBooleanExtraDataData {
    pub base: NiObjectNETData,
    pub value: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawBlockData {
    pub type_name: String,
    pub raw_data: Vec<u8>,
}

// ─────────────────────────────────────────────────────────────────────────────
// NiObject Enum
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NiObject {
    NiNode(NiNodeData),
    NiSortAdjustNode(NiSortAdjustNodeData),
    NiCamera(NiCameraData),
    NiMesh(NiMeshData),
    NiPSParticleSystem(NiPSParticleSystemData),
    NiDataStream(NiDataStreamData),
    NiMaterialProperty(NiMaterialPropertyData),
    NiAlphaProperty(NiAlphaPropertyData),
    NiSourceTexture(NiSourceTextureData),
    NiWiiUTextureData(NiWiiUPersistentSrcTextureRendererData),
    NiStringExtraData(NiStringExtraDataData),
    NiIntegerExtraData(NiIntegerExtraDataData),
    NiFloatExtraData(NiFloatExtraDataData),
    NiBooleanExtraData(NiBooleanExtraDataData),
    Unknown(RawBlockData),
}

impl NiObject {
    pub fn type_name(&self) -> &str {
        match self {
            NiObject::NiNode(_) => "NiNode",
            NiObject::NiCamera(_) => "NiCamera",
            NiObject::NiSortAdjustNode(_) => "NiSortAdjustNode",
            NiObject::NiMesh(_) => "NiMesh",
            NiObject::NiPSParticleSystem(_) => "NiPSParticleSystem",
            NiObject::NiDataStream(_) => "NiDataStream",
            NiObject::NiMaterialProperty(_) => "NiMaterialProperty",
            NiObject::NiAlphaProperty(_) => "NiAlphaProperty",
            NiObject::NiSourceTexture(_) => "NiSourceTexture",
            NiObject::NiWiiUTextureData(_) => "NiWiiUPersistentSrcTextureRendererData",
            NiObject::NiStringExtraData(_) => "NiStringExtraData",
            NiObject::NiIntegerExtraData(_) => "NiIntegerExtraData",
            NiObject::NiFloatExtraData(_) => "NiFloatExtraData",
            NiObject::NiBooleanExtraData(_) => "NiBooleanExtraData",
            NiObject::Unknown(raw) => raw.type_name.as_str(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            NiObject::NiNode(n) => n.base.base.name.as_str(),
            NiObject::NiCamera(c) => c.base.base.name.as_str(),
            NiObject::NiSortAdjustNode(n) => n.base.base.base.name.as_str(),
            NiObject::NiMesh(m) => m.base.base.base.name.as_str(),
            NiObject::NiPSParticleSystem(ps) => ps.base.base.base.base.name.as_str(),
            NiObject::NiMaterialProperty(m) => m.base.name.as_str(),
            NiObject::NiAlphaProperty(a) => a.base.name.as_str(),
            NiObject::NiSourceTexture(t) => t.base.name.as_str(),
            NiObject::NiStringExtraData(s) => s.base.name.as_str(),
            NiObject::NiIntegerExtraData(i) => i.base.name.as_str(),
            NiObject::NiFloatExtraData(f) => f.base.name.as_str(),
            NiObject::NiBooleanExtraData(b) => b.base.name.as_str(),
            _ => "",
        }
    }

    pub fn name_mut(&mut self) -> Option<&mut String> {
        match self {
            NiObject::NiNode(n) => Some(&mut n.base.base.name),
            NiObject::NiCamera(c) => Some(&mut c.base.base.name),
            NiObject::NiSortAdjustNode(n) => Some(&mut n.base.base.base.name),
            NiObject::NiMesh(m) => Some(&mut m.base.base.base.name),
            NiObject::NiPSParticleSystem(ps) => Some(&mut ps.base.base.base.base.name),
            NiObject::NiMaterialProperty(m) => Some(&mut m.base.name),
            NiObject::NiAlphaProperty(a) => Some(&mut a.base.name),
            NiObject::NiSourceTexture(t) => Some(&mut t.base.name),
            NiObject::NiStringExtraData(s) => Some(&mut s.base.name),
            NiObject::NiIntegerExtraData(i) => Some(&mut i.base.name),
            NiObject::NiFloatExtraData(f) => Some(&mut f.base.name),
            NiObject::NiBooleanExtraData(b) => Some(&mut b.base.name),
            _ => None,
        }
    }

    pub fn children(&self) -> &[Option<u32>] {
        match self {
            NiObject::NiNode(n) => &n.children,
            NiObject::NiSortAdjustNode(n) => &n.base.children,
            _ => &[],
        }
    }

    pub fn as_av_object(&self) -> Option<&NiAVObjectData> {
        match self {
            NiObject::NiNode(n) => Some(&n.base),
            NiObject::NiCamera(c) => Some(&c.base),
            NiObject::NiSortAdjustNode(n) => Some(&n.base.base),
            NiObject::NiMesh(m) => Some(&m.base.base),
            NiObject::NiPSParticleSystem(ps) => Some(&ps.base.base.base),
            _ => None,
        }
    }

    pub fn as_av_object_mut(&mut self) -> Option<&mut NiAVObjectData> {
        match self {
            NiObject::NiNode(n) => Some(&mut n.base),
            NiObject::NiCamera(c) => Some(&mut c.base),
            NiObject::NiSortAdjustNode(n) => Some(&mut n.base.base),
            NiObject::NiMesh(m) => Some(&mut m.base.base),
            NiObject::NiPSParticleSystem(ps) => Some(&mut ps.base.base.base),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NifFile
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NifFile {
    pub version: u32,
    pub user_version: u32,
    pub source_is_little_endian: bool,
    pub blocks: Vec<NiObject>,
    pub string_table: Vec<String>,
    pub roots: Vec<Option<u32>>,
    pub block_types: Vec<String>,
    pub block_type_indices: Vec<u16>,
}

impl NifFile {
    pub fn load_bytes(data: &[u8]) -> Result<Self, String> {
        let mut cursor = Cursor::new(data);
        Self::load(&mut cursor)
    }

    /// Retry a preserved raw payload in a bounded cursor. Callers can surface
    /// the typed decoder's actual failure instead of only reporting Unknown.
    /// No source data or decoded block list is modified.
    pub fn decode_raw_block(&self, block: &RawBlockData) -> Result<NiObject, String> {
        let size=u32::try_from(block.raw_data.len()).map_err(|_|"NIF block exceeds its u32 size".to_owned())?;
        let mut reader=Cursor::new(block.raw_data.as_slice());
        let endian=if self.source_is_little_endian {EndianType::LITTLE}else{EndianType::BIG};
        read_block(&mut reader,&block.type_name,size,0,endian,self.version,self.user_version,&self.string_table)
            .map_err(|error|format!("{} at byte {}/{}: {error}",block.type_name,reader.position(),size))
    }

    pub fn load<R: Read + Seek>(reader: &mut R) -> Result<Self, String> {
        let mut header_buf = Vec::new();
        let mut b = [0u8; 1];
        while reader.read_exact(&mut b).is_ok() {
            if b[0] == b'\n' {
                break;
            }
            header_buf.push(b[0]);
        }
        let header_str = String::from_utf8_lossy(&header_buf);
        if !header_str.contains("File Format") {
            return Err("Not a valid NIF file format header".into());
        }

        let version = read_u32(reader, EndianType::LITTLE)?;
        let source_is_little_endian = if version >= 0x14000003 {
            read_u8(reader)? != 0
        } else {
            true
        };

        let user_version = if version >= 0x0A000108 {
            read_u32(reader, EndianType::LITTLE)?
        } else {
            0
        };

        let num_blocks = read_u32(reader, EndianType::LITTLE)? as usize;
        let endian = if source_is_little_endian {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        // RTTI table
        let (block_types, block_type_indices) = if version >= 0x05000001 {
            let num_types = read_u16(reader, endian)? as usize;
            let mut types = Vec::with_capacity(num_types.min(10000));
            for _ in 0..num_types {
                types.push(read_sized_string(reader, endian)?);
            }
            let mut indices = Vec::with_capacity(num_blocks.min(100000));
            for _ in 0..num_blocks {
                indices.push(read_u16(reader, endian)?);
            }
            (types, indices)
        } else {
            (Vec::new(), Vec::new())
        };

        // Block sizes
        let block_sizes: Vec<u32> = if version >= 0x14020005 {
            (0..num_blocks)
                .map(|_| read_u32(reader, endian))
                .collect::<Result<_, _>>()?
        } else {
            vec![0; num_blocks]
        };

        // Fixed string table
        let string_table: Vec<String> = if version >= 0x14010001 {
            let count = read_u32(reader, endian)? as usize;
            let _max_len = read_u32(reader, endian)?;
            if count > 100_000 {
                return Err(format!("Fixed string table count too large: {}", count));
            }
            (0..count)
                .map(|_| read_sized_string(reader, endian))
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };

        // Object groups
        if version >= 0x05000006 {
            let num_groups = read_u32(reader, endian)? as usize;
            for _ in 1..num_groups {
                let _ = read_u32(reader, endian)?;
            }
        }

        // Parse Blocks with strict block size boundary enforcement matching cheese
        let mut blocks = Vec::with_capacity(num_blocks.min(100000));
        let mut block_start_pos = reader.stream_position().map_err(|e| e.to_string())?;

        for i in 0..num_blocks {
            let _ = reader.seek(SeekFrom::Start(block_start_pos));
            let type_name = if !block_types.is_empty() {
                let idx = block_type_indices.get(i).copied().unwrap_or(0) as usize;
                block_types
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| "Unknown".into())
            } else {
                read_sized_string(reader, endian)?
            };

            let block_size = block_sizes.get(i).copied().unwrap_or(0);

            let block = match read_block(
                reader,
                &type_name,
                block_size,
                block_start_pos,
                endian,
                version,
                user_version,
                &string_table,
            ) {
                Ok(b) => b,
                Err(_) => {
                    let _ = reader.seek(SeekFrom::Start(block_start_pos));
                    let mut raw_data = vec![0u8; block_size as usize];
                    if block_size > 0 {
                        let _ = reader.read_exact(&mut raw_data);
                    }
                    NiObject::Unknown(RawBlockData {
                        type_name,
                        raw_data,
                    })
                }
            };

            if version >= 0x14020005 && block_size > 0 {
                block_start_pos += block_size as u64;
            } else {
                block_start_pos = reader.stream_position().map_err(|e| e.to_string())?;
            }
            blocks.push(block);
        }

        let _ = reader.seek(SeekFrom::Start(block_start_pos));

        // Footer: Roots
        let roots = if version >= 0x0303000D {
            let count = read_u32(reader, endian)? as usize;
            if count > 100_000 {
                Vec::new()
            } else {
                (0..count)
                    .map(|_| read_link_id(reader, endian))
                    .collect::<Result<_, _>>()?
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            version,
            user_version,
            source_is_little_endian,
            blocks,
            string_table,
            roots,
            block_types,
            block_type_indices,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut writer = Cursor::new(Vec::new());
        self.save_stream(&mut writer)?;
        Ok(writer.into_inner())
    }

    pub fn save_stream<W: Write>(&self, writer: &mut W) -> Result<(), String> {
        let header_str = format!(
            "Gamebryo File Format, Version {}.{}.{}.{}\n",
            (self.version >> 24) & 0xFF,
            (self.version >> 16) & 0xFF,
            (self.version >> 8) & 0xFF,
            self.version & 0xFF
        );
        writer
            .write_all(header_str.as_bytes())
            .map_err(|e| e.to_string())?;

        write_u32(writer, self.version, EndianType::LITTLE)?;
        if self.version >= 0x14000003 {
            write_u8(writer, if self.source_is_little_endian { 1 } else { 0 })?;
        }
        if self.version >= 0x0A000108 {
            write_u32(writer, self.user_version, EndianType::LITTLE)?;
        }
        write_u32(writer, self.blocks.len() as u32, EndianType::LITTLE)?;

        let endian = if self.source_is_little_endian {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        // RTTI table
        if self.version >= 0x05000001 {
            write_u16(writer, self.block_types.len() as u16, endian)?;
            for t in &self.block_types {
                write_sized_string(writer, t, endian)?;
            }
            for &idx in &self.block_type_indices {
                write_u16(writer, idx, endian)?;
            }
        }

        // Serialize block payloads
        let mut block_buffers = Vec::with_capacity(self.blocks.len());
        for block in &self.blocks {
            let mut buf = Cursor::new(Vec::new());
            write_block(
                &mut buf,
                block,
                endian,
                self.version,
                self.user_version,
                &self.string_table,
            )?;
            block_buffers.push(buf.into_inner());
        }

        // Write block sizes
        if self.version >= 0x14020005 {
            for buf in &block_buffers {
                write_u32(writer, buf.len() as u32, endian)?;
            }
        }

        // String table
        if self.version >= 0x14010001 {
            write_u32(writer, self.string_table.len() as u32, endian)?;
            let max_len = self.string_table.iter().map(|s| s.len()).max().unwrap_or(0);
            write_u32(writer, max_len as u32, endian)?;
            for s in &self.string_table {
                write_sized_string(writer, s, endian)?;
            }
        }

        // Object groups
        if self.version >= 0x05000006 {
            write_u32(writer, 0, endian)?;
        }

        // Write block data payloads
        for buf in &block_buffers {
            writer.write_all(buf).map_err(|e| e.to_string())?;
        }

        // Write roots
        if self.version >= 0x0303000D {
            write_u32(writer, self.roots.len() as u32, endian)?;
            for &r in &self.roots {
                write_link_id(writer, r, endian)?;
            }
        }

        Ok(())
    }
}

pub fn f16_to_f32(half: u16) -> f32 {
    let sign = ((half >> 15) & 0x0001) as u32;
    let exp = ((half >> 10) & 0x001f) as u32;
    let mant = (half & 0x03ff) as u32;

    if exp == 0 {
        if mant == 0 {
            f32::from_bits(sign << 31)
        } else {
            let mut m = mant;
            let mut e = 0;
            while (m & 0x0400) == 0 {
                m <<= 1;
                e += 1;
            }
            let exp_f32 = (127 - 15 - e + 1) as u32;
            let mant_f32 = (m & 0x03ff) << 13;
            f32::from_bits((sign << 31) | (exp_f32 << 23) | mant_f32)
        }
    } else if exp == 31 {
        if mant == 0 {
            f32::from_bits((sign << 31) | (0xff << 23))
        } else {
            f32::from_bits((sign << 31) | (0xff << 23) | (mant << 13))
        }
    } else {
        let exp_f32 = (exp + 127 - 15) as u32;
        let mant_f32 = mant << 13;
        f32::from_bits((sign << 31) | (exp_f32 << 23) | mant_f32)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3D Geometry Extraction
// ─────────────────────────────────────────────────────────────────────────────

pub struct ExtractedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub colors: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
    pub vertex_count: usize,
}

pub fn fmt_stride(fmt: u32) -> usize {
    let count = ((fmt >> 16) & 0xFF) as usize;
    let bytes = ((fmt >> 8) & 0xFF) as usize;
    count * bytes
}

pub fn sem_offset(stream: &NiDataStreamData, sem_idx: usize) -> usize {
    stream.component_formats[..sem_idx]
        .iter()
        .map(|&f| fmt_stride(f))
        .sum()
}

pub fn vertex_stride(stream: &NiDataStreamData) -> usize {
    stream
        .component_formats
        .iter()
        .map(|&f| fmt_stride(f))
        .sum()
}

pub fn extract_f32x3_stream(
    is_le: bool,
    stream: &NiDataStreamData,
    sem_idx: usize,
    start: usize,
    count: usize,
) -> Vec<[f32; 3]> {
    let stride = vertex_stride(stream);
    let offset = sem_offset(stream, sem_idx);
    let fmt = stream.component_formats.get(sem_idx).copied().unwrap_or(0);
    let num_comp = ((fmt >> 16) & 0xFF).max(1) as usize;
    let elem_bytes = ((fmt >> 8) & 0xFF).max(1) as usize;
    let elem_type = fmt & 0xFF;

    let mut out = Vec::with_capacity(count.min(1000000));
    for i in start..start + count {
        let base = i * stride + offset;
        if elem_bytes == 1 && base + num_comp <= stream.buffer.len() {
            let mut coords = [0.0f32; 3];
            for c in 0..num_comp.min(3) {
                let off = base + c;
                let b = stream.buffer[off];
                coords[c] = match elem_type {
                    0x12 | 0x13 => (b as i8) as f32 / 127.0,
                    0x10 | 0x11 => b as f32 / 255.0,
                    0x16 | 0x17 => b as f32,
                    0x18 | 0x19 => (b as i8) as f32,
                    _ => (b as i8) as f32 / 127.0, // Default to signed normalized
                };
            }
            out.push(coords);
        } else if elem_bytes == 2 && base + num_comp * 2 <= stream.buffer.len() {
            let mut coords = [0.0f32; 3];
            for c in 0..num_comp.min(3) {
                let off = base + c * 2;
                let b = &stream.buffer[off..off + 2];
                let u16_val = if is_le {
                    u16::from_le_bytes(b[0..2].try_into().unwrap())
                } else {
                    u16::from_be_bytes(b[0..2].try_into().unwrap())
                };
                let i16_val = u16_val as i16;
                coords[c] = match elem_type {
                    0x31 | 0x32 | 0x33 | 0x34 => f16_to_f32(u16_val),
                    0x1A | 0x1B => i16_val as f32 / 32767.0,
                    0x1E | 0x1F => u16_val as f32 / 65535.0,
                    _ => i16_val as f32,
                };
            }
            out.push(coords);
        } else if base + num_comp * 4 <= stream.buffer.len() {
            let mut coords = [0.0f32; 3];
            for c in 0..num_comp.min(3) {
                let off = base + c * 4;
                let b = &stream.buffer[off..off + 4];
                coords[c] = if is_le {
                    f32::from_le_bytes(b[0..4].try_into().unwrap())
                } else {
                    f32::from_be_bytes(b[0..4].try_into().unwrap())
                };
            }
            out.push(coords);
        } else {
            out.push([0.0, 0.0, 1.0]);
        }
    }
    out
}

pub fn extract_f32x2_stream(
    is_le: bool,
    stream: &NiDataStreamData,
    sem_idx: usize,
    start: usize,
    count: usize,
) -> Vec<[f32; 2]> {
    let stride = vertex_stride(stream);
    let offset = sem_offset(stream, sem_idx);
    let fmt = stream.component_formats.get(sem_idx).copied().unwrap_or(0);
    let comp = fmt;
    let elem_type = fmt & 0xFF;
    let elem_bytes = ((fmt >> 8) & 0xFF) as usize;
    let mut out = Vec::with_capacity(count.min(1000000));
    for i in start..start + count {
        let base = i * stride + offset;
        if elem_bytes == 2 && base + 4 <= stream.buffer.len() {
            let b = &stream.buffer[base..base + 4];
            let u16_0 = if is_le {
                u16::from_le_bytes(b[0..2].try_into().unwrap())
            } else {
                u16::from_be_bytes(b[0..2].try_into().unwrap())
            };
            let u16_1 = if is_le {
                u16::from_le_bytes(b[2..4].try_into().unwrap())
            } else {
                u16::from_be_bytes(b[2..4].try_into().unwrap())
            };
            let i16_0 = u16_0 as i16;
            let i16_1 = u16_1 as i16;

            let (u, v) = match elem_type {
                0x31 | 0x32 | 0x33 | 0x34 => (f16_to_f32(u16_0), f16_to_f32(u16_1)),
                0x1A | 0x1B => (i16_0 as f32 / 32767.0, i16_1 as f32 / 32767.0),
                0x1E | 0x1F => (u16_0 as f32 / 65535.0, u16_1 as f32 / 65535.0),
                0x40 => (i16_0 as f32 / 1023.0, i16_1 as f32 / 1023.0),
                _ => (i16_0 as f32 / 1023.0, i16_1 as f32 / 1023.0),
            };
            out.push([u, v]);
        } else if base + 8 <= stream.buffer.len() {
            let b = &stream.buffer[base..base + 8];
            let u = if is_le {
                f32::from_le_bytes(b[0..4].try_into().unwrap())
            } else {
                f32::from_be_bytes(b[0..4].try_into().unwrap())
            };
            let v = if is_le {
                f32::from_le_bytes(b[4..8].try_into().unwrap())
            } else {
                f32::from_be_bytes(b[4..8].try_into().unwrap())
            };
            out.push([u, v]);
        } else {
            out.push([0.0, 0.0]);
        }
    }
    out
}

pub fn extract_f32x4_stream(
    is_le: bool,
    stream: &NiDataStreamData,
    sem_idx: usize,
    start: usize,
    count: usize,
) -> Vec<[f32; 4]> {
    let stride = vertex_stride(stream);
    let offset = sem_offset(stream, sem_idx);
    let fmt = stream.component_formats.get(sem_idx).copied().unwrap_or(0);
    let num_comp = ((fmt >> 16) & 0xFF).max(1) as usize;
    let elem_bytes = ((fmt >> 8) & 0xFF).max(1) as usize;
    let elem_type = fmt & 0xFF;

    let mut out = Vec::with_capacity(count.min(1000000));
    for i in start..start + count {
        let base = i * stride + offset;
        if elem_bytes == 1 && base + num_comp <= stream.buffer.len() {
            let mut rgba = [1.0f32; 4];
            for c in 0..num_comp.min(4) {
                let off = base + c;
                let b = stream.buffer[off];
                rgba[c] = b as f32 / 255.0;
            }
            out.push(rgba);
        } else if elem_bytes == 2 && base + num_comp * 2 <= stream.buffer.len() {
            let mut rgba = [1.0f32; 4];
            for c in 0..num_comp.min(4) {
                let off = base + c * 2;
                let b = &stream.buffer[off..off + 2];
                let u16_val = if is_le {
                    u16::from_le_bytes(b[0..2].try_into().unwrap())
                } else {
                    u16::from_be_bytes(b[0..2].try_into().unwrap())
                };
                rgba[c] = match elem_type {
                    0x31 | 0x32 | 0x33 | 0x34 => f16_to_f32(u16_val),
                    _ => u16_val as f32 / 65535.0,
                };
            }
            out.push(rgba);
        } else if base + num_comp * 4 <= stream.buffer.len() {
            let mut rgba = [1.0f32; 4];
            for c in 0..num_comp.min(4) {
                let off = base + c * 4;
                let b = &stream.buffer[off..off + 4];
                rgba[c] = if is_le {
                    f32::from_le_bytes(b[0..4].try_into().unwrap())
                } else {
                    f32::from_be_bytes(b[0..4].try_into().unwrap())
                };
            }
            out.push(rgba);
        } else {
            out.push([1.0, 1.0, 1.0, 1.0]);
        }
    }
    out
}

fn extract_display_list_mesh(
    nif: &NifFile,
    mesh: &NiMeshData,
    dl_stream: &NiDataStreamData,
    source_positions: &mut Vec<u32>,
) -> Option<ExtractedMesh> {
    let is_le = nif.source_is_little_endian;

    let get_stream = |link: u32| -> Option<&NiDataStreamData> {
        match nif.blocks.get(link as usize)? {
            NiObject::NiDataStream(ds) => Some(ds),
            _ => None,
        }
    };

    struct AttrStream {
        name: String,
        sem_name: String,
        sem_index: u32,
        positions: Option<Vec<[f32; 3]>>,
        normals: Option<Vec<[f32; 3]>>,
        uvs: Option<Vec<[f32; 2]>>,
        colors: Option<Vec<[f32; 4]>>,
        count: usize,
    }

    let mut pos_attr: Option<AttrStream> = None;
    let mut norm_attr: Option<AttrStream> = None;
    let mut tex_attrs: Vec<AttrStream> = Vec::new();
    let mut color_attrs: Vec<AttrStream> = Vec::new();

    for r in &mesh.stream_refs {
        for (sem_idx, sem) in r.semantics.iter().enumerate() {
            let name_upper = sem.name.to_uppercase();
            if name_upper.starts_with("INDEX") || name_upper.starts_with("DISPLAYLIST") {
                continue;
            }

            let stream = match get_stream(r.stream_link) {
                Some(s) => s,
                None => continue,
            };
            let r_idx = *r.submesh_region_indices.first().unwrap_or(&0) as usize;
            let reg = stream.regions.get(r_idx).or_else(|| stream.regions.first());
            let stride = vertex_stride(stream);
            let offset = sem_offset(stream, sem_idx);
            let (start, count) = if let Some(rg) = reg {
                let c = if rg.count > 0 {
                    rg.count as usize
                } else {
                    if stride > 0 {
                        (stream.buffer.len().saturating_sub(offset)) / stride
                    } else {
                        0
                    }
                };
                (rg.start_index as usize, c)
            } else {
                let c = if stride > 0 {
                    (stream.buffer.len().saturating_sub(offset)) / stride
                } else {
                    0
                };
                (0, c)
            };

            if name_upper.starts_with("POSITION") {
                let pos = extract_f32x3_stream(is_le, stream, sem_idx, start, count);
                pos_attr = Some(AttrStream {
                    name: sem.name.clone(),
                    sem_name: "POSITION".into(),
                    sem_index: sem.index,
                    positions: Some(pos),
                    normals: None,
                    uvs: None,
                    colors: None,
                    count,
                });
            } else if name_upper.starts_with("NORMAL") {
                let norm = extract_f32x3_stream(is_le, stream, sem_idx, start, count);
                norm_attr = Some(AttrStream {
                    name: sem.name.clone(),
                    sem_name: "NORMAL".into(),
                    sem_index: sem.index,
                    positions: None,
                    normals: Some(norm),
                    uvs: None,
                    colors: None,
                    count,
                });
            } else if name_upper.starts_with("TEXCOORD") {
                let uv = extract_f32x2_stream(is_le, stream, sem_idx, start, count);
                tex_attrs.push(AttrStream {
                    name: sem.name.clone(),
                    sem_name: "TEXCOORD".into(),
                    sem_index: sem.index,
                    positions: None,
                    normals: None,
                    uvs: Some(uv),
                    colors: None,
                    count,
                });
            } else if name_upper.starts_with("COLOR") {
                let col = extract_f32x4_stream(is_le, stream, sem_idx, start, count);
                color_attrs.push(AttrStream {
                    name: sem.name.clone(),
                    sem_name: "COLOR".into(),
                    sem_index: sem.index,
                    positions: None,
                    normals: None,
                    uvs: None,
                    colors: Some(col),
                    count,
                });
            }
        }
    }

    tex_attrs.sort_by_key(|a| a.sem_index);
    color_attrs.sort_by_key(|a| a.sem_index);

    let raw_positions = pos_attr.as_ref()?.positions.clone()?;
    let raw_normals = norm_attr.as_ref().and_then(|a| a.normals.clone());
    let raw_uvs = tex_attrs.first().and_then(|a| a.uvs.clone());

    struct AttrInfo {
        kind: &'static str,
        stream_idx: usize,
    }

    let mut attr_infos: Vec<AttrInfo> = Vec::new();
    attr_infos.push(AttrInfo {
        kind: "POSITION",
        stream_idx: 0,
    });
    if norm_attr.is_some() {
        attr_infos.push(AttrInfo {
            kind: "NORMAL",
            stream_idx: 0,
        });
    }
    for (i, _) in color_attrs.iter().enumerate() {
        attr_infos.push(AttrInfo {
            kind: "COLOR",
            stream_idx: i,
        });
    }
    for (i, _) in tex_attrs.iter().enumerate() {
        attr_infos.push(AttrInfo {
            kind: "TEXCOORD",
            stream_idx: i,
        });
    }

    let buf = &dl_stream.buffer;

    // Morph/instancing modifiers do not add GX matrix-index bytes. Only
    // a skin binding can select the legacy skinned display-list path.
    let has_weights = mesh.hw_skin_flag != 0 || mesh.modifiers.iter().flatten().any(|id|
        nif.blocks.get(*id as usize).is_some_and(|block|block.type_name()=="NiSkinningMeshModifier"));
    let weight_bytes = if has_weights { 2 } else { 0 };

    let read_attr_index = |buf: &[u8], offset: usize, bytes: usize| -> usize {
        match bytes {
            1 => buf.get(offset).copied().unwrap_or(0) as usize,
            2 => {
                if offset + 2 <= buf.len() {
                    u16::from_be_bytes(buf[offset..offset + 2].try_into().unwrap()) as usize
                } else {
                    0
                }
            }
            _ => 0,
        }
    };

    // Detect index_bytes (1 vs 2) by testing full display list stride and position bounds validity
    let detect_index_bytes = || -> usize {
        let test_stride = |idx_bytes: usize| -> bool {
            let vert_stride = weight_bytes + attr_infos.len() * idx_bytes;
            let mut cur = 0;
            let mut prim_count = 0;
            while cur < buf.len() {
                let cmd = buf[cur];
                if cmd == 0 && buf[cur..].iter().all(|&b| b == 0) {
                    break;
                }
                cur += 1;
                // We want to debug this! So let's print!
                // Actually, let's ONLY print when it returns false, because there's too much output.
                let is_prim = matches!(
                    cmd & 0xF8,
                    0x80 | 0x88 | 0x90 | 0x98 | 0xA0 | 0xA8 | 0xB0 | 0xB8
                );
                if is_prim {
                    if cur + 2 > buf.len() {
                        return false;
                    }
                    let num_verts =
                        u16::from_be_bytes(buf[cur..cur + 2].try_into().unwrap()) as usize;
                    cur += 2;
                    if cur + num_verts * vert_stride > buf.len() {
                        return false;
                    }
                    for _ in 0..num_verts {
                        let pos_idx = read_attr_index(buf, cur + weight_bytes, idx_bytes);
                        if pos_idx >= raw_positions.len() {
                            return false;
                        }
                        cur += vert_stride;
                    }
                    prim_count += 1;
                } else {
                    match cmd {
                        0x00 => { /* just skip padding */ }
                        0x08 | 0x10 | 0x18 => {
                            if cur + 4 <= buf.len() {
                                cur += 4;
                            } else {
                                return false;
                            }
                        }
                        0x20..=0x3F => {
                            if cur + 2 <= buf.len() {
                                cur += 2;
                            } else {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
            }
            if prim_count > 0 && cur <= buf.len() {
                true
            } else {
                false
            }
        };
        let fits_1 = test_stride(1);
        let fits_2 = test_stride(2);
        if fits_1 && !fits_2 {
            1
        } else if fits_2 && !fits_1 {
            2
        } else if raw_positions.len() <= 255 {
            // u8 indices can only address 0..=255; use 1-byte only when strictly within range
            1
        } else {
            2
        }
    };

    let index_bytes = detect_index_bytes();
    println!("    Detected index_bytes = {}", index_bytes);
    let vert_stride: usize = weight_bytes + attr_infos.len() * index_bytes;

    let mut vert_map: std::collections::HashMap<(usize, usize, usize), u32> =
        std::collections::HashMap::new();

    let mut out_positions = Vec::new();
    let mut out_normals = Vec::new();
    let mut out_uvs = Vec::new();
    let mut out_indices = Vec::new();

    let mut cursor = 0;
    while cursor < buf.len() {
        let cmd = buf[cursor];
        cursor += 1;

        let is_prim = matches!(cmd & 0xF8, 0x80 | 0x88 | 0x90 | 0xA0 | 0xA8 | 0xB0 | 0xB8);
        let is_tristrip = (cmd & 0xF8) == 0x98;

        if is_prim || is_tristrip {
            if cursor + 2 > buf.len() {
                break;
            }
            let num_verts =
                u16::from_be_bytes(buf[cursor..cursor + 2].try_into().unwrap()) as usize;
            println!(
                "    GX Command: {:#X}, is_tristrip={}, num_verts={}",
                cmd, is_tristrip, num_verts
            );
            cursor += 2;

            let mut strip_indices = Vec::with_capacity(num_verts);

            for _ in 0..num_verts {
                if cursor + vert_stride > buf.len() {
                    break;
                }
                let vert_buf = &buf[cursor..cursor + vert_stride];
                cursor += vert_stride;

                let mut idx_off = weight_bytes;

                let mut pos_idx = 0;
                let mut norm_idx = 0;
                let mut first_uv_idx = None;

                for info in &attr_infos {
                    let idx = read_attr_index(vert_buf, idx_off, index_bytes);
                    idx_off += index_bytes;
                    match info.kind {
                        "POSITION" => pos_idx = idx,
                        "NORMAL" => norm_idx = idx,
                        "TEXCOORD" => {
                            if info.stream_idx == 0 {
                                first_uv_idx = Some(idx);
                            }
                        }
                        _ => {}
                    }
                }

                let uv_idx = first_uv_idx.unwrap_or(0);

                // Skip vertices whose position index is out of range — these are
                // a parsing error, not a valid "use the first vertex" fallback.
                // Silently mapping them to pos[0] caused the stretched-vert spike.
                if pos_idx >= raw_positions.len() {
                    println!(
                        "    WARNING: pos_idx {} >= raw_positions.len() {}",
                        pos_idx,
                        raw_positions.len()
                    );
                    // Push a sentinel so strip_indices length stays consistent;
                    // degenerate-triangle detection below will discard any tri using it.
                    strip_indices.push(u32::MAX);
                    continue;
                }

                let key = (pos_idx, norm_idx, uv_idx);
                let combined_idx = *vert_map.entry(key).or_insert_with(|| {
                    let new_idx = out_positions.len() as u32;
                    let p = raw_positions[pos_idx];
                    out_positions.push(p);
                    source_positions.push(pos_idx as u32);

                    if let Some(ref ns) = raw_normals {
                        let n = ns.get(norm_idx).copied().unwrap_or([0.0, 1.0, 0.0]);
                        out_normals.push(n);
                    }
                    if let Some(ref us) = raw_uvs {
                        let u = us.get(uv_idx).copied().unwrap_or([0.0, 0.0]);
                        out_uvs.push(u);
                    }
                    new_idx
                });

                strip_indices.push(combined_idx);
            }

            // Sentinel value used above for out-of-range position indices
            let is_valid = |i: u32| i != u32::MAX;

            if is_tristrip {
                for i in 0..(strip_indices.len().saturating_sub(2)) {
                    let (a, b, c) = if i % 2 == 0 {
                        (strip_indices[i], strip_indices[i + 1], strip_indices[i + 2])
                    } else {
                        (strip_indices[i], strip_indices[i + 2], strip_indices[i + 1])
                    };
                    if a != b && b != c && a != c && is_valid(a) && is_valid(b) && is_valid(c) {
                        out_indices.push(a);
                        out_indices.push(b);
                        out_indices.push(c);
                    }
                }
            } else {
                for chunk in strip_indices.chunks_exact(3) {
                    let (a, b, c) = (chunk[0], chunk[1], chunk[2]);
                    if is_valid(a) && is_valid(b) && is_valid(c) {
                        out_indices.push(a);
                        out_indices.push(b);
                        out_indices.push(c);
                    }
                }
            }
        } else {
            match cmd {
                0x00 => {}
                0x08 | 0x10 | 0x18 => {
                    if cursor + 4 <= buf.len() {
                        cursor += 4;
                    } else {
                        break;
                    }
                }
                0x20..=0x3F => {
                    if cursor + 2 <= buf.len() {
                        cursor += 2;
                    } else {
                        break;
                    }
                }
                _ => {
                    println!(
                        "    Unknown GX Command: {:#X} at cursor {}",
                        cmd,
                        cursor - 1
                    );
                    if cursor + 1 <= buf.len() {
                        cursor += 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    if out_positions.is_empty() {
        return None;
    }

    let v_count = out_positions.len();
    Some(ExtractedMesh {
        positions: out_positions,
        normals: if raw_normals.is_some() {
            Some(out_normals)
        } else {
            None
        },
        uvs: if raw_uvs.is_some() {
            Some(out_uvs)
        } else {
            None
        },
        colors: None,
        indices: out_indices,
        vertex_count: v_count,
    })
}

pub fn extract_mesh(nif: &NifFile, mesh: &NiMeshData) -> Option<ExtractedMesh> {
    extract_mesh_impl(nif, mesh, &mut Vec::new())
}

/// Retains original POSITION stream indices after region concatenation or GX welding.
/// Unlike the legacy preview entry point, refuses synthetic geometry without sources.
pub fn extract_mesh_with_source_indices(nif: &NifFile, mesh: &NiMeshData) -> Option<(ExtractedMesh, Vec<u32>)> {
    let mut sources = Vec::new();
    let output = extract_mesh_impl(nif, mesh, &mut sources)?;
    if sources.len() != output.positions.len() { return None; }
    Some((output, sources))
}
fn extract_mesh_impl(nif: &NifFile, mesh: &NiMeshData, source_positions: &mut Vec<u32>) -> Option<ExtractedMesh> {
    let is_le = nif.source_is_little_endian;

    let get_stream = |link: u32| -> Option<&NiDataStreamData> {
        match nif.blocks.get(link as usize)? {
            NiObject::NiDataStream(ds) => Some(ds),
            _ => None,
        }
    };

    let find_stream_ref = |sem_name: &str| -> Option<(&NiDataStreamRef, usize)> {
        for r in &mesh.stream_refs {
            if let Some(sem_idx) = r
                .semantics
                .iter()
                .position(|s| s.name.starts_with(sem_name))
            {
                return Some((r, sem_idx));
            }
        }
        None
    };

    if let Some((dl_ref, _)) = find_stream_ref("DISPLAYLIST") {
        if let Some(dl_stream) = get_stream(dl_ref.stream_link) {
            if let Some(ext) = extract_display_list_mesh(nif, mesh, dl_stream, source_positions) {
                return Some(ext);
            }
        }
    }

    source_positions.clear();
    let (pos_ref, pos_sem_idx) = match find_stream_ref("POSITION") {
        Some(res) => res,
        None => {
            let half_w = if mesh.bound_radius > 0.0 {
                mesh.bound_radius * 0.5
            } else {
                0.5
            };
            let positions = vec![
                [-half_w, -half_w, 0.0],
                [half_w, -half_w, 0.0],
                [half_w, half_w, 0.0],
                [-half_w, half_w, 0.0],
            ];
            let uvs = Some(vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
            let indices = vec![0, 1, 2, 0, 2, 3];
            let vertex_count = 4;
            return Some(ExtractedMesh {
                positions,
                normals: None,
                uvs,
                colors: None,
                indices,
                vertex_count,
            });
        }
    };
    let pos_stream = match get_stream(pos_ref.stream_link) {
        Some(ds) => ds,
        None => return None,
    };

    let idx_ref_opt = find_stream_ref("INDEX");
    let norm_ref_opt = find_stream_ref("NORMAL");
    let uv_ref_opt = find_stream_ref("TEXCOORD");
    let color_ref_opt = find_stream_ref("COLOR");

    let num_regions = pos_ref.submesh_region_indices.len().max(1);

    let mut out_positions = Vec::new();
    let mut out_normals = Vec::new();
    let mut out_uvs = Vec::new();
    let mut out_colors = Vec::new();
    let mut out_indices = Vec::new();

    let mut has_normals = false;
    let mut has_uvs = false;
    let mut has_colors = false;

    for r_i in 0..num_regions {
        let pos_region_idx = pos_ref
            .submesh_region_indices
            .get(r_i)
            .copied()
            .unwrap_or(0) as usize;
        let pos_reg = match pos_stream
            .regions
            .get(pos_region_idx)
            .or_else(|| pos_stream.regions.first())
        {
            Some(r) => r,
            None => continue,
        };

        let pos_stride = vertex_stride(pos_stream);
        let pos_offset = sem_offset(pos_stream, pos_sem_idx);
        let v_start = pos_reg.start_index as usize;
        let v_count = if pos_reg.count > 0 {
            pos_reg.count as usize
        } else if pos_stride > 0 {
            (pos_stream.buffer.len().saturating_sub(pos_offset)) / pos_stride
        } else {
            0
        };

        let vertex_base_offset = out_positions.len() as u32;

        // 1. Positions
        let reg_positions = extract_f32x3_stream(is_le, pos_stream, pos_sem_idx, v_start, v_count);
        source_positions.extend((0..reg_positions.len()).map(|i| (v_start + i) as u32));
        out_positions.extend(reg_positions);

        // 2. Normals
        if let Some((norm_ref, norm_sem_idx)) = norm_ref_opt {
            if let Some(norm_stream) = get_stream(norm_ref.stream_link) {
                let norm_region_idx = norm_ref
                    .submesh_region_indices
                    .get(r_i)
                    .copied()
                    .unwrap_or(0) as usize;
                let norm_reg_start = norm_stream
                    .regions
                    .get(norm_region_idx)
                    .map(|r| r.start_index as usize)
                    .unwrap_or(v_start);
                let reg_normals =
                    extract_f32x3_stream(is_le, norm_stream, norm_sem_idx, norm_reg_start, v_count);
                out_normals.extend(reg_normals);
                has_normals = true;
            }
        }

        // 3. UVs
        if let Some((uv_ref, uv_sem_idx)) = uv_ref_opt {
            if let Some(uv_stream) = get_stream(uv_ref.stream_link) {
                let uv_region_idx =
                    uv_ref.submesh_region_indices.get(r_i).copied().unwrap_or(0) as usize;
                let uv_reg_start = uv_stream
                    .regions
                    .get(uv_region_idx)
                    .map(|r| r.start_index as usize)
                    .unwrap_or(v_start);
                let reg_uvs =
                    extract_f32x2_stream(is_le, uv_stream, uv_sem_idx, uv_reg_start, v_count);
                out_uvs.extend(reg_uvs);
                has_uvs = true;
            }
        }

        // 3.5. Colors
        if let Some((color_ref, color_sem_idx)) = color_ref_opt {
            if let Some(color_stream) = get_stream(color_ref.stream_link) {
                let color_region_idx = color_ref
                    .submesh_region_indices
                    .get(r_i)
                    .copied()
                    .unwrap_or(0) as usize;
                let color_reg_start = color_stream
                    .regions
                    .get(color_region_idx)
                    .map(|r| r.start_index as usize)
                    .unwrap_or(v_start);
                let reg_colors = extract_f32x4_stream(
                    is_le,
                    color_stream,
                    color_sem_idx,
                    color_reg_start,
                    v_count,
                );
                out_colors.extend(reg_colors);
                has_colors = true;
            }
        }

        // 4. Indices
        if let Some((idx_ref, _)) = idx_ref_opt {
            if let Some(idx_stream) = get_stream(idx_ref.stream_link) {
                let idx_region_idx = idx_ref
                    .submesh_region_indices
                    .get(r_i)
                    .copied()
                    .unwrap_or(0) as usize;
                if let Some(idx_reg) = idx_stream
                    .regions
                    .get(idx_region_idx)
                    .or_else(|| idx_stream.regions.first())
                {
                    let i_start = idx_reg.start_index as usize;
                    let i_count = idx_reg.count as usize;
                    let index_bytes = if idx_stream.buffer.len() < i_count * 2 {
                        1
                    } else {
                        2
                    };
                    for i in i_start..i_start + i_count {
                        let base = i * index_bytes;
                        if base + index_bytes <= idx_stream.buffer.len() {
                            let idx = if index_bytes == 1 {
                                idx_stream.buffer[base] as u32
                            } else {
                                let b = &idx_stream.buffer[base..base + 2];
                                if is_le {
                                    u16::from_le_bytes(b.try_into().unwrap()) as u32
                                } else {
                                    u16::from_be_bytes(b.try_into().unwrap()) as u32
                                }
                            };
                            out_indices.push(idx + vertex_base_offset);
                        }
                    }
                }
            }
        }
    }

    if out_positions.is_empty() {
        return None;
    }

    if out_indices.is_empty() {
        for i in (0..out_positions.len()).step_by(3) {
            if i + 2 < out_positions.len() {
                out_indices.push(i as u32);
                out_indices.push((i + 1) as u32);
                out_indices.push((i + 2) as u32);
            }
        }
    }

    let v_count = out_positions.len();
    Some(ExtractedMesh {
        positions: out_positions,
        normals: if has_normals { Some(out_normals) } else { None },
        uvs: if has_uvs { Some(out_uvs) } else { None },
        colors: if has_colors { Some(out_colors) } else { None },
        indices: out_indices,
        vertex_count: v_count,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Reader & Writer Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, String> {
    let mut b = [0u8; 1];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b[0])
}

fn write_u8<W: Write>(writer: &mut W, val: u8) -> Result<(), String> {
    writer.write_all(&[val]).map_err(|e| e.to_string())
}

fn read_u16<R: Read>(reader: &mut R, endian: EndianType) -> Result<u16, String> {
    let mut b = [0u8; 2];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(match endian {
        EndianType::LITTLE => u16::from_le_bytes(b),
        EndianType::BIG => u16::from_be_bytes(b),
    })
}

fn write_u16<W: Write>(writer: &mut W, val: u16, endian: EndianType) -> Result<(), String> {
    let b = match endian {
        EndianType::LITTLE => val.to_le_bytes(),
        EndianType::BIG => val.to_be_bytes(),
    };
    writer.write_all(&b).map_err(|e| e.to_string())
}

fn read_u32<R: Read>(reader: &mut R, endian: EndianType) -> Result<u32, String> {
    let mut b = [0u8; 4];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(match endian {
        EndianType::LITTLE => u32::from_le_bytes(b),
        EndianType::BIG => u32::from_be_bytes(b),
    })
}

fn write_u32<W: Write>(writer: &mut W, val: u32, endian: EndianType) -> Result<(), String> {
    let b = match endian {
        EndianType::LITTLE => val.to_le_bytes(),
        EndianType::BIG => val.to_be_bytes(),
    };
    writer.write_all(&b).map_err(|e| e.to_string())
}

fn read_f32<R: Read>(reader: &mut R, endian: EndianType) -> Result<f32, String> {
    let mut b = [0u8; 4];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(match endian {
        EndianType::LITTLE => f32::from_le_bytes(b),
        EndianType::BIG => f32::from_be_bytes(b),
    })
}

fn write_f32<W: Write>(writer: &mut W, val: f32, endian: EndianType) -> Result<(), String> {
    let b = match endian {
        EndianType::LITTLE => val.to_le_bytes(),
        EndianType::BIG => val.to_be_bytes(),
    };
    writer.write_all(&b).map_err(|e| e.to_string())
}

fn read_vector3<R: Read>(reader: &mut R, endian: EndianType) -> Result<Vector3, String> {
    Ok(Vector3 {
        x: read_f32(reader, endian)?,
        y: read_f32(reader, endian)?,
        z: read_f32(reader, endian)?,
    })
}

fn write_vector3<W: Write>(
    writer: &mut W,
    vec: &Vector3,
    endian: EndianType,
) -> Result<(), String> {
    write_f32(writer, vec.x, endian)?;
    write_f32(writer, vec.y, endian)?;
    write_f32(writer, vec.z, endian)
}

fn read_matrix3<R: Read>(reader: &mut R, endian: EndianType) -> Result<Matrix3, String> {
    let mut rows = [[0.0f32; 3]; 3];
    for r in &mut rows {
        for val in r {
            *val = read_f32(reader, endian)?;
        }
    }
    Ok(Matrix3 { rows })
}

fn write_matrix3<W: Write>(
    writer: &mut W,
    mat: &Matrix3,
    endian: EndianType,
) -> Result<(), String> {
    for r in &mat.rows {
        for &val in r {
            write_f32(writer, val, endian)?;
        }
    }
    Ok(())
}

fn read_sized_string<R: Read>(reader: &mut R, endian: EndianType) -> Result<String, String> {
    let len = read_u32(reader, endian)? as usize;
    if len > 100_000 {
        return Err(format!("String length too large: {} bytes", len));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn write_sized_string<W: Write>(writer: &mut W, s: &str, endian: EndianType) -> Result<(), String> {
    let bytes = s.as_bytes();
    write_u32(writer, bytes.len() as u32, endian)?;
    writer.write_all(bytes).map_err(|e| e.to_string())
}

fn read_link_id<R: Read>(reader: &mut R, endian: EndianType) -> Result<Option<u32>, String> {
    let val = read_u32(reader, endian)?;
    Ok(if val == 0xFFFF_FFFF { None } else { Some(val) })
}

fn write_link_id<W: Write>(
    writer: &mut W,
    link: Option<u32>,
    endian: EndianType,
) -> Result<(), String> {
    write_u32(writer, link.unwrap_or(0xFFFF_FFFF), endian)
}

fn read_multiple_link_ids<R: Read>(
    reader: &mut R,
    endian: EndianType,
) -> Result<Vec<Option<u32>>, String> {
    let count = read_u32(reader, endian)? as usize;
    if count > 100_000 {
        return Err(format!("Link count too large: {}", count));
    }
    let mut links = Vec::with_capacity(count);
    for _ in 0..count {
        links.push(read_link_id(reader, endian)?);
    }
    Ok(links)
}

fn write_multiple_link_ids<W: Write>(
    writer: &mut W,
    links: &[Option<u32>],
    endian: EndianType,
) -> Result<(), String> {
    write_u32(writer, links.len() as u32, endian)?;
    for &l in links {
        write_link_id(writer, l, endian)?;
    }
    Ok(())
}

fn read_string_or_index<R: Read>(
    reader: &mut R,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<String, String> {
    if version >= 0x14010001 {
        let idx = read_u32(reader, endian)? as usize;
        Ok(strings.get(idx).cloned().unwrap_or_default())
    } else {
        read_sized_string(reader, endian)
    }
}

fn write_string_or_index<W: Write>(
    writer: &mut W,
    s: &str,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<(), String> {
    if version >= 0x14010001 {
        let idx = strings.iter().position(|str_val| str_val == s).unwrap_or(0) as u32;
        write_u32(writer, idx, endian)
    } else {
        write_sized_string(writer, s, endian)
    }
}

fn read_object_net_data<R: Read>(
    reader: &mut R,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<NiObjectNETData, String> {
    let name = read_string_or_index(reader, endian, version, strings)?;
    let extra_data_links = read_multiple_link_ids(reader, endian)?;
    let controller_link = read_link_id(reader, endian)?;
    Ok(NiObjectNETData {
        name,
        extra_data_links,
        controller_link,
    })
}

fn write_object_net_data<W: Write>(
    writer: &mut W,
    data: &NiObjectNETData,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<(), String> {
    write_string_or_index(writer, &data.name, endian, version, strings)?;
    write_multiple_link_ids(writer, &data.extra_data_links, endian)?;
    write_link_id(writer, data.controller_link, endian)
}

fn read_av_object_data<R: Read>(
    reader: &mut R,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<NiAVObjectData, String> {
    let base = read_object_net_data(reader, endian, version, strings)?;
    let flags = read_u16(reader, endian)?;
    let translation = read_vector3(reader, endian)?;
    let rotation = read_matrix3(reader, endian)?;
    let scale = read_f32(reader, endian)?;
    let transform = NiTransform {
        translation,
        rotation,
        scale,
    };
    let property_links = read_multiple_link_ids(reader, endian)?;
    let collision_object_link = read_link_id(reader, endian)?;
    Ok(NiAVObjectData {
        base,
        flags,
        transform,
        property_links,
        collision_object_link,
    })
}

fn write_av_object_data<W: Write>(
    writer: &mut W,
    data: &NiAVObjectData,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<(), String> {
    write_object_net_data(writer, &data.base, endian, version, strings)?;
    write_u16(writer, data.flags, endian)?;
    write_vector3(writer, &data.transform.translation, endian)?;
    write_matrix3(writer, &data.transform.rotation, endian)?;
    write_f32(writer, data.transform.scale, endian)?;
    write_multiple_link_ids(writer, &data.property_links, endian)?;
    write_link_id(writer, data.collision_object_link, endian)
}

fn read_render_object_data<R: Read>(
    reader: &mut R,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<NiRenderObjectData, String> {
    let base = read_av_object_data(reader, endian, version, strings)?;
    let num_materials = read_u32(reader, endian)? as usize;
    if num_materials > 1000 {
        return Err(format!("Material count too large: {}", num_materials));
    }
    let mut names = Vec::with_capacity(num_materials);
    for _ in 0..num_materials {
        names.push(read_string_or_index(reader, endian, version, strings)?);
    }
    let mut materials = Vec::with_capacity(num_materials);
    for name in names {
        let extra_data = read_u32(reader, endian)?;
        materials.push(MaterialEntry { name, extra_data });
    }
    let active_material = read_u32(reader, endian)? as i32;
    let needs_update_default = read_u8(reader)?;
    Ok(NiRenderObjectData {
        base,
        materials,
        active_material,
        needs_update_default,
    })
}

fn write_render_object_data<W: Write>(
    writer: &mut W,
    data: &NiRenderObjectData,
    endian: EndianType,
    version: u32,
    strings: &[String],
) -> Result<(), String> {
    write_av_object_data(writer, &data.base, endian, version, strings)?;
    write_u32(writer, data.materials.len() as u32, endian)?;
    for m in &data.materials {
        write_string_or_index(writer, &m.name, endian, version, strings)?;
    }
    for m in &data.materials {
        write_u32(writer, m.extra_data, endian)?;
    }
    write_u32(writer, data.active_material as u32, endian)?;
    write_u8(writer, data.needs_update_default)
}

fn read_mesh_data<R: Read + Seek>(
    reader: &mut R,
    endian: EndianType,
    version: u32,
    user_version: u32,
    strings: &[String],
) -> Result<NiMeshData, String> {
    let base = read_render_object_data(reader, endian, version, strings)?;
    let primitive_type = read_u32(reader, endian)?;

    let mut em1_fields=Vec::new();
    if version == 0x14060500 {
        let count=2+usize::from(user_version>14)+if user_version>3 {3}else{0};
        for _ in 0..count {em1_fields.push(read_u32(reader,endian)?);}
    }

    let submesh_count = read_u16(reader, endian)?;
    let instancing_enabled = read_u8(reader)?;
    let bound_center = read_vector3(reader, endian)?;
    let bound_radius = read_f32(reader, endian)?;

    let stream_ref_count = read_u32(reader, endian)? as usize;
    if stream_ref_count > 1000 {
        return Err("Stream ref count too large".into());
    }
    let mut stream_refs = Vec::with_capacity(stream_ref_count);
    for _ in 0..stream_ref_count {
        let stream_link = read_u32(reader, endian)?;
        let is_per_instance = read_u8(reader)?;
        let num_submesh_regions = read_u16(reader, endian)? as usize;
        if num_submesh_regions > 1000 {
            return Err("Submesh region count too large".into());
        }
        let mut submesh_region_indices = Vec::with_capacity(num_submesh_regions);
        for _ in 0..num_submesh_regions {
            submesh_region_indices.push(read_u16(reader, endian)?);
        }
        let num_semantics = read_u32(reader, endian)? as usize;
        if num_semantics > 1000 {
            return Err("Semantics count too large".into());
        }
        let mut semantics = Vec::with_capacity(num_semantics);
        for _ in 0..num_semantics {
            let name = read_string_or_index(reader, endian, version, strings)?;
            let index = read_u32(reader, endian)?;
            semantics.push(SemanticData { name, index });
        }
        stream_refs.push(NiDataStreamRef {
            stream_link,
            is_per_instance,
            submesh_region_indices,
            semantics,
        });
    }
    let modifiers = read_multiple_link_ids(reader, endian)?;
    let hw_skin_flag = if mesh_has_skin_tag(version,user_version) {
        read_u8(reader)?
    } else {
        0
    };

    Ok(NiMeshData {
        base,
        primitive_type,
        em1_fields,
        submesh_count,
        instancing_enabled,
        bound_center,
        bound_radius,
        stream_refs,
        modifiers,
        hw_skin_flag,
    })
}

// EM1 NiMesh::LoadBinary8054722c..80547248 skips the embedded-object tag
// for user versions below10. Preserve the existing other-format contract.
fn mesh_has_skin_tag(version:u32,user_version:u32)->bool {
    version>=0x0A000000 && (version!=0x14060500 || user_version>=10)
}

fn write_mesh_extension<W:Write>(writer:&mut W,mesh:&NiMeshData,endian:EndianType,version:u32,user:u32)->Result<(),String> {
    let count=if version==0x14060500 {2+usize::from(user>14)+if user>3 {3}else{0}}else{0};
    if mesh.em1_fields.len()!=count {return Err(format!("Mesh requires {count} retained EM1 prefix words, found {}",mesh.em1_fields.len()));}
    for &word in &mesh.em1_fields {write_u32(writer,word,endian)?;}
    Ok(())
}

fn read_block<R: Read + Seek>(
    reader: &mut R,
    type_name: &str,
    block_size: u32,
    block_start_pos: u64,
    endian: EndianType,
    version: u32,
    user_version: u32,
    strings: &[String],
) -> Result<NiObject, String> {
    if type_name == "NiCamera" && version >= 0x14000004 {
        let base = read_av_object_data(reader, endian, version, strings)?;
        let obsolete_flags = read_u16(reader, endian)?;
        let mut frustum = [0.;6]; for value in &mut frustum {*value=read_f32(reader,endian)?;}
        let orthographic = read_u8(reader)?;
        let mut viewport = [0.;4]; for value in &mut viewport {*value=read_f32(reader,endian)?;}
        let lod_adjust = read_f32(reader,endian)?;
        let scene_link = read_link_id(reader,endian)?;
        let screen_polygons = read_multiple_link_ids(reader,endian)?;
        let screen_textures = read_multiple_link_ids(reader,endian)?;
        return Ok(NiObject::NiCamera(NiCameraData {base,obsolete_flags,frustum,orthographic,viewport,lod_adjust,
            scene_link,screen_polygons,screen_textures}));
    }
    if type_name == "NiSortAdjustNode" {
        let base = read_av_object_data(reader, endian, version, strings)?;
        let children = read_multiple_link_ids(reader, endian)?;
        let effects = read_multiple_link_ids(reader, endian)?;
        let sorting_mode = read_u32(reader, endian)?;
        let legacy_accumulator = if version < 0x14000004 {
            read_link_id(reader, endian)?
        } else { None };
        return Ok(NiObject::NiSortAdjustNode(NiSortAdjustNodeData {
            base: NiNodeData {base, children, effects}, sorting_mode, legacy_accumulator,
        }));
    }
    if type_name.starts_with("NiNode")
        || type_name.starts_with("NiBillboardNode")
        || type_name.starts_with("NiLODNode")
        || type_name.starts_with("NiSwitchNode")
        || type_name.starts_with("BSFadeNode")
        || type_name.starts_with("BSTreeNode")
        || type_name.starts_with("JPSJigsawNode")
    {
        let mut base = read_av_object_data(reader, endian, version, strings)?;
        base.flags |= 0x0100;
        let children = read_multiple_link_ids(reader, endian)?;
        let effects = read_multiple_link_ids(reader, endian)?;
        return Ok(NiObject::NiNode(NiNodeData {
            base,
            children,
            effects,
        }));
    }
    if type_name.starts_with("NiPSParticleSystem")
        || type_name.starts_with("NiPSMeshParticleSystem")
        || type_name.starts_with("NonUniformParticleSystem")
        || type_name.starts_with("RibbonParticleSystem")
    {
        let base = read_mesh_data(reader, endian, version, user_version, strings)?;

        let simulator_link = read_link_id(reader, endian)?;
        let generator_link = read_link_id(reader, endian)?;

        let num_emitters = read_u32(reader, endian)? as usize;
        if num_emitters > 1000 {
            return Err("Emitter count too large".into());
        }
        let mut emitter_links = Vec::with_capacity(num_emitters);
        for _ in 0..num_emitters {
            emitter_links.push(read_link_id(reader, endian)?);
        }

        let num_spawners = read_u32(reader, endian)? as usize;
        if num_spawners > 1000 {
            return Err("Spawner count too large".into());
        }
        let mut spawner_links = Vec::with_capacity(num_spawners);
        for _ in 0..num_spawners {
            spawner_links.push(read_link_id(reader, endian)?);
        }

        let bound_updater_link = read_link_id(reader, endian)?;
        let max_num_particles = read_u32(reader, endian)?;

        let flag1 = read_u8(reader)?;
        let flag2 = read_u8(reader)?;
        let flag3 = read_u8(reader)?;
        let flag4 = if version >= 0x14060064 {
            read_u8(reader)?
        } else {
            0
        };
        let flag5 = read_u8(reader)?;

        let (
            normal_method,
            normal_direction,
            up_method,
            up_direction,
            living_spawner_link,
            rotations,
        ) = if version >= 0x14060064 {
            let norm_meth = read_u32(reader, endian)?;
            let norm_dir = read_vector3(reader, endian)?;
            let up_meth = read_u32(reader, endian)?;
            let up_dir = read_vector3(reader, endian)?;
            let living_spawner = read_link_id(reader, endian)?;

            let num_rotations = read_u8(reader)? as usize;
            let mut rots = Vec::with_capacity(num_rotations.min(256));
            for _ in 0..num_rotations {
                let initial_axis = read_f32(reader, endian)?;
                let rotation_speed = read_f32(reader, endian)?;
                rots.push(RotationKey {
                    initial_axis,
                    rotation_speed,
                });
            }
            (norm_meth, norm_dir, up_meth, up_dir, living_spawner, rots)
        } else {
            (
                0,
                Vector3::default(),
                0,
                Vector3::default(),
                None,
                Vec::new(),
            )
        };

        let (unknown_has_float, unknown_float) = if user_version >= 11 {
            let has_f = read_u16(reader, endian)?;
            let val = if has_f != 0 {
                Some(read_f32(reader, endian)?)
            } else {
                None
            };
            (has_f, val)
        } else {
            (0, None)
        };

        let mut extension=Vec::new();
        if block_size>0 && version>=0x14020005 {
            let position=reader.stream_position().map_err(|e|e.to_string())?;
            let end=block_start_pos.checked_add(u64::from(block_size)).ok_or("Particle block size overflow")?;
            if position>end {return Err("Particle prefix exceeds its block".into());}
            extension.resize((end-position) as usize,0);
            reader.read_exact(&mut extension).map_err(|e|e.to_string())?;
        }
        return Ok(NiObject::NiPSParticleSystem(NiPSParticleSystemData {
            base,
            simulator_link,
            generator_link,
            emitter_links,
            spawner_links,
            bound_updater_link,
            max_num_particles,
            flag1,
            flag2,
            flag3,
            flag4,
            flag5,
            normal_method,
            normal_direction,
            up_method,
            up_direction,
            living_spawner_link,
            rotations,
            unknown_has_float,
            unknown_float,
            extension,
        }));
    }
    if type_name.starts_with("NiMesh") {
        let mesh = read_mesh_data(reader, endian, version, user_version, strings)?;
        return Ok(NiObject::NiMesh(mesh));
    }
    if type_name.starts_with("NiDataStream") {
        let size_in_bytes = read_u32(reader, endian)?;
        if size_in_bytes > 50_000_000 {
            return Err("NiDataStream size too large".into());
        }
        let cloning_behavior = read_u32(reader, endian)?;
        let num_regions = read_u32(reader, endian)? as usize;
        if num_regions > 1000 {
            return Err("Regions count too large".into());
        }
        let mut regions = Vec::with_capacity(num_regions);
        for _ in 0..num_regions {
            regions.push(NiDataStreamRegion {
                start_index: read_u32(reader, endian)?,
                count: read_u32(reader, endian)?,
            });
        }
        let num_components = read_u32(reader, endian)? as usize;
        if num_components > 1000 {
            return Err("Component format count too large".into());
        }
        let mut component_formats = Vec::with_capacity(num_components);
        for _ in 0..num_components {
            component_formats.push(read_u32(reader, endian)?);
        }
        let mut buffer = vec![0u8; size_in_bytes as usize];
        reader.read_exact(&mut buffer).map_err(|e| e.to_string())?;
        let streamable = read_u8(reader)?;

        return Ok(NiObject::NiDataStream(NiDataStreamData {
            size_in_bytes,
            cloning_behavior,
            regions,
            component_formats,
            buffer,
            streamable,
        }));
    }
    if type_name.starts_with("NiWiiUPersistentSrcTextureRendererData")
        || type_name.starts_with("NiPersistentSrcTextureRendererData")
    {
        let format = read_u32(reader, endian)?;
        let bits_per_pixel = read_u8(reader)?;
        let renderer_hint = read_u32(reader, endian)?;
        let extra_data = read_u32(reader, endian)?;
        let flags = read_u8(reader)?;
        let tiling = read_u32(reader, endian)?;
        let srgb_space = if version >= 0x14030004 {
            read_u8(reader)?
        } else {
            0
        };

        for _ in 0..4 {
            let _component = read_u32(reader, endian)?;
            let _representation = read_u32(reader, endian)?;
            let _bits_per_comp = read_u8(reader)?;
            let _is_signed = read_u8(reader)?;
        }

        let pixel_format = NiPixelFormatData {
            format,
            bits_per_pixel,
            renderer_hint,
            extra_data,
            flags,
            tiling,
            srgb_space,
        };
        let palette_link = read_link_id(reader, endian)?;

        let mip_count = read_u32(reader, endian)? as usize;
        let face_count_field = read_u32(reader, endian)?;

        let mut mip_levels = Vec::with_capacity(mip_count.min(16));
        for _ in 0..mip_count {
            let width = read_u32(reader, endian)?;
            let height = read_u32(reader, endian)?;
            let offset_in_bytes = read_u32(reader, endian)?;
            mip_levels.push(MipLevel {
                width,
                height,
                offset_in_bytes,
            });
        }

        let face_size = read_u32(reader, endian)?;
        let pad_offset = if version >= 0x14020006 {
            read_u32(reader, endian)?
        } else {
            face_size
        };

        let face_count = read_u32(reader, endian)?;
        let target_platform = read_u32(reader, endian)?;
        let pixel_stride = face_count_field;

        let gx2_format = if renderer_hint != 0xFFFF_FFFF {
            renderer_hint
        } else {
            match format {
                4 => 0x31,
                5 => 0x32,
                6 => 0x33,
                7 => 0x34,
                8 => 0x35,
                2 | 1 => 0x1a,
                _ => 0,
            }
        };

        let (width, height) = mip_levels
            .first()
            .map(|m| (m.width, m.height))
            .unwrap_or((0, 0));

        let current_pos = reader.stream_position().map_err(|e| e.to_string())?;
        let block_end = block_start_pos + block_size as u64;
        let remaining_bytes = block_end.saturating_sub(current_pos) as usize;
        let mut raw_pixels = vec![0u8; remaining_bytes];
        if remaining_bytes > 0 {
            let _ = reader.read_exact(&mut raw_pixels);
        }

        return Ok(NiObject::NiWiiUTextureData(
            NiWiiUPersistentSrcTextureRendererData {
                pixel_format,
                palette_link,
                mip_levels,
                face_size,
                pad_offset,
                face_count,
                target_platform,
                pixel_stride,
                raw_pixels,
                gx2_format,
                width,
                height,
            },
        ));
    }
    if type_name.starts_with("NiMaterialProperty") {
        let base = read_object_net_data(reader, endian, version, strings)?;
        let ambient = read_vector3(reader, endian)?;
        let diffuse = read_vector3(reader, endian)?;
        let specular = read_vector3(reader, endian)?;
        let emissive = read_vector3(reader, endian)?;
        let shininess = read_f32(reader, endian)?;
        let alpha = read_f32(reader, endian)?;
        return Ok(NiObject::NiMaterialProperty(NiMaterialPropertyData {
            base,
            ambient,
            diffuse,
            specular,
            emissive,
            shininess,
            alpha,
        }));
    }
    if type_name.starts_with("NiAlphaProperty") {
        let base = read_object_net_data(reader, endian, version, strings)?;
        let flags = read_u16(reader, endian)?;
        let threshold = read_u8(reader)?;
        return Ok(NiObject::NiAlphaProperty(NiAlphaPropertyData {
            base,
            flags,
            threshold,
        }));
    }
    if type_name.starts_with("NiSourceTexture") {
        let base = read_object_net_data(reader, endian, version, strings)?;
        let use_external = read_u8(reader)?;
        let file_name = Some(read_string_or_index(reader, endian, version, strings)?);
        let link = read_u32(reader, endian)?;
        let pixel_data_link = if link == 0xFFFF_FFFF {
            None
        } else {
            Some(link)
        };
        let pixel_layout = read_u32(reader, endian)?;
        let use_mipmaps = read_u32(reader, endian)?;
        let alpha_format = read_u32(reader, endian)?;
        let is_static = read_u8(reader)?;
        let direct_render = read_u8(reader)?;
        let persist_render_data = read_u8(reader)?;
        return Ok(NiObject::NiSourceTexture(NiSourceTextureData {
            base,
            use_external,
            file_name,
            pixel_data_link,
            pixel_layout,
            use_mipmaps,
            alpha_format,
            is_static,
            direct_render,
            persist_render_data,
        }));
    }
    if type_name == "NiStringExtraData" {
        let base = if version >= 0x14010001 {
            NiObjectNETData { name: read_string_or_index(reader, endian, version, strings)?, ..Default::default() }
        } else { read_object_net_data(reader, endian, version, strings)? };
        let string_data = read_string_or_index(reader, endian, version, strings)?;
        return Ok(NiObject::NiStringExtraData(NiStringExtraDataData {
            base,
            string_data,
        }));
    }
    if type_name == "NiIntegerExtraData" {
        let base = if version >= 0x14010001 {
            NiObjectNETData { name: read_string_or_index(reader, endian, version, strings)?, ..Default::default() }
        } else { read_object_net_data(reader, endian, version, strings)? };
        let value = read_u32(reader, endian)? as i32;
        return Ok(NiObject::NiIntegerExtraData(NiIntegerExtraDataData {
            base,
            value,
        }));
    }
    if type_name == "NiFloatExtraData" {
        let base = if version >= 0x14010001 {
            NiObjectNETData { name: read_string_or_index(reader, endian, version, strings)?, ..Default::default() }
        } else { read_object_net_data(reader, endian, version, strings)? };
        let value = read_f32(reader, endian)?;
        return Ok(NiObject::NiFloatExtraData(NiFloatExtraDataData {
            base,
            value,
        }));
    }
    if type_name == "NiBooleanExtraData" {
        let base = if version >= 0x14010001 {
            NiObjectNETData { name: read_string_or_index(reader, endian, version, strings)?, ..Default::default() }
        } else { read_object_net_data(reader, endian, version, strings)? };
        let value = read_u8(reader)?;
        return Ok(NiObject::NiBooleanExtraData(NiBooleanExtraDataData {
            base,
            value,
        }));
    }

    let mut raw_data = vec![0u8; block_size as usize];
    if block_size > 0 {
        let _ = reader.read_exact(&mut raw_data);
    }
    Ok(NiObject::Unknown(RawBlockData {
        type_name: type_name.to_string(),
        raw_data,
    }))
}

fn write_block<W: Write>(
    writer: &mut W,
    block: &NiObject,
    endian: EndianType,
    version: u32,
    user_version: u32,
    strings: &[String],
) -> Result<(), String> {
    match block {
        NiObject::NiNode(n) => {
            write_av_object_data(writer, &n.base, endian, version, strings)?;
            write_multiple_link_ids(writer, &n.children, endian)?;
            write_multiple_link_ids(writer, &n.effects, endian)
        }
        NiObject::NiCamera(c) => {
            if version < 0x14000004 {return Err("NiCamera writing for this legacy version is not verified".into());}
            write_av_object_data(writer,&c.base,endian,version,strings)?;
            write_u16(writer,c.obsolete_flags,endian)?;
            for &value in &c.frustum {write_f32(writer,value,endian)?;}
            write_u8(writer,c.orthographic)?;
            for &value in &c.viewport {write_f32(writer,value,endian)?;}
            write_f32(writer,c.lod_adjust,endian)?;
            write_link_id(writer,c.scene_link,endian)?;
            write_multiple_link_ids(writer,&c.screen_polygons,endian)?;
            write_multiple_link_ids(writer,&c.screen_textures,endian)
        }
        NiObject::NiSortAdjustNode(n) => {
            write_av_object_data(writer, &n.base.base, endian, version, strings)?;
            write_multiple_link_ids(writer, &n.base.children, endian)?;
            write_multiple_link_ids(writer, &n.base.effects, endian)?;
            write_u32(writer, n.sorting_mode, endian)?;
            if version < 0x14000004 {
                write_link_id(writer, n.legacy_accumulator, endian)?;
            }
            Ok(())
        }
        NiObject::NiMesh(m) => {
            write_render_object_data(writer, &m.base, endian, version, strings)?;
            write_u32(writer, m.primitive_type, endian)?;
            write_mesh_extension(writer,m,endian,version,user_version)?;
            write_u16(writer, m.submesh_count, endian)?;
            write_u8(writer, m.instancing_enabled)?;
            write_vector3(writer, &m.bound_center, endian)?;
            write_f32(writer, m.bound_radius, endian)?;
            write_u32(writer, m.stream_refs.len() as u32, endian)?;
            for sref in &m.stream_refs {
                write_u32(writer, sref.stream_link, endian)?;
                write_u8(writer, sref.is_per_instance)?;
                write_u16(writer, sref.submesh_region_indices.len() as u16, endian)?;
                for &idx in &sref.submesh_region_indices {
                    write_u16(writer, idx, endian)?;
                }
                write_u32(writer, sref.semantics.len() as u32, endian)?;
                for sem in &sref.semantics {
                    write_string_or_index(writer, &sem.name, endian, version, strings)?;
                    write_u32(writer, sem.index, endian)?;
                }
            }
            write_multiple_link_ids(writer, &m.modifiers, endian)?;
            if mesh_has_skin_tag(version,user_version) {
                write_u8(writer, m.hw_skin_flag)?;
            }
            Ok(())
        }
        NiObject::NiPSParticleSystem(ps) => {
            write_render_object_data(writer, &ps.base.base, endian, version, strings)?;
            write_u32(writer, ps.base.primitive_type, endian)?;
            write_mesh_extension(writer,&ps.base,endian,version,user_version)?;
            write_u16(writer, ps.base.submesh_count, endian)?;
            write_u8(writer, ps.base.instancing_enabled)?;
            write_vector3(writer, &ps.base.bound_center, endian)?;
            write_f32(writer, ps.base.bound_radius, endian)?;
            write_u32(writer, ps.base.stream_refs.len() as u32, endian)?;
            for sref in &ps.base.stream_refs {
                write_u32(writer, sref.stream_link, endian)?;
                write_u8(writer, sref.is_per_instance)?;
                write_u16(writer, sref.submesh_region_indices.len() as u16, endian)?;
                for &idx in &sref.submesh_region_indices {
                    write_u16(writer, idx, endian)?;
                }
                write_u32(writer, sref.semantics.len() as u32, endian)?;
                for sem in &sref.semantics {
                    write_string_or_index(writer, &sem.name, endian, version, strings)?;
                    write_u32(writer, sem.index, endian)?;
                }
            }
            write_multiple_link_ids(writer, &ps.base.modifiers, endian)?;
            if mesh_has_skin_tag(version,user_version) {
                write_u8(writer, ps.base.hw_skin_flag)?;
            }

            write_link_id(writer, ps.simulator_link, endian)?;
            write_link_id(writer, ps.generator_link, endian)?;

            write_u32(writer, ps.emitter_links.len() as u32, endian)?;
            for &link in &ps.emitter_links {
                write_link_id(writer, link, endian)?;
            }

            write_u32(writer, ps.spawner_links.len() as u32, endian)?;
            for &link in &ps.spawner_links {
                write_link_id(writer, link, endian)?;
            }

            write_link_id(writer, ps.bound_updater_link, endian)?;
            write_u32(writer, ps.max_num_particles, endian)?;

            write_u8(writer, ps.flag1)?;
            write_u8(writer, ps.flag2)?;
            write_u8(writer, ps.flag3)?;
            if version >= 0x14060064 {
                write_u8(writer, ps.flag4)?;
            }
            write_u8(writer, ps.flag5)?;

            if version >= 0x14060064 {
                write_u32(writer, ps.normal_method, endian)?;
                write_vector3(writer, &ps.normal_direction, endian)?;
                write_u32(writer, ps.up_method, endian)?;
                write_vector3(writer, &ps.up_direction, endian)?;
                write_link_id(writer, ps.living_spawner_link, endian)?;

                write_u8(writer, ps.rotations.len() as u8)?;
                for rot in &ps.rotations {
                    write_f32(writer, rot.initial_axis, endian)?;
                    write_f32(writer, rot.rotation_speed, endian)?;
                }
            }

            if user_version >= 11 {
                write_u16(writer, ps.unknown_has_float, endian)?;
                if let Some(val) = ps.unknown_float {
                    write_f32(writer, val, endian)?;
                }
            }
            writer.write_all(&ps.extension).map_err(|e|e.to_string())?;

            Ok(())
        }
        NiObject::NiDataStream(ds) => {
            write_u32(writer, ds.size_in_bytes, endian)?;
            write_u32(writer, ds.cloning_behavior, endian)?;
            write_u32(writer, ds.regions.len() as u32, endian)?;
            for r in &ds.regions {
                write_u32(writer, r.start_index, endian)?;
                write_u32(writer, r.count, endian)?;
            }
            write_u32(writer, ds.component_formats.len() as u32, endian)?;
            for &cf in &ds.component_formats {
                write_u32(writer, cf, endian)?;
            }
            writer.write_all(&ds.buffer).map_err(|e| e.to_string())?;
            write_u8(writer, ds.streamable)
        }
        NiObject::NiMaterialProperty(mat) => {
            write_object_net_data(writer, &mat.base, endian, version, strings)?;
            write_vector3(writer, &mat.ambient, endian)?;
            write_vector3(writer, &mat.diffuse, endian)?;
            write_vector3(writer, &mat.specular, endian)?;
            write_vector3(writer, &mat.emissive, endian)?;
            write_f32(writer, mat.shininess, endian)?;
            write_f32(writer, mat.alpha, endian)
        }
        NiObject::NiAlphaProperty(alpha) => {
            write_object_net_data(writer, &alpha.base, endian, version, strings)?;
            write_u16(writer, alpha.flags, endian)?;
            write_u8(writer, alpha.threshold)
        }
        NiObject::NiSourceTexture(tex) => {
            write_object_net_data(writer, &tex.base, endian, version, strings)?;
            write_u8(writer, tex.use_external)?;
            write_string_or_index(
                writer,
                tex.file_name.as_deref().unwrap_or(""),
                endian,
                version,
                strings,
            )?;
            write_link_id(writer, tex.pixel_data_link, endian)?;
            write_u32(writer, tex.pixel_layout, endian)?;
            write_u32(writer, tex.use_mipmaps, endian)?;
            write_u32(writer, tex.alpha_format, endian)?;
            write_u8(writer, tex.is_static)?;
            write_u8(writer, tex.direct_render)?;
            write_u8(writer, tex.persist_render_data)
        }
        NiObject::NiStringExtraData(s) => {
            if version >= 0x14010001 {
                write_string_or_index(writer, &s.base.name, endian, version, strings)?;
            } else { write_object_net_data(writer, &s.base, endian, version, strings)?; }
            write_string_or_index(writer, &s.string_data, endian, version, strings)
        }
        NiObject::NiIntegerExtraData(i) => {
            if version >= 0x14010001 {
                write_string_or_index(writer, &i.base.name, endian, version, strings)?;
            } else { write_object_net_data(writer, &i.base, endian, version, strings)?; }
            write_u32(writer, i.value as u32, endian)
        }
        NiObject::NiFloatExtraData(f) => {
            if version >= 0x14010001 {
                write_string_or_index(writer, &f.base.name, endian, version, strings)?;
            } else { write_object_net_data(writer, &f.base, endian, version, strings)?; }
            write_f32(writer, f.value, endian)
        }
        NiObject::NiBooleanExtraData(b) => {
            if version >= 0x14010001 {
                write_string_or_index(writer, &b.base.name, endian, version, strings)?;
            } else { write_object_net_data(writer, &b.base, endian, version, strings)?; }
            write_u8(writer, b.value)
        }
        NiObject::Unknown(raw) => writer.write_all(&raw.raw_data).map_err(|e| e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod compact_extra_data_tests {
    use super::*;
    #[test]
    fn extra_data_controllers_are_not_scalar_extra_data() {
        for endian in [EndianType::BIG,EndianType::LITTLE] {
            for kind in ["NiFloatExtraDataController","NiStringExtraDataController",
                "NiIntegerExtraDataController","NiBooleanExtraDataController"] {
                let payload:Vec<u8>=(0..34).collect();
                let mut adjacent=payload.clone();adjacent.extend_from_slice(&[0xee;8]);
                let mut reader=Cursor::new(adjacent);
                let block=read_block(&mut reader,kind,34,0,endian,0x14030009,0,&[]).unwrap();
                let NiObject::Unknown(raw)=&block else {panic!("{kind} was misclassified")};
                assert_eq!(raw.type_name,kind);assert_eq!(raw.raw_data,payload);
                assert_eq!(reader.position(),34);
                let mut encoded=Vec::new();
                write_block(&mut encoded,&block,endian,0x14030009,0,&[]).unwrap();
                assert_eq!(encoded,payload);
            }
        }
    }
    #[test]
    fn modern_extra_data_uses_name_and_value_without_object_net_links() {
        for endian in [EndianType::BIG,EndianType::LITTLE] {
            let strings=vec!["parameter".to_owned(),"value".to_owned()];
            for (kind,value) in [("NiIntegerExtraData",7u32),("NiFloatExtraData",0.5f32.to_bits()),("NiStringExtraData",1u32)] {
                let mut payload=Vec::new();
                write_u32(&mut payload,0,endian).unwrap();
                write_u32(&mut payload,value,endian).unwrap();
                let mut adjacent=payload.clone();adjacent.extend_from_slice(&[0xff;16]);
                let mut reader=Cursor::new(adjacent);
                let block=read_block(&mut reader,kind,8,0,endian,0x14030009,0,&strings).unwrap();
                assert_eq!(reader.position(),8,"read past {kind}");
                let mut encoded=Vec::new();
                write_block(&mut encoded,&block,endian,0x14030009,0,&strings).unwrap();
                assert_eq!(encoded,payload,"{kind} wire format changed");
            }
            let mut payload=Vec::new();write_u32(&mut payload,0,endian).unwrap();payload.push(1);
            let mut reader=Cursor::new(payload.clone());
            let block=read_block(&mut reader,"NiBooleanExtraData",5,0,endian,0x14030009,0,&strings).unwrap();
            assert_eq!(reader.position(),5);
            let mut encoded=Vec::new();
            write_block(&mut encoded,&block,endian,0x14030009,0,&strings).unwrap();
            assert_eq!(encoded,payload);
        }
    }
}

#[cfg(test)]
mod sort_adjust_node_tests {
    use super::*;
    #[test]
    fn sort_adjust_preserves_wire_enum_links_and_endianness() {
        for endian in [EndianType::BIG, EndianType::LITTLE] {
            for version in [0x14000003, 0x14060500] {
                for mode in [0, 1, 2, u32::MAX] {
                    let mut node = NiSortAdjustNodeData::default();
                    node.base.base.flags = 0x12;
                    node.base.children = vec![Some(3), None, Some(9)];
                    node.base.effects = vec![Some(4)];
                    node.sorting_mode = mode;
                    node.legacy_accumulator = if version < 0x14000004 {Some(7)} else {None};
                    let block = NiObject::NiSortAdjustNode(node);
                    let mut bytes = Vec::new();
                    write_block(&mut bytes, &block, endian, version, 0, &[]).unwrap();
                    let mut adjacent = bytes.clone(); adjacent.extend_from_slice(&[0xee; 8]);
                    let mut reader = Cursor::new(adjacent);
                    let decoded = read_block(&mut reader, "NiSortAdjustNode", bytes.len() as u32, 0,
                        endian, version, 0, &[]).unwrap();
                    assert_eq!(reader.position(), bytes.len() as u64);
                    assert_eq!(decoded.children(), &[Some(3), None, Some(9)]);
                    assert_eq!(decoded.as_av_object().unwrap().flags, 0x12);
                    let NiObject::NiSortAdjustNode(sort) = &decoded else {panic!("Lost class")};
                    assert_eq!(sort.sorting_mode, mode);
                    assert_eq!(sort.legacy_accumulator, if version < 0x14000004 {Some(7)} else {None});
                    let mut encoded = Vec::new();
                    write_block(&mut encoded, &decoded, endian, version, 0, &[]).unwrap();
                    assert_eq!(bytes, encoded);
                }
            }
        }
    }
}

#[cfg(test)]mod camera_wire_tests {
    use super::*;
    #[test]fn modern_camera_roundtrips_both_endians_without_consuming_next_block() {
        for endian in [EndianType::BIG,EndianType::LITTLE] {
            let mut camera=NiCameraData::default();camera.base.flags=0x127;camera.base.transform.scale=1.;
            camera.obsolete_flags=0x9abc;camera.frustum=[-0.4,0.5,0.3,-0.2,1.,1000.];
            camera.orthographic=2;camera.viewport=[0.1,0.9,0.8,0.2];camera.lod_adjust=1.25;
            camera.scene_link=Some(8);camera.screen_polygons=vec![Some(2),None];camera.screen_textures=vec![Some(7)];
            let block=NiObject::NiCamera(camera);let mut payload=Vec::new();
            write_block(&mut payload,&block,endian,0x14060500,0,&[]).unwrap();
            let mut adjacent=payload.clone();adjacent.extend_from_slice(&[0xfe;12]);
            let mut reader=Cursor::new(adjacent);
            let decoded=read_block(&mut reader,"NiCamera",payload.len() as u32,0,endian,0x14060500,0,&[]).unwrap();
            assert_eq!(reader.position(),payload.len() as u64);assert_eq!(decoded.type_name(),"NiCamera");
            assert_eq!(decoded.as_av_object().unwrap().flags,0x127);
            let mut encoded=Vec::new();write_block(&mut encoded,&decoded,endian,0x14060500,0,&[]).unwrap();
            assert_eq!(encoded,payload);
        }
    }
}
