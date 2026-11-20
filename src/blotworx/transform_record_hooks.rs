//! Actual owned child/cached-record mutation for transform_runtime::Hooks.
//! Record kind/lifetime reconstruction remains separate; no cache is invented.
use super::{
    transform_child_list::Children,
    transform_hierarchy::{self, Resolver},
    transform_storage::Storage,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub struct Hooks<'a, R: Resolver> {
    pub children: &'a mut Children,
    pub resolver: &'a mut R,
    pub components: &'a BTreeMap<usize, usize>,
    pub child_records: &'a mut BTreeMap<usize, Vec<u8>>,
    pub related_records: &'a mut BTreeMap<usize, Vec<u8>>,
}
impl<R: Resolver> super::transform_runtime::Hooks for Hooks<'_, R> {
    fn children(&mut self, _: &mut Storage<'_>) -> Result<()> {
        super::transform_children::mark(&mut transform_hierarchy::Mark {
            children: &mut *self.children,
            resolver: &mut *self.resolver,
            components: self.components,
            records: &mut *self.child_records,
        })
    }
    fn related(&mut self, storage: &mut Storage<'_>, offset: usize) -> Result<Option<usize>> {
        storage.related(offset)
    }
    fn mark(&mut self, _: &mut Storage<'_>, token: usize, offset: usize, value: u8) -> Result<()> {
        ensure!(
            matches!(offset, 0x56 | 0x3e),
            "Unsupported cached transform dirty byte {offset:x}"
        );
        let record = self
            .related_records
            .get_mut(&token)
            .with_context(|| format!("Missing related transform record {token}"))?;
        *record
            .get_mut(offset)
            .with_context(|| format!("Truncated related transform record {token}"))? = value;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::super::{
        entity_handle_lookup::Table,
        entity_handle_runtime::{Allocator, Direction, Registry},
        transform_runtime::{Constants, Runtime},
        transform_storage::Space,
    };
    use super::*;
    struct Nodes(usize);
    impl Allocator for Nodes {
        fn node(&mut self, _: Direction, _: &mut Table, _: &mut u32) -> Result<usize> {
            self.0 += 1;
            Ok(self.0)
        }
    }
    #[test]
    fn owned_registry_and_related_records_receive_actual_dirty_writes() -> Result<()> {
        let mut registry = Registry::initialized(8, Nodes(0))?;
        registry.register(70)?;
        let mut children = Children::default();
        children.register(1, |_, _| anyhow::bail!("Unexpected lookup"))?;
        let components = BTreeMap::from([(70, 300)]);
        let mut child_records = BTreeMap::from([(300, vec![0; 0x140])]);
        let mut related_records = BTreeMap::from([(400, vec![0; 0x80]), (500, vec![0; 0x80])]);
        let mut bytes = [0; 0x140];
        bytes[0x134..0x138].copy_from_slice(&400u32.to_be_bytes());
        bytes[0x138..0x13c].copy_from_slice(&500u32.to_be_bytes());
        let mut storage = Storage::new(&mut bytes)?;
        storage.store_cached_parent([1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1.]);
        let id = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut runtime = Runtime {
            storage,
            hooks: Hooks {
                children: &mut children,
                resolver: &mut registry,
                components: &components,
                child_records: &mut child_records,
                related_records: &mut related_records,
            },
            constants: Constants {
                zero: 0.,
                one: 1.,
                identity: id,
            },
        };
        assert!(super::super::transform_translation::set(
            [1., 2., 3.],
            &mut runtime
        )?);
        assert_eq!(runtime.storage.position(Space::Local), [1., 2., 3.]);
        assert_eq!(runtime.hooks.child_records[&300][0x124], 1);
        assert_eq!(runtime.hooks.related_records[&400][0x56], 1);
        assert_eq!(runtime.hooks.related_records[&500][0x3e], 1);
        runtime.hooks.child_records.get_mut(&300).unwrap()[0x124] = 0;
        assert!(!super::super::transform_translation::set(
            [1., 2., 3.],
            &mut runtime
        )?);
        assert_eq!(runtime.hooks.child_records[&300][0x124], 0);
        runtime.hooks.related_records.remove(&500);
        assert!(super::super::transform_translation::set([4., 5., 6.], &mut runtime).is_err());
        assert_eq!(runtime.storage.position(Space::World), [4., 5., 6.]);
        assert_eq!(runtime.storage.position(Space::Local), [1., 2., 3.]);
        assert_eq!(runtime.hooks.child_records[&300][0x124], 1);
        Ok(())
    }
}
