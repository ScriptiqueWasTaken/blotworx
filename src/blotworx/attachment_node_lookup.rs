//! Owned node fallback guards REL355008..35502c. Does not synthesize a pose
//! when a guard fails: the caller's existing secondary-pose workspace survives.
use super::{
    actor_bone_query::Pose, native_transform_record::Transform, transform_storage::Storage,
};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
pub struct Owned<'a, 's> {
    pub storage: &'a Storage<'s>,
    pub cache_records: &'a BTreeMap<usize, Vec<u8>>,
    pub nodes: &'a BTreeMap<u32, Vec<u8>>,
}
fn word(record: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        record
            .get(offset..offset + 4)
            .context("Truncated attachment node record")?
            .try_into()
            .unwrap(),
    ))
}
impl Owned<'_, '_> {
    pub fn secondary_pose(
        &self,
        has_nonactor_component: bool,
        parent: Transform,
    ) -> Result<Option<Pose>> {
        if !has_nonactor_component {
            return Ok(None);
        }
        let cache = self
            .cache_records
            .get(&self.storage.cache_record())
            .context("Missing attachment cache record")?;
        let node = word(cache, 0x34)?;
        if node == 0 {
            return Ok(None);
        }
        let node = self.nodes.get(&node).context("Missing attached NiNode")?;
        let node_parent = word(node, 0x1c)?;
        if node_parent == 0 {
            return Ok(None);
        }
        let record = self
            .nodes
            .get(&node_parent)
            .context("Missing attached NiNode parent")?;
        let mut transform = [0.; 13];
        for (i, v) in transform.iter_mut().enumerate() {
            *v = f32::from_bits(word(record, 0x64 + i * 4)?);
        }
        super::attachment_direction::node_pose(parent, transform).map(Some)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_node_lookup_reads_owned_parent_world_and_preserves_guards() -> Result<()> {
        let parent = [1., 0., 0., 0., 1., 0., 0., 0., 1., 10., 20., 30., 2.];
        let node_parent = [1f32, 0., 0., 0., 1., 0., 0., 0., 1., 12., 24., 36., 6.];
        let mut bytes = [0; 0x140];
        bytes[0x11c..0x120].copy_from_slice(&100u32.to_be_bytes());
        let storage = Storage::new(&mut bytes)?;
        let mut cache = vec![0; 0x3c];
        cache[0x34..0x38].copy_from_slice(&200u32.to_be_bytes());
        let mut node = vec![0; 0x20];
        node[0x1c..0x20].copy_from_slice(&300u32.to_be_bytes());
        let mut owner = vec![0xa5; 0x98];
        for (i, v) in node_parent.into_iter().enumerate() {
            owner[0x64 + i * 4..0x68 + i * 4].copy_from_slice(&v.to_be_bytes());
        }
        let records = BTreeMap::from([(100, cache)]);
        let mut nodes = BTreeMap::from([(200, node), (300, owner)]);
        let before = nodes.clone();
        let source = Owned {
            storage: &storage,
            cache_records: &records,
            nodes: &nodes,
        };
        let pose = source.secondary_pose(true, parent)?.unwrap();
        assert_eq!(&pose[..4], &[1., 2., 3., 0.]);
        assert_eq!(&pose[8..], &[3., 3., 3., 0.]);
        assert_eq!(source.secondary_pose(false, parent)?, None);
        assert_eq!(nodes, before);
        nodes.get_mut(&200).unwrap()[0x1c..0x20].fill(0);
        assert_eq!(
            Owned {
                storage: &storage,
                cache_records: &records,
                nodes: &nodes
            }
            .secondary_pose(true, parent)?,
            None
        );
        let absent_records = BTreeMap::new();
        let absent_nodes = BTreeMap::new();
        assert_eq!(
            Owned {
                storage: &storage,
                cache_records: &absent_records,
                nodes: &absent_nodes
            }
            .secondary_pose(false, parent)?,
            None
        );
        assert!(Owned {
            storage: &storage,
            cache_records: &absent_records,
            nodes: &absent_nodes
        }
        .secondary_pose(true, parent)
        .is_err());
        Ok(())
    }
}
