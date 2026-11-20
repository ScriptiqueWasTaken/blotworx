//! Concrete direct-child dirty-byte publication into caller-owned records.
//! Native handle resolution123a38 remains explicit; names aren't handles.
use super::{transform_child_list::Children, transform_children};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub trait Resolver {
    fn resolve(&mut self, handle: u32, children: &mut Children) -> Result<Option<usize>>;
}
pub struct Mark<'a, R: Resolver> {
    pub children: &'a mut Children,
    pub resolver: &'a mut R,
    pub components: &'a BTreeMap<usize, usize>, //resolved entity -> Transform token
    pub records: &'a mut BTreeMap<usize, Vec<u8>>,
}
impl<R: Resolver> transform_children::Backend for Mark<'_, R> {
    fn child_count(&mut self) -> Result<u16> {
        Ok(self.children.count())
    }
    fn child_handle(&mut self, i: usize) -> Result<u32> {
        self.children.handle(i)
    }
    fn resolve_entity(&mut self, h: u32) -> Result<Option<usize>> {
        self.resolver.resolve(h, self.children)
    }
    fn transform(&mut self, e: usize) -> Result<usize> {
        self.components
            .get(&e)
            .copied()
            .with_context(|| format!("Resolved child entity {e} lacks required Transform"))
    }
    fn mark_dirty_124(&mut self, t: usize, v: u8) -> Result<()> {
        let record = self
            .records
            .get_mut(&t)
            .with_context(|| format!("Missing owned child Transform record {t}"))?;
        ensure!(
            record.len() > 0x124,
            "Truncated child Transform dirty storage {t}"
        );
        record[0x124] = v;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Handles {
        calls: Vec<u32>,
        mutate: bool,
    }
    impl Resolver for Handles {
        fn resolve(&mut self, h: u32, children: &mut Children) -> Result<Option<usize>> {
            self.calls.push(h);
            if self.mutate && h == 1 {
                children.remove(2);
                children.register(3, |_, _| anyhow::bail!("Unexpected lookup"))?;
                children.register(4, |_, _| anyhow::bail!("Unexpected lookup"))?;
            }
            Ok(match h {
                1 => Some(10),
                2 => Some(20),
                _ => None,
            })
        }
    }
    #[test]
    fn child_marking_mutates_owned_records_skips_stale_and_retains_partial_failure() -> Result<()> {
        let mut children = Children::default();
        for h in [0, 7, 1, 2] {
            children.register(h, |_, _| anyhow::bail!("Unexpected large-list lookup"))?;
        }
        let mut resolver = Handles {
            calls: vec![],
            mutate: false,
        };
        let components = BTreeMap::from([(10, 100), (20, 200)]);
        let mut records = BTreeMap::from([(100, vec![0xa5; 0x140]), (200, vec![0x5a; 0x140])]);
        let first_address = records[&100].as_ptr();
        transform_children::mark(&mut Mark {
            children: &mut children,
            resolver: &mut resolver,
            components: &components,
            records: &mut records,
        })?;
        assert_eq!(resolver.calls, [7, 1, 2]);
        assert_eq!(records[&100].as_ptr(), first_address);
        assert_eq!(records[&100][0x124], 1);
        assert_eq!(records[&200][0x124], 1);
        assert!(records[&100][..0x124].iter().all(|b| *b == 0xa5));
        assert!(records[&100][0x125..].iter().all(|b| *b == 0xa5));
        records.get_mut(&100).unwrap()[0x124] = 0;
        records.remove(&200);
        resolver.calls.clear();
        assert!(transform_children::mark(&mut Mark {
            children: &mut children,
            resolver: &mut resolver,
            components: &components,
            records: &mut records
        })
        .is_err());
        assert_eq!(records[&100][0x124], 1);
        assert_eq!(records.len(), 1);
        assert_eq!(resolver.calls, [7, 1, 2]);
        resolver.calls.clear();
        resolver.mutate = true;
        transform_children::mark(&mut Mark {
            children: &mut children,
            resolver: &mut resolver,
            components: &components,
            records: &mut records,
        })?;
        assert_eq!(resolver.calls, [7, 1, 3]);
        assert_eq!(children.count(), 5); // Appended4 outside captured count.
        Ok(())
    }
}
