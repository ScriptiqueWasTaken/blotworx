//! Owned paired registry binding for native2b2400 publication. Host allocation
//! identities are explicit; full entity creation/destruction order is separate.
use super::{
    entity_handle_lookup::{self, Reverse, Table},
    entity_handle_registration,
};
use anyhow::Result;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}
pub trait Allocator {
    fn node(&mut self, direction: Direction, table: &mut Table, counter: &mut u32)
        -> Result<usize>;
}
pub struct Registry<A: Allocator> {
    pub forward: Table,
    pub reverse: Reverse,
    pub counter: u32,
    pub forward_pool_mode: u32,
    pub reverse_pool_mode: u32,
    pub allocator: A,
}
impl<A: Allocator> Registry<A> {
    pub fn initialized(selector: i32, allocator: A) -> Result<Self> {
        Ok(Self {
            forward: Table::initialized(selector)?,
            reverse: Reverse::initialized(selector)?,
            counter: 1,
            forward_pool_mode: 0,
            reverse_pool_mode: 0,
            allocator,
        })
    }
    pub fn register(&mut self, entity: u32) -> Result<()> {
        entity_handle_registration::register(entity, self)
    }
    pub fn entity(&mut self, handle: u32) -> Result<Option<usize>> {
        entity_handle_lookup::resolve(handle, &mut self.forward)
    }
    pub fn handle(&mut self, entity: u32) -> Result<u32> {
        self.reverse.handle(entity)
    }
    ///2b2234 removes only the handle stored in the current reverse node.
    /// Host bool reports found state; native routine has no claimed bool result.
    pub fn remove(&mut self, entity: u32) -> Result<bool> {
        let Some((index, previous, token)) = self.reverse.table.find_node(entity, |key| key)?
        else {
            return Ok(false);
        };
        let handle = u32::try_from(self.reverse.table.nodes[&token].entity.unwrap_or(0))
            .map_err(|_| anyhow::anyhow!("Reverse removal handle exceeds native32bit storage"))?;
        self.forward
            .remove_key(handle, entity_handle_lookup::hash_key)?;
        self.reverse.table.unlink_node(index, previous, token)?;
        Ok(true)
    }
}
impl<A: Allocator> entity_handle_registration::Backend for Registry<A> {
    fn insert_forward_2c1790(&mut self, entity: u32) -> Result<()> {
        // Native tests backing bucket capacity16, not current active count.
        if self.forward.buckets.len() <= 1 {
            self.forward.rehash(8, entity_handle_lookup::hash_key)?;
        }
        let allocator = &mut self.allocator;
        let counter = &mut self.counter;
        let token = self.forward.acquire(self.forward_pool_mode, |table| {
            allocator.node(Direction::Forward, table, counter)
        })?;
        let key = self.counter; // Caller holder read occurs after acquisition.
        self.forward.prepare(token, key, entity)?;
        self.forward
            .link_prepared(token, entity_handle_lookup::hash_key(key))
    }
    fn insert_reverse_2c1c14(&mut self, entity: u32) -> Result<()> {
        if self.reverse.table.buckets.len() <= 1 {
            self.reverse.table.rehash(8, |key| key)?;
        }
        let allocator = &mut self.allocator;
        let counter = &mut self.counter;
        let token = self
            .reverse
            .table
            .acquire(self.reverse_pool_mode, |table| {
                allocator.node(Direction::Reverse, table, counter)
            })?;
        self.reverse.table.prepare(token, entity, self.counter)?;
        self.reverse.table.link_prepared(token, entity)
    }
    fn counter_40(&mut self) -> Result<u32> {
        Ok(self.counter)
    }
    fn store_counter_40(&mut self, value: u32) -> Result<()> {
        self.counter = value;
        Ok(())
    }
}
impl<A: Allocator> super::transform_hierarchy::Resolver for Registry<A> {
    fn resolve(
        &mut self,
        handle: u32,
        _: &mut super::transform_child_list::Children,
    ) -> Result<Option<usize>> {
        self.entity(handle)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Nodes {
        next: usize,
        calls: Vec<Direction>,
        fail_reverse: bool,
    }
    impl Allocator for Nodes {
        fn node(&mut self, d: Direction, _: &mut Table, _: &mut u32) -> Result<usize> {
            self.calls.push(d);
            if self.fail_reverse && d == Direction::Reverse {
                anyhow::bail!("Reverse allocation unavailable")
            };
            self.next += 1;
            Ok(self.next)
        }
    }
    #[test]
    fn paired_owned_registration_lookup_pool_reuse_and_partial_failure() -> Result<()> {
        let mut registry = Registry::initialized(
            0,
            Nodes {
                next: 100,
                calls: vec![],
                fail_reverse: false,
            },
        )?;
        registry.register(70)?;
        registry.register(80)?;
        registry.register(70)?;
        assert_eq!(registry.counter, 4);
        assert_eq!(registry.forward.count, 257);
        assert_eq!(registry.reverse.table.count, 257);
        assert_eq!(registry.entity(1)?, Some(70));
        assert_eq!(registry.entity(3)?, Some(70));
        assert_eq!(registry.handle(70)?, 3);
        assert_eq!(registry.forward.entries, 3);
        assert_eq!(registry.reverse.table.entries, 2);
        let reverse_nodes = registry.reverse.table.nodes.len();
        let calls = registry.allocator.calls.len();
        registry.register(70)?;
        assert_eq!(registry.handle(70)?, 4);
        assert_eq!(registry.reverse.table.nodes.len(), reverse_nodes);
        assert_eq!(registry.allocator.calls.len(), calls + 1);
        let mut children = super::super::transform_child_list::Children::default();
        children.register(4, |_, _| anyhow::bail!("Unexpected lookup"))?;
        let components = std::collections::BTreeMap::from([(70, 700)]);
        let mut records = std::collections::BTreeMap::from([(700, vec![0; 0x140])]);
        super::super::transform_children::mark(&mut super::super::transform_hierarchy::Mark {
            children: &mut children,
            resolver: &mut registry,
            components: &components,
            records: &mut records,
        })?;
        assert_eq!(records[&700][0x124], 1);
        let mut failed = Registry::initialized(
            8,
            Nodes {
                next: 100,
                calls: vec![],
                fail_reverse: true,
            },
        )?;
        assert!(failed.register(90).is_err());
        assert_eq!(failed.counter, 1);
        assert_eq!(failed.entity(1)?, Some(90));
        assert_eq!(failed.handle(90)?, 0);
        assert_eq!(failed.forward.entries, 1);
        assert_eq!(failed.reverse.table.entries, 0);
        Ok(())
    }
    #[test]
    fn removal_retires_current_mapping_only_and_reuses_both_nodes() -> Result<()> {
        let mut registry = Registry::initialized(
            8,
            Nodes {
                next: 100,
                calls: vec![],
                fail_reverse: false,
            },
        )?;
        registry.register(70)?;
        registry.register(80)?;
        registry.register(70)?;
        let calls = registry.allocator.calls.len();
        let counter = registry.counter;
        assert!(registry.remove(70)?);
        assert_eq!(registry.handle(70)?, 0);
        assert_eq!(registry.entity(3)?, None);
        assert_eq!(registry.entity(1)?, Some(70));
        assert_eq!(registry.counter, counter);
        assert!(!registry.remove(70)?);
        registry.register(90)?;
        assert_eq!(registry.handle(90)?, 4);
        assert_eq!(registry.entity(4)?, Some(90));
        assert_eq!(registry.allocator.calls.len(), calls);
        assert_eq!(registry.forward.entries, 3);
        assert_eq!(registry.reverse.table.entries, 2);
        Ok(())
    }
}
