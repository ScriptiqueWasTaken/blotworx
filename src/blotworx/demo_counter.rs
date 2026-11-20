//! Functional demo connection for projectile counters and NPC outputs.
use super::assets::World;
use anyhow::Result;
use std::collections::BTreeMap;
#[derive(Default)]
pub struct State {
    pub(super) states: BTreeMap<usize, u32>,
    pub hits: u64,
    pub changes: u64,
}
impl World {
    pub(super) fn initialize_demo_counters(&mut self) {
        self.demo_story.counters.states = self
            .paint_counters
            .iter()
            .map(|(&id, c)| (id, c.status()))
            .collect();
    }
    pub(super) fn demo_counter_stimulus(
        &mut self,
        entity: usize,
        stimuli: &[(u32, f32)],
        self_hit: bool,
    ) -> Result<()> {
        let Some(counter) = self.paint_counters.get(&entity) else {
            return Ok(());
        };
        let component = self.entities[entity]
            .components
            .iter()
            .find(|c| c.class_name == "PaintThinnerCounterComponent")
            .unwrap();
        let force = component
            .properties
            .iter()
            .any(|p| p.name == "ForceInvulnerable" && p.value.as_bool() == Some(true));
        if force
            || self
                .counter_materials
                .get(&entity)
                .is_some_and(|m| m.invulnerable_remaining > 0.)
        {
            return Ok(());
        }
        let duration = component
            .properties
            .iter()
            .find(|p| p.name == "InvulnerableDuration")
            .and_then(|p| p.value.as_f64())
            .unwrap_or(0.) as f32;
        if let Some(change) = counter.prepare_stimulus(stimuli, 1., self_hit)? {
            self.demo_story
                .counters
                .states
                .entry(entity)
                .or_insert(counter.status());
            if change.scaled_amount != 0. {
                self.paint_counters.get_mut(&entity).unwrap().value = change.value;
                self.demo_story.counters.hits += 1;
                self.advance_counter_materials(entity..entity + 1);
                self.activate_counter_impact(entity, change.scaled_amount)?;
                if self.paint_counters[&entity].status() == self.demo_story.counters.states[&entity] {
                    self.counter_actor_effect(entity, &["spatterimpactfx", "fx_npc_hit"], false);
                }
                if let Some(material) = self.counter_materials.get_mut(&entity) {
                    material.invulnerable_remaining = duration;
                }
                self.advance_demo_counter_states()?;
            }
        }
        Ok(())
    }
    fn counter_actor_effect(&mut self, entity: usize, candidates: &[&str], detached: bool) {
        if !self.entities[entity].has_character_body() {
            return;
        }
        let Some(templates) = self.entities[entity].components.iter()
            .find(|component| component.class_name == "FXComponent")
            .and_then(|component| component.properties.iter().find(|p| p.name == "FXTemplate"))
            .and_then(|property| property.value.as_array())
        else {
            return;
        };
        let Some(name) = candidates.iter().copied().find(|name| templates.iter().any(|value| {
            value.as_str().is_some_and(|template| template.eq_ignore_ascii_case(name))
        })) else {
            return;
        };
        let result = if detached {
            self.start_one_shot_effect(entity, name)
        } else {
            self.dispatch_animation_emitter(entity, 0x214, name)
        };
        if let Err(error) = result {
            let warning = format!("Counter actor effect {} -> {name}: {error:#}", self.entities[entity].name);
            if !self.report.unsupported.contains(&warning) {
                self.report.unsupported.push(warning);
            }
        }
    }
    pub(super) fn advance_demo_counter_states(&mut self) -> Result<()> {
        if !self.demo_story.enabled {
            return Ok(());
        }
        let changes = self
            .paint_counters
            .iter()
            .filter_map(|(&id, c)| {
                let previous = self
                    .demo_story
                    .counters
                    .states
                    .get(&id)
                    .copied()
                    .unwrap_or(c.status());
                (c.status() != previous).then_some((id, previous, c.status()))
            })
            .collect::<Vec<_>>();
        for (entity, previous, current) in changes {
            if self.removed_entities.contains(&entity) {
                continue;
            }
            self.demo_story.counters.states.insert(entity, current);
            self.paint_counters
                .get_mut(&entity)
                .unwrap()
                .published_status = current;
            self.demo_story.counters.changes += 1;
            // Remove the converted-team outcome before publishing a death.
            // GV's death callback immediately checks the encounter count;
            // reversing these events briefly counts the same enemy twice.
            if (previous == 1) != (current == 1) && self.ai_brains.contains_key(&entity) {
                self.emit_entity_output(entity, "OnTeamChanged", entity);
            }
            if current == 2 {
                self.counter_actor_effect(entity, &["blotlingpuddle_fx_small", "puddlesplash"], true);
                self.emit_entity_output(entity, "OnFullySketched", self.player_entity);
                self.emit_entity_output(entity, "OnCharacterPuddled", self.player_entity);
                let zero = self.entities[entity]
                    .components
                    .iter()
                    .filter(|c| c.class_name == "PaintThinnerCounterComponent")
                    .any(|c| {
                        c.properties
                            .iter()
                            .any(|p| p.name == "SendZeroHPEvent" && p.value.as_bool() == Some(true))
                    });
                if zero {
                    let actions = self.component_events.receive_zero_health(
                        entity,
                        true,
                        self.animation_time,
                    )?;
                    self.execute_component_events(actions)?;
                    // The existing playable defeat bridge already publishes
                    // a logical death here. Route its reward receiver as well;
                    // native AI death-animation/event timing remains separate.
                    if let Err(error)=self.receive_death_rewards(entity,2) {
                        self.report.unsupported.push(format!("Death reward counter receiver {}: {error:#}",self.entities[entity].name));
                    }
                    self.emit_entity_output(entity, "OnDeath", self.player_entity);
                }
                // Puddle animation is deferred; defeated actors leave the demo
                // collision/render worlds while their authored outputs run.
                if self.ai_brains.contains_key(&entity) {
                    self.set_entity_hidden(entity, true)?;
                }
            } else {
                if previous == 2 {
                    if self.ai_brains.contains_key(&entity) {
                        self.set_entity_hidden(entity, false)?;
                    }
                    self.emit_entity_output(entity, "OnCharacterRestored", self.player_entity);
                }
                if current == 1 {
                    self.counter_actor_effect(entity,
                        &["fx_turn_good_gremlin", "fx_turn_good_seer", "fx_turn_good", "fx_turn_good_shared"], false);
                    self.emit_entity_output(entity, "OnFullyPainted", self.player_entity);
                    self.emit_entity_output(entity, "OnCharacterPainted", self.player_entity);
                }
            }
        }
        Ok(())
    }
}
