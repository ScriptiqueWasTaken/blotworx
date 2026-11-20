//! Retail decal scheduling (REL 0x25b8f0). Geometry/material work is supplied
//! by the scene renderer, never silently replaced by an empty callback.
use anyhow::Result;

pub struct Entry<P, M> {
    pub payload: P,
    pub mesh: Option<M>,
    pub remaining: f32,
    pub lifetime: f32,
}
pub trait Renderer<P, M> {
    fn project(&mut self, entry: &Entry<P, M>, children: &mut Children<P, M>) -> Result<Option<M>>;
    /// Original optional completion callback runs before the lifetime reset.
    fn created(&mut self, entry: &mut Entry<P, M>) -> Result<()>;
    fn animate(&mut self, mesh: &mut M, dt: f32) -> Result<()>;
    /// Detach/compact the original scene children as required by the renderer.
    fn expire(&mut self, mesh: &mut M) -> Result<()>;
}
/// Mesh traversal (0x2589bc) copies the current request for independently
/// projected children. These copies retain the expired timer, not a new 0.1s
/// delay. Capacity includes the parent until projection finishes.
pub struct Children<P, M> {
    entries: Vec<Entry<P, M>>,
    available: usize,
    remaining: f32,
    lifetime: f32,
}
impl<P, M> Children<P, M> {
    pub fn enqueue(&mut self, payload: P) -> bool {
        if self.entries.len() >= self.available {
            return false;
        }
        self.entries.push(Entry {
            payload,
            mesh: None,
            remaining: self.remaining,
            lifetime: self.lifetime,
        });
        true
    }
}
pub struct Queue<P, M> {
    pub entries: Vec<Entry<P, M>>,
    initialized: bool,
    initial_remaining: f32,
}
impl<P, M> Queue<P, M> {
    /// Constructor 0x258040 initially creates 128 null request slots. The
    /// first manager update removes them; before it the queue is full. Model
    /// those null entries without allocating undefined native payload fields.
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(128),
            initialized: false,
            initial_remaining: 0.,
        }
    }
    /// Original manager capacity is 128 (0x258144), not an invented budget.
    /// A missing target or full queue is rejected, with no replacement/eviction.
    pub fn enqueue(&mut self, target_payload: Option<P>, lifetime: f32) -> bool {
        if !self.initialized || self.entries.len() >= 128 {
            return false;
        }
        let Some(payload) = target_payload else {
            return false;
        };
        self.entries.push(Entry {
            payload,
            mesh: None,
            remaining: 0.1,
            lifetime,
        });
        true
    }
    /// dt is the original clock+0x44 input, supplied by the owner. Reverse
    /// iteration and swap removal preserve the retail ordering. Only successful
    /// projection consumes the per-update allowance; failed requests disappear.
    pub fn update(
        &mut self,
        dt: f32,
        clear: bool,
        renderer: &mut impl Renderer<P, M>,
    ) -> Result<()> {
        if !self.initialized {
            self.initial_remaining -= dt;
            if self.initial_remaining < 0. || clear {
                self.initialized = true;
            }
            return Ok(());
        }
        let mut projected = false;
        for index in (0..self.entries.len()).rev() {
            let available = 128 - self.entries.len();
            let entry = &mut self.entries[index];
            entry.remaining -= dt;
            if entry.remaining < 0. || clear {
                if entry.mesh.is_none() {
                    if !clear {
                        if projected {
                            continue;
                        }
                        let mut children = Children {
                            entries: Vec::new(),
                            available,
                            remaining: entry.remaining,
                            lifetime: entry.lifetime,
                        };
                        let result = renderer.project(entry, &mut children);
                        self.entries.extend(children.entries);
                        let entry = &mut self.entries[index];
                        entry.mesh = result?;
                        projected = entry.mesh.is_some();
                    }
                    let entry = &mut self.entries[index];
                    if entry.mesh.is_none() {
                        self.entries.swap_remove(index);
                    } else {
                        renderer.created(entry)?;
                        // Re-read lifetime after synchronous callback, like
                        // 0x25bab4..0x25bac0; discard the elapsed overshoot.
                        entry.remaining = entry.lifetime;
                    }
                } else {
                    renderer.expire(entry.mesh.as_mut().unwrap())?;
                    self.entries.swap_remove(index);
                }
            } else if let Some(mesh) = &mut entry.mesh {
                renderer.animate(mesh, dt)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Host {
        calls: Vec<(char, u32)>,
    }
    impl Renderer<u32, u32> for Host {
        fn project(
            &mut self,
            e: &Entry<u32, u32>,
            _: &mut Children<u32, u32>,
        ) -> Result<Option<u32>> {
            self.calls.push(('p', e.payload));
            Ok((e.payload != 3).then_some(e.payload))
        }
        fn created(&mut self, e: &mut Entry<u32, u32>) -> Result<()> {
            assert!(e.remaining < 0.);
            self.calls.push(('c', e.payload));
            e.lifetime = 1.;
            Ok(())
        }
        fn animate(&mut self, m: &mut u32, _: f32) -> Result<()> {
            self.calls.push(('a', *m));
            Ok(())
        }
        fn expire(&mut self, m: &mut u32) -> Result<()> {
            self.calls.push(('e', *m));
            Ok(())
        }
    }
    #[test]
    fn delayed_newest_first_projection_failed_requests_do_not_consume_allowance() -> Result<()> {
        let mut q = Queue::new();
        let mut host = Host::default();
        assert!(!q.enqueue(Some(1), 5.));
        q.update(0., false, &mut host)?;
        assert!(!q.enqueue(Some(1), 5.));
        q.update(0.01, false, &mut host)?;
        for id in [1, 2, 3] {
            assert!(q.enqueue(Some(id), 5.));
        }
        q.update(0.1, false, &mut host)?;
        assert!(host.calls.is_empty()); // Exactly zero is still pending.
        q.update(0.01, false, &mut host)?;
        assert_eq!(host.calls, vec![('p', 3), ('p', 2), ('c', 2)]);
        assert_eq!(q.entries[1].remaining, 1.); // Callback overrides lifetime.
        q.update(0.01, false, &mut host)?;
        assert_eq!(&host.calls[3..], &[('a', 2), ('p', 1), ('c', 1)]);
        host.calls.clear();
        q.update(1., false, &mut host)?;
        assert_eq!(host.calls, vec![('e', 2), ('a', 1)]); // Zero remains visible.
        q.update(0.01, false, &mut host)?;
        assert_eq!(host.calls.last(), Some(&('e', 1)));
        assert!(q.entries.is_empty());
        Ok(())
    }
    #[test]
    fn clear_does_not_project_and_capacity_rejects_without_eviction() -> Result<()> {
        let mut q = Queue::new();
        let mut host = Host::default();
        q.update(0.01, false, &mut host)?;
        assert!(!q.enqueue(None, 5.));
        for id in 0..128 {
            assert!(q.enqueue(Some(id), 5.));
        }
        assert!(!q.enqueue(Some(999), 5.));
        assert_eq!(q.entries.first().unwrap().payload, 0);
        assert_eq!(q.entries.last().unwrap().payload, 127);
        q.update(0., true, &mut host)?;
        assert!(q.entries.is_empty() && host.calls.is_empty());
        assert!(q.enqueue(Some(5), 5.));
        q.update(0.2, false, &mut host)?;
        q.update(0., true, &mut host)?;
        assert_eq!(host.calls, vec![('p', 5), ('c', 5), ('e', 5)]);
        assert!(q.entries.is_empty());
        Ok(())
    }
    #[test]
    fn child_requests_preserve_timer_and_wait_until_next_update() -> Result<()> {
        struct Split {
            seen: Vec<u32>,
        }
        impl Renderer<u32, u32> for Split {
            fn project(
                &mut self,
                e: &Entry<u32, u32>,
                children: &mut Children<u32, u32>,
            ) -> Result<Option<u32>> {
                self.seen.push(e.payload);
                if e.payload == 1 {
                    assert!(children.enqueue(2));
                    assert!(children.enqueue(3));
                    Ok(None)
                } else {
                    Ok(Some(e.payload))
                }
            }
            fn created(&mut self, _: &mut Entry<u32, u32>) -> Result<()> {
                Ok(())
            }
            fn animate(&mut self, _: &mut u32, _: f32) -> Result<()> {
                Ok(())
            }
            fn expire(&mut self, _: &mut u32) -> Result<()> {
                Ok(())
            }
        }
        let mut q = Queue::new();
        let mut host = Split { seen: vec![] };
        q.update(0.01, false, &mut host)?;
        q.enqueue(Some(1), 7.);
        q.update(0.2, false, &mut host)?;
        assert_eq!(host.seen, vec![1]);
        // Removing the parent swaps the last appended child into its slot.
        assert_eq!(
            q.entries
                .iter()
                .map(|e| (e.payload, e.remaining, e.lifetime))
                .collect::<Vec<_>>(),
            vec![(3, -0.1, 7.), (2, -0.1, 7.)]
        );
        q.update(0., false, &mut host)?;
        assert_eq!(host.seen, vec![1, 2]);
        q.update(0., false, &mut host)?;
        assert_eq!(host.seen, vec![1, 2, 3]);
        Ok(())
    }
    #[test]
    fn removing_middle_entry_does_not_advance_swapped_tail_twice() -> Result<()> {
        let mut q = Queue::new();
        let mut host = Host::default();
        q.update(0.01, false, &mut host)?;
        for (id, remaining) in [(10, 1.), (11, 0.), (12, 2.)] {
            q.entries.push(Entry {
                payload: id,
                mesh: Some(id),
                remaining,
                lifetime: 5.,
            });
        }
        q.update(0.25, false, &mut host)?;
        assert_eq!(host.calls, vec![('a', 12), ('e', 11), ('a', 10)]);
        assert_eq!(
            q.entries
                .iter()
                .map(|e| (e.payload, e.remaining))
                .collect::<Vec<_>>(),
            vec![(10, 0.75), (12, 1.75)]
        );
        Ok(())
    }
}
