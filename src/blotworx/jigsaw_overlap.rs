//! Recovered paint-restoration rejection policy (REL 0x37dd68/0x243af0).
//! The contact phase is connected to live projectiles through jigsaw_collision.
//! Native AABB/ray behavior and pair-collector parity remain unverified.

/// Native query temporarily applies this filter to Mickey's body.
pub const PLAYER_QUERY_FILTER: u32 = 0x00602003;
pub const AABB_TOLERANCE: f32 = 0.01;
pub const KEY_SCOPE_PROPERTY: &str = "Allow Nonoverlapping Painting";
pub const ENABLED_SETTING: &str = "DontPaintObjectIfMickeyIsOverlapping";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub lower: [f32; 3],
    pub upper: [f32; 3],
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestorationRay {
    pub from: [f32; 3],
    pub to: [f32; 3],
}

/// REL 0x243bb0..0x243d84. Inputs are the shape getAabb results, already
/// expanded by the native 0.01 tolerance. Touching bounds count as overlapping.
/// The ray starts at Mickey's AABB center and extends upward by the associated
/// body's full AABB height, not Mickey's height or the distance to its top.
/// The 0.5 vector multiplier is provisional until the runtime initializer at
/// DOL 0x8070b150 is verified. This helper is not connected to gameplay yet.
pub fn restoration_ray(body: Bounds, player: Bounds) -> Option<RestorationRay> {
    if !(0..3).all(|i| body.lower[i] <= player.upper[i] && player.lower[i] <= body.upper[i]) {
        return None;
    }
    let from = std::array::from_fn(|i| (player.lower[i] + player.upper[i]) * 0.5);
    let half_height = (body.upper[1] - body.lower[1]) * 0.5;
    let mut to = from;
    to[1] = 2.0_f32.mul_add(half_height, from[1]);
    Some(RestorationRay { from, to })
}

/// REL 0x2442b0..0x244370: oriented triangle-plane test performed on the
/// world-space vertices of the ray's triangle leaf and Mickey's AABB center.
/// Preserve the paired-single subtraction and fused multiply/subtract order;
/// normalizing this normal would change boundary behavior. The final branch
/// is !GT, including equality and unordered values, rather than strictly LT.
pub fn triangle_plane_rejects_restore(vertices: [[f32; 3]; 3], center: [f32; 3]) -> bool {
    let [a, b, c] = vertices;
    let u: [f32; 3] = std::array::from_fn(|i| c[i] - b[i]);
    let v: [f32; 3] = std::array::from_fn(|i| a[i] - b[i]);
    let delta: [f32; 3] = std::array::from_fn(|i| center[i] - a[i]);
    let nx = u[1].mul_add(v[2], -(v[1] * u[2]));
    let ny = -u[0].mul_add(v[2], -(v[0] * u[2]));
    let nz = -u[1].mul_add(v[0], -(v[1] * u[0]));
    let distance = delta[2].mul_add(nz, delta[0] * nx) + delta[1] * ny;
    !(distance > 0.)
}

/// REL 0x37dd9c..0x37ddc4 registers the default string "true".
/// Uses the same supported boolean spellings as the current config loader.
pub fn enabled(settings: &std::collections::BTreeMap<String, String>) -> anyhow::Result<bool> {
    match settings
        .get(ENABLED_SETTING)
        .map(|v| v.trim().to_ascii_lowercase())
    {
        None => Ok(true),
        Some(value) if value == "true" || value == "1" => Ok(true),
        Some(value) if value == "false" || value == "0" => Ok(false),
        Some(value) => anyhow::bail!("Invalid {ENABLED_SETTING}: {value}"),
    }
}

pub fn load_key_scope(
    component: &epic_mickey_lib_rs::scene_file::Component,
) -> anyhow::Result<bool> {
    use anyhow::Context;
    // Apply after prefab/global property resolution. Base constructor
    // 0x13b0d8 (called by 0x125520) initializes this boolean to false.
    match component
        .properties
        .iter()
        .find(|p| p.name == KEY_SCOPE_PROPERTY)
    {
        Some(property) => property
            .value
            .as_bool()
            .context("Invalid Allow Nonoverlapping Painting"),
        None => Ok(false),
    }
}

/// REL 0x3746f0..0x374720 (extended mesh) and 0x37459c..0x3745cc
/// (list shape). The boolean is component byte +0x80, accessed through
/// 0x371ba8/0x3313b0: Allow Nonoverlapping Painting.
/// None means the already-matching filter bypasses the overlap call entirely.
pub fn requested_key(
    current_filter: u32,
    desired_filter: u32,
    hit_key: u32,
    restrict_to_hit: bool,
) -> Option<u32> {
    (current_filter != desired_filter).then_some(if restrict_to_hit { hit_key } else { u32::MAX })
}

/// Only paint (native stimulus type 1) requests this guard. The force path,
/// disabled configuration, and absent associated bodies bypass the query.
pub fn needs_query(stimulus: u32, forced: bool, enabled: bool, bodies: usize) -> bool {
    !forced && enabled && stimulus == 1 && bodies != 0
}

/// REL 0x24409c scans eight native shape-key slots. Preserve its unusual
/// unterminated-stack fallback to the first slot instead of inventing a ninth.
pub fn ray_leaf_key(keys: &[u32; 8]) -> Option<u32> {
    match keys.iter().position(|&key| key == u32::MAX) {
        Some(0) => None,
        Some(end) => Some(keys[end - 1]),
        None => Some(keys[0]),
    }
}

/// Contact acceptance after the native AABB and ray tests. `filter` is the
/// resolved child/material word from 0x24317c, not necessarily the body word.
/// REL 0x243ed0..0x243f24 and 0x24327c: strict penetration and clear bits 15/16.
pub fn contact_rejects_restore(
    requested_key: u32,
    key: u32,
    distance: f32,
    filter: Option<u32>,
) -> bool {
    (requested_key == u32::MAX || requested_key == key)
        && distance < -0.05
        && filter.is_some_and(|word| word & 0x00018000 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ray_bounds_include_touching_and_use_body_height() {
        let body = Bounds {
            lower: [-5., -10., -5.],
            upper: [5., 10., 5.],
        };
        let player = Bounds {
            lower: [4., 8., 4.],
            upper: [6., 12., 6.],
        };
        assert_eq!(
            restoration_ray(body, player),
            Some(RestorationRay {
                from: [5., 10., 5.],
                to: [5., 30., 5.]
            })
        );
        for axis in 0..3 {
            let mut touching = player;
            touching.lower[axis] = body.upper[axis];
            touching.upper[axis] = body.upper[axis] + 2.;
            assert!(restoration_ray(body, touching).is_some());
            touching.lower[axis] += 0.001;
            assert!(restoration_ray(body, touching).is_none());
            let mut below = player;
            below.upper[axis] = body.lower[axis] - 0.001;
            below.lower[axis] = below.upper[axis] - 2.;
            assert!(restoration_ray(body, below).is_none());
        }
        let point = Bounds {
            lower: [0.; 3],
            upper: [0.; 3],
        };
        assert_eq!(
            restoration_ray(point, point),
            Some(RestorationRay {
                from: [0.; 3],
                to: [0.; 3]
            })
        );
    }
    #[test]
    fn ray_triangle_plane_preserves_winding_boundary_and_translation() {
        let triangle = [[0., 0., 0.], [0., 0., 1.], [1., 0., 0.]];
        assert!(triangle_plane_rejects_restore(
            triangle,
            [0.25, -0.01, 0.25]
        ));
        assert!(triangle_plane_rejects_restore(triangle, [0.25, 0., 0.25]));
        assert!(!triangle_plane_rejects_restore(
            triangle,
            [0.25, 0.01, 0.25]
        ));
        let reversed = [triangle[0], triangle[2], triangle[1]];
        assert!(!triangle_plane_rejects_restore(
            reversed,
            [0.25, -0.01, 0.25]
        ));
        let shift = [123., 45., -67.];
        let translated = triangle.map(|v| std::array::from_fn(|i| v[i] + shift[i]));
        assert!(triangle_plane_rejects_restore(
            translated,
            [123.25, 44.99, -66.75]
        ));
        // The native function is only the plane predicate; the preceding
        // shape raycast, not this predicate, establishes triangle containment.
        assert!(triangle_plane_rejects_restore(triangle, [100., -1., 100.]));
        assert!(triangle_plane_rejects_restore([[0.; 3]; 3], [1.; 3]));
    }
    #[test]
    fn global_setting_defaults_on_and_command_line_overrides_config() {
        let mut settings = std::collections::BTreeMap::new();
        assert!(enabled(&settings).unwrap());
        settings.extend(crate::render_support::game_config::runtime_settings(
            "DontPaintObjectIfMickeyIsOverlapping=true",
            false,
        ));
        settings.extend(crate::render_support::game_config::runtime_settings(
            "-Set DontPaintObjectIfMickeyIsOverlapping=false",
            true,
        ));
        assert!(!enabled(&settings).unwrap());
        settings.insert(ENABLED_SETTING.into(), "unknown".into());
        assert!(enabled(&settings).is_err());
    }
    #[test]
    fn receiver_skips_unchanged_filter_and_preserves_key_scope() {
        assert_eq!(
            requested_key(0x16, 0x8016, 0x80000005, true),
            Some(0x80000005)
        );
        assert_eq!(
            requested_key(0x16, 0x8016, 0x80000005, false),
            Some(u32::MAX)
        );
        assert_eq!(requested_key(0x8016, 0x8016, 5, true), None);
        assert_eq!(requested_key(0x8016, 0x8016, 5, false), None);
        assert_eq!(requested_key(0x8002, 0x8016, 5, true), Some(5));
    }
    #[test]
    fn bypasses_do_not_request_physics() {
        assert!(needs_query(1, false, true, 1));
        assert!(!needs_query(0, false, true, 1));
        assert!(!needs_query(2, false, true, 1));
        assert!(!needs_query(1, true, true, 1));
        assert!(!needs_query(1, false, false, 1));
        assert!(!needs_query(1, false, true, 0));
    }
    #[test]
    fn key_stack_preserves_native_termination_and_fallback() {
        let mut keys = [u32::MAX; 8];
        assert_eq!(ray_leaf_key(&keys), None);
        for i in 0..8 {
            keys[i] = 40 + i as u32;
            assert_eq!(
                ray_leaf_key(&keys),
                Some(if i == 7 { 40 } else { 40 + i as u32 })
            );
        }
    }
    #[test]
    fn touching_shallow_wrong_key_and_filtered_contacts_do_not_reject() {
        assert!(contact_rejects_restore(7, 7, -0.051, Some(0)));
        assert!(contact_rejects_restore(u32::MAX, 9, -1., Some(0x602003)));
        for distance in [0., -0.049, -0.05, f32::NAN] {
            assert!(!contact_rejects_restore(7, 7, distance, Some(0)));
        }
        assert!(!contact_rejects_restore(7, 8, -1., Some(0)));
        for filter in [None, Some(0x8000), Some(0x10000), Some(0x18000)] {
            assert!(!contact_rejects_restore(7, 7, -1., filter));
        }
    }
}
