use super::assets::World;
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    vacuum: BTreeMap<usize, Vacuum>,
}

#[derive(Default)]
struct Vacuum {
    until: f32,
    next_attack: f32,
    effective_thinner: f32,
}

impl State {
    pub(super) fn retire(&mut self, entity: usize) {
        self.vacuum.remove(&entity);
    }
}

impl World {
    fn is_blob(&self, actor: usize) -> bool {
        self.ai_brains.get(&actor).is_some_and(|brain| {
            brain.behaviors.iter().any(|behavior| behavior == "TheBlobVacuum")
        })
    }

    fn blob_vacuum_setting(&self, name: &str, fallback: f32) -> f32 {
        self.runtime_settings.get(&format!("BEH_TheBlobVacuum_Default_{name}"))
            .and_then(|value| super::config_value::decimal_float(value).ok())
            .filter(|value| value.is_finite() && *value >= 0.).unwrap_or(fallback)
    }

    pub(super) fn record_blob_vacuum_stimuli(&mut self, target: usize, stimuli: &[(u32, f32)]) {
        let actor = if self.entities[target].name.to_ascii_lowercase().contains("theblobmouth") {
            self.transform_hierarchy.get(&target).map_or(target, |node| node.parent)
        } else {
            target
        };
        if let Some(vacuum) = self.demo_story.boss_ai.vacuum.get_mut(&actor) {
            if vacuum.until > self.animation_time {
                vacuum.effective_thinner += stimuli.iter().filter(|&&(kind, amount)| kind == 0 && amount > 0.)
                    .map(|&(_, amount)| amount).sum::<f32>();
            }
        }
    }

    pub(super) fn advance_blob_vacuum(
        &mut self, actor: usize, can_start: bool, paused: bool, shape: usize,
        pose: [f32; 3], aim: [f32; 3], facing: [f32; 2],
    ) -> Result<bool> {
        if !self.is_blob(actor) || !self.authored_havok.has_event(actor, "EVENT_Start_AttackVacuum") {
            return Ok(false);
        }
        let now = self.animation_time;
        let x = pose[0] - aim[0];
        let z = pose[2] - aim[2];
        let distance = x.hypot(z);
        let forward = (-x * facing[0] - z * facing[1]) / distance.max(0.001);
        let cone = self.blob_vacuum_setting("ConeHalfAngle", 45.).to_radians().cos();
        let abort_distance = self.blob_vacuum_setting("AbortDistance", 4.);
        let choke_damage = self.blob_vacuum_setting("DamageToChoke", 8.);
        let state = self.demo_story.boss_ai.vacuum.entry(actor).or_default();
        if state.until > 0. {
            let loop_active = self.authored_havok.active_state_time(actor, "Vacuum_loop").is_some();
            let choke = state.effective_thinner >= choke_damage && choke_damage > 0.;
            if choke || now >= state.until || paused || (loop_active && distance <= abort_distance) {
                state.until = 0.;
                let event = if choke { "EVENT_Start_AttackVacuumChoke" } else { "EVENT_Finish_AttackVacuum" };
                self.authored_havok.send_event(actor, event)?;
            } else if loop_active && forward >= cone && distance > 0.001 && self.script_player_available
                && self.demo_melee_clear(shape, pose, aim, self.player_entity)?
            {
                let near = self.blob_vacuum_setting("SuctionMaxSpeed", 10.);
                let far = self.blob_vacuum_setting("SuctionMinSpeed", 6.);
                let weight = ((distance - abort_distance) / 7.).clamp(0., 1.);
                let pull = near + (far - near) * weight;
                let velocity = &mut self.script_player.velocity;
                let direction = [x / distance, z / distance];
                let along = velocity[0] * direction[0] + velocity[2] * direction[1];
                let add = (pull - along).max(0.);
                velocity[0] += direction[0] * add;
                velocity[2] += direction[1] * add;
            }
            return Ok(true);
        }
        if self.authored_havok.active_state_time(actor, "VacuumAttack").is_some() {
            return Ok(true);
        }
        if !can_start || paused || now < state.next_attack || forward < cone {
            return Ok(false);
        }
        let Some(weapon) = self.child_entity_by_name(actor, "theblobvacuum")? else { return Ok(false); };
        let Some(component) = self.entities[weapon].components.iter().find(|c| c.class_name == "WeaponComponent") else {
            return Ok(false);
        };
        let value = |name| component.properties.iter().find(|p| p.name == name)
            .and_then(|p| p.value.as_f64()).map(|value| value as f32);
        let (Some(minimum), Some(maximum), Some(refire)) = (value("WeaponMinRange"), value("WeaponMaxRange"), value("WeaponMinRefireTime")) else {
            return Ok(false);
        };
        if !(minimum..=maximum).contains(&distance) {
            return Ok(false);
        }
        self.authored_havok.send_event(actor, "EVENT_Start_AttackVacuum")?;
        self.demo_story.boss_ai.vacuum.insert(actor, Vacuum {
            until: now + 6.,
            next_attack: now + refire,
            effective_thinner: 0.,
        });
        Ok(true)
    }
}
