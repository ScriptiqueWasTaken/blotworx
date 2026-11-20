//! Original shape AABB queries. Other shape types must not fall back to mesh
//! bounds: their cached bounds, child transforms and convex radii differ.
use super::native_hk_transform::Transform;
use anyhow::{ensure, Context, Result};

/// Native shape identities, separate from the flattened collision meshes.
/// Children must already exist on insertion, so cycles/invalid references
/// cannot enter the graph. Multiple parents may retain the same child.
#[derive(Clone)]
pub enum Shape {
    Box(BoxShape),
    Cylinder(CylinderShape),
    Convex(ConvexBounds),
    ExtendedMesh(ExtendedMeshBounds),
    List {
        bounds: ListBounds,
        children: std::sync::Arc<[usize]>,
    },
    Mopp {
        child: usize,
    },
    Translated {
        child: usize,
        translation: [f32; 4],
    },
    Transformed {
        child: usize,
        transform: Transform,
    },
}
#[derive(Default, Clone)]
pub struct ShapeGraph {
    nodes: Vec<Shape>,
}
impl ShapeGraph {
    /// Root collection selection for native query/material routing. MOPP
    /// materials require the Jigsaw component; List/ExtendedMesh do not.
    /// A nested wrapped collection needs its complete key chain, not flattening.
    pub(super) fn query_collection(&self, index: usize) -> Result<Option<bool>> {
        Ok(
            match self
                .nodes
                .get(index)
                .context("Missing query shape identity")?
            {
                Shape::Mopp { .. } => Some(true),
                Shape::List { .. } | Shape::ExtendedMesh(_) => Some(false),
                Shape::Translated { child, .. } | Shape::Transformed { child, .. } => {
                    ensure!(
                        self.query_collection(*child)?.is_none(),
                        "Wrapped collection query key chain is not reconstructed"
                    );
                    None
                }
                _ => None,
            },
        )
    }
    pub fn insert(&mut self, shape: Shape) -> Result<usize> {
        match &shape {
            Shape::Box(value) => value.validate()?,
            Shape::Cylinder(value) => value.validate()?,
            Shape::Convex(value) => value.validate()?,
            Shape::ExtendedMesh(value) => value.parameters().validate()?,
            Shape::List { bounds, children } => {
                ensure!(
                    bounds
                        .half
                        .iter()
                        .chain(bounds.center.iter())
                        .all(|v| v.is_finite()),
                    "Invalid list cache"
                );
                ensure!(
                    children.iter().all(|&i| i < self.nodes.len()),
                    "Invalid list child identity"
                );
            }
            Shape::Mopp { child } => {
                ensure!(*child < self.nodes.len(), "Invalid MOPP child identity")
            }
            Shape::Translated { child, translation } => {
                ensure!(
                    *child < self.nodes.len(),
                    "Invalid translated child identity"
                );
                ensure!(
                    translation.iter().all(|value| value.is_finite()),
                    "Nonfinite shape translation"
                );
            }
            Shape::Transformed { child, transform } => {
                ensure!(
                    *child < self.nodes.len(),
                    "Invalid transformed child identity"
                );
                ensure!(
                    transform.iter().all(|value| value.is_finite()),
                    "Nonfinite shape transform"
                );
            }
        }
        let index = self.nodes.len();
        self.nodes.push(shape);
        Ok(index)
    }
    pub fn aabb(
        &mut self,
        index: usize,
        transform: &Transform,
        tolerance: f32,
    ) -> Result<[[f32; 4]; 2]> {
        // Copy small parameters / retain the existing child slice. No vertices
        // or child vectors are rebuilt on a query.
        let shape = self
            .nodes
            .get(index)
            .context("Missing shape bounds identity")?
            .clone();
        match shape {
            Shape::Box(value) => value.aabb(transform, tolerance),
            Shape::Cylinder(value) => value.aabb(transform, tolerance),
            Shape::Convex(value) => value.aabb(transform, tolerance),
            Shape::ExtendedMesh(value) => value.aabb(transform, tolerance),
            Shape::Mopp { child } => self.aabb(child, transform, tolerance), // DOL 802767f0.
            Shape::Translated { child, translation } => {
                let local = super::native_hk_transform::from_entity(
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                    translation[..3].try_into().unwrap(),
                );
                let placed = super::native_hk_transform::compose(transform, &local);
                self.aabb(child, &placed, tolerance)
            }
            // DOL80278ddc: compose the query transform with the stored
            // hkTransform, then delegate to the original child with the same
            // tolerance. The wrapper must not add another convex radius.
            Shape::Transformed {
                child,
                transform: local,
            } => {
                let placed = super::native_hk_transform::compose(transform, &local);
                self.aabb(child, &placed, tolerance)
            }
            Shape::List {
                mut bounds,
                children,
            } => {
                let result = bounds.aabb(children.len(), transform, tolerance, |i, t, tol| {
                    self.aabb(children[i], t, tol)
                });
                // A completed local-cache rebuild survives a later query error.
                if let Shape::List { bounds: stored, .. } = &mut self.nodes[index] {
                    *stored = bounds;
                }
                result
            }
        }
    }
}

/// hkpListShape cached fields +30/+40. Child identity stays with the owning
/// shape tree; queries enumerate its full child array, not enabled-child bits.
#[derive(Clone, Copy)]
pub struct ListBounds {
    pub half: [f32; 4],
    pub center: [f32; 4],
}
impl ListBounds {
    pub fn decode(data: &[u8], shape: usize) -> Result<Self> {
        let bytes = data
            .get(
                shape
                    ..shape
                        .checked_add(0x50)
                        .context("List bounds offset overflow")?,
            )
            .context("Truncated list bounds")?;
        let vector = |at: usize| {
            std::array::from_fn(|i| {
                f32::from_be_bytes(bytes[at + i * 4..at + i * 4 + 4].try_into().unwrap())
            })
        };
        let result = Self {
            half: vector(0x30),
            center: vector(0x40),
        };
        ensure!(
            result
                .half
                .iter()
                .chain(result.center.iter())
                .all(|v| v.is_finite()),
            "Nonfinite stored list bounds"
        );
        Ok(result)
    }
    /// DOL 802728d4..80272bf0. The cache rebuild and requested-space pass are
    /// distinct observable query sequences; neither can be elided or fused.
    pub fn aabb(
        &mut self,
        count: usize,
        transform: &Transform,
        tolerance: f32,
        mut child: impl FnMut(usize, &Transform, f32) -> Result<[[f32; 4]; 2]>,
    ) -> Result<[[f32; 4]; 2]> {
        ensure!(
            self.half
                .iter()
                .chain(self.center.iter())
                .all(|v| v.is_finite())
                && transform.iter().all(|v| v.is_finite())
                && tolerance.is_finite()
                && tolerance >= 0.,
            "Invalid list bounds query"
        );
        // 8070b0a0 is initialized by 8014f25c from zero-filled 8070b0e0.
        if self.half.iter().all(|v| *v <= 0.) {
            ensure!(count != 0, "Uninitialized empty list bounds");
            let identity = super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0.; 3],
            );
            let mut bounds = child(0, &identity, 0.)?;
            validate_bounds(&bounds)?;
            for index in 1..count {
                let next = child(index, &identity, 0.)?;
                validate_bounds(&next)?;
                merge_bounds(&mut bounds, next);
            }
            // 8014f37c copies 806a96c0's four 3f000000 words to 8070b150.
            let center = std::array::from_fn(|i| (bounds[0][i] + bounds[1][i]) * 0.5);
            let half = std::array::from_fn(|i| (bounds[1][i] - bounds[0][i]) * 0.5);
            ensure!(
                center.iter().chain(half.iter()).all(|v| v.is_finite()),
                "List bounds cache overflow"
            );
            self.center = center;
            self.half = half;
        }
        let limit = f32::from_bits(0x7effffee);
        let mut bounds = [[limit; 4], [-limit; 4]];
        for index in 0..count {
            let next = child(index, transform, tolerance)?;
            validate_bounds(&next)?;
            merge_bounds(&mut bounds, next);
        }
        Ok(bounds)
    }
}
fn validate_bounds(bounds: &[[f32; 4]; 2]) -> Result<()> {
    ensure!(
        bounds.iter().flatten().all(|v| v.is_finite())
            && (0..3).all(|i| bounds[0][i] <= bounds[1][i]),
        "Invalid child shape bounds"
    );
    Ok(())
}
fn merge_bounds(bounds: &mut [[f32; 4]; 2], next: [[f32; 4]; 2]) {
    for i in 0..4 {
        // ps_sub/ps_sel keeps the old operand on equality, including signed zero.
        if !(next[0][i] - bounds[0][i] >= 0.) {
            bounds[0][i] = next[0][i];
        }
        if !(bounds[1][i] - next[1][i] >= 0.) {
            bounds[1][i] = next[1][i];
        }
    }
}

#[derive(Clone, Copy)]
pub struct ConvexBounds {
    pub half: [f32; 3],
    pub center: [f32; 3],
    pub radius: f32,
}

/// DOL 802705cc, inherited unchanged by hkpStorageExtendedMeshShape.
/// These cached bounds already describe the mesh; do not apply subpart scale
/// or triangle radius a second time during a bounds query.
#[derive(Clone, Copy)]
pub struct ExtendedMeshBounds {
    pub half: [f32; 3],
    pub center: [f32; 3],
}
impl ExtendedMeshBounds {
    pub fn decode(data: &[u8], shape: usize) -> Result<Self> {
        let bytes = data
            .get(
                shape
                    ..shape
                        .checked_add(0xb0)
                        .context("Mesh bounds offset overflow")?,
            )
            .context("Truncated extended mesh bounds")?;
        let float = |at: usize| f32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let result = Self {
            half: std::array::from_fn(|i| float(0x90 + i * 4)),
            center: std::array::from_fn(|i| float(0xa0 + i * 4)),
        };
        result.parameters().validate()?;
        Ok(result)
    }
    fn parameters(&self) -> ConvexBounds {
        ConvexBounds {
            half: self.half,
            center: self.center,
            radius: 0.,
        }
    }
    pub fn aabb(&self, transform: &Transform, tolerance: f32) -> Result<[[f32; 4]; 2]> {
        // Same extent and center arithmetic as convex bounds, with no radius.
        self.parameters().aabb(transform, tolerance)
    }
}
impl ConvexBounds {
    pub fn decode(data: &[u8], shape: usize) -> Result<Self> {
        Self::decode_endian(data, shape, false)
    }
    /// Verified 32-bit hkpConvexVerticesShape fields in Wii 7.0 / PC 2012.1.
    pub(super) fn decode_endian(data: &[u8], shape: usize, little: bool) -> Result<Self> {
        let bytes = data
            .get(
                shape
                    ..shape
                        .checked_add(0x40)
                        .context("Hull bounds offset overflow")?,
            )
            .context("Truncated hull bounds")?;
        let float = |at: usize| {
            let raw = bytes[at..at + 4].try_into().unwrap();
            if little { f32::from_le_bytes(raw) } else { f32::from_be_bytes(raw) }
        };
        let result = Self {
            half: std::array::from_fn(|i| float(0x20 + i * 4)),
            center: std::array::from_fn(|i| float(0x30 + i * 4)),
            radius: float(0x10),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.radius.is_finite() && self.radius >= 0.,
            "Invalid hull convex radius"
        );
        ensure!(
            self.half.iter().all(|v| v.is_finite() && *v >= 0.)
                && self.center.iter().all(|v| v.is_finite()),
            "Invalid hull cached bounds"
        );
        Ok(())
    }
    /// DOL 8027b630, convex-vertices vtable 806b8740 slot +24.
    /// Uses stored bounds, with translation inside the FIRST center FMA.
    pub fn aabb(&self, transform: &Transform, tolerance: f32) -> Result<[[f32; 4]; 2]> {
        self.validate()?;
        let extent = transformed_extent(self.half, self.radius, transform, tolerance)?;
        let center: [f32; 4] = std::array::from_fn(|i| {
            transform[8 + i].mul_add(
                self.center[2],
                transform[4 + i].mul_add(
                    self.center[1],
                    transform[i].mul_add(self.center[0], transform[12 + i]),
                ),
            )
        });
        let lower = std::array::from_fn(|i| -extent[i] + center[i]);
        let upper = std::array::from_fn(|i| extent[i] + center[i]);
        ensure!(
            lower.iter().chain(upper.iter()).all(|v| v.is_finite()),
            "Hull AABB overflow"
        );
        Ok([lower, upper])
    }
}

fn transformed_extent(
    half: [f32; 3],
    radius: f32,
    transform: &Transform,
    tolerance: f32,
) -> Result<[f32; 4]> {
    ensure!(
        transform.iter().all(|v| v.is_finite()) && tolerance.is_finite() && tolerance >= 0.,
        "Invalid shape AABB query"
    );
    let margin = tolerance + radius;
    Ok(std::array::from_fn(|i| {
        let x = (half[0] * transform[i]).abs();
        let y = (half[1] * transform[4 + i]).abs();
        let z = (half[2] * transform[8 + i]).abs();
        (x + y) + (z + margin)
    }))
}

#[derive(Clone, Copy)]
pub struct BoxShape {
    pub half: [f32; 3],
    pub radius: f32,
}

/// Havok cylinder defined by the centers of its two circular ends. Its convex
/// radius is kept separate from the cylinder radius, matching hkpConvexShape.
#[derive(Clone, Copy)]
pub struct CylinderShape {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub cylinder_radius: f32,
    pub convex_radius: f32,
}
impl CylinderShape {
    pub fn decode(data: &[u8], shape: usize) -> Result<Self> {
        let bytes = data
            .get(
                shape
                    ..shape
                        .checked_add(0x60)
                        .context("Cylinder offset overflow")?,
            )
            .context("Truncated cylinder shape")?;
        let float = |at: usize| f32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let vector = |at: usize| [float(at), float(at + 4), float(at + 8)];
        let result = Self {
            convex_radius: float(0x10),
            // Retail DOL hkClassMember table at file 0x66fd28..0x66fdb8:
            // cylRadius +14, vertexA +20, vertexB +30. +40/+50 are
            // perpendicular vectors, not endpoints. The complete shape is 60.
            cylinder_radius: float(0x14),
            a: vector(0x20),
            b: vector(0x30),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.a.iter().chain(&self.b).all(|v| v.is_finite())
                && self.cylinder_radius.is_finite()
                && self.cylinder_radius > 0.
                && self.convex_radius.is_finite()
                && self.convex_radius >= 0.,
            "Invalid cylinder shape"
        );
        ensure!(
            self.a
                .iter()
                .zip(self.b)
                .any(|(a, b)| (*a - b).abs() > 1e-6),
            "Degenerate cylinder axis"
        );
        Ok(())
    }
    pub fn aabb(&self, transform: &Transform, tolerance: f32) -> Result<[[f32; 4]; 2]> {
        self.validate()?;
        ensure!(
            transform.iter().all(|v| v.is_finite()) && tolerance.is_finite() && tolerance >= 0.,
            "Invalid cylinder AABB query"
        );
        let axis = std::array::from_fn::<_, 3, _>(|i| self.b[i] - self.a[i]);
        let length = axis.iter().map(|x| x * x).sum::<f32>().sqrt();
        let axis = axis.map(|v| v / length);
        let round_margin = self.convex_radius + tolerance;
        let extent: [f32; 3] = std::array::from_fn(|i| {
            let end_a: f32 = (0..3).map(|j| transform[j * 4 + i] * self.a[j]).sum();
            let end_b: f32 = (0..3).map(|j| transform[j * 4 + i] * self.b[j]).sum();
            let radial_axis: f32 = (0..3).map(|j| transform[j * 4 + i] * axis[j]).sum();
            (end_a - end_b).abs().max(0.) * 0.5
                + self.cylinder_radius * (1. - radial_axis * radial_axis).max(0.).sqrt()
                + round_margin
        });
        let center: [f32; 3] = std::array::from_fn(|i| {
            (0..3)
                .map(|j| transform[j * 4 + i] * ((self.a[j] + self.b[j]) * 0.5))
                .sum::<f32>()
                + transform[12 + i]
        });
        let lower = [
            center[0] - extent[0],
            center[1] - extent[1],
            center[2] - extent[2],
            0.,
        ];
        let upper = [
            center[0] + extent[0],
            center[1] + extent[1],
            center[2] + extent[2],
            0.,
        ];
        ensure!(
            lower.iter().chain(&upper).all(|v| v.is_finite()),
            "Cylinder AABB overflow"
        );
        Ok([lower, upper])
    }
    /// Convex vertices on the two end rings; Havok uses virtual tessellation.
    pub fn vertices(&self, segments: usize) -> Result<Vec<[f32; 3]>> {
        self.validate()?;
        // Eight vertices (four per end) make the stable circumscribed prism
        // used when the current collision backend rejects larger virtual
        // Havok cylinder hulls. Bounds and authored cylinder data stay exact.
        ensure!(
            segments >= 4 && segments % 4 == 0,
            "Invalid cylinder tessellation"
        );
        let axis = std::array::from_fn::<_, 3, _>(|i| self.b[i] - self.a[i]);
        let length = axis.iter().map(|x| x * x).sum::<f32>().sqrt();
        let axis = axis.map(|v| v / length);
        let seed = if axis[0].abs() < 0.8 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        };
        let cross = |u: [f32; 3], v: [f32; 3]| {
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        };
        let normalize = |v: [f32; 3]| {
            let l = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.map(|x| x / l)
        };
        let u = normalize(cross(axis, seed));
        let v = cross(axis, u);
        let mut out = Vec::with_capacity(segments * 2);
        let polygon_radius = self.cylinder_radius / (std::f32::consts::PI / segments as f32).cos();
        for end in [self.a, self.b] {
            for i in 0..segments {
                // Circumscribe the native circle so the low-sided fallback
                // never shrinks collision space beneath the authored radius.
                let angle = std::f32::consts::TAU * i as f32 / segments as f32
                    + std::f32::consts::PI / segments as f32;
                let (sin, cos) = angle.sin_cos();
                out.push(std::array::from_fn(|j| {
                    end[j] + polygon_radius * (u[j] * cos + v[j] * sin)
                }));
            }
        }
        Ok(out)
    }
}
impl BoxShape {
    pub fn decode(data: &[u8], shape: usize) -> Result<Self> {
        let bytes = data
            .get(
                shape
                    ..shape
                        .checked_add(0x30)
                        .context("Box shape offset overflow")?,
            )
            .context("Truncated box shape")?;
        let float = |at: usize| f32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let result = Self {
            half: [float(0x20), float(0x24), float(0x28)],
            radius: float(0x10),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.radius.is_finite() && self.radius >= 0.,
            "Invalid box convex radius"
        );
        // Retail 2D invisible walls include hkpBoxShape with an exact zero
        // thickness lane. Preserve the plane and its authored convex radius.
        ensure!(
            self.half.iter().all(|v| v.is_finite() && *v >= 0.),
            "Invalid box half extents"
        );
        Ok(())
    }
    /// DOL 80277274..80277434, box vtable 806b8500 slot +24.
    /// Four output lanes are calculated by the paired operations, including W.
    pub fn aabb(&self, transform: &Transform, tolerance: f32) -> Result<[[f32; 4]; 2]> {
        self.validate()?;
        let extent = transformed_extent(self.half, self.radius, transform, tolerance)?;
        let lower = std::array::from_fn(|i| -extent[i] + transform[12 + i]);
        let upper = std::array::from_fn(|i| extent[i] + transform[12 + i]);
        ensure!(
            lower.iter().chain(upper.iter()).all(|v| v.is_finite()),
            "Box AABB overflow"
        );
        Ok([lower, upper])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_gear_cylinder_uses_reflected_offsets_and_exact_object_size() {
        // GV Zone D p_gear_v01_dynamic_simple.hkx_wii cylinder at data+680.
        // Include perpendicular vectors to catch the old endpoint misread.
        let mut bytes = [0u8; 0x60];
        for (at, values) in [
            (0x10, [0.05f32, 0.5349591, 0.8, 0.]),
            (0x20, [2.9802322e-8, 0., 0.08651873, 0.5849591]),
            (0x30, [2.9802322e-8, 0., -0.08651873, 0.5849591]),
            (0x40, [0., -0.99999994, -0., 0.]),
            (0x50, [-1., 0., -0., 0.]),
        ] {
            for (lane, value) in values.into_iter().enumerate() {
                bytes[at + lane * 4..at + lane * 4 + 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        let cylinder = CylinderShape::decode(&bytes, 0).unwrap();
        assert_eq!(cylinder.cylinder_radius, 0.5349591);
        assert_eq!(cylinder.a, [2.9802322e-8, 0., 0.08651873]);
        assert_eq!(cylinder.b, [2.9802322e-8, 0., -0.08651873]);
        assert_eq!(cylinder.convex_radius, 0.05);
        assert!(CylinderShape::decode(&bytes[..0x5f], 0).is_err());
    }
    #[test]
    fn retail_planar_box_preserves_zero_thickness_and_convex_radius() {
        let mut bytes = [0u8; 0x30];
        for (offset, value) in [
            (0x10, 0.05f32),
            (0x20, 0.),
            (0x24, 22.107074737548828),
            (0x28, 4.074029445648193),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let shape = BoxShape::decode(&bytes, 0).unwrap();
        assert_eq!(shape.half[0].to_bits(), 0);
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let bounds = shape.aabb(&transform, 0.).unwrap();
        assert_eq!(bounds[0][0], -0.05);
        assert_eq!(bounds[1][0], 0.05);
        bytes[0x20..0x24].copy_from_slice(&(-1f32).to_be_bytes());
        assert!(BoxShape::decode(&bytes, 0).is_err());
    }
    #[test]
    fn extended_mesh_queries_cached_bounds_without_rescaling_or_radius_expansion() {
        let mut data = [0; 0xf0];
        for (at, v) in [
            (0x80, 100f32),
            (0x84, 100.),
            (0x88, 100.),
            (0xe4, 100.),
            (0x90, 1.),
            (0x94, 2.),
            (0x98, 3.),
            (0xa0, 4.),
            (0xa4, 5.),
            (0xa8, 6.),
        ] {
            data[at..at + 4].copy_from_slice(&v.to_be_bytes());
        }
        let bounds = ExtendedMeshBounds::decode(&data, 0).unwrap();
        let t = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        assert_eq!(
            bounds.aabb(&t, 0.25).unwrap(),
            [[2.75, 22.75, 32.75, -0.25], [7.25, 25.25, 39.25, 0.25]]
        );
        assert!(ExtendedMeshBounds::decode(&data[..0xaf], 0).is_err());
    }
    #[test]
    fn graph_routes_mopp_and_shared_children_into_actual_list_cache() {
        let mut graph = ShapeGraph::default();
        assert!(graph.insert(Shape::Mopp { child: 0 }).is_err());
        let leaf = graph
            .insert(Shape::Box(BoxShape {
                half: [1., 2., 3.],
                radius: 0.25,
            }))
            .unwrap();
        let hull = graph
            .insert(Shape::Convex(ConvexBounds {
                half: [1.; 3],
                center: [4., 0., 0.],
                radius: 0.5,
            }))
            .unwrap();
        let list = graph
            .insert(Shape::List {
                bounds: ListBounds {
                    half: [0.; 4],
                    center: [0.; 4],
                },
                children: vec![leaf, hull, leaf].into(),
            })
            .unwrap();
        let root = graph.insert(Shape::Mopp { child: list }).unwrap();
        let other = graph.insert(Shape::Mopp { child: list }).unwrap();
        let t = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let expected = [[-1.25, -2.25, -3.25, -0.5], [5.5, 2.25, 3.25, 0.5]];
        assert_eq!(graph.aabb(root, &t, 0.).unwrap(), expected);
        match &graph.nodes[list] {
            Shape::List { bounds, children } => {
                assert_eq!(bounds.half, [3.375, 2.25, 3.25, 0.5]);
                assert_eq!(bounds.center, [2.125, 0., 0., 0.]);
                assert_eq!(children[0], children[2]);
            }
            _ => panic!("Expected list"),
        }
        assert_eq!(graph.aabb(other, &t, 0.).unwrap(), expected);
        assert_eq!(graph.nodes.len(), 5);
        assert!(graph.aabb(5, &t, 0.).is_err());
    }
    #[test]
    fn imported_list_cache_preserves_all_lanes_and_skips_rebuild() {
        let mut bytes = [0u8; 0x50];
        // W participates in the cache-validity comparison, unlike most XYZ tests.
        bytes[0x3c..0x40].copy_from_slice(&1f32.to_be_bytes());
        bytes[0x40..0x44].copy_from_slice(&7f32.to_be_bytes());
        let mut list = ListBounds::decode(&bytes, 0).unwrap();
        let t = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [9., 0., 0.],
        );
        let mut calls = 0;
        list.aabb(1, &t, 0.25, |_, actual, tolerance| {
            calls += 1;
            assert_eq!(*actual, t);
            assert_eq!(tolerance, 0.25);
            Ok([[-1.; 4], [1.; 4]])
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(list.half, [0., 0., 0., 1.]);
        assert_eq!(list.center, [7., 0., 0., 0.]);
        assert!(ListBounds::decode(&bytes[..0x4f], 0).is_err());
        assert!(ListBounds::decode(&bytes, usize::MAX).is_err());
    }
    #[test]
    fn list_rebuilds_locally_before_querying_every_child_in_requested_space() {
        let children = [
            BoxShape {
                half: [1., 2., 3.],
                radius: 0.25,
            },
            BoxShape {
                half: [4., 1., 1.],
                radius: 0.5,
            },
        ];
        let mut list = ListBounds {
            half: [0.; 4],
            center: [0.; 4],
        };
        let mut calls = Vec::new();
        let t = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let bounds = list
            .aabb(2, &t, 0.25, |i, t, tolerance| {
                calls.push((i, t[12], tolerance));
                children[i].aabb(t, tolerance)
            })
            .unwrap();
        assert_eq!(
            calls,
            [(0, 0., 0.), (1, 0., 0.), (0, 10., 0.25), (1, 10., 0.25)]
        );
        assert_eq!(list.center, [0.; 4]);
        assert_eq!(list.half, [4.5, 2.25, 3.25, 0.5]);
        assert_eq!(
            bounds,
            [[5.25, 17.5, 26.5, -0.75], [14.75, 22.5, 33.5, 0.75]]
        );
        calls.clear();
        list.aabb(2, &t, 0., |i, t, tolerance| {
            calls.push((i, t[12], tolerance));
            children[i].aabb(t, tolerance)
        })
        .unwrap();
        assert_eq!(calls, [(0, 10., 0.), (1, 10., 0.)]);
    }
    #[test]
    fn list_query_failure_keeps_completed_cache_and_zero_ties_keep_first_bits() {
        let mut list = ListBounds {
            half: [0.; 4],
            center: [0.; 4],
        };
        let t = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [9., 0., 0.],
        );
        assert!(list
            .aabb(1, &t, 0., |_, t, _| {
                anyhow::ensure!(t[12] == 0., "Requested query failed");
                Ok([[-1.; 4], [1.; 4]])
            })
            .is_err());
        assert_eq!(list.half, [1.; 4]);
        let mut bounds = [[-0.; 4], [0.; 4]];
        merge_bounds(&mut bounds, [[0.; 4], [-0.; 4]]);
        assert_eq!(bounds[0].map(f32::to_bits), [(-0f32).to_bits(); 4]);
        assert_eq!(bounds[1].map(f32::to_bits), [0; 4]);
    }
    #[test]
    fn hull_bounds_transform_cached_center_and_preserve_fused_order() {
        let hull = ConvexBounds {
            half: [1., 2., 3.],
            center: [4., 5., 6.],
            radius: 0.25,
        };
        let t = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        assert_eq!(
            hull.aabb(&t, 0.25).unwrap(),
            [[2.5, 22.5, 32.5, -0.5], [7.5, 25.5, 39.5, 0.5]]
        );
        // Translation must enter before the Y contribution: (2^24 + -2^24)+1.
        let hull = ConvexBounds {
            half: [0.; 3],
            center: [16777216., 1., 0.],
            radius: 0.,
        };
        let mut t = [0.; 16];
        t[0] = 1.;
        t[4] = 1.;
        t[12] = -16777216.;
        assert_eq!(hull.aabb(&t, 0.).unwrap()[0][0], 1.);
        let mut bytes = [0; 0x40];
        bytes[0x30..0x34].copy_from_slice(&4f32.to_be_bytes());
        assert_eq!(
            ConvexBounds::decode(&bytes, 0).unwrap().center,
            [4., 0., 0.]
        );
        assert!(ConvexBounds::decode(&bytes[..0x3f], 0).is_err());
    }
    #[test]
    fn rotated_box_uses_core_extents_and_separate_margin_in_all_lanes() {
        let shape = BoxShape {
            half: [1., 2., 3.],
            radius: 0.25,
        };
        let t = super::super::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        assert_eq!(
            shape.aabb(&t, 0.25).unwrap(),
            [[7.5, 18.5, 26.5, -0.5], [12.5, 21.5, 33.5, 0.5]]
        );
        assert!(shape.aabb(&t, -1.).is_err());
    }
    #[test]
    fn decoded_box_supplies_center_update_bounds_without_rebuilding_vertices() {
        let mut bytes = [0; 0x30];
        for (at, value) in [(0x10, 0.25f32), (0x20, 1.), (0x24, 2.), (0x28, 3.)] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let shape = BoxShape::decode(&bytes, 0).unwrap();
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut motion = super::super::native_motion_record::MotionRecord([0; 0x120]);
        motion.0[8] = 3;
        motion.set_transform(identity).unwrap();
        motion
            .set_center_with_bounds([1., 0., 0., 0.], |_| shape.aabb(&identity, 0.))
            .unwrap();
        let radius = f32::from_be_bytes(motion.0[0xb0..0xb4].try_into().unwrap());
        assert!((radius - 20.6875f32.sqrt()).abs() < 0.000002);
        assert!(BoxShape::decode(&bytes[..0x2f], 0).is_err());
    }
}
