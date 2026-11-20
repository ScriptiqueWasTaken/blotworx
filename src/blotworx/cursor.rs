//! Recovered Cursor3D geometry and camera unprojection. Collision filtering and
//! action scheduling are not connected yet; this is not a playable aim cursor.
use super::physics::V;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: V,
    pub direction: V,
}
/// Eligibility only; geometric rejection and target-handle assignment happen later.
/// Names describe the branch, not an unverified material/component classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitEligibility {
    Reject,
    Flagged,
    Ordinary,
}

/// REL 0x732a4..0x73380. Keep the callback lazy: flagged hits and exact
/// zero-fraction hits never invoke the original virtual filter at slot 0x28.
pub fn hit_eligibility(
    fraction: f32,
    collision_filter: u32,
    reject_ordinary: bool,
    rejection_callback: impl FnOnce() -> u32,
) -> HitEligibility {
    if fraction == 0. {
        return HitEligibility::Reject;
    }
    if collision_filter & 0x18000 != 0 {
        return HitEligibility::Flagged;
    }
    if reject_ordinary || rejection_callback() >> 24 != 0 {
        return HitEligibility::Reject;
    }
    HitEligibility::Ordinary
}

/// Havok ray records contain exactly eight shape-key slots. The cursor asks for
/// the last key before the sentinel only when the root exposes a collection.
pub fn filter_shape_key(has_collection: bool, keys: &[u32; 8]) -> u32 {
    if !has_collection {
        return u32::MAX;
    }
    keys.iter()
        .copied()
        .take_while(|key| *key != u32::MAX)
        .last()
        .unwrap_or(u32::MAX)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryDecision {
    Accept,
    Skip,
    Stop,
}

/// Only supply this when the optional ray-direction pointer and active camera
/// exist. The original tests pointer presence, not the direction's contents.
/// threshold is Cursor_Detection_AimBehind_MinDistance (registered default 0),
/// not a guessed camera clipping distance.
#[derive(Clone, Copy)]
pub struct NearCamera {
    pub position: V,
    pub threshold: f32,
}

/// Native collector record after conversion by REL 0x23d1a0.
pub struct CursorHit<C> {
    pub fraction: f32,
    pub normal: V,
    pub collidable: C,
    pub shape_keys: [u32; 8],
}
pub struct TargetState<H> {
    pub hit: bool,
    pub hit_normal: V,
    pub entity: Option<H>,
}

/// World operations must report missing collision/component support as errors.
/// candidate_target implements 0x242ebc + 0x73670: only entities with
/// TargetableComponent (BSS 0x31b48, registration 0x3c7610) qualify.
pub trait TargetWorld<C, H> {
    fn collision_filter(&mut self, hit: &CursorHit<C>) -> anyhow::Result<u32>;
    fn rejection(&mut self, collidable: &C) -> anyhow::Result<u32>;
    fn is_live(&mut self, handle: &H) -> anyhow::Result<bool>;
    fn candidate_target(&mut self, collidable: &C) -> anyhow::Result<Option<H>>;
}
pub struct RaySelection {
    pub start: V,
    pub end: V,
    pub reject_ordinary: bool,
    pub use_hit_normal: bool,
    pub owner: Option<(V, f32)>,
    pub near_camera: Option<NearCamera>,
}

/// Selection after the world query, REL 0x73250..0x73624. The caller supplies
/// the ORIGINAL collector order, including tie ordering; this does not sort or
/// truncate hits. Hit normal/output normal remain unchanged unless accepted.
pub fn select_ray_hits<C, H>(
    hits: &[CursorHit<C>],
    query: &RaySelection,
    world: &mut impl TargetWorld<C, H>,
    state: &mut TargetState<H>,
    point: &mut V,
    normal: &mut V,
) -> anyhow::Result<bool> {
    state.hit = false;
    state.entity = None;
    for hit in hits {
        // Native zero-fraction branch precedes even the collision-filter lookup.
        if hit.fraction == 0. {
            continue;
        }
        let filter = world.collision_filter(hit)?;
        let eligibility = hit_eligibility(hit.fraction, filter, query.reject_ordinary, || 0);
        if eligibility == HitEligibility::Reject {
            continue;
        }
        let ordinary = eligibility == HitEligibility::Ordinary;
        if ordinary && world.rejection(&hit.collidable)? >> 24 != 0 {
            continue;
        }
        let live = match &state.entity {
            Some(handle) => world.is_live(handle)?,
            None => false,
        };
        if !live {
            if let Some(target) = world.candidate_target(&hit.collidable)? {
                state.entity = Some(target);
            }
        }
        // Target assignment intentionally precedes rejection: a skipped hit may
        // own the target handle even when a later hit supplies the output point.
        let (candidate, decision) = hit_geometry(
            query.start,
            query.end,
            hit.fraction,
            ordinary,
            query.owner,
            query.near_camera,
        );
        *point = candidate;
        match decision {
            GeometryDecision::Stop => return Ok(false),
            GeometryDecision::Skip => continue,
            GeometryDecision::Accept => {
                state.hit_normal = hit.normal;
                state.hit = true;
                *normal = if query.use_hit_normal {
                    hit.normal
                } else {
                    [0., 1., 0.]
                };
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// REL 0x733f0..0x735a4, after eligibility and target-handle assignment.
/// Return the interpolated point even on rejection: the original writes it
/// before these checks. Stop terminates the whole search; Skip tries the next hit.
pub fn hit_geometry(
    start: V,
    end: V,
    fraction: f32,
    ordinary: bool,
    owner: Option<(V, f32)>,
    near_camera: Option<NearCamera>,
) -> (V, GeometryDecision) {
    let point = std::array::from_fn(|i| fraction.mul_add(end[i], (1. - fraction) * start[i]));
    let Some((owner, range)) = owner else {
        return (point, GeometryDecision::Accept);
    };
    let subtract = |a: V, b: V| std::array::from_fn::<_, 3, _>(|i| a[i] - b[i]);
    let squared = |v: V| v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    let from_owner = subtract(point, owner);
    if squared(from_owner) > range * range {
        return (point, GeometryDecision::Stop);
    }
    if ordinary {
        if let Some(camera) = near_camera {
            let camera_from_owner = subtract(camera.position, owner);
            let point_from_camera = subtract(point, camera.position);
            let threshold_squared = camera.threshold * camera.threshold;
            // Native bge instructions bypass the check at equality.
            if !(squared(camera_from_owner) >= threshold_squared
                || squared(point_from_camera) >= threshold_squared)
            {
                let flatten =
                    |v: V| super::brush::target_direction([0.; 3], [v[0], 0., v[2]]).direction;
                let a = flatten(camera_from_owner);
                let b = flatten(from_owner);
                let dot = a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
                if dot >= 0. {
                    return (point, GeometryDecision::Skip);
                }
            }
        }
    }
    (point, GeometryDecision::Accept)
}
#[derive(Clone, Copy)]
pub struct Rect {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}
pub struct Camera {
    pub position: V,
    /// Gamebryo world rotation column zero is the negative screen-right vector.
    pub left: V,
    pub up: V,
    pub forward: V,
    pub viewport: Rect,
    pub frustum: Rect,
    pub orthographic: bool,
}
fn sdk_unitize(vector: V) -> V {
    let squared = vector[2].mul_add(vector[2], vector[0] * vector[0]) + vector[1] * vector[1];
    if squared == 0. {
        return [0.; 3];
    }
    let estimate = super::pose_blend::estimate_double(squared);
    let square = super::pose_blend::square_estimate(estimate);
    let half = (estimate * 0.5) as f32;
    let factor = (-square).mul_add(squared, 3.) * half;
    if squared * factor > f32::from_bits(0x358637bd) {
        vector.map(|v| v * factor)
    } else {
        [0.; 3]
    }
}
impl Camera {
    /// DOL 0x8048e7cc -> 0x8044cc68 -> 0x8044cd60. Pixel coordinates are
    /// cast to unsigned by the original renderer and Y is flipped into buffer space.
    /// Exact viewport edges are accepted; absent/zero-sized targets return None.
    pub fn window_ray(&self, pixel: [i32; 2], size: [u32; 2]) -> anyhow::Result<Option<Ray>> {
        if size.contains(&0) {
            return Ok(None);
        }
        let p = self.viewport;
        let f = self.frustum;
        anyhow::ensure!(
            self.position
                .iter()
                .chain(&self.left)
                .chain(&self.up)
                .chain(&self.forward)
                .chain([p.left, p.right, p.top, p.bottom, f.left, f.right, f.top, f.bottom].iter())
                .all(|x| x.is_finite())
                && p.right > p.left
                && p.top > p.bottom,
            "Invalid cursor camera parameters"
        );
        let bx = pixel[0] as u32 as f32 / size[0] as f32;
        let by = 1. - pixel[1] as u32 as f32 / size[1] as f32;
        if bx > p.right || bx < p.left || by > p.top || by < p.bottom {
            return Ok(None);
        }
        let px = (bx - p.left) / (p.right - p.left);
        let py = (by - p.bottom) / (p.top - p.bottom);
        let vx = px.mul_add(f.right - f.left, f.left);
        let vy = py.mul_add(f.top - f.bottom, f.bottom);
        // DOL calls separate vector scale/add routines, not fused multiply-add.
        let right = self.left.map(|v| (v * -1.) * vx);
        let up = self.up.map(|v| v * vy);
        let ray = if self.orthographic {
            Ray {
                origin: std::array::from_fn(|i| (self.position[i] + right[i]) + up[i]),
                direction: sdk_unitize(self.forward),
            }
        } else {
            Ray {
                origin: self.position,
                direction: sdk_unitize(std::array::from_fn(|i| {
                    (self.forward[i] + right[i]) + up[i]
                })),
            }
        };
        anyhow::ensure!(
            ray.origin
                .iter()
                .chain(&ray.direction)
                .all(|v| v.is_finite()),
            "Cursor unprojection overflow"
        );
        Ok(Some(ray))
    }
}

/// DOL 0x800071f0. Preserve the original frsqrte + single Newton step and fsel.
/// Negative inputs select the original input after the NaN refinement, then
/// multiply by it. Replacing this with host sqrt changes native edge behavior.
pub(super) fn native_sqrt(value: f32) -> f32 {
    if !(value > 0.) || !value.is_finite() {
        return value * value;
    }
    let estimate = super::pose_blend::estimate_double(value);
    //80007208 fmuls retains the double A operand but rounds the C operand
    //to25 significant bits before multiplication. Narrowing the final square
    //alone changes the native Newton refinement (and subsequent angle/pose math).
    let bits = estimate.to_bits();
    let operand = f64::from_bits(((bits >> 28) + ((bits >> 27) & 1)) << 28);
    let squared = (estimate * operand) as f32;
    let half = (estimate * 0.5) as f32;
    let refined = (-squared).mul_add(value, 3.) * half;
    value * if refined >= 0. { refined } else { value }
}

#[derive(Debug, PartialEq)]
pub enum Intersections {
    None,
    One(V),
    Two(V, V),
}

/// REL 0x147e90..0x148068: returns only forward intersections. The two-hit
/// order is near then far; Cursor3D deliberately picks the far exit point.
/// Negative discriminants are NOT silently corrected; native_sqrt preserves
/// the original helper's unusual negative-input behavior.
pub fn sphere_intersections(center: V, radius: f32, origin: V, direction: V) -> Intersections {
    let mut d = direction;
    let norm = super::brush::target_direction([0.; 3], direction);
    // Same length helper and strict normalization threshold as the original.
    if norm.distance > 0.0001 {
        d = norm.direction;
    }
    let delta = std::array::from_fn::<_, 3, _>(|i| origin[i] - center[i]);
    let squared = delta[2].mul_add(delta[2], delta[0].mul_add(delta[0], delta[1] * delta[1]));
    let projection = d[2].mul_add(delta[2], d[0].mul_add(delta[0], d[1] * delta[1]));
    let constant = (-radius).mul_add(radius, squared);
    let b = 2. * projection;
    let discriminant = b.mul_add(b, -(4. * constant));
    let root = native_sqrt(discriminant);
    let near = (-b - root) * 0.5;
    let far = (-b + root) * 0.5;
    let point = |t: f32| std::array::from_fn(|i| d[i].mul_add(t, origin[i]));
    if near > 0. && far > 0. {
        Intersections::Two(point(near), point(far))
    } else if far > 0. {
        Intersections::One(point(far))
    } else {
        Intersections::None
    }
}

/// REL 0x73c54..0x73ce8. This is the preferred endpoint passed into collision
/// queries, not a final target until hit selection and fallback queries finish.
pub fn preferred_endpoint(center: V, radius: f32, origin: V, direction: V) -> V {
    let intersections = sphere_intersections(center, radius, origin, direction);
    match intersections {
        Intersections::One(point) | Intersections::Two(_, point) => return point,
        Intersections::None => {}
    }
    // REL 0xb128 computes the distance between the two positions.
    let delta = std::array::from_fn::<_, 3, _>(|i| center[i] - origin[i]);
    let squared = delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1];
    // 0xb128 uses a different length helper: frsp immediately after frsqrte.
    let distance = if squared == 0. {
        0.
    } else {
        let estimate = super::pose_blend::estimate(squared);
        let correction = (-squared).mul_add(estimate * estimate, 1.);
        squared * correction.mul_add(0.5 * estimate, estimate)
    };
    std::array::from_fn(|i| direction[i].mul_add(distance, origin[i]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct TestWorld {
        calls: Vec<(u8, u32)>,
        flagged: bool,
    }
    impl TargetWorld<u32, u32> for TestWorld {
        fn collision_filter(&mut self, h: &CursorHit<u32>) -> anyhow::Result<u32> {
            self.calls.push((0, h.collidable));
            Ok(if self.flagged { 0x8000 } else { 0 })
        }
        fn rejection(&mut self, c: &u32) -> anyhow::Result<u32> {
            self.calls.push((1, *c));
            Ok(0)
        }
        fn is_live(&mut self, h: &u32) -> anyhow::Result<bool> {
            self.calls.push((2, *h));
            Ok(true)
        }
        fn candidate_target(&mut self, c: &u32) -> anyhow::Result<Option<u32>> {
            self.calls.push((3, *c));
            Ok(Some(*c))
        }
    }
    fn record(fraction: f32, id: u32) -> CursorHit<u32> {
        CursorHit {
            fraction,
            normal: [1., 0., 0.],
            collidable: id,
            shape_keys: [u32::MAX; 8],
        }
    }
    fn selection() -> RaySelection {
        RaySelection {
            start: [2., 0., 0.],
            end: [-2., 0., 0.],
            reject_ordinary: false,
            use_hit_normal: false,
            owner: Some(([0.; 3], 10.)),
            near_camera: Some(NearCamera {
                position: [2., 0., 0.],
                threshold: 4.,
            }),
        }
    }
    #[test]
    fn selector_retains_target_from_skipped_hit_and_accepts_later_point() -> anyhow::Result<()> {
        let mut world = TestWorld::default();
        let mut state = TargetState {
            hit: true,
            hit_normal: [9.; 3],
            entity: Some(99),
        };
        let mut point = [9.; 3];
        let mut normal = [9.; 3];
        assert!(select_ray_hits(
            &[record(0., 10), record(0.25, 11), record(0.75, 12)],
            &selection(),
            &mut world,
            &mut state,
            &mut point,
            &mut normal
        )?);
        assert_eq!(point, [-1., 0., 0.]);
        assert_eq!(normal, [0., 1., 0.]);
        assert!(state.hit);
        assert_eq!(state.hit_normal, [1., 0., 0.]);
        assert_eq!(state.entity, Some(11));
        assert_eq!(
            world.calls,
            vec![(0, 11), (1, 11), (3, 11), (0, 12), (1, 12), (2, 11)]
        );
        Ok(())
    }
    #[test]
    fn selector_stop_preserves_normal_and_target_but_does_not_visit_next_hit() -> anyhow::Result<()>
    {
        let mut world = TestWorld::default();
        let mut query = selection();
        query.owner = Some(([0.; 3], 0.5));
        let mut state = TargetState {
            hit: true,
            hit_normal: [8.; 3],
            entity: Some(99),
        };
        let mut point = [9.; 3];
        let mut normal = [7.; 3];
        assert!(!select_ray_hits(
            &[record(0.25, 11), record(0.5, 12)],
            &query,
            &mut world,
            &mut state,
            &mut point,
            &mut normal
        )?);
        assert_eq!(point, [1., 0., 0.]);
        assert_eq!(normal, [7.; 3]);
        assert_eq!(state.hit_normal, [8.; 3]);
        assert!(!state.hit);
        assert_eq!(state.entity, Some(11));
        assert_eq!(world.calls, vec![(0, 11), (1, 11), (3, 11)]);
        assert!(!select_ray_hits(
            &[],
            &query,
            &mut world,
            &mut state,
            &mut point,
            &mut normal
        )?);
        assert_eq!(state.entity, None);
        assert_eq!(point, [1., 0., 0.]);
        Ok(())
    }
    #[test]
    fn selector_flagged_hit_bypasses_filter_callback_and_outputs_surface_normal(
    ) -> anyhow::Result<()> {
        let mut world = TestWorld {
            flagged: true,
            ..Default::default()
        };
        let mut query = selection();
        query.reject_ordinary = true;
        query.use_hit_normal = true;
        let mut state = TargetState {
            hit: false,
            hit_normal: [0.; 3],
            entity: None,
        };
        let mut point = [0.; 3];
        let mut normal = [0.; 3];
        assert!(select_ray_hits(
            &[record(0.25, 11)],
            &query,
            &mut world,
            &mut state,
            &mut point,
            &mut normal
        )?);
        assert_eq!(normal, [1., 0., 0.]);
        assert_eq!(world.calls, vec![(0, 11), (3, 11)]);
        Ok(())
    }
    #[test]
    fn range_rejection_stops_search_and_preserves_written_point() {
        let point = [4., 0., 0.];
        assert_eq!(
            hit_geometry([0.; 3], [8., 0., 0.], 0.5, false, Some(([0.; 3], 3.)), None),
            (point, GeometryDecision::Stop)
        );
        assert_eq!(
            hit_geometry([0.; 3], [8., 0., 0.], 0.5, false, Some(([0.; 3], 4.)), None),
            (point, GeometryDecision::Accept)
        );
        assert_eq!(
            hit_geometry(
                [0.; 3],
                [8., 0., 0.],
                0.5,
                true,
                None,
                Some(NearCamera {
                    position: [2., 0., 0.],
                    threshold: 10.
                })
            )
            .1,
            GeometryDecision::Accept
        );
    }
    #[test]
    fn close_camera_rejection_only_applies_to_ordinary_hits_on_camera_side() {
        let evaluate = |point: V, ordinary, threshold, camera: V| {
            hit_geometry(
                point,
                point,
                0.5,
                ordinary,
                Some(([0.; 3], 10.)),
                Some(NearCamera {
                    position: camera,
                    threshold,
                }),
            )
            .1
        };
        assert_eq!(
            evaluate([1., 0., 0.], true, 3., [2., 0., 0.]),
            GeometryDecision::Skip
        );
        assert_eq!(
            evaluate([1., 0., 0.], false, 3., [2., 0., 0.]),
            GeometryDecision::Accept
        );
        assert_eq!(
            evaluate([-0.5, 0., 0.], true, 3., [2., 0., 0.]),
            GeometryDecision::Accept
        );
        // Camera-owner and point-camera boundaries both bypass rejection.
        assert_eq!(
            evaluate([1., 0., 0.], true, 2., [2., 0., 0.]),
            GeometryDecision::Accept
        );
        assert_eq!(
            evaluate([5., 0., 0.], true, 3., [2., 0., 0.]),
            GeometryDecision::Accept
        );
        // Vertical separation matters for proximity, but yaw comparison is flat.
        assert_eq!(
            evaluate([1., 4., 0.], true, 3., [2., 0., 0.]),
            GeometryDecision::Accept
        );
        assert_eq!(
            evaluate([0., 1., 0.], true, 3., [2., 0., 0.]),
            GeometryDecision::Skip
        );
    }
    #[test]
    fn hit_filter_preserves_callback_order_and_high_byte_semantics() {
        let forbidden = || -> u32 {
            panic!("Original branch must not call filter");
        };
        for zero in [0., -0.] {
            assert_eq!(
                hit_eligibility(zero, 0x18000, false, forbidden),
                HitEligibility::Reject
            );
        }
        for bit in [0x8000, 0x10000, 0x18000] {
            assert_eq!(
                hit_eligibility(0.5, bit, true, forbidden),
                HitEligibility::Flagged
            );
        }
        assert_eq!(
            hit_eligibility(0.5, 0, true, forbidden),
            HitEligibility::Reject
        );
        assert_eq!(
            hit_eligibility(0.5, 0, false, || 0x00ffffff),
            HitEligibility::Ordinary
        );
        for result in [0x01000000, 0x80000000, 0xff000000] {
            assert_eq!(
                hit_eligibility(0.5, 0, false, || result),
                HitEligibility::Reject
            );
        }
        let mut calls = 0;
        assert_eq!(
            hit_eligibility(0.5, 0, false, || {
                calls += 1;
                0
            }),
            HitEligibility::Ordinary
        );
        assert_eq!(calls, 1);
    }
    #[test]
    fn compound_filter_uses_last_key_before_sentinel() {
        let keys = [3, 7, u32::MAX, 9, 10, 11, 12, 13];
        assert_eq!(filter_shape_key(false, &keys), u32::MAX);
        assert_eq!(filter_shape_key(true, &keys), 7);
        assert_eq!(filter_shape_key(true, &[u32::MAX; 8]), u32::MAX);
        assert_eq!(filter_shape_key(true, &[0, 1, 2, 3, 4, 5, 6, 7]), 7);
    }
    fn camera() -> Camera {
        Camera {
            position: [10., 20., 30.],
            left: [-1., 0., 0.],
            up: [0., 1., 0.],
            forward: [0., 0., 1.],
            viewport: Rect {
                left: 0.,
                right: 1.,
                top: 1.,
                bottom: 0.,
            },
            frustum: Rect {
                left: -1.,
                right: 1.,
                top: 1.,
                bottom: -1.,
            },
            orthographic: false,
        }
    }
    #[test]
    fn window_ray_preserves_viewport_edges_and_y_flip() -> anyhow::Result<()> {
        let mut c = camera();
        let center = c.window_ray([320, 240], [640, 480])?.unwrap();
        assert_eq!(center.origin, c.position);
        assert!((center.direction[2] - 1.).abs() < 1e-6);
        let top = c.window_ray([0, 0], [640, 480])?.unwrap();
        assert!(top.direction[0] < 0. && top.direction[1] > 0.);
        assert!(c.window_ray([640, 480], [640, 480])?.is_some());
        assert!(c.window_ray([-1, 0], [640, 480])?.is_none());
        assert!(c.window_ray([641, 0], [640, 480])?.is_none());
        assert!(c.window_ray([0, 0], [0, 480])?.is_none());
        c.viewport = Rect {
            left: 0.5,
            right: 1.,
            top: 1.,
            bottom: 0.,
        };
        assert!(c.window_ray([319, 240], [640, 480])?.is_none());
        assert!(c.window_ray([480, 240], [640, 480])?.unwrap().direction[0].abs() < 1e-6);
        c.orthographic = true;
        let top = c.window_ray([320, 0], [640, 480])?.unwrap();
        assert_eq!(top.origin, [9., 21., 30.]);
        assert!((top.direction[2] - 1.).abs() < 1e-6);
        Ok(())
    }
    #[test]
    fn cursor_uses_far_sphere_exit_not_nearest_intersection() {
        let intersections = sphere_intersections([0.; 3], 2., [0., 0., -5.], [0., 0., 1.]);
        let Intersections::Two(near, far) = intersections else {
            panic!("Expected entry and exit");
        };
        assert!((near[2] + 2.).abs() < 1e-5);
        assert!((far[2] - 2.).abs() < 1e-5);
        assert_eq!(
            preferred_endpoint([0.; 3], 2., [0., 0., -5.], [0., 0., 1.]),
            far
        );
        let Intersections::One(exit) = sphere_intersections([0.; 3], 2., [0.; 3], [0., 0., 1.])
        else {
            panic!("Expected only exit");
        };
        assert!((exit[2] - 2.).abs() < 1e-5);
        assert_eq!(
            sphere_intersections([0.; 3], 2., [0., 0., 5.], [0., 0., 1.]),
            Intersections::None
        );
    }
    #[test]
    fn behind_sphere_falls_back_to_camera_player_distance() {
        let point = preferred_endpoint([0.; 3], 2., [0., 0., 5.], [0., 0., 1.]);
        assert!((point[2] - 10.).abs() < 1e-5);
        // Zero direction is retained; no fabricated forward ray.
        assert_eq!(
            preferred_endpoint([0.; 3], 2., [0., 0., 5.], [0.; 3]),
            [0., 0., 5.]
        );
    }
    #[test]
    fn unprojection_normalization_matches_original_sdk_fixture() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/sdk_vector.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1030);
        for case in cases {
            let vector = std::array::from_fn(|i| case["vector"][i].as_f64().unwrap() as f32);
            let length = f32::from_bits(case["length_bits"].as_u64().unwrap() as u32);
            let expected = if length > f32::from_bits(0x358637bd) {
                std::array::from_fn(|i| case["normalized_bits"][i].as_u64().unwrap() as u32)
            } else {
                [0; 3]
            };
            let camera = Camera {
                position: [0.; 3],
                left: [1., 0., 0.],
                up: [0., 1., 0.],
                forward: vector,
                viewport: Rect {
                    left: 0.,
                    right: 1.,
                    top: 1.,
                    bottom: 0.,
                },
                frustum: Rect {
                    left: -1.,
                    right: 1.,
                    top: 1.,
                    bottom: -1.,
                },
                orthographic: true,
            };
            assert_eq!(
                camera
                    .window_ray([320, 240], [640, 480])?
                    .unwrap()
                    .direction
                    .map(f32::to_bits),
                expected
            );
        }
        Ok(())
    }
    #[test]
    fn sqrt_matches_original_instruction_fixture() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/sqrt_float.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2061);
        for row in cases {
            let input = row[0].as_u64().unwrap() as u32;
            let expected = row[1].as_u64().unwrap() as u32;
            assert_eq!(
                native_sqrt(f32::from_bits(input)).to_bits(),
                expected,
                "input {input:08x}"
            );
        }
        Ok(())
    }
    #[test]
    fn sqrt_preserves_native_negative_and_zero_behavior() {
        assert_eq!(native_sqrt(-2.), 4.);
        assert_eq!(native_sqrt(-0.).to_bits(), 0);
        assert!((native_sqrt(9.) - 3.).abs() < 1e-6);
    }
}
