//! Retail shadow bicone construction15fdb8..15ffc8 and component bounds3b4ff8.
//! Ownership/enrollment, the depth-mask passes and composite are not connected.
use anyhow::Result;
#[derive(Clone, Debug)]
pub struct Geometry {
    pub positions: [[f32; 3]; 18],
    pub indices: [u16; 96],
}
impl Geometry {
    /// Caller supplies the sin/cos provider. Do not hide host libm differences
    /// behind an assertion that the retail trigonometric library is reproduced.
    pub fn build(
        heights: [f32; 3],
        mut trig: impl FnMut(f32) -> Result<(f64, f64)>,
    ) -> Result<Self> {
        let [top, ring, bottom] = heights;
        let mut positions = [[0.; 3]; 18];
        positions[0] = [0., top, 0.];
        positions[17] = [0., bottom, 0.];
        let mut angle = 0f32;
        for position in &mut positions[1..17] {
            let (sin, cos) = trig(angle)?;
            *position = [(cos as f32) * 0.5, ring, (sin as f32) * 0.5];
            // Original loop accumulates fadds, rather than computing i*angle.
            angle += f32::from_bits(0x3ec90fdb);
        }
        let mut indices = [0; 96];
        for i in 0..16 {
            let current = i as u16 + 1;
            let next = if i == 15 { 1 } else { current + 1 };
            indices[i * 3..i * 3 + 3].copy_from_slice(&[0, next, current]);
            indices[48 + i * 3..51 + i * 3].copy_from_slice(&[17, current, next]);
        }
        Ok(Self { positions, indices })
    }
    /// Constructor17abb0..17ac24 uses these values for both face-culling copies.
    pub fn retail(trig: impl FnMut(f32) -> Result<(f64, f64)>) -> Result<Self> {
        Self::build([f32::from_bits(0x3c23d70a), 0., -10.], trig)
    }
}
/// Exact component arithmetic3b505c..3b50a8; this is the native conservative
/// bound calculation, not an AABB fit or a corrected replacement formula.
pub fn local_bound(scale: [f32; 3]) -> ([f32; 3], f32) {
    let extent = -10. * scale[1];
    let center = extent * 0.5;
    ([0., center, 0.], (10. * scale[1]) * 0.5)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bicone_matches_original_instruction_vertices_and_topology() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/shadow_geometry.json"
        ))?;
        for case in data["cases"].as_array().unwrap() {
            let heights = std::array::from_fn(|i| case["heights"][i].as_f64().unwrap() as f32);
            let calls = case["calls"].as_array().unwrap();
            let mut index = 0;
            let mut provider = |angle: f32| -> Result<(f64, f64)> {
                let sin = &calls[index];
                let cos = &calls[index + 1];
                index += 2;
                assert_eq!(sin["kind"], "sin");
                assert_eq!(cos["kind"], "cos");
                assert_eq!(
                    angle.to_bits(),
                    (sin["argument"].as_f64().unwrap() as f32).to_bits()
                );
                assert_eq!(sin["argument"], cos["argument"]);
                Ok((
                    sin["result"].as_f64().unwrap(),
                    cos["result"].as_f64().unwrap(),
                ))
            };
            let mesh = Geometry::build(heights, &mut provider)?;
            assert_eq!(index, 32);
            for (i, p) in mesh.positions.iter().enumerate() {
                for j in 0..3 {
                    assert_eq!(
                        p[j].to_bits(),
                        case["vertex_bits"][i][j].as_u64().unwrap() as u32,
                        "vertex{i} axis{j}"
                    );
                }
            }
            for (i, &v) in mesh.indices.iter().enumerate() {
                assert_eq!(v as u64, case["indices"][i].as_u64().unwrap());
            }
            // Every edge has one face in each direction: no seam or open cap.
            let mut edges = std::collections::BTreeMap::<(u16, u16), i32>::new();
            for t in mesh.indices.chunks_exact(3) {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    *edges.entry((a.min(b), a.max(b))).or_default() += if a < b { 1 } else { -1 };
                }
            }
            assert!(edges.values().all(|&n| n == 0));
            assert_eq!(edges.len(), 48);
        }
        Ok(())
    }
    #[test]
    fn retail_constructor_and_component_bound_preserve_original_dimensions() -> Result<()> {
        let mesh = Geometry::retail(|angle| Ok(((angle as f64).sin(), (angle as f64).cos())))?;
        assert_eq!(mesh.positions[0], [0., f32::from_bits(0x3c23d70a), 0.]);
        assert_eq!(mesh.positions[17], [0., -10., 0.]);
        assert_eq!(mesh.positions[1], [0.5, 0., 0.]);
        assert_eq!(local_bound([0.5, 0.3, 0.5]), ([0., -1.5, 0.], 1.5));
        assert_eq!(local_bound([20., 0.3, 10.]), ([0., -1.5, 0.], 1.5));
        Ok(())
    }
}
