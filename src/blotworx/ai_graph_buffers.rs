//! Group buffer ctor482530, bind4825f0 and destructor482554.
//! Allocation/free/indexed access are mandatory services; no source file offset is inferred.
use anyhow::{Context as _,Result};
use std::collections::BTreeMap;
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
pub struct Pointer {pub allocation:Option<usize>,pub offset:u32}
impl Pointer {
    pub fn is_null(self)->bool {self.allocation.is_none() && self.offset==0}
    pub fn add(self,bytes:u32)->Self {Self{offset:self.offset.wrapping_add(bytes),..self}}
}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct State {pub base_vtable:bool,pub borrowed:u8,pub buffer:Pointer,pub count:u32}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Binding {pub payload:Pointer,pub header:Pointer,pub points:Pointer}
impl Binding {pub fn bind(&mut self){self.header=self.payload;self.points=self.payload.add(8);}}
#[derive(Default,Debug,serde::Serialize,serde::Deserialize)]
pub struct Bindings {pub resources:BTreeMap<u32,Binding>}
pub trait Host {
    fn allocate(&mut self,state:&mut State,bytes:u32)->Result<Pointer>;
    fn free(&mut self,state:&mut State,pointer:Pointer)->Result<()>;
    fn resource_at(&mut self,state:&mut State,index:u32)->Result<u32>;
}
impl State {
    pub fn construct(count:u32)->Self {Self{base_vtable:true,borrowed:0,buffer:Pointer::default(),count}}
    pub fn bind(&mut self,bindings:&mut Bindings,stride:u32,provided:Pointer,reference:Pointer,host:&mut impl Host)->Result<Pointer> {
        if !provided.is_null() && !reference.is_null() {
            self.buffer=provided;
            // Equality does not clear a previously set ownership flag.
            if provided!=reference {self.borrowed=1;}
        } else if stride!=0 && self.count!=0 {
            let bytes=self.count.wrapping_mul(stride);
            self.buffer=host.allocate(self,bytes)?;
        } else {self.buffer=Pointer::default();}
        let mut cursor=self.buffer;let mut index=0;
        while index<self.count {
            let resource=host.resource_at(self,index)?;
            bindings.resources.get_mut(&resource).context("Graph payload binding resource is absent")?.payload=cursor;
            // Native invokes the indexed getter twice. The second may return
            // a different resource; bind that object's existing payload field.
            let resource=host.resource_at(self,index)?;
            let binding=bindings.resources.get_mut(&resource).context("Graph header binding resource is absent")?;
            binding.bind();
            if stride!=0 {cursor=cursor.add(stride);}
            index+=1;
        }
        Ok(self.buffer)
    }
    pub fn destroy(&mut self,this:Pointer,delete_flag:i32,host:&mut impl Host)->Result<Pointer> {
        if this.is_null(){return Ok(this)};
        self.base_vtable=true;
        let buffer=self.buffer;
        if !buffer.is_null() && self.borrowed==0 {host.free(self,buffer)?;}
        self.buffer=Pointer::default();
        if delete_flag>0 {host.free(self,this)?;}
        Ok(this)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,getters:usize}
    impl Fixture<'_> {
        fn mutate(&self,state:&mut State,key:&str) {
            if let Some(v)=self.case[key].as_object() {
                if let Some(count)=v.get("count"){state.count=count.as_u64().unwrap() as u32;}
                if let Some(flag)=v.get("borrowed"){state.borrowed=flag.as_u64().unwrap() as u8;}
                if let Some(buffer)=v.get("buffer"){state.buffer=serde_json::from_value(buffer.clone()).unwrap();}
            }
        }
    }
    impl Host for Fixture<'_> {
        fn allocate(&mut self,state:&mut State,bytes:u32)->Result<Pointer> {
            self.trace.push(json!(["allocate",bytes]));
            let pointer=if self.case["allocated"].as_bool().unwrap_or(true){Pointer{allocation:Some(100),offset:0}}else{Pointer::default()};
            if self.case["observer"].as_bool().unwrap_or(false){self.trace.push(json!(["observer",pointer,bytes,123]));}
            self.mutate(state,"allocate_mutation");Ok(pointer)
        }
        fn free(&mut self,state:&mut State,pointer:Pointer)->Result<()> {
            self.trace.push(json!(["free",pointer,state]));self.mutate(state,"free_mutation");Ok(())
        }
        fn resource_at(&mut self,state:&mut State,index:u32)->Result<u32> {
            let values=self.case["getters"].as_array();let resource=values.and_then(|v|v.get(self.getters)).and_then(Value::as_u64).unwrap_or(10+index as u64) as u32;
            self.getters+=1;self.trace.push(json!(["get",index,resource]));
            if self.getters==1 {self.mutate(state,"getter_mutation");}
            Ok(resource)
        }
    }
    #[test]
    fn original_buffer_ownership_binding_and_destruction_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_buffers.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut state:State=serde_json::from_value(c["state"].clone())?;
            let mut bindings:Bindings=serde_json::from_value(c["bindings"].clone())?;
            let mut host=Fixture{case:c,trace:Vec::new(),getters:0};
            let result=match c["op"].as_str().unwrap(){
                "construct"=>{state=State::construct(c["count"].as_u64().unwrap() as u32);Pointer{allocation:Some(1),offset:0}},
                "bind"=>state.bind(&mut bindings,c["stride"].as_u64().unwrap() as u32,serde_json::from_value(c["provided"].clone())?,serde_json::from_value(c["reference"].clone())?,&mut host)?,
                "destroy"=>state.destroy(serde_json::from_value(c["this"].clone())?,c["delete_flag"].as_i64().unwrap() as i32,&mut host)?,_=>unreachable!()
            };
            assert_eq!(json!(result),case["result"],"{c}");assert_eq!(host.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");assert_eq!(serde_json::to_value(bindings)?,case["bindings"],"{c}");
        }
        Ok(())
    }
}
