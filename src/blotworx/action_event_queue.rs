//! REL1f4438/1f45ec's shared deferred queue, with typed event payloads.
//! This does not replace the subscription registry or concrete receivers.
use anyhow::{ensure, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ActionEvent {
    pub source: Option<usize>,
    /// Base event word8: an opaque entity identity, not an integer option.
    pub header: Option<usize>,
    pub action: u32,
    /// Preserve the four raw payload lanes at20..2c and reference at30.
    /// Native equality deliberately does not compare these fields.
    pub vector_bits: [u32; 4],
    pub reference: Option<usize>,
}
impl From<super::ai_registry::BroadcastEvent> for ActionEvent {
    fn from(event: super::ai_registry::BroadcastEvent) -> Self {
        Self {
            source: Some(event.owner),
            header: event.header,
            action: event.action,
            vector_bits: [0; 4],
            reference: None,
        }
    }
}
impl ActionEvent {
    /// Virtual10, REL129c34: kind0c, source, header, then action. Queue
    /// listener identity and additional action payload are not compared.
    pub(super) fn equivalent(&self, other: &Self) -> bool {
        self.source == other.source && self.header == other.header && self.action == other.action
    }
}
pub trait QueuedEvent: Clone {
    fn kind(&self) -> u16;
    fn equivalent_with(
        &self,
        other: &Self,
        reference: &mut dyn FnMut(usize, usize) -> Result<bool>,
    ) -> Result<bool>;
}
impl QueuedEvent for ActionEvent {
    fn kind(&self) -> u16 {
        0x0c
    }
    fn equivalent_with(
        &self,
        other: &Self,
        _: &mut dyn FnMut(usize, usize) -> Result<bool>,
    ) -> Result<bool> {
        Ok(self.equivalent(other))
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Entry<E = ActionEvent> {
    pub event: E,
    pub listener: Option<usize>,
}
/// Global AI callbacks enqueue before changing the independent manager gate.
/// None records that the manager constructor's initial byte is not yet ported.
pub struct AiBroadcast<'a, E = ActionEvent> {
    pub queue: &'a mut Queue<E>,
    pub manager_enabled: &'a mut Option<bool>,
}
impl<E: QueuedEvent + From<ActionEvent>> super::ai_registry::Host for AiBroadcast<'_, E> {
    fn enqueue(
        &mut self,
        _: &mut super::ai_registry::Registry,
        event: super::ai_registry::BroadcastEvent,
    ) -> Result<()> {
        self.queue
            .enqueue(&E::from(ActionEvent::from(event)), None)?;
        Ok(())
    }
    fn set_manager_enabled(&mut self, enabled: bool) -> Result<()> {
        *self.manager_enabled = Some(enabled);
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Queued,
    DuplicateWhileProcessing,
}
pub trait Host<E = ActionEvent> {
    /// REL1f4638..4650: targeted delivery first checks that the listener still
    /// has a registration for the entry's kind. Broadcast entries bypass it.
    fn listener_registered(&mut self, listener: usize, kind: u16) -> Result<bool>;
    /// Actual entity-event-manager delivery, including native subscriptions,
    /// exclusion, reentrancy and concrete receiver callbacks. Mandatory.
    fn deliver(&mut self, queue: &mut Queue<E>, entry: &Entry<E>) -> Result<()>;
}
pub struct Queue<E = ActionEvent> {
    entries: Vec<Entry<E>>,
    processing: bool,
    // On an unsupported callback, retain the failed entry and unprocessed
    // tail. Earlier deliveries must not be repeated when integration resumes.
    cursor: usize,
}
impl<E> Default for Queue<E> {
    fn default() -> Self {
        Self {
            entries: vec![],
            processing: false,
            cursor: 0,
        }
    }
}
impl<E: QueuedEvent> Queue<E> {
    pub fn entries(&self) -> &[Entry<E>] {
        &self.entries
    }
    pub fn pending(&self) -> usize {
        self.entries.len() - self.cursor
    }
    pub fn enqueue(&mut self, event: &E, listener: Option<usize>) -> Result<Admission> {
        self.enqueue_with(event, listener, &mut |_, _| {
            anyhow::bail!("Native referenced-event equality is not connected")
        })
    }
    pub fn enqueue_with(
        &mut self,
        event: &E,
        listener: Option<usize>,
        reference: &mut dyn FnMut(usize, usize) -> Result<bool>,
    ) -> Result<Admission> {
        // Only processing mode deduplicates, and it scans the entire retained
        // array, including entries delivered earlier in this pass. REL1f4464.
        if self.processing {
            for entry in &self.entries {
                if entry.event.equivalent_with(event, reference)? {
                    return Ok(Admission::DuplicateWhileProcessing);
                }
            }
        }
        // Retail stores size in16 bits. Reject overflow explicitly rather
        // than reproducing its wraparound/memory corruption or dropping data.
        ensure!(
            self.entries.len() < u16::MAX as usize,
            "Native action-event queue count would overflow"
        );
        self.entries.push(Entry {
            event: event.clone(),
            listener,
        });
        Ok(Admission::Queued)
    }
    pub fn process(&mut self, host: &mut impl Host<E>) -> Result<()> {
        ensure!(
            !self.processing,
            "Reentrant queue processing requires native reference validation"
        );
        self.processing = true;
        let result = (|| {
            // Native reevaluates count each iteration: appended distinct
            // events are delivered within this same pass, not next frame.
            while self.cursor < self.entries.len() {
                let entry = self.entries[self.cursor].clone();
                if match entry.listener {
                    Some(listener) => host.listener_registered(listener, entry.event.kind())?,
                    None => true,
                } {
                    host.deliver(self, &entry)?;
                }
                self.cursor += 1;
            }
            Ok(())
        })();
        self.processing = false;
        if result.is_ok() {
            self.entries.clear();
            self.cursor = 0;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(action: u32) -> ActionEvent {
        ActionEvent {
            source: Some(4),
            header: Some(7),
            action,
            vector_bits: [0; 4],
            reference: None,
        }
    }
    #[test]
    fn global_pause_enqueues_in_registry_order_before_manager_gate_without_receiver_execution(
    ) -> Result<()> {
        let mut registry = super::super::ai_registry::Registry::default();
        for owner in [3, 8, 2] {
            registry.register(owner);
        }
        let mut queue: Queue = Queue::default();
        let mut gate = None;
        registry.broadcast(
            super::super::ai_registry::Action::Pause,
            Some(500),
            &mut AiBroadcast {
                queue: &mut queue,
                manager_enabled: &mut gate,
            },
        )?;
        assert_eq!(gate, Some(false));
        assert_eq!(
            queue
                .entries()
                .iter()
                .map(|entry| entry.event.source)
                .collect::<Vec<_>>(),
            [Some(2), Some(8), Some(3)]
        );
        assert!(queue.entries().iter().all(|entry| entry.event.action == 13
            && entry.event.header == Some(500)
            && entry.listener.is_none()));
        // Outside processing, a second identical call must retain all events.
        registry.broadcast(
            super::super::ai_registry::Action::Pause,
            Some(500),
            &mut AiBroadcast {
                queue: &mut queue,
                manager_enabled: &mut gate,
            },
        )?;
        assert_eq!(queue.pending(), 6);
        registry.broadcast(
            super::super::ai_registry::Action::Resume,
            None,
            &mut AiBroadcast {
                queue: &mut queue,
                manager_enabled: &mut gate,
            },
        )?;
        assert_eq!(gate, Some(true));
        assert_eq!(queue.pending(), 9);
        assert!(queue.entries()[6..]
            .iter()
            .all(|entry| entry.event.action == 14 && entry.event.header.is_none()));
        Ok(())
    }
    #[test]
    fn queue_clones_and_does_not_deduplicate_before_processing() -> Result<()> {
        let mut queue = Queue::default();
        let mut original = event(13);
        assert_eq!(queue.enqueue(&original, None)?, Admission::Queued);
        original.vector_bits[0] = 0x3f800000;
        assert_eq!(queue.enqueue(&original, Some(99))?, Admission::Queued);
        original.action = 14;
        assert_eq!(queue.entries()[0].event.action, 13);
        assert_eq!(queue.entries()[0].event.vector_bits, [0; 4]);
        assert_eq!(queue.entries()[1].event.vector_bits[0], 0x3f800000);
        assert_eq!(queue.pending(), 2);
        Ok(())
    }
    #[test]
    fn processing_compares_delivered_entries_ignores_payload_and_delivers_appends() -> Result<()> {
        struct Receiver {
            seen: Vec<u32>,
        }
        impl Host for Receiver {
            fn listener_registered(&mut self, _: usize, _: u16) -> Result<bool> {
                anyhow::bail!("Unexpected targeted lookup")
            }
            fn deliver(&mut self, queue: &mut Queue, entry: &Entry) -> Result<()> {
                self.seen.push(entry.event.action);
                if entry.event.action == 14 {
                    let mut duplicate = event(13);
                    duplicate.vector_bits = [u32::MAX; 4];
                    duplicate.reference = Some(100);
                    assert_eq!(
                        queue.enqueue(&duplicate, Some(900))?,
                        Admission::DuplicateWhileProcessing
                    );
                    assert_eq!(queue.enqueue(&event(11), None)?, Admission::Queued);
                }
                Ok(())
            }
        }
        let mut queue = Queue::default();
        queue.enqueue(&event(13), None)?;
        queue.enqueue(&event(14), None)?;
        let mut host = Receiver { seen: vec![] };
        queue.process(&mut host)?;
        assert_eq!(host.seen, [13, 14, 11]);
        assert_eq!(queue.pending(), 0);
        // Processing mode ended, so a matching new event is admitted again.
        assert_eq!(queue.enqueue(&event(13), None)?, Admission::Queued);
        Ok(())
    }
    #[test]
    fn unsupported_delivery_retains_tail_and_targeted_retirement_is_checked() -> Result<()> {
        struct Receiver {
            seen: Vec<u32>,
            fail: bool,
            checks: Vec<usize>,
        }
        impl Host for Receiver {
            fn listener_registered(&mut self, id: usize, kind: u16) -> Result<bool> {
                assert_eq!(kind, 0x0c);
                self.checks.push(id);
                Ok(id != 99)
            }
            fn deliver(&mut self, _: &mut Queue, entry: &Entry) -> Result<()> {
                if self.fail && entry.event.action == 14 {
                    anyhow::bail!("Missing required receiver")
                }
                self.seen.push(entry.event.action);
                Ok(())
            }
        }
        let mut queue = Queue::default();
        queue.enqueue(&event(13), None)?;
        queue.enqueue(&event(12), Some(99))?;
        queue.enqueue(&event(14), Some(100))?;
        let mut host = Receiver {
            seen: vec![],
            fail: true,
            checks: vec![],
        };
        assert!(queue.process(&mut host).is_err());
        assert_eq!(queue.pending(), 1);
        assert_eq!(host.seen, [13]);
        assert_eq!(host.checks, [99, 100]);
        host.fail = false;
        queue.process(&mut host)?;
        assert_eq!(host.seen, [13, 14]);
        assert_eq!(queue.pending(), 0);
        Ok(())
    }
}
