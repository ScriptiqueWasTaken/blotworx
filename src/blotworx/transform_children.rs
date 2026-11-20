//! Direct-child invalidation, REL351f80..35200c. This is not the recursive
//! transform/physics update at REL3541d0.
use anyhow::Result;

pub trait Backend {
    fn child_count(&mut self) -> Result<u16>;
    // The native array pointer is reread on each iteration, not snapshotted.
    fn child_handle(&mut self, index: usize) -> Result<u32>;
    fn resolve_entity(&mut self, handle: u32) -> Result<Option<usize>>;
    // REL54acc returns the transformation component. A missing component must
    // diagnose instead of silently dropping the required write.
    fn transform(&mut self, entity: usize) -> Result<usize>;
    fn mark_dirty_124(&mut self, transform: usize, value: u8) -> Result<()>;
}

pub fn mark(backend: &mut impl Backend) -> Result<()> {
    let count = backend.child_count()?;
    for index in 0..usize::from(count) {
        let handle = backend.child_handle(index)?;
        if handle == 0 {
            continue;
        }
        if let Some(entity) = backend.resolve_entity(handle)? {
            let transform = backend.transform(entity)?;
            backend.mark_dirty_124(transform, 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        handles: Vec<u32>,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn child_count(&mut self) -> Result<u16> {
            self.calls.push("count".into());
            Ok(self.handles.len() as u16)
        }
        fn child_handle(&mut self, i: usize) -> Result<u32> {
            self.calls.push(format!("handle {i}"));
            Ok(self.handles[i])
        }
        fn resolve_entity(&mut self, h: u32) -> Result<Option<usize>> {
            self.calls.push(format!("resolve {h}"));
            Ok((h != 9).then_some(h as usize))
        }
        fn transform(&mut self, e: usize) -> Result<usize> {
            self.calls.push(format!("transform {e}"));
            if self.fail && e == 2 {
                anyhow::bail!("Missing transform")
            };
            Ok(e + 100)
        }
        fn mark_dirty_124(&mut self, t: usize, v: u8) -> Result<()> {
            assert_eq!(v, 1);
            self.calls.push(format!("mark {t}"));
            if t == 101 {
                self.handles[3] = 2;
                self.handles.push(3);
            }
            Ok(())
        }
    }
    #[test]
    fn captured_count_fresh_handles_stale_entries_and_partial_failure() -> Result<()> {
        let mut s = State {
            handles: vec![0, 9, 1, 1],
            calls: vec![],
            fail: false,
        };
        mark(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "count",
                "handle 0",
                "handle 1",
                "resolve 9",
                "handle 2",
                "resolve 1",
                "transform 1",
                "mark 101",
                "handle 3",
                "resolve 2",
                "transform 2",
                "mark 102"
            ]
        );
        // Earlier marks persist; later entries aren't visited after failure.
        s.calls.clear();
        s.fail = true;
        assert!(mark(&mut s).is_err());
        assert!(s.calls.contains(&"mark 101".into()));
        assert_eq!(s.calls.last().unwrap(), "transform 2");
        assert!(!s.calls.contains(&"handle 4".into()));
        Ok(())
    }
}
