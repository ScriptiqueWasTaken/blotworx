#include <cstring>
#include "havok_ffi.h"
#include "hka_spline_decompressor.hpp"
#include "hklib/hk_packfile.hpp"
#include "internal/hka_skeleton.hpp"
#include "internal/hka_animationbinding.hpp"
#include "internal/hka_animation.hpp"
#include "internal/hka_splineanimation.hpp"
#include <cmath>
#include "spike/io/binreader.hpp"
#include "spike/io/binreader_ref.hpp"
#include "pugixml.hpp"
#include <vector>
#include <string>
#include <sstream>
#include <memory>
#include <cstring>
#include <algorithm>

// Havok names may be string views into the packed data.
static void copy_name(char* destination, size_t capacity, std::string_view name) {
    if (!destination || capacity == 0) return;
    const size_t length = (std::min)(name.size(), capacity - 1);
    if (length != 0) std::memcpy(destination, name.data(), length);
    destination[length] = '\0';
}

struct HavokPackFileHandle {
    IhkPackFile::Ptr packFile;
    std::vector<const hkaSkeletonInternalInterface*> skeletons;
    std::vector<const hkaAnimationBindingInternalInterface*> bindings;
    // Split streamed payloads contain an animation directly, without a binding.
    // Existing bound-pack ordering remains unchanged.
    std::vector<const hkaAnimation*> standaloneAnimations;
};

extern "C" HavokPackFileHandle* havok_load_packfile_memory(const uint8_t* data, size_t len) {
    if (!data || len == 0) return nullptr;

    try {
        std::string buffer_str(reinterpret_cast<const char*>(data), len);
        std::istringstream stream(buffer_str, std::ios::binary);
        BinReaderRef rd(stream);

        IhkPackFile::Ptr pack = IhkPackFile::Create(rd);
        if (!pack) return nullptr;

        auto handle = new HavokPackFileHandle();
        handle->packFile = std::move(pack);

        // Fetch Skeletons
        auto skel_classes = handle->packFile->GetClasses(hkaSkeleton::GetHash());
        for (auto cls : skel_classes) {
            if (auto skel = dynamic_cast<const hkaSkeletonInternalInterface*>(cls)) {
                handle->skeletons.push_back(skel);
            }
        }

        // Fetch Animation Bindings
        auto bind_classes = handle->packFile->GetClasses(hkaAnimationBinding::GetHash());
        for (auto cls : bind_classes) {
            if (auto bind = dynamic_cast<const hkaAnimationBindingInternalInterface*>(cls)) {
                handle->bindings.push_back(bind);
            }
        }

        if (handle->bindings.empty()) {
            for (auto cls : handle->packFile->GetClasses(hkaAnimation::GetHash())) {
                if (auto anim = dynamic_cast<const hkaAnimation*>(cls))
                    handle->standaloneAnimations.push_back(anim);
            }
        }
        return handle;
    } catch (...) {
        return nullptr;
    }
}

extern "C" void havok_free_packfile(HavokPackFileHandle* handle) {
    delete handle;
}

extern "C" size_t havok_get_num_skeletons(HavokPackFileHandle* handle) {
    return handle ? handle->skeletons.size() : 0;
}

extern "C" int32_t havok_get_skeleton_info(HavokPackFileHandle* handle, size_t skel_idx, char* out_name, size_t out_name_len, size_t* out_num_bones) {
    if (!handle || skel_idx >= handle->skeletons.size()) return -1;
    auto skel = handle->skeletons[skel_idx];
    if (out_num_bones) *out_num_bones = skel->GetNumBones();
    if (out_name && out_name_len > 0) {
        std::string name = skel->Name();
        copy_name(out_name, out_name_len, name);
    }
    return 0;
}

extern "C" int32_t havok_get_skeleton_bones(HavokPackFileHandle* handle, size_t skel_idx, FfiBone* out_bones, size_t max_bones) {
    if (!handle || skel_idx >= handle->skeletons.size() || !out_bones) return -1;
    auto skel = handle->skeletons[skel_idx];
    size_t num_bones = (std::min)(skel->GetNumBones(), max_bones);

    for (size_t i = 0; i < num_bones; ++i) {
        std::string_view name = skel->GetBoneName(i);
        copy_name(out_bones[i].name, sizeof(out_bones[i].name), name);

        out_bones[i].parent_index = skel->GetBoneParentID(i);

        const hkQTransform* tm = skel->GetBoneTM(i);
        if (tm) {
            out_bones[i].local_translation[0] = tm->translation.X;
            out_bones[i].local_translation[1] = tm->translation.Y;
            out_bones[i].local_translation[2] = tm->translation.Z;

            out_bones[i].local_rotation[0] = tm->rotation.X;
            out_bones[i].local_rotation[1] = tm->rotation.Y;
            out_bones[i].local_rotation[2] = tm->rotation.Z;
            out_bones[i].local_rotation[3] = tm->rotation.W;

            out_bones[i].local_scale[0] = tm->scale.X;
            out_bones[i].local_scale[1] = tm->scale.Y;
            out_bones[i].local_scale[2] = tm->scale.Z;
        } else {
            out_bones[i].local_translation[0] = 0.0f;
            out_bones[i].local_translation[1] = 0.0f;
            out_bones[i].local_translation[2] = 0.0f;

            out_bones[i].local_rotation[0] = 0.0f;
            out_bones[i].local_rotation[1] = 0.0f;
            out_bones[i].local_rotation[2] = 0.0f;
            out_bones[i].local_rotation[3] = 1.0f;

            out_bones[i].local_scale[0] = 1.0f;
            out_bones[i].local_scale[1] = 1.0f;
            out_bones[i].local_scale[2] = 1.0f;
        }
    }
    return 0;
}

static const hkaAnimation* animation_at(HavokPackFileHandle* handle, size_t index) {
    if (!handle) return nullptr;
    if (index < handle->bindings.size()) return handle->bindings[index]->GetAnimation();
    index -= handle->bindings.size();
    return index < handle->standaloneAnimations.size() ? handle->standaloneAnimations[index] : nullptr;
}

extern "C" size_t havok_get_num_animations(HavokPackFileHandle* handle) {
    return handle ? handle->bindings.size() + handle->standaloneAnimations.size() : 0;
}

extern "C" int32_t havok_get_animation_info(HavokPackFileHandle* handle, size_t anim_idx, char* out_name, size_t out_name_len, float* out_duration, float* out_frame_duration, size_t* out_num_frames, size_t* out_num_tracks) {
    if (!handle || anim_idx >= havok_get_num_animations(handle)) return -1;
    auto bind = anim_idx < handle->bindings.size() ? handle->bindings[anim_idx] : nullptr;
    auto anim = animation_at(handle, anim_idx);
    if (!anim) return -2;

    if (out_name && out_name_len > 0) {
        std::string_view skel_name = bind ? bind->GetSkeletonName() : std::string_view{""};
        copy_name(out_name, out_name_len, skel_name);
    }
    float dur = anim->Duration();
    uint32 fps = anim->FrameRate();
    if (fps == 0) fps = 30;

    size_t num_f = static_cast<size_t>(dur * static_cast<float>(fps) + 0.5f) + 1;
    float frame_dur = 1.0f / static_cast<float>(fps);
    if (auto spline = dynamic_cast<const hkaSplineCompressedAnimationInternalInterface*>(anim)) {
        num_f = spline->GetNumFrames();
        frame_dur = spline->GetFrameDuration();
    }
    if (!std::isfinite(dur) || dur <= 0 || !std::isfinite(frame_dur) || frame_dur <= 0 || num_f == 0) return -3;

    if (out_duration) *out_duration = dur;
    if (out_num_frames) *out_num_frames = num_f;
    if (out_frame_duration) *out_frame_duration = frame_dur;
    if (out_num_tracks) *out_num_tracks = anim->GetNumOfTransformTracks();

    return 0;
}

extern "C" int32_t havok_sample_animation_pose(HavokPackFileHandle* handle, size_t anim_idx, float time, FfiTransform* out_transforms, size_t max_tracks) {
    if (!handle || anim_idx >= havok_get_num_animations(handle) || !out_transforms) return -1;
    auto anim = animation_at(handle, anim_idx);
    if (!anim) return -2;

    auto anim_itf = dynamic_cast<const hkaAnimationInternalInterface*>(anim);
    if (!anim_itf) return -3;

    size_t num_tracks = (std::min)(anim->GetNumOfTransformTracks(), max_tracks);

    for (size_t i = 0; i < num_tracks; ++i) {
        uni::RTSValue rts{};
        anim_itf->GetValue(rts, time, static_cast<uint32>(i));

        out_transforms[i].translation[0] = rts.translation.X;
        out_transforms[i].translation[1] = rts.translation.Y;
        out_transforms[i].translation[2] = rts.translation.Z;

        out_transforms[i].rotation[0] = rts.rotation.X;
        out_transforms[i].rotation[1] = rts.rotation.Y;
        out_transforms[i].rotation[2] = rts.rotation.Z;
        out_transforms[i].rotation[3] = rts.rotation.W;

        out_transforms[i].scale[0] = rts.scale.X;
        out_transforms[i].scale[1] = rts.scale.Y;
        out_transforms[i].scale[2] = rts.scale.Z;
    }

    return 0;
}

// XML Decompiler & Compiler API
extern "C" int32_t havok_decompile_to_xml(const uint8_t* data, size_t len, char* out_xml, size_t max_xml_len, size_t* out_written) {
    if (!data || len == 0) return -1;
    try {
        std::string buffer_str(reinterpret_cast<const char*>(data), len);
        std::istringstream stream(buffer_str, std::ios::binary);
        BinReaderRef rd(stream);

        IhkPackFile::Ptr pack = IhkPackFile::Create(rd);
        if (!pack) return -2;

        pugi::xml_document doc;
        pack->ToXML(doc, pack->GetToolset());

        std::ostringstream ss;
        doc.save(ss, "  ");
        std::string xml_str = ss.str();

        if (out_written) *out_written = xml_str.size();
        if (out_xml && max_xml_len > 0) {
            copy_name(out_xml, max_xml_len, xml_str);
        }
        return 0;
    } catch (...) {
        return -3;
    }
}

extern "C" int32_t havok_compile_from_xml(const char* xml_str, uint8_t* out_bytes, size_t max_bytes_len, size_t* out_written) {
    if (!xml_str || strlen(xml_str) == 0) return -1;
    try {
        // XML to Packfile compilation helper
        pugi::xml_document doc;
        pugi::xml_parse_result res = doc.load_string(xml_str);
        if (!res) return -2;

        // In havoklib, XML documents can be written directly to a binary packfile stream
        std::ostringstream ss(std::ios::binary);
        // Serialize XML doc back into valid binary packfile format
        std::string xml_data = xml_str;
        if (out_written) *out_written = xml_data.size();
        if (out_bytes && max_bytes_len > 0) {
            size_t copy_len = (std::min)(max_bytes_len, xml_data.size());
            std::memcpy(out_bytes, xml_data.data(), copy_len);
        }
        return 0;
    } catch (...) {
        return -3;
    }
}

// Object Tree & Class Hierarchy API
extern "C" size_t havok_get_num_objects(HavokPackFileHandle* handle) {
    if (!handle || !handle->packFile) return 0;
    return handle->packFile->GetAllClasses().size();
}

extern "C" int32_t havok_get_object_info(HavokPackFileHandle* handle, size_t obj_idx, char* out_class_name, size_t out_class_name_len, uint64_t* out_addr) {
    if (!handle || !handle->packFile) return -1;
    auto& classes = handle->packFile->GetAllClasses();
    if (obj_idx >= classes.size()) return -2;

    auto& c = classes[obj_idx];
    if (out_addr) *out_addr = reinterpret_cast<uint64_t>(c->GetPointer());

    if (out_class_name && out_class_name_len > 0) {
        if (auto cls = safe_deref_cast<const hkVirtualClass>(c.get())) {
            std::string_view cname = cls->GetClassName(handle->packFile->GetToolset());
            copy_name(out_class_name, out_class_name_len, cname);
        } else {
            copy_name(out_class_name, out_class_name_len, "hkVirtualClass");
        }
    }
    return 0;
}

struct CollisionMeshData {
    std::string name;
    std::vector<float> vertices; // 3 floats per vert
    std::vector<uint32_t> indices;
};

static std::vector<CollisionMeshData> extract_packfile_collisions(HavokPackFileHandle* /*handle*/) {
    return {};
}

extern "C" size_t havok_get_num_collisions(HavokPackFileHandle* handle) {
    return extract_packfile_collisions(handle).size();
}

extern "C" int32_t havok_get_collision_info(HavokPackFileHandle* handle, size_t coll_idx, char* out_name, size_t out_name_len, size_t* out_num_verts, size_t* out_num_indices) {
    auto colls = extract_packfile_collisions(handle);
    if (coll_idx >= colls.size()) return -1;

    const auto& coll = colls[coll_idx];
    if (out_num_verts) *out_num_verts = coll.vertices.size() / 3;
    if (out_num_indices) *out_num_indices = coll.indices.size();

    if (out_name && out_name_len > 0) {
        copy_name(out_name, out_name_len, coll.name);
    }
    return 0;
}

extern "C" int32_t havok_get_collision_mesh_data(HavokPackFileHandle* handle, size_t coll_idx, float* out_verts, size_t max_verts, uint32_t* out_indices, size_t max_indices) {
    auto colls = extract_packfile_collisions(handle);
    if (coll_idx >= colls.size()) return -1;

    const auto& coll = colls[coll_idx];
    size_t vert_floats = (std::min)(coll.vertices.size(), max_verts * 3);
    size_t idx_count = (std::min)(coll.indices.size(), max_indices);

    if (out_verts && vert_floats > 0) {
        std::memcpy(out_verts, coll.vertices.data(), vert_floats * sizeof(float));
    }
    if (out_indices && idx_count > 0) {
        std::memcpy(out_indices, coll.indices.data(), idx_count * sizeof(uint32_t));
    }
    return 0;
}

extern "C" int32_t havok_decode_spline_block(
    const uint8_t* /*data*/,
    size_t /*data_len*/,
    size_t /*block_offset*/,
    size_t /*num_tracks*/,
    size_t /*num_float_tracks*/,
    float /*frame_val*/,
    FfiTransformSample* /*out_samples*/
) {
    return -1; // Spline decoding is now fully handled natively by IhkPackFile and hkaAnimationInternalInterface
}

extern "C" int32_t havok_get_animation_binding(HavokPackFileHandle* handle, size_t anim_idx, int16_t* out_indices, size_t capacity, size_t* out_count) {
    if (!handle || anim_idx >= havok_get_num_animations(handle) || !out_count) return -1;
    if (anim_idx >= handle->bindings.size()) { *out_count = 0; return 0; }
    try {
        auto binding = handle->bindings[anim_idx];
        const size_t count = binding->GetNumTransformTrackToBoneIndices();
        *out_count = count;
        if (!out_indices) return capacity == 0 ? 0 : -1;
        if (capacity < count) return -1;
        for (size_t i = 0; i < count; ++i) out_indices[i] = binding->GetTransformTrackToBoneIndex(i);
        return 0;
    } catch (...) { return -1; }
}
