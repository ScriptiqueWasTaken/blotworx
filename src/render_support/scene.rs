use anyhow::Result;
use egui::{
    Align2, Color32, FontId, Grid, Pos2, RichText, ScrollArea, TextEdit, TextureHandle, Ui, Vec2,
};
use epic_mickey_lib_rs::file_manipulator::EndianType;
use epic_mickey_lib_rs::nif_file::{NiTransform, NiWiiUPersistentSrcTextureRendererData, NifFile};
use epic_mickey_lib_rs::packfile::VirtualFile;
use epic_mickey_lib_rs::scene_file::{
    ColorRGB, Component, Entity, Matrix3, Point3, Property, SceneFile, ID,
};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
pub type SceneViewportState = super::viewport3d::Viewport3DState;
pub type DecodedImage = super::viewport3d::DecodedImage;
pub type LightingMode = super::viewport3d::LightingMode;

pub(crate) fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

pub(crate) fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn perspective_matrix(fov_rad: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov_rad * 0.5).tan();
    let range_inv = 1.0 / (near - far);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * range_inv,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * range_inv,
        0.0,
    ]
}

pub(crate) fn look_at_matrix(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = normalize3([target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]]);
    let r = normalize3(cross3(f, up));
    let u = cross3(r, f);

    [
        r[0],
        u[0],
        -f[0],
        0.0,
        r[1],
        u[1],
        -f[1],
        0.0,
        r[2],
        u[2],
        -f[2],
        0.0,
        -dot3(r, eye),
        -dot3(u, eye),
        dot3(f, eye),
        1.0,
    ]
}

pub(crate) fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = a[0 * 4 + row] * b[col * 4 + 0]
                + a[1 * 4 + row] * b[col * 4 + 1]
                + a[2 * 4 + row] * b[col * 4 + 2]
                + a[3 * 4 + row] * b[col * 4 + 3];
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3D {
    pub translation: [f32; 3],
    pub rotation: [[f32; 3]; 3],
    pub scale: [f32; 3],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

fn transform_point(p: [f32; 3], t: &Transform3D) -> [f32; 3] {
    let sx = p[0] * t.scale[0];
    let sy = p[1] * t.scale[1];
    let sz = p[2] * t.scale[2];
    let rx = t.rotation[0][0] * sx + t.rotation[1][0] * sy + t.rotation[2][0] * sz;
    let ry = t.rotation[0][1] * sx + t.rotation[1][1] * sy + t.rotation[2][1] * sz;
    let rz = t.rotation[0][2] * sx + t.rotation[1][2] * sy + t.rotation[2][2] * sz;
    [
        t.translation[0] + rx,
        t.translation[1] + ry,
        t.translation[2] + rz,
    ]
}

fn transform_nif_point(p: [f32; 3], t: &Transform3D) -> [f32; 3] {
    transform_point(p, t)
}

#[derive(Clone, Copy)]
struct Mat4 {
    m: [[f32; 4]; 4],
}

impl Mat4 {
    fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn from_transform(trans: &NiTransform) -> Self {
        let r = &trans.rotation.rows;
        let s = if trans.scale == 0.0 { 1.0 } else { trans.scale };
        let t = &trans.translation;
        Self {
            m: [
                [r[0][0] * s, r[0][1] * s, r[0][2] * s, t.x],
                [r[1][0] * s, r[1][1] * s, r[1][2] * s, t.y],
                [r[2][0] * s, r[2][1] * s, r[2][2] * s, t.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn mul(&self, other: &Mat4) -> Mat4 {
        let mut res = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                res[r][c] = self.m[r][0] * other.m[0][c]
                    + self.m[r][1] * other.m[1][c]
                    + self.m[r][2] * other.m[2][c]
                    + self.m[r][3] * other.m[3][c];
            }
        }
        Mat4 { m: res }
    }

    fn from_transform3d(t: &Transform3D) -> Self {
        let r = t.rotation;
        let s = t.scale;
        let pos = t.translation;
        Self {
            m: [
                [r[0][0] * s[0], r[0][1] * s[1], r[0][2] * s[2], pos[0]],
                [r[1][0] * s[0], r[1][1] * s[1], r[1][2] * s[2], pos[1]],
                [r[2][0] * s[0], r[2][1] * s[1], r[2][2] * s[2], pos[2]],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn to_flat_array_column_major(&self) -> [f32; 16] {
        [
            self.m[0][0],
            self.m[1][0],
            self.m[2][0],
            self.m[3][0],
            self.m[0][1],
            self.m[1][1],
            self.m[2][1],
            self.m[3][1],
            self.m[0][2],
            self.m[1][2],
            self.m[2][2],
            self.m[3][2],
            self.m[0][3],
            self.m[1][3],
            self.m[2][3],
            self.m[3][3],
        ]
    }

    fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let x = self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.m[0][2] * p[2] + self.m[0][3];
        let y = self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.m[1][2] * p[2] + self.m[1][3];
        let z = self.m[2][0] * p[0] + self.m[2][1] * p[1] + self.m[2][2] * p[2] + self.m[2][3];
        [x, y, z]
    }

    fn transform_vector(&self, v: [f32; 3]) -> [f32; 3] {
        let x = self.m[0][0] * v[0] + self.m[0][1] * v[1] + self.m[0][2] * v[2];
        let y = self.m[1][0] * v[0] + self.m[1][1] * v[1] + self.m[1][2] * v[2];
        let z = self.m[2][0] * v[0] + self.m[2][1] * v[1] + self.m[2][2] * v[2];
        let len = (x * x + y * y + z * z).sqrt();
        if len > 1e-5 {
            [x / len, y / len, z / len]
        } else {
            [0.0, 1.0, 0.0]
        }
    }
}

fn parse_entity_id(val: &serde_json::Value) -> Option<u32> {
    if let Some(num) = val.as_u64() {
        Some(num as u32)
    } else if let Some(obj) = val.as_object() {
        obj.get("id").and_then(|v| v.as_u64()).map(|v| v as u32)
    } else {
        None
    }
}

fn parse_color3(val: &serde_json::Value) -> Option<[f32; 3]> {
    if let Some(obj) = val.as_object() {
        let r = obj
            .get("r")
            .or_else(|| obj.get("red"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;
        let g = obj
            .get("g")
            .or_else(|| obj.get("green"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;
        let b = obj
            .get("b")
            .or_else(|| obj.get("blue"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;
        let scale = if r > 1.0 || g > 1.0 || b > 1.0 {
            1.0 / 255.0
        } else {
            1.0
        };
        Some([r * scale, g * scale, b * scale])
    } else if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let r = arr[0].as_f64().unwrap_or(1.0) as f32;
            let g = arr[1].as_f64().unwrap_or(1.0) as f32;
            let b = arr[2].as_f64().unwrap_or(1.0) as f32;
            let scale = if r > 1.0 || g > 1.0 || b > 1.0 {
                1.0 / 255.0
            } else {
                1.0
            };
            return Some([r * scale, g * scale, b * scale]);
        }
        None
    } else {
        None
    }
}

fn compute_scene_entity_world_transforms(scene: &SceneFile) -> Vec<Mat4> {
    let count = scene.objects.len();
    let mut world_mats = vec![Mat4::identity(); count];

    for i in 0..count {
        let self_trans = get_entity_transform(&scene.objects[i], &scene.objects);
        world_mats[i] = Mat4::from_transform3d(&self_trans);
    }

    world_mats
}
fn extract_scene_lights(
    scene: &SceneFile,
    entity_world_mats: &[Mat4],
) -> Vec<super::viewport3d::GpuLight> {
    extract_scene_lights_filtered(scene, entity_world_mats, |_| true)
}
fn extract_scene_lights_filtered(
    scene: &SceneFile,
    entity_world_mats: &[Mat4],
    include: impl Fn(&Entity) -> bool,
) -> Vec<super::viewport3d::GpuLight> {
    let mut lights = Vec::new();
    for (idx, entity) in scene.objects.iter().enumerate() {
        if !include(entity) {
            continue;
        }
        let eff_comps = get_effective_components(entity, &scene.objects);

        let mut has_light = false;
        let mut light = super::viewport3d::GpuLight::default();
        let wm = &entity_world_mats[idx];
        light.position = [wm.m[0][3], wm.m[1][3], wm.m[2][3]];

        let dir_x = -wm.m[0][2];
        let dir_y = -wm.m[1][2];
        let dir_z = -wm.m[2][2];
        let len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z)
            .sqrt()
            .max(0.0001);
        light.direction = [dir_x / len, dir_y / len, dir_z / len];

        for comp in &eff_comps {
            let class_lc = comp.class_name.to_lowercase();
            if class_lc.contains("light") && !class_lc.contains("baking") {
                has_light = true;
                for prop in &comp.properties {
                    let name_lc = prop.name.to_lowercase();
                    match name_lc.as_str() {
                        "light group" => {
                            // Store trimmed group; "" and "(null)" both treated as dynamic
                            light.light_group =
                                prop.value.as_str().unwrap_or("").trim().to_string();
                        }
                        "light type" | "type" => {
                            if let Some(t_str) = prop.value.as_str() {
                                match t_str.to_lowercase().as_str() {
                                    "ambient" => light.light_type = 0,
                                    "directional" => light.light_type = 1,
                                    "point" => light.light_type = 2,
                                    "spot" => light.light_type = 3,
                                    _ => {}
                                }
                            } else if let Some(t_num) = prop.value.as_i64() {
                                light.light_type = t_num as i32;
                            }
                        }
                        "dimmer" | "intensity" => {
                            if let Some(d) = prop.value.as_f64() {
                                let val = d as f32;
                                light.dimmer = if val.is_finite() { val.max(0.0) } else { 0.0 };
                            }
                        }
                        "color (ambient)" | "ambient color" | "ambient" => {
                            if let Some(c) = parse_color3(&prop.value) {
                                light.ambient = c;
                            }
                        }
                        "color (diffuse)" | "diffuse color" | "diffuse" | "color" => {
                            if let Some(c) = parse_color3(&prop.value) {
                                light.diffuse = c;
                            }
                        }
                        "color (specular)" | "specular color" | "specular" => {
                            if let Some(c) = parse_color3(&prop.value) {
                                light.specular = c;
                            }
                        }
                        "attenuation (constant)" | "attenuation constant" | "atten0" => {
                            if let Some(a) = prop.value.as_f64() {
                                light.attenuation[0] = a as f32;
                            }
                        }
                        "attenuation (linear)" | "attenuation linear" | "atten1" => {
                            if let Some(a) = prop.value.as_f64() {
                                light.attenuation[1] = a as f32;
                            }
                        }
                        "attenuation (quadratic)" | "attenuation quadratic" | "atten2" => {
                            if let Some(a) = prop.value.as_f64() {
                                light.attenuation[2] = a as f32;
                            }
                        }
                        // Use Max Falloff Range as the attenuation radius
                        "max falloff range" | "maxfalloffrange" | "range" | "radius" => {
                            if let Some(r) = prop.value.as_f64() {
                                light.range = r as f32;
                            }
                        }
                        "spot angle (inner)" | "inner spot angle" | "spot inner angle" => {
                            if let Some(a) = prop.value.as_f64() {
                                light.spot_inner = (a as f32).to_radians().cos();
                            }
                        }
                        "spot angle (outer)" | "outer spot angle" | "spot angle" => {
                            if let Some(a) = prop.value.as_f64() {
                                light.spot_outer = (a as f32).to_radians().cos();
                            }
                        }
                        "spot exponent" => {
                            if let Some(e) = prop.value.as_f64() {
                                light.spot_exponent = e as f32;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if has_light {
            lights.push(light);
        }
    }

    lights
}

fn transform_normal(n: [f32; 3], t: &Transform3D) -> [f32; 3] {
    // Transform without scale or translation
    let rx = t.rotation[0][0] * n[0] + t.rotation[1][0] * n[1] + t.rotation[2][0] * n[2];
    let ry = t.rotation[0][1] * n[0] + t.rotation[1][1] * n[1] + t.rotation[2][1] * n[2];
    let rz = t.rotation[0][2] * n[0] + t.rotation[1][2] * n[1] + t.rotation[2][2] * n[2];
    [rx, ry, rz]
}

fn parse_scale_float(val: &serde_json::Value) -> Option<f32> {
    if let Some(num) = val.as_f64() {
        Some(num as f32)
    } else if let Some(s_str) = val.as_str() {
        s_str.trim().parse::<f32>().ok()
    } else {
        None
    }
}

pub(crate) fn get_entity_transform(entity: &Entity, all_objects: &[Entity]) -> Transform3D {
    let mut trans = Transform3D::default();
    let mut found_trans = false;
    let mut found_rot = false;
    let mut found_scale = false;

    for e in resolve_master_chain(entity, all_objects) {
        for comp in &e.components {
            for prop in &comp.properties {
                if prop.name.eq_ignore_ascii_case("Translation") {
                    if !found_trans {
                        if let Some(t) = parse_point3(&prop.value) {
                            trans.translation = t;
                            found_trans = true;
                        }
                    }
                } else if prop.name.eq_ignore_ascii_case("Rotation") {
                    if !found_rot {
                        if let Some(r) = parse_matrix3(&prop.value) {
                            trans.rotation = r;
                            found_rot = true;
                        }
                    }
                } else if prop.name.eq_ignore_ascii_case("Scale") {
                    if !found_scale {
                        if let Some(s) = parse_scale_float(&prop.value) {
                            trans.scale = [s, s, s];
                            found_scale = true;
                        }
                    }
                }
            }
        }

        if found_trans && found_rot && found_scale {
            break;
        }
    }

    trans
}

pub(crate) fn set_entity_transform(entity: &mut Entity, trans: Transform3D) {
    let mut found_rotation_prop = false;
    let mut found_scale_prop = false;
    let mut found_trans_prop = false;
    let scale_float = trans.scale[0];

    for comp in &mut entity.components {
        if comp.name.eq_ignore_ascii_case("Transformation")
            || comp.class_name.contains("Transformation")
            || comp.class_name.contains("Transform")
        {
            for prop in &mut comp.properties {
                if prop.name.eq_ignore_ascii_case("Translation") {
                    found_trans_prop = true;
                    prop.value = Point3::new(
                        trans.translation[0],
                        trans.translation[1],
                        trans.translation[2],
                    )
                    .to_dict();
                } else if prop.name.eq_ignore_ascii_case("Rotation") {
                    found_rotation_prop = true;
                    prop.value = Matrix3::new(trans.rotation).to_dict();
                } else if prop.name.eq_ignore_ascii_case("Scale") {
                    found_scale_prop = true;
                    if let Some(num) = serde_json::Number::from_f64(scale_float as f64) {
                        prop.value = serde_json::Value::Number(num);
                    }
                }
            }

            if !found_rotation_prop {
                comp.properties.push(Property::new(
                    "Matrix3".into(),
                    "Rotation".into(),
                    false,
                    false,
                    false,
                    Matrix3::new(trans.rotation).to_dict(),
                ));
                found_rotation_prop = true;
            }
            if !found_trans_prop {
                comp.properties.push(Property::new(
                    "Point3".into(),
                    "Translation".into(),
                    false,
                    false,
                    false,
                    Point3::new(
                        trans.translation[0],
                        trans.translation[1],
                        trans.translation[2],
                    )
                    .to_dict(),
                ));
                found_trans_prop = true;
            }

            if !found_scale_prop {
                if let Some(num) = serde_json::Number::from_f64(scale_float as f64) {
                    comp.properties.push(Property::new(
                        "Float".into(),
                        "Scale".into(),
                        false,
                        false,
                        false,
                        serde_json::Value::Number(num),
                    ));
                    found_scale_prop = true;
                }
            }
        }
    }
}

pub(crate) fn parse_point3(val: &serde_json::Value) -> Option<[f32; 3]> {
    let obj = val.as_object()?;
    let x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let z = obj.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    Some([x, y, z])
}

pub(crate) fn parse_matrix3(val: &serde_json::Value) -> Option<[[f32; 3]; 3]> {
    if let Some(obj) = val.as_object() {
        if let Some(m_val) = obj.get("m") {
            if let Some(arr) = m_val.as_array() {
                if arr.len() == 3 {
                    let mut mat = [[0.0f32; 3]; 3];
                    let mut valid = true;
                    for i in 0..3 {
                        if let Some(row) = arr[i].as_array() {
                            if row.len() == 3 {
                                for j in 0..3 {
                                    mat[i][j] = row[j].as_f64().unwrap_or(0.0) as f32;
                                }
                            } else {
                                valid = false;
                            }
                        } else {
                            valid = false;
                        }
                    }
                    if valid {
                        return Some(mat);
                    }
                } else if arr.len() == 9 {
                    return Some([
                        [
                            arr[0].as_f64().unwrap_or(0.0) as f32,
                            arr[1].as_f64().unwrap_or(0.0) as f32,
                            arr[2].as_f64().unwrap_or(0.0) as f32,
                        ],
                        [
                            arr[3].as_f64().unwrap_or(0.0) as f32,
                            arr[4].as_f64().unwrap_or(0.0) as f32,
                            arr[5].as_f64().unwrap_or(0.0) as f32,
                        ],
                        [
                            arr[6].as_f64().unwrap_or(0.0) as f32,
                            arr[7].as_f64().unwrap_or(0.0) as f32,
                            arr[8].as_f64().unwrap_or(0.0) as f32,
                        ],
                    ]);
                }
            }
        }
        if obj.contains_key("m00") {
            let g =
                |k: &str, d: f32| obj.get(k).and_then(|v| v.as_f64()).unwrap_or(d as f64) as f32;
            return Some([
                [g("m00", 1.0), g("m01", 0.0), g("m02", 0.0)],
                [g("m10", 0.0), g("m11", 1.0), g("m12", 0.0)],
                [g("m20", 0.0), g("m21", 0.0), g("m22", 1.0)],
            ]);
        }
    }
    None
}

pub struct CachedNifMesh {
    pub(crate) source_attributes:
        Option<Result<std::sync::Arc<crate::nif_vertex_attributes::Source>, String>>,
    pub extra_uvs:
        Option<std::sync::Arc<std::collections::BTreeMap<u32, std::sync::Arc<Vec<[f32; 2]>>>>>,
    pub mesh_index: usize,
    pub source_indices: Option<Vec<u32>>,
    pub mesh_to_model: [[f32; 4]; 4],
    pub hierarchy_path: Option<Vec<u32>>,
    pub colour_indices: Option<Vec<u32>>,
    pub colour_count: Option<usize>,
    pub uv_transform: Option<crate::nif_textures::UvTransform>,
    pub material: std::sync::Arc<super::nif::MaterialBindings>,
    pub sampler: super::viewport3d::TextureSampler,
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    pub indices: std::sync::Arc<Vec<u32>>,
    pub texture_name: Option<String>,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
    pub v_data: std::sync::Arc<Vec<f32>>,
}

// ─────────────────────────────────────────────────────────────────────────────
/// Ancestors (deepest first) -> Direct Master -> Entity itself.
/// Properties defined closer to `entity` override properties from further up the chain.
pub(crate) fn get_effective_components(entity: &Entity, all_objects: &[Entity]) -> Vec<Component> {
    let chain = resolve_master_chain(entity, all_objects);
    let mut effective: Vec<Component> = Vec::new();
    let mut ancestry: Vec<HashSet<u32>> = Vec::new();
    for owner in chain.iter().rev() {
        for component in &owner.components {
            let id = component.link_id.to_u32();
            let parent = component.master_link_id.to_u32();
            let slot = ancestry.iter().position(|ids| {
                (id != 0 && ids.contains(&id)) || (parent != 0 && ids.contains(&parent))
            });
            if let Some(slot) = slot {
                let mut properties = effective[slot].properties.clone();
                let mut has_parent_override = false;
                for property in &component.properties {
                    // Placed prefab transforms can carry a second Parent Entity
                    // row after their authored parent. Keep the first row, as
                    // direct transform lookup does for an unmerged component.
                    if component.class_name == "JPSTransformationComponent"
                        && property.name == "Parent Entity"
                        && std::mem::replace(&mut has_parent_override, true)
                    {
                        continue;
                    }
                    if let Some(index) = properties.iter().position(|p| p.name == property.name) {
                        // The override carries its own type and asset flags, not only a value.
                        properties[index] = property.clone();
                    } else {
                        properties.push(property.clone());
                    }
                }
                effective[slot] = component.clone();
                effective[slot].properties = properties;
                ancestry[slot].insert(id);
            } else {
                effective.push(component.clone());
                ancestry.push(HashSet::from([id]));
            }
        }
    }
    effective
}
fn find_entity_nif_path(entity: &Entity) -> Option<String> {
    for comp in &entity.components {
        for prop in &comp.properties {
            let nm_lower = prop.name.to_lowercase();
            if nm_lower.contains("nif") && nm_lower.contains("path") {
                if let Some(s) = prop.value.as_str() {
                    let clean = s.trim_matches(&['"', '\'', ' '][..]);
                    if !clean.is_empty() {
                        return Some(clean.to_string());
                    }
                }
            }
        }
        for prop in &comp.properties {
            if let Some(s) = prop.value.as_str() {
                let clean = s.trim_matches(&['"', '\'', ' '][..]).to_lowercase();
                if clean.ends_with(".nif") || clean.ends_with(".nif_wii") {
                    return Some(s.trim_matches(&['"', '\'', ' '][..]).to_string());
                }
            }
        }
    }
    None
}

/// Like find_entity_nif_path but recursively falls back through the entire master chain.
pub(crate) fn find_entity_nif_path_with_master(
    entity: &Entity,
    all_objects: &[Entity],
) -> Option<String> {
    for e in resolve_master_chain(entity, all_objects) {
        if let Some(path) = find_entity_nif_path(e) {
            return Some(path);
        }
    }
    None
}

/// Returns [entity, direct_master, grandmaster, …] walking master_link_id recursively.
/// Cycle-safe — stops if the same link_id is seen twice.
pub(crate) fn resolve_master_chain<'a>(
    entity: &'a Entity,
    all_objects: &'a [Entity],
) -> Vec<&'a Entity> {
    let mut chain = vec![entity];
    let mut seen = std::collections::HashSet::new();
    seen.insert(entity.link_id.to_u32());
    let mut cur_master_id = entity.master_link_id.to_u32();
    while cur_master_id != 0 {
        if !seen.insert(cur_master_id) {
            break;
        } // cycle guard
        if let Some(master) = all_objects
            .iter()
            .find(|e| e.link_id.to_u32() == cur_master_id)
        {
            chain.push(master);
            cur_master_id = master.master_link_id.to_u32();
        } else {
            break;
        }
    }
    chain
}

/// Search the master chain (nearest first) for a component+property value.
/// Returns value_clone of the first match found in masters.
fn find_master_prop_value(
    prop_name: &str,
    mut component_id: ID,
    chain: &[&Entity],
) -> Option<serde_json::Value> {
    let mut visited = HashSet::new();
    while component_id.to_u32() != 0 && visited.insert(component_id.to_u32()) {
        let component = chain
            .iter()
            .flat_map(|e| &e.components)
            .find(|c| c.link_id == component_id)?;
        if let Some(property) = component.properties.iter().find(|p| p.name == prop_name) {
            return Some(property.value.clone());
        }
        component_id = component.master_link_id;
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Asset loading (NIF + texture)
// ─────────────────────────────────────────────────────────────────────────────

fn load_nif_bytes(nif_path: &str, pak_files: &[VirtualFile]) -> Option<Vec<u8>> {
    crate::wii_mesh::resolve_model(pak_files, nif_path).map(|f| f.data.clone())
}

pub(crate) fn ensure_nif_cached(
    nif_path: &str,
    pak_files: &[VirtualFile],
    loaded_nifs: &mut HashMap<String, Vec<CachedNifMesh>>,
) -> String {
    let clean_path = nif_path
        .trim_matches(&['/', '\\', '"', ' '][..])
        .replace('\\', "/");

    if !loaded_nifs.contains_key(&clean_path) {
        let mut meshes = Vec::new();
        if let Some(data) = load_nif_bytes(&clean_path, pak_files) {
            if let Ok(nif) = NifFile::load_bytes(&data) {
                if let Ok(scene) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::nif::build_cached_scene(&nif)
                })) {
                    for mesh in scene.submeshes {
                        let mut mn = [f32::INFINITY; 3];
                        let mut mx = [f32::NEG_INFINITY; 3];
                        for p in &mesh.positions {
                            for axis in 0..3 {
                                mn[axis] = mn[axis].min(p[axis]);
                                mx[axis] = mx[axis].max(p[axis]);
                            }
                        }
                        let normals = Some(
                            mesh.v_data
                                .chunks_exact(12)
                                .map(|v| [v[3], v[4], v[5]])
                                .collect(),
                        );
                        meshes.push(CachedNifMesh {
                            source_attributes: mesh.source_attributes,
                            extra_uvs: mesh.extra_uvs,
                            mesh_index: mesh.mesh_index,
                            source_indices: mesh.source_indices,
                            mesh_to_model: mesh.mesh_to_model,
                            hierarchy_path: mesh.hierarchy_path,
                            colour_indices: mesh.colour_indices,
                            colour_count: mesh.colour_count,
                            uv_transform: mesh.uv_transform,
                            material: mesh.material,
                            sampler: mesh.sampler,
                            positions: mesh.positions,
                            normals,
                            uvs: mesh.uvs,
                            indices: mesh.indices,
                            texture_name: mesh.texture_name,
                            aabb_min: mn,
                            aabb_max: mx,
                            v_data: mesh.v_data,
                        });
                    }
                }
            }
        }
        loaded_nifs.insert(clean_path.clone(), meshes);
    }
    clean_path
}

// ─────────────────────────────────────────────────────────────────────────────
/// Shared cooked transforms for the native runtime; uses the same matrix convention as rendering.
pub(crate) fn runtime_matrix(entity: &Entity, objects: &[Entity]) -> [f32; 16] {
    Mat4::from_transform3d(&get_entity_transform(entity, objects)).to_flat_array_column_major()
}
pub(crate) fn runtime_lights(scene: &SceneFile) -> Vec<super::viewport3d::GpuLight> {
    runtime_lights_filtered(scene, |_| true)
}
pub(crate) fn runtime_lights_filtered(
    scene: &SceneFile,
    include: impl Fn(&Entity) -> bool,
) -> Vec<super::viewport3d::GpuLight> {
    extract_scene_lights_filtered(
        scene,
        &compute_scene_entity_world_transforms(scene),
        |entity| scene.scene.contains(&entity.link_id) && include(entity),
    )
}
