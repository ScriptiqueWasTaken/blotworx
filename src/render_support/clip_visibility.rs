//! GPU-only conservative rejection against the four lateral homogeneous
//! clip planes. This is not the game's entity/animation visibility scheduler.
//! Keep ambiguous cases, including denormals, overflow and unknown shaders.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}
impl Bounds {
    pub fn from_interleaved(vertices: &[f32]) -> Option<Self> {
        if vertices.is_empty() || vertices.len() % 12 != 0 {
            return None;
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in vertices.chunks_exact(12) {
            for axis in 0..3 {
                let value = vertex[axis];
                if !value.is_finite() || value.is_subnormal() {
                    return None;
                }
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        Some(Self { min, max })
    }
    pub fn outside(self, mvp: &[f32; 16]) -> bool {
        if mvp.iter().any(|v| !v.is_finite() || v.is_subnormal()) {
            return false;
        }
        let extent: [f64; 4] = [
            self.min[0].abs().max(self.max[0].abs()) as f64,
            self.min[1].abs().max(self.max[1].abs()) as f64,
            self.min[2].abs().max(self.max[2].abs()) as f64,
            1.,
        ];
        let absolute: [f64; 4] = std::array::from_fn(|row| {
            (0..4)
                .map(|col| (mvp[col * 4 + row] as f64).abs() * extent[col])
                .sum()
        });
        // Any intermediate f32 overflow invalidates the arithmetic model.
        if absolute.iter().any(|v| *v > f32::MAX as f64) {
            return false;
        }
        // Budget 32 roundings for four products, their sums and clipping
        // comparisons (more than the unfused scalar path). Include FTZ error
        // and f64 interval-evaluation rounding; no image-tuned epsilon.
        let u = 2f64.powi(-24);
        let gamma = 32. * u / (1. - 32. * u) + 32. * f64::EPSILON;
        for row in 0..2 {
            for sign in [-1., 1.] {
                let coefficients: [f64; 4] = std::array::from_fn(|col| {
                    mvp[col * 4 + 3] as f64 + sign * mvp[col * 4 + row] as f64
                });
                let mut upper = coefficients[3];
                for col in 0..3 {
                    upper += coefficients[col]
                        * if coefficients[col] >= 0. {
                            self.max[col] as f64
                        } else {
                            self.min[col] as f64
                        };
                }
                let error = gamma * (absolute[3] + absolute[row]) + 32. * f32::MIN_POSITIVE as f64;
                if upper < -error {
                    return true;
                }
            }
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn vertices(points: &[[f32; 3]]) -> Vec<f32> {
        points
            .iter()
            .flat_map(|p| {
                let mut v = [0.; 12];
                v[..3].copy_from_slice(p);
                v
            })
            .collect()
    }
    fn identity() -> [f32; 16] {
        [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ]
    }
    #[test]
    fn side_planes_reject_only_separated_bounds_and_keep_crossing_triangles() {
        for (axis, sign) in [(0, 1.), (0, -1.), (1, 1.), (1, -1.)] {
            let points = std::array::from_fn::<_, 3, _>(|i| {
                let mut p = [0.; 3];
                p[axis] = sign * (2. + i as f32 * 0.1);
                p[1 - axis] = i as f32 * 0.1;
                p
            });
            assert!(Bounds::from_interleaved(&vertices(&points))
                .unwrap()
                .outside(&identity()));
        }
        let full = [[-4., -4., 0.], [4., -4., 0.], [0., 4., 0.]];
        assert!(!Bounds::from_interleaved(&vertices(&full))
            .unwrap()
            .outside(&identity()));
        let touch = [[1., -0.5, 0.], [1., 0.5, 0.], [1., 0., 0.]];
        assert!(!Bounds::from_interleaved(&vertices(&touch))
            .unwrap()
            .outside(&identity()));
        let mut uncertain = touch;
        for p in &mut uncertain {
            p[0] = f32::from_bits(1f32.to_bits() + 1);
        }
        assert!(!Bounds::from_interleaved(&vertices(&uncertain))
            .unwrap()
            .outside(&identity()));
    }
    #[test]
    fn malformed_subnormal_and_overflow_data_never_reject() {
        assert!(Bounds::from_interleaved(&[0.; 13]).is_none());
        for bad in [f32::NAN, f32::INFINITY, f32::from_bits(1)] {
            assert!(Bounds::from_interleaved(&vertices(&[[bad, 0., 0.]])).is_none());
        }
        let bounds = Bounds::from_interleaved(&vertices(&[[2., 0., 0.]])).unwrap();
        for bad in [f32::NAN, f32::INFINITY, f32::from_bits(1)] {
            let mut m = identity();
            m[0] = bad;
            assert!(!bounds.outside(&m));
        }
        let mut m = identity();
        m[0] = f32::MAX;
        assert!(!bounds.outside(&m));
        // No dependence on depth-clamp or clip-control state: Z is not culled.
        assert!(!Bounds::from_interleaved(&vertices(&[[0., 0., 1e10]]))
            .unwrap()
            .outside(&identity()));
    }
    #[test]
    fn rejected_random_boxes_have_a_common_outside_plane_for_f32_vertices() {
        let mut seed = 0x9e3779b9u32;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as i32 as f32) / (i32::MAX as f32)
        };
        let mut rejected = 0;
        for _ in 0..2048 {
            let matrix = std::array::from_fn(|_| random() * 8.);
            let points =
                std::array::from_fn::<_, 8, _>(|_| std::array::from_fn(|_| random() * 16.));
            let bounds = Bounds::from_interleaved(&vertices(&points)).unwrap();
            if !bounds.outside(&matrix) {
                continue;
            }
            rejected += 1;
            let clip: Vec<[f32; 4]> = points
                .iter()
                .map(|p| {
                    std::array::from_fn(|row| {
                        matrix[row] * p[0]
                            + matrix[4 + row] * p[1]
                            + matrix[8 + row] * p[2]
                            + matrix[12 + row]
                    })
                })
                .collect();
            assert!((0..2).any(|row| [-1., 1.]
                .iter()
                .any(|sign| clip.iter().all(|v| v[3] + sign * v[row] < 0.))));
        }
        assert!(rejected > 0);
    }
}
