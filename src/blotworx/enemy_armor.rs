use super::{assets::World, projectile_stimulus::Emission};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    windows: BTreeMap<usize, Window>,
}

#[derive(Default)]
struct Window {
    expires: Option<f32>,
    shell_was_thin: bool,
    last_hit: Option<f32>,
    repaint_at: Option<f32>,
}

impl State {
    pub(super) fn retire(&mut self, entity: usize) {
        self.windows.remove(&entity);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Beetleworx {
    Cannon,
    Crawler,
    Fodder,
}

impl Beetleworx {
    fn of(kind: &str) -> Option<Self> {
        let family = kind.split(':').next()?;
        if family.eq_ignore_ascii_case("BeetleworxCannon") {
            Some(Self::Cannon)
        } else if family.eq_ignore_ascii_case("BeetleworxCrawler") {
            Some(Self::Crawler)
        } else if family.eq_ignore_ascii_case("BeetleworxFodder") {
            Some(Self::Fodder)
        } else {
            None
        }
    }

    fn is_shell(self, name: &str) -> bool {
        match self {
            Self::Cannon => name.contains("beetleworxcannoncar"),
            Self::Crawler => ["teacup", "_body", "_head", "_leftarm", "_leftleg", "_rightarm", "_rightleg"]
                .iter().any(|part| name.contains("beetleworxcrawler") && name.contains(part)),
            Self::Fodder => name.contains("beetleworxfodder_toon"),
        }
    }

    fn weak_collision(self) -> &'static str {
        match self {
            Self::Cannon => "beetleworxcannonweakspot",
            Self::Crawler => "beetleworxcrawlerweakcollision",
            Self::Fodder => "beetleworxfodder_weakcollision",
        }
    }
}

impl World {
    fn beetleworx_kind(&self, actor: usize) -> Option<Beetleworx> {
        Beetleworx::of(&self.ai_brains.get(&actor)?.ai_type)
    }

    pub(super) fn advance_enemy_armor(&mut self, actor: usize) -> Result<bool> {
        let Some(kind) = self.beetleworx_kind(actor) else {
            return Ok(false);
        };
        if self.runtime_health.get(&actor).is_some_and(|health| health.health <= 0)
            || !self.prepare_child_slots(actor)?
        {
            return Ok(false);
        }
        let now = self.animation_time;
        let shell = self.live_child_slots.get(&actor).into_iter().flatten().flatten().copied()
            .filter(|child| {
                if self.removed_entities.contains(child) || !self.model_paint.contains_key(child) {
                    return false;
                }
                let name = self.entities[*child].name.to_ascii_lowercase();
                kind.is_shell(&name)
            })
            .collect::<Vec<_>>();
        let shell_thin = !shell.is_empty() && shell.iter().all(|child| !self.model_paint[child].painted);
        let duration = self.runtime_settings.get("BEH_BeetleworxCrawlerVulnerable_Default_VulnerableTime")
            .and_then(|value| super::config_value::decimal_float(value).ok())
            .filter(|duration| duration.is_finite() && *duration > 0.).unwrap_or(5.);
        let state = self.demo_story.enemy_armor.windows.entry(actor).or_default();
        let just_thinned = shell_thin && !state.shell_was_thin && state.repaint_at.is_none();
        state.shell_was_thin = shell_thin;
        if just_thinned {
            state.expires = Some(now + duration);
            if self.authored_havok.has_event(actor, "EVENT_Become_Vulnerable") {
                self.authored_havok.send_event(actor, "EVENT_Become_Vulnerable")?;
            }
        }
        let state = self.demo_story.enemy_armor.windows.get_mut(&actor).unwrap();
        let exposed = state.expires.is_some_and(|expires| expires > now);
        if !exposed && state.expires.take().is_some() {
            state.repaint_at = Some(now);
            let event = if kind == Beetleworx::Crawler {
                "EVENT_Become_Invulnerable_Part1_Start"
            } else {
                "EVENT_Become_Invulnerable"
            };
            if self.authored_havok.has_event(actor, event) {
                self.authored_havok.send_event(actor, event)?;
            }
        }
        if self.demo_story.enemy_armor.windows[&actor].repaint_at.is_some_and(|at| now >= at) {
            let player = self.entities[self.player_entity].transform[12..15].try_into()?;
            let mut pending = false;
            for child in shell {
                if !self.model_paint[&child].painted {
                    self.change_whole_model_paint(child, true, player, true)?;
                    pending |= !self.model_paint[&child].painted;
                }
            }
            let state = self.demo_story.enemy_armor.windows.get_mut(&actor).unwrap();
            state.repaint_at = pending.then_some(now + 0.25);
        }
        Ok(exposed)
    }

    pub(super) fn prepare_cannon_weapons(&mut self, actor: usize) -> Result<()> {
        if self.beetleworx_kind(actor) != Some(Beetleworx::Cannon)
            || !self.prepare_child_slots(actor)?
        {
            return Ok(());
        }
        let Some(children) = self.live_child_slots.get(&actor) else {
            return Ok(());
        };
        let berserk = children.iter().flatten().any(|child| {
            Beetleworx::Cannon.is_shell(&self.entities[*child].name.to_ascii_lowercase())
                && self.model_paint.get(child).is_some_and(|model| !model.painted)
        });
        // The original Cannon enters combat with the weapon set enabled by
        // its car shell. Its paint/thinner callbacks can change that set later.
        for child in children.iter().flatten().copied() {
            let child = &mut self.entities[child];
            let name = child.name.to_ascii_lowercase();
            if !name.contains("beetleworxcannon") || !(name.contains("shot") || name.contains("swipe")) {
                continue;
            }
            let enabled = name.contains("berserkshot") == berserk;
            if let Some(shots) = child.components.iter_mut().find(|c| c.class_name == "WeaponComponent")
                .and_then(|component| component.properties.iter_mut().find(|p| p.name == "WeaponShots"))
            {
                shots.value = serde_json::json!(if enabled { -1 } else { 0 });
            }
        }
        Ok(())
    }

    pub(super) fn enemy_armor_animation(&mut self, actor: usize, event: u32) -> Result<()> {
        if self.beetleworx_kind(actor).is_none() {
            return Ok(());
        }
        let Some(name) = self.animation_events.as_ref().ok().and_then(|catalog| catalog.name(event).ok()) else {
            return Ok(());
        };
        match name {
            "EVENT_Become_Invulnerable_Part1_Done" => {
                self.authored_havok.send_event(actor, "EVENT_Become_Invulnerable_Part2_Start")?;
            }
            "EVENT_Become_Invulnerable" | "EVENT_Become_Invulnerable_Part2_Start" => {
                if let Some(state) = self.demo_story.enemy_armor.windows.get_mut(&actor) {
                    if state.expires.take().is_some() {
                        state.repaint_at = Some(self.animation_time);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn enemy_weak_point_hit(&mut self, target: usize, emissions: &[Emission]) -> Result<()> {
        let Some(parent) = self.transform_hierarchy.get(&target).map(|node| node.parent) else {
            return Ok(());
        };
        let Some(kind) = self.beetleworx_kind(parent) else {
            return Ok(());
        };
        if !self.entities[target].name.to_ascii_lowercase().contains(kind.weak_collision()) {
            return Ok(());
        }
        let Some(spin) = emissions.iter().find(|e| e.stimulus == 8 && e.amount > 0.
            && e.owner == Some(self.player_entity as u64)) else {
            return Ok(());
        };
        let Some(state) = self.demo_story.enemy_armor.windows.get_mut(&parent) else {
            return Ok(());
        };
        if !state.expires.is_some_and(|expires| self.animation_time < expires)
            || state.last_hit.is_some_and(|hit| self.animation_time - hit < 0.5)
        {
            return Ok(());
        }
        state.last_hit = Some(self.animation_time);
        let mut hit = spin.clone();
        hit.target = Some(parent as u64);
        self.receive_combat_inner(parent, &[hit], Some(1.))?;
        if self.authored_havok.has_event(parent, "EVENT_Start_VulnerableSpotHitReaction") {
            self.authored_havok.send_event(parent, "EVENT_Start_VulnerableSpotHitReaction")?;
        }
        Ok(())
    }
}
