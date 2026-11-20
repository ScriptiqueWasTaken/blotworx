//! Rendering and asset decoding shared by the runtime.
pub(crate) mod baked_preview;
pub(crate) mod clip_visibility;
pub(crate) mod clustered_lights;
pub(crate) mod contact_occlusion;
pub(crate) mod game_config;
pub(crate) mod gpu_timing;
pub(crate) mod material_preview;
pub(crate) mod material_vertex_color;
pub(crate) mod model_shadow;
pub(crate) mod native_material;
pub(crate) mod nif;
pub(crate) mod scene;
pub(crate) mod scene_finish;
#[cfg(any(test, debug_assertions))]
pub(crate) mod shader;
pub(crate) mod spatial_lights;
pub(crate) mod surface_lighting;
pub(crate) mod surface_material;
pub mod viewport3d;
pub(crate) mod volume_shadow;
pub(crate) mod wii_default;
