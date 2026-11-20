//! Publish an already normalized payload into the caller's shared graph arena.
//! This transfers buffer ownership; readiness/admission is not changed here.
use super::{
    ai_graph_link_passes::{Memory, View},
    ai_graph_normalization::{Layout, Payload},
};
use anyhow::{ensure, Context as _, Result};
impl Memory {
    pub fn bind_payload(
        &mut self,
        resource: u32,
        base: u32,
        payload: Payload,
        layout: Layout,
    ) -> Result<u32> {
        ensure!(
            !self.resources.contains_key(&resource),
            "Graph resource already has a bound payload"
        );
        let bytes = u32::try_from(payload.bytes.len())?;
        let end = base
            .checked_add(bytes)
            .context("Graph payload address overflow")?;
        for (&start, data) in &self.regions {
            let other_end = start
                .checked_add(u32::try_from(data.len())?)
                .context("Existing graph region address overflow")?;
            ensure!(
                end <= start || base >= other_end,
                "Graph payload overlaps an existing allocation"
            );
        }
        let (stride, edge_stride) = match layout {
            Layout::Fixed => (32, 16),
            Layout::Dynamic {
                node_stride,
                edge_stride,
            } => (node_stride, edge_stride),
        };
        let view = View {
            auxiliary: Some(match payload.field38 {
                super::ai_graph_normalization::Auxiliary::Scalar(v) => {
                    super::ai_graph_normalization::Auxiliary::Scalar(v)
                }
                super::ai_graph_normalization::Auxiliary::Tail(o) => {
                    super::ai_graph_normalization::Auxiliary::Tail(base.wrapping_add(o))
                }
            }),
            payload: Some(base),
            header: base,
            points: base.wrapping_add(payload.points),
            edges: Some(base.wrapping_add(payload.edges)),
            edge_stride: Some(edge_stride),
            external: payload.external.map_or(0, |o| base.wrapping_add(o)),
            external_count: payload.external_count,
            stride,
        };
        self.regions.insert(base, payload.bytes);
        self.resources.insert(resource, view);
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_binding_transfers_one_allocation_and_preserves_resource_identity() -> Result<()> {
        let original = [0u8; 8];
        let p = super::super::ai_graph_normalization::convert(
            &original,
            Layout::Dynamic {
                node_stride: 36,
                edge_stride: 12,
            },
        )?;
        let allocation = p.bytes.as_ptr();
        let mut m = Memory::default();
        let size = m.bind_payload(
            7,
            0x1000,
            p,
            Layout::Dynamic {
                node_stride: 36,
                edge_stride: 12,
            },
        )?;
        assert_eq!(size, 8);
        assert_eq!(m.regions[&0x1000].as_ptr(), allocation);
        let v = m.view(7)?;
        assert_eq!(v.payload, Some(0x1000));
        assert_eq!(v.header, 0x1000);
        assert_eq!(v.points, 0x1008);
        assert_eq!(v.edges, Some(0x1008));
        assert_eq!(v.edge_stride, Some(12));
        assert_eq!(m.regions.len(), 1);
        assert!(m.word(7 + 0x14).is_err());
        Ok(())
    }
    #[test]
    fn overlapping_or_duplicate_bindings_do_not_overwrite_live_storage() -> Result<()> {
        let mut m = Memory::default();
        m.regions.insert(0x1000, vec![0xa5; 8]);
        let p = super::super::ai_graph_normalization::convert(&[0; 8], Layout::Fixed)?;
        assert!(m.bind_payload(1, 0x1004, p, Layout::Fixed).is_err());
        assert_eq!(m.regions[&0x1000], vec![0xa5; 8]);
        assert!(m.resources.is_empty());
        let p = super::super::ai_graph_normalization::convert(&[0; 8], Layout::Fixed)?;
        m.bind_payload(1, 0x2000, p, Layout::Fixed)?;
        let p = super::super::ai_graph_normalization::convert(&[0; 8], Layout::Fixed)?;
        assert!(m.bind_payload(1, 0x3000, p, Layout::Fixed).is_err());
        assert!(!m.regions.contains_key(&0x3000));
        Ok(())
    }
}
