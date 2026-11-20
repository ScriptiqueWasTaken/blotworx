//! AABB phantom overlap bookkeeping, DOL802bccf4 / 802bcde4.
//! Actual collidables and registered callbacks must come from the world backend.
use anyhow::{ensure, Result};

#[derive(Default, Debug)]
pub struct Overlaps {
    pub collidables: Vec<usize>,
    pub listeners: Vec<Option<usize>>,
    pub order_dirty: bool,
}
/// Native event+0 is the owning phantom, not the listener. Standalone array
/// probes have no allocation binding; actual Objects always supplies Some(id).
pub struct Added {
    pub phantom: Option<usize>,
    pub collidable: usize,
    pub result: u32,
}
pub struct Removed {
    pub phantom: Option<usize>,
    pub collidable: usize,
    pub was_present: bool,
}
pub trait Callbacks {
    fn added(&mut self, listener: usize, phantom: &mut Overlaps, event: &mut Added) -> Result<()>;
    fn removed(
        &mut self,
        listener: usize,
        phantom: &mut Overlaps,
        event: &mut Removed,
    ) -> Result<()>;
}
impl Overlaps {
    /// DOL802be418 appends without deduplication, including null slots.
    pub fn register(&mut self, listener: Option<usize>) {
        self.listeners.push(listener);
    }
    /// DOL802be490 removes the first match with ordered compaction. A missing
    /// match would corrupt native storage; diagnose it instead of ignoring it.
    pub fn unregister(&mut self, listener: Option<usize>) -> Result<()> {
        let index = self
            .listeners
            .iter()
            .position(|&slot| slot == listener)
            .ok_or_else(|| anyhow::anyhow!("Removing an unregistered phantom overlap listener"))?;
        self.listeners.remove(index);
        Ok(())
    }
    pub fn contains(&self, collidable: usize) -> bool {
        self.collidables.contains(&collidable)
    }
    pub fn add(&mut self, collidable: usize, callbacks: &mut impl Callbacks) -> Result<bool> {
        self.add_event(None, collidable, callbacks)
    }
    pub fn add_from(
        &mut self,
        phantom: usize,
        collidable: usize,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        ensure!(phantom != 0, "Null phantom in overlap entry");
        self.add_event(Some(phantom), collidable, callbacks)
    }
    /// hkpSimpleShapePhantom, DOL802bfc7c: shapeless visitors are ignored
    /// before listener callbacks. AABB phantoms do not have this gate.
    pub fn simple_add_from(
        &mut self,
        phantom: usize,
        collidable: usize,
        has_shape: bool,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        ensure!(phantom != 0, "Null simple phantom in overlap entry");
        if !has_shape {
            return Ok(false);
        }
        self.add_event(Some(phantom), collidable, callbacks)
    }
    /// DOL802bfd74 searches backwards; duplicate removal differs from AABB.
    pub fn simple_remove_from(
        &mut self,
        phantom: usize,
        collidable: usize,
        has_shape: bool,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        ensure!(phantom != 0, "Null simple phantom in overlap exit");
        if !has_shape {
            return Ok(false);
        }
        let index = self.collidables.iter().rposition(|&c| c == collidable);
        self.remove_index(Some(phantom), collidable, index, callbacks)
    }
    fn add_event(
        &mut self,
        phantom: Option<usize>,
        collidable: usize,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        let mut event = Added {
            phantom,
            collidable,
            result: 0,
        };
        // Capture count, but reread each slot after callbacks. Do not snapshot
        // listeners or return early when a listener rejects the shared event.
        for index in (0..self.listeners.len()).rev() {
            let listener = *self.listeners.get(index).ok_or_else(|| {
                anyhow::anyhow!("Phantom listener mutation invalidated native iteration")
            })?;
            if let Some(listener) = listener {
                callbacks.added(listener, self, &mut event)?;
            }
        }
        if event.result != 0 {
            return Ok(false);
        }
        self.collidables.push(collidable);
        self.order_dirty = true;
        Ok(true)
    }
    pub fn remove(&mut self, collidable: usize, callbacks: &mut impl Callbacks) -> Result<bool> {
        self.remove_event(None, collidable, callbacks)
    }
    pub fn remove_from(
        &mut self,
        phantom: usize,
        collidable: usize,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        ensure!(phantom != 0, "Null phantom in overlap exit");
        self.remove_event(Some(phantom), collidable, callbacks)
    }
    fn remove_event(
        &mut self,
        phantom: Option<usize>,
        collidable: usize,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        let index = self.collidables.iter().position(|&c| c == collidable);
        self.remove_index(phantom, collidable, index, callbacks)
    }
    fn remove_index(
        &mut self,
        phantom: Option<usize>,
        collidable: usize,
        index: Option<usize>,
        callbacks: &mut impl Callbacks,
    ) -> Result<bool> {
        let mut event = Removed {
            phantom,
            collidable,
            was_present: index.is_some(),
        };
        for slot in (0..self.listeners.len()).rev() {
            let listener = *self.listeners.get(slot).ok_or_else(|| {
                anyhow::anyhow!("Phantom listener mutation invalidated native iteration")
            })?;
            if let Some(listener) = listener {
                callbacks.removed(listener, self, &mut event)?;
            }
        }
        if let Some(index) = index {
            // Original uses the saved index after callbacks and swaps the last
            // entry into it; no stable compaction or membership recheck.
            ensure!(
                index < self.collidables.len(),
                "Phantom overlap mutation invalidated native removal index"
            );
            self.collidables.swap_remove(index);
            self.order_dirty = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Receiver {
        calls: Vec<(usize, usize, bool)>,
        reject: bool,
    }
    impl Callbacks for Receiver {
        fn added(&mut self, l: usize, p: &mut Overlaps, e: &mut Added) -> Result<()> {
            assert!(!p.contains(e.collidable));
            self.calls.push((l, e.collidable, true));
            if self.reject {
                e.result = 1;
            }
            Ok(())
        }
        fn removed(&mut self, l: usize, _: &mut Overlaps, e: &mut Removed) -> Result<()> {
            self.calls.push((l, e.collidable, e.was_present));
            Ok(())
        }
    }
    #[test]
    fn registration_preserves_duplicates_and_ordered_teardown() -> Result<()> {
        let mut p = Overlaps::default();
        for l in [Some(1), Some(2), Some(1), None, Some(3)] {
            p.register(l);
        }
        p.unregister(Some(1))?;
        assert_eq!(p.listeners, vec![Some(2), Some(1), None, Some(3)]);
        p.unregister(None)?;
        let before = p.listeners.clone();
        assert!(p.unregister(Some(9)).is_err());
        assert_eq!(p.listeners, before);
        let mut r = Receiver::default();
        p.add(42, &mut r)?;
        assert_eq!(r.calls, vec![(3, 42, true), (1, 42, true), (2, 42, true)]);
        Ok(())
    }
    #[test]
    fn reverse_callbacks_rejection_and_absent_removal() -> Result<()> {
        let mut p = Overlaps {
            listeners: vec![Some(1), None, Some(3)],
            ..Default::default()
        };
        let mut r = Receiver {
            reject: true,
            ..Default::default()
        };
        assert!(!p.add(20, &mut r)?);
        assert!(p.collidables.is_empty());
        assert!(!p.order_dirty);
        assert!(!p.remove(20, &mut r)?);
        assert_eq!(
            r.calls,
            vec![(3, 20, true), (1, 20, true), (3, 20, false), (1, 20, false)]
        );
        Ok(())
    }
    #[test]
    fn accepted_removal_swaps_last_and_preserves_duplicates() -> Result<()> {
        let mut p = Overlaps::default();
        let mut r = Receiver::default();
        for c in [10, 20, 30, 20] {
            assert!(p.add(c, &mut r)?);
        }
        assert_eq!(p.collidables, vec![10, 20, 30, 20]);
        p.order_dirty = false;
        assert!(p.remove(10, &mut r)?);
        assert_eq!(p.collidables, vec![20, 20, 30]);
        assert!(p.order_dirty);
        assert!(p.remove(20, &mut r)?);
        assert_eq!(p.collidables, vec![30, 20]);
        Ok(())
    }
    #[test]
    fn simple_shape_gate_and_reverse_duplicate_removal_match_native() -> Result<()> {
        let mut p = Overlaps {
            listeners: vec![Some(1)],
            ..Default::default()
        };
        let mut r = Receiver::default();
        assert!(!p.simple_add_from(8, 20, false, &mut r)?);
        assert!(!p.simple_remove_from(8, 20, false, &mut r)?);
        assert!(r.calls.is_empty());
        assert!(!p.order_dirty);
        p.collidables = vec![20, 10, 20, 30];
        assert!(p.simple_remove_from(8, 20, true, &mut r)?);
        assert_eq!(p.collidables, vec![20, 10, 30]);
        assert_eq!(r.calls, vec![(1, 20, true)]);
        p.order_dirty = false;
        assert!(!p.simple_remove_from(8, 99, true, &mut r)?);
        assert_eq!(r.calls.last(), Some(&(1, 99, false)));
        assert!(!p.order_dirty);
        assert!(p.simple_add_from(8, 99, true, &mut r)?);
        assert_eq!(p.collidables, vec![20, 10, 30, 99]);
        Ok(())
    }
}
