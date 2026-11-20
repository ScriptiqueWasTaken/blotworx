//! Shared compatibility records; broadphase objects still need Box3D binding.
use super::{hk_reference::Header, phantom_overlaps::Overlaps, phantom_update::Aabb};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub struct Phantom {
    pub header: Header,
    pub overlaps: Overlaps,
    /// Only AABB phantoms own a stored AABB. Simple shape phantoms calculate
    /// bounds from their shape and motion; do not fabricate a zero AABB.
    pub bounds: Option<Aabb>,
    pub world: Option<usize>,
    pub user_data: Option<super::collidable_owner::EntityData>,
    /// Unknown for externally imported records until their collidable is read.
    pub collidable: Option<AabbCollidable>,
}
/// DOL802bc26c / 802e0dec: shape-less broadphase kind 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AabbCollidable {
    pub filter_info: u32,
    pub broadphase_kind: u8,
    pub owner_offset: i8,
    pub shape: Option<usize>,
    pub shape_key: u32,
}
#[derive(Default)]
pub struct Objects {
    pub phantoms: BTreeMap<usize, Phantom>,
    pub listeners: BTreeMap<usize, Header>,
    pub shapes: BTreeMap<usize, Header>,
    pub systems: BTreeMap<usize, PhysicsSystem>,
    next_identity: usize,
}
/// DOL802d7eb0: empty body/constraint/action/phantom arrays, null name/userdata,
/// active=true. Component-created systems contain phantoms only so far.
pub struct PhysicsSystem {
    pub header: Header,
    pub phantoms: Vec<usize>,
    pub active: bool,
}
/// Actual broadphase bindings resolve handles and collidable ownership. Neither
/// entity indices nor Box3D shape indices are native handle addresses.
pub trait PairBindings {
    fn handle(&self, address: usize) -> Result<super::phantom_pairs::Handle>;
    fn phantom_identity(&self, collidable: usize) -> Result<usize>;
}
pub trait RefilterBindings {
    fn collision(&mut self, collidables: [usize; 2]) -> Result<u32>;
    fn collidable_kind(&self, collidable: usize) -> Result<u8>;
    fn phantom_identity(&self, collidable: usize) -> Result<usize>;
}
/// Registered world agents operate on these same records, including callback
/// mutations. No copied listener/membership facade or default agent is used.
pub trait PairDispatcher {
    fn handle(&self, address: usize) -> Result<super::phantom_pairs::Handle>;
    fn filter(&mut self, objects: &mut Objects, collidables: [usize; 2]) -> Result<u32>;
    fn removed(
        &mut self,
        objects: &mut Objects,
        types: [usize; 2],
        pair: super::phantom_pairs::Pair,
    ) -> Result<()>;
    fn added(
        &mut self,
        objects: &mut Objects,
        types: [usize; 2],
        pair: super::phantom_pairs::Pair,
    ) -> Result<()>;
}
struct Dispatch<'a, B> {
    objects: &'a mut Objects,
    bindings: &'a mut B,
}
impl<B: PairDispatcher> super::phantom_pairs::Dispatcher for Dispatch<'_, B> {
    fn handle(&self, address: usize) -> Result<super::phantom_pairs::Handle> {
        self.bindings.handle(address)
    }
    fn filter(&mut self, c: [usize; 2]) -> Result<u32> {
        self.bindings.filter(self.objects, c)
    }
    fn removed(&mut self, t: [usize; 2], p: super::phantom_pairs::Pair) -> Result<()> {
        self.bindings.removed(self.objects, t, p)
    }
    fn added(&mut self, t: [usize; 2], p: super::phantom_pairs::Pair) -> Result<()> {
        self.bindings.added(self.objects, t, p)
    }
}
impl Objects {
    /// DOL802bdc20..802bdd68: test initial pair counts, cancel/deliver, then
    /// ordered removal of null listeners. Even fully cancelled input enters
    /// cleanup; an initially empty update skips it. Callbacks see shared state.
    pub fn update_pairs(
        &mut self,
        phantom: usize,
        added: &mut Vec<super::phantom_pairs::Pair>,
        removed: &mut Vec<super::phantom_pairs::Pair>,
        bindings: &mut impl PairDispatcher,
    ) -> Result<()> {
        ensure!(
            self.phantoms.contains_key(&phantom),
            "Unknown phantom for pair update"
        );
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }
        super::phantom_pairs::dispatch(
            added,
            removed,
            &mut Dispatch {
                objects: self,
                bindings,
            },
        )?;
        self.phantoms
            .get_mut(&phantom)
            .context("Phantom disappeared during pair update")?
            .overlaps
            .listeners
            .retain(Option::is_some);
        Ok(())
    }
    /// DOL802da71c..802da890. Membership precedes filter; second-side type and
    /// membership are reread after first-side callbacks. Rejecting an enter
    /// listener on one side doesn't suppress the other phantom's update.
    pub fn refilter(
        &mut self,
        phantom: usize,
        collidable: usize,
        other: usize,
        bindings: &mut impl RefilterBindings,
        callbacks: &mut impl super::phantom_overlaps::Callbacks,
    ) -> Result<()> {
        let was = self
            .phantoms
            .get(&phantom)
            .context("Unknown phantom for refilter")?
            .overlaps
            .contains(other);
        let admitted = bindings.collision([collidable, other])? >> 24 != 0;
        if admitted && !was {
            self.add_overlap(phantom, other, callbacks)?;
        } else if !admitted && was {
            self.remove_overlap(phantom, other, callbacks)?;
        }
        if bindings.collidable_kind(other)? == 2 {
            let second = bindings.phantom_identity(other)?;
            let was = self
                .phantoms
                .get(&second)
                .context("Unknown second phantom for refilter")?
                .overlaps
                .contains(collidable);
            if admitted && !was {
                self.add_overlap(second, collidable, callbacks)?;
            } else if !admitted && was {
                self.remove_overlap(second, collidable, callbacks)?;
            }
        }
        Ok(())
    }
    /// Registered phantom pair agent; membership belongs to the shared records.
    /// Pair generation/filtering must precede this call in the world dispatcher.
    pub fn deliver_pair(
        &mut self,
        pair: super::phantom_pairs::Pair,
        entered: bool,
        bindings: &impl PairBindings,
        callbacks: &mut impl super::phantom_overlaps::Callbacks,
    ) -> Result<()> {
        super::phantom_pairs::phantom_pair(
            pair,
            entered,
            &mut PairDelivery {
                objects: self,
                bindings,
                callbacks,
            },
        )
    }
    /// Callbacks must be bound to this phantom's actual collidable/owner.
    pub fn add_overlap(
        &mut self,
        identity: usize,
        collidable: usize,
        callbacks: &mut impl super::phantom_overlaps::Callbacks,
    ) -> Result<bool> {
        let record = self
            .phantoms
            .get_mut(&identity)
            .context("Unknown phantom for overlap entry")?;
        ensure!(
            record.collidable.is_none_or(|c| c.shape.is_none()),
            "Simple phantom overlap requires a visiting-shape binding"
        );
        record.overlaps.add_from(identity, collidable, callbacks)
    }
    pub fn remove_overlap(
        &mut self,
        identity: usize,
        collidable: usize,
        callbacks: &mut impl super::phantom_overlaps::Callbacks,
    ) -> Result<bool> {
        let record = self
            .phantoms
            .get_mut(&identity)
            .context("Unknown phantom for overlap exit")?;
        ensure!(
            record.collidable.is_none_or(|c| c.shape.is_none()),
            "Simple phantom overlap requires a visiting-shape binding"
        );
        record.overlaps.remove_from(identity, collidable, callbacks)
    }
    pub fn retain_phantom(
        &mut self,
        identity: usize,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<()> {
        let phantom = self
            .phantoms
            .get_mut(&identity)
            .context("Unknown phantom for retain")?;
        super::hk_reference::retain(&mut phantom.header, backend)
    }
    /// Backend must complete this phantom's world/broadphase/listener teardown,
    /// shape ownership and allocation destruction. Removing a record is last.
    pub fn release_phantom(
        &mut self,
        identity: usize,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<()> {
        let phantom = self
            .phantoms
            .get_mut(&identity)
            .context("Unknown phantom for release")?;
        let mut adapter = Release {
            backend,
            destroyed: false,
        };
        super::hk_reference::release(&mut phantom.header, &mut adapter)?;
        if adapter.destroyed {
            self.phantoms.remove(&identity);
        }
        Ok(())
    }
    pub fn retain_listener(
        &mut self,
        identity: usize,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<()> {
        let header = self
            .listeners
            .get_mut(&identity)
            .context("Unknown listener for retain")?;
        super::hk_reference::retain(header, backend)
    }
    /// Backend destruction must target this identity. Record is removed only
    /// after successful native final-release destruction; errors retain state.
    pub fn release_listener(
        &mut self,
        identity: usize,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<()> {
        let header = self
            .listeners
            .get_mut(&identity)
            .context("Unknown listener for release")?;
        let mut adapter = Release {
            backend,
            destroyed: false,
        };
        super::hk_reference::release(header, &mut adapter)?;
        if adapter.destroyed {
            self.listeners.remove(&identity);
        }
        Ok(())
    }
    /// Identity comes from the owning allocation/binding backend, not entity index.
    pub fn insert_phantom(
        &mut self,
        identity: usize,
        header: Header,
        bounds: Aabb,
        world: Option<usize>,
    ) -> Result<()> {
        ensure!(identity != 0, "Null phantom identity");
        self.ensure_free(identity)?;
        self.next_identity = self.next_identity.max(identity);
        self.phantoms.insert(
            identity,
            Phantom {
                header,
                overlaps: Default::default(),
                bounds: Some(bounds),
                world,
                user_data: None,
                collidable: None,
            },
        );
        Ok(())
    }
    /// REL377ca0 creates userdata, stores the owning entity in its first field,
    /// then publishes its address to phantom+c at377d84. This retains the
    /// resolver-visible semantics only; the rest of the176-byte userdata needs
    /// its separate collision/transform/contact implementation.
    pub fn bind_phantom_entity(&mut self, identity: usize, entity_token: usize) -> Result<()> {
        ensure!(
            entity_token != 0,
            "Null entity token requires embedded userdata semantics"
        );
        self.phantoms
            .get_mut(&identity)
            .context("Unknown phantom for entity binding")?
            .user_data = Some(super::collidable_owner::EntityData::phantom(Some(
            entity_token,
        )));
        Ok(())
    }
    pub fn owner_user_data(&self, identity: usize) -> Result<super::collidable_owner::UserData> {
        Ok(self
            .phantoms
            .get(&identity)
            .context("Unknown phantom owner record")?
            .user_data
            .map(|data| data.owner())
            .unwrap_or(super::collidable_owner::UserData::Missing))
    }
    /// Host allocation token, never a Wii address, entity ID or Box3D shape ID.
    /// Do not reuse freed identities: stale component bindings must diagnose.
    pub fn allocate_size_phantom(&mut self, bounds: Aabb, filter_info: u32) -> Result<usize> {
        let identity = self
            .next_identity
            .checked_add(1)
            .context("Physics identity space exhausted")?;
        self.insert_size_phantom(identity, bounds, filter_info)?;
        Ok(identity)
    }
    fn ensure_free(&self, identity: usize) -> Result<()> {
        ensure!(identity != 0, "Null physics allocation identity");
        ensure!(
            !self.phantoms.contains_key(&identity)
                && !self.listeners.contains_key(&identity)
                && !self.shapes.contains_key(&identity)
                && !self.systems.contains_key(&identity),
            "Duplicate physics object identity {identity}"
        );
        Ok(())
    }
    pub fn allocate_shape_reference(&mut self, header: Header) -> Result<usize> {
        let identity = self
            .next_identity
            .checked_add(1)
            .context("Physics identity space exhausted")?;
        self.ensure_free(identity)?;
        self.next_identity = identity;
        self.shapes.insert(identity, header);
        Ok(identity)
    }
    /// DOL802bf09c ->802bec9c ->802e0dec. The geometry/motion backing is
    /// retained by World; this identity is neither a Wii pointer nor shape index.
    pub fn allocate_simple_phantom(
        &mut self,
        shape: usize,
        filter_info: u32,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<usize> {
        let identity = self
            .next_identity
            .checked_add(1)
            .context("Physics identity space exhausted")?;
        self.ensure_free(identity)?;
        super::hk_reference::retain(
            self.shapes
                .get_mut(&shape)
                .context("Unknown phantom shape reference")?,
            backend,
        )?;
        self.next_identity = identity;
        self.phantoms.insert(
            identity,
            Phantom {
                header: Header {
                    memory_size: 0x170,
                    references: 1,
                },
                overlaps: Default::default(),
                bounds: None,
                world: None,
                user_data: None,
                collidable: Some(AabbCollidable {
                    filter_info,
                    broadphase_kind: 2,
                    owner_offset: -16,
                    shape: Some(shape),
                    shape_key: u32::MAX,
                }),
            },
        );
        Ok(identity)
    }
    pub fn allocate_phantom_system(&mut self) -> Result<usize> {
        let identity = self
            .next_identity
            .checked_add(1)
            .context("Physics identity space exhausted")?;
        self.ensure_free(identity)?;
        self.next_identity = identity;
        self.systems.insert(
            identity,
            PhysicsSystem {
                header: Header {
                    memory_size: 0x44,
                    references: 1,
                },
                phantoms: Vec::new(),
                active: true,
            },
        );
        Ok(identity)
    }
    /// DOL802d895c retains before appending; duplicates retain again.
    pub fn system_add_phantom(
        &mut self,
        system: usize,
        phantom: usize,
        backend: &mut impl super::hk_reference::Backend,
    ) -> Result<()> {
        ensure!(
            self.systems.contains_key(&system),
            "Unknown phantom physics system"
        );
        self.retain_phantom(phantom, backend)?;
        self.systems
            .get_mut(&system)
            .unwrap()
            .phantoms
            .try_reserve(1)?;
        self.systems
            .get_mut(&system)
            .unwrap()
            .phantoms
            .push(phantom);
        Ok(())
    }
    pub fn allocate_pickup_listener(&mut self) -> Result<usize> {
        let identity = self
            .next_identity
            .checked_add(1)
            .context("Physics identity space exhausted")?;
        self.insert_pickup_listener(identity)?;
        Ok(identity)
    }
    /// Native size-fallback allocation and constructor state. The backend must
    /// provide the allocation identity; this does not register a Box3D sensor.
    pub fn insert_size_phantom(
        &mut self,
        identity: usize,
        bounds: Aabb,
        filter_info: u32,
    ) -> Result<()> {
        self.insert_phantom(
            identity,
            Header {
                memory_size: super::phantom_bounds::SIZE_PHANTOM_MEMORY,
                references: 1,
            },
            bounds,
            None,
        )?;
        self.phantoms.get_mut(&identity).unwrap().collidable = Some(AabbCollidable {
            filter_info,
            broadphase_kind: 2,
            owner_offset: -16,
            shape: None,
            shape_key: u32::MAX,
        });
        Ok(())
    }
    /// PlayerPickup listener constructor389fec..38a018: size12, references1.
    pub fn insert_pickup_listener(&mut self, identity: usize) -> Result<()> {
        ensure!(identity != 0, "Null listener identity");
        self.ensure_free(identity)?;
        self.next_identity = self.next_identity.max(identity);
        self.listeners.insert(
            identity,
            Header {
                memory_size: 12,
                references: 1,
            },
        );
        Ok(())
    }
    pub fn register(&mut self, phantom: usize, listener: Option<usize>) -> Result<()> {
        if let Some(listener) = listener {
            ensure!(
                self.listeners.contains_key(&listener),
                "Unknown pickup listener {listener}"
            );
        }
        self.phantoms
            .get_mut(&phantom)
            .context("Unknown phantom for listener registration")?
            .overlaps
            .register(listener);
        Ok(())
    }
    pub fn unregister(&mut self, phantom: usize, listener: Option<usize>) -> Result<()> {
        self.phantoms
            .get_mut(&phantom)
            .context("Unknown phantom for listener removal")?
            .overlaps
            .unregister(listener)
    }
}
struct PairDelivery<'a, B, C> {
    objects: &'a mut Objects,
    bindings: &'a B,
    callbacks: &'a mut C,
}
impl<B: PairBindings, C: super::phantom_overlaps::Callbacks> super::phantom_pairs::PhantomListener
    for PairDelivery<'_, B, C>
{
    fn handle(&self, address: usize) -> Result<super::phantom_pairs::Handle> {
        self.bindings.handle(address)
    }
    fn overlap(
        &mut self,
        phantom_collidable: usize,
        other_collidable: usize,
        entered: bool,
    ) -> Result<()> {
        let identity = self.bindings.phantom_identity(phantom_collidable)?;
        if entered {
            self.objects
                .add_overlap(identity, other_collidable, self.callbacks)?;
        } else {
            self.objects
                .remove_overlap(identity, other_collidable, self.callbacks)?;
        }
        Ok(())
    }
}
struct Release<'a, T> {
    backend: &'a mut T,
    destroyed: bool,
}
impl<T: super::hk_reference::Backend> super::hk_reference::Backend for Release<'_, T> {
    fn enter(&mut self) -> Result<()> {
        self.backend.enter()
    }
    fn leave(&mut self) -> Result<()> {
        self.backend.leave()
    }
    fn destroy(&mut self, header: &mut Header) -> Result<()> {
        self.backend.destroy(header)?;
        self.destroyed = true;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simple_phantom_system_retains_real_shape_and_phantom_references() -> Result<()> {
        let mutex = std::sync::Mutex::new(());
        let mut monitor = super::super::hk_reference::Monitor {
            mutex: &mutex,
            held: None,
        };
        let mut objects = Objects::default();
        let shape = objects.allocate_shape_reference(Header {
            memory_size: 64,
            references: 1,
        })?;
        let phantom = objects.allocate_simple_phantom(shape, 0x18003, &mut monitor)?;
        let system = objects.allocate_phantom_system()?;
        assert_eq!(objects.shapes[&shape].references, 2);
        assert_eq!(objects.phantoms[&phantom].bounds, None);
        assert_eq!(
            objects.phantoms[&phantom].collidable.unwrap().shape,
            Some(shape)
        );
        assert_eq!(
            objects.phantoms[&phantom].header,
            Header {
                memory_size: 0x170,
                references: 1
            }
        );
        objects.system_add_phantom(system, phantom, &mut monitor)?;
        objects.system_add_phantom(system, phantom, &mut monitor)?;
        assert_eq!(objects.phantoms[&phantom].header.references, 3);
        assert_eq!(objects.systems[&system].phantoms, vec![phantom, phantom]);
        assert!(objects.systems[&system].active);
        assert!(objects.insert_pickup_listener(shape).is_err());
        assert!(objects.insert_size_phantom(system, [0; 8], 0).is_err());
        assert!(monitor.held.is_none());
        Ok(())
    }
    #[test]
    fn phantom_entity_userdata_is_owned_mutable_and_distinct_from_allocation() -> Result<()> {
        use super::super::collidable_owner::UserData;
        let mut objects = Objects::default();
        let phantom = objects.allocate_size_phantom([0; 8], 0x18001)?;
        assert_eq!(objects.owner_user_data(phantom)?, UserData::Missing);
        objects.bind_phantom_entity(phantom, 100)?;
        assert_eq!(objects.owner_user_data(phantom)?, UserData::Entity(100));
        objects.bind_phantom_entity(phantom, 200)?;
        assert_eq!(objects.owner_user_data(phantom)?, UserData::Entity(200));
        assert!(objects.bind_phantom_entity(phantom, 0).is_err());
        assert_eq!(objects.owner_user_data(phantom)?, UserData::Entity(200));
        assert!(objects.bind_phantom_entity(999, 300).is_err());
        assert!(objects.owner_user_data(999).is_err());
        assert_eq!(objects.phantoms[&phantom].header.references, 1);
        assert_eq!(objects.phantoms[&phantom].world, None);
        Ok(())
    }
    #[test]
    fn allocation_tokens_remain_unique_after_teardown_and_external_bindings() -> Result<()> {
        let mut objects = Objects::default();
        objects.insert_pickup_listener(20)?;
        let first = objects.allocate_size_phantom([1; 8], 0x18001)?;
        assert_eq!(first, 21);
        // Simulate the record having completed its tested release lifecycle.
        objects.phantoms.remove(&first);
        let second = objects.allocate_size_phantom([2; 8], 0x18003)?;
        assert_eq!(second, 22);
        assert!(!objects.phantoms.contains_key(&first));
        objects.insert_pickup_listener(usize::MAX)?;
        assert!(objects.allocate_size_phantom([3; 8], 0).is_err());
        assert_eq!(objects.phantoms.len(), 1);
        Ok(())
    }
    #[test]
    fn bounds_pair_dispatch_updates_shared_overlaps_before_null_cleanup() -> Result<()> {
        use super::super::phantom_overlaps::{Added, Callbacks, Removed};
        use super::super::phantom_pairs::{Handle, Pair};
        struct Bindings;
        impl PairBindings for Bindings {
            fn handle(&self, a: usize) -> Result<Handle> {
                Ok(Handle {
                    kind: if a == 100 { 2 } else { 1 },
                    collidable_offset: -8,
                })
            }
            fn phantom_identity(&self, c: usize) -> Result<usize> {
                assert_eq!(c, 92);
                Ok(7)
            }
        }
        struct Receiver {
            events: Vec<(usize, bool)>,
        }
        impl Callbacks for Receiver {
            fn added(&mut self, _: usize, p: &mut Overlaps, e: &mut Added) -> Result<()> {
                assert_eq!(e.phantom, Some(7));
                self.events.push((e.collidable, true));
                p.register(None);
                Ok(())
            }
            fn removed(&mut self, _: usize, p: &mut Overlaps, e: &mut Removed) -> Result<()> {
                assert_eq!(e.phantom, Some(7));
                assert!(e.was_present);
                self.events.push((e.collidable, false));
                p.register(None);
                Ok(())
            }
        }
        struct Dispatcher {
            receiver: Receiver,
            filters: Vec<usize>,
            fail: bool,
        }
        impl PairDispatcher for Dispatcher {
            fn handle(&self, a: usize) -> Result<Handle> {
                Bindings.handle(a)
            }
            fn filter(&mut self, o: &mut Objects, c: [usize; 2]) -> Result<u32> {
                assert!(!o.phantoms[&7].overlaps.contains(192));
                self.filters.push(c[1]);
                if self.fail {
                    anyhow::bail!("Filter failed")
                };
                Ok(if c[1] == 392 { 1 } else { 0x01000000 })
            }
            fn removed(&mut self, o: &mut Objects, t: [usize; 2], p: Pair) -> Result<()> {
                assert_eq!(t, [2, 1]);
                o.deliver_pair(p, false, &Bindings, &mut self.receiver)
            }
            fn added(&mut self, o: &mut Objects, t: [usize; 2], p: Pair) -> Result<()> {
                assert_eq!(t, [2, 1]);
                o.deliver_pair(p, true, &Bindings, &mut self.receiver)
            }
        }
        let mut objects = Objects::default();
        objects.insert_size_phantom(7, [0; 8], 0x18001)?;
        let p = &mut objects.phantoms.get_mut(&7).unwrap().overlaps;
        p.collidables.push(192);
        p.listeners = vec![None, Some(11), None];
        let mut dispatcher = Dispatcher {
            receiver: Receiver { events: vec![] },
            filters: vec![],
            fail: false,
        };
        objects.update_pairs(
            7,
            &mut vec![Pair([100, 300]), Pair([100, 400]), Pair([100, 500])],
            &mut vec![Pair([100, 200]), Pair([500, 100])],
            &mut dispatcher,
        )?;
        assert_eq!(dispatcher.receiver.events, vec![(192, false), (292, true)]);
        assert_eq!(dispatcher.filters, vec![292, 392]);
        assert_eq!(objects.phantoms[&7].overlaps.collidables, vec![292]);
        assert_eq!(objects.phantoms[&7].overlaps.listeners, vec![Some(11)]);
        objects
            .phantoms
            .get_mut(&7)
            .unwrap()
            .overlaps
            .register(None);
        objects.update_pairs(7, &mut vec![], &mut vec![], &mut dispatcher)?;
        assert_eq!(
            objects.phantoms[&7].overlaps.listeners,
            vec![Some(11), None]
        );
        objects.update_pairs(
            7,
            &mut vec![Pair([100, 500])],
            &mut vec![Pair([500, 100])],
            &mut dispatcher,
        )?;
        assert_eq!(objects.phantoms[&7].overlaps.listeners, vec![Some(11)]);
        objects
            .phantoms
            .get_mut(&7)
            .unwrap()
            .overlaps
            .register(None);
        dispatcher.fail = true;
        assert!(objects
            .update_pairs(7, &mut vec![Pair([100, 600])], &mut vec![], &mut dispatcher)
            .is_err());
        assert_eq!(
            objects.phantoms[&7].overlaps.listeners,
            vec![Some(11), None]
        );
        Ok(())
    }
    #[test]
    fn refilter_preserves_two_sided_membership_rejection_and_metadata_reread() -> Result<()> {
        use super::super::phantom_overlaps::{Added, Callbacks, Removed};
        use std::cell::Cell;
        struct Bindings<'a> {
            kind: &'a Cell<u8>,
            result: u32,
        }
        impl RefilterBindings for Bindings<'_> {
            fn collision(&mut self, c: [usize; 2]) -> Result<u32> {
                assert_eq!(c, [92, 192]);
                Ok(self.result)
            }
            fn collidable_kind(&self, c: usize) -> Result<u8> {
                assert_eq!(c, 192);
                Ok(self.kind.get())
            }
            fn phantom_identity(&self, c: usize) -> Result<usize> {
                assert_eq!(c, 192);
                Ok(9)
            }
        }
        struct Receiver<'a> {
            kind: &'a Cell<u8>,
            reject: bool,
            flip: bool,
            calls: Vec<(usize, bool)>,
        }
        impl Callbacks for Receiver<'_> {
            fn added(&mut self, l: usize, _: &mut Overlaps, e: &mut Added) -> Result<()> {
                self.calls.push((l, true));
                if l == 10 && self.reject {
                    e.result = 1;
                }
                Ok(())
            }
            fn removed(&mut self, l: usize, _: &mut Overlaps, _: &mut Removed) -> Result<()> {
                self.calls.push((l, false));
                if l == 10 && self.flip {
                    self.kind.set(1);
                }
                Ok(())
            }
        }
        let kind = Cell::new(2);
        let mut bindings = Bindings {
            kind: &kind,
            result: 0x01000000,
        };
        let mut receiver = Receiver {
            kind: &kind,
            reject: true,
            flip: false,
            calls: vec![],
        };
        let mut objects = Objects::default();
        for id in [8, 9] {
            objects.insert_size_phantom(id, [0; 8], 0x1800c)?;
        }
        for (p, l) in [(8, 10), (9, 11)] {
            objects.insert_pickup_listener(l)?;
            objects.register(p, Some(l))?;
        }
        objects.refilter(8, 92, 192, &mut bindings, &mut receiver)?;
        assert_eq!(receiver.calls, vec![(10, true), (11, true)]);
        assert!(!objects.phantoms[&8].overlaps.contains(192));
        assert!(objects.phantoms[&9].overlaps.contains(92));
        receiver.calls.clear();
        bindings.result = 1;
        objects.refilter(8, 92, 192, &mut bindings, &mut receiver)?;
        assert_eq!(receiver.calls, vec![(11, false)]);
        bindings.result = 0x01000000;
        receiver.reject = false;
        receiver.calls.clear();
        objects.refilter(8, 92, 192, &mut bindings, &mut receiver)?;
        receiver.calls.clear();
        objects.refilter(8, 92, 192, &mut bindings, &mut receiver)?;
        assert!(receiver.calls.is_empty());
        bindings.result = 0;
        receiver.flip = true;
        objects.refilter(8, 92, 192, &mut bindings, &mut receiver)?;
        assert_eq!(receiver.calls, vec![(10, false)]);
        assert!(objects.phantoms[&9].overlaps.contains(92));
        Ok(())
    }
    #[test]
    fn pair_agent_updates_shared_membership_on_both_sides_in_native_order() -> Result<()> {
        use super::super::{
            phantom_overlaps::{Added, Callbacks, Removed},
            phantom_pairs::{Handle, Pair},
        };
        struct Bindings;
        impl PairBindings for Bindings {
            fn handle(&self, address: usize) -> Result<Handle> {
                ensure!([100, 200].contains(&address), "Unknown handle");
                Ok(Handle {
                    kind: 2,
                    collidable_offset: -8,
                })
            }
            fn phantom_identity(&self, collidable: usize) -> Result<usize> {
                match collidable {
                    92 => Ok(8),
                    192 => Ok(9),
                    _ => anyhow::bail!("Unknown collidable"),
                }
            }
        }
        struct Receiver {
            calls: Vec<(usize, usize, bool)>,
            reject: bool,
        }
        impl Callbacks for Receiver {
            fn added(&mut self, l: usize, p: &mut Overlaps, e: &mut Added) -> Result<()> {
                assert!(!p.contains(e.collidable));
                self.calls.push((l, e.collidable, true));
                if self.reject && l == 10 {
                    e.result = 1;
                }
                Ok(())
            }
            fn removed(&mut self, l: usize, p: &mut Overlaps, e: &mut Removed) -> Result<()> {
                assert_eq!(p.contains(e.collidable), e.was_present);
                self.calls.push((l, e.collidable, false));
                Ok(())
            }
        }
        let mut objects = Objects::default();
        for id in [8, 9] {
            objects.insert_size_phantom(id, [0; 8], 0x1800c)?;
        }
        for (phantom, listener) in [(8, 10), (8, 11), (9, 12)] {
            objects.insert_pickup_listener(listener)?;
            objects.register(phantom, Some(listener))?;
        }
        let mut receiver = Receiver {
            calls: vec![],
            reject: true,
        };
        objects.deliver_pair(Pair([100, 200]), true, &Bindings, &mut receiver)?;
        assert_eq!(
            receiver.calls,
            vec![(11, 192, true), (10, 192, true), (12, 92, true)]
        );
        assert!(objects.phantoms[&8].overlaps.collidables.is_empty());
        assert_eq!(objects.phantoms[&9].overlaps.collidables, vec![92]);
        receiver.calls.clear();
        objects.deliver_pair(Pair([100, 200]), false, &Bindings, &mut receiver)?;
        assert_eq!(
            receiver.calls,
            vec![(11, 192, false), (10, 192, false), (12, 92, false)]
        );
        assert!(objects
            .phantoms
            .values()
            .all(|p| p.overlaps.collidables.is_empty()));
        assert!(objects
            .deliver_pair(Pair([300, 200]), true, &Bindings, &mut receiver)
            .is_err());
        Ok(())
    }
    #[test]
    fn size_constructor_preserves_padded_bounds_and_filter() -> Result<()> {
        let mut objects = Objects::default();
        let bounds = [0x80000000, 1, 2, 0x7fc12345, 4, 5, 6, 7];
        objects.insert_size_phantom(8, bounds, 0x61a003)?;
        let phantom = &objects.phantoms[&8];
        assert_eq!(
            phantom.header,
            Header {
                memory_size: 224,
                references: 1
            }
        );
        assert_eq!(phantom.bounds, Some(bounds));
        assert_eq!(phantom.world, None);
        assert_eq!(
            phantom.collidable,
            Some(AabbCollidable {
                filter_info: 0x61a003,
                broadphase_kind: 2,
                owner_offset: -16,
                shape: None,
                shape_key: u32::MAX,
            })
        );
        assert!(objects.insert_size_phantom(8, [0; 8], 0).is_err());
        assert_eq!(objects.phantoms[&8].bounds, Some(bounds));
        assert!(objects.insert_size_phantom(0, [0; 8], 0).is_err());
        Ok(())
    }
    #[test]
    fn shared_overlap_delivery_preserves_callback_order_and_membership() -> Result<()> {
        use super::super::phantom_overlaps::{Added, Callbacks, Removed};
        struct Listener {
            calls: Vec<(usize, bool)>,
            reject: bool,
        }
        impl Callbacks for Listener {
            fn added(&mut self, l: usize, p: &mut Overlaps, e: &mut Added) -> Result<()> {
                assert!(!p.contains(e.collidable));
                self.calls.push((l, true));
                if self.reject {
                    e.result = 1;
                }
                Ok(())
            }
            fn removed(&mut self, l: usize, p: &mut Overlaps, e: &mut Removed) -> Result<()> {
                assert_eq!(p.contains(e.collidable), e.was_present);
                self.calls.push((l, false));
                Ok(())
            }
        }
        let mut objects = Objects::default();
        objects.insert_phantom(
            8,
            Header {
                memory_size: 128,
                references: 1,
            },
            [0; 8],
            Some(3),
        )?;
        for id in [9, 10] {
            objects.insert_pickup_listener(id)?;
            objects.register(8, Some(id))?;
        }
        let mut listener = Listener {
            calls: vec![],
            reject: true,
        };
        assert!(!objects.add_overlap(8, 100, &mut listener)?);
        assert_eq!(listener.calls, vec![(10, true), (9, true)]);
        assert!(!objects.phantoms[&8].overlaps.contains(100));
        listener.reject = false;
        listener.calls.clear();
        assert!(objects.add_overlap(8, 100, &mut listener)?);
        assert!(objects.phantoms[&8].overlaps.contains(100));
        assert!(objects.remove_overlap(8, 100, &mut listener)?);
        assert_eq!(
            listener.calls,
            vec![(10, true), (9, true), (10, false), (9, false)]
        );
        assert!(!objects.phantoms[&8].overlaps.contains(100));
        assert!(objects.add_overlap(99, 100, &mut listener).is_err());
        Ok(())
    }
    #[test]
    fn phantom_ownership_keeps_membership_and_bounds_until_final_destruction() -> Result<()> {
        struct Backend(usize);
        impl super::super::hk_reference::Backend for Backend {
            fn enter(&mut self) -> Result<()> {
                Ok(())
            }
            fn leave(&mut self) -> Result<()> {
                Ok(())
            }
            fn destroy(&mut self, h: &mut Header) -> Result<()> {
                assert_eq!(h.references, 0);
                self.0 += 1;
                Ok(())
            }
        }
        let mut objects = Objects::default();
        let bounds = [7; 8];
        objects.insert_phantom(
            8,
            Header {
                memory_size: 128,
                references: 1,
            },
            bounds,
            Some(3),
        )?;
        let mut backend = Backend(0);
        objects.retain_phantom(8, &mut backend)?;
        objects.release_phantom(8, &mut backend)?;
        assert_eq!(objects.phantoms[&8].header.references, 1);
        assert_eq!(objects.phantoms[&8].bounds, Some(bounds));
        assert_eq!(objects.phantoms[&8].world, Some(3));
        objects.release_phantom(8, &mut backend)?;
        assert!(!objects.phantoms.contains_key(&8));
        assert_eq!(backend.0, 1);
        objects.insert_phantom(
            9,
            Header {
                memory_size: 0,
                references: 1,
            },
            bounds,
            None,
        )?;
        objects.release_phantom(9, &mut backend)?;
        assert!(objects.phantoms.contains_key(&9));
        assert_eq!(backend.0, 1);
        assert!(objects.retain_phantom(99, &mut backend).is_err());
        Ok(())
    }
    #[test]
    fn listener_release_removes_only_after_real_destructor_success() -> Result<()> {
        struct Backend {
            fail: bool,
            destroyed: usize,
        }
        impl super::super::hk_reference::Backend for Backend {
            fn enter(&mut self) -> Result<()> {
                Ok(())
            }
            fn leave(&mut self) -> Result<()> {
                Ok(())
            }
            fn destroy(&mut self, h: &mut Header) -> Result<()> {
                assert_eq!(h.references, 0);
                if self.fail {
                    anyhow::bail!("No allocator release")
                };
                self.destroyed += 1;
                Ok(())
            }
        }
        let mut objects = Objects::default();
        objects.insert_pickup_listener(9)?;
        let mut backend = Backend {
            fail: false,
            destroyed: 0,
        };
        objects.retain_listener(9, &mut backend)?;
        objects.release_listener(9, &mut backend)?;
        assert_eq!(objects.listeners[&9].references, 1);
        objects.release_listener(9, &mut backend)?;
        assert!(!objects.listeners.contains_key(&9));
        assert_eq!(backend.destroyed, 1);
        objects.insert_pickup_listener(10)?;
        backend.fail = true;
        assert!(objects.release_listener(10, &mut backend).is_err());
        assert_eq!(objects.listeners[&10].references, 0);
        assert!(objects.release_listener(99, &mut backend).is_err());
        Ok(())
    }
    #[test]
    fn object_records_keep_identity_raw_bounds_and_native_listener_duplicates() -> Result<()> {
        let mut objects = Objects::default();
        let bounds = [0x80000000, 1, 2, 0x7fc12345, 4, 5, 6, 7];
        objects.insert_phantom(
            8,
            Header {
                memory_size: 128,
                references: 1,
            },
            bounds,
            Some(3),
        )?;
        objects.insert_pickup_listener(9)?;
        assert!(objects.insert_pickup_listener(8).is_err());
        assert!(objects
            .insert_phantom(
                9,
                Header {
                    memory_size: 1,
                    references: 1
                },
                [0; 8],
                None
            )
            .is_err());
        for listener in [Some(9), None, Some(9)] {
            objects.register(8, listener)?;
        }
        assert_eq!(objects.phantoms[&8].bounds, Some(bounds));
        assert_eq!(objects.phantoms[&8].world, Some(3));
        assert_eq!(
            objects.listeners[&9],
            Header {
                memory_size: 12,
                references: 1
            }
        ); // Registration does not retain.
        objects.unregister(8, Some(9))?;
        assert_eq!(objects.phantoms[&8].overlaps.listeners, vec![None, Some(9)]);
        assert!(objects.register(8, Some(10)).is_err());
        assert!(objects.unregister(7, None).is_err());
        assert!(objects.insert_pickup_listener(0).is_err());
        Ok(())
    }
}
