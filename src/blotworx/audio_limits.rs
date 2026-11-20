//! Authored per-object limit inheritance and retail priority selection.
//! Bus/virtual-voice limits and native stop ramps remain separate work.
use super::{audio_bank::{Bank,NodePrefix},audio_sound::{Priority,Radius}};
use anyhow::{Context,Result};
#[derive(Clone,Copy,Debug)]
pub struct Limit {pub node:u32,pub maximum:u16,pub kill_newest:bool}
#[derive(Clone,Debug)]
pub struct Definition {pub limit:Limit,base:f32,offset:f32,radius:Radius}
impl Definition {
    pub fn from_bank(bank:&Bank<'_>,sound:u32)->Result<Self> {
        let chain=bank.parameter_chain(sound)?;
        //80337834: only the first override (or the root) registers the
        // object's count. An authored nonzero value without override is inert.
        let limit=chain.iter().find(|n|n.voices.override_maximum || n.parent==0).context("Missing audio limit root")?;
        let mut priority=None;
        for n in &chain {
            let object=&bank.objects[&n.id];
            let prefix=NodePrefix::parse(if object.kind==2 {object.source()?.node_parameters}else {object.payload})?;
            if prefix.priority[1]!=0 || n.parent==0 {
                priority=Some((prefix.priority[0] as i8 as f32,if prefix.priority[2]!=0 {prefix.priority[3] as i8 as f32}else {0.}));break;
            }
        }
        let (base,offset)=priority.context("Missing audio priority root")?;
        Ok(Self{limit:Limit{node:limit.id,maximum:limit.voices.maximum_instances,kill_newest:limit.voices.kill_newest},base,offset,radius:bank.maximum_radius(sound)?})
    }
    pub fn priority(&self,owner:&super::audio_distance::Emitter,listeners:&super::audio_listener::Listeners)->Result<Priority> {
        struct Spatial<'a>{owner:&'a super::audio_distance::Emitter,listeners:&'a super::audio_listener::Listeners}
        impl super::audio_sound::Spatial for Spatial<'_> {
            fn object_scale(&mut self)->Result<f32>{Ok(self.owner.radius_scale())}
            fn distance(&mut self)->Result<f32>{self.owner.minimum_distance(&self.listeners.distance_records())}
        }
        Ok(super::audio_sound::spatial_priority(self.base,self.offset,self.radius,&mut Spatial{owner,listeners})?.0)
    }
}
/// Call with oldest voices first, as in the native active list. Scope and
/// stopping-voice exclusion are applied by the caller before this kernel.
pub fn victim<T:Copy>(incoming:f32,kill_newest:bool,voices:impl IntoIterator<Item=(T,f32)>)->Option<T> {
    let mut lowest=101.;let mut selected=None;
    for (id,priority) in voices {
        if priority<lowest || (kill_newest && priority==lowest) {lowest=priority;selected=Some(id);}
    }
    if incoming<lowest || (kill_newest && incoming==lowest) {None}else {selected}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_priority_selection_owner_scope_stopping_and_ties()->Result<()> {
        let data:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_limits.json"))?;
        for case in data["cases"].as_array().unwrap() {
            let owner=case["owner"].as_u64().unwrap();let group=case["group"].as_u64().unwrap();
            let voices=case["voices"].as_array().unwrap().iter().filter(|v|!v["stopping"].as_bool().unwrap() && (owner==0 || v["owner"].as_u64()==Some(owner)) && (group==0 || v["matches"].as_bool().unwrap())).map(|v|(v["id"].as_u64().unwrap(),v["priority"].as_f64().unwrap() as f32));
            assert_eq!(victim(case["incoming"].as_f64().unwrap() as f32,case["kill_newest"].as_bool().unwrap(),voices),case["selected"].as_u64(),"{case}");
        }Ok(())
    }
}
