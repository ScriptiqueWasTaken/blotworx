//! Ordinary event tables sharing REL1f4110's entity event dispatcher/stack.
//! Keys are the native32-bit listener identities, not asset/component IDs.
//! World listener construction and concrete receivers remain separate.
use super::{
    action_event_queue::{ActionEvent, Entry, Queue, QueuedEvent},
    game_events::Registration,
};
use anyhow::{ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};

pub trait DispatchEvent: QueuedEvent {
    fn source(&self) -> Option<usize>;
}
impl DispatchEvent for ActionEvent {
    fn source(&self) -> Option<usize> {
        self.source
    }
}
impl DispatchEvent for super::entity_events::Event {
    fn source(&self) -> Option<usize> {
        match self {
            Self::Action(event) => event.source,
            Self::CounterStatus(event) => event.source,
            Self::Damage(event) => event.source,
            Self::ZeroHealth(event) => event.source,
            Self::Basic(event) => event.source,
            Self::Animation(event) => event.source,
            Self::Variable(event) => event.source,
            Self::BehaviorFinished(event) => event.source,
        }
    }
}

struct Node {
    kind: u16,
    key: u32,
    next: Option<usize>,
    registration: Registration,
}
pub struct EventRegistry<E: DispatchEvent> {
    buckets: BTreeMap<u16, Vec<Option<usize>>>,
    nodes: Vec<Node>,
    epoch: u32,
    excluded: BTreeSet<u32>,
    active: Vec<E>,
}
pub type Registry = EventRegistry<ActionEvent>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Delivered,
    Suppressed,
}
pub trait Host<E: DispatchEvent = ActionEvent> {
    /// Concrete listener callback. May register/remove listeners, enqueue
    /// events, or dispatch nested events. Unsupported callbacks must error.
    fn receive(&mut self, registry: &mut EventRegistry<E>, listener: u32, event: &E) -> Result<()>;
    /// Native virtual10 comparison is required for payloads carrying references.
    /// Action equality never calls this hook. Missing comparison is an error.
    fn reference_equal(&mut self, _left: usize, _right: usize) -> Result<bool> {
        anyhow::bail!("Native event reference comparison is not connected")
    }
}
impl<E: DispatchEvent> Default for EventRegistry<E> {
    fn default() -> Self {
        // Constructor1f1d94 fills ordinary tables with4e5dbc=20. Only kind1
        // is overridden to10 at1f1dac; kind0c retains20 buckets.
        Self::with_buckets(20).unwrap()
    }
}
impl<E: DispatchEvent> EventRegistry<E> {
    /// Explicit native/captured action-table width. Other ordinary tables use
    /// constructor1f1d94's20 buckets, except kind1's verified10 override.
    pub fn with_buckets(count: u32) -> Result<Self> {
        ensure!(count > 0, "Native listener table has no buckets");
        Ok(Self {
            buckets: BTreeMap::from([(0x0c, vec![None; count as usize])]),
            nodes: vec![],
            epoch: 0,
            excluded: BTreeSet::new(),
            active: vec![],
        })
    }
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    fn bucket(&self, kind: u16, key: u32) -> usize {
        key as usize % self.buckets[&kind].len()
    }
    fn find(&self, kind: u16, key: u32) -> Option<usize> {
        let buckets = self.buckets.get(&kind)?;
        let mut next = buckets[self.bucket(kind, key)];
        while let Some(index) = next {
            let node = &self.nodes[index];
            if node.key == key {
                return Some(index);
            }
            next = node.next;
        }
        None
    }
    /// REL1f30e4..3248 inserts new keys at bucket heads, preserving existing
    /// node positions when a registration is extended or reactivated.
    pub fn register(&mut self, key: u32, source: Option<usize>) -> Result<()> {
        self.register_for(0x0c, key, source)
    }
    pub fn register_for(&mut self, kind: u16, key: u32, source: Option<usize>) -> Result<()> {
        ensure!(
            kind & 0x8000 == 0,
            "Native external event-bank initialization is not connected"
        );
        ensure!(key != 0, "Null native listener identity");
        let source = source_token(source)?;
        if self.excluded.contains(&key) {
            anyhow::bail!(
                "Registering an excluded listener requires native deferred-removal cleanup"
            );
        }
        self.buckets
            .entry(kind)
            .or_insert_with(|| vec![None; if kind == 1 { 10 } else { 20 }]);
        let index = match self.find(kind, key) {
            Some(index) => index,
            None => {
                let bucket = self.bucket(kind, key);
                let index = self.nodes.len();
                self.nodes.push(Node {
                    kind,
                    key,
                    next: self.buckets[&kind][bucket],
                    registration: Registration::default(),
                });
                self.buckets.get_mut(&kind).unwrap()[bucket] = Some(index);
                index
            }
        };
        self.nodes[index]
            .registration
            .register(source, &mut self.epoch);
        Ok(())
    }
    /// Marks a registration inactive while preserving its table node. Native
    /// map-node reclamation is deferred, not performed inside callback delivery.
    pub fn deactivate(&mut self, key: u32) {
        self.deactivate_for(0x0c, key)
    }
    pub fn deactivate_for(&mut self, kind: u16, key: u32) {
        if let Some(index) = self.find(kind, key) {
            self.nodes[index].registration.deactivate();
        }
    }
    pub fn exclude(&mut self, key: u32) {
        self.excluded.insert(key);
    }
    pub fn clear_exclusion(&mut self, key: u32) {
        self.excluded.remove(&key);
    }
    /// Queue's targeted lookup1f4bf8 checks node existence, not its enable bit.
    pub fn registered_node(&self, key: u32) -> bool {
        self.registered_node_for(0x0c, key)
    }
    pub fn registered_node_for(&self, kind: u16, key: u32) -> bool {
        self.find(kind, key).is_some()
    }
    /// Registration query1f34b8 is distinct from event-zero delivery.
    pub fn contains(&self, key: u32, source: Option<usize>) -> Result<bool> {
        self.contains_for(0x0c, key, source)
    }
    pub fn contains_for(&self, kind: u16, key: u32, source: Option<usize>) -> Result<bool> {
        let source = source_token(source)?;
        Ok(self.find(kind, key).is_some_and(|index| {
            self.nodes[index]
                .registration
                .contains(source, self.excluded.contains(&key))
        }))
    }
    fn first(&self, kind: u16) -> Option<usize> {
        self.buckets.get(&kind)?.iter().find_map(|value| *value)
    }
    fn next(&self, index: usize) -> Option<usize> {
        let node = &self.nodes[index];
        node.next.or_else(|| {
            self.buckets[&node.kind][self.bucket(node.kind, node.key) + 1..]
                .iter()
                .find_map(|value| *value)
        })
    }
    /// Iteration identity/order for compact reference evidence. Runtime callers
    /// must supply real listener keys; component IDs are not interchangeable.
    pub fn order(&self) -> Vec<u32> {
        self.order_for(0x0c)
    }
    pub(super) fn has_nodes_for(&self,kind:u16)->bool {self.first(kind).is_some()}
    pub fn order_for(&self, kind: u16) -> Vec<u32> {
        let mut result = vec![];
        let mut next = self.first(kind);
        while let Some(index) = next {
            result.push(self.nodes[index].key);
            next = self.next(index);
        }
        result
    }
    fn admitted(&self, index: usize, epoch: u32) -> bool {
        let subscription = self.nodes[index].registration.subscription();
        subscription.enabled
            && subscription.activation_epoch <= epoch
            && !self.excluded.contains(&self.nodes[index].key)
    }
    fn filters(
        &mut self,
        index: usize,
        event: &E,
        host: &mut impl Host<E>,
        targeted: bool,
    ) -> Result<()> {
        let source = source_token(event.source())?;
        let mut at = 0;
        loop {
            // Do not snapshot: callback additions/removals change the live
            // filter count inspected by1f43c0/1f424c on the next iteration.
            let Some(&filter) = self.nodes[index]
                .registration
                .subscription()
                .entity_filters
                .get(at)
            else {
                break;
            };
            if filter == 0 || source == 0 || filter == source {
                let key = self.nodes[index].key;
                if targeted {
                    self.active.push(event.clone());
                }
                let result = host.receive(self, key, event);
                if targeted {
                    self.active.pop();
                }
                result?;
            }
            at += 1;
        }
        Ok(())
    }
    pub fn dispatch(
        &mut self,
        event: &E,
        listener: Option<u32>,
        host: &mut impl Host<E>,
    ) -> Result<Admission> {
        ensure!(
            event.kind() & 0x8000 == 0,
            "Native external event-bank dispatch is not connected"
        );
        // The native stack has exactly10 entries. This is a recovered engine
        // recursion rule, not an implementation-imposed resource cap.
        if self.active.len() == 10 {
            return Ok(Admission::Suppressed);
        }
        for active in &self.active {
            if active
                .equivalent_with(event, &mut |left, right| host.reference_equal(left, right))?
            {
                return Ok(Admission::Suppressed);
            }
        }
        let epoch = self.epoch;
        if let Some(key) = listener {
            if let Some(index) = self
                .find(event.kind(), key)
                .filter(|&index| self.admitted(index, epoch))
            {
                self.filters(index, event, host, true)?;
            }
        } else {
            self.active.push(event.clone());
            let result = (|| {
                let mut next = self.first(event.kind());
                while let Some(index) = next {
                    // Native iterator advances before callbacks. No recipient
                    // snapshot, and newly activated nodes fail the epoch gate.
                    next = self.next(index);
                    if self.admitted(index, epoch) {
                        self.filters(index, event, host, false)?;
                    }
                }
                Ok::<_, anyhow::Error>(())
            })();
            self.active.pop();
            result?;
        }
        Ok(Admission::Delivered)
    }
}
/// World entities use zero-based indices; retail null is a different identity.
/// Preserve that distinction consistently with Lua's non-null entity tokens.
fn source_token(source: Option<usize>) -> Result<u32> {
    source
        .map(|source| {
            u32::try_from(source.checked_add(1).context("Entity token overflow")?)
                .context("Event source requires a native32-bit identity")
        })
        .transpose()
        .map(|value| value.unwrap_or(0))
}

/// Joins the retained queue to actual subscription delivery. A concrete Host
/// remains mandatory; this adapter cannot turn missing receivers into success.
pub struct QueuedDelivery<'a, H> {
    pub registry: &'a mut Registry,
    pub host: &'a mut H,
}
impl<H: Host> super::action_event_queue::Host for QueuedDelivery<'_, H> {
    fn listener_registered(&mut self, listener: usize, kind: u16) -> Result<bool> {
        ensure!(
            kind == 0x0c,
            "Action registry cannot select another native event table"
        );
        Ok(self.registry.registered_node(u32::try_from(listener)?))
    }
    fn deliver(&mut self, _: &mut Queue, entry: &Entry) -> Result<()> {
        self.registry.dispatch(
            &entry.event,
            entry.listener.map(u32::try_from).transpose()?,
            self.host,
        )?;
        Ok(())
    }
}
pub struct MixedQueuedDelivery<'a, H> {
    pub registry: &'a mut EventRegistry<super::entity_events::Event>,
    pub host: &'a mut H,
}
impl<H: Host<super::entity_events::Event>>
    super::action_event_queue::Host<super::entity_events::Event> for MixedQueuedDelivery<'_, H>
{
    fn listener_registered(&mut self, listener: usize, kind: u16) -> Result<bool> {
        Ok(self
            .registry
            .registered_node_for(kind, u32::try_from(listener)?))
    }
    fn deliver(
        &mut self,
        _: &mut super::entity_events::Queue,
        entry: &Entry<super::entity_events::Event>,
    ) -> Result<()> {
        self.registry.dispatch(
            &entry.event,
            entry.listener.map(u32::try_from).transpose()?,
            self.host,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::entity_events::{CounterStatus, Damage, Event, ZeroHealth};
    use super::*;
    fn status_event() -> Event {
        CounterStatus {
            source: Some(3),
            header: None,
            previous: 0,
            current: 1,
            auxiliary: 0,
            reference: None,
        }
        .into()
    }
    #[test]
    fn mixed_tables_route_same_listener_keys_by_kind_and_queue_targets_use_the_correct_table(
    ) -> Result<()> {
        struct Receiver(Vec<(u16, u32)>);
        impl Host<Event> for Receiver {
            fn receive(
                &mut self,
                _: &mut EventRegistry<Event>,
                key: u32,
                event: &Event,
            ) -> Result<()> {
                self.0.push((event.kind(), key));
                Ok(())
            }
        }
        let mut registry = EventRegistry::<Event>::default();
        registry.register_for(0x0c, 5, None)?;
        registry.register_for(0x75, 5, Some(3))?;
        registry.register_for(0x75, 25, Some(8))?;
        let mut receiver = Receiver(vec![]);
        registry.dispatch(&status_event(), None, &mut receiver)?;
        assert_eq!(receiver.0, [(0x75, 5)]);
        assert_eq!(registry.order_for(0x75), [25, 5]); // Same bucket; newest first.
        let mut queue = super::super::entity_events::Queue::default();
        queue.enqueue(&status_event(), Some(5))?;
        queue.enqueue(&status_event(), Some(99))?;
        queue.process(&mut MixedQueuedDelivery {
            registry: &mut registry,
            host: &mut receiver,
        })?;
        assert_eq!(receiver.0, [(0x75, 5), (0x75, 5)]);
        registry.deactivate_for(0x75, 5);
        assert!(registry.registered_node_for(0x75, 5));
        registry.dispatch(&status_event(), Some(5), &mut receiver)?;
        assert_eq!(receiver.0.len(), 2);
        assert!(registry.contains_for(0x0c, 5, None)?);
        Ok(())
    }
    #[test]
    fn cross_kind_dispatch_shares_native_depth_and_reference_equality() -> Result<()> {
        struct Receiver {
            seen: Vec<u16>,
            comparisons: usize,
            suppressed: usize,
        }
        impl Host<Event> for Receiver {
            fn reference_equal(&mut self, left: usize, right: usize) -> Result<bool> {
                self.comparisons += 1;
                Ok(left == right)
            }
            fn receive(
                &mut self,
                r: &mut EventRegistry<Event>,
                _: u32,
                event: &Event,
            ) -> Result<()> {
                self.seen.push(event.kind());
                match event {
                    Event::Action(_) => {
                        r.dispatch(&status_event(), None, self)?;
                    }
                    Event::CounterStatus(_) => {
                        r.dispatch(
                            &Damage {
                                source: Some(3),
                                header: None,
                                amount_bits: 1f32.to_bits(),
                                displacement_bits: 0,
                                reference: 50,
                            }
                            .into(),
                            None,
                            self,
                        )?;
                    }
                    Event::Damage(original) => {
                        let mut duplicate = original.clone();
                        duplicate.amount_bits = 9f32.to_bits();
                        assert_eq!(
                            r.dispatch(&duplicate.into(), None, self)?,
                            Admission::Suppressed
                        );
                        r.dispatch(
                            &ZeroHealth {
                                source: Some(3),
                                header: Some(0),
                            }
                            .into(),
                            None,
                            self,
                        )?;
                    }
                    Event::ZeroHealth(event) => {
                        let next = Event::from(ZeroHealth {
                            source: event.source,
                            header: Some(event.header.unwrap() + 1),
                        });
                        if r.dispatch(&next, None, self)? == Admission::Suppressed {
                            self.suppressed += 1;
                        }
                    }
                    Event::Basic(_) => anyhow::bail!("Test receiver has no basic-event handler"),
                    Event::Animation(_) => {
                        anyhow::bail!("Test receiver has no animation-event handler")
                    }
                    Event::Variable(_) => {
                        anyhow::bail!("Test receiver has no variable-event handler")
                    }
                    Event::BehaviorFinished(_) => anyhow::bail!("Test receiver has no behavior-event handler"),
                }
                Ok(())
            }
        }
        let mut r = EventRegistry::<Event>::default();
        for kind in [0x0c, 0x75, 0x10, 0x35] {
            r.register_for(kind, 5, None)?;
        }
        let mut host = Receiver {
            seen: vec![],
            comparisons: 0,
            suppressed: 0,
        };
        r.dispatch(&event(Some(3), 13).into(), None, &mut host)?;
        assert_eq!(host.seen.len(), 10);
        assert_eq!(&host.seen[..3], &[0x0c, 0x75, 0x10]);
        assert_eq!(host.comparisons, 1);
        assert_eq!(host.suppressed, 1);
        assert!(r.active.is_empty());
        Ok(())
    }
    #[test]
    fn native_table_widths_and_unimplemented_external_bank_are_explicit() -> Result<()> {
        let mut r = EventRegistry::<Event>::default();
        r.register_for(1, 5, None)?;
        r.register_for(0x10, 5, None)?;
        assert_eq!(r.buckets[&1].len(), 10);
        assert_eq!(r.buckets[&0x10].len(), 20);
        let before = r.epoch();
        assert!(r.register_for(0x8048, 5, None).is_err());
        assert_eq!(r.epoch(), before);
        assert!(!r.buckets.contains_key(&0x8048));
        Ok(())
    }
    #[test]
    fn missing_damage_reference_comparison_fails_without_leaking_active_dispatch() -> Result<()> {
        struct Receiver;
        impl Host<Event> for Receiver {
            fn receive(
                &mut self,
                r: &mut EventRegistry<Event>,
                key: u32,
                event: &Event,
            ) -> Result<()> {
                r.dispatch(event, Some(key), self)?;
                Ok(())
            }
        }
        let mut r = EventRegistry::<Event>::default();
        r.register_for(0x10, 5, None)?;
        let damage = Damage {
            source: Some(3),
            header: None,
            amount_bits: 1f32.to_bits(),
            displacement_bits: 0,
            reference: 50,
        }
        .into();
        let error = r.dispatch(&damage, None, &mut Receiver).unwrap_err();
        assert!(error.to_string().contains("reference comparison"));
        assert!(r.active.is_empty());
        Ok(())
    }
    fn event(source: Option<usize>, action: u32) -> ActionEvent {
        ActionEvent {
            source,
            header: None,
            action,
            vector_bits: [0; 4],
            reference: None,
        }
    }
    #[derive(Default)]
    struct Recorder(Vec<(u32, u32)>);
    impl Host for Recorder {
        fn receive(&mut self, _: &mut Registry, key: u32, event: &ActionEvent) -> Result<()> {
            self.0.push((key, event.action));
            Ok(())
        }
    }
    #[test]
    fn buckets_visit_ascending_heads_reverse_collisions_and_filters_keep_native_zero_rules(
    ) -> Result<()> {
        let mut registry = Registry::with_buckets(5)?;
        for key in [6, 2, 11, 5] {
            registry.register(key, Some(8))?;
        }
        registry.register(2, Some(9))?;
        registry.register(2, Some(9))?;
        assert_eq!(registry.order(), [5, 11, 6, 2]);
        assert_eq!(registry.epoch(), 4);
        let mut host = Recorder::default();
        registry.dispatch(&event(None, 13), None, &mut host)?;
        assert_eq!(host.0, [(5, 13), (11, 13), (6, 13), (2, 13), (2, 13)]);
        assert!(!registry.contains(2, None)?);
        assert!(registry.contains(2, Some(9))?);
        host.0.clear();
        registry.dispatch(&event(Some(9), 14), None, &mut host)?;
        assert_eq!(host.0, [(2, 14)]);
        Ok(())
    }
    #[test]
    fn callbacks_advance_first_ignore_new_activations_and_read_live_filter_arrays() -> Result<()> {
        struct Mutate {
            seen: Vec<u32>,
        }
        impl Host for Mutate {
            fn receive(&mut self, r: &mut Registry, key: u32, _: &ActionEvent) -> Result<()> {
                self.seen.push(key);
                if key == 1 {
                    r.register(6, None)?;
                    r.register(4, None)?; // future nodes, new epochs
                    r.deactivate(2); // future existing node is no longer active
                    r.register(1, Some(9))?; // active node's added filter is live now
                }
                Ok(())
            }
        }
        let mut r = Registry::with_buckets(5)?;
        r.register(1, Some(8))?;
        r.register(2, Some(8))?;
        r.register(3, Some(8))?;
        let mut host = Mutate { seen: vec![] };
        r.dispatch(&event(None, 13), None, &mut host)?;
        assert_eq!(host.seen, [1, 1, 3]);
        assert_eq!(r.order(), [6, 1, 2, 3, 4]);
        Ok(())
    }
    #[test]
    fn exclusion_reactivation_epoch_and_callback_errors_do_not_leak_dispatch_depth() -> Result<()> {
        struct Fail;
        impl Host for Fail {
            fn receive(&mut self, _: &mut Registry, _: u32, _: &ActionEvent) -> Result<()> {
                anyhow::bail!("Missing native callback")
            }
        }
        let mut r = Registry::default();
        r.register(4, None)?;
        r.exclude(4);
        r.dispatch(&event(Some(7), 13), None, &mut Fail)?;
        assert!(r.register(4, Some(7)).is_err());
        r.clear_exclusion(4);
        assert!(r.dispatch(&event(Some(7), 13), Some(4), &mut Fail).is_err());
        assert!(r.active.is_empty());
        r.deactivate(4);
        let epoch = r.epoch();
        r.register(4, Some(7))?;
        assert_eq!(r.epoch(), epoch + 1);
        let mut host = Recorder::default();
        r.dispatch(&event(Some(7), 13), None, &mut host)?;
        assert_eq!(host.0, [(4, 13)]);
        Ok(())
    }
    #[test]
    fn recursive_equivalence_ignores_action_payload_and_native_depth_stops_at_ten() -> Result<()> {
        struct Reenter {
            calls: Vec<u32>,
        }
        impl Host for Reenter {
            fn receive(&mut self, r: &mut Registry, _: u32, e: &ActionEvent) -> Result<()> {
                self.calls.push(e.action);
                let mut duplicate = e.clone();
                duplicate.vector_bits = [u32::MAX; 4];
                duplicate.reference = Some(400);
                assert_eq!(r.dispatch(&duplicate, None, self)?, Admission::Suppressed);
                r.dispatch(&event(None, e.action + 1), None, self)?;
                Ok(())
            }
        }
        let mut r = Registry::default();
        r.register(4, None)?;
        let mut host = Reenter { calls: vec![] };
        r.dispatch(&event(None, 0), None, &mut host)?;
        assert_eq!(host.calls, (0..10).collect::<Vec<_>>());
        assert!(r.active.is_empty());
        Ok(())
    }
    #[test]
    fn queued_delivery_uses_registered_nodes_and_does_not_drop_missing_callbacks() -> Result<()> {
        let mut r = Registry::default();
        r.register(4, Some(8))?;
        let mut queue = Queue::default();
        queue.enqueue(&event(Some(8), 13), Some(99))?; // missing explicit node skips
        queue.enqueue(&event(Some(8), 14), None)?;
        let mut host = Recorder::default();
        queue.process(&mut QueuedDelivery {
            registry: &mut r,
            host: &mut host,
        })?;
        assert_eq!(host.0, [(4, 14)]);
        assert_eq!(queue.pending(), 0);
        Ok(())
    }
    #[test]
    fn first_world_entity_is_not_a_null_broadcast_source() -> Result<()> {
        let mut r = Registry::default();
        r.register(4, Some(0))?;
        r.register(5, Some(1))?;
        assert!(!r.contains(4, None)?);
        assert!(r.contains(4, Some(0))?);
        let mut host = Recorder::default();
        r.dispatch(&event(Some(0), 13), None, &mut host)?;
        assert_eq!(host.0, [(4, 13)]);
        Ok(())
    }
}
