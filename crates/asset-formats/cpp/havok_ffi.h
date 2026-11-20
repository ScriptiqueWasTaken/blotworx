#ifndef HAVOK_FFI_H
#define HAVOK_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct HavokPackFileHandle HavokPackFileHandle;

typedef struct {
    char name[128];
    int16_t parent_index;
    float local_translation[3];
    float local_rotation[4]; // xyzw
    float local_scale[3];
} FfiBone;

typedef struct {
    float translation[3];
    float rotation[4]; // xyzw
    float scale[3];
} FfiTransform;

// Packfile loading & lifecycle
HavokPackFileHandle* havok_load_packfile_memory(const uint8_t* data, size_t len);
void havok_free_packfile(HavokPackFileHandle* handle);

// Skeleton API
size_t havok_get_num_skeletons(HavokPackFileHandle* handle);
int32_t havok_get_skeleton_info(HavokPackFileHandle* handle, size_t skel_idx, char* out_name, size_t out_name_len, size_t* out_num_bones);
int32_t havok_get_skeleton_bones(HavokPackFileHandle* handle, size_t skel_idx, FfiBone* out_bones, size_t max_bones);

// Animation API
int32_t havok_get_animation_binding(HavokPackFileHandle* handle, size_t anim_idx, int16_t* out_indices, size_t capacity, size_t* out_count);
size_t havok_get_num_animations(HavokPackFileHandle* handle);
int32_t havok_get_animation_info(HavokPackFileHandle* handle, size_t anim_idx, char* out_name, size_t out_name_len, float* out_duration, float* out_frame_duration, size_t* out_num_frames, size_t* out_num_tracks);
int32_t havok_sample_animation_pose(HavokPackFileHandle* handle, size_t anim_idx, float time, FfiTransform* out_transforms, size_t max_tracks);

// XML Decompiler & Compiler API
int32_t havok_decompile_to_xml(const uint8_t* data, size_t len, char* out_xml, size_t max_xml_len, size_t* out_written);
int32_t havok_compile_from_xml(const char* xml_str, uint8_t* out_bytes, size_t max_bytes_len, size_t* out_written);

// Object Tree & Class Hierarchy API
size_t havok_get_num_objects(HavokPackFileHandle* handle);
int32_t havok_get_object_info(HavokPackFileHandle* handle, size_t obj_idx, char* out_class_name, size_t out_class_name_len, uint64_t* out_addr);

// Collision & Physics API
size_t havok_get_num_collisions(HavokPackFileHandle* handle);
int32_t havok_get_collision_info(HavokPackFileHandle* handle, size_t coll_idx, char* out_name, size_t out_name_len, size_t* out_num_verts, size_t* out_num_indices);
int32_t havok_get_collision_mesh_data(HavokPackFileHandle* handle, size_t coll_idx, float* out_verts, size_t max_verts, uint32_t* out_indices, size_t max_indices);

// Legacy block decompressor
typedef struct {
    float translation[3];
    float rotation[4];
    float scale[3];
    uint8_t has_pos;
    uint8_t has_rot;
    uint8_t has_scale;
} FfiTransformSample;

int32_t havok_decode_spline_block(
    const uint8_t* block_data,
    size_t block_data_len,
    size_t block_offset,
    size_t num_tracks,
    size_t num_float_tracks,
    float frame_val,
    FfiTransformSample* out_samples
);

#ifdef __cplusplus
}
#endif

#endif // HAVOK_FFI_H
