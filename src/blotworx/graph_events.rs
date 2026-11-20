//! Ordinary active-node event traversal, DOL 0x801ed624..0x801ed7c4.
//! Global-wildcard delivery (0x801ed7cc) is a distinct graph phase.
use super::{
    behavior::Document,
    generator_plan::Prepared,
    machine_runtime::{Notice, Runtime},
    state_machine::Event,
};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

/// Native reverse event mapping preserves negative sentinel values; -1 is
/// filtered by delivery. An absent map means graph-local identity mapping.
pub fn local_id(id: i32, map: Option<&BTreeMap<i32, i32>>) -> i32 {
    if id < 0 {
        id
    } else {
        map.map_or(id, |m| m.get(&id).copied().unwrap_or(-1))
    }
}

impl Prepared {
    /// Commit topology only after every node in the old tree has received the
    /// event. Resources and clocks remain owned by this same character.
    pub fn dispatch_and_rebuild(
        &mut self,
        graph: &Document,
        event: &Event,
        resources: &mut super::animation_resources::Resources,
        nested: impl FnMut(&Self, &str) -> Result<Option<i32>>,
        mut notify: impl FnMut(&Runtime, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        self.activate_pending(graph, resources)?;
        let changed = self.dispatch_supported_event(graph, event, None, nested, &mut notify)?;
        if changed || self.transitions.topology_changed {
            let removed = super::generator_plan::rebuild_in_place(graph, self)?;
            self.deactivate_removed(graph, &removed, &mut notify)?;
            self.activate_pending(graph, resources)?;
            self.transitions.topology_changed = false;
        }
        Ok(changed)
    }
    /// Concrete ordinary handlers whose retail virtual methods are verified.
    /// Unsupported node classes fail instead of swallowing the incoming event.
    pub fn handle_node_event(
        &mut self,
        graph: &Document,
        index: usize,
        _event: &Event,
    ) -> Result<()> {
        let node = self.plan.nodes.get(index).context("Missing event node")?;
        let object = graph
            .objects
            .get(&node.object)
            .context("Missing event node definition")?;
        match object.klass.as_str() {
            // These classes' virtual +0x20 points to 0x801ed7c8 (blr).
            "hkbClipGenerator"
            | "hkbBlenderGenerator"
            | "hkbModifierGenerator"
            | "hkbModifierList"
            | "hkbBlendingTransitionEffect"
            | "hkbGeneratorTransitionEffect" => Ok(()),
            "hkbEvaluateExpressionModifier" => {
                self.expressions
                    .handle_event(graph, &mut self.state, &node.object)
            }
            // Attribute activation, event and deactivation methods are all
            // inherited no-ops. Its assignments execute only in output modify.
            "hkbAttributeModifier" | "hkbTwistModifier" => Ok(()),
            "hkbAttachmentModifier" => self
                .attachment_modifiers
                .synchronize(graph, &node.object)
                .map(|_| ()),
            class => anyhow::bail!("Ordinary event handler for {class} is not implemented"),
        }
    }
    pub fn dispatch_supported_event(
        &mut self,
        graph: &Document,
        event: &Event,
        mapping: Option<&BTreeMap<i32, i32>>,
        nested: impl FnMut(&Self, &str) -> Result<Option<i32>>,
        notify: impl FnMut(&Runtime, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        self.dispatch_active_event(
            graph,
            event,
            mapping,
            nested,
            |prepared, index, event| prepared.handle_node_event(graph, index, event),
            notify,
        )
    }
    /// Single-owner graph traversal. Caller supplies its event map and the
    /// concrete handlers for non-machine nodes, including modifier lifecycle.
    /// No unknown generator/modifier is silently treated as a no-op here.
    /// Rebuild only after delivery: newly activated nodes do not receive this
    /// same event retrospectively. Emitted notices remain caller-owned.
    pub fn dispatch_active_event(
        &mut self,
        graph: &Document,
        event: &Event,
        mapping: Option<&BTreeMap<i32, i32>>,
        mut nested: impl FnMut(&Self, &str) -> Result<Option<i32>>,
        mut other: impl FnMut(&mut Self, usize, &Event) -> Result<()>,
        mut notify: impl FnMut(&Runtime, Notice<'_>) -> Result<()>,
    ) -> Result<bool> {
        graph.single("hkbBehaviorGraph")?;
        let local = local_id(event.id, mapping);
        if local == -1 {
            return Ok(false);
        }
        let event = Event {
            id: local,
            payload: event.payload.clone(),
        };
        let count = self.plan.nodes.len();
        let mut changed = false;
        for index in 0..count {
            let node = &self.plan.nodes[index];
            if node.flags.discard_events {
                continue;
            }
            let id = node.object.clone();
            if node.effect.is_none() && self.state.machines.contains_key(&id) {
                let nested_state = nested(self, &id)?;
                changed |= self.dispatch_machine_event(&id, event.id, nested_state, &mut notify)?;
            } else {
                other(self, index, &event)?;
            }
            anyhow::ensure!(
                self.plan.nodes.len() == count,
                "Active graph topology must not rebuild during event delivery"
            );
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn concrete_startup_handlers_activate_expression_without_evaluating_it() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(
            &graph,
            super::super::behavior_state::State::new(&graph)?,
            0,
        )?;
        let index = prepared.state.index("InternalVar_InCombatIdle")?;
        assert_eq!(prepared.state.words()[index], 0);
        assert!(!prepared.expressions.is_active("2:6240"));
        assert!(prepared.dispatch_supported_event(
            &graph,
            &Event {
                id: 0,
                payload: None
            },
            None,
            |_, _| Ok(None),
            |_, _| Ok(())
        )?);
        assert!(prepared.expressions.is_active("2:6240"));
        assert_eq!(prepared.state.words()[index], 0);
        assert_eq!(prepared.machines["2:240"].current, 5);
        Ok(())
    }
    #[test]
    fn reverse_mapping_preserves_sentinels_and_filters_unmapped_positive_ids() {
        let map = BTreeMap::from([(8, 0), (2, 12)]);
        assert_eq!(local_id(8, Some(&map)), 0);
        assert_eq!(local_id(7, Some(&map)), -1);
        assert_eq!(local_id(-2, Some(&map)), -2);
        assert_eq!(local_id(-1, None), -1);
        assert_eq!(local_id(7, None), 7);
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn original_tree_delivers_child_first_and_defers_destination_topology() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(
            &graph,
            super::super::behavior_state::State::new(&graph)?,
            0,
        )?;
        let count = prepared.plan.nodes.len();
        let mut visits = vec![];
        let event = Event {
            id: 8,
            payload: Some(super::super::state_machine::Reference {
                id: "test-payload".into(),
            }),
        };
        assert!(prepared.dispatch_active_event(
            &graph,
            &event,
            Some(&BTreeMap::from([(8, 0)])),
            |_, _| Ok(None),
            |prepared, index, event| {
                assert_eq!(event.id, 0);
                assert_eq!(event.payload.as_ref().unwrap().id, "test-payload");
                visits.push(prepared.plan.nodes[index].object.clone());
                // Explicit test observer, not a product handler for these nodes.
                Ok(())
            },
            |_, _| Ok(())
        )?);
        assert_eq!(prepared.machines["2:240"].current, 5);
        assert_eq!(prepared.plan.nodes.len(), count);
        assert_eq!(visits, ["2:6480", "2:6240", "2:6176", "2:2576"]);
        assert!(!prepared.plan.nodes.iter().any(|n| n.object == "2:35968"));
        // An unmapped event performs no node calls and cannot change state.
        assert!(!prepared.dispatch_active_event(
            &graph,
            &event,
            Some(&BTreeMap::new()),
            |_, _| anyhow::bail!("unexpected machine"),
            |_, _, _| anyhow::bail!("unexpected node"),
            |_, _| anyhow::bail!("unexpected notice")
        )?);
        Ok(())
    }
}
