use crate::hkx_file::{
    HkxAnimation, HkxBone, HkxCollisionMesh, HkxObjectInfo, HkxSkeleton, HkxTransformSample,
};

enum HavokPackFileHandle {}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiBone {
    pub name: [u8; 128],
    pub parent_index: i16,
    pub local_translation: [f32; 3],
    pub local_rotation: [f32; 4],
    pub local_scale: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiTransformSample {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub has_pos: u8,
    pub has_rot: u8,
    pub has_scale: u8,
}

extern "C" {
    fn havok_load_packfile_memory(data: *const u8, len: usize) -> *mut HavokPackFileHandle;
    fn havok_free_packfile(handle: *mut HavokPackFileHandle);

    fn havok_get_num_skeletons(handle: *mut HavokPackFileHandle) -> usize;
    fn havok_get_skeleton_info(
        handle: *mut HavokPackFileHandle,
        skel_idx: usize,
        out_name: *mut u8,
        out_name_len: usize,
        out_num_bones: *mut usize,
    ) -> i32;
    fn havok_get_skeleton_bones(
        handle: *mut HavokPackFileHandle,
        skel_idx: usize,
        out_bones: *mut FfiBone,
        max_bones: usize,
    ) -> i32;

    fn havok_get_animation_binding(handle: *mut HavokPackFileHandle, index: usize, out_indices: *mut i16, capacity: usize, out_count: *mut usize) -> i32;
    fn havok_get_num_animations(handle: *mut HavokPackFileHandle) -> usize;
    fn havok_get_animation_info(
        handle: *mut HavokPackFileHandle,
        anim_idx: usize,
        out_name: *mut u8,
        out_name_len: usize,
        out_duration: *mut f32,
        out_frame_duration: *mut f32,
        out_num_frames: *mut usize,
        out_num_tracks: *mut usize,
    ) -> i32;
    fn havok_sample_animation_pose(
        handle: *mut HavokPackFileHandle,
        anim_idx: usize,
        time: f32,
        out_transforms: *mut FfiTransform,
        max_tracks: usize,
    ) -> i32;

    fn havok_decode_spline_block(
        block_data: *const u8,
        block_data_len: usize,
        block_offset: usize,
        num_tracks: usize,
        num_float_tracks: usize,
        frame_val: f32,
        out_samples: *mut FfiTransformSample,
    ) -> i32;

    fn havok_decompile_to_xml(
        data: *const u8,
        len: usize,
        out_xml: *mut u8,
        max_xml_len: usize,
        out_written: *mut usize,
    ) -> i32;

    fn havok_compile_from_xml(
        xml_str: *const u8,
        out_bytes: *mut u8,
        max_bytes_len: usize,
        out_written: *mut usize,
    ) -> i32;

    fn havok_get_num_objects(handle: *mut HavokPackFileHandle) -> usize;
    fn havok_get_object_info(
        handle: *mut HavokPackFileHandle,
        obj_idx: usize,
        out_class_name: *mut u8,
        out_class_name_len: usize,
        out_addr: *mut u64,
    ) -> i32;

    fn havok_get_num_collisions(handle: *mut HavokPackFileHandle) -> usize;
    fn havok_get_collision_info(
        handle: *mut HavokPackFileHandle,
        coll_idx: usize,
        out_name: *mut u8,
        out_name_len: usize,
        out_num_verts: *mut usize,
        out_num_indices: *mut usize,
    ) -> i32;
    fn havok_get_collision_mesh_data(
        handle: *mut HavokPackFileHandle,
        coll_idx: usize,
        out_verts: *mut f32,
        max_verts: usize,
        out_indices: *mut u32,
        max_indices: usize,
    ) -> i32;
}

/// Authored animation metadata without a decompressed frame cache.
#[derive(Debug)]
pub struct NativeAnimationInfo {
    pub name: String,
    pub duration: f32,
    pub frame_duration: f32,
    pub num_frames: usize,
    pub num_tracks: usize,
    pub track_to_bone: Vec<i16>,
}

pub struct NativeHavokPackFile {
    handle: *mut HavokPackFileHandle,
}

impl NativeHavokPackFile {
    pub fn load_memory(data: &[u8]) -> Option<Self> {
        let handle = unsafe { havok_load_packfile_memory(data.as_ptr(), data.len()) };
        if handle.is_null() {
            None
        } else {
            Some(Self { handle })
        }
    }

    pub fn num_skeletons(&self) -> usize {
        unsafe { havok_get_num_skeletons(self.handle) }
    }

    pub fn get_skeleton(&self, index: usize) -> Option<HkxSkeleton> {
        let mut name_buf = [0u8; 128];
        let mut num_bones: usize = 0;
        let res = unsafe {
            havok_get_skeleton_info(
                self.handle,
                index,
                name_buf.as_mut_ptr(),
                name_buf.len(),
                &mut num_bones as *mut usize,
            )
        };
        if res != 0 || num_bones == 0 {
            return None;
        }

        let name_end = name_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_buf.len());
        let skel_name = String::from_utf8_lossy(&name_buf[..name_end]).to_string();

        let mut ffi_bones = vec![
            FfiBone {
                name: [0; 128],
                parent_index: -1,
                local_translation: [0.0; 3],
                local_rotation: [0.0, 0.0, 0.0, 1.0],
                local_scale: [1.0; 3],
            };
            num_bones
        ];

        let status = unsafe {
            havok_get_skeleton_bones(self.handle, index, ffi_bones.as_mut_ptr(), num_bones)
        };

        if status != 0 {
            return None;
        }

        let bones = ffi_bones
            .into_iter()
            .map(|b| {
                let n_end = b.name.iter().position(|&c| c == 0).unwrap_or(b.name.len());
                let bname = String::from_utf8_lossy(&b.name[..n_end]).to_string();
                HkxBone {
                    name: bname,
                    parent_index: b.parent_index,
                    local_translation: b.local_translation,
                    local_rotation: b.local_rotation,
                    local_scale: b.local_scale,
                    lock_translation: false,
                }
            })
            .collect();

        Some(HkxSkeleton {
            name: skel_name,
            bones,
        })
    }

    pub fn num_animations(&self) -> usize {
        unsafe { havok_get_num_animations(self.handle) }
    }

    /// Sample the compressed animation directly, without interpolating a frame cache.
    /// Time is explicitly bounded; looping is the caller's responsibility.
    pub fn sample_animation(&self, index: usize, time: f32) -> Option<Vec<FfiTransform>> {
        let mut duration = 0.;
        let mut frame_duration = 0.;
        let mut frames = 0;
        let mut tracks = 0;
        let status = unsafe { havok_get_animation_info(self.handle, index,
            std::ptr::null_mut(), 0, &mut duration, &mut frame_duration, &mut frames, &mut tracks) };
        if status != 0 || !time.is_finite() || !duration.is_finite()
            || time < 0. || time > duration || tracks == 0 { return None; }
        let mut result = vec![FfiTransform {
            translation: [f32::NAN; 3], rotation: [f32::NAN; 4], scale: [f32::NAN; 3],
        }; tracks];
        let status = unsafe { havok_sample_animation_pose(self.handle, index, time,
            result.as_mut_ptr(), result.len()) };
        if status != 0 || result.iter().any(|t| t.translation.iter().chain(&t.rotation)
            .chain(&t.scale).any(|v| !v.is_finite())) { return None; }
        Some(result)
    }

    pub fn animation_info(&self, index: usize) -> Option<NativeAnimationInfo> {
        let mut name_buf = [0u8; 128];
        let mut duration: f32 = 0.0;
        let mut frame_duration: f32 = 0.03333333;
        let mut num_frames: usize = 0;
        let mut num_tracks: usize = 0;

        let res = unsafe {
            havok_get_animation_info(
                self.handle,
                index,
                name_buf.as_mut_ptr(),
                name_buf.len(),
                &mut duration as *mut f32,
                &mut frame_duration as *mut f32,
                &mut num_frames as *mut usize,
                &mut num_tracks as *mut usize,
            )
        };

        if res != 0 {
            return None;
        }

        let name_end = name_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_buf.len());
        let anim_name = String::from_utf8_lossy(&name_buf[..name_end]).to_string();

        if num_frames == 0 || num_tracks == 0 { return None; }
        if !duration.is_finite() || duration <= 0. || !frame_duration.is_finite() || frame_duration <= 0. { return None; }
        let mut binding_count=0;
        if unsafe {havok_get_animation_binding(self.handle,index,std::ptr::null_mut(),0,&mut binding_count)} != 0 {return None;}
        // Empty bindings remain empty; never invent evidence of authored track order.
        if binding_count != 0 && binding_count != num_tracks {return None;}
        let mut track_to_bone=vec![0i16;binding_count];
        if binding_count>0 && unsafe {havok_get_animation_binding(self.handle,index,track_to_bone.as_mut_ptr(),binding_count,&mut binding_count)} != 0 {return None;}

        Some(NativeAnimationInfo { name: anim_name, duration, frame_duration, num_frames, num_tracks, track_to_bone })
    }

    pub fn get_animation(&self, index: usize) -> Option<HkxAnimation> {
        let info = self.animation_info(index)?;
        let mut frames = Vec::with_capacity(info.num_frames);
        for frame in 0..info.num_frames {
            // Floating-point multiplication can place the last authored sample
            // slightly beyond duration. The final sample is the endpoint.
            let time = (frame as f32 * info.frame_duration).min(info.duration);
            let pose = self.sample_animation(index, time)?.into_iter().map(|t| HkxTransformSample {
                translation: t.translation, rotation: t.rotation, scale: t.scale,
                has_pos: true, has_rot: true, has_scale: true,
            }).collect();
            frames.push(pose);
        }
        Some(HkxAnimation {
            name: info.name, duration: info.duration, frame_duration: info.frame_duration,
            num_frames: info.num_frames, num_tracks: info.num_tracks,
            animation_type: "hkaAnimation".to_string(), track_to_bone: info.track_to_bone, frames,
        })
    }

    pub fn decompile_to_xml(data: &[u8]) -> Option<String> {
        let mut written: usize = 0;
        let mut buf = vec![0u8; data.len() * 10 + 65536];
        let res = unsafe {
            havok_decompile_to_xml(
                data.as_ptr(),
                data.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut written as *mut usize,
            )
        };

        if res == 0 && written > 0 {
            if written > buf.len() {
                buf.resize(written + 1, 0);
                unsafe {
                    havok_decompile_to_xml(
                        data.as_ptr(),
                        data.len(),
                        buf.as_mut_ptr(),
                        buf.len(),
                        &mut written as *mut usize,
                    );
                }
            }
            let end_pos = buf.iter().position(|&b| b == 0).unwrap_or(written);
            Some(String::from_utf8_lossy(&buf[..end_pos]).to_string())
        } else {
            None
        }
    }

    pub fn compile_from_xml(xml_str: &str) -> Option<Vec<u8>> {
        let mut written: usize = 0;
        let xml_bytes = xml_str.as_bytes();
        let mut buf = vec![0u8; xml_bytes.len() * 2 + 65536];
        let res = unsafe {
            havok_compile_from_xml(
                xml_bytes.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut written as *mut usize,
            )
        };

        if res == 0 && written > 0 {
            buf.truncate(written);
            Some(buf)
        } else {
            None
        }
    }

    pub fn get_objects(&self) -> Vec<HkxObjectInfo> {
        let count = unsafe { havok_get_num_objects(self.handle) };
        let mut results = Vec::with_capacity(count);

        for i in 0..count {
            let mut name_buf = [0u8; 128];
            let mut addr: u64 = 0;
            let res = unsafe {
                havok_get_object_info(
                    self.handle,
                    i,
                    name_buf.as_mut_ptr(),
                    name_buf.len(),
                    &mut addr as *mut u64,
                )
            };
            if res == 0 {
                let end = name_buf
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_buf.len());
                let cname = String::from_utf8_lossy(&name_buf[..end]).to_string();
                results.push(HkxObjectInfo {
                    class_name: cname,
                    address: addr,
                });
            }
        }
        results
    }

    pub fn get_collisions(&self) -> Vec<HkxCollisionMesh> {
        let count = unsafe { havok_get_num_collisions(self.handle) };
        let mut results = Vec::with_capacity(count);

        for i in 0..count {
            let mut name_buf = [0u8; 128];
            let mut num_verts: usize = 0;
            let mut num_indices: usize = 0;

            let res = unsafe {
                havok_get_collision_info(
                    self.handle,
                    i,
                    name_buf.as_mut_ptr(),
                    name_buf.len(),
                    &mut num_verts as *mut usize,
                    &mut num_indices as *mut usize,
                )
            };

            if res == 0 && num_verts > 0 {
                let end = name_buf
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_buf.len());
                let cname = String::from_utf8_lossy(&name_buf[..end]).to_string();

                let mut raw_verts = vec![0.0f32; num_verts * 3];
                let mut raw_indices = vec![0u32; num_indices];

                let data_res = unsafe {
                    havok_get_collision_mesh_data(
                        self.handle,
                        i,
                        raw_verts.as_mut_ptr(),
                        num_verts,
                        raw_indices.as_mut_ptr(),
                        num_indices,
                    )
                };

                if data_res == 0 {
                    let mut vertices = Vec::with_capacity(num_verts);
                    for v_idx in 0..num_verts {
                        vertices.push([
                            raw_verts[v_idx * 3],
                            raw_verts[v_idx * 3 + 1],
                            raw_verts[v_idx * 3 + 2],
                        ]);
                    }
                    results.push(HkxCollisionMesh {
                        name: cname,
                        vertices,
                        indices: raw_indices,
                    });
                }
            }
        }
        results
    }
}

impl Drop for NativeHavokPackFile {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { havok_free_packfile(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

unsafe impl Send for NativeHavokPackFile {}
unsafe impl Sync for NativeHavokPackFile {}

/// Call native C++ Havok decompressor to decode all tracks for a single frame value
pub fn decode_spline_frame_cpp(
    m_data: &[u8],
    block_offset: usize,
    num_tracks: usize,
    num_float_tracks: usize,
    frame_val: f32,
) -> Vec<HkxTransformSample> {
    if m_data.is_empty() || num_tracks == 0 {
        return vec![HkxTransformSample::default(); num_tracks];
    }

    let mut ffi_samples = vec![
        FfiTransformSample {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            has_pos: 0,
            has_rot: 0,
            has_scale: 0,
        };
        num_tracks
    ];

    let status = unsafe {
        havok_decode_spline_block(
            m_data.as_ptr(),
            m_data.len(),
            block_offset,
            num_tracks,
            num_float_tracks,
            frame_val,
            ffi_samples.as_mut_ptr(),
        )
    };

    if status != 0 {
        return vec![HkxTransformSample::default(); num_tracks];
    }

    ffi_samples
        .into_iter()
        .map(|s| HkxTransformSample {
            translation: s.translation,
            rotation: s.rotation,
            scale: s.scale,
            has_pos: s.has_pos != 0,
            has_rot: s.has_rot != 0,
            has_scale: s.has_scale != 0,
        })
        .collect()
}
