//! Native graph adjacency477214 and class/level link dispatch476f88.
use anyhow::{Context as _,Result};
use std::collections::BTreeMap;
use super::{ai_graph_admission::{self,Admission,Listener},ai_graph_handles::State,ai_graph_loading::Loader,ai_graph_queries::{Metadata,Resources}};
#[derive(Debug,serde::Deserialize)]
pub struct Directory {pub float_mode:u32,pub bounds:BTreeMap<u32,[u32;6]>}
fn level(loader:&Loader,state:&State,id:u32)->Result<u32>{let key=state.resources.get(&id).context("Link resource is absent")?.word1c;Ok(loader.records.get(&key).context("Link directory record is absent")?.word1c)}
fn near(a:u32,b:u32)->bool{a.wrapping_sub(b).wrapping_add(1)<=2}
impl Directory {
    pub fn adjacent(&self,loader:&Loader,state:&State,a:u32,b:u32)->Result<bool> {
        let al=level(loader,state,a)?;let bl=level(loader,state,b)?;
        if !near(al,bl) {return Ok(false)}
        let aw=self.bounds.get(&state.resources[&a].word1c).context("First graph region bounds are absent")?;
        if aw[0]==99999 {return Ok(true)}
        let bw=self.bounds.get(&state.resources[&b].word1c).context("Second graph region bounds are absent")?;
        if bw[0]==99999 {return Ok(true)}
        if al==bl {
            if self.float_mode==0 {return Ok((0..3).all(|i|near(aw[i],bw[i])))}
            // PPC bge/ble accept unordered comparisons here: preserve NaN behavior.
            for i in [0,2,4] {
                if f32::from_bits(aw[i+1])<f32::from_bits(bw[i]) || f32::from_bits(aw[i])>f32::from_bits(bw[i+1]) {return Ok(false)}
            }
            return Ok(true)
        }
        let (fine,coarse)=if al>bl {(bw,aw)}else{(aw,bw)};
        Ok((0..3).all(|i|near(coarse[i],((fine[i] as i32)>>1) as u32)))
    }
}
#[derive(Clone,Copy,Debug)]
#[repr(u32)]
pub enum Pass {DynamicToFixed=0x475fe0,FixedToCoarser=0x476074,DynamicPeers=0x476110,FixedPeers=0x476194,FixedToDynamicBack=0x476220,FixedToFinerBack=0x4762f4,FixedToDynamicForward=0x477448,FixedToFinerForward=0x4774e4}
/// All pass bodies, finalization, listeners and context clock are required.
pub trait Backend {
    fn pass(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,a:u32,b:u32,pass:Pass)->Result<()>;
    fn finalize(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>)->Result<()>;
    fn arrived(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>,listener:Listener)->Result<()>;
    fn clock_bits(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State)->Result<u32>;
}
pub struct Native<H> {pub directory:Directory,pub metadata:Metadata,pub backend:H}
impl<H:Backend> Native<H> {
    pub fn link(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,a:u32,b:u32)->Result<()> {
        // Both identified vtables have constant, side-effect-free kind getters.
        // No pass runs before this decision; callbacks cannot change the kind mid-decision.
        let ak=self.metadata.kind(state,a)?;let bk=self.metadata.kind(state,b)?;
        let passes:&[Pass]=match (ak,bk) {
            (1,0)=>&[Pass::DynamicToFixed],(1,1)=>&[Pass::DynamicPeers],
            (0,1)=>&[Pass::FixedToDynamicForward,Pass::FixedToDynamicBack],
            (0,0)=>{let al=level(loader,state,a)?;let bl=level(loader,state,b)?;
                if al<bl {&[Pass::FixedToCoarser]}else if al>bl {&[Pass::FixedToFinerForward,Pass::FixedToFinerBack]}else{&[Pass::FixedPeers]}},
            _=>anyhow::bail!("Unidentified graph class kind"),
        };
        for &pass in passes {self.backend.pass(admission,loader,state,a,b,pass)?;}
        Ok(())
    }
}
impl<H:Backend> ai_graph_admission::Host for Native<H> {
    fn adjacent(&mut self,_:&mut Admission,l:&mut Loader,s:&mut State,a:Option<u32>,b:u32)->Result<u8>{Ok(u8::from(self.directory.adjacent(l,s,a.context("Adjacency received a null resource")?,b)?))}
    fn link(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State,x:Option<u32>,y:u32)->Result<()>{Native::link(self,a,l,s,x.context("Link received a null resource")?,y)}
    fn finalize(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State,r:Option<u32>)->Result<()>{self.backend.finalize(a,l,s,r)}
    fn arrived(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State,r:Option<u32>,listener:Listener)->Result<()>{self.backend.arrived(a,l,s,r,listener)}
    fn clock_bits(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State)->Result<u32>{self.backend.clock_bits(a,l,s)}
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::{Value,json};
    use super::super::ai_graph_queries::{Class,ResourceMetadata};
    const LINK_CASES:&str=include_str!("../../tools/blotworx/fixtures/ai_graph_linking.json");
    struct Fixture {trace:Vec<Value>}
    impl Fixture {fn snapshot(a:&Admission,s:&State)->Value {json!([s.tracked_resource,s.tracked_resource.map(|id|s.resources[&id].state),s.tracked158,s.tracked160,a.index154,a.index15c])}}
    impl Backend for Fixture {
        fn pass(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,x:u32,y:u32,pass:Pass)->Result<()>{self.trace.push(json!(["pass",pass as u32,x,y,Self::snapshot(a,s)]));Ok(())}
        fn finalize(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,r:Option<u32>)->Result<()>{self.trace.push(json!(["finalize",r,Self::snapshot(a,s)]));Ok(())}
        fn arrived(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,r:Option<u32>,listener:Listener)->Result<()>{self.trace.push(json!(["arrived",r,listener,Self::snapshot(a,s)]));Ok(())}
        fn clock_bits(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State)->Result<u32>{self.trace.push(json!(["clock",Self::snapshot(a,s)]));Ok(0x40000000)}
    }
    #[test] fn original_adjacency_grid_levels_sentinel_and_float_bounds_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_adjacency.json"))?;
        let link:Value=serde_json::from_str(LINK_CASES)?;let base=&link["cases"][0]["input"];
        let mut loader:Loader=serde_json::from_value(base["loader"].clone())?;let state:State=serde_json::from_value(base["state"].clone())?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];loader.records.get_mut(&1).unwrap().word1c=c["a"]["level"].as_u64().unwrap() as u32;
            loader.records.get_mut(&2).unwrap().word1c=c["b"]["level"].as_u64().unwrap() as u32;
            let directory=Directory{float_mode:c["mode"].as_u64().unwrap() as u32,bounds:BTreeMap::from([(1,serde_json::from_value(c["a"]["words"].clone())?),(2,serde_json::from_value(c["b"]["words"].clone())?)])};
            assert_eq!(json!(directory.adjacent(&loader,&state,10,11)?),case["result"],"{c}");
        }
        Ok(())
    }
    #[test] fn original_admission_through_concrete_adjacency_and_class_dispatch_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(LINK_CASES)?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let mut a:Admission=serde_json::from_value(c["admission"].clone())?;
            let mut state:State=serde_json::from_value(c["state"].clone())?;let mut loader:Loader=serde_json::from_value(c["loader"].clone())?;
            let mut metadata=Metadata::default();
            for (k,v) in c["geometry"]["kinds"].as_object().unwrap() {metadata.resources.insert(k.parse()?,ResourceMetadata{class:if v==0 {Class::Abstract}else{Class::Concrete},header:None});}
            let mut native=Native{directory:serde_json::from_value(c["geometry"]["directory"].clone())?,metadata,backend:Fixture{trace:Vec::new()}};
            a.advance(&mut loader,&mut state,&mut native)?;
            assert_eq!(native.backend.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(a)?,case["admission"],"{c}");assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
        }
        Ok(())
    }
}
