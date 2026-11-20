//! Worker-side global SIS timing and PBI mute-map gain8035a1b0..8035a2a4.
use super::{audio_control::ParameterNodeControl,audio_nodes::Nodes};
use anyhow::{ensure,Result};
#[derive(Clone,Debug)]
pub struct Command {pub node:u32,pub muting:bool,pub curve:u8,pub duration:i32,pub due:u32}
pub struct Runtime {pub tick:u32,nodes:Nodes,pending:std::collections::VecDeque<Command>}
impl Runtime {
    pub fn new(ids:impl IntoIterator<Item=u32>)->Result<Self> {let mut nodes=Nodes::new()?;nodes.register_mixers(ids);Ok(Self{tick:0,nodes,pending:Default::default()})}
    pub fn define(&mut self,ids:impl IntoIterator<Item=u32>){self.nodes.register_mixers(ids);}
    pub fn enqueue(&mut self,command:Command){let at=self.pending.partition_point(|entry|entry.due<=command.due);self.pending.insert(at,command);}
    pub fn active(&self)->bool {!self.pending.is_empty() || self.nodes.active()}
    pub fn initial(&self,lineage:&[u32])->Vec<(u32,f32)> {lineage.iter().filter_map(|&id|self.nodes.byte(id).filter(|&value|value!=255).map(|value|(id,value as f32))).collect()}
    pub fn advance(&mut self)->Result<Vec<(u32,f32)>> {
        while self.pending.front().is_some_and(|command|command.due<=self.tick) {
            let command=self.pending.pop_front().unwrap();
            if let Some(node)=self.nodes.acquire_node(command.node)? {
                let result=if command.muting {self.nodes.mute_global(node,command.curve,command.duration)}else{self.nodes.unmute_global(node,command.curve,command.duration)};
                self.nodes.release_node(node);result?;
            }
        }
        self.tick=self.tick.wrapping_add(1);self.nodes.update(self.tick)?;
        Ok(self.nodes.notifications())
    }
}
pub fn gain_db(base:f32,values:&[(u32,f32)])->Result<f32> {
    gain_db_local(base,values,255)
}
pub fn gain_db_local(base:f32,values:&[(u32,f32)],local:u8)->Result<f32> {
    ensure!(base.is_finite() && values.iter().all(|(_,value)|value.is_finite()),"Nonfinite mute-map value");
    let mut weight=1.;
    for (_,value) in values {weight=weight*value;weight/=255.;}
    if local!=255 {weight*=local as f32;weight/=255.;}
    Ok(-96.3+(base.min(0.)-(-96.3))*weight)
}
pub fn notify(values:&mut Vec<(u32,f32)>,id:u32,value:f32) {
    if let Some(index)=values.iter().position(|entry|entry.0==id) {
        if value==255. {values.remove(index);}else{values[index].1=value;}
    } else if value!=255. {values.push((id,value));}
}
#[cfg(debug_assertions)]
pub fn validate()->Result<()> {
    let data:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_mute_mix.json"))?;
    for case in data["callbacks"].as_array().unwrap() {
        let mut mute=super::audio_transition::Mute::default();mute.apply_db(f32::from_bits(case["db_bits"].as_u64().unwrap() as u32))?;
        ensure!(mute.byte==case["byte"].as_u64().unwrap() as u8 && mute.callback_value.to_bits()==case["value_bits"].as_u64().unwrap() as u32,"Native mute callback conversion mismatch");
    }
    for case in data["gain"].as_array().unwrap() {
        let values=case["values_bits"].as_array().unwrap().iter().enumerate().map(|(id,value)|(id as u32,f32::from_bits(value.as_u64().unwrap() as u32))).collect::<Vec<_>>();
        ensure!(gain_db(f32::from_bits(case["base_bits"].as_u64().unwrap() as u32),&values)?.to_bits()==case["gain_bits"].as_u64().unwrap() as u32,"Native mute-map gain mismatch: {case}");
    }
    println!("PASS:5 native fractional SIS callbacks and24 exact-bit ordered mute-map gain cases");Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fade_progress_and_delayed_unmute_use_audio_ticks()->Result<()> {
        let mut state=Runtime::new([7])?;
        state.enqueue(Command{node:7,muting:true,curve:4,duration:30,due:0});
        let mut values=vec![];
        for _ in 0..10 {for (id,value) in state.advance()? {notify(&mut values,id,value);}}
        // Native gain/log lookup rounding can leave a fractional callback
        // just below zero. New voices read the exact stored byte instead.
        assert_eq!(state.initial(&[7]),[(7,0.)]);
        assert!(!state.active());
        state.enqueue(Command{node:7,muting:false,curve:4,duration:0,due:15});
        for _ in 0..5 {assert!(state.advance()?.is_empty());}
        let mut endpoint=super::super::audio_transition::Mute::default();endpoint.apply_db(0.)?;
        assert_eq!(state.advance()?,[(7,endpoint.callback_value)]);
        assert_eq!(state.initial(&[7]),[(7,endpoint.byte as f32)]);Ok(())
    }
    #[test]
    fn native_map_removes_full_restore_without_reordering_others() {
        let mut values=vec![(1,90.),(2,100.),(3,110.)];notify(&mut values,2,255.);
        assert_eq!(values,[(1,90.),(3,110.)]);
    }
    #[test]
    fn original_callback_and_mute_map_cases()->Result<()> {validate()}
}
