// hkx_file.rs — Havok binary packfile (.hkx/.hkt/.hkp) parser, 3D skeleton rig calculator, 3D collision extractor & serializer for Havok 2012.2.0

use crate::file_manipulator::EndianType;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};

pub const HKX_MAGIC_0: u32 = 0x57E0E057;
pub const HKX_MAGIC_1: u32 = 0x10C0C010;

// ─────────────────────────────────────────────────────────────────────────────
// Header & Section Structures
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HkxHeader {
    pub magic0: u32,
    pub magic1: u32,
    pub user_tag: i32,
    pub file_version: i32,
    pub pointer_size: u8, // 4 or 8
    pub endianness: u8,   // 1 = Little Endian, 0 = Big Endian
    pub padding_option: u8,
    pub base_format: u8,
    pub num_sections: i32,
    pub contents_section_index: i32,
    pub contents_section_offset: i32,
    pub contents_class_name_section_index: i32,
    pub contents_class_name_section_offset: i32,
    pub contents_version: String, // e.g. "hk_2012.1.0-r1"
    pub flags: i32,
    pub pad: i32,
}

impl Default for HkxHeader {
    fn default() -> Self {
        Self {
            magic0: HKX_MAGIC_0,
            magic1: HKX_MAGIC_1,
            user_tag: 0,
            file_version: 9,
            pointer_size: 4,
            endianness: 1,
            padding_option: 0,
            base_format: 1,
            num_sections: 3,
            contents_section_index: 2,
            contents_section_offset: 0,
            contents_class_name_section_index: 0,
            contents_class_name_section_offset: 0x4B,
            contents_version: "hk_2012.1.0-r1".to_string(),
            flags: 0,
            pad: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HkxSectionHeader {
    pub section_tag: String, // 19 bytes ASCII null-terminated, e.g. "__classnames__"
    pub absolute_data_start: i32,
    pub local_fixups_offset: i32,
    pub global_fixups_offset: i32,
    pub virtual_fixups_offset: i32,
    pub exports_offset: i32,
    pub imports_offset: i32,
    pub end_offset: i32,
}

impl Default for HkxSectionHeader {
    fn default() -> Self {
        Self {
            section_tag: "__data__".to_string(),
            absolute_data_start: 0,
            local_fixups_offset: 0,
            global_fixups_offset: 0,
            virtual_fixups_offset: 0,
            exports_offset: 0,
            imports_offset: 0,
            end_offset: 0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fixups, Bones, Skeletons, Collisions & Behavior Types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HkxLocalFixup {
    pub src_offset: u32,
    pub dst_offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HkxGlobalFixup {
    pub src_offset: u32,
    pub dst_section: u32,
    pub dst_offset: u32,
}

/// Virtual fixup / Finish entry — 12-byte entries in Havok 2012 binary packfiles:
/// `{ src_object_offset(4), section_index(4), class_name_offset(4) }`.
/// `class_name_offset` is a byte offset into the __classnames__ section pointing
/// directly at the null-terminated class name string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HkxVirtualFixup {
    /// Offset of the object's start within __data__ section data.
    pub src_offset: u32,
    /// Section index in packfile.
    pub section_index: u32,
    /// Byte offset into __classnames__ section pointing directly at the name string.
    pub class_name_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HkxClassName {
    pub offset: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HkxBone {
    pub name: String,
    pub parent_index: i16,
    pub local_translation: [f32; 3],
    pub local_rotation: [f32; 4], // Quaternion [x, y, z, w]
    pub local_scale: [f32; 3],
    pub lock_translation: bool,
}

impl Default for HkxBone {
    fn default() -> Self {
        Self {
            name: "Bone".to_string(),
            parent_index: -1,
            local_translation: [0.0, 0.0, 0.0],
            local_rotation: [0.0, 0.0, 0.0, 1.0],
            local_scale: [1.0, 1.0, 1.0],
            lock_translation: false,
        }
    }
}

fn rotate_vector_by_quaternion(v: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    let qx = q[0];
    let qy = q[1];
    let qz = q[2];
    let qw = q[3];
    let vx = v[0];
    let vy = v[1];
    let vz = v[2];

    let ix = qw * vx + qy * vz - qz * vy;
    let iy = qw * vy + qz * vx - qx * vz;
    let iz = qw * vz + qx * vy - qy * vx;
    let iw = -qx * vx - qy * vy - qz * vz;

    [
        ix * qw + iw * -qx + iy * -qz - iz * -qy,
        iy * qw + iw * -qy + iz * -qx - ix * -qz,
        iz * qw + iw * -qz + ix * -qy - iy * -qx,
    ]
}

fn multiply_quaternions(q1: [f32; 4], q2: [f32; 4]) -> [f32; 4] {
    [
        q1[3] * q2[0] + q1[0] * q2[3] + q1[1] * q2[2] - q1[2] * q2[1],
        q1[3] * q2[1] - q1[0] * q2[2] + q1[1] * q2[3] + q1[2] * q2[0],
        q1[3] * q2[2] + q1[0] * q2[1] - q1[1] * q2[0] + q1[2] * q2[3],
        q1[3] * q2[3] - q1[0] * q2[0] - q1[1] * q2[1] - q1[2] * q2[2],
    ]
}

fn slerp_quaternions(q1: [f32; 4], mut q2: [f32; 4], t: f32) -> [f32; 4] {
    let mut dot = q1[0] * q2[0] + q1[1] * q2[1] + q1[2] * q2[2] + q1[3] * q2[3];
    if dot < 0.0 {
        q2 = [-q2[0], -q2[1], -q2[2], -q2[3]];
        dot = -dot;
    }

    if dot > 0.9995 {
        let res = [
            q1[0] + (q2[0] - q1[0]) * t,
            q1[1] + (q2[1] - q1[1]) * t,
            q1[2] + (q2[2] - q1[2]) * t,
            q1[3] + (q2[3] - q1[3]) * t,
        ];
        let len = (res[0] * res[0] + res[1] * res[1] + res[2] * res[2] + res[3] * res[3]).sqrt();
        if len > 0.0 {
            [res[0] / len, res[1] / len, res[2] / len, res[3] / len]
        } else {
            q1
        }
    } else {
        let theta_0 = dot.clamp(-1.0, 1.0).acos();
        let theta = theta_0 * t;
        let sin_theta = theta.sin();
        let sin_theta_0 = theta_0.sin();

        let s0 = (theta_0 - theta).sin() / sin_theta_0;
        let s1 = sin_theta / sin_theta_0;

        [
            s0 * q1[0] + s1 * q2[0],
            s0 * q1[1] + s1 * q2[1],
            s0 * q1[2] + s1 * q2[2],
            s0 * q1[3] + s1 * q2[3],
        ]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxCollisionMesh {
    pub name: String,
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxObjectInfo {
    pub class_name: String,
    pub address: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxSkeleton {
    pub name: String,
    pub bones: Vec<HkxBone>,
}

impl HkxSkeleton {
    /// Compute 3D world-space positions for all bones in the hierarchy
    pub fn compute_world_positions(&self) -> Vec<[f32; 3]> {
        let mut world_pos = vec![[0.0f32; 3]; self.bones.len()];
        let mut world_rot = vec![[0.0f32, 0.0, 0.0, 1.0]; self.bones.len()]; // Quaternion [x, y, z, w]

        for i in 0..self.bones.len() {
            let bone = &self.bones[i];
            let p_idx = bone.parent_index;

            if p_idx >= 0 && (p_idx as usize) < i {
                let parent_p = world_pos[p_idx as usize];
                let parent_r = world_rot[p_idx as usize];

                // Rotate local translation by parent rotation quaternion
                let rotated_trans = rotate_vector_by_quaternion(bone.local_translation, parent_r);
                world_pos[i] = [
                    parent_p[0] + rotated_trans[0],
                    parent_p[1] + rotated_trans[1],
                    parent_p[2] + rotated_trans[2],
                ];
                world_rot[i] = multiply_quaternions(parent_r, bone.local_rotation);
            } else {
                world_pos[i] = bone.local_translation;
                world_rot[i] = bone.local_rotation;
            }
        }

        world_pos
    }
}

/// Keyframe transform sample (translation, rotation quaternion, scale)
/// `has_pos/rot/scale` = false means the animation channel is identity for this track;
/// `sample_pose` will keep the skeleton reference-pose value for that channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HkxTransformSample {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Whether this sample has actual position data (false = use reference pose)
    pub has_pos: bool,
    /// Whether this sample has actual rotation data (false = use reference pose)
    pub has_rot: bool,
    /// Whether this sample has actual scale data (false = use reference pose)
    pub has_scale: bool,
}

impl Default for HkxTransformSample {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
            has_pos: false,
            has_rot: false,
            has_scale: false,
        }
    }
}

/// Parsed skeletal animation track data
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxAnimation {
    pub name: String,
    pub duration: f32,
    pub num_frames: usize,
    pub num_tracks: usize,
    pub frame_duration: f32,
    pub animation_type: String,
    /// Maps animation track index -> skeleton bone index
    pub track_to_bone: Vec<i16>,
    /// Pre-sampled keyframe poses: `[frame_idx][track_idx]`
    pub frames: Vec<Vec<HkxTransformSample>>,
}

impl HkxAnimation {
    /// Sample interpolated pose at time `t` (in seconds).
    /// Returns modified bones matching the target skeleton.
    pub fn sample_pose(&self, time_sec: f32, skeleton: &HkxSkeleton) -> Vec<HkxBone> {
        let mut bones = skeleton.bones.clone();
        if self.num_frames == 0 || self.frames.is_empty() || self.duration <= 0.0 {
            return bones;
        }

        let clamped_time = time_sec.max(0.0) % self.duration;
        let norm_frame = if self.frame_duration > 0.0 {
            clamped_time / self.frame_duration
        } else {
            0.0
        };

        let f0 = (norm_frame.floor() as usize).min(self.num_frames - 1);
        let f1 = (f0 + 1) % self.num_frames;
        let factor = norm_frame.fract();

        let pose0 = &self.frames[f0];
        let pose1 = &self.frames[f1];

        for track_idx in 0..self.num_tracks.min(pose0.len()) {
            let bone_idx = if track_idx < self.track_to_bone.len() {
                self.track_to_bone[track_idx]
            } else {
                track_idx as i16
            };

            if bone_idx >= 0 && (bone_idx as usize) < bones.len() {
                let s0 = &pose0[track_idx];
                let s1 = pose1.get(track_idx).unwrap_or(s0);
                let target_bone = &mut bones[bone_idx as usize];

                // Translation: sampled local translation
                if s0.has_pos || s1.has_pos {
                    let anim_trans = [
                        s0.translation[0] + (s1.translation[0] - s0.translation[0]) * factor,
                        s0.translation[1] + (s1.translation[1] - s0.translation[1]) * factor,
                        s0.translation[2] + (s1.translation[2] - s0.translation[2]) * factor,
                    ];
                    target_bone.local_translation = anim_trans;
                }

                // Rotation: sampled local rotation
                if s0.has_rot || s1.has_rot {
                    let anim_rot = slerp_quaternions(s0.rotation, s1.rotation, factor);
                    target_bone.local_rotation = anim_rot;
                }

                // Scale: sampled local scale
                if s0.has_scale || s1.has_scale {
                    let anim_scale = [
                        s0.scale[0] + (s1.scale[0] - s0.scale[0]) * factor,
                        s0.scale[1] + (s1.scale[1] - s0.scale[1]) * factor,
                        s0.scale[2] + (s1.scale[2] - s0.scale[2]) * factor,
                    ];
                    target_bone.local_scale = anim_scale;
                }
                // else: keep reference-pose local_scale
            }
        }

        bones
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxBehaviorVariable {
    pub name: String,
    pub var_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HkxAnimationSummary {
    pub root_class: String,
    pub animation_type: Option<String>,
    pub num_tracks: usize,
    pub duration: f32,
    pub bone_names: Vec<String>,
    pub track_names: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Main Havok Packfile Representation
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HkxFile {
    pub header: HkxHeader,
    pub sections: Vec<HkxSectionHeader>,
    pub class_names: Vec<HkxClassName>,
    pub local_fixups: Vec<Vec<HkxLocalFixup>>,
    pub global_fixups: Vec<Vec<HkxGlobalFixup>>,
    pub virtual_fixups: Vec<Vec<HkxVirtualFixup>>,
    pub section_data: Vec<Vec<u8>>,
    pub root_class_name: String,
}

impl HkxFile {
    pub fn load_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 64 {
            return Err("File too short for Havok packfile header (minimum 64 bytes)".into());
        }

        let is_le = data[17] == 1;
        let endian = if is_le {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        let mut cursor = Cursor::new(data);
        let header = read_header(&mut cursor, endian)?;

        if header.magic0 != HKX_MAGIC_0 || header.magic1 != HKX_MAGIC_1 {
            return Err(format!(
                "Invalid Havok magic header: expected 0x{:X} 0x{:X}, got 0x{:X} 0x{:X}",
                HKX_MAGIC_0, HKX_MAGIC_1, header.magic0, header.magic1
            ));
        }

        let mut sections = Vec::with_capacity(header.num_sections.max(0) as usize);
        for _ in 0..header.num_sections {
            sections.push(read_section_header(&mut cursor, endian)?);
        }

        let mut section_data = Vec::with_capacity(sections.len());
        let mut local_fixups = Vec::with_capacity(sections.len());
        let mut global_fixups = Vec::with_capacity(sections.len());
        let mut virtual_fixups = Vec::with_capacity(sections.len());

        for sec in &sections {
            let start = sec.absolute_data_start as usize;
            let end = (sec.absolute_data_start + sec.end_offset) as usize;
            if start > data.len() || end > data.len() {
                section_data.push(Vec::new());
                local_fixups.push(Vec::new());
                global_fixups.push(Vec::new());
                virtual_fixups.push(Vec::new());
                continue;
            }

            let sec_bytes = data[start..end].to_vec();

            // Local fixups: local_fixups_offset..global_fixups_offset
            let local_end = sec.local_fixups_offset.max(0) as usize;
            let mut l_fix = Vec::new();
            if local_end <= sec_bytes.len() {
                let glob_end = sec.global_fixups_offset.max(local_end as i32) as usize;
                if glob_end <= sec_bytes.len() {
                    let mut fix_cursor = Cursor::new(&sec_bytes[local_end..glob_end]);
                    while (fix_cursor.position() as usize) + 8 <= (glob_end - local_end) {
                        let src = read_u32(&mut fix_cursor, endian)?;
                        let dst = read_u32(&mut fix_cursor, endian)?;
                        if src == 0xFFFFFFFF && dst == 0xFFFFFFFF {
                            break;
                        }
                        l_fix.push(HkxLocalFixup {
                            src_offset: src,
                            dst_offset: dst,
                        });
                    }
                }
            }

            // Global fixups: global_fixups_offset..virtual_fixups_offset
            let glob_start = sec.global_fixups_offset.max(0) as usize;
            let virt_start = sec.virtual_fixups_offset.max(glob_start as i32) as usize;
            let mut g_fix = Vec::new();
            if glob_start <= sec_bytes.len() && virt_start <= sec_bytes.len() {
                let mut fix_cursor = Cursor::new(&sec_bytes[glob_start..virt_start]);
                while (fix_cursor.position() as usize) + 12 <= (virt_start - glob_start) {
                    let src = read_u32(&mut fix_cursor, endian)?;
                    let dst_sec = read_u32(&mut fix_cursor, endian)?;
                    let dst_off = read_u32(&mut fix_cursor, endian)?;
                    if src == 0xFFFFFFFF {
                        break;
                    }
                    g_fix.push(HkxGlobalFixup {
                        src_offset: src,
                        dst_section: dst_sec,
                        dst_offset: dst_off,
                    });
                }
            }

            // Virtual fixups (Finish table): 12-byte entries in Havok 2012 packfiles.
            // Entry format: { src_object_offset(4), section_index(4), class_name_offset(4) }
            // Terminator sentinel: class_name_offset = 0xFFFFFFFF or src_object_offset = 0xFFFFFFFF.
            let exp_start = sec.exports_offset.max(virt_start as i32) as usize;
            let mut v_fix = Vec::new();
            if virt_start <= sec_bytes.len() && exp_start <= sec_bytes.len() {
                let mut fix_cursor = Cursor::new(&sec_bytes[virt_start..exp_start]);
                while (fix_cursor.position() as usize) + 12 <= (exp_start - virt_start) {
                    let src = read_u32(&mut fix_cursor, endian)?;
                    let sec_idx = read_u32(&mut fix_cursor, endian)?;
                    let cname_off = read_u32(&mut fix_cursor, endian)?;
                    if cname_off == 0xFFFFFFFF || src == 0xFFFFFFFF {
                        break;
                    }
                    v_fix.push(HkxVirtualFixup {
                        src_offset: src,
                        section_index: sec_idx,
                        class_name_offset: cname_off,
                    });
                }
            }

            section_data.push(sec_bytes);
            local_fixups.push(l_fix);
            global_fixups.push(g_fix);
            virtual_fixups.push(v_fix);
        }

        // Parse class names from __classnames__ section.
        // Each entry format: [hash_u32 (4 bytes)][0x09 tab (1 byte)][name_cstr][0x00]
        let mut class_names = Vec::new();
        let mut root_class_name = String::new();

        for (idx, sec) in sections.iter().enumerate() {
            if sec.section_tag == "__classnames__" && idx < section_data.len() {
                let sbytes = &section_data[idx];
                let data_size = sec.local_fixups_offset.max(0) as usize;
                let limit = data_size.min(sbytes.len());

                // Scan for entries: each entry = 4-byte hash + 0x09 tab + name_cstr + 0x00
                let mut offset = 0;
                while offset + 5 < limit {
                    // Skip bytes until we find the 0x09 tab that precedes a printable ASCII name.
                    // The pattern is: hash(4) tab(0x09) name... null
                    // We detect by checking: byte at (offset+4) == 0x09 and byte at (offset+5) is printable.
                    if sbytes[offset + 4] == 0x09 {
                        let name_start = offset + 5;
                        if name_start < limit {
                            if let Some(pos) =
                                sbytes[name_start..limit].iter().position(|&b| b == 0)
                            {
                                let name_end = name_start + pos;
                                let raw = &sbytes[name_start..name_end];
                                // Only accept fully printable ASCII names
                                if !raw.is_empty() && raw.iter().all(|&b| b >= 32 && b <= 126) {
                                    let name = String::from_utf8_lossy(raw).to_string();
                                    class_names.push(HkxClassName {
                                        // Store the offset of the name string itself (what virtual fixups reference)
                                        offset: name_start as u32,
                                        name,
                                    });
                                }
                                offset = name_end + 1;
                                continue;
                            }
                        }
                    }
                    // Not a valid entry start, advance by 1
                    offset += 1;
                }
            }
        }

        // Extract root class name using the header's classname section offset.
        // The offset points directly to the name string (after the hash+tab header).
        let cn_sec = header.contents_class_name_section_index as usize;
        let cn_off = header.contents_class_name_section_offset as usize;
        if cn_sec < section_data.len() && cn_off < section_data[cn_sec].len() {
            let sbytes = &section_data[cn_sec];
            if let Some(pos) = sbytes[cn_off..].iter().position(|&b| b == 0) {
                let raw = &sbytes[cn_off..cn_off + pos];
                if raw.iter().all(|&b| b >= 32 && b <= 126) {
                    root_class_name = String::from_utf8_lossy(raw).to_string();
                }
            }
        }
        if root_class_name.is_empty() {
            root_class_name = class_names
                .last()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "hkRootLevelContainer".to_string());
        }

        Ok(HkxFile {
            header,
            sections,
            class_names,
            local_fixups,
            global_fixups,
            virtual_fixups,
            section_data,
            root_class_name,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let is_le = self.header.endianness == 1;
        let endian = if is_le {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        let mut out = Vec::new();
        write_header(&mut out, &self.header, endian);

        for sec in &self.sections {
            write_section_header(&mut out, sec, endian);
        }

        // Align to 16 bytes
        while out.len() % 16 != 0 {
            out.push(0);
        }

        for (idx, sec_bytes) in self.section_data.iter().enumerate() {
            if idx < self.sections.len() {
                let sec_hdr = &self.sections[idx];
                let current_pos = out.len();
                if (sec_hdr.absolute_data_start as usize) > current_pos {
                    out.resize(sec_hdr.absolute_data_start as usize, 0);
                }
                out.extend_from_slice(sec_bytes);
            }
        }

        out
    }

    /// Parse 3D Skeleton Rig (`hkaSkeleton`), tracing exact Havok pointer fixup graphs.
    /// Requires the virtual fixup table to correctly identify the hkaSkeleton object.
    pub fn parse_skeleton(&self) -> Option<HkxSkeleton> {
        // Primary: Use official C++ Havok engine
        if let Some(native_pack) =
            crate::ffi_anim::NativeHavokPackFile::load_memory(&self.to_bytes())
        {
            if native_pack.num_skeletons() > 0 {
                if let Some(skel) = native_pack.get_skeleton(0) {
                    return Some(skel);
                }
            }
        }

        let is_le = self.header.endianness == 1;
        let endian = if is_le {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        let data_sec_idx = self
            .sections
            .iter()
            .position(|s| s.section_tag == "__data__")?;
        let sbytes = self.section_data.get(data_sec_idx)?;
        let l_fixups = self.local_fixups.get(data_sec_idx)?;
        let v_fixups = self.virtual_fixups.get(data_sec_idx)?;

        // Build local fixup map: src_offset -> dst_offset (both relative to __data__ start)
        let mut local_map = std::collections::HashMap::<u32, u32>::new();
        for f in l_fixups {
            local_map.insert(f.src_offset, f.dst_offset);
        }

        // Classnames section — cn_off in virtual fixups points directly to the name string.
        let cn_sec_idx = self
            .sections
            .iter()
            .position(|s| s.section_tag == "__classnames__")
            .unwrap_or(0);
        let cn_bytes = self.section_data.get(cn_sec_idx)?;

        let get_cname = |cname_off: u32| -> String {
            let off = cname_off as usize;
            if off < cn_bytes.len() {
                if let Some(pos) = cn_bytes[off..].iter().position(|&b| b == 0) {
                    let raw = &cn_bytes[off..off + pos];
                    if raw.iter().all(|&b| b >= 32 && b <= 126) {
                        return String::from_utf8_lossy(raw).to_string();
                    }
                }
            }
            String::new()
        };

        // Find hkaSkeleton object via virtual fixups — exact match required.
        let mut skel_offset: Option<usize> = None;
        for vf in v_fixups {
            let cname = get_cname(vf.class_name_offset);
            if cname == "hkaSkeleton" {
                skel_offset = Some(vf.src_offset as usize);
                break;
            }
        }

        // Fallback: trace hkaAnimationContainer -> m_skeletons array -> first skeleton ptr.
        // hkaAnimationContainer (32-bit) layout:
        //   +0: vtable (4)
        //   +4: m_memSizeAndRefCount (4)
        //   +8: m_skeletons.ptr (4) <- local fixup to array of skeleton ptrs
        //   +12: m_skeletons.size (4)
        //   +16: m_skeletons.cap (4)
        if skel_offset.is_none() {
            for vf in v_fixups {
                let cname = get_cname(vf.class_name_offset);
                if cname == "hkaAnimationContainer" {
                    let ac_off = vf.src_offset as u32;
                    // m_skeletons.ptr is at hkaAnimationContainer + 8
                    if let Some(&skel_arr_dst) = local_map.get(&(ac_off + 8)) {
                        let size_off = ac_off as usize + 12;
                        if size_off + 4 <= sbytes.len() {
                            let arr_size =
                                read_u32_slice(&sbytes[size_off..size_off + 4], endian) as usize;
                            if arr_size > 0 {
                                // The first element of the skeleton pointer array
                                let elem_src = skel_arr_dst;
                                if let Some(&skel_ptr) = local_map.get(&elem_src) {
                                    skel_offset = Some(skel_ptr as usize);
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }

        let skel_off = skel_offset?;
        if skel_off + 48 > sbytes.len() {
            return None;
        }

        // hkaSkeleton layout (32-bit, extends hkReferencedObject):
        // +0:  vtable* (4)
        // +4:  m_memSizeAndRefCount (4)
        // +8:  m_name hkStringPtr (4)            -> local fixup -> C-string
        // +12: m_parentIndices.ptr (4)           -> local fixup -> [i16; N]
        // +16: m_parentIndices.size (i32)
        // +20: m_parentIndices.cap (i32)
        // +24: m_bones.ptr (4)                   -> local fixup -> [hkaBone; N]
        // +28: m_bones.size (i32)
        // +32: m_bones.cap (i32)
        // +36: m_referencePose.ptr (4)           -> local fixup -> [hkQsTransform; N]
        // +40: m_referencePose.size (i32)
        // +44: m_referencePose.cap (i32)

        let name_dst = local_map.get(&((skel_off + 8) as u32)).copied();
        let parents_dst = local_map.get(&((skel_off + 12) as u32)).copied()?;
        let parents_size = read_u32_slice(&sbytes[skel_off + 16..skel_off + 20], endian) as usize;
        let bones_dst = local_map.get(&((skel_off + 24) as u32)).copied()?;
        let bones_size = read_u32_slice(&sbytes[skel_off + 28..skel_off + 32], endian) as usize;
        let ref_dst = local_map.get(&((skel_off + 36) as u32)).copied()?;
        let ref_size = read_u32_slice(&sbytes[skel_off + 40..skel_off + 44], endian) as usize;

        let skel_name = name_dst
            .and_then(|ndst| read_cstring(sbytes, ndst as usize))
            .unwrap_or_else(|| "hkaSkeleton".to_string());

        // Sanity check — refuse implausibly large sizes
        if parents_size > 4096 || bones_size > 4096 || ref_size > 4096 {
            return None;
        }

        // Read parent indices — [i16; N] at parents_dst
        let p_start = parents_dst as usize;
        let mut parent_indices = Vec::with_capacity(parents_size);
        for i in 0..parents_size {
            let off = p_start + i * 2;
            if off + 2 <= sbytes.len() {
                parent_indices.push(read_i16_slice(&sbytes[off..off + 2], endian));
            }
        }

        // Read bone names — hkaBone = 8 bytes: +0 hkStringPtr(4), +4 bool+pad(4)
        let b_start = bones_dst as usize;
        let mut bone_names = Vec::with_capacity(bones_size);
        for i in 0..bones_size {
            let b_off = (b_start + i * 8) as u32;
            let bname = local_map
                .get(&b_off)
                .and_then(|&dst| read_cstring(sbytes, dst as usize))
                .unwrap_or_else(|| format!("Bone_{i:02}"));
            bone_names.push(bname);
        }

        // Read reference pose — hkQsTransform = 48 bytes: 3 × hkVector4
        //   +0:  translation [f32; 4]  (x, y, z, w)
        //   +16: rotation    [f32; 4]  (x, y, z, w) quaternion
        //   +32: scale       [f32; 4]  (x, y, z, w)
        let r_start = ref_dst as usize;
        let mut ref_poses: Vec<([f32; 3], [f32; 4], [f32; 3])> = Vec::with_capacity(ref_size);
        for i in 0..ref_size {
            let t_off = r_start + i * 48;
            if t_off + 48 > sbytes.len() {
                break;
            }
            let tx = read_f32_slice(&sbytes[t_off..], endian);
            let ty = read_f32_slice(&sbytes[t_off + 4..], endian);
            let tz = read_f32_slice(&sbytes[t_off + 8..], endian);
            // t_off+12 is w (ignored)
            let rx = read_f32_slice(&sbytes[t_off + 16..], endian);
            let ry = read_f32_slice(&sbytes[t_off + 20..], endian);
            let rz = read_f32_slice(&sbytes[t_off + 24..], endian);
            let rw = read_f32_slice(&sbytes[t_off + 28..], endian);
            let sx = read_f32_slice(&sbytes[t_off + 32..], endian);
            let sy = read_f32_slice(&sbytes[t_off + 36..], endian);
            let sz = read_f32_slice(&sbytes[t_off + 40..], endian);
            ref_poses.push(([tx, ty, tz], [rx, ry, rz, rw], [sx, sy, sz]));
        }

        let count = parents_size.min(bones_size).min(ref_poses.len());
        if count == 0 {
            return None;
        }

        let mut bones = Vec::with_capacity(count);
        for i in 0..count {
            let bname = bone_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("Bone_{i:02}"));
            let p_idx = parent_indices.get(i).copied().unwrap_or(-1);
            let (trans, rot, scale) = ref_poses.get(i).copied().unwrap_or((
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
            ));
            bones.push(HkxBone {
                name: bname,
                parent_index: p_idx,
                local_translation: trans,
                local_rotation: rot,
                local_scale: scale,
                lock_translation: false,
            });
        }

        Some(HkxSkeleton {
            name: skel_name,
            bones,
        })
    }

    /// Parse Skeletal Animations (`hkaAnimationBinding`, `hkaSplineCompressedAnimation`, `hkaInterleavedUncompressedAnimation`).
    pub fn parse_animations(&self) -> Vec<HkxAnimation> {
        // Primary: Use official C++ Havok engine
        if let Some(native_pack) =
            crate::ffi_anim::NativeHavokPackFile::load_memory(&self.to_bytes())
        {
            if native_pack.num_animations() > 0 {
                let mut anims = Vec::new();
                for i in 0..native_pack.num_animations() {
                    if let Some(anim) = native_pack.get_animation(i) {
                        anims.push(anim);
                    }
                }
                if !anims.is_empty() {
                    return anims;
                }
            }
        }

        Vec::new()
    }

    /// Decompile this binary `.hkx` packfile into Havok XML text using official C++ engine.
    pub fn decompile_to_xml(&self) -> Option<String> {
        crate::ffi_anim::NativeHavokPackFile::decompile_to_xml(&self.to_bytes())
    }

    pub fn to_xml(&self) -> String {
        let objects = self.get_objects();
        if let Some(xml) = self.decompile_to_xml() {
            let hkobject_count = xml.matches("<hkobject").count();
            if hkobject_count >= objects.len() || objects.is_empty() {
                return xml;
            }
        }

        let ver = if self.header.contents_version.is_empty() {
            "hk_2012.2.0-r1"
        } else {
            &self.header.contents_version
        };

        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"ascii\"?>\n");
        out.push_str(&format!(
            "<hkpackfile classversion=\"9\" contentsversion=\"{ver}\">\n"
        ));
        out.push_str("  <hksection name=\"__data__\">\n");

        for obj in &objects {
            let addr_str = format!("0x{:016X}", obj.address);
            out.push_str(&format!(
                "    <hkobject name=\"{addr_str}\" class=\"{}\">\n",
                obj.class_name
            ));

            match obj.class_name.as_str() {
                "hkRootLevelContainer" => {
                    out.push_str("      <hkparam name=\"namedVariants\" numelements=\"0\" />\n");
                }
                "hkpPhysicsData" => {
                    out.push_str("      <hkparam name=\"worldCinfo\">null</hkparam>\n");
                    out.push_str("      <hkparam name=\"systems\" numelements=\"0\" />\n");
                }
                "hkpPhysicsSystem" => {
                    out.push_str("      <hkparam name=\"rigidBodies\" numelements=\"0\" />\n");
                    out.push_str("      <hkparam name=\"constraints\" numelements=\"0\" />\n");
                }
                "hkpRigidBody" => {
                    out.push_str("      <hkparam name=\"name\">RigidBody</hkparam>\n");
                    out.push_str("      <hkparam name=\"shape\">null</hkparam>\n");
                }
                "hkpStorageExtendedMeshShape" => {
                    out.push_str("      <hkparam name=\"meshstorage\" numelements=\"1\" />\n");
                }
                "hkpStorageExtendedMeshShapeMeshSubpartStorage" => {
                    out.push_str("      <hkparam name=\"vertices\" numelements=\"0\" />\n");
                    out.push_str("      <hkparam name=\"indices16\" numelements=\"0\" />\n");
                    out.push_str("      <hkparam name=\"indices32\" numelements=\"0\" />\n");
                }
                _ => {}
            }

            out.push_str("    </hkobject>\n");
        }

        out.push_str("  </hksection>\n");
        out.push_str("</hkpackfile>\n");
        out
    }

    /// Compile Havok XML text into an official binary `.hkx` packfile.
    pub fn compile_from_xml(xml_str: &str) -> Option<Vec<u8>> {
        crate::ffi_anim::NativeHavokPackFile::compile_from_xml(xml_str)
    }

    pub fn from_xml(xml_str: &str) -> Result<Self, String> {
        if let Some(bytes) = Self::compile_from_xml(xml_str) {
            Self::load_bytes(&bytes).map_err(|e| e.to_string())
        } else {
            Err(
                "Failed to compile XML to binary HKX packfile using native Havok C++ engine."
                    .to_string(),
            )
        }
    }

    /// Parse 3D physics and collision shapes (`hkpPhysicsData`, `hkpRigidBody`, `hkpShape`).
    pub fn parse_collisions(&self) -> Vec<HkxCollisionMesh> {
        let native_meshes = self.parse_collisions_native();
        if !native_meshes.is_empty() {
            return native_meshes;
        }

        if let Some(native_pack) =
            crate::ffi_anim::NativeHavokPackFile::load_memory(&self.to_bytes())
        {
            native_pack.get_collisions()
        } else {
            Vec::new()
        }
    }

    /// Parse collision mesh geometry directly from binary packfile section data and virtual fixups.
    pub fn parse_collisions_native(&self) -> Vec<HkxCollisionMesh> {
        let mut meshes = Vec::new();
        let is_le = self.header.endianness == 1;
        let endian = if is_le {
            EndianType::LITTLE
        } else {
            EndianType::BIG
        };

        let data_sec_idx = match self
            .sections
            .iter()
            .position(|s| s.section_tag == "__data__")
        {
            Some(idx) => idx,
            None => return meshes,
        };
        let sbytes = match self.section_data.get(data_sec_idx) {
            Some(b) => b,
            None => return meshes,
        };
        let l_fixups = match self.local_fixups.get(data_sec_idx) {
            Some(f) => f,
            None => return meshes,
        };
        let v_fixups = match self.virtual_fixups.get(data_sec_idx) {
            Some(f) => f,
            None => return meshes,
        };

        // Build local fixup map: src_offset -> dst_offset
        let mut local_map = std::collections::HashMap::<u32, u32>::new();
        for f in l_fixups {
            local_map.insert(f.src_offset, f.dst_offset);
        }

        // Classnames section offset resolver
        let cn_sec_idx = self
            .sections
            .iter()
            .position(|s| s.section_tag == "__classnames__")
            .unwrap_or(0);
        let cn_bytes = match self.section_data.get(cn_sec_idx) {
            Some(b) => b,
            None => return meshes,
        };

        let get_cname = |cname_off: u32| -> String {
            let off = cname_off as usize;
            if off < cn_bytes.len() {
                if let Some(pos) = cn_bytes[off..].iter().position(|&b| b == 0) {
                    let raw = &cn_bytes[off..off + pos];
                    if raw.iter().all(|&b| b >= 32 && b <= 126) {
                        return String::from_utf8_lossy(raw).to_string();
                    }
                }
            }
            String::new()
        };

        let mut mesh_counter = 0;

        for vf in v_fixups {
            let cname = get_cname(vf.class_name_offset);
            let obj_off = vf.src_offset as usize;

            if cname == "hkpStorageExtendedMeshShapeMeshSubpartStorage" {
                if obj_off + 52 <= sbytes.len() {
                    let v_dst = local_map.get(&((obj_off + 8) as u32)).copied();
                    let v_size =
                        read_u32_slice(&sbytes[obj_off + 12..obj_off + 16], endian) as usize;

                    let mut vertices = Vec::new();
                    if let Some(v_start) = v_dst {
                        let vs = v_start as usize;
                        for i in 0..v_size {
                            let v_off = vs + i * 16;
                            if v_off + 12 <= sbytes.len() {
                                let x = read_f32_slice(&sbytes[v_off..], endian);
                                let y = read_f32_slice(&sbytes[v_off + 4..], endian);
                                let z = read_f32_slice(&sbytes[v_off + 8..], endian);
                                vertices.push([x, y, z]);
                            }
                        }
                    }

                    let mut indices = Vec::new();
                    let idx16_dst = local_map.get(&((obj_off + 32) as u32)).copied();
                    let idx16_size =
                        read_u32_slice(&sbytes[obj_off + 36..obj_off + 40], endian) as usize;
                    if let Some(i_start) = idx16_dst {
                        let is = i_start as usize;
                        for i in 0..idx16_size {
                            let i_off = is + i * 2;
                            if i_off + 2 <= sbytes.len() {
                                indices
                                    .push(read_u16_slice(&sbytes[i_off..i_off + 2], endian) as u32);
                            }
                        }
                    }

                    if indices.is_empty() {
                        let idx32_dst = local_map.get(&((obj_off + 44) as u32)).copied();
                        let idx32_size =
                            read_u32_slice(&sbytes[obj_off + 48..obj_off + 52], endian) as usize;
                        if let Some(i_start) = idx32_dst {
                            let is = i_start as usize;
                            for i in 0..idx32_size {
                                let i_off = is + i * 4;
                                if i_off + 4 <= sbytes.len() {
                                    indices.push(read_u32_slice(&sbytes[i_off..i_off + 4], endian));
                                }
                            }
                        }
                    }

                    if !vertices.is_empty() {
                        mesh_counter += 1;
                        meshes.push(HkxCollisionMesh {
                            name: format!("CollisionMesh_{mesh_counter}"),
                            vertices,
                            indices,
                        });
                    }
                }
            } else if cname == "hkpSimpleMeshShape" {
                if obj_off + 28 <= sbytes.len() {
                    let v_dst = local_map.get(&((obj_off + 8) as u32)).copied();
                    let v_size =
                        read_u32_slice(&sbytes[obj_off + 12..obj_off + 16], endian) as usize;

                    let mut vertices = Vec::new();
                    if let Some(v_start) = v_dst {
                        let vs = v_start as usize;
                        for i in 0..v_size {
                            let v_off = vs + i * 16;
                            if v_off + 12 <= sbytes.len() {
                                let x = read_f32_slice(&sbytes[v_off..], endian);
                                let y = read_f32_slice(&sbytes[v_off + 4..], endian);
                                let z = read_f32_slice(&sbytes[v_off + 8..], endian);
                                vertices.push([x, y, z]);
                            }
                        }
                    }

                    let mut indices = Vec::new();
                    let tri_dst = local_map.get(&((obj_off + 20) as u32)).copied();
                    let tri_size =
                        read_u32_slice(&sbytes[obj_off + 24..obj_off + 28], endian) as usize;
                    if let Some(t_start) = tri_dst {
                        let ts = t_start as usize;
                        for i in 0..tri_size {
                            let t_off = ts + i * 16;
                            if t_off + 12 <= sbytes.len() {
                                indices.push(read_u32_slice(&sbytes[t_off..], endian));
                                indices.push(read_u32_slice(&sbytes[t_off + 4..], endian));
                                indices.push(read_u32_slice(&sbytes[t_off + 8..], endian));
                            }
                        }
                    }

                    if !vertices.is_empty() {
                        mesh_counter += 1;
                        meshes.push(HkxCollisionMesh {
                            name: format!("CollisionMesh_{mesh_counter}"),
                            vertices,
                            indices,
                        });
                    }
                }
            } else if cname == "hkpConvexVerticesShape" {
                if obj_off + 72 <= sbytes.len() {
                    let v_dst = local_map.get(&((obj_off + 0x40) as u32)).copied();
                    let v_size =
                        read_u32_slice(&sbytes[obj_off + 0x44..obj_off + 0x48], endian) as usize;

                    let mut vertices = Vec::new();
                    if let Some(v_start) = v_dst {
                        let vs = v_start as usize;
                        for i in 0..v_size {
                            let v_off = vs + i * 16;
                            if v_off + 12 <= sbytes.len() {
                                let x = read_f32_slice(&sbytes[v_off..], endian);
                                let y = read_f32_slice(&sbytes[v_off + 4..], endian);
                                let z = read_f32_slice(&sbytes[v_off + 8..], endian);
                                vertices.push([x, y, z]);
                            }
                        }
                    }

                    let mut indices = Vec::new();
                    if vertices.len() >= 3 {
                        for i in 1..(vertices.len() - 1) {
                            indices.push(0);
                            indices.push(i as u32);
                            indices.push((i + 1) as u32);
                        }
                    }

                    if !vertices.is_empty() {
                        mesh_counter += 1;
                        meshes.push(HkxCollisionMesh {
                            name: format!("ConvexShape_{mesh_counter}"),
                            vertices,
                            indices,
                        });
                    }
                }
            }
        }

        meshes
    }

    /// List all registered Havok classes and instances in this packfile.
    pub fn get_objects(&self) -> Vec<HkxObjectInfo> {
        let mut results = Vec::new();

        let data_sec_idx = match self
            .sections
            .iter()
            .position(|s| s.section_tag == "__data__")
        {
            Some(idx) => idx,
            None => return results,
        };
        let v_fixups = match self.virtual_fixups.get(data_sec_idx) {
            Some(f) => f,
            None => return results,
        };
        let cn_sec_idx = self
            .sections
            .iter()
            .position(|s| s.section_tag == "__classnames__")
            .unwrap_or(0);
        let cn_bytes = match self.section_data.get(cn_sec_idx) {
            Some(b) => b,
            None => return results,
        };

        for vf in v_fixups {
            let off = vf.class_name_offset as usize;
            if off < cn_bytes.len() {
                if let Some(pos) = cn_bytes[off..].iter().position(|&b| b == 0) {
                    let raw = &cn_bytes[off..off + pos];
                    if !raw.is_empty() && raw.iter().all(|&b| b >= 32 && b <= 126) {
                        let cname = String::from_utf8_lossy(raw).to_string();
                        results.push(HkxObjectInfo {
                            class_name: cname,
                            address: vf.src_offset as u64,
                        });
                    }
                }
            }
        }

        if !results.is_empty() {
            return results;
        }

        if let Some(native_pack) =
            crate::ffi_anim::NativeHavokPackFile::load_memory(&self.to_bytes())
        {
            native_pack.get_objects()
        } else {
            Vec::new()
        }
    }

    /// Parse Behavior Variables (`hkbBehaviorGraphData`)
    pub fn parse_behavior_variables(&self) -> Vec<HkxBehaviorVariable> {
        let mut vars = Vec::new();
        for sdata in &self.section_data {
            let mut offset = 0;
            while offset < sdata.len() {
                if let Some(pos) = sdata[offset..].iter().position(|&b| b == 0) {
                    let end = offset + pos;
                    if pos >= 2 && pos <= 64 {
                        let raw = &sdata[offset..end];
                        if raw.iter().all(|&b| b >= 32 && b <= 126) {
                            let str_val = String::from_utf8_lossy(raw).to_string();
                            if str_val.contains("b_")
                                || str_val.contains("f_")
                                || str_val.contains("i_")
                                || str_val.contains("v_")
                                || str_val.to_lowercase().contains("var")
                                || str_val.to_lowercase().contains("state")
                                || str_val.to_lowercase().contains("event")
                                || str_val.to_lowercase().contains("trigger")
                            {
                                let var_type = if str_val.starts_with("b_") {
                                    "Bool"
                                } else if str_val.starts_with("f_") {
                                    "Float"
                                } else if str_val.starts_with("i_") {
                                    "Int"
                                } else {
                                    "Event / Variable"
                                };

                                vars.push(HkxBehaviorVariable {
                                    name: str_val,
                                    var_type: var_type.to_string(),
                                    value: "0.0".to_string(),
                                });
                            }
                        }
                    }
                    offset = end + 1;
                } else {
                    break;
                }
            }
        }
        vars
    }

    pub fn animation_summary(&self) -> HkxAnimationSummary {
        let mut summary = HkxAnimationSummary {
            root_class: self.root_class_name.clone(),
            ..Default::default()
        };

        if let Some(skel) = self.parse_skeleton() {
            summary.bone_names = skel.bones.into_iter().map(|b| b.name).collect();
        }

        for sdata in &self.section_data {
            let mut offset = 0;
            while offset < sdata.len() {
                if let Some(pos) = sdata[offset..].iter().position(|&b| b == 0) {
                    let end = offset + pos;
                    if pos >= 3 && pos <= 64 {
                        let raw = &sdata[offset..end];
                        if raw.iter().all(|&b| b >= 32 && b <= 126) {
                            let str_val = String::from_utf8_lossy(raw).to_string();
                            if str_val.contains("Animation")
                                || str_val.contains("Binding")
                                || str_val.contains("Container")
                            {
                                if summary.animation_type.is_none() {
                                    summary.animation_type = Some(str_val);
                                }
                            } else if str_val.contains('_') || str_val.contains('/') {
                                if !summary.track_names.contains(&str_val)
                                    && summary.track_names.len() < 128
                                {
                                    summary.track_names.push(str_val);
                                }
                            }
                        }
                    }
                    offset = end + 1;
                } else {
                    break;
                }
            }
        }

        summary.num_tracks = summary.track_names.len();
        summary
    }

    /// Update a bone's name in internal section string tables
    pub fn update_bone_name(&mut self, _bone_idx: usize, old_name: &str, new_name: &str) -> bool {
        let mut updated = false;
        let old_bytes = old_name.as_bytes();
        let new_bytes = new_name.as_bytes();

        for sdata in &mut self.section_data {
            if let Some(pos) = sdata.windows(old_bytes.len()).position(|w| w == old_bytes) {
                if old_bytes.len() == new_bytes.len() {
                    sdata[pos..pos + new_bytes.len()].copy_from_slice(new_bytes);
                    updated = true;
                }
            }
        }
        updated
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Quaternion & Math Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn read_u32_slice(slice: &[u8], endian: EndianType) -> u32 {
    let b = [slice[0], slice[1], slice[2], slice[3]];
    match endian {
        EndianType::LITTLE => u32::from_le_bytes(b),
        EndianType::BIG => u32::from_be_bytes(b),
    }
}

fn read_i16_slice(slice: &[u8], endian: EndianType) -> i16 {
    let b = [slice[0], slice[1]];
    match endian {
        EndianType::LITTLE => i16::from_le_bytes(b),
        EndianType::BIG => i16::from_be_bytes(b),
    }
}

fn read_u16_slice(slice: &[u8], endian: EndianType) -> u16 {
    let b = [slice[0], slice[1]];
    match endian {
        EndianType::LITTLE => u16::from_le_bytes(b),
        EndianType::BIG => u16::from_be_bytes(b),
    }
}

fn read_cstring(data: &[u8], offset: usize) -> Option<String> {
    if offset >= data.len() {
        return None;
    }
    let pos = data[offset..].iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&data[offset..offset + pos]).to_string())
}

fn read_f32_slice(slice: &[u8], endian: EndianType) -> f32 {
    let b = [slice[0], slice[1], slice[2], slice[3]];
    match endian {
        EndianType::LITTLE => f32::from_le_bytes(b),
        EndianType::BIG => f32::from_be_bytes(b),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Binary IO Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn read_header<R: Read>(reader: &mut R, endian: EndianType) -> Result<HkxHeader, String> {
    let magic0 = read_u32(reader, endian)?;
    let magic1 = read_u32(reader, endian)?;
    let user_tag = read_i32(reader, endian)?;
    let file_version = read_i32(reader, endian)?;
    let pointer_size = read_u8(reader)?;
    let endianness = read_u8(reader)?;
    let padding_option = read_u8(reader)?;
    let base_format = read_u8(reader)?;
    let num_sections = read_i32(reader, endian)?;
    let contents_section_index = read_i32(reader, endian)?;
    let contents_section_offset = read_i32(reader, endian)?;
    let contents_class_name_section_index = read_i32(reader, endian)?;
    let contents_class_name_section_offset = read_i32(reader, endian)?;

    let mut ver_buf = [0u8; 16];
    reader.read_exact(&mut ver_buf).map_err(|e| e.to_string())?;
    let contents_version = String::from_utf8_lossy(&ver_buf)
        .trim_matches(|c: char| c == '\0' || c == '\u{fffd}' || c.is_whitespace())
        .to_string();

    let flags = read_i32(reader, endian)?;
    let pad = read_i32(reader, endian)?;

    Ok(HkxHeader {
        magic0,
        magic1,
        user_tag,
        file_version,
        pointer_size,
        endianness,
        padding_option,
        base_format,
        num_sections,
        contents_section_index,
        contents_section_offset,
        contents_class_name_section_index,
        contents_class_name_section_offset,
        contents_version,
        flags,
        pad,
    })
}

fn write_header<W: Write>(writer: &mut W, header: &HkxHeader, endian: EndianType) {
    let _ = write_u32(writer, header.magic0, endian);
    let _ = write_u32(writer, header.magic1, endian);
    let _ = write_i32(writer, header.user_tag, endian);
    let _ = write_i32(writer, header.file_version, endian);
    let _ = write_u8(writer, header.pointer_size);
    let _ = write_u8(writer, header.endianness);
    let _ = write_u8(writer, header.padding_option);
    let _ = write_u8(writer, header.base_format);
    let _ = write_i32(writer, header.num_sections, endian);
    let _ = write_i32(writer, header.contents_section_index, endian);
    let _ = write_i32(writer, header.contents_section_offset, endian);
    let _ = write_i32(writer, header.contents_class_name_section_index, endian);
    let _ = write_i32(writer, header.contents_class_name_section_offset, endian);

    let mut ver_buf = [0u8; 16];
    let bytes = header.contents_version.as_bytes();
    let len = bytes.len().min(16);
    ver_buf[..len].copy_from_slice(&bytes[..len]);
    let _ = writer.write_all(&ver_buf);

    let _ = write_i32(writer, header.flags, endian);
    let _ = write_i32(writer, header.pad, endian);
}

fn read_section_header<R: Read>(
    reader: &mut R,
    endian: EndianType,
) -> Result<HkxSectionHeader, String> {
    let mut tag_buf = [0u8; 19];
    reader.read_exact(&mut tag_buf).map_err(|e| e.to_string())?;
    let section_tag = String::from_utf8_lossy(&tag_buf)
        .trim_matches('\0')
        .to_string();

    let _null_byte = read_u8(reader)?;

    let absolute_data_start = read_i32(reader, endian)?;
    let local_fixups_offset = read_i32(reader, endian)?;
    let global_fixups_offset = read_i32(reader, endian)?;
    let virtual_fixups_offset = read_i32(reader, endian)?;
    let exports_offset = read_i32(reader, endian)?;
    let imports_offset = read_i32(reader, endian)?;
    let end_offset = read_i32(reader, endian)?;

    Ok(HkxSectionHeader {
        section_tag,
        absolute_data_start,
        local_fixups_offset,
        global_fixups_offset,
        virtual_fixups_offset,
        exports_offset,
        imports_offset,
        end_offset,
    })
}

fn write_section_header<W: Write>(writer: &mut W, sec: &HkxSectionHeader, endian: EndianType) {
    let mut tag_buf = [0u8; 19];
    let bytes = sec.section_tag.as_bytes();
    let len = bytes.len().min(19);
    tag_buf[..len].copy_from_slice(&bytes[..len]);
    let _ = writer.write_all(&tag_buf);
    let _ = write_u8(writer, 0);

    let _ = write_i32(writer, sec.absolute_data_start, endian);
    let _ = write_i32(writer, sec.local_fixups_offset, endian);
    let _ = write_i32(writer, sec.global_fixups_offset, endian);
    let _ = write_i32(writer, sec.virtual_fixups_offset, endian);
    let _ = write_i32(writer, sec.exports_offset, endian);
    let _ = write_i32(writer, sec.imports_offset, endian);
    let _ = write_i32(writer, sec.end_offset, endian);
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, String> {
    let mut b = [0u8; 1];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b[0])
}

fn write_u8<W: Write>(writer: &mut W, val: u8) -> Result<(), String> {
    writer.write_all(&[val]).map_err(|e| e.to_string())
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

fn read_i32<R: Read>(reader: &mut R, endian: EndianType) -> Result<i32, String> {
    let mut b = [0u8; 4];
    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(match endian {
        EndianType::LITTLE => i32::from_le_bytes(b),
        EndianType::BIG => i32::from_be_bytes(b),
    })
}

fn write_i32<W: Write>(writer: &mut W, val: i32, endian: EndianType) -> Result<(), String> {
    let b = match endian {
        EndianType::LITTLE => val.to_le_bytes(),
        EndianType::BIG => val.to_be_bytes(),
    };
    writer.write_all(&b).map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// XML Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Escapes special XML characters in string values for output inside element text.
#[allow(dead_code)]
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mickey_rig_skin() {
        let path = r"D:\Modding\Assets\EM2PC\characters\mickey\characters\mickey_rig_skin.hkx";
        if let Ok(bytes) = std::fs::read(path) {
            let hkx = HkxFile::load_bytes(&bytes).expect("Failed to parse mickey_rig_skin.hkx");
            let skel = hkx
                .parse_skeleton()
                .expect("Failed to find hkaSkeleton in mickey_rig_skin.hkx");
            assert_eq!(skel.name, "Tracker");
            assert_eq!(skel.bones.len(), 95);
            assert_eq!(skel.bones[0].name, "Tracker");
            assert_eq!(skel.bones[44].name, "Head");

            let xml = hkx.to_xml();
            assert!(xml.contains("class=\"hkaSkeleton\""));
            assert!(xml.contains("class=\"hkaAnimationContainer\""));
            assert!(xml.contains("class=\"hkRootLevelContainer\""));
        }
    }

    #[test]
    fn test_parse_mickey_walk_animation() {
        let path = r"D:\Modding\Assets\EM2PC\characters\mickey\animations\mickey_walk.hkx";
        if let Ok(bytes) = std::fs::read(path) {
            let hkx = HkxFile::load_bytes(&bytes).expect("Failed to parse mickey_walk.hkx");
            let anims = hkx.parse_animations();
            assert!(!anims.is_empty(), "No animations found in mickey_walk.hkx");
            let anim = &anims[0];
            println!(
                "Animation: name='{}' duration={} frame_duration={} num_frames={} num_tracks={}",
                anim.name, anim.duration, anim.frame_duration, anim.num_frames, anim.num_tracks
            );
            assert_eq!(anim.num_tracks, 95);
            assert!(anim.duration > 0.0);
            assert!(anim.num_frames > 0);

            // Test sampling pose paired with mickey_rig_skin skeleton
            let skel_path =
                r"D:\Modding\Assets\EM2PC\characters\mickey\characters\mickey_rig_skin.hkx";
            if let Ok(skel_bytes) = std::fs::read(skel_path) {
                let skel_hkx = HkxFile::load_bytes(&skel_bytes).unwrap();
                let skel = skel_hkx.parse_skeleton().unwrap();

                // Verify frames decoded from real spline data
                assert!(
                    !anim.frames.is_empty(),
                    "No frames decoded from spline animation"
                );
                assert_eq!(
                    anim.frames[0].len(),
                    95,
                    "Wrong number of tracks in frame 0"
                );

                let sampled_bones = anim.sample_pose(0.1, &skel);
                let sampled_skel = HkxSkeleton {
                    name: skel.name.clone(),
                    bones: sampled_bones,
                };
                let wpos = sampled_skel.compute_world_positions();

                let head_pos = wpos[44]; // Head bone
                assert!(
                    head_pos[1] > 0.5,
                    "Head position unreasonably low: {:?}",
                    head_pos
                );
            }
        }
    }

    #[test]
    fn test_parse_wii_collision_mesh() {
        let path =
            r"D:\Modding\Assets\EM1\Environments\Demo\Demo_GV_Island01_Inert_static_mesh.hkx_wii";
        if let Ok(bytes) = std::fs::read(path) {
            let hkx = HkxFile::load_bytes(&bytes)
                .expect("Failed to load Demo_GV_Island01_Inert_static_mesh.hkx_wii");
            let collisions = hkx.parse_collisions();
            assert!(
                !collisions.is_empty(),
                "No collision meshes found in hkx_wii file"
            );
            let mesh = &collisions[0];
            assert!(
                mesh.vertices.len() > 1000,
                "Too few vertices in collision mesh: {}",
                mesh.vertices.len()
            );
            assert!(
                mesh.indices.len() > 1000,
                "Too few indices in collision mesh: {}",
                mesh.indices.len()
            );
            println!(
                "Wii Collision Mesh: vertices={} indices={}",
                mesh.vertices.len(),
                mesh.indices.len()
            );
        }
    }
}
