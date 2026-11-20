//! EM1 automatic music control. REL31cc30 selects the control mode;
//! REL1e0c and REL22ec update the authored Wwise state groups.
use super::{assets::World, config_value, physics};
use anyhow::Result;

#[derive(Default)]
enum Mode {
    #[default]
    Auto,
    Exploration,
    Combat,
    Override,
}

#[derive(Default)]
pub(super) struct State {
    mode: Mode,
    bias: f32,
    last_update: Option<f64>,
    last_threat: Option<f64>,
    last: Option<(&'static str, &'static str)>,
}
impl State {
    fn request(&mut self, value: &str) {
        self.mode = match value {
            "Auto" => Mode::Auto,
            "Exploration" => Mode::Exploration,
            "Combat" => Mode::Combat,
            _ => Mode::Override,
        };
        self.last_update = None;
        self.last = None;
    }
}

impl World {
    pub(super) fn set_authored_audio_state(&mut self, group: &str, value: &str) -> Result<()> {
        if group == "Music_State" {
            self.music_state.request(value);
            // The retail Auto selector switches the engine controller back on
            // and returns before submitting a Wwise state with that name.
            if value == "Auto" { return Ok(()); }
        }
        self.audio_playback.set_state(group, value)
    }
    pub(super) fn update_automatic_music(&mut self, now: f64) -> Result<()> {
        if matches!(self.music_state.mode, Mode::Override) { return Ok(()); }
        let setting = |key, fallback| self.runtime_settings.get(key)
            .and_then(|value| config_value::decimal_float(value).ok()).unwrap_or(fallback);
        let update_interval = setting("Audio_CombatIntensity_UpdateFrequency", 1.).max(0.);
        if self.music_state.last_update.is_some_and(|previous| now - previous < f64::from(update_interval)) {
            return Ok(());
        }
        self.music_state.last_update = Some(now);
        let player = self.script_player.position;
        let player_team = self.music_team(self.player_entity).map_or(1, |(team, _)| team);
        let mut threat = self.music_state.bias;
        for &id in self.ai_brains.keys() {
            if id == self.player_entity || self.hidden_entities.contains(&id) || self.removed_entities.contains(&id)
                || self.runtime_health.get(&id).is_some_and(|health| health.health <= 0)
            { continue; }
            let Some((team, component)) = self.music_team(id) else { continue };
            let enemy = team == 5 || player_team == 5
                || (team != 4 && player_team != 4 && (team == 2) != (player_team == 2));
            if !enemy { continue; }
            let property = |name| component.properties.iter().find(|p| p.name == name)
                .and_then(|p| p.value.as_f64()).map_or(0., |v| v as f32);
            let position: [f32; 3] = self.entities[id].transform[12..15].try_into().unwrap();
            let range = property("ThreatAmountRange");
            let distance = physics::length(physics::sub(position, player));
            threat += if distance > range { property("ThreatAmountOutOfRange") } else { property("ThreatAmount") };
        }
        let threat = (threat * setting("Audio_CombatIntensity_Multiplier", 1.)).clamp(0., 100.);
        if threat > 0. { self.music_state.last_threat = Some(now); }
        let delay = setting("Audio_CombatIntensity_ReturnToExploreDelay", 3.).max(0.);
        let recent_combat = self.music_state.last.is_some_and(|(state, _)| state == "Combat")
            && self.music_state.last_threat.is_some_and(|last| now - last < f64::from(delay));
        let value = if matches!(self.music_state.mode, Mode::Combat)
            || (matches!(self.music_state.mode, Mode::Auto) && (threat > 0. || recent_combat))
        {
            ("Combat", if threat >= 66. { "Level_3" } else if threat >= 33. { "Level_2" } else { "Level_1" })
        } else {
            let high = self.guardian_entities.entities().len() > 2;
            ("Exploration", match (self.guardians.selected_kind, high) {
                (49, true) => "Thin_2",
                (49, false) => "Thin_1",
                (51, true) => "Paint_2",
                (51, false) => "Paint_1",
                _ => "Neutral",
            })
        };
        if self.music_state.last != Some(value) {
            self.audio_playback.set_state("Music_State", value.0)?;
            self.audio_playback.set_state(if value.0 == "Combat" { "Music_Combat" } else { "Music_Exploration" }, value.1)?;
            self.music_state.last = Some(value);
        }
        Ok(())
    }
    fn music_team(&self, id: usize) -> Option<(i64, &epic_mickey_lib_rs::scene_file::Component)> {
        let component = self.entities.get(id)?.components.iter().find(|c| c.class_name == "TeamComponent")?;
        let team = ["CurrentTeam", "Team"].into_iter().find_map(|name| component.properties.iter()
            .find(|p| p.name == name).and_then(|p| p.value.as_i64())).unwrap_or(0);
        let team = if team == 2 && self.paint_counters.get(&id).is_some_and(|counter| counter.status() == 1) { 3 } else { team };
        Some((team, component))
    }
    pub(super) fn set_music_bias(&mut self, bias: f32) {
        self.music_state.bias = bias;
        self.music_state.last_update = None;
    }
    pub(super) fn music_bias(&self) -> f32 { self.music_state.bias }
}
