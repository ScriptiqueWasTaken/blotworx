//! Primary pending-operation ordering, DOL 0x802d25c8..0x802d30d0.
//! Island-merge preparation and world lock admission are separate. Operations
//! own their body handles so enqueue retains them until dispatch completes.
use anyhow::{ensure, Result};
use std::collections::VecDeque;

/// Original world fields +0x94 and +0x9c. This is separate from the general
/// world lock/count at +0xbc. Initialization comes from the world owner.
pub struct DrainGate {
    pub critical_locks: u32,
    pub blocked: bool,
}
pub trait PendingExecution {
    fn primary_pending(&self) -> bool;
    fn body_pending(&self) -> bool;
    fn execution_depth(&self) -> u32;
    /// Includes original 0x802e06f0's pending-count reset and primary drain.
    fn execute_primary(&mut self) -> Result<()>;
    /// Includes original 0x802e0700's body-count reset and body operation pass.
    fn execute_bodies(&mut self) -> Result<()>;
}
impl DrainGate {
    /// DOL 0x802b7aa0..0x802b7aec. Call at an actual critical unlock boundary,
    /// not every renderer update or general world-unlock operation.
    pub fn unlock(&mut self, backend: &mut impl PendingExecution) -> Result<()> {
        ensure!(
            self.critical_locks != 0,
            "Unbalanced physics critical unlock"
        );
        self.critical_locks -= 1;
        self.attempt(backend)
    }
    pub fn attempt(&self, backend: &mut impl PendingExecution) -> Result<()> {
        if self.critical_locks != 0 || self.blocked {
            return Ok(());
        }
        if backend.primary_pending() {
            backend.execute_primary()?;
        }
        // Read after primary execution; it can change depth/body pending state.
        if backend.execution_depth() == 1 && backend.body_pending() {
            backend.execute_bodies()?;
        }
        Ok(())
    }
}

pub struct Queue<T> {
    pending: VecDeque<T>,
    batches: Vec<VecDeque<T>>,
    failed: Option<T>,
    draining: bool,
}
impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            batches: vec![],
            failed: None,
            draining: false,
        }
    }
}
impl<T> Queue<T> {
    pub fn enqueue(&mut self, operation: T) -> Result<()> {
        ensure!(
            self.failed.is_none(),
            "Physics operation queue aborted after a dispatch failure"
        );
        self.pending.push_back(operation);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.pending.len()
            + self.batches.iter().map(VecDeque::len).sum::<usize>()
            + usize::from(self.failed.is_some())
    }
    pub fn failed_operation(&self) -> Option<&T> {
        self.failed.as_ref()
    }
    /// Drain one admitted boundary. The original detaches the current batch,
    /// walks it forward and recursively drains new operations after each item
    /// (0x802d2fc8). An explicit stack preserves that order without a host
    /// recursion limit. No coalescing or operation-count cap discards requests.
    /// A backend error poisons the queue: pending/failed operations stay owned
    /// for diagnosis, and partially executed operations cannot be replayed.
    pub fn drain(&mut self, mut dispatch: impl FnMut(&mut Self, &T) -> Result<()>) -> Result<()> {
        ensure!(!self.draining, "Physics queue drain is already active");
        ensure!(
            self.failed.is_none(),
            "Physics operation queue cannot retry a partial failure"
        );
        self.draining = true;
        self.batches.push(std::mem::take(&mut self.pending));
        while !self.batches.is_empty() {
            let Some(operation) = self.batches.last_mut().unwrap().pop_front() else {
                self.batches.pop();
                continue;
            };
            if let Err(error) = dispatch(self, &operation) {
                self.failed = Some(operation);
                self.draining = false;
                return Err(error);
            }
            // Original type-4 dispatch releases the retained body before
            // entering the newly queued child batch (0x802d281c..0x802d2824).
            drop(operation);
            if !self.pending.is_empty() {
                self.batches.push(std::mem::take(&mut self.pending));
            }
        }
        self.draining = false;
        Ok(())
    }
}

/// Type 4 payload retains byte values exactly, including the activation and
/// collision-filter modes. World admission/dispatch must interpret them.
#[derive(Debug)]
pub struct MotionRequest<B> {
    pub body: B,
    pub motion: u8,
    pub activation: u8,
    pub filter_update: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Backend {
        queue: Queue<u8>,
        body_pending: bool,
        depth: u32,
        events: Vec<u8>,
        depth_after_primary: u32,
    }
    impl PendingExecution for Backend {
        fn primary_pending(&self) -> bool {
            self.queue.len() != 0
        }
        fn body_pending(&self) -> bool {
            self.body_pending
        }
        fn execution_depth(&self) -> u32 {
            self.depth
        }
        fn execute_primary(&mut self) -> Result<()> {
            let events = &mut self.events;
            self.queue.drain(|q, &op| {
                events.push(op);
                if op == 1 {
                    q.enqueue(3)?;
                }
                Ok(())
            })?;
            self.depth = self.depth_after_primary;
            Ok(())
        }
        fn execute_bodies(&mut self) -> Result<()> {
            self.body_pending = false;
            self.events.push(4);
            Ok(())
        }
    }
    #[test]
    fn nested_locks_and_blocking_preserve_operations_until_admitted() {
        let mut backend = Backend {
            queue: Queue::default(),
            body_pending: true,
            depth: 1,
            events: vec![],
            depth_after_primary: 1,
        };
        backend.queue.enqueue(1).unwrap();
        backend.queue.enqueue(2).unwrap();
        let mut gate = DrainGate {
            critical_locks: 2,
            blocked: false,
        };
        gate.unlock(&mut backend).unwrap();
        assert!(backend.events.is_empty());
        gate.blocked = true;
        gate.unlock(&mut backend).unwrap();
        assert!(backend.events.is_empty());
        gate.blocked = false;
        gate.attempt(&mut backend).unwrap();
        assert_eq!(backend.events, [1, 3, 2, 4]);
        assert_eq!(backend.queue.len(), 0);
        assert!(gate.unlock(&mut backend).is_err());
        assert_eq!(gate.critical_locks, 0);
    }
    #[test]
    fn body_phase_uses_post_primary_depth() {
        let mut backend = Backend {
            queue: Queue::default(),
            body_pending: true,
            depth: 1,
            events: vec![],
            depth_after_primary: 2,
        };
        backend.queue.enqueue(2).unwrap();
        let gate = DrainGate {
            critical_locks: 0,
            blocked: false,
        };
        gate.attempt(&mut backend).unwrap();
        assert_eq!(backend.events, [2]);
        assert!(backend.body_pending);
        backend.depth = 1;
        gate.attempt(&mut backend).unwrap();
        assert_eq!(backend.events, [2, 4]);
    }
    #[test]
    fn child_batches_precede_siblings_without_coalescing() {
        let mut queue = Queue::default();
        queue.enqueue(1).unwrap();
        queue.enqueue(2).unwrap();
        let mut observed = vec![];
        queue
            .drain(|q, &operation| {
                observed.push(operation);
                match operation {
                    1 => {
                        q.enqueue(3)?;
                        q.enqueue(4)?;
                    }
                    3 => {
                        q.enqueue(5)?;
                        q.enqueue(5)?;
                    }
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(observed, [1, 3, 5, 5, 4, 2]);
        assert_eq!(queue.len(), 0);
    }
    #[test]
    fn retains_handle_until_dispatch_then_releases_before_children() {
        let body = std::sync::Arc::new(7);
        let mut queue = Queue::default();
        queue
            .enqueue(MotionRequest {
                body: body.clone(),
                motion: 4,
                activation: 1,
                filter_update: 0,
            })
            .unwrap();
        assert_eq!(std::sync::Arc::strong_count(&body), 2);
        queue
            .drain(|q, request| {
                assert_eq!(request.activation, 1);
                assert_eq!(request.filter_update, 0);
                assert_eq!(std::sync::Arc::strong_count(&body), 2);
                if request.motion == 4 {
                    q.enqueue(MotionRequest {
                        body: body.clone(),
                        motion: 1,
                        activation: 1,
                        filter_update: 0,
                    })?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(std::sync::Arc::strong_count(&body), 1);
    }
    #[test]
    fn failure_preserves_pending_work_without_replaying_side_effects() {
        let mut queue = Queue::default();
        queue.enqueue(1).unwrap();
        queue.enqueue(2).unwrap();
        let mut calls = 0;
        assert!(queue
            .drain(|q, &operation| {
                calls += 1;
                q.enqueue(3)?;
                anyhow::bail!("operation {operation} partially executed")
            })
            .is_err());
        assert_eq!(calls, 1);
        assert_eq!(queue.failed_operation(), Some(&1));
        assert_eq!(queue.len(), 3);
        assert!(queue.drain(|_, _| panic!("Must not retry")).is_err());
        assert!(queue.enqueue(4).is_err());
        assert_eq!(queue.len(), 3);
    }
}
