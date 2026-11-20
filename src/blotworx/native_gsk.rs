//! Recovered EM1 GSK cache/witness operations, not a replacement GJK solver.
use anyhow::{ensure, Result};

/// DOL 802ec1f8 writes vertex halfwords at +0, counts at +8/+9, and
/// preserves the metadata bytes at +10/+11 and unused vertex slots.
#[derive(Clone, Debug, PartialEq)]
pub struct Cache(pub [u8; 12]);
/// Native shape virtual calls +44 (decode ID), +40 (support), +34 (sphere
/// count). The support result keeps the original ID separate from float W.
pub trait SupportShape {
    fn vertex(&mut self, id: u16) -> Result<[f32; 4]>;
    fn support(&mut self, direction: [f32; 4]) -> Result<(u16, [f32; 4])>;
    fn sphere_count(&mut self) -> Result<u32>;
    /// Shape virtual+4c. It is not necessarily a support vertex or average.
    fn center(&mut self) -> Result<[f32; 4]> {
        anyhow::bail!("Native shape center is not implemented")
    }
}
/// EM1 triangle vtable 806b88e0: support 8027ea74, ID conversion
/// 8027eba8 and sphere count 8027f758. Extrusion flag and vector are distinct
/// source fields: support uses the vector even though the flag selects GSK init.
pub struct TriangleSupport {
    pub vertices: [[f32; 4]; 3],
    pub extrusion: [f32; 4],
    pub extrusion_flag: u8,
}
impl SupportShape for TriangleSupport {
    /// 8027ed04: sum the three stored vectors in order, then multiply
    /// by constant806b88c8. Extrusion and encoded support IDs are not used.
    fn center(&mut self) -> Result<[f32; 4]> {
        let center = std::array::from_fn(|i| {
            ((self.vertices[0][i] + self.vertices[1][i]) + self.vertices[2][i])
                * f32::from_bits(0x3eaaaaab)
        });
        ensure!(
            center.iter().all(|v| v.is_finite()),
            "Triangle center overflow"
        );
        Ok(center)
    }
    fn vertex(&mut self, id: u16) -> Result<[f32; 4]> {
        ensure!(id < 6, "Invalid native triangle vertex ID");
        let base = self.vertices[id as usize % 3];
        // Original table 806b88b0 = [0,0,0,1,1,1]. Conversion uses FMA
        // even for the base vertices; support below instead uses ps_add.
        let factor = if id < 3 { 0. } else { 1. };
        let mut point = std::array::from_fn(|i| self.extrusion[i].mul_add(factor, base[i]));
        point[3] = f32::from_bits(0x3f000000 | id as u32);
        ensure!(
            point[..3].iter().all(|v| v.is_finite()),
            "Nonfinite native triangle vertex"
        );
        Ok(point)
    }
    fn support(&mut self, d: [f32; 4]) -> Result<(u16, [f32; 4])> {
        ensure!(
            d[..3].iter().all(|v| v.is_finite()),
            "Nonfinite native triangle support direction"
        );
        let scores = self
            .vertices
            .map(|v| v[2].mul_add(d[2], v[1].mul_add(d[1], v[0] * d[0])));
        ensure!(
            scores.iter().all(|v| v.is_finite()),
            "Native triangle support score overflow"
        );
        let mut id = if scores[2] > scores[1] { 2 } else { 1 };
        if scores[0] > scores[id] {
            id = 0;
        }
        let mut point = self.vertices[id];
        let extruded =
            d[2].mul_add(self.extrusion[2], d[0] * self.extrusion[0]) + d[1] * self.extrusion[1];
        ensure!(
            extruded.is_finite(),
            "Native triangle extrusion score overflow"
        );
        if extruded > 0. {
            point = std::array::from_fn(|i| point[i] + self.extrusion[i]);
            id += 3;
        }
        point[3] = f32::from_bits(0x3f000000 | id as u32);
        ensure!(
            point[..3].iter().all(|v| v.is_finite()),
            "Native triangle support point overflow"
        );
        Ok((id as u16, point))
    }
    fn sphere_count(&mut self) -> Result<u32> {
        Ok(3 * (self.extrusion_flag as u32 + 1))
    }
}
/// Convex support scans every serialized lane, including padding beyond
/// sphere_count. Keep these points separate from Box3D's logical hull vertices.
pub struct ConvexSupport {
    pub packed_vertices: Vec<[f32; 3]>,
    pub vertex_count: u32,
    pub stored_center: [f32; 4],
}
impl SupportShape for ConvexSupport {
    fn center(&mut self) -> Result<[f32; 4]> {
        ensure!(
            self.stored_center.iter().all(|v| v.is_finite()),
            "Nonfinite native convex center"
        );
        Ok(self.stored_center)
    }
    fn vertex(&mut self, id: u16) -> Result<[f32; 4]> {
        let p = self
            .packed_vertices
            .get(id as usize)
            .ok_or_else(|| anyhow::anyhow!("Convex source ID out of range"))?;
        ensure!(
            p.iter().all(|v| v.is_finite()),
            "Nonfinite convex source vertex"
        );
        Ok([p[0], p[1], p[2], f32::from_bits(0x3f000000 | id as u32)])
    }
    fn support(&mut self, direction: [f32; 4]) -> Result<(u16, [f32; 4])> {
        ensure!(
            self.vertex_count > 0
                && self.packed_vertices.len() == (self.vertex_count as usize).div_ceil(4) * 4
                && self.packed_vertices.len() <= 65536,
            "Invalid packed convex support layout"
        );
        ensure!(
            direction.iter().all(|v| v.is_finite()),
            "Nonfinite convex support direction"
        );
        let mut best = f32::from_bits(0xff7fffee);
        let mut chosen = 0usize;
        for (block, points) in self.packed_vertices.chunks_exact(4).enumerate() {
            // 8027b928..978: separately rounded products and additions, no FMA.
            let score =
                |p: [f32; 3]| (p[0] * direction[0] + p[1] * direction[1]) + p[2] * direction[2];
            let scores = [
                score(points[0]),
                score(points[1]),
                score(points[2]),
                score(points[3]),
            ];
            ensure!(
                scores.iter().all(|v| v.is_finite()),
                "Convex support projection overflow"
            );
            // Strict block improvement; jump-table comparisons prefer later
            // lanes on equal scores within the winning block.
            let mut lane = 0;
            for i in 1..4 {
                if scores[i] >= scores[lane] {
                    lane = i;
                }
            }
            if scores[lane] > best {
                best = scores[lane];
                chosen = block * 4 + lane;
            }
        }
        let id = chosen as u16;
        Ok((id, self.vertex(id)?))
    }
    fn sphere_count(&mut self) -> Result<u32> {
        Ok(self.vertex_count)
    }
}
fn max_dimension(count: u32) -> u8 {
    if count > 3 {
        15
    } else {
        count as u8
    }
}
fn transpose_direction(m: &super::native_hk_transform::Transform, v: [f32; 4]) -> [f32; 4] {
    let mut out = [0.; 4];
    for i in 0..3 {
        out[i] = m[4 * i + 2].mul_add(v[2], m[4 * i + 1].mul_add(v[1], m[4 * i] * v[0]));
    }
    out[3] = m[11].mul_add(v[2], m[8].mul_add(v[1], m[9] * v[0]));
    out
}
impl Cache {
    /// DOL 802ed8e8..802edb40. General convex initialization, also selected
    /// for an extruded triangle. Preserve callback ordering and zero unused IDs.
    pub fn initialize_convex(
        &mut self,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
    ) -> Result<()> {
        use super::native_hk_transform::transform_point;
        let a0 = a.vertex(0)?;
        let b0 = transform_point(b_in_a, b.vertex(0)?);
        let direction = std::array::from_fn(|i| b0[i] - a0[i]);
        ensure!(
            direction.iter().all(|v: &f32| v.is_finite()),
            "Nonfinite GSK initial direction"
        );
        let (id_a, point_a) = a.support(direction)?;
        let delta = std::array::from_fn(|i| b0[i] - point_a[i]);
        let direction = transpose_direction(b_in_a, delta).map(|v| -v);
        ensure!(
            direction.iter().all(|v| v.is_finite()),
            "Nonfinite GSK second support direction"
        );
        let (id_b, _) = b.support(direction)?;
        self.0[..2].copy_from_slice(&id_a.to_be_bytes());
        self.0[2..4].copy_from_slice(&id_b.to_be_bytes());
        self.0[4..8].fill(0);
        self.0[8] = 1;
        self.0[9] = 1;
        let max_a = max_dimension(a.sphere_count()?);
        let max_b = max_dimension(b.sphere_count()?);
        self.0[10] = (max_a << 4) | max_b;
        self.0[11] = 0;
        Ok(())
    }
    /// DOL 802ed5b0..802ed8c8, non-extruded triangle branch only. An extruded
    /// triangle must use initialize_convex, as selected by original byte +17.
    pub fn initialize_triangle(
        &mut self,
        a: &mut dyn SupportShape,
        triangle: [[f32; 4]; 3],
        b_in_a: &super::native_hk_transform::Transform,
    ) -> Result<()> {
        use super::native_hk_transform::transform_point;
        self.0[2..8].copy_from_slice(&[0, 0, 0, 1, 0, 2]);
        let vertices = triangle.map(|v| transform_point(b_in_a, v));
        let e: [f32; 4] = std::array::from_fn(|i| vertices[1][i] - vertices[0][i]);
        let f: [f32; 4] = std::array::from_fn(|i| vertices[2][i] - vertices[1][i]);
        let y = -e[0].mul_add(f[2], -(f[0] * e[2]));
        let normal = [
            e[1].mul_add(f[2], -(f[1] * e[2])),
            y,
            -e[1].mul_add(f[0], -(f[1] * e[0])),
            y,
        ];
        let vertex = a.vertex(0)?;
        let delta: [f32; 4] = std::array::from_fn(|i| vertex[i] - vertices[0][i]);
        let side = delta[2].mul_add(normal[2], delta[0] * normal[0]) + delta[1] * normal[1];
        // 802ed824..860 XORs the sign bit, including negative zero.
        let sign = side.to_bits() & 0x80000000;
        let direction = normal.map(|v| f32::from_bits(v.to_bits() ^ sign));
        ensure!(
            side.is_finite() && direction.iter().all(|v| v.is_finite()),
            "Nonfinite GSK triangle direction"
        );
        let (id, _) = a.support(direction)?;
        self.0[..2].copy_from_slice(&id.to_be_bytes());
        self.0[8] = 1;
        self.0[9] = 3;
        self.0[10] = (max_dimension(a.sphere_count()?) << 4) | 3;
        self.0[11] = 0;
        Ok(())
    }
    pub fn update(&mut self, a: &[u16], b: &[u16]) -> Result<()> {
        ensure!(
            (1..=3).contains(&a.len()) && (1..=3).contains(&b.len()) && a.len() + b.len() <= 4,
            "Invalid native GSK feature dimensions"
        );
        let mut next = self.0;
        next[8] = a.len() as u8;
        next[9] = b.len() as u8;
        let mut offset = 0;
        for vertices in [a, b] {
            let ordered = canonical_feature(vertices);
            for &id in &ordered[..vertices.len()] {
                next[offset..offset + 2].copy_from_slice(&id.to_be_bytes());
                offset += 2;
            }
        }
        self.0 = next;
        Ok(())
    }
    pub fn features(&self) -> Result<(&[u8], &[u8])> {
        let a = self.0[8] as usize;
        let b = self.0[9] as usize;
        ensure!(
            (1..=3).contains(&a) && (1..=3).contains(&b) && a + b <= 4,
            "Invalid native GSK cache dimensions"
        );
        Ok((&self.0[..2 * a], &self.0[2 * a..2 * (a + b)]))
    }
}

// Strict comparisons and swap parity match both native shape branches.
// For three distinct IDs this is a cyclic rotation, not a descending sort.
fn canonical_feature(vertices: &[u16]) -> [u16; 3] {
    let mut ids = [0; 3];
    ids[..vertices.len()].copy_from_slice(vertices);
    if vertices.len() > 1 {
        let mut swaps = 0;
        if ids[1] > ids[0] {
            ids.swap(0, 1);
            swaps = 1;
        }
        if vertices.len() > 2 {
            if ids[2] > ids[0] {
                ids.swap(0, 2);
                swaps += 1;
            }
            if swaps == 1 {
                ids.swap(1, 2);
            }
        }
    }
    ids
}

#[derive(Debug, PartialEq)]
pub struct CoreWitness {
    pub normal: [f32; 4],
    pub point_a: [f32; 4],
    pub distance: f32,
}

#[derive(Debug)]
pub struct EdgePairWitness {
    /// Native bits: A end/start = 1/2; B end/start = 4/8.
    pub endpoint_mask: u8,
    pub point_a: [f32; 4],
    pub delta: [f32; 4],
    pub squared_distance: f32,
}
/// DOL 8013dadc..8013dce4, called at 802eb704. Inputs are origins and
/// full edge vectors, not normalized directions. Preserve the original
/// near-parallel decision and sequential endpoint corrections.
pub fn edge_pair(a: [f32; 4], da: [f32; 4], b: [f32; 4], db: [f32; 4]) -> Result<EdgePairWitness> {
    ensure!(
        a.iter()
            .chain(da.iter())
            .chain(b.iter())
            .chain(db.iter())
            .all(|v| v.is_finite()),
        "Nonfinite GSK edge pair"
    );
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let r = std::array::from_fn(|i| b[i] - a[i]);
    let aa = dot(da, da);
    let bb = dot(db, db);
    let ab = dot(da, db);
    let ar = dot(da, r);
    let br = dot(db, r);
    let ab_squared = ab * ab;
    let denominator = aa.mul_add(bb, -ab_squared).abs();
    let numerator = ar.mul_add(bb, -(br * ab));
    ensure!(
        [aa, bb, ab, ar, br, denominator, numerator]
            .iter()
            .all(|v| v.is_finite()),
        "GSK edge pair overflow"
    );
    let (mut s, mut mask) = if numerator * denominator >= denominator * denominator {
        (1., 1)
    } else if numerator <= 0. {
        (0., 2)
    } else if denominator > f32::from_bits(0x35800000) * aa.mul_add(bb, ab_squared) {
        (numerator / denominator, 0)
    } else {
        (1., 1)
    };
    let t_numerator = s.mul_add(ab, -br);
    let t: f32;
    if t_numerator >= bb || t_numerator <= 0. {
        if t_numerator >= bb {
            t = 1.;
            mask = 4;
        } else {
            t = 0.;
            mask = 8;
        }
        let revised = t.mul_add(ab, ar);
        s = 0.;
        if revised <= 0. {
            mask |= 2;
        } else if revised >= aa {
            s = 1.;
            mask |= 1;
        } else {
            s = revised / aa;
        }
    } else {
        t = t_numerator / bb;
    }
    let point_a = std::array::from_fn(|i| da[i].mul_add(s, a[i]));
    let point_b: [f32; 4] = std::array::from_fn(|i| db[i].mul_add(t, b[i]));
    let delta = std::array::from_fn(|i| point_a[i] - point_b[i]);
    let squared_distance = dot(delta, delta);
    ensure!(
        point_a.iter().chain(delta.iter()).all(|v| v.is_finite()) && squared_distance.is_finite(),
        "GSK edge pair witness overflow"
    );
    Ok(EdgePairWitness {
        endpoint_mask: mask,
        point_a,
        delta,
        squared_distance,
    })
}

/// Active feature vertices loaded by 80236834..802368b8, then transformed
/// by 802eb158..802eb198. Source B retains encoded IDs independently of its
/// transformed copy. This is solver input, not a completed distance query.
pub struct Features {
    pub count_a: usize,
    pub count_b: usize,
    pub max_dimension_a: u8,
    pub max_dimension_b: u8,
    // Native scratch reserves four vertices per shape for transient expansion.
    // Persistent cache counts remain restricted to three per shape.
    pub vertices_a: [[f32; 4]; 4],
    pub vertices_b: [[f32; 4]; 4],
    pub vertices_b_in_a: [[f32; 4]; 4],
}
/// DOL jump table806bb320, index=(dimensionA<<3)|dimensionB.
/// Cache dimensions total at most four, but expansion temporarily reaches
/// point/tetrahedron and edge/face before another reduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureCase {
    PointPoint,
    PointEdge,
    PointFace,
    EdgeEdge,
    EdgeFace,
    PointTetrahedron,
    Failure,
}
pub fn feature_case(a: usize, b: usize) -> FeatureCase {
    match (a, b) {
        (1, 1) => FeatureCase::PointPoint,
        (1, 2) | (2, 1) => FeatureCase::PointEdge,
        (1, 3) | (3, 1) => FeatureCase::PointFace,
        (2, 2) => FeatureCase::EdgeEdge,
        (2, 3) | (3, 2) => FeatureCase::EdgeFace,
        (1, 4) | (4, 1) => FeatureCase::PointTetrahedron,
        _ => FeatureCase::Failure,
    }
}
#[derive(Debug)]
pub struct FeatureReduction {
    /// Native scratch+174: suppress A support at8, B at-8, neither at0.
    pub blocked_orientation: i32,
    pub direction: [f32; 4],
    pub face: Option<FaceClassification>,
    pub edge_witness: Option<EdgePairWitness>,
}
/// Status 1 from 802eaa44 transfers to the solver's failure/penetration
/// branch, not a successful separating direction or an absent contact.
#[derive(Debug)]
pub enum ExpandedReduction {
    Reduced(FeatureReduction),
    NativeFailure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpandedShape {
    A,
    B,
}
/// Paired differences supplied to 802e7d6c. This is its input preparation,
/// not a solved penetration result or a valid persistent feature cache.
#[derive(Debug)]
pub struct FallbackSeed {
    pub count: usize,
    pub differences: [[f32; 4]; 4],
}
/// Vertex record used by the native expanding penetration polytope.
/// A and B remain in their own source frames, with encoded W IDs intact.
#[derive(Clone, Debug)]
pub struct PenetrationVertex {
    pub difference: [f32; 4],
    pub point_a: [f32; 4],
    pub point_b: [f32; 4],
}
/// Native seed arrays retain inactive entries when only context+6c changes.
#[derive(Clone, Debug)]
pub struct PenetrationSeed {
    pub slots: [PenetrationVertex; 4],
    count: usize,
}
impl PenetrationSeed {
    pub fn from_slots(slots: [PenetrationVertex; 4], count: usize) -> Result<Self> {
        ensure!(count <= 4, "Invalid penetration seed count");
        Ok(Self { slots, count })
    }
    fn clear(&mut self) {
        self.count = 0;
    }
    fn truncate(&mut self, count: usize) {
        self.count = self.count.min(count);
    }
    fn push(&mut self, vertex: PenetrationVertex) {
        assert!(self.count < 4, "Native penetration seed capacity exceeded");
        self.slots[self.count] = vertex;
        self.count += 1;
    }
}
impl std::ops::Deref for PenetrationSeed {
    type Target = [PenetrationVertex];
    fn deref(&self) -> &Self::Target {
        &self.slots[..self.count]
    }
}
impl std::ops::DerefMut for PenetrationSeed {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slots[..self.count]
    }
}
#[cfg(test)]
impl FromIterator<PenetrationVertex> for PenetrationSeed {
    fn from_iter<T: IntoIterator<Item = PenetrationVertex>>(items: T) -> Self {
        let slots = std::array::from_fn(|_| PenetrationVertex {
            difference: [0.; 4],
            point_a: [0.; 4],
            point_b: [0.; 4],
        });
        let mut seed = Self { slots, count: 0 };
        for vertex in items {
            seed.push(vertex);
        }
        seed
    }
}
#[cfg(test)]
impl From<Vec<PenetrationVertex>> for PenetrationSeed {
    fn from(vertices: Vec<PenetrationVertex>) -> Self {
        vertices.into_iter().collect()
    }
}
#[derive(Clone, Debug)]
pub struct PenetrationFace {
    pub vertices: [usize; 3],
    /// Opposite directed edge as (face index, edge index), in source order.
    pub opposite: [(usize, usize); 3],
    pub normal: [f32; 4],
    pub distance: f32,
}
#[derive(Debug, PartialEq, Eq)]
pub enum PenetrationStatus {
    Converged,
    Capacity,
    Retry,
}
#[derive(Debug)]
pub struct PenetrationTerminalSearch {
    pub normal: [f32; 4],
    pub vertex: PenetrationVertex,
    pub distance: f32,
}
/// 802e8a00..8bd4: all four trials share the first trial's normalization factor.
fn terminal_trial_directions(normal: [f32; 4], step: f32) -> Result<[[f32; 4]; 4]> {
    let (small, first) = if normal[1].abs() < normal[0].abs() {
        (1, 0)
    } else {
        (0, 1)
    };
    let second = if normal[2].abs() < normal[small].abs() {
        small
    } else {
        2
    };
    let mut perpendicular = [0.; 4];
    perpendicular[first] = normal[second];
    perpendicular[second] = -normal[first];
    let perpendicular = normalize_four_lanes(perpendicular)?;
    let y = -normal[0].mul_add(perpendicular[2], -(perpendicular[0] * normal[2]));
    let cross = [
        normal[1].mul_add(perpendicular[2], -(perpendicular[1] * normal[2])),
        y,
        -normal[1].mul_add(perpendicular[0], -(perpendicular[1] * normal[0])),
        y,
    ];
    let offset = perpendicular.map(|v| v * step);
    let other = cross.map(|v| v * step);
    let mut trials: [[_; 4]; 4] = [
        std::array::from_fn(|i| normal[i] + offset[i]),
        std::array::from_fn(|i| normal[i] - offset[i]),
        std::array::from_fn(|i| normal[i] + other[i]),
        std::array::from_fn(|i| normal[i] - other[i]),
    ];
    let p = trials[0];
    let length = p[2].mul_add(p[2], p[0] * p[0]) + p[1] * p[1];
    ensure!(
        length.is_finite() && length > 0.,
        "Invalid terminal trial direction"
    );
    let estimate = super::pose_blend::estimate(length);
    let residual = -length.mul_add(estimate * estimate, -1.);
    let refined = residual.mul_add(0.5 * estimate, estimate);
    let factor = if refined >= 0. { refined } else { 0. };
    for trial in &mut trials {
        *trial = trial.map(|v| v * factor);
    }
    Ok(trials)
}
/// 802e8578 terminal fallback, including shrinking direction refinement.
pub fn penetration_terminal_fallback(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
) -> Result<PenetrationTerminalSearch> {
    let mut best = penetration_terminal_axes(a, b, b_in_a)?;
    let mut step = 1.;
    while step > 0.001_f32 {
        step *= 0.99_f32;
        let trials = terminal_trial_directions(best.normal, step)?;
        for normal in trials {
            let vertex = penetration_support(a, b, b_in_a, normal.map(|v| -v))?;
            let d = vertex.difference;
            let score = d[2].mul_add(normal[2], d[0] * normal[0]) + d[1] * normal[1];
            ensure!(score.is_finite(), "Terminal refinement score overflow");
            if score > best.distance {
                let mut stored = normal;
                stored[3] = score;
                best = PenetrationTerminalSearch {
                    normal: stored,
                    vertex,
                    distance: score,
                };
            }
        }
        step *= 0.5;
    }
    Ok(best)
}
/// 802e8684..899c: initial six-axis search only. Native subsequently refines
/// around this direction; this intermediate must not be published as a contact.
pub fn penetration_terminal_axes(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
) -> Result<PenetrationTerminalSearch> {
    let mut best = None;
    let mut distance = f32::from_bits(0xff7fffee);
    for axis in 0..3 {
        let mut positive = [0.; 4];
        positive[axis] = 1.;
        for normal in [positive, positive.map(|v| -v)] {
            let vertex = penetration_support(a, b, b_in_a, normal.map(|v| -v))?;
            let d = vertex.difference;
            let score = d[2].mul_add(normal[2], d[0] * normal[0]) + d[1] * normal[1];
            ensure!(score.is_finite(), "Terminal support score overflow");
            if score > distance {
                distance = score;
                let mut stored = normal;
                stored[3] = score;
                best = Some(PenetrationTerminalSearch {
                    normal: stored,
                    vertex,
                    distance: score,
                });
            }
        }
    }
    best.ok_or_else(|| anyhow::anyhow!("Terminal support search did not improve native sentinel"))
}
#[derive(Debug, PartialEq)]
pub enum PenetrationRetry {
    KeepFirstSeed,
    EmptySeed { translation_delta: [f32; 3] },
    TerminalFallback,
}
/// DOL 802e7c48..7d18. Count is incremented before selecting this action.
/// Only XYZ is established here: native padded W reads stack+5c, whose
/// provenance requires runtime validation before applying the full transform.
pub fn penetration_retry_action(retry_count: u32) -> Result<PenetrationRetry> {
    ensure!(
        retry_count > 0,
        "Retry count must be incremented before retry selection"
    );
    if retry_count == 1 {
        return Ok(PenetrationRetry::KeepFirstSeed);
    }
    if retry_count >= 20 {
        return Ok(PenetrationRetry::TerminalFallback);
    }
    let integer = retry_count.wrapping_mul(0x41c64e6d).wrapping_add(0x3039) & 0x7fffffff;
    // Native independently converts the same integer for each coordinate.
    let unit = (integer as f32) / f32::from_bits(0x4f000000);
    let shifted = unit + f32::from_bits(0xb8d1b717);
    let delta = shifted * f32::from_bits(0x3951b717);
    Ok(PenetrationRetry::EmptySeed {
        translation_delta: [delta; 3],
    })
}
#[derive(Debug)]
pub enum PenetrationAttempt {
    Retry,
    Contact {
        status: PenetrationStatus,
        normal: [f32; 4],
        point_a: [f32; 4],
        distance: f32,
    },
}
#[derive(Debug)]
pub struct PenetrationSolve {
    /// Native wrapper status1, requiring singleton feature restoration.
    pub terminal_fallback: bool,
    pub status: PenetrationStatus,
    pub normal: [f32; 4],
    pub point_a: [f32; 4],
    pub distance: f32,
    pub retries: u32,
}
/// 802e7cfc..7d14 stores the resulting W back into both the local transform
/// and stack+5c, making it the next retry's increment.
fn perturb_penetration_transform(
    transform: &mut super::native_hk_transform::Transform,
    delta: [f32; 3],
    padding: &mut f32,
) -> Result<()> {
    ensure!(padding.is_finite(), "Nonfinite penetration retry padding");
    for i in 0..3 {
        transform[12 + i] += delta[i];
    }
    transform[15] += *padding;
    *padding = transform[15];
    ensure!(
        transform[12..].iter().all(|v| v.is_finite()),
        "Penetration retry transform overflow"
    );
    Ok(())
}
/// Join802e78b8 retries and802e7d6c terminal fallback. Initial stack+5c
/// must come from reference evidence; no guessed default is supplied. Later
/// increments follow native writeback. Terminal fallback uses original transform.
pub fn solve_penetration_with_retry_padding(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    original: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
    tolerance: f32,
    initial_retry_padding: Option<f32>,
) -> Result<PenetrationSolve> {
    let mut transform = *original;
    let mut retries = 0;
    let mut retry_padding = initial_retry_padding;
    loop {
        if let PenetrationAttempt::Contact {
            status,
            normal,
            point_a,
            distance,
        } = penetration_attempt(a, b, &transform, seed, tolerance)?
        {
            return Ok(PenetrationSolve {
                terminal_fallback: false,
                status,
                normal,
                point_a,
                distance,
                retries,
            });
        }
        retries += 1;
        match penetration_retry_action(retries)? {
            PenetrationRetry::KeepFirstSeed => seed.truncate(1),
            PenetrationRetry::EmptySeed { translation_delta } => {
                let padding = retry_padding.as_mut().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing original padded-W evidence for penetration retry {retries}"
                    )
                })?;
                perturb_penetration_transform(&mut transform, translation_delta, padding)?;
                seed.clear();
            }
            PenetrationRetry::TerminalFallback => {
                let contact = penetration_terminal_fallback(a, b, original)?;
                ensure!(
                    !seed.is_empty(),
                    "Terminal fallback has no retained difference slot"
                );
                seed.truncate(1);
                seed[0].point_a = contact.vertex.point_a;
                seed[0].point_b = contact.vertex.point_b;
                return Ok(PenetrationSolve {
                    terminal_fallback: true,
                    status: PenetrationStatus::Converged,
                    normal: contact.normal,
                    point_a: contact.vertex.point_a,
                    distance: contact.distance,
                    retries,
                });
            }
        }
    }
}
/// One 802e78b8 attempt, before its outer perturbation/restart policy.
/// Seed source pairs are retained on retry and replaced by extracted pairs on contact.
pub fn penetration_attempt(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
    tolerance: f32,
) -> Result<PenetrationAttempt> {
    match initialize_penetration_seed(a, b, b_in_a, seed, tolerance)? {
        PenetrationInitialization::Retry => return Ok(PenetrationAttempt::Retry),
        PenetrationInitialization::Contact {
            normal,
            point_a,
            distance,
        } => {
            return Ok(PenetrationAttempt::Contact {
                status: PenetrationStatus::Converged,
                normal,
                point_a,
                distance,
            })
        }
        PenetrationInitialization::Ready => {}
    }
    ensure!(
        seed.len() == 4,
        "Initialized penetration seed is not a tetrahedron"
    );
    let polytope = expand_penetration_polytope(
        a,
        b,
        b_in_a,
        std::array::from_fn(|i| seed[i].clone()),
        [tolerance; 3],
    )?;
    if polytope.status == PenetrationStatus::Retry {
        return Ok(PenetrationAttempt::Retry);
    }
    let contact = polytope.extract_contact()?;
    seed.clear();
    for i in 0..3 {
        seed.push(PenetrationVertex {
            difference: contact.differences[i],
            point_a: contact.source_a[i],
            point_b: contact.source_b[i],
        });
    }
    Ok(PenetrationAttempt::Contact {
        status: polytope.status,
        normal: contact.normal,
        point_a: contact.point_a,
        distance: contact.distance,
    })
}
#[derive(Debug)]
pub enum PenetrationInitialization {
    Ready,
    Retry,
    Contact {
        normal: [f32; 4],
        point_a: [f32; 4],
        distance: f32,
    },
}
/// Join the recovered branches of802e6fc4. Restarts retain native
/// expansion history, preventing repeated fallback from looping indefinitely.
pub fn initialize_penetration_seed(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
    tolerance: f32,
) -> Result<PenetrationInitialization> {
    ensure!(
        seed.len() <= 4 && tolerance.is_finite() && tolerance >= 0.,
        "Invalid penetration initialization input"
    );
    let squared = tolerance * tolerance;
    let mut last_extended = -1;
    loop {
        if seed.is_empty() {
            let translation: [f32; 4] = std::array::from_fn(|i| b_in_a[12 + i]);
            let length_squared = |v: [f32; 4]| v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
            // 8014c838 copies806a95b0 into8070afc0: positive Y, W zero.
            let direction = if length_squared(translation) <= squared {
                [0., 1., 0., 0.]
            } else {
                translation.map(|v| -v)
            };
            let vertex = penetration_support(a, b, b_in_a, direction)?;
            let length = length_squared(vertex.difference);
            let point_a = vertex.point_a;
            seed.push(vertex);
            last_extended = 0;
            if length <= squared {
                // 80167fd0: length squared times the refined inverse square root.
                let distance = if length == 0. {
                    0.
                } else {
                    let estimate = super::pose_blend::estimate(length);
                    let residual = -length.mul_add(estimate * estimate, -1.);
                    let refined = residual.mul_add(0.5 * estimate, estimate);
                    length * if refined >= 0. { refined } else { 0. }
                };
                return Ok(PenetrationInitialization::Contact {
                    normal: direction,
                    point_a,
                    distance,
                });
            }
        }
        let points: Vec<_> = seed.iter().map(|v| v.difference).collect();
        let output = match penetration_seed_gate(&points, squared, last_extended)? {
            PenetrationSeedGate::Ready => return Ok(PenetrationInitialization::Ready),
            PenetrationSeedGate::Retry => return Ok(PenetrationInitialization::Retry),
            PenetrationSeedGate::Restart(count) => {
                seed.truncate(count);
                continue;
            }
            PenetrationSeedGate::CollapsedTriangle => collapsed_penetration_triangle(seed)?,
            PenetrationSeedGate::Extend => {
                last_extended = seed.len() as i32;
                match seed.len() {
                    1 => {
                        extend_penetration_point(a, b, b_in_a, seed)?;
                        continue;
                    }
                    2 => {
                        extend_penetration_segment(a, b, b_in_a, seed)?;
                        continue;
                    }
                    3 => extend_penetration_triangle(a, b, b_in_a, seed, tolerance)?,
                    _ => unreachable!(),
                }
            }
        };
        if let PenetrationTriangleExpansion::Contact {
            normal,
            point_a,
            distance,
        } = output
        {
            return Ok(PenetrationInitialization::Contact {
                normal,
                point_a,
                distance,
            });
        }
    }
}
#[derive(Debug)]
pub enum PenetrationTriangleExpansion {
    Expanded,
    Contact {
        normal: [f32; 4],
        point_a: [f32; 4],
        distance: f32,
    },
}
/// DOL 802e7450..75fc, after a repeated collapsed-triangle gate.
pub fn collapsed_penetration_triangle(
    seed: &[PenetrationVertex],
) -> Result<PenetrationTriangleExpansion> {
    ensure!(
        seed.len() == 3,
        "Collapsed triangle output requires three vertices"
    );
    let edge: [f32; 4] = std::array::from_fn(|i| seed[0].difference[i] - seed[1].difference[i]);
    let (small, first) = if edge[1].abs() < edge[0].abs() {
        (1, 0)
    } else {
        (0, 1)
    };
    let second = if edge[2].abs() < edge[small].abs() {
        small
    } else {
        2
    };
    let mut perpendicular = [0.; 4];
    perpendicular[first] = edge[second];
    perpendicular[second] = -edge[first];
    let normal = normalize_four_lanes(perpendicular)?;
    let d = seed[0].difference;
    let magnitude = (d[0].abs() + d[2].abs()) + d[1].abs();
    // Both native sums use vertex0; vertex1 absolute values are loaded but unused.
    let weight = magnitude / (f32::from_bits(0x34000000) + (magnitude + magnitude));
    let complement = 1. - weight;
    let point_a = std::array::from_fn(|i| {
        weight.mul_add(seed[1].point_a[i], seed[0].point_a[i] * complement)
    });
    ensure!(
        point_a.iter().all(|v| v.is_finite()),
        "Collapsed triangle witness overflow"
    );
    Ok(PenetrationTriangleExpansion::Contact {
        normal,
        point_a,
        distance: 0.,
    })
}
/// DOL 802e7600..7758. Called only after the nonzero triangle-area gate.
pub fn extend_penetration_triangle(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
    tolerance: f32,
) -> Result<PenetrationTriangleExpansion> {
    ensure!(
        seed.len() == 3 && tolerance.is_finite(),
        "Invalid triangle expansion input"
    );
    let u: [f32; 4] = std::array::from_fn(|i| seed[0].difference[i] - seed[1].difference[i]);
    let v: [f32; 4] = std::array::from_fn(|i| seed[1].difference[i] - seed[2].difference[i]);
    let y = -u[0].mul_add(v[2], -(v[0] * u[2]));
    let cross = [
        u[1].mul_add(v[2], -(v[1] * u[2])),
        y,
        -u[1].mul_add(v[0], -(v[1] * u[0])),
        y,
    ];
    ensure!(
        cross[2].mul_add(cross[2], cross[0] * cross[0]) + cross[1] * cross[1] != 0.,
        "Collapsed triangle requires native collapsed output path"
    );
    let normal = normalize_four_lanes(cross)?;
    let (candidate, score) = penetration_axis_support(a, b, b_in_a, normal, seed[0].difference)?;
    if score >= tolerance {
        seed.push(candidate);
        return Ok(PenetrationTriangleExpansion::Expanded);
    }
    let weights = penetration_weights([0.; 4], std::array::from_fn(|i| seed[i].difference))?;
    let point_a = std::array::from_fn(|i| {
        seed[2].point_a[i].mul_add(
            weights[2],
            seed[1].point_a[i].mul_add(weights[1], seed[0].point_a[i] * weights[0]),
        )
    });
    // Native publishes the triangle normal unchanged and a positive zero depth.
    Ok(PenetrationTriangleExpansion::Contact {
        normal,
        point_a,
        distance: 0.,
    })
}
/// DOL 802e72d0..739c, after the segment degeneracy gate.
pub fn extend_penetration_segment(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
) -> Result<()> {
    ensure!(
        seed.len() == 2,
        "Segment expansion requires two seed vertices"
    );
    let edge: [f32; 4] = std::array::from_fn(|i| seed[0].difference[i] - seed[1].difference[i]);
    // 80195b34: strict comparisons preserve native axis ordering at ties.
    let (small, first) = if edge[1].abs() < edge[0].abs() {
        (1, 0)
    } else {
        (0, 1)
    };
    let second = if edge[2].abs() < edge[small].abs() {
        small
    } else {
        2
    };
    let mut perpendicular = [0.; 4];
    perpendicular[first] = edge[second];
    perpendicular[second] = -edge[first];
    let y = -edge[0].mul_add(perpendicular[2], -(perpendicular[0] * edge[2]));
    let other = [
        edge[1].mul_add(perpendicular[2], -(perpendicular[1] * edge[2])),
        y,
        -edge[1].mul_add(perpendicular[0], -(perpendicular[1] * edge[0])),
        y,
    ];
    let perpendicular = normalize_four_lanes(perpendicular)?;
    let other = normalize_four_lanes(other)?;
    let origin = seed[0].difference;
    let (first, first_score) = penetration_axis_support(a, b, b_in_a, perpendicular, origin)?;
    let (second, second_score) = penetration_axis_support(a, b, b_in_a, other, origin)?;
    // Unlike the opposed-axis helper, this outer comparison chooses second on ties.
    seed.push(if first_score > second_score {
        first
    } else {
        second
    });
    Ok(())
}
/// DOL 802e67b8: compare signed support displacement along opposite directions.
/// Used by segment and triangle seed expansion; this is not squared distance.
pub fn penetration_axis_support(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    direction: [f32; 4],
    origin: [f32; 4],
) -> Result<(PenetrationVertex, f32)> {
    ensure!(
        origin.iter().all(|v| v.is_finite()),
        "Nonfinite penetration support origin"
    );
    let reverse = direction.map(|v| -v);
    let forward = penetration_support(a, b, b_in_a, direction)?;
    let backward = penetration_support(a, b, b_in_a, reverse)?;
    let projection = |vertex: &PenetrationVertex, axis: [f32; 4]| {
        let delta: [f32; 4] = std::array::from_fn(|i| vertex.difference[i] - origin[i]);
        delta[2].mul_add(axis[2], delta[0] * axis[0]) + delta[1] * axis[1]
    };
    let first = projection(&forward, direction);
    let second = projection(&backward, reverse);
    ensure!(
        first.is_finite() && second.is_finite(),
        "Penetration support projection overflow"
    );
    Ok(if first >= second {
        (forward, first)
    } else {
        (backward, second)
    })
}
/// DOL 802e71b0..802e7264, after the one-point degeneracy gate.
/// Query both directions before choosing; equal squared distances retain the first.
pub fn extend_penetration_point(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: &mut PenetrationSeed,
) -> Result<()> {
    ensure!(
        seed.len() == 1,
        "Point expansion requires exactly one seed vertex"
    );
    let direction = seed[0].difference;
    let first = penetration_support(a, b, b_in_a, direction)?;
    let second = penetration_support(a, b, b_in_a, direction.map(|v| -v))?;
    let distance = |candidate: &PenetrationVertex| {
        let delta: [f32; 4] = std::array::from_fn(|i| candidate.difference[i] - direction[i]);
        delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1]
    };
    let first_distance = distance(&first);
    let second_distance = distance(&second);
    ensure!(
        first_distance.is_finite() && second_distance.is_finite(),
        "Point expansion distance overflow"
    );
    seed.push(if first_distance >= second_distance {
        first
    } else {
        second
    });
    Ok(())
}
#[derive(Debug, PartialEq, Eq)]
pub enum PenetrationSeedGate {
    Extend,
    Ready,
    Restart(usize),
    Retry,
    /// Native collapsed-triangle output path at 802e7450, not an EPA seed.
    CollapsedTriangle,
}
/// Degeneracy gates in 802e6fc4. last_extended is native r30, initially -1.
/// Threshold is context+50 (the already-squared tolerance).
pub fn penetration_seed_gate(
    points: &[[f32; 4]],
    tolerance_squared: f32,
    last_extended: i32,
) -> Result<PenetrationSeedGate> {
    ensure!(
        (1..=4).contains(&points.len()),
        "Invalid penetration seed size"
    );
    ensure!(
        tolerance_squared.is_finite()
            && tolerance_squared >= 0.
            && points.iter().flatten().all(|v| v.is_finite()),
        "Invalid penetration seed input"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] { std::array::from_fn(|i| a[i] - b[i]) };
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let degenerate = match points.len() {
        1 => dot(points[0], points[0]) <= tolerance_squared,
        2 => {
            let d = sub(points[0], points[1]);
            dot(d, d) <= tolerance_squared
        }
        _ => {
            let a = sub(points[0], points[1]);
            let b = sub(points[1], points[2]);
            let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
            let normal = [
                a[1].mul_add(b[2], -(b[1] * a[2])),
                y,
                -a[1].mul_add(b[0], -(b[1] * a[0])),
                y,
            ];
            let length = dot(normal, normal);
            if points.len() == 3 {
                length == 0.
            } else {
                let projection = dot(sub(points[3], points[0]), normal);
                projection * projection <= length * tolerance_squared
            }
        }
    };
    if !degenerate {
        return Ok(if points.len() == 4 {
            PenetrationSeedGate::Ready
        } else {
            PenetrationSeedGate::Extend
        });
    }
    let previous = points.len() - 1;
    Ok(if last_extended < (previous as i32) {
        PenetrationSeedGate::Restart(previous)
    } else if points.len() == 3 {
        PenetrationSeedGate::CollapsedTriangle
    } else {
        PenetrationSeedGate::Retry
    })
}
/// DOL 802e9b90: follow the first eligible edge, avoiding immediate backtracking.
pub fn penetration_contact_face(
    faces: &[PenetrationFace],
    vertices: &[PenetrationVertex],
    start: usize,
) -> Result<usize> {
    ensure!(start < faces.len(), "Invalid penetration contact face");
    let original_distance = faces[start].distance;
    let mut visited = vec![false; faces.len()];
    let mut current = start;
    let mut incoming = None;
    loop {
        visited[current] = true;
        let face = &faces[current];
        let mut next = None;
        for edge in 0..3 {
            if incoming == Some((current, edge)) {
                continue;
            }
            let i = face.vertices[edge];
            let j = face.vertices[(edge + 1) % 3];
            ensure!(
                i < vertices.len() && j < vertices.len(),
                "Invalid contact edge vertex"
            );
            let p = vertices[i].difference;
            let d: [f32; 4] = std::array::from_fn(|k| vertices[j].difference[k] - p[k]);
            let n = face.normal;
            let cross = [
                d[1].mul_add(n[2], -(n[1] * d[2])),
                -d[0].mul_add(n[2], -(n[0] * d[2])),
                -d[1].mul_add(n[0], -(n[1] * d[0])),
            ];
            let length = d[2].mul_add(d[2], d[0] * d[0]) + d[1] * d[1];
            let side = cross[2].mul_add(p[2], cross[0] * p[0]) + cross[1] * p[1];
            if side * side.abs() >= -1e-10_f32 * length {
                continue;
            }
            let (other, opposite) = face.opposite[edge];
            ensure!(
                other < faces.len() && opposite < 3,
                "Invalid contact opposite edge"
            );
            if original_distance > 0. && faces[other].distance < 0.9 * original_distance {
                continue;
            }
            next = Some((other, opposite));
            break;
        }
        let Some((other, opposite)) = next else {
            return Ok(current);
        };
        current = other;
        incoming = Some((other, opposite));
        if visited[current] {
            return Ok(current);
        }
    }
}
/// DOL 8028a7f4: projected barycentric weights used by penetration extraction.
/// Degenerate triangles select the longest edge, without clamping weights.
pub fn penetration_weights(point: [f32; 4], triangle: [[f32; 4]; 3]) -> Result<[f32; 3]> {
    ensure!(
        point
            .iter()
            .chain(triangle.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite penetration weight input"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] { std::array::from_fn(|i| a[i] - b[i]) };
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let u = sub(triangle[0], triangle[1]);
    let v = sub(triangle[2], triangle[1]);
    let uu = dot(u, u);
    let vv = dot(v, v);
    let uv = dot(v, u);
    let determinant = uu * vv - uv * uv;
    ensure!(
        determinant.is_finite(),
        "Penetration weight determinant overflow"
    );
    let weights = if determinant > 0. {
        let delta = sub(triangle[1], point);
        let du = dot(delta, u);
        let dv = dot(delta, v);
        let reciprocal = 1. / determinant;
        let numerator_a = dv.mul_add(uv, -(vv * du));
        let numerator_c = du.mul_add(uv, -(uu * dv));
        [
            reciprocal * numerator_a,
            reciprocal * ((determinant - numerator_a) - numerator_c),
            reciprocal * numerator_c,
        ]
    } else {
        let w = sub(triangle[0], triangle[2]);
        let ww = dot(w, w);
        if (uu >= vv && ww >= uu) || (uu < vv && ww >= vv) {
            if ww > 0. {
                let t = dot(sub(point, triangle[2]), w) / ww;
                [t, 0., 1. - t]
            } else {
                [0.; 3]
            }
        } else if uu >= vv {
            let t = dot(sub(point, triangle[1]), u) / uu;
            [t, 1. - t, 0.]
        } else {
            let t = dot(sub(point, triangle[1]), v) / vv;
            [0., 1. - t, t]
        }
    };
    ensure!(
        weights.iter().all(|v| v.is_finite()),
        "Penetration weight overflow"
    );
    Ok(weights)
}
/// Intermediate result, before native 802e6d9c witness extraction.
#[derive(Debug)]
pub struct PenetrationPolytope {
    pub vertices: Vec<PenetrationVertex>,
    pub faces: Vec<PenetrationFace>,
    pub selected_face: usize,
    pub status: PenetrationStatus,
}
#[derive(Debug)]
pub struct PenetrationContact {
    pub face: usize,
    pub normal: [f32; 4],
    pub distance: f32,
    pub point_a: [f32; 4],
    pub source_a: [[f32; 4]; 3],
    pub source_b: [[f32; 4]; 3],
    pub differences: [[f32; 4]; 3],
    pub weights: [f32; 3],
}
impl PenetrationPolytope {
    /// DOL 802e6d9c. Startup 8014f25c initializes 8070b0a0 from zero-filled BSS.
    pub fn extract_contact(&self) -> Result<PenetrationContact> {
        ensure!(
            self.status != PenetrationStatus::Retry,
            "Retry polytope has no contact result"
        );
        let mut selected = self.selected_face;
        let mut differences = [[0.; 4]; 3];
        let mut weights = [0.; 3];
        for _ in 0..10 {
            let face = self
                .faces
                .get(selected)
                .ok_or_else(|| anyhow::anyhow!("Invalid extraction face"))?;
            ensure!(
                face.vertices.iter().all(|&i| i < self.vertices.len()),
                "Invalid extraction vertex"
            );
            differences = face.vertices.map(|i| self.vertices[i].difference);
            weights = penetration_weights([0.; 4], differences)?;
            if weights.iter().all(|&w| w >= 0.) {
                break;
            }
            selected = penetration_contact_face(&self.faces, &self.vertices, selected)?;
            // Native exits after its tenth face walk without recomputing weights.
        }
        let face = &self.faces[selected];
        ensure!(
            face.vertices.iter().all(|&i| i < self.vertices.len()),
            "Invalid final extraction vertex"
        );
        let source_a = face.vertices.map(|i| self.vertices[i].point_a);
        let source_b = face.vertices.map(|i| self.vertices[i].point_b);
        let point_a = std::array::from_fn(|i| {
            source_a[2][i].mul_add(
                weights[2],
                source_a[1][i].mul_add(weights[1], source_a[0][i] * weights[0]),
            )
        });
        Ok(PenetrationContact {
            face: selected,
            normal: face.normal.map(|v| -v),
            distance: -face.distance,
            point_a,
            source_a,
            source_b,
            differences,
            weights,
        })
    }
}
/// DOL 802e7a30..802e7c1c, after seed completion has supplied four vertices.
/// This does not replace native initialization, retry perturbation or extraction.
pub fn expand_penetration_polytope(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    seed: [PenetrationVertex; 4],
    tolerances: [f32; 3],
) -> Result<PenetrationPolytope> {
    let mut faces =
        penetration_initial_faces(std::array::from_fn(|i| seed[i].difference))?.to_vec();
    let mut vertices = Vec::from(seed);
    let mut classes = vec![0; 4];
    let mut free = Vec::new();
    loop {
        // Strict comparison retains the first face on ties, including retired slots.
        let mut selected = 0;
        for i in 1..faces.len() {
            if faces[i].distance < faces[selected].distance {
                selected = i;
            }
        }
        let candidate = penetration_support(a, b, b_in_a, faces[selected].normal)?;
        let difference = candidate.difference;
        vertices.push(candidate);
        let check = check_penetration_candidate(
            difference,
            faces[selected].vertices.map(|i| vertices[i].difference),
            faces[selected].normal,
            tolerances,
        )?;
        let status = match check {
            PenetrationCandidateCheck::Converged => Some(PenetrationStatus::Converged),
            PenetrationCandidateCheck::Retry { .. } => Some(PenetrationStatus::Retry),
            PenetrationCandidateCheck::Expand { lower, upper } => {
                classes[selected] = 1;
                let mut horizon = penetration_horizon(
                    &mut faces,
                    &vertices,
                    &mut classes,
                    selected,
                    difference,
                    &mut [lower, upper],
                )?;
                if horizon.retry {
                    Some(PenetrationStatus::Retry)
                } else if horizon.boundary_count
                    > 100 - faces.len() + free.len() + horizon.removed.len()
                {
                    Some(PenetrationStatus::Capacity)
                } else if !replace_penetration_faces(
                    &mut faces,
                    &vertices,
                    &mut classes,
                    &mut free,
                    &mut horizon,
                    vertices.len() - 1,
                )? {
                    Some(PenetrationStatus::Retry)
                } else if vertices.len() >= 55 {
                    Some(PenetrationStatus::Capacity)
                } else {
                    None
                }
            }
        };
        if let Some(status) = status {
            // Keep the index: replacement may have reused this selected slot.
            return Ok(PenetrationPolytope {
                vertices,
                faces,
                selected_face: selected,
                status,
            });
        }
    }
}
#[derive(Debug)]
pub struct PenetrationHorizon {
    pub removed: Vec<usize>,
    pub boundary_count: usize,
    pub last_boundary: Option<(usize, usize)>,
    pub outgoing: Vec<Option<(usize, usize)>>,
    pub retry: bool,
}
/// DOL 802e94a0: reuse removed faces last-first, then append slots, and
/// walk the vertex-linked boundary starting at the last recorded edge.
/// False is native retry status2; partial mutation on that path is retained.
pub fn replace_penetration_faces(
    faces: &mut Vec<PenetrationFace>,
    vertices: &[PenetrationVertex],
    classes: &mut Vec<u8>,
    free: &mut Vec<usize>,
    horizon: &mut PenetrationHorizon,
    candidate: usize,
) -> Result<bool> {
    ensure!(
        faces.len() == classes.len()
            && candidate < vertices.len()
            && horizon.outgoing.len() == vertices.len(),
        "Invalid penetration replacement input"
    );
    ensure!(
        free.iter()
            .chain(horizon.removed.iter())
            .all(|&i| i < faces.len()),
        "Invalid penetration free face"
    );
    if horizon.retry {
        return Ok(false);
    }
    free.append(&mut horizon.removed);
    if horizon.boundary_count > 100usize.saturating_sub(faces.len()) + free.len() {
        return Ok(false);
    }
    let mut slots = Vec::with_capacity(horizon.boundary_count);
    for _ in 0..horizon.boundary_count {
        if let Some(index) = free.pop() {
            slots.push(index);
        } else {
            slots.push(faces.len());
            faces.push(PenetrationFace {
                vertices: [0; 3],
                opposite: [(0, 0); 3],
                normal: [0.; 4],
                distance: 0.,
            });
            classes.push(0);
        }
    }
    let first = horizon.last_boundary;
    let mut current = first;
    for i in 0..slots.len() {
        let Some((owner, edge)) = current else {
            return Ok(false);
        };
        ensure!(
            owner < faces.len() && edge < 3,
            "Invalid penetration boundary edge"
        );
        let origin = faces[owner].vertices[edge];
        let target = faces[owner].vertices[(edge + 1) % 3];
        ensure!(
            origin < vertices.len() && target < vertices.len(),
            "Invalid penetration boundary vertex"
        );
        let slot = slots[i];
        let next = slots[(i + 1) % slots.len()];
        let previous = slots[(i + slots.len() - 1) % slots.len()];
        faces[slot].vertices = [target, origin, candidate];
        faces[slot].opposite = [(owner, edge), (previous, 2), (next, 1)];
        faces[owner].opposite[edge] = (slot, 0);
        let points = [
            vertices[target].difference,
            vertices[origin].difference,
            vertices[candidate].difference,
        ];
        let a: [f32; 4] = std::array::from_fn(|j| points[0][j] - points[1][j]);
        let b: [f32; 4] = std::array::from_fn(|j| points[1][j] - points[2][j]);
        let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
        let x = a[1].mul_add(b[2], -(b[1] * a[2]));
        let z = -a[1].mul_add(b[0], -(b[1] * a[0]));
        if z.mul_add(z, x * x) + y * y == 0. {
            return Ok(false);
        }
        (faces[slot].normal, faces[slot].distance) = penetration_plane(points)?;
        current = horizon.outgoing[target].take();
    }
    classes.fill(0);
    horizon.outgoing.fill(None);
    Ok(current == first)
}
/// DOL 802e92dc. Face classes are 0 unknown, 1 visible, 2 boundary, 4
/// invalid interval. Start is already classified visible by candidate check.
/// The caller resets these marks before each candidate. Native removal order
/// is postorder; boundary edges point into the surviving adjacent faces.
pub fn penetration_horizon(
    faces: &mut [PenetrationFace],
    vertices: &[PenetrationVertex],
    classes: &mut [u8],
    start: usize,
    candidate: [f32; 4],
    interval: &mut [f32; 2],
) -> Result<PenetrationHorizon> {
    ensure!(
        faces.len() == classes.len() && start < faces.len() && classes[start] == 1,
        "Invalid penetration horizon start"
    );
    ensure!(
        candidate
            .iter()
            .chain(interval.iter())
            .all(|v| v.is_finite()),
        "Nonfinite penetration horizon input"
    );
    for face in faces.iter() {
        ensure!(
            face.vertices.iter().all(|&i| i < vertices.len())
                && face.opposite.iter().all(|&(f, e)| f < faces.len() && e < 3),
            "Invalid penetration topology reference"
        );
    }
    ensure!(
        classes.iter().all(|v| matches!(v, 0 | 1 | 2 | 4)),
        "Invalid penetration face class"
    );
    let mut result = PenetrationHorizon {
        removed: Vec::new(),
        boundary_count: 0,
        last_boundary: None,
        outgoing: vec![None; vertices.len()],
        retry: false,
    };
    fn visit(
        index: usize,
        faces: &mut [PenetrationFace],
        vertices: &[PenetrationVertex],
        classes: &mut [u8],
        candidate: [f32; 4],
        interval: &mut [f32; 2],
        out: &mut PenetrationHorizon,
    ) -> Result<()> {
        for edge in 0..3 {
            let (other, opposite) = faces[index].opposite[edge];
            let mut class = classes[other];
            if class == 0 {
                let face = &faces[other];
                let point = vertices[face.vertices[0]].difference;
                let d: [f32; 4] = std::array::from_fn(|i| candidate[i] - point[i]);
                let gap =
                    d[2].mul_add(face.normal[2], d[0] * face.normal[0]) + d[1] * face.normal[1];
                ensure!(gap.is_finite(), "Penetration horizon projection overflow");
                class = 1;
                if gap > interval[0] {
                    if gap < interval[1] {
                        interval[1] = gap;
                    }
                } else {
                    let shifted = f32::from_bits(0x33d6bf95) + gap;
                    if shifted > interval[0] {
                        interval[0] = shifted;
                    }
                    class = 2;
                }
                if interval[0] >= interval[1] {
                    class = 4;
                }
                classes[other] = class;
                if class == 1 {
                    visit(other, faces, vertices, classes, candidate, interval, out)?;
                    if out.retry {
                        break;
                    }
                } else if class == 4 {
                    out.retry = true;
                    break;
                }
            }
            if class == 2 {
                let boundary = (other, opposite);
                out.outgoing[faces[other].vertices[opposite]] = Some(boundary);
                out.boundary_count += 1;
                out.last_boundary = Some(boundary);
            }
        }
        faces[index].distance = f32::from_bits(0x7f7fffee);
        // This is the original free-face capacity check, not an invented cap.
        if out.removed.len() >= 100 {
            out.retry = true;
        } else {
            out.removed.push(index);
        }
        Ok(())
    }
    visit(
        start,
        faces,
        vertices,
        classes,
        candidate,
        interval,
        &mut result,
    )?;
    Ok(result)
}
#[derive(Debug, PartialEq)]
pub enum PenetrationCandidateCheck {
    Converged,
    Expand {
        lower: f32,
        upper: f32,
    },
    /// Native face classification 2 or 4 leads to status3/retry.
    Retry {
        classification: u8,
        lower: f32,
        upper: f32,
    },
}
/// DOL 802e6adc..802e6cf0: candidate acceptance before topology mutation.
/// The three packed projections use X/Y/Z FMA order, while the subsequent
/// interval check recomputes vertex0 projection with Z-into-X then Y.
pub fn check_penetration_candidate(
    candidate: [f32; 4],
    face_vertices: [[f32; 4]; 3],
    normal: [f32; 4],
    tolerances: [f32; 3],
) -> Result<PenetrationCandidateCheck> {
    ensure!(
        candidate
            .iter()
            .chain(face_vertices.iter().flatten())
            .chain(normal.iter())
            .chain(tolerances.iter())
            .all(|v| v.is_finite()),
        "Nonfinite penetration candidate input"
    );
    let deltas = face_vertices.map(|v| std::array::from_fn::<_, 4, _>(|i| candidate[i] - v[i]));
    let projections =
        deltas.map(|d| d[2].mul_add(normal[2], d[1].mul_add(normal[1], d[0] * normal[0])));
    ensure!(
        projections.iter().all(|v| v.is_finite()),
        "Penetration projection overflow"
    );
    if (0..3).any(|i| projections[i] < tolerances[i]) {
        return Ok(PenetrationCandidateCheck::Converged);
    }
    let mut lower = f32::from_bits(0x33d6bf95);
    let mut upper = f32::from_bits(0x7f7fffee);
    let d = deltas[0];
    let gap = d[2].mul_add(normal[2], d[0] * normal[0]) + d[1] * normal[1];
    ensure!(gap.is_finite(), "Penetration interval overflow");
    let mut classification = 1;
    if gap > lower {
        if gap < upper {
            upper = gap;
        }
    } else {
        let shifted = lower + gap;
        if shifted > lower {
            lower = shifted;
        }
        classification = 2;
    }
    if lower >= upper {
        classification = 4;
    }
    Ok(if classification == 1 {
        PenetrationCandidateCheck::Expand { lower, upper }
    } else {
        PenetrationCandidateCheck::Retry {
            classification,
            lower,
            upper,
        }
    })
}
fn penetration_plane(vertices: [[f32; 4]; 3]) -> Result<([f32; 4], f32)> {
    // 802e9a98: cross(v0-v1,v1-v2), one estimate/refinement, dot(v0,N).
    let a: [f32; 4] = std::array::from_fn(|i| vertices[0][i] - vertices[1][i]);
    let b: [f32; 4] = std::array::from_fn(|i| vertices[1][i] - vertices[2][i]);
    let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
    let cross = [
        a[1].mul_add(b[2], -(b[1] * a[2])),
        y,
        -a[1].mul_add(b[0], -(b[1] * a[0])),
        y,
    ];
    let mut normal = normalized_separation(cross, vertices[0])?;
    let distance = normal[3];
    normal[3] = normal[1];
    Ok((normal, distance))
}
/// DOL 802e9844..802e9a94, table8067c980. Builds four initial faces after
/// the penetration initializer has produced four valid difference vertices.
/// Retains native edge order, reciprocal links and orientation correction.
pub fn penetration_initial_faces(vertices: [[f32; 4]; 4]) -> Result<[PenetrationFace; 4]> {
    ensure!(
        vertices.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite penetration tetrahedron"
    );
    let indices = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    let opposite = [
        [(2, 2), (3, 0), (1, 0)],
        [(0, 2), (3, 2), (2, 0)],
        [(1, 2), (3, 1), (0, 0)],
        [(0, 1), (2, 1), (1, 1)],
    ];
    let mut faces = std::array::from_fn(|i| PenetrationFace {
        vertices: indices[i],
        opposite: opposite[i],
        normal: [0.; 4],
        distance: 0.,
    });
    let (normal, _) = penetration_plane(indices[3].map(|i| vertices[i]))?;
    let delta: [f32; 4] = std::array::from_fn(|i| vertices[0][i] - vertices[1][i]);
    let side = delta[2].mul_add(normal[2], delta[0] * normal[0]) + delta[1] * normal[1];
    ensure!(side.is_finite(), "Penetration orientation overflow");
    if side > 0. {
        for face in &mut faces {
            for vertex in &mut face.vertices {
                if *vertex < 2 {
                    *vertex = 1 - *vertex;
                }
            }
        }
    }
    for face in &mut faces {
        (face.normal, face.distance) = penetration_plane(face.vertices.map(|i| vertices[i]))?;
    }
    Ok(faces)
}
/// DOL 802e7a88..802e7bd0. Unlike ordinary GSK expansion, this queries A
/// along the face normal and B along its negative transformed into B space.
/// Both calls occur, in A/B order, for each candidate polytope vertex.
pub fn penetration_support(
    a: &mut dyn SupportShape,
    b: &mut dyn SupportShape,
    b_in_a: &super::native_hk_transform::Transform,
    face_normal: [f32; 4],
) -> Result<PenetrationVertex> {
    ensure!(
        face_normal
            .iter()
            .chain(b_in_a.iter())
            .all(|v| v.is_finite()),
        "Nonfinite penetration support input"
    );
    let (_, point_a) = a.support(face_normal)?;
    let search_b = transpose_direction(b_in_a, face_normal.map(|v| -v));
    let (_, point_b) = b.support(search_b)?;
    let transformed_b = super::native_hk_transform::transform_point(b_in_a, point_b);
    let difference = std::array::from_fn(|i| point_a[i] - transformed_b[i]);
    ensure!(
        point_a
            .iter()
            .chain(point_b.iter())
            .chain(difference.iter())
            .all(|v| v.is_finite()),
        "Penetration support overflow"
    );
    Ok(PenetrationVertex {
        difference,
        point_a,
        point_b,
    })
}
/// The penetration routine returns three paired vertices, with repeated IDs.
/// Native rlwinm mask keeps bits c0ffffff, not just the low ID halfword.
/// Preserve its ordered copy-from-last behavior rather than stable dedup.
fn collapse_fallback_feature(vertices: &mut [[f32; 4]; 4]) -> usize {
    let key = |v: [f32; 4]| v[3].to_bits() & 0xc0ffffff;
    let mut count = 3;
    if key(vertices[1]) == key(vertices[2]) || key(vertices[0]) == key(vertices[2]) {
        count = 2;
    }
    if key(vertices[0]) == key(vertices[1]) {
        count -= 1;
        vertices[0] = vertices[count];
    }
    count
}
#[derive(Debug, PartialEq, Eq)]
pub enum FallbackFeatures {
    /// Native return1 sets both counts to one, without retransformation.
    Singleton,
    /// Counts fit a cache; a 2/2 pair still needs its edge witness calculated.
    CacheSized,
    /// 802ecd18 must reduce the combined count before cache publication.
    NeedsCombinedReduction,
}
#[derive(Debug)]
pub struct SeparationCheck {
    pub length_squared: f32,
    pub projection: f32,
    pub support_threshold: f32,
    pub requires_fallback: bool,
}
/// DOL 802eb8a0..802eb92c. The outer loop starts its tolerance at
/// 8067ca58 (2edbe6ff) and doubles it before every reduction, including
/// the first. A failed reduction always takes the fallback branch.
pub fn check_separation(
    direction: [f32; 4],
    delta: [f32; 4],
    iteration_tolerance: f32,
    reduction_failed: bool,
) -> Result<SeparationCheck> {
    ensure!(
        direction.iter().chain(delta.iter()).all(|v| v.is_finite())
            && iteration_tolerance.is_finite()
            && iteration_tolerance >= 0.,
        "Invalid native GSK separation input"
    );
    let length_squared = direction[2].mul_add(direction[2], direction[0] * direction[0])
        + direction[1] * direction[1];
    let projection =
        delta[2].mul_add(direction[2], delta[0] * direction[0]) + delta[1] * direction[1];
    let support_threshold = iteration_tolerance * length_squared;
    let signed_square = projection * projection.abs();
    ensure!(
        [length_squared, projection, support_threshold, signed_square]
            .iter()
            .all(|v| v.is_finite()),
        "Native GSK separation overflow"
    );
    Ok(SeparationCheck {
        length_squared,
        projection,
        support_threshold,
        requires_fallback: reduction_failed
            || signed_square <= support_threshold
            || length_squared < iteration_tolerance,
    })
}
/// DOL 802ebea0..802ebf4c: one Gekko estimate/refinement, followed by
/// signed projected distance in W. Do not use sqrt or normalize() here.
pub fn normalized_separation(direction: [f32; 4], delta: [f32; 4]) -> Result<[f32; 4]> {
    ensure!(
        direction.iter().chain(delta.iter()).all(|v| v.is_finite()),
        "Nonfinite GSK normalization input"
    );
    let mut result = normalize_four_lanes(direction)?;
    result[3] = delta[2].mul_add(result[2], delta[0] * result[0]) + delta[1] * result[1];
    ensure!(
        result.iter().all(|v| v.is_finite()),
        "GSK normalized separation overflow"
    );
    Ok(result)
}
/// 801664d4 shares the estimate/refinement with GSK but scales W as well.
fn normalize_four_lanes(direction: [f32; 4]) -> Result<[f32; 4]> {
    ensure!(
        direction.iter().all(|v| v.is_finite()),
        "Nonfinite vector normalization input"
    );
    let length = direction[2].mul_add(direction[2], direction[0] * direction[0])
        + direction[1] * direction[1];
    ensure!(length.is_finite(), "GSK normalization length overflow");
    let factor = if length == 0. {
        // frsqrte(0) -> infinity; the refinement produces NaN, and fsel
        // selects the zero double at8067ca68. Preserve that resulting scale.
        0.
    } else {
        let estimate = super::pose_blend::estimate(length);
        let squared = estimate * estimate;
        let half = 0.5 * estimate;
        let residual = -length.mul_add(squared, -1.);
        let refined = residual.mul_add(half, estimate);
        if refined >= 0. {
            refined
        } else {
            0.
        }
    };
    let result = direction.map(|v| v * factor);
    ensure!(
        result.iter().all(|v| v.is_finite()),
        "GSK normalized separation overflow"
    );
    Ok(result)
}
/// 802eba08..802ebaf0 and the corresponding B/reversed-order branches.
/// The first vertex requires strict improvement; subsequent vertices accept
/// equality. A triangle tests vertex2 before vertex1.
fn support_improves(
    candidate: [f32; 4],
    vertices: &[[f32; 4]],
    direction: [f32; 4],
    threshold: f32,
) -> Result<bool> {
    ensure!(
        (1..=3).contains(&vertices.len()) && threshold.is_finite() && threshold >= 0.,
        "Invalid GSK expansion threshold or count"
    );
    ensure!(
        candidate
            .iter()
            .chain(vertices.iter().flatten())
            .chain(direction.iter())
            .all(|v| v.is_finite()),
        "Nonfinite GSK support candidate"
    );
    for index in [0, 2, 1] {
        if index >= vertices.len() {
            continue;
        }
        let d: [f32; 4] = std::array::from_fn(|i| candidate[i] - vertices[index][i]);
        let projection = d[2].mul_add(direction[2], d[0] * direction[0]) + d[1] * direction[1];
        let signed_square = projection * projection.abs();
        ensure!(
            signed_square.is_finite(),
            "GSK support improvement overflow"
        );
        if (index == 0 && signed_square <= threshold) || (index != 0 && signed_square < threshold) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// DOL 802ea6a8..802eaa40. Selects one of the three new faces formed by
/// vertex 3. None is native -1, which enters the failure/penetration path.
/// The signed scale is +1 for a B tetrahedron, -1 for A. The caller's r22
/// is the constant table8067ca30, not mutable solver context.
pub fn tetrahedron_face_to_remove(
    point: [f32; 4],
    vertices: [[f32; 4]; 4],
    scale: f32,
) -> Result<Option<usize>> {
    ensure!(
        point
            .iter()
            .chain(vertices.iter().flatten())
            .all(|v| v.is_finite())
            && scale.is_finite(),
        "Nonfinite native GSK tetrahedron input"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<_, 4, _>(|i| a[i] - b[i]);
    let cross = |a: [f32; 4], b: [f32; 4]| {
        let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
        [
            a[1].mul_add(b[2], -(b[1] * a[2])),
            y,
            -a[1].mul_add(b[0], -(b[1] * a[0])),
            y,
        ]
    };
    let normals = [
        cross(sub(vertices[2], vertices[1]), sub(vertices[3], vertices[1])),
        cross(sub(vertices[0], vertices[2]), sub(vertices[3], vertices[2])),
        cross(sub(vertices[1], vertices[0]), sub(vertices[3], vertices[0])),
    ];
    let delta = sub(point, vertices[3]).map(|v| v * scale);
    // Packed projections use X then Y then Z; normal lengths use Z into X,
    // followed by Y. Preserve the distinct native summation orders.
    let mut projections =
        normals.map(|n| n[2].mul_add(delta[2], n[1].mul_add(delta[1], n[0] * delta[0])));
    let mut scores = [0.; 3];
    for i in 0..3 {
        let n = normals[i];
        let length = n[2].mul_add(n[2], n[0] * n[0]) + n[1] * n[1];
        scores[i] = if length == 0. {
            f32::from_bits(0x7f7fffee)
        } else {
            projections[i] * projections[i].abs() / length
        };
    }
    ensure!(
        scores
            .iter()
            .chain(projections.iter())
            .all(|v| v.is_finite()),
        "Native GSK tetrahedron arithmetic overflow"
    );
    let mut largest = 1;
    if scores[0] > scores[1] {
        scores.swap(0, 1);
        largest = 0;
    }
    if scores[1] > scores[2] {
        scores.swap(1, 2);
        if scores[0] > scores[1] {
            scores.swap(0, 1);
        }
    } else {
        largest = 2;
    }
    if scores[2] < f32::from_bits(0x358637be) {
        return Ok(None);
    }
    if scores[2] > f32::from_bits(0x3f8ccccd) * scores[1] {
        return Ok(Some(largest));
    }
    // Close scores use 802ea074's ordered elimination, not max-by-score.
    // Each mutation changes the eligibility of subsequent iterations.
    let adjacent = [2usize, 0, 1, 2, 0];
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    for i in 0..3 {
        let first = adjacent[i];
        let second = adjacent[i + 2];
        if projections[second] >= 0. && projections[first] >= 0. {
            let a = sub(vertices[first], vertices[i]);
            let b = sub(vertices[second], vertices[i]);
            let p = sub(point, vertices[i]);
            let v = sub(vertices[3], vertices[i]);
            let product = dot(p, a) * dot(v, b);
            let determinant = dot(p, b).mul_add(dot(v, a), -product);
            ensure!(
                determinant.is_finite(),
                "Native GSK tetrahedron elimination overflow"
            );
            projections[if determinant < 0. { first } else { second }] = -1.;
        }
    }
    Ok(projections.iter().position(|v| *v > 0.))
}
/// Result of the recovered iterative core, before caller-specific radius handling.
pub enum QueryResult {
    Contact(CoreWitness),
    Unresolved,
}
impl Features {
    /// Compose reduction, alternating support searches and the common epilogue.
    /// Initial features must already be loaded/transformed. No guessed retry
    /// padding or iteration limit is introduced.
    pub fn query_core(
        &mut self,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
        penetration_enabled: bool,
        difference_scratch: &mut [[f32; 4]; 4],
        initial_retry_padding: Option<f32>,
    ) -> Result<QueryResult> {
        self.query_core_with_cache_flag(
            a,
            b,
            b_in_a,
            penetration_enabled,
            difference_scratch,
            initial_retry_padding,
            &mut 0,
        )
    }
    /// Preserve native GSK +14 dirty state: OR the signed dimension delta
    /// after reduction; accepted support and completed fallback assign one.
    /// Caller owns initialization and decides whether/when to publish.
    pub fn query_core_with_cache_flag(
        &mut self,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
        penetration_enabled: bool,
        difference_scratch: &mut [[f32; 4]; 4],
        initial_retry_padding: Option<f32>,
        cache_flag: &mut u32,
    ) -> Result<QueryResult> {
        ensure!(
            (1..=3).contains(&self.count_a)
                && (1..=3).contains(&self.count_b)
                && self.count_a + self.count_b <= 4,
            "Query requires cache-sized initial features"
        );
        let mut tolerance = f32::from_bits(0x2edbe6ff);
        let mut iteration = 0u32;
        let mut previous_face = None;
        let mut multi_feature_point = [0.; 4];
        loop {
            tolerance *= 2.;
            iteration = iteration.wrapping_add(1);
            ensure!(
                tolerance.is_finite(),
                "Native GSK iteration tolerance overflow"
            );
            let previous_total = self.count_a + self.count_b;
            let reduction = self.reduce_features(previous_face.as_ref())?;
            *cache_flag |=
                ((self.count_a + self.count_b) as u32).wrapping_sub(previous_total as u32);
            let reduced = match reduction {
                ExpandedReduction::Reduced(result) => Some(result),
                ExpandedReduction::NativeFailure => None,
            };
            let fallback = if let Some(result) = reduced.as_ref() {
                let delta =
                    std::array::from_fn(|i| self.vertices_a[0][i] - self.vertices_b_in_a[0][i]);
                check_separation(result.direction, delta, tolerance, false)?.requires_fallback
            } else {
                true
            };
            if fallback {
                if !penetration_enabled {
                    self.finish_without_penetration()?;
                    return Ok(QueryResult::Unresolved);
                }
                let (solved, edge) =
                    self.solve_fallback(a, b, b_in_a, difference_scratch, initial_retry_padding)?;
                *cache_flag = 1;
                if let Some(edge) = edge {
                    multi_feature_point = edge.point_a;
                }
                return Ok(QueryResult::Contact(
                    self.finish_contact(solved.normal, multi_feature_point)?,
                ));
            }
            let result = reduced.expect("successful separation requires reduction");
            if let Some(edge) = result.edge_witness {
                multi_feature_point = edge.point_a;
            }
            let delta = std::array::from_fn(|i| self.vertices_a[0][i] - self.vertices_b_in_a[0][i]);
            let threshold =
                check_separation(result.direction, delta, tolerance, false)?.support_threshold;
            if self
                .expand_support(
                    a,
                    b,
                    b_in_a,
                    result.direction,
                    threshold,
                    iteration & 1 != 0,
                    result.blocked_orientation,
                )?
                .is_none()
            {
                return Ok(QueryResult::Contact(
                    self.finish_contact(result.direction, multi_feature_point)?,
                ));
            }
            *cache_flag = 1;
            previous_face = result.face;
        }
    }
}
impl Features {
    /// 802eb950..802eb994: when penetration handling is disabled, return
    /// native status4 with cache-sized counts. Source slots are not moved.
    /// This is an unresolved query, never a successful separating contact.
    pub fn finish_without_penetration(&mut self) -> Result<()> {
        ensure!(
            (1..=4).contains(&self.count_a)
                && (1..=4).contains(&self.count_b)
                && self.count_a + self.count_b <= 5,
            "Invalid unresolved GSK dimensions"
        );
        if self.count_a + self.count_b > 4 {
            if self.count_a > self.count_b {
                self.count_a = 3;
                self.count_b = 1;
            } else {
                self.count_a = 1;
                self.count_b = 3;
            }
        }
        Ok(())
    }
    /// Join native fallback preparation, penetration solve, and source-feature
    /// restoration. Difference scratch is caller-owned, including inactive slots.
    /// The returned penetration normal is not replaced by the edge witness.
    pub fn solve_fallback(
        &mut self,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
        difference_scratch: &mut [[f32; 4]; 4],
        initial_retry_padding: Option<f32>,
    ) -> Result<(PenetrationSolve, Option<EdgePairWitness>)> {
        let prepared = self.prepare_fallback_seed()?;
        difference_scratch[..prepared.count]
            .copy_from_slice(&prepared.differences[..prepared.count]);
        let mut seed = PenetrationSeed::from_slots(
            std::array::from_fn(|i| PenetrationVertex {
                difference: difference_scratch[i],
                point_a: self.vertices_a[i],
                point_b: self.vertices_b[i],
            }),
            prepared.count,
        )?;
        // 802ec9f0 loads 8067ca70, independently of iteration tolerance.
        let solved = solve_penetration_with_retry_padding(
            a,
            b,
            b_in_a,
            &mut seed,
            f32::from_bits(0x38d1b717),
            initial_retry_padding,
        );
        // Native arrays alias the solver context, including mutations before an
        // unsuccessful attempt. Copy every slot, not just the active seed.
        for i in 0..4 {
            difference_scratch[i] = seed.slots[i].difference;
            self.vertices_a[i] = seed.slots[i].point_a;
            self.vertices_b[i] = seed.slots[i].point_b;
        }
        let solved = solved?;
        let edge = self.finish_fallback_features(solved.terminal_fallback, b_in_a)?;
        Ok((solved, edge))
    }
    /// Complete feature restoration after 802e7d6c returns. The caller must
    /// supply that solver's output arrays and retain its returned normal.
    /// Recomputes the 2/2 witness without applying ordinary endpoint reduction.
    pub fn finish_fallback_features(
        &mut self,
        singleton_result: bool,
        b_in_a: &super::native_hk_transform::Transform,
    ) -> Result<Option<EdgePairWitness>> {
        match self.restore_fallback_features(singleton_result, b_in_a)? {
            FallbackFeatures::Singleton => return Ok(None),
            FallbackFeatures::NeedsCombinedReduction => self.reduce_combined_fallback()?,
            FallbackFeatures::CacheSized => {}
        }
        if self.count_a == 2 && self.count_b == 2 {
            let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
            return Ok(Some(edge_pair(
                self.vertices_a[0],
                sub(self.vertices_a[1], self.vertices_a[0]),
                self.vertices_b_in_a[0],
                sub(self.vertices_b_in_a[1], self.vertices_b_in_a[0]),
            )?));
        }
        Ok(None)
    }
    /// DOL 802ecd18..802ed05c: fallback outputs with combined count > 4.
    /// Prefer the first interior point (A against B first), otherwise search
    /// edge pairs in source order and retain the first strictly best distance.
    /// No ordinary feature reducer is substituted: this path keeps both edge
    /// endpoints even when the closest witness lies at an endpoint.
    pub fn reduce_combined_fallback(&mut self) -> Result<()> {
        ensure!(
            matches!((self.count_a, self.count_b), (2, 3) | (3, 2) | (3, 3)),
            "Invalid combined fallback dimensions"
        );
        if self.count_b == 3 {
            let face = self.face_vertices(true);
            for i in 0..self.count_a {
                if classify_face_mode(self.vertices_a[i], face, None)?.mask == 14 {
                    self.vertices_a[0] = self.vertices_a[i];
                    self.count_a = 1;
                    return Ok(());
                }
            }
        }
        if self.count_a == 3 {
            let face = self.face_vertices(false);
            for i in 0..self.count_b {
                if classify_face_mode(self.vertices_b_in_a[i], face, None)?.mask == 14 {
                    self.vertices_b[0] = self.vertices_b[i];
                    self.vertices_b_in_a[0] = self.vertices_b_in_a[i];
                    self.count_b = 1;
                    return Ok(());
                }
            }
        }
        let edges_a = if self.count_a == 3 { 3 } else { 1 };
        let edges_b = if self.count_b == 3 { 3 } else { 1 };
        let adjacent = [2usize, 0, 1, 2, 0];
        let mut best = f32::from_bits(0x7f7fffee);
        let mut selected = (0, 0);
        let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
        for a in 0..edges_a {
            for b in 0..edges_b {
                let witness = edge_pair(
                    self.vertices_a[a],
                    sub(self.vertices_a[adjacent[a + 2]], self.vertices_a[a]),
                    self.vertices_b_in_a[b],
                    sub(
                        self.vertices_b_in_a[adjacent[b + 2]],
                        self.vertices_b_in_a[b],
                    ),
                )?;
                if witness.squared_distance < best {
                    best = witness.squared_distance;
                    selected = (a, b);
                }
            }
        }
        if edges_a == 3 {
            self.remove_face_vertex(false, adjacent[selected.0])?;
        }
        if edges_b == 3 {
            self.remove_face_vertex(true, adjacent[selected.1])?;
        }
        Ok(())
    }
    /// DOL 802eca10..802ecc60, immediately after the penetration routine.
    /// Input arrays must contain that routine's actual output, not seed data.
    pub fn restore_fallback_features(
        &mut self,
        singleton_result: bool,
        b_in_a: &super::native_hk_transform::Transform,
    ) -> Result<FallbackFeatures> {
        if singleton_result {
            self.count_a = 1;
            self.count_b = 1;
            return Ok(FallbackFeatures::Singleton);
        }
        ensure!(
            self.vertices_a[..3]
                .iter()
                .flatten()
                .chain(self.vertices_b[..3].iter().flatten())
                .chain(b_in_a.iter())
                .all(|v| v.is_finite()),
            "Nonfinite native fallback result"
        );
        self.count_a = collapse_fallback_feature(&mut self.vertices_a);
        self.count_b = collapse_fallback_feature(&mut self.vertices_b);
        // Original B-in-A slots are rebuilt after source compaction, from
        // the last active vertex backward. Inactive slots remain untouched.
        for i in (0..self.count_b).rev() {
            self.vertices_b_in_a[i] =
                super::native_hk_transform::transform_point(b_in_a, self.vertices_b[i]);
        }
        ensure!(
            self.vertices_b_in_a[..self.count_b]
                .iter()
                .flatten()
                .all(|v| v.is_finite()),
            "Fallback transform overflow"
        );
        Ok(if self.count_a + self.count_b > 4 {
            FallbackFeatures::NeedsCombinedReduction
        } else {
            FallbackFeatures::CacheSized
        })
    }
    /// DOL 802ec68c..802ec9e8. Repeats feature vertices in the original
    /// pair order without changing either feature count. The subsequent
    /// penetration routine owns count reconstruction and final reduction.
    pub fn prepare_fallback_seed(&mut self) -> Result<FallbackSeed> {
        ensure!(
            (1..=4).contains(&self.count_a)
                && (1..=4).contains(&self.count_b)
                && self.count_a + self.count_b <= 5,
            "Invalid native GSK fallback dimensions"
        );
        let count;
        if self.count_b >= 3 {
            count = if self.count_b == 3 && self.count_a == 1 {
                3
            } else {
                4
            };
            if self.count_b > 3 {
                self.vertices_a[3] = self.vertices_a[0];
            } else if self.count_a > 1 {
                self.vertices_a[3] = self.vertices_a[1];
                self.vertices_b[3] = self.vertices_b[0];
                self.vertices_b_in_a[3] = self.vertices_b_in_a[0];
            }
            self.vertices_a[1] = self.vertices_a[0];
            self.vertices_a[2] = self.vertices_a[0];
        } else if self.count_a >= 3 {
            count = if self.count_a == 3 && self.count_b == 1 {
                3
            } else {
                4
            };
            if self.count_a > 3 {
                self.vertices_b[3] = self.vertices_b[0];
                self.vertices_b_in_a[3] = self.vertices_b_in_a[0];
            } else if self.count_b > 1 {
                self.vertices_a[3] = self.vertices_a[0];
                self.vertices_b[3] = self.vertices_b[1];
                self.vertices_b_in_a[3] = self.vertices_b_in_a[1];
            }
            for i in [1, 2] {
                self.vertices_b[i] = self.vertices_b[0];
                self.vertices_b_in_a[i] = self.vertices_b_in_a[0];
            }
        } else {
            count = 1;
            for i in [self.count_a, self.count_a + 1] {
                self.vertices_a[i] = self.vertices_a[0];
            }
            for i in [self.count_b, self.count_b + 1] {
                self.vertices_b[i] = self.vertices_b[0];
                self.vertices_b_in_a[i] = self.vertices_b_in_a[0];
            }
        }
        let mut differences = [[0.; 4]; 4];
        for i in 0..count {
            differences[i] = std::array::from_fn(|axis| {
                self.vertices_a[i][axis] - self.vertices_b_in_a[i][axis]
            });
        }
        ensure!(
            differences[..count].iter().flatten().all(|v| v.is_finite()),
            "Native GSK fallback difference overflow"
        );
        Ok(FallbackSeed { count, differences })
    }
    /// DOL 802eb998..802ebea0. Odd iterations try A then B; even iterations
    /// try B then A. threshold is f31 (iteration tolerance * direction length
    /// squared). blocked_orientation is scratch+174, reset each iteration.
    /// None means neither support expanded, not a completed distance query.
    pub fn expand_support(
        &mut self,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
        direction: [f32; 4],
        threshold: f32,
        odd_iteration: bool,
        blocked_orientation: i32,
    ) -> Result<Option<ExpandedShape>> {
        ensure!(
            (1..=3).contains(&self.count_a)
                && (1..=3).contains(&self.count_b)
                && self.count_a + self.count_b <= 4,
            "GSK expansion requires reduced features"
        );
        ensure!(
            matches!(blocked_orientation, -8 | 0 | 8),
            "Invalid GSK blocked orientation"
        );
        ensure!(
            direction.iter().chain(b_in_a.iter()).all(|v| v.is_finite())
                && threshold.is_finite()
                && threshold >= 0.,
            "Invalid GSK expansion input"
        );
        let order = if odd_iteration {
            [ExpandedShape::A, ExpandedShape::B]
        } else {
            [ExpandedShape::B, ExpandedShape::A]
        };
        for side in order {
            match side {
                ExpandedShape::A => {
                    if self.count_a == self.max_dimension_a as usize || blocked_orientation == 8 {
                        continue;
                    }
                    let search = direction.map(|v| -v);
                    let (_, point) = a.support(search)?;
                    // Native support writes the inactive slot even if rejected.
                    self.vertices_a[self.count_a] = point;
                    if support_improves(point, &self.vertices_a[..self.count_a], search, threshold)?
                    {
                        self.count_a += 1;
                        return Ok(Some(side));
                    }
                }
                ExpandedShape::B => {
                    if self.count_b == self.max_dimension_b as usize || blocked_orientation == -8 {
                        continue;
                    }
                    let search = transpose_direction(b_in_a, direction);
                    let (_, point) = b.support(search)?;
                    self.vertices_b[self.count_b] = point;
                    if support_improves(point, &self.vertices_b[..self.count_b], search, threshold)?
                    {
                        let transformed =
                            super::native_hk_transform::transform_point(b_in_a, point);
                        ensure!(
                            transformed.iter().all(|v| v.is_finite()),
                            "GSK expanded transform overflow"
                        );
                        self.vertices_b_in_a[self.count_b] = transformed;
                        self.count_b += 1;
                        return Ok(Some(side));
                    }
                }
            }
        }
        Ok(None)
    }
    /// DOL 802eb7dc..802eb884: remove the selected vertex by copying the
    /// newly added fourth vertex over it, then resume point/face reduction.
    pub fn reduce_point_tetrahedron(&mut self) -> Result<ExpandedReduction> {
        let (point, tetrahedron, point_is_a) = match (self.count_a, self.count_b) {
            (1, 4) => (self.vertices_a[0], self.vertices_b_in_a, true),
            (4, 1) => (self.vertices_b_in_a[0], self.vertices_a, false),
            _ => anyhow::bail!("GSK point/tetrahedron requires dimensions 1/4 or 4/1"),
        };
        // 8067ca54 = +1 (B); 8067ca44 = -1 (A).
        let scale = if point_is_a { 1. } else { -1. };
        let Some(remove) = tetrahedron_face_to_remove(point, tetrahedron, scale)? else {
            return Ok(ExpandedReduction::NativeFailure);
        };
        if point_is_a {
            self.count_b = 3;
            self.vertices_b_in_a[remove] = self.vertices_b_in_a[3];
            self.vertices_b[remove] = self.vertices_b[3];
        } else {
            self.count_a = 3;
            self.vertices_a[remove] = self.vertices_a[3];
        }
        let result = self.reduce_point_face()?;
        Ok(ExpandedReduction::Reduced(FeatureReduction {
            blocked_orientation: 0,
            direction: result.direction,
            face: result.face,
            edge_witness: None,
        }))
    }
    fn face_vertices(&self, point_is_a: bool) -> [[f32; 4]; 3] {
        let vertices = if point_is_a {
            &self.vertices_b_in_a
        } else {
            &self.vertices_a
        };
        [vertices[0], vertices[1], vertices[2]]
    }
    /// Full recovered feature-case dispatch. The caller must carry the
    /// previous face state; None means the native fresh-plane path.
    /// This reduces features only, without support expansion/convergence.
    pub fn reduce_features(
        &mut self,
        previous_face: Option<&FaceClassification>,
    ) -> Result<ExpandedReduction> {
        match feature_case(self.count_a, self.count_b) {
            FeatureCase::EdgeFace => self.reduce_edge_face(previous_face),
            FeatureCase::PointTetrahedron => self.reduce_point_tetrahedron(),
            _ => Ok(ExpandedReduction::Reduced(self.reduce_current_features()?)),
        }
    }
    /// DOL 802eaa44..802eb020. None selects scratch+170 == 2 (rebuild
    /// the plane); Some supplies the preceding face normal and weights.
    /// This state is required and cannot be inferred from feature counts.
    pub fn reduce_edge_face(
        &mut self,
        previous: Option<&FaceClassification>,
    ) -> Result<ExpandedReduction> {
        ensure!(
            matches!((self.count_a, self.count_b), (2, 3) | (3, 2)),
            "GSK edge/face requires dimensions 2/3 or 3/2"
        );
        let edge_is_a = self.count_a == 2;
        let mut edge = if edge_is_a {
            [self.vertices_a[0], self.vertices_a[1]]
        } else {
            [self.vertices_b_in_a[0], self.vertices_b_in_a[1]]
        };
        let triangle = self.face_vertices(edge_is_a);
        let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<_, 4, _>(|i| a[i] - b[i]);
        let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
        let face_edges = [sub(triangle[1], triangle[2]), sub(triangle[0], triangle[2])];
        let normal = if let Some(prior) = previous {
            ensure!(
                prior
                    .direction
                    .iter()
                    .chain(prior.weights.iter())
                    .all(|v| v.is_finite()),
                "Nonfinite prior GSK face state"
            );
            prior.direction
        } else {
            let a = face_edges[0];
            let b = face_edges[1];
            let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
            [
                a[1].mul_add(b[2], -(b[1] * a[2])),
                y,
                -a[1].mul_add(b[0], -(b[1] * a[0])),
                y,
            ]
        };
        let sides = edge.map(|v| dot(normal, sub(v, triangle[0])));
        ensure!(
            sides.iter().all(|v| v.is_finite()),
            "GSK edge/face side overflow"
        );
        // Fresh path compares signs separately. Cached path compares their
        // rounded product: underflow and an exact zero must retain that rule.
        let crosses = if previous.is_some() {
            sides[0] * sides[1] < 0.
        } else {
            (sides[0] < 0.) != (sides[1] < 0.)
        };
        if crosses {
            let t = sides[0] / (sides[0] - sides[1]);
            let complement = 1. - t;
            let point = std::array::from_fn(|i| t.mul_add(edge[1][i], edge[0][i] * complement));
            if classify_face_mode(point, triangle, None)?.mask == 14 {
                return Ok(ExpandedReduction::NativeFailure);
            }
        }
        let remove = if let Some(prior) = previous {
            // Classification-only call preserves scratch+120 and winding.
            let mut face = classify_face_mode(edge[1], triangle, None)?;
            if face.mask == 14 {
                if edge_is_a {
                    self.vertices_a[0] = self.vertices_a[1];
                    self.count_a = 1;
                } else {
                    self.vertices_b[0] = self.vertices_b[1];
                    self.vertices_b_in_a[0] = self.vertices_b_in_a[1];
                    self.count_b = 1;
                }
                face.direction = prior.direction;
                return Ok(ExpandedReduction::Reduced(FeatureReduction {
                    blocked_orientation: if edge_is_a { 8 } else { -8 },
                    direction: face.direction,
                    face: Some(face),
                    edge_witness: None,
                }));
            }
            let w = face.weights;
            let old = prior.weights;
            // Original uses two rounded products, not a fused determinant.
            match face.mask {
                6 => 0,
                10 => 1,
                12 => 2,
                8 => {
                    if (w[1] - old[1]) * old[2] > (w[2] - old[2]) * old[1] {
                        2
                    } else {
                        1
                    }
                }
                4 => {
                    if (w[2] - old[2]) * old[0] > (w[0] - old[0]) * old[2] {
                        0
                    } else {
                        2
                    }
                }
                2 => {
                    if (w[0] - old[0]) * old[1] > (w[1] - old[1]) * old[0] {
                        1
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        } else {
            if !crosses {
                if sides[0].abs() > sides[1].abs() {
                    edge.swap(0, 1);
                    if edge_is_a {
                        self.vertices_a.swap(0, 1);
                    } else {
                        self.vertices_b.swap(0, 1);
                        self.vertices_b_in_a.swap(0, 1);
                    }
                }
                let face = classify_face(edge[0], triangle, edge_is_a)?;
                if face.face_reversed {
                    if edge_is_a {
                        self.vertices_b.swap(0, 1);
                        self.vertices_b_in_a.swap(0, 1);
                    } else {
                        self.vertices_a.swap(0, 1);
                    }
                }
                if face.mask == 14 {
                    if edge_is_a {
                        self.count_a = 1;
                    } else {
                        self.count_b = 1;
                    }
                    return Ok(ExpandedReduction::Reduced(FeatureReduction {
                        blocked_orientation: 0,
                        direction: face.direction,
                        face: Some(face),
                        edge_witness: None,
                    }));
                }
            }
            let delta = sub(edge[1], edge[0]);
            let first = edge_pair(edge[0], delta, triangle[2], face_edges[0])?;
            let second = edge_pair(edge[0], delta, triangle[2], face_edges[1])?;
            usize::from(first.squared_distance > second.squared_distance)
        };
        self.remove_face_vertex(edge_is_a, remove)?;
        let (witness, direction) = self.finish_edge_pair()?;
        Ok(ExpandedReduction::Reduced(FeatureReduction {
            blocked_orientation: 0,
            direction,
            face: None,
            edge_witness: Some(witness),
        }))
    }
    /// Dispatch persistent-cache feature cases in original shape order.
    /// Expanded cases use reduce_features, which carries previous face state
    /// and preserves the separate native failure/penetration result.
    pub fn reduce_current_features(&mut self) -> Result<FeatureReduction> {
        let mut face = None;
        let mut edge_witness = None;
        let direction=match feature_case(self.count_a,self.count_b) {
            FeatureCase::PointPoint=>std::array::from_fn(|i|self.vertices_a[0][i]-self.vertices_b_in_a[0][i]),
            FeatureCase::PointEdge=>self.reduce_point_edge()?,
            FeatureCase::PointFace=>{let result=self.reduce_point_face()?;face=result.face;result.direction},
            FeatureCase::EdgeEdge=>{let (witness,direction)=self.finish_edge_pair()?;edge_witness=Some(witness);direction},
            FeatureCase::EdgeFace=>anyhow::bail!("Native GSK edge/face requires reduce_features with previous face state"),
            FeatureCase::PointTetrahedron=>anyhow::bail!("Native GSK point/tetrahedron requires reduce_features to preserve native failure status"),
            FeatureCase::Failure=>anyhow::bail!("Native GSK rejects feature dimensions {}/{}",self.count_a,self.count_b),
        };
        ensure!(
            direction.iter().all(|v| v.is_finite()),
            "Nonfinite native GSK search direction"
        );
        Ok(FeatureReduction {
            blocked_orientation: 0,
            direction,
            face,
            edge_witness,
        })
    }
    /// Direct edge/edge branch802eb6d0..802eb7d8. Jump-table entry2/2
    /// reaches this directly; 802eaa44 belongs to expanded edge/face cases.
    /// This does not include simplex expansion or the GSK convergence loop.
    pub fn finish_edge_pair(&mut self) -> Result<(EdgePairWitness, [f32; 4])> {
        ensure!(
            self.count_a == 2 && self.count_b == 2,
            "GSK edge pair requires dimensions 2/2"
        );
        let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<_, 4, _>(|i| a[i] - b[i]);
        let da = sub(self.vertices_a[1], self.vertices_a[0]);
        let db = sub(self.vertices_b_in_a[1], self.vertices_b_in_a[0]);
        let witness = edge_pair(self.vertices_a[0], da, self.vertices_b_in_a[0], db)?;
        let mask = witness.endpoint_mask;
        let direction = if mask == 0 {
            let y = -da[0].mul_add(db[2], -(db[0] * da[2]));
            let mut normal = [
                da[1].mul_add(db[2], -(db[1] * da[2])),
                y,
                -da[1].mul_add(db[0], -(db[1] * da[0])),
                y,
            ];
            let d = sub(self.vertices_a[0], self.vertices_b_in_a[0]);
            let side = normal[2].mul_add(d[2], normal[0] * d[0]) + normal[1] * d[1];
            ensure!(side.is_finite(), "GSK edge pair normal overflow");
            if side < 0. {
                normal = normal.map(|v| -v);
            }
            normal
        } else {
            if mask & 1 != 0 {
                self.vertices_a[0] = self.vertices_a[1];
                self.count_a = 1;
            } else if mask & 2 != 0 {
                self.count_a = 1;
            }
            if mask & 4 != 0 {
                self.vertices_b[0] = self.vertices_b[1];
                self.vertices_b_in_a[0] = self.vertices_b_in_a[1];
                self.count_b = 1;
            } else if mask & 8 != 0 {
                self.count_b = 1;
            }
            if self.count_a == 1 && self.count_b == 1 {
                sub(self.vertices_a[0], self.vertices_b_in_a[0])
            } else {
                self.reduce_point_edge()?
            }
        };
        Ok((witness, direction))
    }
    /// DOL 802eb274..802eb620: complete point/triangle reduction dispatch.
    /// Face weights are retained only on the interior path; exterior paths
    /// reduce counts and continue through the point/edge or point/point case.
    pub fn reduce_point_face(&mut self) -> Result<PointFaceReduction> {
        let point_is_a = self.count_a == 1;
        let face = self.classify_point_face()?;
        if face.mask == 14 {
            return Ok(PointFaceReduction {
                direction: face.direction,
                face: Some(face),
            });
        }
        // Exact signed-byte lookup from DOL 80674f98.
        const REGION: [i8; 16] = [-1, -1, 2, -1, 1, -1, -8, -1, 0, -1, -7, -1, -6, -1, -1, -1];
        let region = REGION[face.mask as usize];
        if region < 0 {
            let candidate = (region + 8) as usize;
            let remove = if candidate > 3 { 1 } else { candidate };
            self.remove_face_vertex(point_is_a, remove)?;
        } else {
            let vertex = region as usize;
            // DOL 80674fa8, read at vertex and vertex+2.
            let adjacent = [2usize, 0, 1, 2, 0];
            let first = adjacent[vertex];
            let mut second = adjacent[vertex + 2];
            let triangle = self.face_vertices(point_is_a);
            let point = if point_is_a {
                self.vertices_a[0]
            } else {
                self.vertices_b_in_a[0]
            };
            let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<_, 4, _>(|i| a[i] - b[i]);
            let delta = sub(point, triangle[vertex]);
            let dot = |v: [f32; 4]| delta[2].mul_add(v[2], delta[0] * v[0]) + delta[1] * v[1];
            let first_side = dot(sub(triangle[first], triangle[vertex]));
            let second_side = dot(sub(triangle[second], triangle[vertex]));
            ensure!(
                first_side.is_finite() && second_side.is_finite(),
                "GSK vertex region overflow"
            );
            if first_side < 0. {
                self.remove_face_vertex(point_is_a, first)?;
                if second_side < 0. {
                    if second == 2 {
                        second = first;
                    }
                    self.remove_face_vertex(point_is_a, second)?;
                }
            } else {
                self.remove_face_vertex(point_is_a, second)?;
            }
        }
        let direction = if self.count_a == 1 && self.count_b == 1 {
            std::array::from_fn(|i| self.vertices_a[0][i] - self.vertices_b_in_a[0][i])
        } else {
            self.reduce_point_edge()?
        };
        Ok(PointFaceReduction {
            direction,
            face: None,
        })
    }
    fn remove_face_vertex(&mut self, point_is_a: bool, index: usize) -> Result<()> {
        let count = if point_is_a {
            self.count_b
        } else {
            self.count_a
        };
        ensure!(
            (2..=3).contains(&count) && index < count,
            "Invalid GSK feature removal"
        );
        let last = count - 1;
        if point_is_a {
            self.vertices_b[index] = self.vertices_b[last];
            self.vertices_b_in_a[index] = self.vertices_b_in_a[last];
            self.count_b = last;
        } else {
            self.vertices_a[index] = self.vertices_a[last];
            self.count_a = last;
        }
        Ok(())
    }
    /// DOL 802ea290, called by point/face dispatch. A non-14 mask still
    /// requires the original edge/vertex reduction dispatch, not acceptance.
    pub fn classify_point_face(&mut self) -> Result<FaceClassification> {
        let (point, triangle, point_is_a) = match (self.count_a, self.count_b) {
            (1, 3) => (self.vertices_a[0], self.face_vertices(true), true),
            (3, 1) => (self.vertices_b_in_a[0], self.face_vertices(false), false),
            _ => anyhow::bail!("GSK point/face classification requires dimensions 1/3 or 3/1"),
        };
        let result = classify_face(point, triangle, point_is_a)?;
        if result.face_reversed {
            if point_is_a {
                self.vertices_b.swap(0, 1);
                self.vertices_b_in_a.swap(0, 1);
            } else {
                self.vertices_a.swap(0, 1);
            }
        }
        Ok(result)
    }
    /// Point/edge cases of the GSK dispatch (802eb23c/258). Reduces the
    /// active endpoint count and keeps original/transformed B IDs together.
    /// Returns the unnormalized search direction, not a solved distance.
    pub fn reduce_point_edge(&mut self) -> Result<[f32; 4]> {
        let (point, edge, point_is_a) = match (self.count_a, self.count_b) {
            (1, 2) => (
                self.vertices_a[0],
                [self.vertices_b_in_a[0], self.vertices_b_in_a[1]],
                true,
            ),
            (2, 1) => (
                self.vertices_b_in_a[0],
                [self.vertices_a[0], self.vertices_a[1]],
                false,
            ),
            _ => anyhow::bail!("GSK point/edge reduction requires dimensions 1/2 or 2/1"),
        };
        let result = point_edge(point, edge, point_is_a)?;
        if let Some(endpoint) = result.endpoint {
            if point_is_a {
                self.vertices_b[0] = self.vertices_b[endpoint];
                self.vertices_b_in_a[0] = self.vertices_b_in_a[endpoint];
                self.count_b = 1;
            } else {
                self.vertices_a[0] = self.vertices_a[endpoint];
                self.count_a = 1;
            }
        }
        Ok(result.direction)
    }
    pub fn load(
        cache: &Cache,
        a: &mut dyn SupportShape,
        b: &mut dyn SupportShape,
        b_in_a: &super::native_hk_transform::Transform,
    ) -> Result<Self> {
        let (ids_a, ids_b) = cache.features()?;
        ensure!(
            b_in_a.iter().all(|v| v.is_finite()),
            "Nonfinite native GSK relative transform"
        );
        let mut result = Self {
            count_a: ids_a.len() / 2,
            count_b: ids_b.len() / 2,
            max_dimension_a: cache.0[10] >> 4,
            max_dimension_b: cache.0[10] & 15,
            vertices_a: [[0.; 4]; 4],
            vertices_b: [[0.; 4]; 4],
            vertices_b_in_a: [[0.; 4]; 4],
        };
        // Original converts all A IDs, then all B IDs, before transforming B.
        for (i, id) in ids_a.chunks_exact(2).enumerate() {
            result.vertices_a[i] = a.vertex(u16::from_be_bytes(id.try_into()?))?;
        }
        for (i, id) in ids_b.chunks_exact(2).enumerate() {
            result.vertices_b[i] = b.vertex(u16::from_be_bytes(id.try_into()?))?;
        }
        for i in (0..result.count_b).rev() {
            result.vertices_b_in_a[i] =
                super::native_hk_transform::transform_point(b_in_a, result.vertices_b[i]);
        }
        ensure!(
            result.vertices_a[..result.count_a]
                .iter()
                .flatten()
                .chain(result.vertices_b[..result.count_b].iter().flatten())
                .chain(result.vertices_b_in_a[..result.count_b].iter().flatten())
                .all(|v| v.is_finite()),
            "Nonfinite native GSK feature vertex"
        );
        Ok(result)
    }
    /// 802ec1f8 extracts IDs from source W, never transformed B W.
    pub fn publish_cache(&self, cache: &mut Cache) -> Result<()> {
        ensure!(
            (1..=3).contains(&self.count_a)
                && (1..=3).contains(&self.count_b)
                && self.count_a + self.count_b <= 4,
            "Cannot publish unreduced GSK features"
        );
        let a: [u16; 3] = std::array::from_fn(|i| self.vertices_a[i][3].to_bits() as u16);
        let b: [u16; 3] = std::array::from_fn(|i| self.vertices_b[i][3].to_bits() as u16);
        cache.update(&a[..self.count_a], &b[..self.count_b])
    }
    pub fn witness(
        &self,
        separating: [f32; 4],
        multi_feature_point: [f32; 4],
    ) -> Result<CoreWitness> {
        ensure!(
            self.count_a <= 3 && self.count_b <= 3,
            "Invalid loaded GSK feature dimensions"
        );
        core_witness(
            &self.vertices_a[..self.count_a],
            &self.vertices_b_in_a[..self.count_b],
            separating,
            multi_feature_point,
        )
    }
    /// Common query epilogue 802ebea0..802ebf4c, followed by witness
    /// selection. Always project the restored first pair, including after
    /// penetration fallback; the fallback's independently computed depth is
    /// not the final separating distance. Preserve existing transformed B
    /// scratch on the terminal singleton path, as the original does.
    pub fn finish_contact(
        &self,
        direction: [f32; 4],
        multi_feature_point: [f32; 4],
    ) -> Result<CoreWitness> {
        ensure!(
            (1..=3).contains(&self.count_a)
                && (1..=3).contains(&self.count_b)
                && self.count_a + self.count_b <= 4,
            "Cannot finish unreduced GSK contact"
        );
        let delta = std::array::from_fn(|i| self.vertices_a[0][i] - self.vertices_b_in_a[0][i]);
        let separating = normalized_separation(direction, delta)?;
        self.witness(separating, multi_feature_point)
    }
}

#[derive(Debug)]
pub struct FaceClassification {
    pub mask: u8,
    pub weights: [f32; 4],
    pub direction: [f32; 4],
    pub face_reversed: bool,
}
pub struct PointFaceReduction {
    pub direction: [f32; 4],
    pub face: Option<FaceClassification>,
}
fn classify_face(
    point: [f32; 4],
    triangle: [[f32; 4]; 3],
    point_is_a: bool,
) -> Result<FaceClassification> {
    classify_face_mode(point, triangle, Some(point_is_a))
}
// r6 == 0 updates weights only and must not reverse the face. The returned
// geometric direction is not a replacement for the caller's saved normal.
fn classify_face_mode(
    point: [f32; 4],
    triangle: [[f32; 4]; 3],
    orientation: Option<bool>,
) -> Result<FaceClassification> {
    ensure!(
        point
            .iter()
            .chain(triangle.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite GSK face input"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let cross = |a: [f32; 4], b: [f32; 4]| {
        let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
        [
            a[1].mul_add(b[2], -(b[1] * a[2])),
            y,
            -a[1].mul_add(b[0], -(b[1] * a[0])),
            y,
        ]
    };
    let edges = [
        sub(triangle[1], triangle[0]),
        sub(triangle[2], triangle[1]),
        sub(triangle[0], triangle[2]),
    ];
    let mut direction = cross(edges[1], edges[2]);
    // Scratch +160/+164/+168 corresponds to edges 1,2,0 respectively.
    let mut weights = [
        dot(cross(sub(point, triangle[1]), edges[1]), direction),
        dot(cross(sub(point, triangle[2]), edges[2]), direction),
        dot(cross(sub(point, triangle[0]), edges[0]), direction),
        0.,
    ];
    ensure!(
        weights
            .iter()
            .chain(direction.iter())
            .all(|v| v.is_finite()),
        "GSK face arithmetic overflow"
    );
    let mask = (if weights[0] < 0. { 8 } else { 0 })
        | (if weights[1] < 0. { 4 } else { 0 })
        | (if weights[2] < 0. { 2 } else { 0 });
    let delta = if orientation.unwrap_or(true) {
        sub(point, triangle[0])
    } else {
        sub(triangle[0], point)
    };
    let side = dot(direction, delta);
    ensure!(side.is_finite(), "GSK face side overflow");
    let face_reversed = orientation.is_some() && mask == 14 && side < 0.;
    if face_reversed {
        direction = direction.map(|v| -v);
        weights.swap(0, 1);
    }
    Ok(FaceClassification {
        mask,
        weights,
        direction,
        face_reversed,
    })
}

struct PointEdgeResult {
    direction: [f32; 4],
    endpoint: Option<usize>,
}
/// DOL 802ebfb4..802ec1e8. Endpoint tests are strict <0. Interior uses
/// frsqrte followed by frsp without Newton refinement, then two cross products.
fn point_edge(point: [f32; 4], edge: [[f32; 4]; 2], point_is_a: bool) -> Result<PointEdgeResult> {
    ensure!(
        point
            .iter()
            .chain(edge.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite GSK point/edge input"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let cross = |a: [f32; 4], b: [f32; 4]| {
        let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
        [
            a[1].mul_add(b[2], -(b[1] * a[2])),
            y,
            -a[1].mul_add(b[0], -(b[1] * a[0])),
            y,
        ]
    };
    let d = sub(edge[1], edge[0]);
    let v = sub(edge[1], point);
    let u = sub(edge[0], point);
    let first = dot(v, d);
    let second = dot(u, d);
    ensure!(
        first.is_finite() && second.is_finite(),
        "GSK point/edge projection overflow"
    );
    let (direction, endpoint) = if (first < 0.) == (second < 0.) {
        let endpoint = usize::from(first < 0.);
        (
            if point_is_a {
                sub(point, edge[endpoint])
            } else {
                sub(edge[endpoint], point)
            },
            Some(endpoint),
        )
    } else {
        let squared = dot(d, d);
        ensure!(
            squared > 0. && squared.is_finite(),
            "Invalid GSK edge length"
        );
        let inverse = super::pose_blend::estimate(squared);
        let unit = d.map(|v| v * inverse);
        let perpendicular = cross(v, u);
        (
            if point_is_a {
                cross(unit, perpendicular)
            } else {
                cross(perpendicular, unit)
            },
            None,
        )
    };
    ensure!(
        direction.iter().all(|v| v.is_finite()),
        "GSK point/edge direction overflow"
    );
    Ok(PointEdgeResult {
        direction,
        endpoint,
    })
}

/// DOL 802ec330..802ec3b0. B vertices must already be transformed into
/// A's frame. Multi-feature point is the original GSK scratch value at +130,
/// which must be produced by the solver, not an average of feature vertices.
pub fn core_witness(
    vertices_a: &[[f32; 4]],
    vertices_b_in_a: &[[f32; 4]],
    separating: [f32; 4],
    multi_feature_point: [f32; 4],
) -> Result<CoreWitness> {
    ensure!(
        (1..=3).contains(&vertices_a.len())
            && (1..=3).contains(&vertices_b_in_a.len())
            && vertices_a.len() + vertices_b_in_a.len() <= 4,
        "Invalid native GSK witness dimensions"
    );
    ensure!(
        separating.iter().all(|v| v.is_finite()),
        "Nonfinite native GSK separating vector"
    );
    let point_a = if vertices_a.len() == 1 {
        vertices_a[0]
    } else if vertices_b_in_a.len() == 1 {
        std::array::from_fn(|i| separating[i].mul_add(separating[3], vertices_b_in_a[0][i]))
    } else {
        multi_feature_point
    };
    ensure!(
        point_a.iter().all(|v| v.is_finite()),
        "Nonfinite native GSK core witness"
    );
    Ok(CoreWitness {
        normal: separating,
        point_a,
        distance: separating[3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn convex_support_preserves_padding_and_block_ties() -> Result<()> {
        let mut hull = ConvexSupport {
            packed_vertices: vec![[1., 0., 0.]; 8],
            vertex_count: 5,
            stored_center: [2., 3., 4., 5.],
        };
        assert_eq!(hull.sphere_count()?, 5);
        assert_eq!(hull.center()?, [2., 3., 4., 5.]);
        assert_eq!(hull.support([1., 0., 0., 0.])?.0, 3);
        hull.packed_vertices[7] = [2., 0., 0.];
        let (id, point) = hull.support([1., 0., 0., 0.])?;
        assert_eq!(id, 7);
        assert_eq!(point, [2., 0., 0., f32::from_bits(0x3f000007)]);
        assert_eq!(hull.vertex(7)?, point);
        assert!(hull.vertex(8).is_err());
        assert_eq!(hull.support([0.; 4])?.0, 3);
        Ok(())
    }

    #[test]
    fn query_core_runs_real_triangle_support_and_unresolved_path() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let triangle = [[0., 0., 0., 0.], [2., 0., 0., 0.], [0., 2., 0., 0.]];
        let mut a = TriangleSupport {
            vertices: triangle.map(|mut v| {
                v[2] = 2.;
                v
            }),
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices: triangle,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut cache = Cache([0; 12]);
        cache.initialize_convex(&mut a, &mut b, &transform)?;
        let mut features = Features::load(&cache, &mut a, &mut b, &transform)?;
        let QueryResult::Contact(contact) =
            features.query_core(&mut a, &mut b, &transform, false, &mut [[0.; 4]; 4], None)?
        else {
            panic!("separated triangles unresolved")
        };
        assert!(contact.distance > 1.999 && contact.distance <= 2.);
        assert_eq!(contact.normal[0], 0.);
        assert_eq!(contact.normal[1], 0.);
        features.publish_cache(&mut cache)?;
        let mut repeated = Features::load(&cache, &mut a, &mut b, &transform)?;
        let mut dirty = 0;
        assert!(matches!(
            repeated.query_core_with_cache_flag(
                &mut a,
                &mut b,
                &transform,
                false,
                &mut [[0.; 4]; 4],
                None,
                &mut dirty
            )?,
            QueryResult::Contact(_)
        ));
        assert_eq!(dirty, 0, "Stable feature cache should not be republished");
        dirty = 0x80;
        repeated.query_core_with_cache_flag(
            &mut a,
            &mut b,
            &transform,
            false,
            &mut [[0.; 4]; 4],
            None,
            &mut dirty,
        )?;
        assert_eq!(
            dirty, 0x80,
            "Query must preserve incoming dirty state when no update occurs"
        );
        // Coincident point features must take explicit unresolved status when
        // penetration is disabled, rather than fabricate a zero normal contact.
        features.count_a = 1;
        features.count_b = 1;
        features.vertices_a[0] = features.vertices_b_in_a[0];
        assert!(matches!(
            features.query_core(&mut a, &mut b, &transform, false, &mut [[0.; 4]; 4], None)?,
            QueryResult::Unresolved
        ));
        Ok(())
    }

    #[test]
    #[ignore = "requires original executable capture in BLOTWORX_GSK_REFERENCE"]
    fn original_gsk_capture_replays_bit_exact() -> Result<()> {
        use anyhow::Context;
        use serde_json::Value;
        fn words(v: &Value) -> Result<Vec<u32>> {
            v.as_array()
                .context("Missing captured words")?
                .iter()
                .map(|v| {
                    u32::try_from(v.as_u64().context("Invalid captured word")?).map_err(Into::into)
                })
                .collect()
        }
        fn vector(words: &[u32], offset: usize) -> Result<[f32; 4]> {
            let raw = words
                .get(offset / 4..offset / 4 + 4)
                .context("Truncated captured vector")?;
            Ok(std::array::from_fn(|i| f32::from_bits(raw[i])))
        }
        fn shape(v: &Value) -> Result<Box<dyn SupportShape>> {
            let h = words(&v["header"])?;
            ensure!(h.len() == 24, "Invalid shape capture header");
            match h[0] {
                0x806b88e0 => Ok(Box::new(TriangleSupport {
                    vertices: [vector(&h, 0x20)?, vector(&h, 0x30)?, vector(&h, 0x40)?],
                    extrusion: vector(&h, 0x50)?,
                    extrusion_flag: h[0x14 / 4] as u8,
                })),
                0x806b8740 => {
                    let packed = words(&v["packed_words"])?;
                    ensure!(
                        h[19] > 0
                            && h[19] <= 65536
                            && h[17] == h[19].div_ceil(4)
                            && packed.len() == h[17] as usize * 12,
                        "Invalid captured convex lanes"
                    );
                    let mut vertices = Vec::new();
                    for block in packed.chunks_exact(12) {
                        for lane in 0..4 {
                            vertices.push(std::array::from_fn(|axis| {
                                f32::from_bits(block[axis * 4 + lane])
                            }));
                        }
                    }
                    Ok(Box::new(ConvexSupport {
                        packed_vertices: vertices,
                        vertex_count: h[19],
                        stored_center: vector(&h, 0x30)?,
                    }))
                }
                _ => anyhow::bail!("Unsupported captured shape"),
            }
        }
        let path =
            std::env::var_os("BLOTWORX_GSK_REFERENCE").context("Set BLOTWORX_GSK_REFERENCE")?;
        let capture: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            capture["complete"] == true && capture["mode"] == "gsk",
            "Incomplete GSK reference capture"
        );
        ensure!(
            capture["dol_sha256"]
                == "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920",
            "Unverified DOL identity"
        );
        let samples = capture["samples"]
            .as_array()
            .context("Missing GSK samples")?;
        ensure!(!samples.is_empty(), "Empty GSK reference capture");
        for (index, sample) in samples.iter().enumerate() {
            let input = words(&sample["features"])?;
            let expected = words(&sample["result_features"])?;
            ensure!(
                input.len() == 96 && expected.len() == 96,
                "Invalid GSK context size"
            );
            let matrix = words(&sample["transform"])?;
            ensure!(matrix.len() == 16, "Invalid captured transform");
            let matrix = std::array::from_fn(|i| f32::from_bits(matrix[i]));
            let mut features = Features {
                count_a: input[0] as usize,
                count_b: input[1] as usize,
                max_dimension_a: u8::try_from(input[2])?,
                max_dimension_b: u8::try_from(input[3])?,
                vertices_a: [[0.; 4]; 4],
                vertices_b: [[0.; 4]; 4],
                vertices_b_in_a: [[0.; 4]; 4],
            };
            ensure!(
                (1..=3).contains(&features.count_b),
                "Invalid captured B count"
            );
            for i in 0..4 {
                features.vertices_a[i] = vector(&input, 0x20 + i * 16)?;
                features.vertices_b[i] = vector(&input, 0xe0 + i * 16)?;
                features.vertices_b_in_a[i] = vector(&input, 0xa0 + i * 16)?;
            }
            for i in 0..features.count_b {
                features.vertices_b_in_a[i] = super::super::native_hk_transform::transform_point(
                    &matrix,
                    features.vertices_b[i],
                );
            }
            let mut scratch = [[0.; 4]; 4];
            if sample["fallback_entered"]
                .as_bool()
                .context("Capture lacks fallback observation")?
            {
                let captured = words(&sample["difference_scratch"])?;
                for (i, v) in scratch.iter_mut().enumerate() {
                    *v = vector(&captured, i * 16)?;
                }
            } // Otherwise the original never reads these fallback-only slots.
            let mut a = shape(&sample["shape_a"])?;
            let mut b = shape(&sample["shape_b"])?;
            let mut dirty = input[5];
            let retry_padding = sample
                .get("initial_retry_padding_bits")
                .map(|v| -> Result<f32> {
                    Ok(f32::from_bits(u32::try_from(
                        v.as_u64().context("Invalid captured retry padding")?,
                    )?))
                })
                .transpose()?;
            let result = features
                .query_core_with_cache_flag(
                    a.as_mut(),
                    b.as_mut(),
                    &matrix,
                    input[4] == 0,
                    &mut scratch,
                    retry_padding,
                    &mut dirty,
                )
                .with_context(|| format!("GSK reference sample {index}"))?;
            let status = match result {
                QueryResult::Unresolved => 4,
                QueryResult::Contact(contact) => {
                    assert_eq!(
                        contact.normal.map(f32::to_bits).as_slice(),
                        words(&sample["separating"])?,
                        "sample {index} separating"
                    );
                    0
                }
            };
            assert_eq!(sample["status"], status, "sample {index} status");
            assert_eq!(
                [features.count_a as u32, features.count_b as u32, dirty],
                [expected[0], expected[1], expected[5]],
                "sample {index} feature state"
            );
            for (vertices, count, offset) in [
                (&features.vertices_a, features.count_a, 0x20),
                (&features.vertices_b, features.count_b, 0xe0),
            ] {
                for i in 0..count {
                    assert_eq!(
                        vertices[i].map(f32::to_bits),
                        vector(&expected, offset + i * 16)?.map(f32::to_bits),
                        "sample {index} feature {offset:x}/{i}"
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn contact_epilogue_projects_restored_pair_and_selects_witness() -> Result<()> {
        let mut features = Features {
            count_a: 1,
            count_b: 1,
            max_dimension_a: 15,
            max_dimension_b: 15,
            vertices_a: [[6., 8., 0., 7.]; 4],
            vertices_b: [[99.; 4]; 4],
            vertices_b_in_a: [[0.; 4]; 4],
        };
        let singleton = features.finish_contact([3., 4., 0., 123.], [77.; 4])?;
        assert_eq!(singleton.normal, [0.6, 0.8, 0., 10.]);
        assert_eq!(singleton.distance, 10.);
        assert_eq!(singleton.point_a, features.vertices_a[0]);
        features.count_a = 2;
        let b_singleton = features.finish_contact([3., 4., 0., -456.], [77.; 4])?;
        assert_eq!(b_singleton.point_a, [6., 8., 0., 100.]);
        features.count_b = 2;
        let edges = features.finish_contact([3., 4., 0., 123.], [1., 2., 3., 4.])?;
        assert_eq!(edges.point_a, [1., 2., 3., 4.]);
        assert_eq!(edges.distance, 10.);
        features.count_a = 3;
        assert!(features.finish_contact([3., 4., 0., 0.], [0.; 4]).is_err());
        Ok(())
    }
    #[test]
    fn disabled_penetration_preserves_sources_and_reduces_only_counts() -> Result<()> {
        let points = std::array::from_fn(|i| [i as f32; 4]);
        for (a, b, expected) in [
            (1, 1, (1, 1)),
            (2, 2, (2, 2)),
            (1, 3, (1, 3)),
            (3, 1, (3, 1)),
            (2, 3, (1, 3)),
            (3, 2, (3, 1)),
            (1, 4, (1, 3)),
            (4, 1, (3, 1)),
        ] {
            let mut features = Features {
                count_a: a,
                count_b: b,
                max_dimension_a: 15,
                max_dimension_b: 15,
                vertices_a: points,
                vertices_b: points,
                vertices_b_in_a: points,
            };
            features.finish_without_penetration()?;
            assert_eq!((features.count_a, features.count_b), expected);
            assert_eq!(features.vertices_a, points);
            assert_eq!(features.vertices_b, points);
            assert_eq!(features.vertices_b_in_a, points);
        }
        Ok(())
    }
    #[test]
    fn feature_publication_uses_source_ids_and_preserves_metadata() -> Result<()> {
        let point = |id: u32| [0., 0., 0., f32::from_bits(0x3f000000 | id)];
        let mut features = Features {
            count_a: 1,
            count_b: 2,
            max_dimension_a: 15,
            max_dimension_b: 15,
            vertices_a: [point(42); 4],
            vertices_b: [point(2), point(7), point(99), point(99)],
            vertices_b_in_a: [[0.; 4]; 4],
        };
        let mut cache = Cache([0xaa; 12]);
        features.publish_cache(&mut cache)?;
        assert_eq!(cache.0, [0, 42, 0, 7, 0, 2, 0xaa, 0xaa, 1, 2, 0xaa, 0xaa]);
        let before = cache.clone();
        features.count_a = 3;
        assert!(features.publish_cache(&mut cache).is_err());
        assert_eq!(cache, before);
        Ok(())
    }

    #[test]
    fn joined_fallback_restores_terminal_features_and_preserves_scratch() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut a = Shape {
            vertex: [0.; 4],
            point: [1., 0., 0., 7.],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut b = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut features = Features {
            count_a: 1,
            count_b: 1,
            max_dimension_a: 1,
            max_dimension_b: 1,
            vertices_a: [a.point; 4],
            vertices_b: [b.point; 4],
            vertices_b_in_a: [b.point; 4],
        };
        features.vertices_a[3] = [31.; 4];
        features.vertices_b[3] = [32.; 4];
        let mut scratch = [[99.; 4]; 4];
        let (result, edge) =
            features.solve_fallback(&mut a, &mut b, &transform, &mut scratch, Some(0.))?;
        assert!(result.terminal_fallback);
        assert_eq!(result.retries, 20);
        assert!(edge.is_none());
        assert_eq!((features.count_a, features.count_b), (1, 1));
        assert_eq!(features.vertices_a[0], a.point);
        assert_eq!(features.vertices_b[0], b.point);
        assert_eq!(result.normal[3], result.distance);
        assert_eq!(result.distance, 1.);
        assert_eq!(features.vertices_a[3], [31.; 4]);
        assert_eq!(features.vertices_b[3], [32.; 4]);
        assert_eq!(scratch[3], [99.; 4]);
        Ok(())
    }
    #[test]
    fn penetration_seed_count_changes_preserve_inactive_native_scratch() -> Result<()> {
        let slots = std::array::from_fn(|i| PenetrationVertex {
            difference: [i as f32; 4],
            point_a: [10. + i as f32; 4],
            point_b: [20. + i as f32; 4],
        });
        let mut seed = PenetrationSeed::from_slots(slots, 4)?;
        seed.truncate(1);
        assert_eq!(seed.len(), 1);
        assert_eq!(seed.slots[3].point_a, [13.; 4]);
        seed.clear();
        assert!(seed.is_empty());
        assert_eq!(seed.slots[1].point_b, [21.; 4]);
        seed.push(PenetrationVertex {
            difference: [8.; 4],
            point_a: [9.; 4],
            point_b: [7.; 4],
        });
        assert_eq!(seed.slots[0].point_a, [9.; 4]);
        assert_eq!(seed.slots[2].difference, [2.; 4]);
        assert_eq!(seed.slots[3].point_b, [23.; 4]);
        Ok(())
    }
    #[test]
    fn penetration_retry_padding_accumulates_via_native_stack_writeback() -> Result<()> {
        let mut transform = [0.; 16];
        transform[15] = 0.25;
        let mut padding = 0.125;
        for expected in [0.375, 0.75, 1.5] {
            perturb_penetration_transform(&mut transform, [1., 2., 3.], &mut padding)?;
            assert_eq!(transform[15], expected);
            assert_eq!(padding, expected);
        }
        assert_eq!(transform[12..15], [3., 6., 9.]);
        assert_eq!(transform[..12], [0.; 12]);
        Ok(())
    }
    #[test]
    fn penetration_wrapper_requires_padding_and_uses_original_terminal_transform() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut a = Shape {
            vertex: [0.; 4],
            point: [1., 0., 0., 7.],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut b = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let initial = PenetrationVertex {
            difference: a.point,
            point_a: a.point,
            point_b: b.point,
        };
        let mut seed: PenetrationSeed = vec![initial.clone()].into();
        assert!(solve_penetration_with_retry_padding(
            &mut a, &mut b, &transform, &mut seed, 0., None
        )
        .is_err());
        let mut seed: PenetrationSeed = vec![initial].into();
        let result = solve_penetration_with_retry_padding(
            &mut a,
            &mut b,
            &transform,
            &mut seed,
            0.,
            Some(0.),
        )?;
        assert!(result.terminal_fallback);
        assert_eq!(result.retries, 20);
        assert_eq!(result.distance, 1.);
        assert_eq!(result.point_a, a.point);
        assert_eq!(seed.len(), 1);
        assert_eq!(seed[0].point_a, a.point);
        assert_eq!(seed[0].point_b, b.point);
        Ok(())
    }
    #[test]
    fn terminal_refinement_improves_diagonal_direction_with_native_schedule() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut a = Shape {
            vertex: [0.; 4],
            point: [1., 1., 1., 7.],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut b = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let result = penetration_terminal_fallback(&mut a, &mut b, &transform)?;
        assert!(result.distance > 1.7);
        assert_eq!(result.normal[3], result.distance);
        assert_eq!(result.vertex.point_a, a.point);
        assert_eq!(a.calls.len(), 46);
        assert_eq!(b.calls.len(), 46);
        let trials = terminal_trial_directions([1., 0., 0., 8.], 0.99)?;
        assert_eq!(trials[0][0].to_bits(), trials[1][0].to_bits());
        assert_eq!(trials[2][0].to_bits(), trials[3][0].to_bits());
        assert_eq!(trials[0][3].to_bits(), trials[1][3].to_bits());
        Ok(())
    }
    #[test]
    fn penetration_terminal_axis_search_preserves_order_sign_and_source() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut a = Shape {
            vertex: [0.; 4],
            point: [-2., 3., 1., 7.],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut b = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let result = penetration_terminal_axes(&mut a, &mut b, &transform)?;
        assert_eq!(result.distance, 3.);
        assert_eq!(result.normal, [0., 1., 0., 3.]);
        assert_eq!(result.vertex.point_a, a.point);
        assert_eq!(a.calls.len(), 6);
        assert_eq!(b.calls.len(), 6);
        assert_eq!(
            a.directions
                .iter()
                .map(|d| [d[0], d[1], d[2]])
                .collect::<Vec<_>>(),
            vec![
                [-1., 0., 0.],
                [1., 0., 0.],
                [0., -1., 0.],
                [0., 1., 0.],
                [0., 0., -1.],
                [0., 0., 1.]
            ]
        );
        a.point = [1., 1., 1., 0.];
        assert_eq!(
            penetration_terminal_axes(&mut a, &mut b, &transform)?.normal,
            [1., 0., 0., 1.]
        );
        Ok(())
    }
    #[test]
    fn penetration_retry_retains_native_limits_and_identical_axis_offsets() -> Result<()> {
        assert!(penetration_retry_action(0).is_err());
        assert_eq!(
            penetration_retry_action(1)?,
            PenetrationRetry::KeepFirstSeed
        );
        for (count, bits) in [(2, 0x36b9738d), (3, 0x38e31efb), (19, 0x392014dc)] {
            let PenetrationRetry::EmptySeed { translation_delta } =
                penetration_retry_action(count)?
            else {
                panic!("expected perturbation")
            };
            assert_eq!(translation_delta.map(f32::to_bits), [bits; 3]);
        }
        for count in 2..20 {
            assert!(matches!(
                penetration_retry_action(count)?,
                PenetrationRetry::EmptySeed { .. }
            ));
        }
        for count in [20, 21, u32::MAX] {
            assert_eq!(
                penetration_retry_action(count)?,
                PenetrationRetry::TerminalFallback
            );
        }
        Ok(())
    }
    #[test]
    fn penetration_attempt_connects_tetrahedron_to_contact_and_preserves_sources() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let points = [
            [0.; 4],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ];
        let mut seed = points
            .map(|p| PenetrationVertex {
                difference: p,
                point_a: p,
                point_b: [0.; 4],
            })
            .into_iter()
            .collect();
        let mut a = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let mut b = Shape {
            vertex: [0.; 4],
            point: [0.; 4],
            id: 0,
            count: 1,
            directions: vec![],
            calls: vec![],
        };
        let result = penetration_attempt(&mut a, &mut b, &transform, &mut seed, 0.001)?;
        let PenetrationAttempt::Contact {
            status,
            point_a,
            distance,
            ..
        } = result
        else {
            panic!("expected contact")
        };
        assert_eq!(status, PenetrationStatus::Converged);
        assert_eq!(point_a, [0.; 4]);
        assert_eq!(distance, 0.);
        assert_eq!(seed.len(), 3);
        assert!(seed
            .iter()
            .all(|v| points.contains(&v.point_a) && v.point_b == [0.; 4]));
        assert_eq!(a.calls.len(), 1);
        assert_eq!(b.calls.len(), 1);
        seed.clear();
        assert!(matches!(
            penetration_attempt(&mut a, &mut b, &transform, &mut seed, 0.001)?,
            PenetrationAttempt::Contact { .. }
        ));
        assert_eq!(seed.len(), 1);
        Ok(())
    }
    #[test]
    fn penetration_initialization_connects_restarts_and_early_contacts() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut a = TriangleSupport {
            vertices: [[-1., -1., 0., 0.], [1., -1., 0., 0.], [0., 1., 0., 0.]],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices: [[0.; 4]; 3],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut seed: PenetrationSeed = vec![PenetrationVertex {
            difference: [1., 0., 0., 0.],
            point_a: [1., 0., 0., 0.],
            point_b: [0.; 4],
        }]
        .into();
        assert!(matches!(
            initialize_penetration_seed(&mut a, &mut b, &transform, &mut seed, 0.001)?,
            PenetrationInitialization::Contact { .. }
        ));
        assert_eq!(seed.len(), 3);
        let mut seed: PenetrationSeed = vec![PenetrationVertex {
            difference: [0.; 4],
            point_a: [0.; 4],
            point_b: [0.; 4],
        }]
        .into();
        assert!(matches!(
            initialize_penetration_seed(&mut a, &mut b, &transform, &mut seed, 0.001)?,
            PenetrationInitialization::Contact { .. }
        ));
        assert_eq!(seed.len(), 3);
        a.vertices = [[0.; 4]; 3];
        seed.clear();
        let PenetrationInitialization::Contact {
            normal,
            point_a,
            distance,
        } = initialize_penetration_seed(&mut a, &mut b, &transform, &mut seed, 0.001)?
        else {
            panic!("expected empty-seed contact")
        };
        assert_eq!(normal, [0., 1., 0., 0.]);
        assert_eq!(distance, 0.);
        assert_eq!(point_a, seed[0].point_a);
        assert_eq!(seed.len(), 1);
        Ok(())
    }
    #[test]
    fn collapsed_triangle_uses_native_first_vertex_weight_and_two_sources() -> Result<()> {
        for first in [0., 2.] {
            let seed = [first, 7., 11.].map(|x| PenetrationVertex {
                difference: [x, 0., 0., 0.],
                point_a: [x + 10., 4., 6., 8.],
                point_b: [0.; 4],
            });
            let PenetrationTriangleExpansion::Contact {
                normal,
                point_a,
                distance,
            } = collapsed_penetration_triangle(&seed)?
            else {
                panic!("expected contact")
            };
            let weight = first / (f32::from_bits(0x34000000) + (first + first));
            assert_eq!(
                point_a[0],
                weight.mul_add(17., (first + 10.) * (1. - weight))
            );
            assert_eq!(point_a[1..], [4., 6., 8.]);
            assert_eq!(distance.to_bits(), 0);
            assert!(normal[2] > 0.99);
            assert_eq!(normal[0], 0.);
            assert_eq!(normal[1], 0.);
        }
        Ok(())
    }
    #[test]
    fn penetration_triangle_expansion_retains_early_contact_and_equality() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let points = [[-1., -1., 0., 0.], [1., -1., 0., 0.], [0., 1., 0., 0.]];
        for tolerance in [0.01, 0.] {
            let mut seed = points
                .map(|p| PenetrationVertex {
                    difference: p,
                    point_a: p,
                    point_b: [0.; 4],
                })
                .into_iter()
                .collect();
            let mut a = TriangleSupport {
                vertices: points,
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let mut b = TriangleSupport {
                vertices: [[0.; 4]; 3],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            match extend_penetration_triangle(&mut a, &mut b, &transform, &mut seed, tolerance)? {
                PenetrationTriangleExpansion::Contact {
                    normal,
                    point_a,
                    distance,
                } => {
                    assert!(tolerance > 0.);
                    assert_eq!(seed.len(), 3);
                    assert_eq!(point_a, [0.; 4]);
                    assert_eq!(distance.to_bits(), 0);
                    assert!(normal[2] > 0.99);
                }
                PenetrationTriangleExpansion::Expanded => {
                    assert_eq!(tolerance, 0.);
                    assert_eq!(seed.len(), 4);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn penetration_segment_expansion_adds_support_off_the_line() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        for axis in 0..3 {
            let mut direction = [0.; 4];
            direction[axis] = 1.;
            let mut seed: PenetrationSeed = vec![
                PenetrationVertex {
                    difference: direction,
                    point_a: direction,
                    point_b: [0.; 4],
                },
                PenetrationVertex {
                    difference: direction.map(|v| -v),
                    point_a: direction.map(|v| -v),
                    point_b: [0.; 4],
                },
            ]
            .into();
            let mut a = TriangleSupport {
                vertices: [[3., 0., 0., 0.], [0., 3., 0., 0.], [0., 0., 3., 0.]],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let mut b = TriangleSupport {
                vertices: [[0.; 4]; 3],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            extend_penetration_segment(&mut a, &mut b, &transform, &mut seed)?;
            assert_eq!(seed.len(), 3);
            assert!((0..3).any(|i| i != axis && seed[2].difference[i] != 0.));
            assert_eq!(
                penetration_seed_gate(
                    &seed.iter().map(|v| v.difference).collect::<Vec<_>>(),
                    0.,
                    2
                )?,
                PenetrationSeedGate::Extend
            );
        }
        Ok(())
    }
    #[test]
    fn penetration_axis_support_scores_displacement_and_retains_forward_ties() -> Result<()> {
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        for (origin, expected, score) in
            [(0., 3., 3.), (2., -2., 4.), (0.5, 3., 2.5), (10., -2., 12.)]
        {
            let mut a = TriangleSupport {
                vertices: [[-2., 0., 0., 0.], [0.; 4], [3., 0., 0., 0.]],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let mut b = TriangleSupport {
                vertices: [[0.; 4]; 3],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let (vertex, projection) = penetration_axis_support(
                &mut a,
                &mut b,
                &transform,
                [1., 0., 0., 0.],
                [origin, 0., 0., 0.],
            )?;
            assert_eq!(vertex.point_a[0], expected);
            assert_eq!(projection, score);
            assert_eq!(
                vertex.point_a[3].to_bits(),
                0x3f000000 | if expected == 3. { 2 } else { 0 }
            );
        }
        Ok(())
    }
    #[test]
    fn penetration_point_expansion_queries_both_directions_and_keeps_first_tie() -> Result<()> {
        struct Sequence {
            points: Vec<[f32; 4]>,
            directions: Vec<[f32; 4]>,
        }
        impl SupportShape for Sequence {
            fn vertex(&mut self, _: u16) -> Result<[f32; 4]> {
                unreachable!()
            }
            fn sphere_count(&mut self) -> Result<u32> {
                unreachable!()
            }
            fn support(&mut self, d: [f32; 4]) -> Result<(u16, [f32; 4])> {
                let i = self.directions.len();
                self.directions.push(d);
                Ok((i as u16, self.points[i]))
            }
        }
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        for (left, right, expected) in [(3., -1., 3.), (2., -2., -2.), (5., 0., 5.)] {
            let mut a = Sequence {
                points: vec![[left, 0., 0., 11.], [right, 0., 0., 22.]],
                directions: vec![],
            };
            let mut b = Sequence {
                points: vec![[0.; 4]; 2],
                directions: vec![],
            };
            let mut seed: PenetrationSeed = vec![PenetrationVertex {
                difference: [1., 0., 0., 7.],
                point_a: [1., 0., 0., 7.],
                point_b: [0.; 4],
            }]
            .into();
            extend_penetration_point(&mut a, &mut b, &transform, &mut seed)?;
            assert_eq!(seed.len(), 2);
            assert_eq!(seed[1].point_a[0], expected);
            assert_eq!(a.directions[0], [1., 0., 0., 7.]);
            assert_eq!(
                a.directions[1].map(f32::to_bits),
                [-1., -0., -0., -7.].map(f32::to_bits)
            );
            assert_eq!(b.directions.len(), 2);
            assert_eq!(seed[0].difference, [1., 0., 0., 7.]);
        }
        Ok(())
    }
    #[test]
    fn penetration_seed_gates_preserve_restart_history_and_exact_thresholds() -> Result<()> {
        let points = [
            [0.; 4],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ];
        assert_eq!(
            penetration_seed_gate(&points[..1], 0., -1)?,
            PenetrationSeedGate::Restart(0)
        );
        assert_eq!(
            penetration_seed_gate(&points[..1], 0., 0)?,
            PenetrationSeedGate::Retry
        );
        assert_eq!(
            penetration_seed_gate(&points[..2], 1., -1)?,
            PenetrationSeedGate::Restart(1)
        );
        assert_eq!(
            penetration_seed_gate(&points[..2], 1., 1)?,
            PenetrationSeedGate::Retry
        );
        assert_eq!(
            penetration_seed_gate(&points[..2], 0.5, 1)?,
            PenetrationSeedGate::Extend
        );
        assert_eq!(
            penetration_seed_gate(&points[..3], 100., 2)?,
            PenetrationSeedGate::Extend
        );
        assert_eq!(
            penetration_seed_gate(&[[0.; 4]; 3], 0., -1)?,
            PenetrationSeedGate::Restart(2)
        );
        assert_eq!(
            penetration_seed_gate(&[[0.; 4]; 3], 0., 2)?,
            PenetrationSeedGate::CollapsedTriangle
        );
        assert_eq!(
            penetration_seed_gate(&points, 1., -1)?,
            PenetrationSeedGate::Restart(3)
        );
        assert_eq!(
            penetration_seed_gate(&points, 1., 3)?,
            PenetrationSeedGate::Retry
        );
        assert_eq!(
            penetration_seed_gate(&points, 0.5, 3)?,
            PenetrationSeedGate::Ready
        );
        Ok(())
    }
    #[test]
    fn penetration_extraction_preserves_source_lanes_and_signed_output() -> Result<()> {
        let points = [[-1., -1., 2., 0.], [1., -1., 2., 0.], [0., 1., 2., 0.]];
        let vertices = points
            .map(|p| PenetrationVertex {
                difference: p,
                point_a: [p[0] + 4., p[1] + 8., p[2], 16.],
                point_b: [4., 8., 0., 32.],
            })
            .into_iter()
            .collect();
        let mut poly = PenetrationPolytope {
            vertices,
            faces: vec![PenetrationFace {
                vertices: [0, 1, 2],
                opposite: [(0, 0); 3],
                normal: [0., 0., 1., 0.],
                distance: 2.,
            }],
            selected_face: 0,
            status: PenetrationStatus::Converged,
        };
        let contact = poly.extract_contact()?;
        assert_eq!(contact.weights, [0.25, 0.25, 0.5]);
        assert_eq!(contact.point_a, [4., 8., 2., 16.]);
        assert_eq!(
            contact.normal.map(f32::to_bits),
            [0x80000000, 0x80000000, 0xbf800000, 0x80000000]
        );
        assert_eq!(contact.distance, -2.);
        assert_eq!(contact.source_b, [[4., 8., 0., 32.]; 3]);
        assert_eq!(contact.differences, points);
        poly.status = PenetrationStatus::Capacity;
        assert!(poly.extract_contact().is_ok());
        poly.status = PenetrationStatus::Retry;
        assert!(poly.extract_contact().is_err());
        Ok(())
    }
    #[test]
    fn penetration_contact_face_obeys_distance_gate_and_first_edge() -> Result<()> {
        let vertices = [[-1., 0., 0., 0.], [-1., 1., 0., 0.], [0.; 4]].map(|p| PenetrationVertex {
            difference: p,
            point_a: p,
            point_b: [0.; 4],
        });
        let mut faces = vec![
            PenetrationFace {
                vertices: [0, 1, 2],
                opposite: [(1, 0); 3],
                normal: [0., 0., 1., 0.],
                distance: 2.,
            },
            PenetrationFace {
                vertices: [1, 0, 2],
                opposite: [(0, 0); 3],
                normal: [0.; 4],
                distance: 1.,
            },
        ];
        assert_eq!(penetration_contact_face(&faces, &vertices, 0)?, 0);
        faces[1].distance = 1.8;
        assert_eq!(penetration_contact_face(&faces, &vertices, 0)?, 1);
        faces[0].distance = 0.;
        faces[1].distance = -3.;
        assert_eq!(penetration_contact_face(&faces, &vertices, 0)?, 1);
        assert!(penetration_contact_face(&faces, &vertices, 2).is_err());
        Ok(())
    }
    #[test]
    fn penetration_weights_keep_projection_extrapolation_and_degenerate_edge_order() -> Result<()> {
        let triangle = [[0., 0., 2., 0.], [2., 0., 2., 0.], [0., 2., 2., 0.]];
        assert_eq!(
            penetration_weights([0.5, 0.5, -4., 0.], triangle)?,
            [0.5, 0.25, 0.25]
        );
        assert_eq!(
            penetration_weights([4., 0., 0., 0.], triangle)?,
            [-1., 2., 0.]
        );
        // Each longest-edge branch, including ties and the all-collapsed result.
        for (x, expected) in [
            ([0., 1., 2.], [0.75, 0., 0.25]),
            ([0., 2., 1.], [0.75, 0.25, 0.]),
            ([1., 0., 2.], [0., 0.75, 0.25]),
            ([0., 1., 0.], [0.5, 0.5, 0.]),
        ] {
            assert_eq!(
                penetration_weights([0.5, 0., 0., 0.], x.map(|v| [v, 0., 0., 0.]))?,
                expected
            );
        }
        assert_eq!(penetration_weights([8.; 4], [[1.; 4]; 3])?, [0.; 3]);
        Ok(())
    }
    #[test]
    fn penetration_loop_expands_then_converges_and_preserves_retry() -> Result<()> {
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let points = [
            [0.; 4],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ];
        for (point, tolerance, status, minimum) in [
            ([-1., -1., -1., 0.], 0.001, PenetrationStatus::Converged, 6),
            ([0.; 4], 0.001, PenetrationStatus::Converged, 5),
            ([0.; 4], 0., PenetrationStatus::Retry, 5),
        ] {
            let mut a = Shape {
                vertex: point,
                point,
                id: 0,
                count: 1,
                directions: vec![],
                calls: vec![],
            };
            let mut b = Shape {
                vertex: [0.; 4],
                point: [0.; 4],
                id: 0,
                count: 1,
                directions: vec![],
                calls: vec![],
            };
            let seed = points.map(|p| PenetrationVertex {
                difference: p,
                point_a: p,
                point_b: [0.; 4],
            });
            let result =
                expand_penetration_polytope(&mut a, &mut b, &identity, seed, [tolerance; 3])?;
            assert_eq!(result.status, status);
            assert!(result.vertices.len() >= minimum);
            assert_eq!(a.calls.len(), result.vertices.len() - 4);
            assert_eq!(b.calls.len(), a.calls.len());
            if result.status == PenetrationStatus::Converged {
                for (i, face) in result.faces.iter().enumerate() {
                    if face.distance == f32::from_bits(0x7f7fffee) {
                        continue;
                    }
                    for (edge, &(other, opposite)) in face.opposite.iter().enumerate() {
                        assert_eq!(result.faces[other].opposite[opposite], (i, edge));
                    }
                }
            }
        }
        Ok(())
    }
    #[test]
    fn penetration_replacement_reuses_slots_and_closes_boundary_ring() -> Result<()> {
        let points = [
            [0., 0., 0., 0.],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ];
        for (start, candidate) in [(3, [1., 1., 1., 0.]), (0, [-1., -1., -1., 0.])] {
            let mut vertices = points
                .map(|p| PenetrationVertex {
                    difference: p,
                    point_a: p,
                    point_b: [0.; 4],
                })
                .into_iter()
                .collect::<Vec<_>>();
            vertices.push(PenetrationVertex {
                difference: candidate,
                point_a: candidate,
                point_b: [0.; 4],
            });
            let mut faces = penetration_initial_faces(points)?.to_vec();
            let mut classes = vec![0; 4];
            classes[start] = 1;
            let mut interval = [f32::from_bits(0x33d6bf95), 10.];
            let mut horizon = penetration_horizon(
                &mut faces,
                &vertices,
                &mut classes,
                start,
                candidate,
                &mut interval,
            )?;
            let mut free = Vec::new();
            assert!(replace_penetration_faces(
                &mut faces,
                &vertices,
                &mut classes,
                &mut free,
                &mut horizon,
                4
            )?);
            assert!(free.is_empty());
            assert!(classes.iter().all(|v| *v == 0));
            assert!(horizon.outgoing.iter().all(Option::is_none));
            assert_eq!(faces.len(), if start == 3 { 6 } else { 4 });
            if start == 3 {
                assert_eq!(faces[3].vertices, [3, 1, 4]);
                assert_eq!(faces[4].vertices, [2, 3, 4]);
                assert_eq!(faces[5].vertices, [1, 2, 4]);
            }
            for (fi, face) in faces.iter().enumerate() {
                for edge in 0..3 {
                    let (other, oe) = face.opposite[edge];
                    let neighbor = &faces[other];
                    assert_eq!(neighbor.opposite[oe], (fi, edge));
                    assert_eq!(face.vertices[edge], neighbor.vertices[(oe + 1) % 3]);
                    assert_eq!(face.vertices[(edge + 1) % 3], neighbor.vertices[oe]);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn penetration_horizon_preserves_boundary_links_and_postorder() -> Result<()> {
        let points = [
            [0., 0., 0., 0.],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
        ];
        let vertices = points.map(|p| PenetrationVertex {
            difference: p,
            point_a: p,
            point_b: [0.; 4],
        });
        let mut faces = penetration_initial_faces(points)?;
        let mut classes = [0, 0, 0, 1];
        let mut interval = [f32::from_bits(0x33d6bf95), 10.];
        let horizon = penetration_horizon(
            &mut faces,
            &vertices,
            &mut classes,
            3,
            [1., 1., 1., 0.],
            &mut interval,
        )?;
        assert!(!horizon.retry);
        assert_eq!(horizon.removed, [3]);
        assert_eq!(horizon.boundary_count, 3);
        assert_eq!(horizon.last_boundary, Some((1, 1)));
        assert_eq!(classes, [2, 2, 2, 1]);
        assert_eq!(horizon.outgoing[1], Some((1, 1)));
        assert_eq!(horizon.outgoing[2], Some((0, 1)));
        assert_eq!(horizon.outgoing[3], Some((2, 1)));
        assert_eq!(faces[3].distance.to_bits(), 0x7f7fffee);
        let mut faces = penetration_initial_faces(points)?;
        let mut classes = [1, 0, 0, 0];
        let mut interval = [f32::from_bits(0x33d6bf95), 10.];
        let horizon = penetration_horizon(
            &mut faces,
            &vertices,
            &mut classes,
            0,
            [-1., -1., -1., 0.],
            &mut interval,
        )?;
        assert!(!horizon.retry);
        assert_eq!(horizon.removed, [1, 2, 0]);
        assert_eq!(horizon.boundary_count, 3);
        assert_eq!(classes, [1, 1, 1, 2]);
        let mut faces = penetration_initial_faces(points)?;
        let mut classes = [1, 0, 0, 0];
        let epsilon = f32::from_bits(0x33d6bf95);
        let mut interval = [epsilon, epsilon * 1.5];
        let horizon = penetration_horizon(
            &mut faces,
            &vertices,
            &mut classes,
            0,
            [-epsilon, 0., 0., 0.],
            &mut interval,
        )?;
        assert!(horizon.retry);
        assert_eq!(classes, [1, 0, 4, 0]);
        assert_eq!(horizon.removed, [0]);
        assert_eq!(horizon.boundary_count, 0);
        // Native estimate/refinement leaves this axis normal one ULP below
        // unit length; keep the resulting interval rather than idealizing it.
        assert_eq!(interval[0].to_bits(), 0x3456bf94);
        Ok(())
    }
    #[test]
    fn penetration_candidate_keeps_strict_convergence_and_retry_boundary() -> Result<()> {
        let epsilon = f32::from_bits(0x33d6bf95);
        let normal = [1., 0., 0., 0.];
        let face = [[0., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        assert_eq!(
            check_penetration_candidate([epsilon * 0.5, 0., 0., 0.], face, normal, [epsilon; 3])?,
            PenetrationCandidateCheck::Converged
        );
        assert_eq!(
            check_penetration_candidate([epsilon, 0., 0., 0.], face, normal, [epsilon; 3])?,
            PenetrationCandidateCheck::Retry {
                classification: 2,
                lower: epsilon + epsilon,
                upper: f32::from_bits(0x7f7fffee)
            }
        );
        assert_eq!(
            check_penetration_candidate([2. * epsilon, 0., 0., 0.], face, normal, [epsilon; 3])?,
            PenetrationCandidateCheck::Expand {
                lower: epsilon,
                upper: 2. * epsilon
            }
        );
        assert_eq!(
            check_penetration_candidate([0.; 4], face, normal, [0.; 3])?,
            PenetrationCandidateCheck::Retry {
                classification: 2,
                lower: epsilon,
                upper: f32::from_bits(0x7f7fffee)
            }
        );
        // Each of the three face projections can independently stop expansion.
        for index in 0..3 {
            let mut tolerances = [0.; 3];
            tolerances[index] = 2.;
            assert_eq!(
                check_penetration_candidate([1., 0., 0., 0.], face, normal, tolerances)?,
                PenetrationCandidateCheck::Converged
            );
        }
        assert!(
            check_penetration_candidate([f32::NAN, 0., 0., 0.], face, normal, [0.; 3]).is_err()
        );
        Ok(())
    }
    #[test]
    fn penetration_initial_faces_keep_reciprocal_edges_and_outward_winding() -> Result<()> {
        let tetra = [
            [0., 0., 0., 10.],
            [1., 0., 0., 11.],
            [0., 1., 0., 12.],
            [0., 0., 1., 13.],
        ];
        for reversed in [false, true] {
            let mut vertices = tetra;
            if reversed {
                vertices.swap(0, 1);
            }
            let faces = penetration_initial_faces(vertices)?;
            assert_eq!(
                faces[0].vertices,
                if reversed { [1, 2, 0] } else { [0, 2, 1] }
            );
            for (fi, face) in faces.iter().enumerate() {
                assert_eq!(face.normal[3], face.normal[1]);
                for edge in 0..3 {
                    let (other, index) = face.opposite[edge];
                    let paired = &faces[other];
                    assert_eq!(paired.opposite[index], (fi, edge));
                    assert_eq!(face.vertices[edge], paired.vertices[(index + 1) % 3]);
                    assert_eq!(face.vertices[(edge + 1) % 3], paired.vertices[index]);
                }
                let opposite = (0..4).find(|i| !face.vertices.contains(i)).unwrap();
                let delta: [f32; 4] =
                    std::array::from_fn(|i| vertices[opposite][i] - vertices[face.vertices[0]][i]);
                let side = delta[2].mul_add(face.normal[2], delta[0] * face.normal[0])
                    + delta[1] * face.normal[1];
                assert!(side < 0.);
            }
            assert_eq!(faces[0].distance, 0.);
            assert!((faces[3].distance - 0.57735026).abs() < 0.0000001);
        }
        let flat = penetration_initial_faces([[0.; 4]; 4])?;
        assert!(flat
            .iter()
            .all(|face| face.normal == [0.; 4] && face.distance == 0.));
        Ok(())
    }
    #[test]
    fn penetration_support_preserves_call_order_direction_and_source_frames() -> Result<()> {
        struct Observed {
            shape: TriangleSupport,
            label: u8,
            calls: std::rc::Rc<std::cell::RefCell<Vec<(u8, [f32; 4])>>>,
        }
        impl SupportShape for Observed {
            fn vertex(&mut self, id: u16) -> Result<[f32; 4]> {
                self.shape.vertex(id)
            }
            fn sphere_count(&mut self) -> Result<u32> {
                self.shape.sphere_count()
            }
            fn support(&mut self, d: [f32; 4]) -> Result<(u16, [f32; 4])> {
                self.calls.borrow_mut().push((self.label, d));
                self.shape.support(d)
            }
        }
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut a = Observed {
            shape: TriangleSupport {
                vertices: [[1., 0., 0., 0.], [0., 2., 0., 0.], [0., 0., 3., 0.]],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            },
            label: 0,
            calls: calls.clone(),
        };
        let mut b = Observed {
            shape: TriangleSupport {
                vertices: [[-2., 0., 0., 0.], [-1., 0., 0., 0.], [0.; 4]],
                extrusion: [0.; 4],
                extrusion_flag: 0,
            },
            label: 1,
            calls: calls.clone(),
        };
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let vertex = penetration_support(&mut a, &mut b, &transform, [1., 0., 0., 7.])?;
        assert_eq!(
            calls.borrow().iter().map(|v| v.0).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(calls.borrow()[0].1, [1., 0., 0., 7.]);
        assert_eq!(&calls.borrow()[1].1[..3], &[-1., 0., 0.]);
        assert_eq!(vertex.point_a, [1., 0., 0., 0.5]);
        assert_eq!(vertex.point_b, [-2., 0., 0., 0.5]);
        assert_eq!(vertex.difference, [-7., -20., -30., 0.5]);
        let rotated = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        calls.borrow_mut().clear();
        let vertex = penetration_support(&mut a, &mut b, &rotated, [0., 1., 0., 9.])?;
        assert_eq!(&calls.borrow()[1].1[..3], &[-1., 0., 0.]);
        assert_eq!(vertex.point_a[3].to_bits(), 0x3f000001);
        assert_eq!(vertex.point_b, [-2., 0., 0., 0.5]);
        assert_eq!(&vertex.difference[..3], &[-10., -16., -30.]);
        Ok(())
    }
    #[test]
    fn combined_fallback_uses_first_interior_then_ordered_edge_pairs() -> Result<()> {
        let triangle = [
            [0., 0., 0., 10.],
            [2., 0., 0., 11.],
            [0., 2., 0., 12.],
            [0.; 4],
        ];
        let edge = [[0.25, 0.25, 1., 20.], [0.5, 0.5, 1., 21.], [0.; 4], [0.; 4]];
        for point_is_a in [true, false] {
            let (a, b, ca, cb) = if point_is_a {
                (edge, triangle, 2, 3)
            } else {
                (triangle, edge, 3, 2)
            };
            let source = b.map(|mut v| {
                v[0] += 100.;
                v
            });
            let mut features = Features {
                count_a: ca,
                count_b: cb,
                max_dimension_a: 15,
                max_dimension_b: 15,
                vertices_a: a,
                vertices_b: source,
                vertices_b_in_a: b,
            };
            features.reduce_combined_fallback()?;
            assert_eq!(
                (features.count_a, features.count_b),
                if point_is_a { (1, 3) } else { (3, 1) }
            );
            assert_eq!(features.vertices_a[0], a[0]);
            assert_eq!(features.vertices_b[0], source[0]);
            assert_eq!(features.vertices_b_in_a[0], b[0]);
        }
        // No projected vertex is interior, but several edges intersect.
        // The first zero-distance pair is A0->A1 against B1->B2.
        let a = [
            [-2., 0., 0., 10.],
            [2., 0., 0., 11.],
            [0., 2., 0., 12.],
            [0.; 4],
        ];
        let b = [
            [-2., 1., 0., 20.],
            [2., 1., 0., 21.],
            [0., -1., 0., 22.],
            [0.; 4],
        ];
        let source = b.map(|mut v| {
            v[0] += 100.;
            v
        });
        let mut features = Features {
            count_a: 3,
            count_b: 3,
            max_dimension_a: 15,
            max_dimension_b: 15,
            vertices_a: a,
            vertices_b: source,
            vertices_b_in_a: b,
        };
        features.reduce_combined_fallback()?;
        assert_eq!((features.count_a, features.count_b), (2, 2));
        assert_eq!(&features.vertices_a[..2], &a[..2]);
        assert_eq!(&features.vertices_b[..2], &[source[2], source[1]]);
        assert_eq!(&features.vertices_b_in_a[..2], &[b[2], b[1]]);
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        features.vertices_a = a;
        features.vertices_b = b;
        let witness = features
            .finish_fallback_features(false, &identity)?
            .unwrap();
        assert_eq!((features.count_a, features.count_b), (2, 2));
        assert_eq!(witness.squared_distance, 0.);
        assert_eq!(&features.vertices_a[..2], &a[..2]);
        assert_eq!(&features.vertices_b[..2], &[b[2], b[1]]);
        Ok(())
    }
    #[test]
    fn fallback_results_collapse_encoded_ids_in_native_order() -> Result<()> {
        let point = |id: u32| [id as f32, 2., 3., f32::from_bits(0x3f000000 | id)];
        for (ids, expected) in [
            ([1, 2, 3], vec![1, 2, 3]),
            ([1, 2, 2], vec![1, 2]),
            ([1, 2, 1], vec![1, 2]),
            ([1, 1, 2], vec![2, 1]),
            ([1, 1, 1], vec![1]),
        ] {
            let mut vertices = [point(ids[0]), point(ids[1]), point(ids[2]), point(99)];
            let count = collapse_fallback_feature(&mut vertices);
            assert_eq!(
                vertices[..count]
                    .iter()
                    .map(|v| v[3].to_bits() & 0xffff)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(vertices[3], point(99));
        }
        let mut masked = [point(1), point(2), point(1), point(99)];
        masked[2][3] = f32::from_bits(0x3e000001);
        assert_eq!(collapse_fallback_feature(&mut masked), 2);
        masked = [point(1), point(2), point(1), point(99)];
        masked[2][3] = f32::from_bits(0x3f010001);
        assert_eq!(collapse_fallback_feature(&mut masked), 3);
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let mut features = Features {
            count_a: 2,
            count_b: 3,
            max_dimension_a: 15,
            max_dimension_b: 15,
            vertices_a: [point(1), point(1), point(2), point(99)],
            vertices_b: [point(3), point(4), point(3), point(99)],
            vertices_b_in_a: [[123.; 4]; 4],
        };
        assert_eq!(
            features.restore_fallback_features(false, &transform)?,
            FallbackFeatures::CacheSized
        );
        assert_eq!((features.count_a, features.count_b), (2, 2));
        assert_eq!(features.vertices_a[0], point(2));
        assert_eq!(features.vertices_b_in_a[0], [13., 22., 33., 0.]);
        assert_eq!(features.vertices_b_in_a[1], [14., 22., 33., 0.]);
        assert_eq!(features.vertices_b_in_a[2], [123.; 4]);
        features.vertices_a = [point(1), point(2), point(3), point(99)];
        assert_eq!(
            features.restore_fallback_features(false, &transform)?,
            FallbackFeatures::NeedsCombinedReduction
        );
        let before = features.vertices_b_in_a;
        assert_eq!(
            features.restore_fallback_features(true, &transform)?,
            FallbackFeatures::Singleton
        );
        assert_eq!((features.count_a, features.count_b), (1, 1));
        assert_eq!(features.vertices_b_in_a, before);
        Ok(())
    }
    #[test]
    fn fallback_seed_preserves_native_pair_order_and_parallel_source_data() -> Result<()> {
        let a: [[f32; 4]; 4] = std::array::from_fn(|i| [i as f32, 0., 0., 10. + i as f32]);
        let b: [[f32; 4]; 4] = std::array::from_fn(|i| [0., i as f32, 0., 20. + i as f32]);
        let transformed: [[f32; 4]; 4] =
            std::array::from_fn(|i| [100., i as f32, 0., 30. + i as f32]);
        for (ca, cb, pairs) in [
            (1, 1, vec![(0, 0)]),
            (1, 2, vec![(0, 0)]),
            (2, 1, vec![(0, 0)]),
            (2, 2, vec![(0, 0)]),
            (1, 3, vec![(0, 0), (0, 1), (0, 2)]),
            (2, 3, vec![(0, 0), (0, 1), (0, 2), (1, 0)]),
            (1, 4, vec![(0, 0), (0, 1), (0, 2), (0, 3)]),
            (3, 1, vec![(0, 0), (1, 0), (2, 0)]),
            (3, 2, vec![(0, 0), (1, 0), (2, 0), (0, 1)]),
            (4, 1, vec![(0, 0), (1, 0), (2, 0), (3, 0)]),
        ] {
            let mut features = Features {
                count_a: ca,
                count_b: cb,
                max_dimension_a: 15,
                max_dimension_b: 15,
                vertices_a: a,
                vertices_b: b,
                vertices_b_in_a: transformed,
            };
            let seed = features.prepare_fallback_seed()?;
            assert_eq!(seed.count, pairs.len());
            assert_eq!((features.count_a, features.count_b), (ca, cb));
            for (i, (ai, bi)) in pairs.into_iter().enumerate() {
                assert_eq!(features.vertices_a[i], a[ai]);
                assert_eq!(features.vertices_b[i], b[bi]);
                assert_eq!(features.vertices_b_in_a[i], transformed[bi]);
                assert_eq!(
                    seed.differences[i],
                    std::array::from_fn(|axis| a[ai][axis] - transformed[bi][axis])
                );
            }
            if ca < 3 && cb < 3 {
                for i in [ca, ca + 1] {
                    assert_eq!(features.vertices_a[i], a[0]);
                }
                for i in [cb, cb + 1] {
                    assert_eq!(features.vertices_b[i], b[0]);
                    assert_eq!(features.vertices_b_in_a[i], transformed[0]);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn separation_checks_keep_signed_thresholds_and_native_normalization() -> Result<()> {
        let check = check_separation([1., 0., 0., 9.], [2., 0., 0., 8.], 1., false)?;
        assert_eq!(
            (
                check.length_squared,
                check.projection,
                check.support_threshold
            ),
            (1., 2., 1.)
        );
        assert!(!check.requires_fallback);
        for (normal, delta, failed) in [
            ([1., 0., 0., 0.], [1., 0., 0., 0.], false),
            ([1., 0., 0., 0.], [-2., 0., 0., 0.], false),
            ([0.5, 0., 0., 0.], [10., 0., 0., 0.], false),
            ([1., 0., 0., 0.], [2., 0., 0., 0.], true),
        ] {
            assert!(check_separation(normal, delta, 1., failed)?.requires_fallback);
        }
        assert_eq!(
            normalized_separation([3., 4., 0., 77.], [6., 8., 0., 99.])?,
            [0.6, 0.8, 0., 10.]
        );
        assert_eq!(
            normalized_separation([3., 4., 0., 77.], [-6., -8., 0., 99.])?,
            [0.6, 0.8, 0., -10.]
        );
        assert_eq!(normalized_separation([0.; 4], [1., 2., 3., 4.])?, [0.; 4]);
        assert!(normalized_separation([f32::NAN, 0., 0., 0.], [0.; 4]).is_err());
        Ok(())
    }
    #[test]
    fn support_expansion_preserves_order_frames_and_threshold_boundaries() -> Result<()> {
        let direction = [1., 0., 0., 0.];
        assert!(!support_improves(
            [1., 0., 0., 0.],
            &[[0.; 4]],
            direction,
            1.
        )?);
        assert!(support_improves(
            [2., 0., 0., 0.],
            &[[0.; 4], [1., 0., 0., 0.]],
            direction,
            1.
        )?);
        assert!(!support_improves(
            [-2., 0., 0., 0.],
            &[[0.; 4]],
            direction,
            1.
        )?);
        struct Fixed {
            point: [f32; 4],
            label: u8,
            calls: std::rc::Rc<std::cell::RefCell<Vec<(u8, [f32; 4])>>>,
        }
        impl SupportShape for Fixed {
            fn vertex(&mut self, _: u16) -> Result<[f32; 4]> {
                unreachable!()
            }
            fn sphere_count(&mut self) -> Result<u32> {
                unreachable!()
            }
            fn support(&mut self, d: [f32; 4]) -> Result<(u16, [f32; 4])> {
                self.calls.borrow_mut().push((self.label, d));
                Ok((0, self.point))
            }
        }
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let make = || Features {
            count_a: 1,
            count_b: 1,
            max_dimension_a: 15,
            max_dimension_b: 15,
            vertices_a: [[0.; 4]; 4],
            vertices_b: [[0.; 4]; 4],
            vertices_b_in_a: [[0.; 4]; 4],
        };
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut a = Fixed {
            point: [-2., 0., 0., 0.],
            label: 0,
            calls: calls.clone(),
        };
        let mut b = Fixed {
            point: [2., 0., 0., 0.],
            label: 1,
            calls: calls.clone(),
        };
        for odd in [true, false] {
            calls.borrow_mut().clear();
            let mut features = make();
            assert_eq!(
                features.expand_support(&mut a, &mut b, &identity, direction, 1., odd, 0)?,
                Some(if odd {
                    ExpandedShape::A
                } else {
                    ExpandedShape::B
                })
            );
            assert_eq!(calls.borrow().len(), 1);
            assert_eq!(calls.borrow()[0].0, if odd { 0 } else { 1 });
        }
        calls.borrow_mut().clear();
        let mut features = make();
        a.point = [-1., 0., 0., 0.];
        assert_eq!(
            features.expand_support(&mut a, &mut b, &identity, direction, 1., true, 0)?,
            Some(ExpandedShape::B)
        );
        assert_eq!(
            calls.borrow().iter().map(|v| v.0).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(features.count_a, 1);
        assert_eq!(features.vertices_a[1], a.point);
        calls.borrow_mut().clear();
        let mut features = make();
        features.max_dimension_b = 1;
        assert_eq!(
            features.expand_support(&mut a, &mut b, &identity, direction, 1., true, 8)?,
            None
        );
        assert!(calls.borrow().is_empty());
        // B support and improvement are local; only an accepted point is
        // transformed, with source metadata retained in its separate array.
        calls.borrow_mut().clear();
        let mut features = make();
        features.max_dimension_a = 1;
        let rotation = super::super::native_hk_transform::from_entity(
            [0., 1., 0., -1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let search = transpose_direction(&rotation, direction);
        b.point = search.map(|v| v * 2.);
        assert_eq!(
            features.expand_support(&mut a, &mut b, &rotation, direction, 1., false, 0)?,
            Some(ExpandedShape::B)
        );
        assert_eq!(calls.borrow()[0].1, search);
        assert_eq!(features.vertices_b[1], b.point);
        assert_eq!(
            features.vertices_b_in_a[1],
            super::super::native_hk_transform::transform_point(&rotation, b.point)
        );
        Ok(())
    }
    #[test]
    fn tetrahedron_reduction_keeps_source_vertices_and_continues_to_face() -> Result<()> {
        let tetra = [
            [0., 0., 0., 10.],
            [1., 0., 0., 11.],
            [0., 1., 0., 12.],
            [0., 0., 1., 13.],
        ];
        for point_is_a in [true, false] {
            let mut tetra = tetra;
            if !point_is_a {
                tetra.swap(0, 1);
            }
            for (point, removed) in [
                ([1., 1., 1., 20.], 10.),
                ([-1., 0.1, 0.1, 20.], 11.),
                ([0.1, -1., 0.1, 20.], 12.),
            ] {
                let (a, b) = if point_is_a {
                    ([point; 4], tetra)
                } else {
                    (tetra, [point; 4])
                };
                let source = b.map(|mut v| {
                    v[0] += 100.;
                    v[1] += 200.;
                    v[2] += 300.;
                    v
                });
                let mut features = Features {
                    count_a: if point_is_a { 1 } else { 4 },
                    count_b: if point_is_a { 4 } else { 1 },
                    max_dimension_a: 15,
                    max_dimension_b: 15,
                    vertices_a: a,
                    vertices_b: source,
                    vertices_b_in_a: b,
                };
                assert!(features.witness([0.; 4], [0.; 4]).is_err());
                let ExpandedReduction::Reduced(result) = features.reduce_features(None)? else {
                    panic!("separated tetrahedron rejected")
                };
                assert_eq!(
                    (features.count_a, features.count_b),
                    if point_is_a { (1, 3) } else { (3, 1) }
                );
                assert_eq!(result.face.unwrap().mask, 14);
                let face = if point_is_a {
                    &features.vertices_b[..3]
                } else {
                    &features.vertices_a[..3]
                };
                assert!(face.iter().all(|v| v[3] != removed));
                assert!(face.iter().any(|v| v[3] == 13.));
                for i in 0..features.count_b {
                    let original = b
                        .iter()
                        .position(|v| v[3] == features.vertices_b_in_a[i][3])
                        .unwrap();
                    assert_eq!(features.vertices_b[i], source[original]);
                    assert_eq!(features.vertices_b_in_a[i], b[original]);
                }
            }
            let point = [0.1, 0.1, 0.1, 20.];
            let (a, b) = if point_is_a {
                ([point; 4], tetra)
            } else {
                (tetra, [point; 4])
            };
            let mut features = Features {
                count_a: if point_is_a { 1 } else { 4 },
                count_b: if point_is_a { 4 } else { 1 },
                max_dimension_a: 15,
                max_dimension_b: 15,
                vertices_a: a,
                vertices_b: b,
                vertices_b_in_a: b,
            };
            assert!(matches!(
                features.reduce_features(None)?,
                ExpandedReduction::NativeFailure
            ));
            assert_eq!(features.vertices_a, a);
            assert_eq!(features.vertices_b, b);
            assert_eq!(features.vertices_b_in_a, b);
            assert_eq!(
                (features.count_a, features.count_b),
                if point_is_a { (1, 4) } else { (4, 1) }
            );
        }
        Ok(())
    }
    #[test]
    fn tetrahedron_selects_native_faces_and_resolves_equal_scores() -> Result<()> {
        let tetra = [
            [0., 0., 0., 10.],
            [1., 0., 0., 11.],
            [0., 1., 0., 12.],
            [0., 0., 1., 13.],
        ];
        for (point, expected) in [
            ([1., 1., 1., 0.], Some(0)),
            ([-1., 0.1, 0.1, 0.], Some(1)),
            ([0.1, -1., 0.1, 0.], Some(2)),
            ([0.1, 0.1, 0.1, 0.], None),
            ([-1., -1., 0.1, 0.], Some(2)),
            ([0., 0., 0., 0.], None),
        ] {
            assert_eq!(tetrahedron_face_to_remove(point, tetra, 1.)?, expected);
        }
        let mut reversed = tetra;
        reversed.swap(0, 1);
        assert_eq!(
            tetrahedron_face_to_remove([1., 1., 1., 0.], reversed, -1.)?,
            Some(1)
        );
        assert_eq!(
            tetrahedron_face_to_remove([0.1, 0.1, 0.1, 0.], reversed, -1.)?,
            None
        );
        // A collapsed tetrahedron goes through the original sentinel and
        // elimination path without dividing by zero or inventing a face.
        assert_eq!(
            tetrahedron_face_to_remove([1., 2., 3., 0.], [[0.; 4]; 4], 1.)?,
            None
        );
        assert!(tetrahedron_face_to_remove([0.; 4], tetra, f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn expanded_edge_face_preserves_plane_state_and_source_ids() -> Result<()> {
        let triangle = [[0., 0., 0., 10.], [2., 0., 0., 11.], [0., 2., 0., 12.]];
        let make = |edge: [[f32; 4]; 2], edge_is_a: bool| {
            let edge = [edge[0], edge[1], [0.; 4], [0.; 4]];
            let face = [triangle[0], triangle[1], triangle[2], [0.; 4]];
            let (a, b) = if edge_is_a {
                (edge, face)
            } else {
                (face, edge)
            };
            Features {
                count_a: if edge_is_a { 2 } else { 3 },
                count_b: if edge_is_a { 3 } else { 2 },
                max_dimension_a: 3,
                max_dimension_b: 3,
                vertices_a: a,
                vertices_b: b,
                vertices_b_in_a: b,
            }
        };
        for edge_is_a in [true, false] {
            let edge = [[0.25, 0.25, 2., 20.], [0.25, 0.25, 1., 21.]];
            let mut fresh = make(edge, edge_is_a);
            let ExpandedReduction::Reduced(result) = fresh.reduce_edge_face(None)? else {
                panic!("separated edge rejected")
            };
            assert_eq!(
                (fresh.count_a, fresh.count_b),
                if edge_is_a { (1, 3) } else { (3, 1) }
            );
            assert_eq!(result.face.as_ref().unwrap().mask, 14);
            assert_eq!(result.blocked_orientation, 0);
            assert_eq!(result.direction[2], if edge_is_a { 4. } else { -4. });
            assert_eq!(
                if edge_is_a {
                    fresh.vertices_a[0][3]
                } else {
                    fresh.vertices_b[0][3]
                },
                21.
            );
            if !edge_is_a {
                assert_eq!(fresh.vertices_b[0], fresh.vertices_b_in_a[0]);
            }

            // A prior oriented classification also reordered the source face.
            // Carry that mutation into the next iteration's fixture.
            let prior = classify_face(edge[0], triangle, edge_is_a)?;
            let mut cached = make(edge, edge_is_a);
            if prior.face_reversed {
                if edge_is_a {
                    cached.vertices_b.swap(0, 1);
                    cached.vertices_b_in_a.swap(0, 1);
                } else {
                    cached.vertices_a.swap(0, 1);
                }
            }
            let ExpandedReduction::Reduced(result) = cached.reduce_edge_face(Some(&prior))? else {
                panic!("cached separated edge rejected")
            };
            assert_eq!(result.direction, prior.direction);
            assert_eq!(result.blocked_orientation, if edge_is_a { 8 } else { -8 });
            assert!(!result.face.unwrap().face_reversed);
            assert_eq!(
                if edge_is_a {
                    cached.vertices_a[0][3]
                } else {
                    cached.vertices_b[0][3]
                },
                21.
            );

            for previous in [None, Some(&prior)] {
                let mut crossing = make([[0.25, 0.25, 1., 20.], [0.25, 0.25, -1., 21.]], edge_is_a);
                if previous.is_some() && prior.face_reversed {
                    if edge_is_a {
                        crossing.vertices_b.swap(0, 1);
                        crossing.vertices_b_in_a.swap(0, 1);
                    } else {
                        crossing.vertices_a.swap(0, 1);
                    }
                }
                assert!(matches!(
                    crossing.reduce_edge_face(previous)?,
                    ExpandedReduction::NativeFailure
                ));
                assert_eq!(
                    (crossing.count_a, crossing.count_b),
                    if edge_is_a { (2, 3) } else { (3, 2) }
                );

                let mut outside = make([[0.25, 0.25, 2., 20.], [3., 0.25, 1., 21.]], edge_is_a);
                if previous.is_some() && prior.face_reversed {
                    if edge_is_a {
                        outside.vertices_b.swap(0, 1);
                        outside.vertices_b_in_a.swap(0, 1);
                    } else {
                        outside.vertices_a.swap(0, 1);
                    }
                }
                let ExpandedReduction::Reduced(result) = outside.reduce_edge_face(previous)? else {
                    panic!("exterior edge rejected")
                };
                assert!(outside.count_a + outside.count_b <= 4);
                assert!(result.edge_witness.is_some());
                let face_ids = if edge_is_a {
                    &outside.vertices_b[..outside.count_b]
                } else {
                    &outside.vertices_a[..outside.count_a]
                };
                assert!(face_ids.iter().all(|v| v[3] != 10.));
                assert_eq!(outside.vertices_b, outside.vertices_b_in_a);
            }
        }
        // Classification-only retains winding on the opposite side; the
        // oriented path reverses it and swaps the corresponding weights.
        let raw = classify_face_mode([0.25, 0.5, -1., 0.], triangle, None)?;
        let oriented = classify_face([0.25, 0.5, -1., 0.], triangle, true)?;
        assert_eq!(raw.mask, 14);
        assert!(!raw.face_reversed);
        assert!(oriented.face_reversed);
        assert_eq!(raw.weights[0], oriented.weights[1]);
        assert_eq!(raw.weights[1], oriented.weights[0]);
        Ok(())
    }
    #[test]
    fn original_dispatch_distinguishes_expanded_features_from_edge_pairs() -> Result<()> {
        assert_eq!(feature_case(2, 2), FeatureCase::EdgeEdge);
        for (a, b) in [(2, 3), (3, 2)] {
            assert_eq!(feature_case(a, b), FeatureCase::EdgeFace);
        }
        for (a, b) in [(1, 4), (4, 1)] {
            assert_eq!(feature_case(a, b), FeatureCase::PointTetrahedron);
        }
        for (a, b) in [(0, 1), (3, 3), (2, 4), (4, 2), (usize::MAX, 1)] {
            assert_eq!(feature_case(a, b), FeatureCase::Failure);
        }
        let mut features = Features {
            count_a: 2,
            count_b: 2,
            max_dimension_a: 3,
            max_dimension_b: 3,
            vertices_a: [[-1., 0., 0., 0.], [1., 0., 0., 0.], [0.; 4], [0.; 4]],
            vertices_b: [[0., -1., 1., 0.], [0., 1., 1., 0.], [0.; 4], [0.; 4]],
            vertices_b_in_a: [[0., -1., 1., 0.], [0., 1., 1., 0.], [0.; 4], [0.; 4]],
        };
        let reduction = features.reduce_current_features()?;
        assert!(reduction.face.is_none());
        assert_eq!(reduction.edge_witness.unwrap().endpoint_mask, 0);
        assert_eq!(&reduction.direction[..3], &[0., 0., -4.]);
        features.count_b = 3;
        assert!(features
            .reduce_current_features()
            .unwrap_err()
            .to_string()
            .contains("edge/face"));
        assert_eq!((features.count_a, features.count_b), (2, 3));
        Ok(())
    }
    #[test]
    fn edge_pair_preserves_native_clamps_parallel_choice_and_feature_reduction() -> Result<()> {
        let interior = edge_pair(
            [-1., 0., 0., 0.],
            [2., 0., 0., 0.],
            [0., -1., 1., 0.],
            [0., 2., 0., 0.],
        )?;
        assert_eq!(interior.endpoint_mask, 0);
        assert_eq!(interior.point_a, [0.; 4]);
        assert_eq!(interior.delta, [0., 0., -1., 0.]);
        assert_eq!(interior.squared_distance, 1.);
        let end = edge_pair(
            [0.; 4],
            [1., 0., 0., 0.],
            [2., -1., 1., 0.],
            [0., 2., 0., 0.],
        )?;
        assert_eq!(end.endpoint_mask, 1);
        assert_eq!(end.point_a, [1., 0., 0., 0.]);
        assert_eq!(end.delta, [-1., 0., -1., 0.]);
        assert_eq!(end.squared_distance, 2.);
        let parallel = edge_pair(
            [0.; 4],
            [2., 0., 0., 0.],
            [0.5, 1., 0., 0.],
            [1., 0., 0., 0.],
        )?;
        assert_eq!(parallel.endpoint_mask, 4);
        assert_eq!(parallel.point_a, [1.5, 0., 0., 0.]);
        assert_eq!(parallel.delta, [0., -1., 0., 0.]);
        let degenerate = edge_pair([0.; 4], [0.; 4], [1., 0., 0., 0.], [0.; 4])?;
        assert_eq!(degenerate.endpoint_mask, 6);
        assert_eq!(degenerate.squared_distance, 1.);
        let mut features = Features {
            count_a: 2,
            count_b: 2,
            max_dimension_a: 3,
            max_dimension_b: 3,
            vertices_a: [
                [0.; 4],
                [1., 0., 0., f32::from_bits(0x3f000002)],
                [0.; 4],
                [0.; 4],
            ],
            vertices_b: [[2., -1., 1., 0.], [2., 1., 1., 0.], [0.; 4], [0.; 4]],
            vertices_b_in_a: [[2., -1., 1., 0.], [2., 1., 1., 0.], [0.; 4], [0.; 4]],
        };
        let (witness, direction) = features.finish_edge_pair()?;
        assert_eq!(witness.endpoint_mask, 1);
        assert_eq!((features.count_a, features.count_b), (1, 2));
        assert_eq!(features.vertices_a[0][3].to_bits(), 0x3f000002);
        assert!(direction[0] < 0. && direction[2] < 0.);
        assert_eq!(direction[0], direction[2]);
        Ok(())
    }
    #[test]
    fn face_dispatch_reduces_all_exterior_regions_in_both_shape_orders() -> Result<()> {
        let mut triangle = TriangleSupport {
            vertices: [[0.; 4], [2., 0., 0., 0.], [0., 2., 0., 0.]],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let local = [
            triangle.vertex(0)?,
            triangle.vertex(1)?,
            triangle.vertex(2)?,
        ];
        for (point, ids, delta) in [
            ([-1., -1., 1., 0.], vec![0], [-1., -1., 1.]),
            ([3., -1., 1., 0.], vec![1], [1., -1., 1.]),
            ([-1., 3., 1., 0.], vec![2], [-1., 1., 1.]),
            ([1., -1., 1., 0.], vec![0, 1], [0., -1., 1.]),
            ([-1., 1., 1., 0.], vec![0, 2], [-1., 0., 1.]),
            ([2., 2., 1., 0.], vec![1, 2], [1., 1., 1.]),
            ([0.5, 0.5, 1., 0.], vec![0, 1, 2], [0., 0., 1.]),
        ] {
            for point_is_a in [true, false] {
                let mut features = Features {
                    count_a: if point_is_a { 1 } else { 3 },
                    count_b: if point_is_a { 3 } else { 1 },
                    max_dimension_a: 3,
                    max_dimension_b: 3,
                    vertices_a: if point_is_a {
                        [point; 4]
                    } else {
                        [local[0], local[1], local[2], [0.; 4]]
                    },
                    vertices_b: if point_is_a {
                        [local[0], local[1], local[2], [0.; 4]]
                    } else {
                        [point; 4]
                    },
                    vertices_b_in_a: if point_is_a {
                        [local[0], local[1], local[2], [0.; 4]]
                    } else {
                        [point; 4]
                    },
                };
                let result = features.reduce_point_face()?;
                let (vertices, count) = if point_is_a {
                    (&features.vertices_b, features.count_b)
                } else {
                    (&features.vertices_a, features.count_a)
                };
                let mut actual = vertices[..count]
                    .iter()
                    .map(|v| v[3].to_bits() & 0xffff)
                    .collect::<Vec<_>>();
                actual.sort();
                assert_eq!(actual, ids, "point={point:?}, point_is_a={point_is_a}");
                assert_eq!(result.face.is_some(), ids.len() == 3);
                assert!(result.direction[2] * (if point_is_a { 1. } else { -1. }) > 0.);
                for axis in 0..2 {
                    assert_eq!(
                        result.direction[axis] * delta[2],
                        result.direction[2] * delta[axis]
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn face_classification_keeps_strict_regions_and_reverses_source_winding() -> Result<()> {
        let triangle = [[0.; 4], [2., 0., 0., 0.], [0., 2., 0., 0.]];
        let front = classify_face([0.5, 0.5, 1., 0.], triangle, true)?;
        assert_eq!(front.mask, 14);
        assert_eq!(front.weights, [-8., -4., -4., 0.]);
        assert_eq!(&front.direction[..3], &[0., 0., 4.]);
        assert!(!front.face_reversed);
        let back = classify_face([0.5, 0.5, -1., 0.], triangle, true)?;
        assert_eq!(back.mask, 14);
        assert_eq!(back.weights, [-4., -8., -4., 0.]);
        assert_eq!(&back.direction[..3], &[0., 0., -4.]);
        assert!(back.face_reversed);
        assert!(classify_face([0.5, 0.5, 1., 0.], triangle, false)?.face_reversed);
        assert!(!classify_face([0.5, 0.5, 0., 0.], triangle, true)?.face_reversed);
        for (point, mask) in [
            ([0.5, 0., 1., 0.], 12),
            ([3., 0.5, 1., 0.], 6),
            ([-1., -1., 1., 0.], 8),
        ] {
            assert_eq!(classify_face(point, triangle, true)?.mask, mask);
        }
        let mut a = TriangleSupport {
            vertices: [[0.5, 0.5, -1., 0.]; 3],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices: triangle,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let cache = Cache([0, 0, 0, 0, 0, 1, 0, 2, 1, 3, 0x33, 0]);
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut features = Features::load(&cache, &mut a, &mut b, &identity)?;
        assert!(features.classify_point_face()?.face_reversed);
        assert_eq!(features.vertices_b[0][3].to_bits(), 0x3f000001);
        assert_eq!(features.vertices_b[1][3].to_bits(), 0x3f000000);
        assert_eq!(features.vertices_b_in_a[0], triangle[1]);
        assert_eq!(features.vertices_b_in_a[1], triangle[0]);
        assert_eq!((features.count_a, features.count_b), (1, 3));
        Ok(())
    }
    #[test]
    fn point_edge_reduction_preserves_endpoint_boundaries_and_source_ids() -> Result<()> {
        let edge = [[0., 0., 0., 0.], [2., 0., 0., 0.]];
        for (x, endpoint) in [
            (-1., Some(0)),
            (0., Some(0)),
            (1., None),
            (2., None),
            (3., Some(1)),
        ] {
            let forward = point_edge([x, 1., 0., 0.], edge, true)?;
            let reverse = point_edge([x, 1., 0., 0.], edge, false)?;
            assert_eq!(forward.endpoint, endpoint);
            assert_eq!(reverse.endpoint, endpoint);
            for i in 0..3 {
                assert_eq!(forward.direction[i], -reverse.direction[i]);
            }
            assert!(forward.direction[1] > 0.);
            if endpoint.is_none() {
                assert_eq!(
                    forward.direction[1].to_bits(),
                    (4. * super::super::pose_blend::estimate(4.)).to_bits()
                );
            }
        }
        let mut a = TriangleSupport {
            vertices: [[0.; 4], [0.; 4], [3., 1., 0., 0.]],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices: [edge[0], edge[1], [0.; 4]],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let cache = Cache([0, 2, 0, 0, 0, 1, 0, 0, 1, 2, 0x33, 0]);
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut features = Features::load(&cache, &mut a, &mut b, &identity)?;
        assert_eq!(&features.reduce_point_edge()?[..3], &[1., 1., 0.]);
        assert_eq!(features.count_b, 1);
        assert_eq!(features.vertices_b[0][3].to_bits(), 0x3f000001);
        assert_eq!(features.vertices_b_in_a[0], edge[1]);
        assert!(features.reduce_point_edge().is_err());
        assert_eq!(
            point_edge([0., 1., 0., 0.], [[0.; 4]; 2], true)?.endpoint,
            Some(0)
        );
        Ok(())
    }
    #[test]
    fn cached_features_keep_local_ids_and_transformed_witnesses_distinct() -> Result<()> {
        let vertices = [[0.; 4], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let mut a = TriangleSupport {
            vertices,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices,
            extrusion: [0., 0., 2., 0.],
            extrusion_flag: 1,
        };
        let mut cache = Cache([0; 12]);
        cache.0[10] = 0x3f;
        cache.update(&[0, 2], &[4])?;
        let transform = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let features = Features::load(&cache, &mut a, &mut b, &transform)?;
        assert_eq!((features.count_a, features.count_b), (2, 1));
        assert_eq!(
            (features.max_dimension_a, features.max_dimension_b),
            (3, 15)
        );
        assert_eq!(features.vertices_a[0][3].to_bits(), 0x3f000002);
        assert_eq!(
            features.vertices_b[0],
            [1., 0., 2., f32::from_bits(0x3f000004)]
        );
        assert_eq!(features.vertices_b_in_a[0], [10., 21., 32., 0.]);
        assert_eq!(
            features.witness([0., 1., 0., 2.], [999.; 4])?.point_a,
            [10., 23., 32., 4.]
        );
        cache.0[8] = 0;
        assert!(Features::load(&cache, &mut a, &mut b, &transform).is_err());
        Ok(())
    }
    #[test]
    fn triangle_support_preserves_ties_extrusion_and_encoded_ids() -> Result<()> {
        let mut shape = TriangleSupport {
            vertices: [[0.; 4], [1., 0., 0., 0.], [0., 1., 0., 0.]],
            extrusion: [0., 0., 2., 0.],
            extrusion_flag: 1,
        };
        for (direction, id) in [
            ([0.; 4], 1),
            ([1., 1., 0., 0.], 1),
            ([0., 1., 0., 0.], 2),
            ([-1., -1., 0., 0.], 0),
            ([0., 0., 1., 0.], 4),
            ([0., 0., -1., 0.], 1),
        ] {
            let (actual, point) = shape.support(direction)?;
            assert_eq!(actual, id);
            assert_eq!(point[3].to_bits(), 0x3f000000 | id as u32);
            assert_eq!(point, shape.vertex(id)?);
        }
        for id in 0..6 {
            let vertex = shape.vertex(id)?;
            assert_eq!(vertex[2], if id < 3 { 0. } else { 2. });
            assert_eq!(vertex[3].to_bits(), 0x3f000000 | id as u32);
        }
        assert!(shape.vertex(6).is_err());
        assert_eq!(shape.sphere_count()?, 6);
        shape.extrusion_flag = 0;
        assert_eq!(shape.sphere_count()?, 3);
        assert_eq!(
            shape.support([0., 0., 1., 0.])?.0,
            4,
            "Support does not gate extrusion on flag"
        );
        // Both real triangle shapes now drive general initialization.
        let mut other = TriangleSupport {
            vertices: [[0.; 4], [1., 0., 0., 0.], [0., 1., 0., 0.]],
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let pose = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0., 0., 5.],
        );
        let mut cache = Cache([0xa5; 12]);
        cache.initialize_convex(&mut shape, &mut other, &pose)?;
        assert_eq!(cache.0, [0, 4, 0, 1, 0, 0, 0, 0, 1, 1, 0x33, 0]);
        Ok(())
    }
    struct Shape {
        vertex: [f32; 4],
        point: [f32; 4],
        id: u16,
        count: u32,
        directions: Vec<[f32; 4]>,
        calls: Vec<&'static str>,
    }
    impl SupportShape for Shape {
        fn vertex(&mut self, id: u16) -> Result<[f32; 4]> {
            assert_eq!(id, 0);
            self.calls.push("vertex");
            Ok(self.vertex)
        }
        fn support(&mut self, d: [f32; 4]) -> Result<(u16, [f32; 4])> {
            self.calls.push("support");
            self.directions.push(d);
            Ok((self.id, self.point))
        }
        fn sphere_count(&mut self) -> Result<u32> {
            self.calls.push("count");
            Ok(self.count)
        }
    }
    #[test]
    fn initial_features_use_triangle_face_or_general_support_pair() -> Result<()> {
        use super::super::native_hk_transform::from_entity;
        let identity = from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        let mut a = Shape {
            vertex: [0., 0., -2., 0.],
            point: [0., 0., -1., 0.],
            id: 42,
            count: 8,
            directions: vec![],
            calls: vec![],
        };
        let mut cache = Cache([0xa5; 12]);
        cache.initialize_triangle(
            &mut a,
            [[0.; 4], [1., 0., 0., 0.], [0., 1., 0., 0.]],
            &identity,
        )?;
        assert_eq!(cache.0, [0, 42, 0, 0, 0, 1, 0, 2, 1, 3, 0xf3, 0]);
        assert_eq!(&a.directions[0][..3], &[0., 0., -1.]);
        assert_eq!(a.calls, ["vertex", "support", "count"]);
        a.calls.clear();
        a.directions.clear();
        let mut b = Shape {
            vertex: [1., 0., 0., 0.],
            point: [0.; 4],
            id: 11,
            count: 2,
            directions: vec![],
            calls: vec![],
        };
        let b_in_a = from_entity([0., -1., 0., 1., 0., 0., 0., 0., 1.], [10., 0., 0.]);
        cache.initialize_convex(&mut a, &mut b, &b_in_a)?;
        assert_eq!(cache.0, [0, 42, 0, 11, 0, 0, 0, 0, 1, 1, 0xf2, 0]);
        assert_eq!(&a.directions[0][..3], &[10., 1., 2.]);
        assert_eq!(&b.directions[0][..3], &[-1., 10., -1.]);
        assert_eq!(a.calls, ["vertex", "support", "count"]);
        assert_eq!(b.calls, ["vertex", "support", "count"]);
        Ok(())
    }
    #[test]
    fn cache_rotation_preserves_winding_and_unused_bytes() -> Result<()> {
        for (input, expected) in [
            ([0, 1, 2], [2, 0, 1]),
            ([0, 2, 1], [2, 1, 0]),
            ([1, 0, 2], [2, 1, 0]),
            ([1, 2, 0], [2, 0, 1]),
            ([2, 0, 1], [2, 0, 1]),
            ([2, 1, 0], [2, 1, 0]),
        ] {
            let mut cache = Cache([0xa5; 12]);
            cache.update(&input, &[7])?;
            assert_eq!(
                cache.features()?.0,
                expected
                    .iter()
                    .flat_map(|v: &u16| v.to_be_bytes())
                    .collect::<Vec<_>>()
            );
            assert_eq!(cache.0[10..], [0xa5; 2]);
            cache.update(&[9], &input)?;
            assert_eq!(
                cache.features()?.1,
                expected
                    .iter()
                    .flat_map(|v: &u16| v.to_be_bytes())
                    .collect::<Vec<_>>()
            );
        }
        let mut cache = Cache([0xa5; 12]);
        cache.update(&[1], &[3, 7])?;
        assert_eq!(cache.0, [0, 1, 0, 7, 0, 3, 0xa5, 0xa5, 1, 2, 0xa5, 0xa5]);
        let previous = cache.clone();
        assert!(cache.update(&[1, 2, 3], &[4, 5]).is_err());
        assert_eq!(cache, previous);
        assert_eq!(canonical_feature(&[2, 2, 1]), [2, 2, 1]);
        assert_eq!(canonical_feature(&[1, 2, 2]), [2, 2, 1]);
        Ok(())
    }
    #[test]
    fn witness_selection_preserves_native_four_lane_arithmetic() -> Result<()> {
        let normal = [0., 1., 0., 2.];
        let a = [4., 5., 6., 7.];
        let b = [1., 3., 5., 11.];
        let scratch = [8., 9., 10., 12.];
        assert_eq!(core_witness(&[a], &[b], normal, scratch)?.point_a, a);
        assert_eq!(
            core_witness(&[a, a], &[b], normal, scratch)?.point_a,
            [1., 5., 5., 15.]
        );
        let witness = core_witness(&[a, a], &[b, b], normal, scratch)?;
        assert_eq!(witness.point_a, scratch);
        assert_eq!(witness.distance, 2.);
        assert_eq!(witness.normal, normal);
        assert!(core_witness(&[], &[b], normal, scratch).is_err());
        Ok(())
    }
}
