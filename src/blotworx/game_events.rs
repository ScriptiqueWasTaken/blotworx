//! Retail gameplay-event subscription filtering, distinct from Havok events.
//! Registry hash traversal, reentrancy and component callbacks remain
//! to be integrated; callers must preserve native subscription traversal order.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkidFeedback {
    PlayerRumble {
        value: f32,
        channel: u32,
    },
    /// Invoke REL 0x36a584 on the controller's +0x20c reference with (1,0).
    /// Resource binding and the called component's lifecycle remain pending.
    ActivateReferencedEffect,
    DeactivateReferencedEffect,
    SetRemaining(f32),
}

/// Event 0x2b receiver, REL 0x8d878..0x8d8d4. Called only after the
/// controller's earlier event handlers have declined to consume the event.
/// Constants are from 0x4ac3e0 + 8 (zero), +0xe0 (0.1), +0xb0 (0.5).
pub fn skid_feedback(has_effect: bool, payload: f32, mut emit: impl FnMut(SkidFeedback)) {
    if !has_effect || payload < 0. {
        return;
    }
    // Ordered PPC comparisons let unordered payloads reach activation while
    // skipping rumble. Do not rewrite this as a >=0 acceptance check.
    if payload <= 0. {
        emit(SkidFeedback::PlayerRumble {
            value: 0.1,
            channel: 0,
        });
    }
    emit(SkidFeedback::ActivateReferencedEffect);
    emit(SkidFeedback::SetRemaining(0.5));
}

/// REL 0x8b9ec..0x8ba24. Stop only on the update crossing zero, retaining
/// the negative remainder. A missing effect pauses this timer's update.
pub fn update_skid_feedback(
    has_effect: bool,
    remaining: &mut f32,
    dt: f32,
    mut emit: impl FnMut(SkidFeedback),
) {
    if !has_effect || *remaining <= 0. {
        return;
    }
    *remaining -= dt;
    if *remaining <= 0. {
        emit(SkidFeedback::DeactivateReferencedEffect);
    }
}

/// One listener's owned registration for one event type. Registry lookup and
/// deferred-removal cleanup must happen before calling `register`.
pub struct Registration {
    enabled: bool,
    activation_epoch: u32,
    entity_filters: Vec<u32>,
}
impl Default for Registration {
    fn default() -> Self {
        Self {
            enabled: false,
            activation_epoch: u32::MAX,
            entity_filters: Vec::new(),
        }
    }
}
impl Registration {
    /// REL 0x1f2eb8..0x1f30e0. Only activation of an inactive listener advances
    /// the manager epoch. Extending an active registration preserves its epoch.
    pub fn register(&mut self, entity: u32, manager_epoch: &mut u32) {
        if self.enabled {
            if entity == 0 {
                self.entity_filters.clear();
                self.entity_filters.push(0);
            } else if self.entity_filters[0] != 0 && !self.entity_filters.contains(&entity) {
                self.entity_filters.push(entity);
            }
        } else {
            self.entity_filters.clear();
            self.entity_filters.push(entity);
            self.enabled = true;
            *manager_epoch = manager_epoch.wrapping_add(1);
            self.activation_epoch = *manager_epoch;
        }
    }
    /// Cleanup 0x1f3f5c..0x1f3f68 clears size and enabled, retaining storage.
    pub fn deactivate(&mut self) {
        self.entity_filters.clear();
        self.enabled = false;
    }
    pub fn subscription(&self) -> Subscription<'_> {
        Subscription {
            enabled: self.enabled,
            activation_epoch: self.activation_epoch,
            entity_filters: &self.entity_filters,
        }
    }
    /// Registration query 0x1f34b8 differs from delivery: event entity zero
    /// does not match an entity-specific registration. It does not test epoch.
    pub fn contains(&self, entity: u32, excluded: bool) -> bool {
        self.enabled
            && !excluded
            && (self.entity_filters.first() == Some(&0) || self.entity_filters.contains(&entity))
    }
}

pub struct Subscription<'a> {
    pub enabled: bool,
    pub activation_epoch: u32,
    pub entity_filters: &'a [u32],
}
impl Subscription<'_> {
    /// REL 0x1f4348..0x1f43c8 (broadcast), also 0x1f41c8..0x1f4258
    /// (explicit listener). Each matching filter invokes the listener. Native
    /// registration prevents wildcard/exact mixtures and duplicate filters;
    /// entity-zero events still deliver once per distinct specific filter.
    /// `excluded` is the result of the manager's 0x1f3dc0 membership query.
    pub fn deliveries(
        &self,
        event_entity: u32,
        dispatch_epoch: u32,
        excluded: bool,
    ) -> impl Iterator<Item = usize> + '_ {
        let eligible = self.enabled && self.activation_epoch <= dispatch_epoch && !excluded;
        self.entity_filters
            .iter()
            .enumerate()
            .filter_map(move |(index, &filter)| {
                (eligible && (filter == 0 || event_entity == 0 || filter == event_entity))
                    .then_some(index)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skid_feedback_expiry_stops_once_and_preserves_timer_remainder() {
        let mut remaining = 0.5;
        let mut effects = Vec::new();
        update_skid_feedback(false, &mut remaining, 1., |e| effects.push(e));
        assert_eq!(remaining, 0.5);
        update_skid_feedback(true, &mut remaining, 0.25, |e| effects.push(e));
        assert!(effects.is_empty());
        update_skid_feedback(true, &mut remaining, 0.5, |e| effects.push(e));
        assert_eq!(remaining, -0.25);
        assert_eq!(effects, vec![SkidFeedback::DeactivateReferencedEffect]);
        update_skid_feedback(true, &mut remaining, 1., |e| effects.push(e));
        assert_eq!(remaining, -0.25);
        assert_eq!(effects.len(), 1);
        remaining = 0.25;
        effects.clear();
        update_skid_feedback(true, &mut remaining, 0.25, |e| effects.push(e));
        assert_eq!(remaining, 0.);
        assert_eq!(effects.len(), 1);
        remaining = f32::NAN;
        effects.clear();
        update_skid_feedback(true, &mut remaining, 1., |e| effects.push(e));
        assert!(remaining.is_nan());
        assert!(effects.is_empty());
    }
    #[test]
    fn skid_receiver_preserves_feedback_order_and_payload_gates() {
        let mut effects = Vec::new();
        skid_feedback(true, 0., |e| effects.push(e));
        assert_eq!(
            effects,
            vec![
                SkidFeedback::PlayerRumble {
                    value: 0.1,
                    channel: 0
                },
                SkidFeedback::ActivateReferencedEffect,
                SkidFeedback::SetRemaining(0.5)
            ]
        );
        for payload in [1., f32::NAN, f32::INFINITY] {
            effects.clear();
            skid_feedback(true, payload, |e| effects.push(e));
            assert_eq!(
                effects,
                vec![
                    SkidFeedback::ActivateReferencedEffect,
                    SkidFeedback::SetRemaining(0.5)
                ]
            );
        }
        for (present, payload) in [(false, 0.), (true, -1.), (true, f32::NEG_INFINITY)] {
            effects.clear();
            skid_feedback(present, payload, |e| effects.push(e));
            assert!(effects.is_empty());
        }
    }
    #[test]
    fn registration_collapses_wildcards_and_deduplicates_specific_entities() {
        let mut registration = Registration::default();
        let mut epoch = 0;
        registration.register(20, &mut epoch);
        registration.register(10, &mut epoch);
        registration.register(20, &mut epoch);
        assert_eq!(epoch, 1);
        assert_eq!(registration.subscription().entity_filters, &[20, 10]);
        assert!(!registration.contains(0, false));
        assert_eq!(
            registration
                .subscription()
                .deliveries(0, epoch, false)
                .count(),
            2
        );
        registration.register(0, &mut epoch);
        registration.register(30, &mut epoch);
        assert_eq!(registration.subscription().entity_filters, &[0]);
        assert!(registration.contains(30, false));
        assert!(!registration.contains(30, true));
        assert_eq!(epoch, 1);
    }
    #[test]
    fn reactivation_gets_a_new_epoch_and_replaces_old_filters() {
        let mut registration = Registration::default();
        let mut epoch = 10;
        registration.register(10, &mut epoch);
        let dispatch_epoch = epoch;
        registration.deactivate();
        assert!(!registration.contains(10, false));
        registration.register(30, &mut epoch);
        assert_eq!(epoch, 12);
        assert_eq!(registration.subscription().entity_filters, &[30]);
        assert_eq!(
            registration
                .subscription()
                .deliveries(30, dispatch_epoch, false)
                .count(),
            0
        );
        assert_eq!(
            registration
                .subscription()
                .deliveries(30, epoch, false)
                .count(),
            1
        );
        epoch = u32::MAX;
        registration.deactivate();
        registration.register(0, &mut epoch);
        assert_eq!(epoch, 0);
        assert_eq!(registration.subscription().activation_epoch, 0);
    }
    #[test]
    fn wildcard_and_exact_registrations_preserve_duplicate_delivery() {
        let subscription = Subscription {
            enabled: true,
            activation_epoch: 7,
            entity_filters: &[20, 0, 10, 10, 30],
        };
        assert_eq!(
            subscription.deliveries(10, 7, false).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            subscription.deliveries(0, 7, false).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            subscription.deliveries(40, 7, false).collect::<Vec<_>>(),
            vec![1]
        );
    }
    #[test]
    fn disabled_future_and_excluded_listeners_do_not_receive_events() {
        let mut subscription = Subscription {
            enabled: true,
            activation_epoch: 7,
            entity_filters: &[0],
        };
        assert_eq!(subscription.deliveries(10, 6, false).count(), 0);
        assert_eq!(subscription.deliveries(10, 7, true).count(), 0);
        assert_eq!(subscription.deliveries(10, 7, false).count(), 1);
        subscription.enabled = false;
        assert_eq!(subscription.deliveries(10, u32::MAX, false).count(), 0);
        subscription.enabled = true;
        subscription.activation_epoch = u32::MAX;
        assert_eq!(subscription.deliveries(10, 0, false).count(), 0);
    }
}
