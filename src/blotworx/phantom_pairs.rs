//! Original broadphase pair cancellation, DOL80263ea4..8026450c.
//! Pair generation, filtering and actual overlap listeners are separate.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair(pub [usize; 2]);
impl Pair {
    fn key(self) -> [usize; 2] {
        if self.0[0] <= self.0[1] {
            self.0
        } else {
            [self.0[1], self.0[0]]
        }
    }
}
/// Broadphase handle bytes +4/+5. The offset is signed in the original.
#[derive(Clone, Copy, Debug)]
pub struct Handle {
    pub kind: i8,
    pub collidable_offset: i8,
}
pub trait Dispatcher {
    fn handle(&self, address: usize) -> Result<Handle>;
    /// Preserve the PPC return register: only its high byte admits a pair.
    fn filter(&mut self, collidables: [usize; 2]) -> Result<u32>;
    fn removed(&mut self, types: [usize; 2], pair: Pair) -> Result<()>;
    fn added(&mut self, types: [usize; 2], pair: Pair) -> Result<()>;
}
pub trait PhantomListener {
    fn handle(&self, address: usize) -> Result<Handle>;
    /// Resolve the phantom owner from the collidable's signed byte +0x10,
    /// then invoke its virtual +0x24 (enter) or +0x2c (leave).
    fn overlap(
        &mut self,
        phantom_collidable: usize,
        other_collidable: usize,
        enter: bool,
    ) -> Result<()>;
}
/// Registered phantom agent, DOL802beaac / 802beb5c. Phantom-phantom
/// pairs notify both sides, rereading membership after the first callback.
pub fn phantom_pair(pair: Pair, enter: bool, listener: &mut impl PhantomListener) -> Result<()> {
    for side in 0..2 {
        let own = listener.handle(pair.0[side])?;
        if own.kind != 2 {
            continue;
        }
        let other = listener.handle(pair.0[1 - side])?;
        let resolve = |address: usize, offset: i8| {
            address
                .checked_add_signed(offset as isize)
                .ok_or_else(|| anyhow::anyhow!("Native phantom collidable offset overflows"))
        };
        listener.overlap(
            resolve(pair.0[side], own.collidable_offset)?,
            resolve(pair.0[1 - side], other.collidable_offset)?,
            enter,
        )?;
    }
    Ok(())
}
fn types(handles: [Handle; 2]) -> Result<[usize; 2]> {
    ensure!(
        handles.iter().all(|h| (0..8).contains(&h.kind)),
        "Broadphase handle type is outside the native 8 by 8 dispatch table"
    );
    Ok([handles[0].kind as usize, handles[1].kind as usize])
}
fn handles(dispatcher: &impl Dispatcher, pair: Pair) -> Result<[Handle; 2]> {
    Ok([dispatcher.handle(pair.0[0])?, dispatcher.handle(pair.0[1])?])
}
/// DOL802bdc38: cancel, then deliver removals before filtered additions.
/// No geometric overlap or default listener is invented here: the world backend
/// must supply its registered type-pair agents and actual handle membership.
pub fn dispatch(
    added: &mut Vec<Pair>,
    removed: &mut Vec<Pair>,
    dispatcher: &mut impl Dispatcher,
) -> Result<()> {
    cancel(added, removed)?;
    for pair in removed.iter().copied() {
        let types = types(handles(dispatcher, pair)?)?;
        dispatcher.removed(types, pair)?;
    }
    for pair in added.iter().copied() {
        let metadata = handles(dispatcher, pair)?;
        let mut collidables = [0; 2];
        for i in 0..2 {
            collidables[i] = pair.0[i]
                .checked_add_signed(metadata[i].collidable_offset as isize)
                .ok_or_else(|| {
                    anyhow::anyhow!("Native collidable offset overflows its handle address")
                })?;
        }
        if dispatcher.filter(collidables)? >> 24 != 0 {
            // Native reads types after the filter callback, not before it.
            let types = types(handles(dispatcher, pair)?)?;
            dispatcher.added(types, pair)?;
        }
    }
    Ok(())
}
pub fn cancel(added: &mut Vec<Pair>, removed: &mut Vec<Pair>) -> Result<()> {
    ensure!(
        added
            .iter()
            .chain(removed.iter())
            .all(|p| p.0[0] != 0 && p.0[1] != 0),
        "Null native broadphase handle"
    );
    if added.len().min(removed.len()) < 32 {
        let mut kept = Vec::with_capacity(removed.len());
        for pair in removed.iter().copied() {
            if let Some(index) = added.iter().position(|p| p.key() == pair.key()) {
                added.remove(index);
            } else {
                kept.push(pair);
            }
        }
        *removed = kept;
        return Ok(());
    }
    // Original value packs first index in bits >=8 and count in low byte.
    // Preserve its addition/decoding rather than introducing a count cap.
    let mut values = BTreeMap::<[usize; 2], u64>::new();
    for (index, pair) in added.iter_mut().enumerate() {
        match values.entry(pair.key()) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = entry.get().wrapping_add(1);
                pair.0[0] = 0;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(((index as u64) << 8) | 1);
            }
        }
    }
    let mut kept = Vec::with_capacity(removed.len());
    for pair in removed.iter().copied() {
        let key = pair.key();
        let Some(value) = values.get_mut(&key) else {
            kept.push(pair);
            continue;
        };
        if (*value as u8) > 1 {
            *value = value.wrapping_sub(1);
        } else {
            let index = (*value as u32 >> 8) as usize;
            ensure!(
                index < added.len(),
                "Native broadphase cancellation index is outside its array"
            );
            added[index].0[0] = 0;
            values.remove(&key);
        }
    }
    *removed = kept;
    added.retain(|pair| pair.0[0] != 0);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phantom_agent_notifies_both_sides_and_rereads_after_callbacks() -> Result<()> {
        struct Listener {
            calls: Vec<(usize, usize, bool)>,
            retype: bool,
        }
        impl PhantomListener for Listener {
            fn handle(&self, address: usize) -> Result<Handle> {
                Ok(Handle {
                    kind: if self.retype && !self.calls.is_empty() && address == 200 {
                        1
                    } else {
                        2
                    },
                    collidable_offset: -8,
                })
            }
            fn overlap(&mut self, a: usize, b: usize, enter: bool) -> Result<()> {
                self.calls.push((a, b, enter));
                Ok(())
            }
        }
        let mut both = Listener {
            calls: vec![],
            retype: false,
        };
        phantom_pair(Pair([100, 200]), true, &mut both)?;
        assert_eq!(both.calls, vec![(92, 192, true), (192, 92, true)]);
        let mut changed = Listener {
            calls: vec![],
            retype: true,
        };
        phantom_pair(Pair([100, 200]), false, &mut changed)?;
        assert_eq!(changed.calls, vec![(92, 192, false)]);
        Ok(())
    }
    #[test]
    fn dispatch_preserves_orientation_signed_offsets_and_native_filter_byte() -> Result<()> {
        struct Backend {
            calls: Vec<String>,
            filters: usize,
        }
        impl Dispatcher for Backend {
            fn handle(&self, address: usize) -> Result<Handle> {
                Ok(Handle {
                    kind: if address == 100 { 2 } else { 1 },
                    collidable_offset: -8,
                })
            }
            fn filter(&mut self, c: [usize; 2]) -> Result<u32> {
                self.calls.push(format!("filter {c:?}"));
                self.filters += 1;
                Ok(if self.filters == 1 { 1 } else { 0x80000000 })
            }
            fn removed(&mut self, t: [usize; 2], p: Pair) -> Result<()> {
                self.calls.push(format!("remove {t:?} {:?}", p.0));
                Ok(())
            }
            fn added(&mut self, t: [usize; 2], p: Pair) -> Result<()> {
                self.calls.push(format!("add {t:?} {:?}", p.0));
                Ok(())
            }
        }
        let mut backend = Backend {
            calls: vec![],
            filters: 0,
        };
        dispatch(
            &mut vec![Pair([100, 200]), Pair([200, 100])],
            &mut vec![Pair([300, 400])],
            &mut backend,
        )?;
        assert_eq!(
            backend.calls,
            vec![
                "remove [1, 1] [300, 400]",
                "filter [92, 192]",
                "filter [192, 92]",
                "add [1, 2] [200, 100]"
            ]
        );
        Ok(())
    }
    #[test]
    fn small_path_cancels_reversed_pairs_and_preserves_order() -> Result<()> {
        let mut added = vec![Pair([1, 2]), Pair([3, 4]), Pair([2, 1]), Pair([5, 6])];
        let mut removed = vec![Pair([2, 1]), Pair([9, 10]), Pair([1, 2])];
        cancel(&mut added, &mut removed)?;
        assert_eq!(added, vec![Pair([3, 4]), Pair([5, 6])]);
        assert_eq!(removed, vec![Pair([9, 10])]);
        Ok(())
    }
    #[test]
    fn native_threshold_changes_duplicate_compaction_without_sorting_output() -> Result<()> {
        for count in [31, 32, 64] {
            let mut added = vec![Pair([2, 1]); count];
            let mut removed = (0..count)
                .map(|i| Pair([100 + i, 200 + i]))
                .collect::<Vec<_>>();
            let expected = removed.clone();
            cancel(&mut added, &mut removed)?;
            assert_eq!(added.len(), if count < 32 { count } else { 1 });
            assert_eq!(added[0], Pair([2, 1]));
            assert_eq!(removed, expected);
        }
        let mut added = vec![Pair([1, 2]); 32];
        let mut removed = vec![Pair([2, 1]); 32];
        cancel(&mut added, &mut removed)?;
        assert!(added.is_empty() && removed.is_empty());
        Ok(())
    }
}
