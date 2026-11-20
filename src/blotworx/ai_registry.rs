//! AIBrain intrusive registration and global action broadcast. REL3312b4
//! inserts at the head; REL2b42f8/12582c advance BEFORE enqueueing each event.
//! The enqueue and manager gate are mandatory, not default no-op callbacks.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Links {
    next: Option<usize>,
    previous: Option<usize>,
}
#[derive(Default)]
pub struct Registry {
    head: Option<usize>,
    links: BTreeMap<usize, Links>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BroadcastEvent {
    pub owner: usize,
    pub header: Option<usize>,
    pub action: u32,
}
pub trait Host {
    /// REL1f4438 queues a cloned action event; it does not execute the brain.
    fn enqueue(&mut self, registry: &mut Registry, event: BroadcastEvent) -> Result<()>;
    /// REL1ce1b4 writes the manager enable byte AFTER all events are queued,
    /// before deferred receiver delivery.
    /// REL1ce250 uses it for budgeted manager work, not animation clocks.
    fn set_manager_enabled(&mut self, enabled: bool) -> Result<()>;
}
impl Registry {
    pub fn len(&self) -> usize {
        self.links.len()
    }
    pub fn contains(&self, owner: usize) -> bool {
        self.links.contains_key(&owner)
    }
    pub fn register(&mut self, owner: usize) {
        // Native non-null next pointer prevents registering the same node twice.
        if self.contains(owner) {
            return;
        }
        let next = self.head;
        if let Some(next) = next {
            self.links.get_mut(&next).unwrap().previous = Some(owner);
        }
        self.links.insert(
            owner,
            Links {
                next,
                previous: None,
            },
        );
        self.head = Some(owner);
    }
    pub fn retire(&mut self, owner: usize) {
        let Some(links) = self.links.remove(&owner) else {
            return;
        };
        match links.previous {
            Some(previous) => self.links.get_mut(&previous).unwrap().next = links.next,
            None => self.head = links.next,
        }
        if let Some(next) = links.next {
            self.links.get_mut(&next).unwrap().previous = links.previous;
        }
    }
    pub fn order(&self) -> Result<Vec<usize>> {
        let mut result = Vec::with_capacity(self.len());
        let mut next = self.head;
        while let Some(owner) = next {
            ensure!(result.len() < self.len(), "Cyclic AI brain registration");
            result.push(owner);
            next = self
                .links
                .get(&owner)
                .context("Broken AI brain registration")?
                .next;
        }
        ensure!(
            result.len() == self.len(),
            "Disconnected AI brain registration"
        );
        Ok(result)
    }
    /// REL184a08/184b74: no recipient snapshot; each event uses its recipient's
    /// entity identity, optional last opaque entity argument, and action13/14.
    /// REL278bec/278b48 resolves strings/numbers as entity names, otherwise
    /// reads Lua's pointer identity. It is not an integer conversion.
    pub fn broadcast(
        &mut self,
        action: Action,
        header: Option<usize>,
        host: &mut impl Host,
    ) -> Result<()> {
        let mut next = self.head;
        while let Some(owner) = next {
            // The original iterator has already advanced when enqueue runs.
            // New head registrations therefore don't receive this broadcast.
            next = self
                .links
                .get(&owner)
                .context("AI broadcast's next recipient was retired during delivery")?
                .next;
            host.enqueue(
                self,
                BroadcastEvent {
                    owner,
                    header,
                    action: match action {
                        Action::Pause => 13,
                        Action::Resume => 14,
                    },
                },
            )?;
        }
        host.set_manager_enabled(action == Action::Resume)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Receiver {
        events: Vec<BroadcastEvent>,
        gates: Vec<bool>,
        insert: bool,
        fail: bool,
    }
    impl Host for Receiver {
        fn enqueue(&mut self, registry: &mut Registry, event: BroadcastEvent) -> Result<()> {
            if self.fail {
                anyhow::bail!("Missing native enqueue host");
            }
            self.events.push(event);
            if self.insert {
                registry.register(99);
                registry.retire(event.owner);
                self.insert = false;
            }
            Ok(())
        }
        fn set_manager_enabled(&mut self, enabled: bool) -> Result<()> {
            self.gates.push(enabled);
            Ok(())
        }
    }
    #[test]
    fn broadcasts_reverse_registration_before_gate_and_preserve_header() -> Result<()> {
        let mut registry = Registry::default();
        for owner in [4, 1, 7, 1] {
            registry.register(owner);
        }
        assert_eq!(registry.order()?, [7, 1, 4]);
        assert_eq!(registry.len(), 3);
        let mut host = Receiver::default();
        registry.broadcast(Action::Pause, Some(37), &mut host)?;
        assert_eq!(
            host.events,
            [7, 1, 4].map(|owner| BroadcastEvent {
                owner,
                header: Some(37),
                action: 13
            })
        );
        assert_eq!(host.gates, [false]);
        host.events.clear();
        registry.retire(1);
        registry.broadcast(Action::Resume, None, &mut host)?;
        assert_eq!(
            host.events,
            [7, 4].map(|owner| BroadcastEvent {
                owner,
                header: None,
                action: 14
            })
        );
        assert_eq!(host.gates, [false, true]);
        Ok(())
    }
    #[test]
    fn iterator_advances_before_callback_without_snapshotting_new_heads() -> Result<()> {
        let mut registry = Registry::default();
        registry.register(1);
        registry.register(2);
        let mut host = Receiver {
            insert: true,
            ..Default::default()
        };
        registry.broadcast(Action::Pause, None, &mut host)?;
        assert_eq!(
            host.events
                .iter()
                .map(|event| event.owner)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(registry.order()?, [99, 1]);
        Ok(())
    }
    #[test]
    fn missing_enqueue_cannot_report_success_or_change_manager_gate() {
        let mut registry = Registry::default();
        registry.register(1);
        let mut host = Receiver {
            fail: true,
            ..Default::default()
        };
        assert!(registry.broadcast(Action::Pause, None, &mut host).is_err());
        assert!(host.gates.is_empty());
    }
}
