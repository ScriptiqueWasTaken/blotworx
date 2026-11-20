//! Conservative clustered forward-light admission. Tile/depth counts divide
//! space; they never cap the number of contributing lights in a cluster.
use super::viewport3d::GpuLight;
#[derive(Clone, Copy, Debug)]
struct Bound {
    lo: [f32; 3],
    hi: [f32; 3],
}
#[derive(Default)]
pub struct Grid {
    pub dims: [i32; 3],
    pub near: f32,
    pub far: f32,
    pub perspective: bool,
    pub rect: [i32; 4],
    projection: [f32; 16],
    bounds: Vec<Bound>,
    cells: Vec<Vec<i32>>,
    pub indices: Vec<i32>,
    pub candidates: usize,
    pub admitted: usize,
}
fn point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[12 + r] + (0..3).map(|c| m[c * 4 + r] * p[c]).sum::<f32>())
}
fn intersects(b: Bound, p: [f32; 3], radius: f32) -> bool {
    let d = (0..3)
        .map(|i| {
            let v = (b.lo[i] - p[i]).max(0.).max(p[i] - b.hi[i]);
            v * v
        })
        .sum::<f32>();
    d <= (radius + radius.abs() * 1e-5 + 1e-4).powi(2)
}
fn depth_plane(near: f32, far: f32, perspective: bool, t: f32) -> f32 {
    if perspective {
        near * (far / near).powf(t)
    } else {
        near + (far - near) * t
    }
}
impl Grid {
    pub fn build(
        &mut self,
        lights: &[GpuLight],
        view: [f32; 16],
        projection: [f32; 16],
        rect: [i32; 4],
    ) {
        let dims = [(rect[2] + 63) / 64, (rect[3] + 63) / 64, 16];
        let perspective = projection[11] != 0.;
        let near = if perspective {
            projection[14] / (projection[10] - 1.)
        } else {
            (projection[14] + 1.) / projection[10]
        };
        let far = if perspective {
            projection[14] / (projection[10] + 1.)
        } else {
            (projection[14] - 1.) / projection[10]
        };
        let resized = self.rect[2..] != rect[2..];
        self.near = near.max(0.0001);
        self.far = far.max(self.near + 0.001);
        self.perspective = perspective;
        self.rect = rect;
        if self.dims != dims || self.projection != projection || resized {
            self.dims = dims;
            self.projection = projection;
            let count = (dims[0] * dims[1] * dims[2]) as usize;
            self.bounds.clear();
            self.cells.resize_with(count, Vec::new);
            for z in 0..dims[2] {
                let z0 = depth_plane(self.near, self.far, perspective, z as f32 / dims[2] as f32);
                let z1 = depth_plane(
                    self.near,
                    self.far,
                    perspective,
                    (z + 1) as f32 / dims[2] as f32,
                );
                for y in 0..dims[1] {
                    for x in 0..dims[0] {
                        let mut lo = [f32::INFINITY; 3];
                        let mut hi = [f32::NEG_INFINITY; 3];
                        for d in [z0, z1] {
                            for u in [x, x + 1] {
                                for v in [y, y + 1] {
                                    let nx =
                                        (u * 64).min(rect[2]) as f32 / rect[2] as f32 * 2. - 1.;
                                    let ny =
                                        (v * 64).min(rect[3]) as f32 / rect[3] as f32 * 2. - 1.;
                                    let p = if perspective {
                                        [
                                            (nx + projection[8]) * d / projection[0],
                                            (ny + projection[9]) * d / projection[5],
                                            -d,
                                        ]
                                    } else {
                                        [
                                            (nx - projection[12]) / projection[0],
                                            (ny - projection[13]) / projection[5],
                                            -d,
                                        ]
                                    };
                                    for i in 0..3 {
                                        lo[i] = lo[i].min(p[i]);
                                        hi[i] = hi[i].max(p[i]);
                                    }
                                }
                            }
                        }
                        self.bounds.push(Bound { lo, hi });
                    }
                }
            }
        }
        for c in &mut self.cells {
            c.clear()
        }
        self.candidates = 0;
        self.admitted = 0;
        for (index, l) in lights.iter().enumerate() {
            if l.dimmer == 0. {
                continue;
            }
            let p = point(&view, l.position);
            let (z0, z1) = if l.range > 0. {
                let lo = (-p[2] - l.range).max(self.near);
                let hi = (-p[2] + l.range).min(self.far);
                if lo > hi {
                    continue;
                }
                (self.slice(lo), self.slice(hi))
            } else {
                (0, dims[2] - 1)
            };
            let (mut x0, mut x1, mut y0, mut y1) = (0, dims[0] - 1, 0, dims[1] - 1);
            if l.range > 0. && -p[2] - l.range > self.near {
                let mut lo = [f32::INFINITY; 2];
                let mut hi = [f32::NEG_INFINITY; 2];
                for x in [-l.range, l.range] {
                    for y in [-l.range, l.range] {
                        for z in [-l.range, l.range] {
                            let q = [p[0] + x, p[1] + y, p[2] + z];
                            let ndc = if perspective {
                                [
                                    projection[0] * q[0] / -q[2] - projection[8],
                                    projection[5] * q[1] / -q[2] - projection[9],
                                ]
                            } else {
                                [
                                    projection[0] * q[0] + projection[12],
                                    projection[5] * q[1] + projection[13],
                                ]
                            };
                            for i in 0..2 {
                                lo[i] = lo[i].min(ndc[i]);
                                hi[i] = hi[i].max(ndc[i]);
                            }
                        }
                    }
                }
                if hi[0] < -1. || lo[0] > 1. || hi[1] < -1. || lo[1] > 1. {
                    continue;
                }
                x0 = (((lo[0] * 0.5 + 0.5) * rect[2] as f32 / 64.).floor() as i32)
                    .clamp(0, dims[0] - 1);
                x1 = (((hi[0] * 0.5 + 0.5) * rect[2] as f32 / 64.).floor() as i32)
                    .clamp(0, dims[0] - 1);
                y0 = (((lo[1] * 0.5 + 0.5) * rect[3] as f32 / 64.).floor() as i32)
                    .clamp(0, dims[1] - 1);
                y1 = (((hi[1] * 0.5 + 0.5) * rect[3] as f32 / 64.).floor() as i32)
                    .clamp(0, dims[1] - 1);
            }
            for z in z0..=z1 {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let cell = ((z * dims[1] + y) * dims[0] + x) as usize;
                        self.candidates += 1;
                        if l.range <= 0. || intersects(self.bounds[cell], p, l.range) {
                            self.cells[cell].push(index as i32);
                            self.admitted += 1;
                        }
                    }
                }
            }
        }
        self.indices.clear();
        let headers = self.cells.len() * 2;
        self.indices.resize(headers, 0);
        for (i, cell) in self.cells.iter().enumerate() {
            self.indices[i * 2] = self.indices.len() as i32;
            self.indices[i * 2 + 1] = cell.len() as i32;
            self.indices.extend_from_slice(cell);
        }
    }
    fn slice(&self, z: f32) -> i32 {
        let t = if self.perspective {
            (z / self.near).ln() / (self.far / self.near).ln()
        } else {
            (z - self.near) / (self.far - self.near)
        };
        ((t * self.dims[2] as f32).floor() as i32).clamp(0, self.dims[2] - 1)
    }
    pub fn cells(&self) -> usize {
        self.cells.len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn projection() -> [f32; 16] {
        [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., -1.002002, -1., 0., 0., -0.2002002, 0.,
        ]
    }
    #[test]
    fn unbounded_light_reaches_every_cluster_without_a_count_cap() {
        let mut g = Grid::default();
        let mut l = GpuLight::default();
        l.range = 0.;
        let mut view = [0.; 16];
        for i in [0, 5, 10, 15] {
            view[i] = 1.;
        }
        let lights = vec![l; 160];
        g.build(&lights, view, projection(), [0, 0, 128, 128]);
        assert_eq!(g.cells(), 64);
        for i in 0..g.cells() {
            assert_eq!(g.indices[i * 2 + 1], 160);
        }
    }
    #[test]
    fn finite_light_has_local_membership_and_stays_inside_view_frustum() {
        let mut g = Grid::default();
        let mut l = GpuLight::default();
        l.position = [0., 0., -5.];
        l.range = 0.5;
        let mut view = [0.; 16];
        for i in [0, 5, 10, 15] {
            view[i] = 1.;
        }
        g.build(&[l], view, projection(), [0, 0, 256, 256]);
        let selected = (0..g.cells()).filter(|&i| g.indices[i * 2 + 1] > 0).count();
        assert!(selected > 0 && selected < g.cells() / 2);
    }
}
