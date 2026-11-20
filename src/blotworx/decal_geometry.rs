//! Geometry arithmetic used by retail decal triangle collection. This is not
//! a replacement for the still-required triangle filtering/projection pass.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
type V = [f32; 3];

pub struct Transform {
    pub rotation: [V; 3],
    pub scale: f32,
    pub translation: V,
}
impl Transform {
    fn rotate(&self, v: V) -> V {
        self.rotation
            .map(|r| r[2].mul_add(v[2], r[0].mul_add(v[0], r[1] * v[1])))
    }
    fn point(&self, v: V) -> V {
        let rotated = self.rotate(v);
        std::array::from_fn(|i| rotated[i] * self.scale + self.translation[i])
    }
}
pub struct Query {
    pub local_position: V,
    pub first_vertex_limit_squared: f32,
    pub triangle_limit_squared: f32,
    /// Live template threshold: REL BSS 0x26c5c (u16) / 0x26c58 (u8).
    pub facing_threshold: f32,
    pub transform: Transform,
}
impl Query {
    /// REL 0x25a158..0x25a210 and 0x259408..0x259480. The triangle
    /// limit is multiplied by inverse scale ONCE; do not "correct" it to
    /// squared scale. The caller restores this shared value after each mesh.
    pub fn for_mesh(
        local_position: V,
        radius: f32,
        inverse_scale: f32,
        facing_threshold: f32,
        transform: Transform,
    ) -> Self {
        let half_radius = radius * 0.5;
        let broad = (7.5 + half_radius) * inverse_scale;
        Self {
            local_position,
            first_vertex_limit_squared: broad * broad,
            triangle_limit_squared: (2. * (radius * radius)) * inverse_scale,
            facing_threshold,
            transform,
        }
    }
}
pub struct Collected {
    pub positions: Vec<V>,
    pub indices: Vec<u16>,
    pub weighted_normal: V,
    pub nearest_normal: V,
    pub nearest_squared: f32,
}
pub struct Projected {
    pub positions: Vec<V>,
    pub indices: Vec<u16>,
    pub uv: Vec<[f32; 2]>,
}
impl Collected {
    pub fn new(initial_normal: V) -> Self {
        Self {
            positions: vec![],
            indices: vec![],
            weighted_normal: [0.; 3],
            nearest_normal: initial_normal,
            nearest_squared: f32::MAX,
        }
    }
    /// REL 0x25a254..0x25a630. `center` is already in target-local space;
    /// `target_inverse_scale` is the inverse transform's uniform scale.
    /// The native RNG and sine/cosine provider are explicit dependencies so
    /// host libm does not silently replace the game's math implementation.
    pub fn project(
        mut self,
        center: V,
        radius: f32,
        target_inverse_scale: f32,
        tangent: Option<V>,
        random: impl FnOnce() -> i32,
        sin_cos: impl FnOnce(f32) -> Result<(f32, f32)>,
    ) -> Result<Option<Projected>> {
        if self.indices.is_empty() {
            return Ok(None);
        }
        let weighted = normalize(self.weighted_normal)?;
        let blended = std::array::from_fn(|i| {
            0.1_f32.mul_add(self.nearest_normal[i], (1. - 0.1_f32) * weighted[i])
        });
        let normal = normalize(blended)?;
        let seed = tangent.unwrap_or([
            normal[0] + normal[1],
            normal[1] + normal[2],
            normal[2] + normal[0],
        ]);
        let first = normalize(cross(seed, normal))?;
        let second = normalize(cross(first, normal))?;
        let (u, v) = if tangent.is_some() {
            (second, first)
        } else {
            let angle = 6.2831854820251465_f32 * ((random() as f32) / 32767.);
            let (sin, cos) = sin_cos(angle)?;
            // Original separately scales each vector, then adds. Do not fuse.
            (
                std::array::from_fn(|i| second[i] * sin + first[i] * cos),
                std::array::from_fn(|i| second[i] * (-cos) + first[i] * sin),
            )
        };
        let diameter = 1_f32 * (target_inverse_scale * radius);
        let uv = self
            .positions
            .iter()
            .map(|position| {
                let delta = std::array::from_fn(|i| position[i] - center[i]);
                [
                    0.5 + dot(u, delta) / diameter,
                    0.5 + dot(v, delta) / diameter,
                ]
            })
            .collect();
        // Retail normalizes the accumulated normal a second time here.
        let offset = normalize(weighted)?.map(|v| v * (target_inverse_scale * 0.015_f32));
        for position in &mut self.positions {
            for i in 0..3 {
                position[i] += offset[i];
            }
        }
        Ok(Some(Projected {
            positions: self.positions,
            indices: self.indices,
            uv,
        }))
    }
    /// REL 0x259698 / 0x259b30. Share remap between regions of one mesh;
    /// clear it for each new mesh, as the original does at 0x259434.
    /// Returns false at the original 0xc00 output capacity. Partial output and
    /// normal contributions are retained exactly where the native loop exits.
    pub fn region(
        &mut self,
        vertices: &[V],
        indices: &[u16],
        offset: u32,
        query: &Query,
        remap: &mut BTreeMap<u16, u16>,
    ) -> Result<bool> {
        ensure!(
            indices.len() % 3 == 0,
            "Decal triangle region has incomplete indices"
        );
        self.triangles(
            indices.chunks_exact(3).map(|triangle| {
                std::array::from_fn(|i| offset.wrapping_add(triangle[i] as u32) as u16)
            }),
            query,
            remap,
            |id| {
                vertices.get(id as usize).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Decal vertex index {id} exceeds {} vertices",
                        vertices.len()
                    )
                })
            },
        )
    }
    /// Borrow the viewport's interleaved source positions and indices directly.
    /// No per-impact copy of all target vertices or index-width conversion.
    /// This is an adapter for the preview layout, not a native stream decoder.
    pub fn interleaved_region(
        &mut self,
        vertices: &[f32],
        stride: usize,
        indices: &[u32],
        query: &Query,
        remap: &mut BTreeMap<u16, u16>,
    ) -> Result<bool> {
        ensure!(
            stride >= 3 && vertices.len() % stride == 0,
            "Invalid decal target vertex layout"
        );
        ensure!(
            indices.len() % 3 == 0,
            "Decal triangle region has incomplete indices"
        );
        ensure!(
            indices.iter().all(|i| *i <= u16::MAX as u32),
            "Decal preview target exceeds u16 indices"
        );
        self.triangles(
            indices
                .chunks_exact(3)
                .map(|triangle| std::array::from_fn(|i| triangle[i] as u16)),
            query,
            remap,
            |id| {
                let start = (id as usize)
                    .checked_mul(stride)
                    .context("Decal vertex offset overflow")?;
                let end = start
                    .checked_add(3)
                    .context("Decal vertex offset overflow")?;
                let p = vertices
                    .get(start..end)
                    .context("Decal vertex index exceeds interleaved storage")?;
                Ok([p[0], p[1], p[2]])
            },
        )
    }
    fn triangles(
        &mut self,
        triangles: impl IntoIterator<Item = [u16; 3]>,
        query: &Query,
        remap: &mut BTreeMap<u16, u16>,
        get: impl Fn(u16) -> Result<V>,
    ) -> Result<bool> {
        for ids in triangles {
            let origin = get(ids[0])?;
            let delta: V = std::array::from_fn(|i| query.local_position[i] - origin[i]);
            let broad = delta[2].mul_add(delta[2], delta[0].mul_add(delta[0], delta[1] * delta[1]));
            if broad > query.first_vertex_limit_squared {
                continue;
            }
            let second = get(ids[1])?;
            let third = get(ids[2])?;
            let e0 = std::array::from_fn(|i| second[i] - origin[i]);
            let e1 = std::array::from_fn(|i| third[i] - origin[i]);
            let distance = point_triangle(query.local_position, origin, e0, e1).squared;
            if distance > query.triangle_limit_squared {
                continue;
            }
            let (normal, area) = triangle_normal(e0, e1)?;
            if dot(delta, normal) <= query.facing_threshold {
                continue;
            }
            let transformed = query.transform.rotate(normal);
            for i in 0..3 {
                self.weighted_normal[i] = area.mul_add(transformed[i], self.weighted_normal[i]);
            }
            if !(distance >= self.nearest_squared) {
                self.nearest_squared = distance;
                self.nearest_normal = transformed;
            }
            for id in ids {
                let output = if let Some(&output) = remap.get(&id) {
                    output
                } else {
                    if self.positions.len() >= 0xc00 {
                        return Ok(false);
                    }
                    let output = self.positions.len() as u16;
                    self.positions.push(query.transform.point(get(id)?));
                    remap.insert(id, output);
                    output
                };
                if self.indices.len() >= 0xc00 {
                    return Ok(false);
                }
                self.indices.push(output);
            }
        }
        Ok(true)
    }
}

/// DOL 0x805aea70; do not reassociate this into a generic SIMD dot product.
fn dot(a: V, b: V) -> f32 {
    a[0].mul_add(b[0], a[1] * b[1]) + a[2] * b[2]
}

#[derive(Debug, PartialEq)]
pub struct Distance {
    pub squared: f32,
    /// Closest point = origin + coordinates[0]*edge0 + coordinates[1]*edge1.
    pub coordinates: [f32; 2],
}

/// REL 0x25020c..0x250784, including the seven triangle regions and original
/// fused operations. Degenerate triangles retain native nonfinite results;
/// replacing them with an arbitrary segment changes the caller's comparisons.
pub fn point_triangle(point: V, origin: V, edge0: V, edge1: V) -> Distance {
    let delta = std::array::from_fn(|i| origin[i] - point[i]);
    let a = dot(edge0, edge0);
    let b = dot(edge0, edge1);
    let c = dot(edge1, edge1);
    let d = dot(delta, edge0);
    let e = dot(delta, edge1);
    let f = dot(delta, delta);
    let determinant = a.mul_add(c, -(b * b)).abs();
    let s = b.mul_add(e, -(c * d));
    let t = b.mul_add(d, -(a * e));
    let output = |s, t, squared: f32| Distance {
        coordinates: [s, t],
        squared: squared.abs(),
    };
    let end_s = || output(1., 0., f + 2_f32.mul_add(d, a));
    let end_t = || output(0., 1., f + 2_f32.mul_add(e, c));
    let edge_s = || {
        if d >= 0. {
            output(0., 0., f)
        } else if -d >= a {
            end_s()
        } else {
            let s = -d / a;
            output(s, 0., d.mul_add(s, f))
        }
    };
    let edge_t = || {
        if e >= 0. {
            output(0., 0., f)
        } else if -e >= c {
            end_t()
        } else {
            let t = -e / c;
            output(0., t, e.mul_add(t, f))
        }
    };
    let opposite_s = |numerator: f32, denominator: f32| {
        let s = numerator / denominator;
        output(s, 1. - s, -numerator.mul_add(s, -(f + 2_f32.mul_add(e, c))))
    };
    if s + t <= determinant {
        if s < 0. {
            if t < 0. && d < 0. {
                return edge_s();
            }
            return edge_t();
        }
        if t < 0. {
            return edge_s();
        }
        let reciprocal = 1. / determinant;
        let s = s * reciprocal;
        let t = t * reciprocal;
        let tpart = 2_f32.mul_add(e, b.mul_add(s, c * t));
        let spart = 2_f32.mul_add(d, a.mul_add(s, b * t));
        return output(s, t, f + s.mul_add(spart, t * tpart));
    }
    if s < 0. {
        let left = b + d;
        let right = c + e;
        if right > left {
            let numerator = right - left;
            let denominator = c - 2_f32.mul_add(b, -a);
            return if numerator >= denominator {
                end_s()
            } else {
                opposite_s(numerator, denominator)
            };
        }
        return if right <= 0. {
            end_t()
        } else if e >= 0. {
            output(0., 0., f)
        } else {
            let t = -e / c;
            output(0., t, e.mul_add(t, f))
        };
    }
    if t < 0. {
        let left = b + e;
        let right = a + d;
        if right > left {
            let numerator = right - left;
            let denominator = c - 2_f32.mul_add(b, -a);
            if numerator >= denominator {
                return end_t();
            }
            let t = numerator / denominator;
            return output(1. - t, t, -numerator.mul_add(t, -(f + 2_f32.mul_add(d, a))));
        }
        return if right <= 0. {
            end_s()
        } else if d >= 0. {
            output(0., 0., f)
        } else {
            let s = -d / a;
            output(s, 0., d.mul_add(s, f))
        };
    }
    let numerator = (c + e) - (b + d);
    if numerator <= 0. {
        return end_t();
    }
    let denominator = c - 2_f32.mul_add(b, -a);
    if numerator >= denominator {
        end_s()
    } else {
        opposite_s(numerator, denominator)
    }
}

/// DOL 0x805aea90 paired-single cross product. The negated second/third
/// components preserve the original fused subtraction orientation.
pub(super) fn cross(a: V, b: V) -> V {
    [
        a[1].mul_add(b[2], -(b[1] * a[2])),
        -a[0].mul_add(b[2], -(b[0] * a[2])),
        -a[1].mul_add(b[0], -(b[1] * a[0])),
    ]
}
/// DOL 0x805aea20: paired length followed by one Gekko estimate refinement.
fn length(v: V) -> Result<f32> {
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    ensure!(squared.is_finite(), "Decal triangle normal overflow");
    if squared == 0. {
        return Ok(0.);
    }
    Ok(squared * reciprocal_length(squared))
}
fn reciprocal_length(squared: f32) -> f32 {
    let estimate = super::pose_blend::estimate_double(squared);
    let square = super::pose_blend::square_estimate(estimate);
    let half = (estimate * 0.5) as f32;
    let correction = -square.mul_add(squared, -3.);
    correction * half
}
fn normalize(v: V) -> Result<V> {
    if length(v)? <= 1e-6 {
        return Ok([0.; 3]);
    }
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    let factor = reciprocal_length(squared);
    Ok(v.map(|x| x * factor))
}

/// REL 0x2507cc: unit normal and cross-product length (twice triangle area).
/// Edges are supplied as in the original NiTrigon representation.
pub fn triangle_normal(edge0: V, edge1: V) -> Result<(V, f32)> {
    ensure!(
        edge0.iter().chain(&edge1).all(|v| v.is_finite()),
        "Non-finite decal triangle"
    );
    let normal = cross(edge0, edge1);
    let area = length(normal)?;
    if area <= 1e-6 {
        return Ok(([0.; 3], 0.));
    }
    let reciprocal = 1. / area;
    Ok((normal.map(|v| v * reciprocal), area))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> Query {
        Query {
            local_position: [0.25, 0.25, 1.],
            first_vertex_limit_squared: 100.,
            triangle_limit_squared: 1.,
            facing_threshold: 0.,
            transform: Transform {
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                scale: 2.,
                translation: [10., 20., 30.],
            },
        }
    }
    fn collected() -> Collected {
        Collected {
            positions: vec![[0.; 3], [1., 0., 0.], [0., 1., 0.]],
            indices: vec![0, 1, 2],
            weighted_normal: [0., 0., 1.],
            nearest_normal: [0., 0., 1.],
            nearest_squared: 1.,
        }
    }
    #[test]
    fn projection_uses_tangent_without_rng_and_offsets_after_uvs() -> Result<()> {
        let out = collected()
            .project(
                [0.; 3],
                1.,
                1.,
                Some([1., 0., 0.]),
                || panic!("tangent consumed RNG"),
                |_| panic!("tangent used trigonometry"),
            )?
            .unwrap();
        assert_eq!(out.indices, [0, 1, 2]);
        assert_eq!(out.uv[0], [0.5, 0.5]);
        // Geometric expectations allow two f32 ULPs for the original single
        // reciprocal-sqrt refinement; these are not runtime parity tolerances.
        assert!((out.uv[1][0] + 0.5).abs() < 2e-7);
        assert!((out.uv[2][1] + 0.5).abs() < 2e-7);
        for position in out.positions {
            assert!((position[2] - 0.015).abs() < 4e-9);
        }
        Ok(())
    }
    #[test]
    fn projection_random_rotation_and_empty_geometry_follow_native_gates() -> Result<()> {
        let out = collected()
            .project(
                [0.; 3],
                1.,
                1.,
                None,
                || 0,
                |angle| {
                    assert_eq!(angle, 0.);
                    Ok((0., 1.))
                },
            )?
            .unwrap();
        // Automatic seed for +Z is (0,1,1), giving first=+X, second=-Y.
        assert!((out.uv[1][0] - 1.5).abs() < 2e-7);
        assert!((out.uv[2][1] - 1.5).abs() < 2e-7);
        assert!(Collected::new([0.; 3])
            .project(
                [0.; 3],
                0.,
                0.,
                None,
                || panic!("empty mesh consumed RNG"),
                |_| panic!("empty mesh used trig")
            )?
            .is_none());
        Ok(())
    }
    #[test]
    fn interleaved_source_preserves_projection_without_copying_attributes() -> Result<()> {
        let vertices = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        // Unused normal/UV/color payload must not be interpreted as positions.
        let packed: Vec<f32> = vertices
            .iter()
            .flat_map(|p| p.iter().copied().chain([f32::NAN; 9]))
            .collect();
        let mut direct = Collected::new([0., 0., 1.]);
        let mut reference = Collected::new([0., 0., 1.]);
        direct.interleaved_region(
            &packed,
            12,
            &[0, 1, 2, 0, 1, 2],
            &query(),
            &mut BTreeMap::new(),
        )?;
        reference.region(
            &vertices,
            &[0, 1, 2, 0, 1, 2],
            0,
            &query(),
            &mut BTreeMap::new(),
        )?;
        assert_eq!(direct.positions, reference.positions);
        assert_eq!(direct.indices, reference.indices);
        assert_eq!(direct.weighted_normal, reference.weighted_normal);
        assert_eq!(direct.nearest_normal, reference.nearest_normal);
        assert_eq!(direct.nearest_squared, reference.nearest_squared);
        let project = |c: Collected| {
            c.project(
                [0.; 3],
                2.,
                1.,
                Some([1., 0., 0.]),
                || panic!(),
                |_| panic!(),
            )
        };
        let a = project(direct)?.unwrap();
        let b = project(reference)?.unwrap();
        assert_eq!(a.positions, b.positions);
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.uv, b.uv);
        Ok(())
    }
    #[test]
    fn interleaved_source_rejects_invalid_layout_and_index_width_before_mutation() {
        let mut out = Collected::new([0., 0., 1.]);
        for (vertices, stride, indices) in [
            (&[0.; 9][..], 0, &[0, 1, 2][..]),
            (&[0.; 9][..], 2, &[0, 1, 2][..]),
            (&[0.; 8][..], 3, &[0, 1, 2][..]),
            (&[0.; 9][..], 3, &[0, 1][..]),
            (&[0.; 9][..], 3, &[0, 1, 65536][..]),
        ] {
            assert!(out
                .interleaved_region(vertices, stride, indices, &query(), &mut BTreeMap::new())
                .is_err());
            assert!(out.positions.is_empty() && out.indices.is_empty());
        }
    }
    #[test]
    fn collection_reuses_mesh_vertices_and_rejects_backfaces() -> Result<()> {
        let vertices = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]];
        let mut out = Collected::new([0., 0., 1.]);
        let mut remap = BTreeMap::new();
        let q = query();
        assert!(out.region(&vertices, &[0, 1, 2, 2, 1, 3, 0, 2, 1], 0, &q, &mut remap)?);
        assert_eq!(
            out.positions,
            [[10., 20., 30.], [12., 20., 30.], [10., 22., 30.]]
        );
        // Second triangle lies beyond the inclusive squared-distance limit;
        // third has reversed winding. Repeating the first reuses all vertices.
        assert!(out.region(&vertices, &[0, 1, 2], 0, &q, &mut remap)?);
        assert_eq!(out.indices, [0, 1, 2, 0, 1, 2]);
        assert_eq!(out.positions.len(), 3);
        assert_eq!(out.nearest_squared, 1.);
        assert!(out.weighted_normal[2] > 1.99);
        let mut q = query();
        q.first_vertex_limit_squared = 0.;
        // Broadphase precedes fetching the other indices.
        assert!(out.region(&vertices, &[0, 999, 999], 0, &q, &mut remap)?);
        Ok(())
    }
    #[test]
    fn original_capacity_exit_keeps_partial_output_and_contributions() -> Result<()> {
        let vertices = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let mut out = Collected::new([0., 0., 1.]);
        let mut remap = BTreeMap::new();
        out.indices.resize(0xc00, 0);
        assert!(!out.region(&vertices, &[0, 1, 2], 65536, &query(), &mut remap)?);
        // Native inserts a new vertex before checking index capacity. Region
        // offsets truncate to 16 bits even for the 8-bit source-index variant.
        assert_eq!(out.positions.len(), 1);
        assert_eq!(out.indices.len(), 0xc00);
        assert_eq!(remap.get(&0), Some(&0));
        assert!(out.weighted_normal[2] > 0.99);
        Ok(())
    }
    #[test]
    fn distance_covers_interior_edges_vertices_and_height() {
        // Binary-exact inputs make these strict, tolerance-free expectations.
        for (point, coordinates, squared) in [
            ([0.25, 0.25, 2.], [0.25, 0.25], 4.),
            ([-1., 0.5, 0.], [0., 0.5], 1.),
            ([0.5, -1., 0.], [0.5, 0.], 1.),
            ([1., 1., 0.], [0.5, 0.5], 0.5),
            ([-1., -1., 0.], [0., 0.], 2.),
            ([2., -1., 0.], [1., 0.], 2.),
            ([-1., 2., 0.], [0., 1.], 2.),
            ([2., 2., 1.], [0.5, 0.5], 5.5),
        ] {
            assert_eq!(
                point_triangle(point, [0.; 3], [1., 0., 0.], [0., 1., 0.]),
                Distance {
                    coordinates,
                    squared
                },
                "point {point:?}"
            );
        }
        let degenerate = point_triangle([0.; 3], [0.; 3], [0.; 3], [0.; 3]);
        assert!(degenerate.squared.is_nan());
    }
    #[test]
    fn normal_keeps_winding_area_and_degenerate_cutoff() -> Result<()> {
        let (normal, area) = triangle_normal([2., 0., 0.], [0., 3., 0.])?;
        assert!((area - 6.).abs() < 1e-6);
        assert!((normal[2] - 1.).abs() < 2e-7);
        let (reverse, reverse_area) = triangle_normal([0., 3., 0.], [2., 0., 0.])?;
        assert_eq!(reverse_area.to_bits(), area.to_bits());
        assert_eq!(reverse[2].to_bits(), (-normal[2]).to_bits());
        assert_eq!(triangle_normal([1., 0., 0.], [2., 0., 0.])?, ([0.; 3], 0.));
        assert_eq!(
            triangle_normal([1e-4, 0., 0.], [0., 1e-4, 0.])?,
            ([0.; 3], 0.)
        );
        assert!(triangle_normal([f32::NAN, 0., 0.], [0.; 3]).is_err());
        Ok(())
    }
}
