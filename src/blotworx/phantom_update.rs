//! Native AABB phantom storage/admission and deferred operation0x11.
//! Actual world, references, snapshots and broadphase publication are backend-owned.
use anyhow::Result;
pub type Aabb = [u32; 8];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundsSource {
    Stored(usize),
    Snapshot(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Update {
    pub phantom: usize,
    pub world: usize,
    pub snapshot: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Detached,
    Deferred,
    Published,
}
pub trait Backend {
    fn store(&mut self, phantom: usize, bounds: Aabb) -> Result<()>;
    fn current_world(&mut self, phantom: usize) -> Result<Option<usize>>;
    fn locks(&mut self, world: usize) -> Result<[u32; 2]>;
    /// Queue append/counter update precedes retain and snapshot allocation.
    fn append(&mut self, world: usize, phantom: usize) -> Result<usize>;
    fn retain(&mut self, phantom: usize) -> Result<()>;
    /// DOL802d2018 retains first, then allocates/copies32 bytes from the source.
    /// Stored sources must be read here, not captured before the retain callback.
    fn attach_snapshot(&mut self, entry: usize, source: BoundsSource) -> Result<()>;
    /// Full802bdadc unlocked broadphase update, including pair delivery and locks.
    fn immediate(&mut self, world: usize, phantom: usize, source: BoundsSource) -> Result<()>;
    fn release(&mut self, phantom: usize) -> Result<()>;
    fn free_snapshot(&mut self, snapshot: usize) -> Result<()>;
}
/// 802bc9a8 stores all eight lanes before802bdadc examines world membership.
pub fn set(phantom: usize, bounds: Aabb, backend: &mut impl Backend) -> Result<Admission> {
    backend.store(phantom, bounds)?;
    publish(phantom, BoundsSource::Stored(phantom), backend)
}
/// Publication does not overwrite stored bounds. Replayed operations can carry
/// older snapshots than the phantom's current storage.
pub fn publish(
    phantom: usize,
    source: BoundsSource,
    backend: &mut impl Backend,
) -> Result<Admission> {
    let Some(world) = backend.current_world(phantom)? else {
        return Ok(Admission::Detached);
    };
    let [critical, secondary] = backend.locks(world)?;
    if critical.wrapping_add(secondary) != 0 {
        let entry = backend.append(world, phantom)?;
        backend.retain(phantom)?;
        backend.attach_snapshot(entry, source)?;
        Ok(Admission::Deferred)
    } else {
        backend.immediate(world, phantom, source)?;
        Ok(Admission::Published)
    }
}
impl Update {
    /// Dispatch802d2b68 re-enters admission, then releases the phantom BEFORE
    /// freeing the32-byte snapshot, including when world membership changed.
    /// Backend failure stops at its actual boundary; no fabricated rollback.
    pub fn execute(self, backend: &mut impl Backend) -> Result<()> {
        if backend.current_world(self.phantom)? == Some(self.world) {
            publish(self.phantom, BoundsSource::Snapshot(self.snapshot), backend)?;
        }
        backend.release(self.phantom)?;
        backend.free_snapshot(self.snapshot)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct World {
        stored: Aabb,
        world: Option<usize>,
        locks: [u32; 2],
        refs: i32,
        pending: Vec<(usize, Option<usize>)>,
        snapshots: BTreeMap<usize, Aabb>,
        calls: Vec<&'static str>,
        delivered: Vec<Aabb>,
        mutate_on_retain: bool,
        fail: bool,
    }
    impl World {
        fn new() -> Self {
            Self {
                stored: [0; 8],
                world: Some(2),
                locks: [0, 1],
                refs: 1,
                pending: vec![],
                snapshots: BTreeMap::new(),
                calls: vec![],
                delivered: vec![],
                mutate_on_retain: false,
                fail: false,
            }
        }
        fn bounds(&self, source: BoundsSource) -> Aabb {
            match source {
                BoundsSource::Stored(7) => self.stored,
                BoundsSource::Snapshot(id) => self.snapshots[&id],
                _ => panic!("Unknown source"),
            }
        }
        fn operation(&self, index: usize) -> Update {
            Update {
                phantom: 7,
                world: 2,
                snapshot: self.pending[index].1.unwrap(),
            }
        }
    }
    impl Backend for World {
        fn store(&mut self, p: usize, b: Aabb) -> Result<()> {
            assert_eq!(p, 7);
            self.calls.push("store");
            self.stored = b;
            Ok(())
        }
        fn current_world(&mut self, p: usize) -> Result<Option<usize>> {
            assert_eq!(p, 7);
            self.calls.push("world");
            Ok(self.world)
        }
        fn locks(&mut self, w: usize) -> Result<[u32; 2]> {
            assert_eq!(w, 2);
            self.calls.push("locks");
            Ok(self.locks)
        }
        fn append(&mut self, w: usize, p: usize) -> Result<usize> {
            assert_eq!(w, 2);
            assert_eq!(p, 7);
            self.calls.push("append");
            let index = self.pending.len();
            self.pending.push((p, None));
            Ok(index)
        }
        fn retain(&mut self, p: usize) -> Result<()> {
            assert_eq!(p, 7);
            self.calls.push("retain");
            self.refs += 1;
            if self.mutate_on_retain {
                self.stored = [9; 8]
            };
            Ok(())
        }
        fn attach_snapshot(&mut self, index: usize, source: BoundsSource) -> Result<()> {
            self.calls.push("snapshot");
            let bounds = self.bounds(source);
            let id = index + 1;
            self.snapshots.insert(id, bounds);
            self.pending[index].1 = Some(id);
            Ok(())
        }
        fn immediate(&mut self, w: usize, p: usize, source: BoundsSource) -> Result<()> {
            assert_eq!((w, p), (2, 7));
            self.calls.push("publish");
            if self.fail {
                anyhow::bail!("Publication failed")
            };
            self.delivered.push(self.bounds(source));
            Ok(())
        }
        fn release(&mut self, p: usize) -> Result<()> {
            assert_eq!(p, 7);
            self.calls.push("release");
            self.refs -= 1;
            Ok(())
        }
        fn free_snapshot(&mut self, id: usize) -> Result<()> {
            self.calls.push("free");
            assert!(self.snapshots.remove(&id).is_some());
            Ok(())
        }
    }
    #[test]
    fn phantom_snapshots_redefer_without_rewinding_current_storage() -> Result<()> {
        let mut world = World::new();
        let first = [0x80000000, 1, 2, 0x7fc12345, 4, 5, 6, 7];
        assert_eq!(set(7, first, &mut world)?, Admission::Deferred);
        assert_eq!(
            world.calls,
            ["store", "world", "locks", "append", "retain", "snapshot"]
        );
        set(7, [8; 8], &mut world)?;
        assert_eq!(world.refs, 3);
        world.operation(0).execute(&mut world)?; // Still locked: copies old snapshot.
        assert_eq!(world.refs, 3);
        assert_eq!(world.stored, [8; 8]);
        assert_eq!(world.pending.len(), 3);
        assert_eq!(world.snapshots[&3], first);
        assert!(!world.snapshots.contains_key(&1));
        world.locks = [0, 0];
        world.operation(1).execute(&mut world)?;
        world.operation(2).execute(&mut world)?;
        assert_eq!(world.delivered, vec![[8; 8], first]);
        assert_eq!(world.stored, [8; 8]);
        assert_eq!(world.refs, 1);
        assert!(world.snapshots.is_empty());
        Ok(())
    }
    #[test]
    fn phantom_storage_retention_membership_and_failure_order() -> Result<()> {
        let mut world = World::new();
        world.mutate_on_retain = true;
        set(7, [1; 8], &mut world)?;
        assert_eq!(world.snapshots[&1], [9; 8]);
        world.world = Some(3);
        world.calls.clear();
        world.operation(0).execute(&mut world)?;
        assert_eq!(world.calls, ["world", "release", "free"]);
        assert_eq!(world.refs, 1);
        world.world = None;
        assert_eq!(set(7, [2; 8], &mut world)?, Admission::Detached);
        assert_eq!(world.stored, [2; 8]);
        world.world = Some(2);
        world.locks = [u32::MAX, 1];
        world.mutate_on_retain = false;
        assert_eq!(set(7, [3; 8], &mut world)?, Admission::Published);
        assert_eq!(world.delivered, vec![[3; 8]]);
        world.fail = true;
        assert!(set(7, [4; 8], &mut world).is_err());
        assert_eq!(world.stored, [4; 8]);
        world.fail = false;
        world.locks = [1, 0];
        set(7, [5; 8], &mut world)?;
        let operation = world.operation(1);
        world.locks = [0, 0];
        world.fail = true;
        world.calls.clear();
        assert!(operation.execute(&mut world).is_err());
        assert_eq!(world.calls, ["world", "world", "locks", "publish"]);
        assert_eq!(world.refs, 2);
        assert!(world.snapshots.contains_key(&operation.snapshot));
        Ok(())
    }
}
