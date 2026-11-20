//! Hierarchy depth publication REL353b6c..353bcc. Notification145054
//! reorders component scheduling; it is mandatory when the depth changes.
use anyhow::Result;
pub trait Backend {
    fn parent(&mut self) -> Result<Option<usize>>;
    fn depth_120(&mut self) -> Result<u32>;
    // REL1f0094 queries Transform54aec; absence returns zero.
    fn parent_depth(&mut self, parent: usize) -> Result<u32>;
    fn store_depth_120(&mut self, value: u32) -> Result<()>;
    fn own_entity(&mut self) -> Result<usize>;
    fn reorder_145054(&mut self, entity: usize, old: u32, new: u32, flag: u32) -> Result<()>;
}
pub fn update(backend: &mut impl Backend) -> Result<()> {
    let parent = backend.parent()?;
    let old = backend.depth_120()?;
    let new = match parent {
        Some(parent) => backend.parent_depth(parent)?.wrapping_add(1),
        None => 0,
    };
    backend.store_depth_120(new)?;
    let new = backend.depth_120()?;
    if old != new {
        let entity = backend.own_entity()?;
        backend.reorder_145054(entity, old, new, 1)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        parent: Option<usize>,
        depth: u32,
        parent_depth: u32,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn parent(&mut self) -> Result<Option<usize>> {
            self.calls.push("parent".into());
            Ok(self.parent)
        }
        fn depth_120(&mut self) -> Result<u32> {
            self.calls.push("depth".into());
            Ok(self.depth)
        }
        fn parent_depth(&mut self, p: usize) -> Result<u32> {
            assert_eq!(p, 7);
            self.calls.push("parent depth".into());
            Ok(self.parent_depth)
        }
        fn store_depth_120(&mut self, v: u32) -> Result<()> {
            self.calls.push(format!("store {v}"));
            self.depth = v;
            Ok(())
        }
        fn own_entity(&mut self) -> Result<usize> {
            self.calls.push("entity".into());
            Ok(9)
        }
        fn reorder_145054(&mut self, e: usize, old: u32, new: u32, f: u32) -> Result<()> {
            assert_eq!((e, f), (9, 1));
            self.calls.push(format!("reorder {old} {new}"));
            if self.fail {
                anyhow::bail!("Scheduler unavailable")
            };
            Ok(())
        }
    }
    #[test]
    fn native_depth_publication_order_wrap_and_partial_failure() -> Result<()> {
        let mut s = State {
            parent: Some(7),
            depth: 0,
            parent_depth: 4,
            calls: vec![],
            fail: false,
        };
        update(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "parent",
                "depth",
                "parent depth",
                "store 5",
                "depth",
                "entity",
                "reorder 0 5"
            ]
        );
        s.calls.clear();
        update(&mut s)?;
        assert_eq!(
            s.calls,
            ["parent", "depth", "parent depth", "store 5", "depth"]
        );
        s.calls.clear();
        s.parent_depth = u32::MAX;
        update(&mut s)?;
        assert_eq!(s.depth, 0);
        assert_eq!(s.calls.last().unwrap(), "reorder 5 0");
        s.calls.clear();
        s.parent = None;
        s.depth = 9;
        s.fail = true;
        assert!(update(&mut s).is_err());
        assert_eq!(s.depth, 0);
        assert_eq!(
            s.calls,
            [
                "parent",
                "depth",
                "store 0",
                "depth",
                "entity",
                "reorder 9 0"
            ]
        );
        Ok(())
    }
}
