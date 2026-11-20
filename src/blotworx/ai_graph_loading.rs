//! Request selection478440, load4784fc/4785c0, start4783ac and failure478168.
//! File callbacks remain mandatory; this module does not fabricate completed IO.
use anyhow::{Context as _,Result};
use std::collections::BTreeMap;
use super::{ai_graph_handles::State,ai_graph_resource_group::{Group,Inputs},ai_graph_buffers::{Bindings,Pointer}};
#[derive(Clone,Copy,Debug,serde::Serialize,serde::Deserialize)]
pub struct Record {pub id:u32,pub word1c:u32,pub size20:u32,pub offset24:u32,pub word28:u32,pub word2c:u32}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Loader {
    pub source:usize,pub direct_mode:u8,pub name:Vec<u8>,pub extra_records:u32,
    pub directory:Vec<u32>,pub records:BTreeMap<u32,Record>,pub bad:Vec<u8>,
    pub groups:[Option<Group>;2], //3c then38
}
#[derive(Debug,serde::Serialize)]
pub struct Direct {pub source:usize,pub resource:u32,pub id:u32,pub word28:u32,pub word2c:u32}
#[derive(Debug,serde::Serialize)]
pub struct Packed {pub source:usize,pub resource:u32,pub offset:u32,pub bytes:u32,pub destination:Pointer}
pub trait Io {
    fn direct(&mut self,loader:&mut Loader,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,request:Direct,output:&mut Pointer)->Result<u32>;
    fn packed(&mut self,loader:&mut Loader,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,request:Packed)->Result<u32>;
}
impl Loader {
    fn record(&self,key:u32)->Result<&Record>{self.records.get(&key).context("Graph directory record is absent")}
    pub fn has_state2(&self,state:&State)->Result<bool> {
        for group in self.groups.iter().flatten() {for id in &group.resources {
            if state.resources.get(id).context("Graph loading group resource is absent")?.state==2 {return Ok(true)}
        }}
        Ok(false)
    }
    pub fn next_request(&self,state:&State)->Result<Option<u32>> {
        let mut selected=None;let mut priority=i32::MIN;
        for group in self.groups.iter().flatten() {for &id in &group.resources {
            let r=state.resources.get(&id).context("Graph request resource is absent")?;
            if r.state==1 && (selected.is_none() || r.priority as i32>priority) {selected=Some(id);priority=r.priority as i32;}
        }}
        Ok(selected)
    }
    pub fn fail(&mut self,state:&mut State,resource:u32)->Result<()> {
        let key=state.resources.get(&resource).context("Failed graph resource is absent")?.word1c;
        let id=self.record(key)?.id;
        for index in 0..self.directory.len() {
            if self.record(self.directory[index])?.id==id {*self.bad.get_mut(index).context("Graph failure flags are truncated")?=1;}
        }
        state.resources.get_mut(&resource).unwrap().state=0;
        for handle in &state.pool {
            let n=state.nodes.get(handle).context("Graph failure handle is absent")?;
            if n.free==0 && n.word18!=0 && self.record(n.word18)?.id==id {state.nodes.get_mut(handle).unwrap().byte38=1;}
        }
        Ok(())
    }
    pub fn direct(&mut self,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,resource:u32,io:&mut impl Io)->Result<u32> {
        let key=state.resources.get(&resource).context("Direct graph resource is absent")?.word1c;
        let record=*self.record(key)?;let mut output=Pointer::default();
        let request=Direct{source:self.source,resource,id:record.id,word28:record.word28,word2c:record.word2c};
        inputs.resources.get_mut(&resource).context("Direct graph IO token is absent")?.word24=0;
        let result=io.direct(self,state,inputs,bindings,request,&mut output)?;
        if result==u32::MAX {self.fail(state,resource)?;}
        else if !output.is_null() {
            let binding=bindings.resources.get_mut(&resource).context("Direct graph binding is absent")?;
            binding.payload=output.add(16);binding.bind();
            // The metadata pointer is retained, but its size word is read after IO.
            state.resources.get_mut(&resource).context("Direct callback removed its graph resource")?.size18=Some(self.record(key)?.size20);
        }
        Ok(result)
    }
    pub fn packed(&mut self,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,resource:u32,io:&mut impl Io)->Result<u32> {
        let key=state.resources.get(&resource).context("Packed graph resource is absent")?.word1c;
        let record=*self.record(key)?;let skip=u32::from(record.word1c!=0);
        let count=u32::try_from(self.directory.len()).context("Graph directory exceeds native count domain")?;
        let offset=record.offset24.wrapping_add(self.extra_records.wrapping_mul(8)).wrapping_add(skip).wrapping_add(count.wrapping_mul(48)).wrapping_add(156);
        let destination=bindings.resources.get(&resource).context("Packed graph binding is absent")?.payload;
        let request=Packed{source:self.source,resource,offset,bytes:record.size20.wrapping_sub(skip),destination};
        inputs.resources.get_mut(&resource).context("Packed graph IO token is absent")?.word24=0;
        let result=io.packed(self,state,inputs,bindings,request)?;
        if result==u32::MAX {self.fail(state,resource)?;}
        Ok(result)
    }
    pub fn start_next(&mut self,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,io:&mut impl Io)->Result<u32> {
        let Some(resource)=self.next_request(state)? else{return Ok(0)};
        //51e224 is the empty string; native C-string comparison ignores suffix
        // bytes after the initial NUL, without trimming spaces or paths.
        let direct=self.direct_mode!=0 && self.name.first().is_none_or(|&c|c==0);
        let result=if direct {self.direct(state,inputs,bindings,resource,io)?} else {self.packed(state,inputs,bindings,resource,io)?};
        if result!=u32::MAX {state.resources.get_mut(&resource).context("IO callback removed its request resource")?.state=2;}
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>}
    impl Fixture<'_> {
        fn complete(&self,loader:&mut Loader,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,resource:u32)->Result<u32> {
            inputs.resources.get_mut(&resource).unwrap().word24=self.c["token"].as_u64().unwrap_or(55) as u32;
            if let Some(size)=self.c["record_size"].as_u64(){loader.records.get_mut(&1).unwrap().size20=size as u32;}
            if let Some(key)=self.c["change_metadata"].as_u64(){state.resources.get_mut(&resource).unwrap().word1c=key as u32;}
            if let Some(value)=self.c["change_state"].as_u64(){state.resources.get_mut(&resource).unwrap().state=value as u32;}
            if let Some(value)=self.c["change_extra"].as_u64(){loader.extra_records=value as u32;}
            if self.c["change_payload"].is_object(){bindings.resources.get_mut(&resource).unwrap().payload=serde_json::from_value(self.c["change_payload"].clone())?;}
            Ok(self.c["status"].as_u64().unwrap_or(0) as u32)
        }
    }
    impl Io for Fixture<'_> {
        fn direct(&mut self,loader:&mut Loader,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,request:Direct,output:&mut Pointer)->Result<u32> {
            self.trace.push(json!(["direct",&request,inputs.resources[&request.resource].word24]));
            *output=serde_json::from_value(self.c["output"].clone())?;
            self.complete(loader,state,inputs,bindings,request.resource)
        }
        fn packed(&mut self,loader:&mut Loader,state:&mut State,inputs:&mut Inputs,bindings:&mut Bindings,request:Packed)->Result<u32> {
            self.trace.push(json!(["packed",&request,inputs.resources[&request.resource].word24]));
            self.complete(loader,state,inputs,bindings,request.resource)
        }
    }
    #[test]
    fn original_graph_load_selection_direct_header_packed_ranges_and_failure_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_loading.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut loader:Loader=serde_json::from_value(c["loader"].clone())?;
            let mut state:State=serde_json::from_value(c["state"].clone())?;
            let mut inputs:Inputs=serde_json::from_value(c["inputs"].clone())?;
            let mut bindings:Bindings=serde_json::from_value(c["bindings"].clone())?;
            let mut io=Fixture{c,trace:Vec::new()};
            let result=match c["op"].as_str().unwrap(){
                "has"=>json!(loader.has_state2(&state)?),"select"=>json!(loader.next_request(&state)?),
                "direct"=>json!(loader.direct(&mut state,&mut inputs,&mut bindings,10,&mut io)?),
                "packed"=>json!(loader.packed(&mut state,&mut inputs,&mut bindings,10,&mut io)?),
                "start"=>json!(loader.start_next(&mut state,&mut inputs,&mut bindings,&mut io)?),
                "fail"=>{loader.fail(&mut state,10)?;Value::Null},_=>unreachable!()
            };
            assert_eq!(result,case["result"],"{c}");assert_eq!(io.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(loader)?,case["loader"],"{c}");assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
            assert_eq!(serde_json::to_value(inputs)?,case["inputs"],"{c}");assert_eq!(serde_json::to_value(bindings)?,case["bindings"],"{c}");
        }
        Ok(())
    }
}
