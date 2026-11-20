//! All eight graph link passes475fe0..4762f4/477448/4774e4.
//! Addresses are portable 32-bit data references, never executable pointers.
use anyhow::{Context as _,Result};
use std::collections::BTreeMap;
use super::{ai_graph_admission::{Admission,Listener},ai_graph_handles::State,ai_graph_loading::Loader,ai_graph_linking::{Backend,Pass},ai_graph_points::{find_tables,external_lookup,Table}};
#[derive(Clone,Copy,Debug,serde::Serialize,serde::Deserialize)]
pub struct View {
#[serde(default,skip_serializing_if="Option::is_none")] pub auxiliary:Option<super::ai_graph_normalization::Auxiliary>,
#[serde(default,skip_serializing_if="Option::is_none")] pub payload:Option<u32>,
#[serde(default,skip_serializing_if="Option::is_none")] pub edges:Option<u32>,
#[serde(default,skip_serializing_if="Option::is_none")] pub edge_stride:Option<u32>,
pub header:u32,pub points:u32,pub external:u32,pub external_count:u32,pub stride:u32}
#[derive(Clone,Copy,Debug,serde::Serialize,serde::Deserialize)]
pub struct Write {pub address:u32,pub value:u32}
#[derive(Default,Debug,serde::Serialize,serde::Deserialize)]
pub struct Memory {pub regions:BTreeMap<u32,Vec<u8>>,pub resources:BTreeMap<u32,View>,pub stride:u32,pub journal:BTreeMap<u32,Write>}
impl Memory {
    /// Fixed-length native memset on an owned mapped allocation. A zero-byte
    ///request does not dereference its address, including null.
    pub(super) fn fill(&mut self,address:u32,count:u32,value:u8)->Result<()> {
        if count==0{return Ok(())}
        let (&base,bytes)=self.regions.range_mut(..=address).next_back().context("Graph fill destination is unmapped")?;
        let at=(address-base) as usize;let end=at.checked_add(count as usize).context("Graph fill range overflow")?;
        bytes.get_mut(at..end).context("Graph fill destination is truncated")?.fill(value);Ok(())
    }
    fn span(&self,address:u32)->Result<&[u8]> {
        let (&base,bytes)=self.regions.range(..=address).next_back().context("Graph data reference is unmapped")?;
        bytes.get((address-base) as usize..).context("Graph data reference exceeds its allocation")
    }
    pub(super) fn byte(&self,address:u32)->Result<u8>{self.span(address)?.first().copied().context("Graph byte is truncated")}
    pub(super) fn write_byte(&mut self,address:u32,value:u8)->Result<()> {
        let (&base,bytes)=self.regions.range_mut(..=address).next_back().context("Graph byte destination is unmapped")?;
        *bytes.get_mut((address-base) as usize).context("Graph byte destination is truncated")?=value;Ok(())
    }
    pub(super) fn word(&self,address:u32)->Result<u32>{Ok(u32::from_be_bytes(self.span(address)?.get(..4).context("Graph word is truncated")?.try_into()?))}
    pub(super) fn write(&mut self,address:u32,value:u32)->Result<()> {
        let (&base,bytes)=self.regions.range_mut(..=address).next_back().context("Graph link destination is unmapped")?;
        let at=(address-base) as usize;let end=at.checked_add(4).context("Graph link range overflow")?;
        bytes.get_mut(at..end).context("Graph link destination is truncated")?.copy_from_slice(&value.to_be_bytes());Ok(())
    }
    pub(super) fn swap_halfword(&mut self,address:u32)->Result<()> {
        let (&base,bytes)=self.regions.range_mut(..=address).next_back().context("Graph halfword is unmapped")?;
        let at=(address-base) as usize;let end=at.checked_add(2).context("Graph halfword range overflow")?;
        bytes.get_mut(at..end).context("Graph halfword is truncated")?.swap(0,1);Ok(())
    }
    pub(super) fn view(&self,id:u32)->Result<View>{self.resources.get(&id).copied().context("Graph link resource view is absent")}
    fn find(&self,id:u32,key:u32,dynamic:bool,external_only:bool)->Result<Option<u32>> {
        let v=self.view(id)?;let stride=if dynamic {v.stride}else{32};
        if external_only {
            if v.external==0 {return Ok(None)}
            return Ok(external_lookup(self.span(v.external)?,stride,v.external_count,key)?.map(|offset|v.external.wrapping_add(offset)))
        }
        let location=self.locate(id,key,dynamic)?;
        Ok(location.map(|l|match l.table {Table::Internal=>v.points,Table::External=>v.external}.wrapping_add(l.offset)))
    }
    pub(super) fn locate(&self,id:u32,key:u32,dynamic:bool)->Result<Option<super::ai_graph_points::Location>> {
        let v=self.view(id)?;let total=self.word(v.header)?;
        find_tables(self.span(v.points)?,if v.external==0 {None}else{Some(self.span(v.external)?)},if dynamic {v.stride}else{32},total.wrapping_sub(v.external_count),v.external_count,key)
    }

    pub fn run(&mut self,admission:&mut Admission,a:u32,b:u32,pass:Pass)->Result<()> {
        use Pass::*;
        match pass {
            DynamicToFixed|FixedToCoarser|FixedToDynamicForward|FixedToFinerForward=>{
                let v=self.view(a)?;let mut at=v.points;let total=self.word(v.header)?;
                let field=if matches!(pass,FixedToDynamicForward|FixedToFinerForward){20}else{16};
                for _ in 0..total {
                    let key=self.word(at.wrapping_add(field))?;
                    if key&0x80000000!=0 {
                        if let Some(found)=self.find(b,key,matches!(pass,FixedToDynamicForward),false)? {self.write(at.wrapping_add(field),found)?;}
                    }
                    at=at.wrapping_add(if matches!(pass,DynamicToFixed){self.stride}else{32});
                }
            }
            DynamicPeers|FixedPeers=>{
                let v=self.view(a)?;let mut at=v.external;let field=if matches!(pass,DynamicPeers){20}else{24};
                let mut i=0i32;
                while i<(v.external_count as i32) {
                    if self.word(at.wrapping_add(field))?==0 {
                        let key=self.word(at)?;
                        if let Some(found)=self.find(b,key,matches!(pass,DynamicPeers),true)? {self.write(at.wrapping_add(field),found)?;}
                    }
                    at=at.wrapping_add(if matches!(pass,DynamicPeers){self.stride}else{32});i+=1;
                }
            }
            FixedToDynamicBack|FixedToFinerBack=>{
                let v=self.view(b)?;let mut at=v.points;let total=self.word(v.header)?;
                for _ in 0..total {
                    let key=self.word(at.wrapping_add(16))?;
                    if key&0x80000000!=0 {
                        if let Some(found)=self.find(a,key&0x7fffffff,false,false)? {
                            // This is the original journal's explicit4096-entry capacity.
                            if admission.word82e4>=4096 {return Ok(())}
                            self.journal.insert(admission.word82e4,Write{address:at.wrapping_add(16),value:found});
                            admission.word82e4=admission.word82e4.wrapping_add(1);
                        }
                    }
                    at=at.wrapping_add(if matches!(pass,FixedToDynamicBack){self.stride}else{32});
                }
            }
        }
        Ok(())
    }
    ///476f38..68 applies staged back-links after the fixed finalization steps.
    pub fn apply_journal(&mut self,admission:&mut Admission)->Result<()> {
        for index in 0..admission.word82e4 {
            let entry=*self.journal.get(&index).context("Graph back-link journal entry is absent")?;
            self.write(entry.address,entry.value)?;
        }
        admission.word82e4=0;Ok(())
    }
}
/// Finalization and World notifications are still required; pass execution is concrete.
pub trait Lifecycle {
    fn finalize(&mut self,memory:&mut Memory,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>)->Result<()>;
    fn arrived(&mut self,memory:&mut Memory,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>,listener:Listener)->Result<()>;
    fn clock_bits(&mut self,memory:&mut Memory,admission:&mut Admission,loader:&mut Loader,state:&mut State)->Result<u32>;
}
pub struct Concrete<L>{pub memory:Memory,pub lifecycle:L}
impl<L:Lifecycle> Backend for Concrete<L> {
    fn pass(&mut self,a:&mut Admission,_:&mut Loader,_:&mut State,x:u32,y:u32,pass:Pass)->Result<()>{self.memory.run(a,x,y,pass)}
    fn finalize(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State,r:Option<u32>)->Result<()>{self.lifecycle.finalize(&mut self.memory,a,l,s,r)}
    fn arrived(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State,r:Option<u32>,listener:Listener)->Result<()>{self.lifecycle.arrived(&mut self.memory,a,l,s,r,listener)}
    fn clock_bits(&mut self,a:&mut Admission,l:&mut Loader,s:&mut State)->Result<u32>{self.lifecycle.clock_bits(&mut self.memory,a,l,s)}
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::Value;
    fn pass(address:u32)->Pass {use Pass::*;match address {0x475fe0=>DynamicToFixed,0x476074=>FixedToCoarser,0x476110=>DynamicPeers,0x476194=>FixedPeers,0x476220=>FixedToDynamicBack,0x4762f4=>FixedToFinerBack,0x477448=>FixedToDynamicForward,0x4774e4=>FixedToFinerForward,_=>unreachable!()}}
    #[test] fn original_all_eight_link_passes_aliases_and_deferred_writes_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_link_passes.json"))?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let mut memory:Memory=serde_json::from_value(c["memory"].clone())?;
            let mut admission:Admission=serde_json::from_value(c["admission"].clone())?;
            memory.run(&mut admission,10,11,pass(c["pass_address"].as_u64().unwrap() as u32))?;
            if c["apply"].as_bool().unwrap(){memory.apply_journal(&mut admission)?;}
            assert_eq!(serde_json::to_value(memory)?,case["memory"],"{c}");
            assert_eq!(serde_json::to_value(admission)?,case["admission"],"{c}");
        }
        Ok(())
    }
    #[test] fn malformed_graph_references_are_errors_without_publishing_a_link()->Result<()> {
        let mut memory=Memory{regions:BTreeMap::from([(100,vec![0;4])]),resources:BTreeMap::new(),stride:32,journal:BTreeMap::new()};
        assert!(memory.word(0).is_err());assert!(memory.word(101).is_err());assert!(memory.write(101,10).is_err());
        assert_eq!(memory.regions[&100],vec![0;4]);
        let mut admission=Admission{index154:0,index15c:0,word82e4:1,changed14:0,listeners:Vec::new()};
        assert!(memory.run(&mut admission,10,11,Pass::FixedPeers).is_err());
        assert!(memory.apply_journal(&mut admission).is_err());assert_eq!(admission.word82e4,1);
        Ok(())
    }
}
