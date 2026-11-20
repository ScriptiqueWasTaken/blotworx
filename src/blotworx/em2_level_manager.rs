//! PC level identity and transition preflight (ec64c0/ec6510/ec6560,
//! 999740,998bc0). Publishing/unpublishing resources remains explicit work.
use super::compiled_level::Level;
use anyhow::{ensure,Context,Result};
use std::{collections::HashSet,sync::Arc};
#[derive(serde::Serialize)]
struct ZoneTarget {name:String,status:u32,files:Vec<super::compiled_level::Resource>}
#[derive(serde::Serialize)]
struct Transition {from:String,to:String,zone_targets:Vec<ZoneTarget>,interior:Option<String>}
pub(super) struct Manager {
    level:Arc<Level>,current:usize,desired:Vec<u32>,interior:Option<usize>,pending:Option<Transition>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_targets_match_isolated_original_pc_instructions()->Result<()> {
        use super::super::compiled_level::{State,Zone,Link};
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/em2_level_state_reference.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let desired=case["desired"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap() as u32).collect::<Vec<_>>();
            let zones=(0..desired.len()).map(|i|Zone {name:format!("zone{i}"),folder:String::new(),metadata:[0;5],files:vec![],dependencies:vec![]}).collect();
            let states=["old","next"].iter().map(|name|State {name:name.to_string(),level:"level".into(),destinations:vec![],
                links:case[name].as_array().unwrap().iter().map(|link|Link {zone:format!("zone{}",link[0]),kind:link[1].as_u64().unwrap() as u32,flag:0}).collect()}).collect();
            let manager=Manager {level:Arc::new(Level {version:9,trusted_measurements:0,zones,states}),current:0,desired:desired.clone(),
                interior:case["interior"].as_u64().map(|n|n as usize),pending:None};
            let (actual,interior)=if case["same"]==true {(desired,manager.interior)}else{manager.targets(Some(0),1)?};
            assert_eq!(serde_json::json!(actual),case["reference"]["desired"],"{}",case["name"]);
            assert_eq!(serde_json::json!(interior),case["reference"]["interior"],"{}",case["name"]);
        }
        Ok(())
    }
}
impl Manager {
    pub fn new(level:Arc<Level>,state:usize)->Result<Self> {
        ensure!(state<level.states.len(),"Missing initial compiled level state");
        let mut manager=Self {desired:vec![0;level.zones.len()],level,current:state,interior:None,pending:None};
        let (desired,interior)=manager.targets(None,state)?;
        manager.desired=desired;manager.interior=interior;Ok(manager)
    }
    fn zone(&self,name:&str)->Result<usize> {
        self.level.zones.iter().position(|z|z.name.eq_ignore_ascii_case(name)).with_context(||format!("Unmounted compiled level zone {name}"))
    }
    fn targets(&self,old:Option<usize>,next:usize)->Result<(Vec<u32>,Option<usize>)> {
        let mut desired=self.desired.clone();let mut interior=None;let mut touched=HashSet::new();
        for link in &self.level.states[next].links {
            let zone=self.zone(&link.zone)?;touched.insert(zone);
            match link.kind {
                0..=2=>desired[zone]=link.kind,
                3=>{if desired[zone]==0 {desired[zone]=2;}},
                4=>{},
                5=>{if desired[zone]!=0 {
                    if let Some(previous)=interior {desired[previous]=0;desired[zone]=0;}
                    else {interior=Some(zone);}
                }},
                kind=>anyhow::bail!("Unsupported compiled level link operation {kind}"),
            }
        }
        if interior.is_some_and(|zone|desired[zone]==0){interior=None;}
        if let Some(old)=old {for link in &self.level.states[old].links {
            let zone=self.zone(&link.zone)?;if !touched.contains(&zone){desired[zone]=0;}
        }}
        Ok((desired,interior))
    }
    pub fn call(&mut self,lua:&mlua::Lua,name:&str,args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        use mlua::{FromLua,Value};
        let state=&self.level.states[self.current];
        let value=match name {
            "LevelManager_GetCurrentLevelName"=>Some(state.level.as_str()),
            "LevelManager_GetCurrentState"=>Some(state.name.as_str()),
            "LevelManager_GetCurrentInterior"=>self.interior.map(|zone|self.level.zones[zone].name.as_str()),
            "LevelManager_SetCurrentState"=>{
                let value=args.back().cloned().unwrap_or(Value::Nil);
                if matches!(value,Value::Nil){return Ok(mlua::MultiValue::new());}
                let string=mlua::String::from_lua(value,lua)?;let bytes=string.as_bytes();
                let value=std::str::from_utf8(bytes.split(|b|*b==0).next().unwrap_or_default()).map_err(mlua::Error::external)?;
                // Native lookup failure and selecting the same state are no-ops.
                let Some(next)=self.level.states.iter().position(|s|s.name.eq_ignore_ascii_case(value)) else{return Ok(mlua::MultiValue::new());};
                if next==self.current {return Ok(mlua::MultiValue::new());}
                let (desired,interior)=self.targets(Some(self.current),next).map_err(mlua::Error::external)?;
                let zone_targets=self.level.zones.iter().zip(desired).map(|(zone,status)|ZoneTarget {name:zone.name.clone(),status,files:zone.files.clone()}).collect();
                self.pending=Some(Transition {from:state.name.clone(),to:self.level.states[next].name.clone(),zone_targets,
                    interior:interior.map(|zone|self.level.zones[zone].name.clone())});
                return Err(mlua::Error::external(format!("EM2 transition {} -> {} requires music pause and resource/entity lifecycle; source graph request retained",state.name,value)));
            },
            _=>return Err(mlua::Error::external("Unknown compiled level operation")),
        };
        Ok(mlua::MultiValue::from_vec(vec![match value {Some(value)=>Value::String(lua.create_string(value)?),None=>Value::Nil}]))
    }
    pub fn summary(&self)->serde_json::Value {
        let state=&self.level.states[self.current];
        serde_json::json!({"level":state.level,"state":state.name,"interior":self.interior.map(|i|&self.level.zones[i].name),"pending_transition":self.pending,
            "scope":"source graph identity and transition preflight; music pause, streamed resources and entity lifecycle remain unconnected"})
    }
}
