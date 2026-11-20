//! Camera animation-feedback subscription refresh, REL e52e0. Entity/proxy
//! resolution precedes this operation and must preserve native failure order.
use super::{action_event_dispatch::EventRegistry, entity_events::Event};
use anyhow::Result;
#[derive(Default)]
pub struct State {
    pub subscribed: bool,
    pub resolved_target: Option<usize>,
}
impl State {
    pub fn refresh(
        &mut self,
        target: Option<usize>,
        registry: &mut EventRegistry<Event>,
        listener: u32,
    ) -> Result<()> {
        if self.subscribed {
            registry.deactivate_for(9, listener);
        }
        if let Some(target) = target {
            // Native stores byte646 before registration, including failure.
            self.subscribed = true;
            self.resolved_target = Some(target);
            registry.register_for(9, listener, Some(target))?;
        } else {
            self.subscribed = false;
            self.resolved_target = None;
        }
        Ok(())
    }
}
/// e5328 unwraps an EntityTrackerComponent to its TargetToTrack before
/// removing an existing subscription. A retired/null handle resolves null.
pub(super) fn resolve(
    target: Option<usize>,
    entities: &[super::assets::RuntimeEntity],
    removed: &std::collections::HashSet<usize>,
    tracker: Option<&super::entity_tracker::Runtime>,
) -> Result<Option<usize>> {
    let Some(target) =
        target.filter(|target| *target < entities.len() && !removed.contains(target))
    else {
        return Ok(None);
    };
    if entities[target]
        .components
        .iter()
        .any(|component| component.class_name == "EntityTrackerComponent")
    {
        let tracker = tracker
            .filter(|tracker| tracker.owner == target)
            .ok_or_else(|| {
                anyhow::anyhow!("Camera target EntityTrackerComponent runtime binding is absent")
            })?;
        Ok(
            (tracker.target < entities.len() && !removed.contains(&tracker.target))
                .then_some(tracker.target),
        )
    } else {
        Ok(Some(target))
    }
}
#[cfg(test)]
mod tests {
    use super::super::{action_event_dispatch::Host, entity_events::Animation};
    use super::*;
    struct Receiver(Vec<u32>);
    impl Host<Event> for Receiver {
        fn receive(
            &mut self,
            _: &mut EventRegistry<Event>,
            listener: u32,
            event: &Event,
        ) -> Result<()> {
            assert_eq!(listener, 100);
            if let Event::Animation(event) = event {
                self.0.push(event.id);
            } else {
                anyhow::bail!("Unexpected event")
            };
            Ok(())
        }
    }
    #[test]
    fn switching_target_replaces_filters_and_null_removes_delivery() -> Result<()> {
        let mut registry = EventRegistry::default();
        let mut state = State::default();
        let mut receiver = Receiver(vec![]);
        state.refresh(Some(3), &mut registry, 100)?;
        let epoch = registry.epoch();
        state.refresh(Some(4), &mut registry, 100)?;
        assert_eq!(registry.epoch(), epoch + 1);
        assert!(!registry.contains_for(9, 100, Some(3))?);
        assert!(registry.contains_for(9, 100, Some(4))?);
        for source in [3, 4] {
            registry.dispatch(
                &Animation {
                    source: Some(source),
                    header: None,
                    id: 85,
                    flag: 1,
                }
                .into(),
                None,
                &mut receiver,
            )?;
        }
        assert_eq!(receiver.0, [85]);
        state.refresh(None, &mut registry, 100)?;
        registry.dispatch(
            &Animation {
                source: Some(4),
                header: None,
                id: 25,
                flag: 0,
            }
            .into(),
            None,
            &mut receiver,
        )?;
        assert_eq!(receiver.0, [85]);
        assert!(!state.subscribed);
        Ok(())
    }
    #[test]
    fn failed_registration_preserves_native_enable_write() -> Result<()> {
        let mut registry = EventRegistry::default();
        registry.exclude(100);
        let mut state = State::default();
        assert!(state.refresh(Some(3), &mut registry, 100).is_err());
        assert!(state.subscribed);
        assert_eq!(state.resolved_target, Some(3));
        Ok(())
    }
}
