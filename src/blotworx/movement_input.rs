//! REL30b61c/30b770: bit1 in both active214 and saved218 flags.
use anyhow::{ensure,Result};
pub fn set_enabled(current:&mut u32,saved:&mut u32,enabled:bool) {
    for word in [current,saved] {if enabled {*word|=2;} else {*word&=!2;}}
}
impl super::assets::World {
    pub(super) fn movement_input_lua(&mut self,lua:&mlua::Lua,mut args:mlua::MultiValue,enabled:bool)->mlua::Result<mlua::MultiValue> {
        let value=args.pop_back().ok_or_else(||mlua::Error::external("Movement input toggle requires an entity"))?;
        let Some(entity)=self.scripts.resolve_native_entity_argument(lua,value)?.filter(|i|!self.removed_entities.contains(i)) else {return Ok(mlua::MultiValue::new());};
        self.set_movement_input(entity,enabled).map_err(mlua::Error::external)?;
        Ok(mlua::MultiValue::new())
    }
    fn set_movement_input(&mut self,entity:usize,enabled:bool)->Result<()> {
        if entity==self.player_entity {
            ensure!(self.script_player_available,"Movement input toggle requires the live player scope");
            set_enabled(&mut self.script_player.motion_input_flags,&mut self.script_player.saved_motion_input_flags,enabled);
        } else if self.entities[entity].components.iter().any(|c|c.class_name=="BasicMotionControllerComponent") {
            let state=self.npc_relocations.entry(entity).or_default();
            set_enabled(&mut state.input_flags,&mut state.saved_input_flags,enabled);
        }
        Ok(())
    }
}
