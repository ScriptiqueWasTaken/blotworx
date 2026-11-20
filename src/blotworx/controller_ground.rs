//! Ground-probe arithmetic/selection, REL305660..305918.
//! Actual filtered cast and original collector sorting remain backend-owned.
pub const MARGIN: f32 = f32::from_bits(0x3c23d70a);
pub const NORMAL_THRESHOLD: f32 = f32::from_bits(0x3f547ae1);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: [f32; 4],
    pub end: [f32; 4],
    pub margin: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub position: [f32; 4],
    pub normal: [f32; 4],
    pub body_filter: u32,
}
/// End is computed before the input pointer is raised. Paired add affects all
/// lanes, including signed-zero padding; it is not a Y-only assignment.
pub fn prepare(position: &mut [f32; 4], distance: f32) -> Segment {
    let end = std::array::from_fn(|i| position[i] + if i == 1 { -distance } else { 0. });
    for (i, value) in position.iter_mut().enumerate() {
        *value += if i == 1 { MARGIN } else { 0. };
    }
    Segment {
        start: *position,
        end,
        margin: MARGIN,
    }
}
/// Hits must already have native collector order (sort80268b9c, key+1c).
/// Do not reorder equal keys or substitute Box3D traversal order.
pub fn select(position: [f32; 4], distance: f32, hits: &[Hit]) -> f32 {
    for hit in hits {
        if let Some(height) = eligible_height(position, hit) {
            return height;
        }
    }
    distance
}
fn eligible_height(position: [f32; 4], hit: &Hit) -> Option<f32> {
    if hit.body_filter & 31 == 3 {
        return None;
    }
    // Match ps_mul/ps_madd/ps_sum0; W is not in the final XYZ dot.
    // Reducing this to normal.y would change nonfinite-input behavior.
    let dot = hit.normal[2].mul_add(0., hit.normal[0] * 0.) + hit.normal[1] * 1.;
    if dot > NORMAL_THRESHOLD {
        let height = position[1] - hit.position[1];
        return Some(if height < 0. { 0. } else { height });
    }
    None
}
/// REL3057dc..3058d8: sort, lazily convert, then read each candidate's body
/// filter+1c in order. Stop after the first accepted normal; do not prefetch
/// filters for later contacts or use child material filters in this test.
pub fn select_collector(
    position: [f32; 4],
    distance: f32,
    collector: &mut super::cast_contacts::Collector,
    mut body_filter: impl FnMut(usize) -> anyhow::Result<u32>,
) -> anyhow::Result<f32> {
    collector.sort()?;
    for contact in collector.contacts()? {
        let filter = body_filter(contact.collidable)?;
        let hit = Hit {
            position: contact.position.map(f32::from_bits),
            normal: contact.normal.map(f32::from_bits),
            body_filter: filter,
        };
        if let Some(height) = eligible_height(position, &hit) {
            return Ok(height);
        }
    }
    Ok(distance)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorted_contacts_query_body_filters_only_until_first_eligible_hit() -> anyhow::Result<()> {
        use super::super::cast_contacts::{Collector, RawContact};
        let mut collector = Collector::default();
        collector.begin_query(999);
        for i in 0..4 {
            collector.push(RawContact {
                position: [0., 1. + i as f32 * 0.1, 0., 0.].map(f32::to_bits),
                normal: [0., if i == 3 { 0.8 } else { 1. }, 0., 0.5].map(f32::to_bits),
                collidables: if i % 2 == 0 {
                    [999, 100 + i]
                } else {
                    [100 + i, 999]
                },
                shape_keys: [0, u32::MAX],
            })?;
        }
        let mut calls = vec![];
        let height = select_collector([0., 2., 0., 0.], 4., &mut collector, |id| {
            calls.push(id);
            assert_ne!(id, 101, "Read a body after selecting a hit");
            Ok(if id == 102 { 3 } else { 4 })
        })?;
        assert_eq!(calls, vec![102, 103, 100]);
        assert_eq!(height, 1.);
        assert_eq!(collector.raw().len(), 4);
        assert!(
            select_collector([0., 2., 0., 0.], 4., &mut collector, |_| anyhow::bail!(
                "Body record missing"
            ))
            .is_err()
        );
        collector.begin_query(999);
        assert_eq!(
            select_collector([0.; 4], 4., &mut collector, |_| panic!(
                "Empty collector read a body"
            ))?,
            4.
        );
        Ok(())
    }
    #[test]
    fn query_end_precedes_margin_and_padding_uses_paired_add() {
        let mut p = [-0., 2., 3., -0.];
        let segment = prepare(&mut p, 4.);
        assert_eq!(segment.end, [0., -2., 3., 0.]);
        assert_eq!(p[1].to_bits(), (2f32 + MARGIN).to_bits());
        assert_eq!(p[0].to_bits(), 0);
        assert_eq!(p[3].to_bits(), 0);
        assert_eq!(segment.start, p);
        assert_eq!(segment.margin.to_bits(), 0x3c23d70a);
        assert_eq!(select(p, 4., &[]), 4.);
    }
    #[test]
    fn selection_retains_native_category_threshold_order_and_clamp() {
        let hit = |y, normal, filter| Hit {
            position: [0., y, 0., 0.],
            normal,
            body_filter: filter,
        };
        let next = f32::from_bits(NORMAL_THRESHOLD.to_bits() + 1);
        let hits = [
            hit(1.9, [0., 1., 0., 0.], 0x18003),
            hit(1.8, [0., NORMAL_THRESHOLD, 0., 0.], 0x18004),
            hit(1.5, [0., next, 0., f32::NAN], 0x18004),
            hit(1.7, [0., 1., 0., 0.], 0x18004),
        ];
        assert_eq!(select([0., 2., 0., 0.], 4., &hits), 0.5);
        assert_eq!(
            select([0., 2., 0., 0.], 4., &[hit(3., [0., 1., 0., 0.], 4)]),
            0.
        );
        assert_eq!(
            select([0., 2., 0., 0.], 4., &[hit(1., [f32::NAN, 1., 0., 0.], 4)]),
            4.
        );
        assert!(select([0., 2., 0., 0.], 4., &[hit(f32::NAN, [0., 1., 0., 0.], 4)]).is_nan());
    }
}
