use super::assets::World;
use anyhow::{Context,Result,ensure};
use epic_mickey_lib_rs::scene_file::{Component,Property};
use mlua::{Lua,Value,MultiValue};

fn number(c:&Component,name:&str)->Result<f32> {
    let value=c.properties.iter().find(|p|p.name==name)
        .map(|p|p.value.as_f64().context("LogicTimer float property has wrong type"))
        .transpose()?.unwrap_or(0.) as f32;
    ensure!(value.is_finite(),"Nonfinite LogicTimer {name}"); Ok(value)
}
fn flag(c:&Component,name:&str)->Result<bool> {
    c.properties.iter().find(|p|p.name==name)
        .map(|p|p.value.as_bool().context("LogicTimer Boolean property has wrong type"))
        .transpose().map(|v|v.unwrap_or(false))
}
fn store(c:&mut Component,name:&str,value:serde_json::Value,class:&str)->Result<()> {
    if let Some(p)=c.properties.iter_mut().find(|p|p.name==name) {
        ensure!(p.class_name==class && !p.value.is_array(),"Invalid LogicTimer property {name}");
        p.value=value;
    } else {c.properties.push(Property::new(class.into(),name.into(),false,false,false,value));}
    Ok(())
}
impl World {
    pub(super) fn initialize_logic_timers(&mut self,range:std::ops::Range<usize>)->Result<()> {
        let keys=self.entities.iter().enumerate().filter(|(i,_)|range.contains(i))
            .flat_map(|(i,e)|e.components.iter().filter(|c|c.class_name=="LogicTimerComponent")
                .map(move|c|(i,c.link_id.to_u32()))).collect::<Vec<_>>();
        for key in keys {
            ensure!(!self.logic_timers.contains_key(&key),"LogicTimer initialized twice");
            self.logic_timers.insert(key,Default::default());
            self.reset_logic_timer(key)?;
        } Ok(())
    }
    fn reset_logic_timer(&mut self,key:(usize,u32))->Result<()> {
        let c=self.entities[key.0].components.iter().find(|c|c.link_id.to_u32()==key.1)
            .context("LogicTimer component disappeared")?;
        let interval=number(c,"TimerInterval")?;
        let random=if flag(c,"UseRandomInterval")? {
            Some((number(c,"MinRandomInterval")?,number(c,"MaxRandomInterval")?,self.game_random.draw()))
        } else {None};
        self.logic_timers.get_mut(&key).context("Uninitialized LogicTimer")?.reset(interval,random);
        Ok(())
    }
    pub(super) fn advance_logic_timers(&mut self,delta:f32)->Result<()> {
        let keys=self.logic_timers.keys().copied().collect::<Vec<_>>();
        for key in keys {
            if self.removed_entities.contains(&key.0) {self.logic_timers.remove(&key);continue}
            if !self.entity_scene_active(key.0) {continue}
            let c=self.entities[key.0].components.iter().find(|c|c.link_id.to_u32()==key.1)
                .context("LogicTimer component disappeared")?;
            let disabled=flag(c,"TimerDisabled")?;
            if let Some(high)=self.logic_timers.get_mut(&key).unwrap().advance(delta,disabled) {
                self.emit_entity_output(key.0,"OnLogicTimer",key.0);
                self.emit_entity_output(key.0,if high {"OnLogicTimerHigh"} else {"OnLogicTimerLow"},key.0);
                self.reset_logic_timer(key)?;
            }
        } Ok(())
    }
    pub(super) fn logic_timer_lua(&mut self,lua:&Lua,name:&str,mut args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=if name=="TimerSetInterval" {2} else {1} {
            return Err(mlua::Error::external(format!("Invalid {name} argument count")));
        }
        let value=if name=="TimerSetInterval" {
            let value=lua.coerce_number(args.pop_back().unwrap())?.unwrap_or(0.) as f32;
            if !value.is_finite() {return Err(mlua::Error::external("Nonfinite timer interval"))}
            Some(value)
        } else {None};
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args.pop_front().unwrap())?
            .filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::new())};
        let Some(index)=self.entities[owner].components.iter().position(|c|c.class_name=="LogicTimerComponent")
            else {return Ok(MultiValue::new())};
        let key=(owner,self.entities[owner].components[index].link_id.to_u32());
        let result=match name {
            "TimerReset"=>self.reset_logic_timer(key),
            "TimerSetInterval"=>store(&mut self.entities[owner].components[index],"TimerInterval",serde_json::json!(value.unwrap()),"Float"),
            "TimerEnable"|"TimerDisable"=>store(&mut self.entities[owner].components[index],"TimerDisabled",serde_json::json!(name=="TimerDisable"),"Boolean"),
            _=>unreachable!(),
        };result.map_err(mlua::Error::external)?;
        Ok(MultiValue::new())
    }
}
