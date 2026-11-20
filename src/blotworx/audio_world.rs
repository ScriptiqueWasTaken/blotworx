//! Game-space emitter/listener publication. Position queries share the native
//! root NiNode world-bound fallback already used by cameras and multiposition.
use super::World;
use anyhow::{Context,Result};
impl World {
    pub(super) fn prepare_audio_position(&mut self,entity:usize)->Result<()> {
        if self.removed_entities.contains(&entity) {return Ok(())}
        let emitter=self.entities.get(entity).context("Audio entity missing")?;
        let transform=&emitter.transform;
        // A projectile sound follows the thrown entity. Its particle model
        // can expose several render roots and has no single bound to query.
        let position=if emitter.components.iter().any(|c|c.class_name=="ProjectileComponent") {
            transform[12..15].try_into()?
        } else {
            let Some(position)=self.entity_position_or_bound_center(entity)? else {return Ok(())};
            position
        };
        // Orientation is retained but cones are not yet admitted. Complete
        // unscaled rotation publication still needs native lifecycle validation.
        let orientation=[transform[8],transform[9],transform[10]];
        self.audio_playback.set_game_position(entity,position,orientation)
    }
    pub(crate) fn update_audio_view(&mut self,eye:[f32;3],forward:[f32;3],up:[f32;3])->Result<()> {
        for entity in self.audio_playback.position_owners() {self.prepare_audio_position(entity)?;}
        let player=self.entities.get(self.player_entity).context("Audio listener player missing")?;
        let position=player.transform[12..15].try_into().unwrap();
        let value=self.runtime_settings.iter().find(|(key,_)|key.eq_ignore_ascii_case("Audio_Microphone_Position_Game")).map(|(_,value)|value.as_str()).unwrap_or("0.0");
        let ratio=crate::blotworx::config_value::decimal_float(value).map_err(anyhow::Error::msg)?;
        let pose=crate::blotworx::audio_spatial::game_pose(position,eye,forward,up,ratio)?;
        self.audio_playback.set_listener_from_game(pose)
    }
}
