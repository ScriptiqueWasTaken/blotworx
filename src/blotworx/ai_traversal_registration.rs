//! Tagged traversal binding487b18/48761c/4878c8 and edge ownership485b18/4859e8.
//! World release/registration and derived48/50 methods remain mandatory services.
use super::ai_graph_link_passes::Memory;
use anyhow::{ensure,Result};
use std::collections::BTreeSet;
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn at(p:u32,i:u32,stride:u32)->u32{p.wrapping_add(i.wrapping_mul(stride))}
fn half(m:&Memory,p:u32)->Result<u32>{Ok(u16::from_be_bytes([m.byte(p)?,m.byte(p.wrapping_add(1))?]) as u32)}
fn entry(m:&Memory,p:u32,i:u32)->Result<u32>{Ok(at(w(m,p,0x80)?,i,24))}
fn matches(m:&Memory,e:u32,a:u32,b:u32,g:u32)->Result<bool>{Ok(w(m,e,0)?&0x7fffffff==a&0x7fffffff && w(m,e,4)?&0x7fffffff==b&0x7fffffff && w(m,e,8)?==g)}
pub fn add_edge(m:&mut Memory,p:u32,a:u32,b:u32,g:u32)->Result<u32>{
 let count=w(m,p,0x84)?;let mut free=None;
 for i in 0..count {
  let e=entry(m,p,i)?;
  if free.is_none() && w(m,e,0)?==0 {free=Some(i);}
  if matches(m,e,a,b,g)? {let e=entry(m,p,i)?;let n=w(m,e,12)?;m.write(e.wrapping_add(12),n.wrapping_add(1))?;return Ok(1)}
 }
 let Some(i)=free else{return Ok(0)};
 for (off,value) in [(0,a),(4,b),(8,g),(12,1),(16,0),(20,0)] {let e=entry(m,p,i)?;m.write(e.wrapping_add(off),value)?;}
 Ok(1)
}
pub fn link_edge(m:&mut Memory,mut p:u32,a:u32,b:u32,g:u32,new:u32)->Result<u32>{
 let mut seen=BTreeSet::new();
 loop {
  ensure!(seen.insert(p),"Cyclic traversal edge ownership");
  let count=w(m,p,0x84)?;let mut forwarded=None;
  for i in 0..count {
   let e=entry(m,p,i)?;
   if !matches(m,e,a,b,g)? {continue}
   let next=w(m,e,16)?;
   if next!=0 {if next==new{return Ok(1)}forwarded=Some(next);break}
   m.write(e.wrapping_add(16),new)?;
   let count=w(m,new,0x84)?;
   for j in 0..count {let e=entry(m,new,j)?;if matches(m,e,a,b,g)? {m.write(e.wrapping_add(20),p)?;break}}
   return Ok(1)
  }
  if let Some(next)=forwarded {p=next}else{return Ok(0)}
 }
}
#[derive(Clone,Debug,PartialEq,serde::Serialize)]
pub struct Endpoint {pub position:[u32;3],pub attributes:[Option<u32>;16]}
pub trait Host {
 fn release_usage(&mut self,m:&mut Memory,policy:u32)->Result<()>;
 fn release_pool(&mut self,m:&mut Memory,policy:u32)->Result<()>;
 fn register18(&mut self,m:&mut Memory,policy:u32)->Result<()>;
 fn initialize48(&mut self,m:&mut Memory,policy:u32)->Result<()>;
 fn marker_slot(&mut self,m:&mut Memory)->Result<u32>;
 fn bind50(&mut self,m:&mut Memory,policy:u32,a:&Endpoint,b:&Endpoint)->Result<()>;
}
pub trait GraphHost:Host {
 fn kind(&mut self,m:&mut Memory,resource:u32)->Result<u32>;
}
/// Finalizer dispatch uses the actual manager/service/policy tables. No
/// tagged edge is considered registered through a supplied success flag.
pub struct Registered<H> {pub host:H}
impl<H:GraphHost> super::ai_graph_finalize::Host for Registered<H> {
 fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32>{self.host.kind(m,r)}
 fn tag_edge(&mut self,m:&mut Memory,c:&mut super::ai_graph_finalize::Context,d:bool,r:u32,i:u32,e:u32)->Result<()> {
  let service=w(m,w(m,c.manager,0x8534)?,0x1c)?;
  bind(m,service,c.manager,i,r,e,d,&mut self.host)?;Ok(())
 }
}
#[derive(Clone,Copy,Debug,PartialEq,serde::Serialize)]
pub struct Lookup {pub header:u32,pub index:u32,pub policy:u32}
fn select(m:&mut Memory,service:u32,g:u32,id:u32,dynamic:bool,h:&mut impl Host)->Result<Option<Lookup>> {
 let header=g.wrapping_add(0x2bc);let local=w(m,header,4)?;
 if local==0{return Ok(None)}
 let count=local.wrapping_add(w(m,service,4)?).wrapping_sub(0x4000);
 let mut selected=None;
 for i in 0..count {if w(m,w(m,header,8)?,i.wrapping_mul(8))?==id {selected=Some(i);break}}
 let Some(index)=selected else{return Ok(None)};
 let mut policy=w(m,w(m,header,16)?,index.wrapping_mul(4))?;
 if policy==0 {
  let captured=w(m,service,12)?;let mut i=0u32;
  while i<if dynamic{captured}else{w(m,service,12)?} && policy==0 {
   let pool=at(w(m,service,8)?,i,12);
   if index>=if dynamic{local}else{w(m,header,4)?} {
    if w(m,pool,8)?==0 {
     let captured=w(m,pool,4)?;let mut j=0u32;
     while j<if dynamic{captured}else{w(m,pool,4)?} && policy==0 {
      let candidate=w(m,w(m,pool,0)?,j.wrapping_mul(4))?;
      if w(m,candidate,0x78)?==id {policy=candidate;}
      j=j.wrapping_add(1);
     }
    }
   } else {
    let metadata=at(w(m,header,8)?,index,8);
    if w(m,pool,8)?==half(m,metadata.wrapping_add(6))? {
     let count=w(m,pool,4)?;
     for j in 0..count {
      let candidate=w(m,w(m,pool,0)?,j.wrapping_mul(4))?;
      if m.byte(candidate.wrapping_add(0x7c))?!=0 {continue}
      policy=candidate;
      let metadata=at(w(m,header,8)?,index,8);
      let flags=half(m,metadata.wrapping_add(4))?;
      if flags&6==0 {if flags&8==0 {h.release_usage(m,policy)?;}else{h.release_pool(m,policy)?;}}
      break
     }
     // Native stops after a matching pool even if every instance is busy.
     break
    }
   }
   i=i.wrapping_add(1);
  }
 }
 if policy==0{return Ok(None)}
 let table=w(m,header,16)?;m.write(at(table,index,4),policy)?;
 Ok(Some(Lookup{header,index,policy}))
}
pub fn lookup(m:&mut Memory,service:u32,g:u32,id:u32,h:&mut impl Host)->Result<Option<Lookup>> {select(m,service,g,id,false,h)}
fn endpoint(m:&mut Memory,p:u32,id:u32,h:&mut impl Host)->Result<Endpoint>{
 let position=[w(m,p,4)?,w(m,p,8)?,w(m,p,12)?];
 let mut point=Endpoint{position,attributes:[None;16]};
 let slot=h.marker_slot(m)?;
 *point.attributes.get_mut(slot as usize).ok_or_else(||anyhow::anyhow!("Traversal marker slot exceeds point attributes"))?=Some(id&0x7fffffff);
 Ok(point)
}
pub fn bind(m:&mut Memory,service:u32,g:u32,id:u32,r:u32,edge:u32,dynamic:bool,h:&mut impl Host)->Result<u32>{
 let Some(found)=select(m,service,g,id,dynamic,h)? else{return Ok(0)};
 let view=m.view(r)?;let stride=if dynamic{view.stride}else{32};
 let a=at(view.points,w(m,edge,4)?,stride);let b=at(view.points,w(m,edge,8)?,stride);
 let aid=w(m,a,0)?;let bid=w(m,b,0)?;
 if add_edge(m,found.policy,aid,bid,g)?&255!=1{return Ok(0)}
 let flags=w(m,edge,0)?;
 if flags&0x80000000!=0 {m.write(edge,(flags&0xffff)|((found.index<<16)&0x7fff0000))?;}
 else {
  let index=(flags>>16)&0x7fff;
  if index!=found.index {
   let table=w(m,found.header,16)?;let previous=w(m,table,index.wrapping_mul(4))?;
   let aid=w(m,a,0)?;let bid=w(m,b,0)?;
   link_edge(m,previous,aid,bid,g,found.policy)?;
  }
 }
 if m.byte(found.policy.wrapping_add(0x7c))?==0 {
  h.register18(m,found.policy)?;
  m.write(found.policy.wrapping_add(0x78),id)?;
  m.write_byte(found.policy.wrapping_add(0x7c),1)?;
  let bitset=w(m,found.policy,0x88)?;let data=w(m,bitset,0)?;let size=w(m,bitset,8)?;m.fill(data,size,0)?;
  h.initialize48(m,found.policy)?;
 }
 let aid=w(m,a,0)?&0x7fffffff;let bid=w(m,b,0)?&0x7fffffff;
 let a=endpoint(m,a,aid,h)?;let b=endpoint(m,b,bid,h)?;
 let table=w(m,found.header,16)?;let policy=w(m,table,found.index.wrapping_mul(4))?;
 h.bind50(m,policy,&a,&b)?;Ok(1)
}
#[cfg(test)] mod tests {
 use super::*;
 use serde_json::{Value,json};
 const S:u32=0x60000000;const G:u32=0x61000000;const N:u32=0x63000000;const E:u32=0x64000000;const TABLE:u32=0x67000000;
 struct Services {trace:Vec<Value>,mutate:bool,slot:u32,markers:u32,dynamic:bool,world_mutate:bool}
 fn state(m:&Memory,p:u32)->Result<Value>{let b=w(m,p,0x88)?;let bytes=(0..8).map(|i|m.byte(b+32+i)).collect::<Result<Vec<_>>>()?;Ok(json!([w(m,p,0x78)?,m.byte(p+0x7c)?,m.word(E)?,bytes]))}
 impl Host for Services {
  fn release_usage(&mut self,_:&mut Memory,p:u32)->Result<()> {self.trace.push(json!(["release_usage",p]));Ok(())}
  fn release_pool(&mut self,_:&mut Memory,p:u32)->Result<()> {self.trace.push(json!(["release_pool",p]));Ok(())}
  fn register18(&mut self,m:&mut Memory,p:u32)->Result<()> {self.trace.push(json!(["register18",p,state(m,p)?]));Ok(())}
  fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()> {
   self.trace.push(json!(["initialize48",p,state(m,p)?]));
   if self.mutate {m.write(TABLE,0x6a001000)?;m.write(N+m.stride+4,0x40e00000)?;}Ok(())
  }
  fn marker_slot(&mut self,m:&mut Memory)->Result<u32>{self.trace.push(json!(["marker",self.slot]));self.markers+=1;if self.mutate && self.markers==1 {m.write(N+m.stride,909)?;}Ok(self.slot)}
  fn bind50(&mut self,_:&mut Memory,p:u32,a:&Endpoint,b:&Endpoint)->Result<()> {self.trace.push(json!(["bind50",p,a,b]));Ok(())}
 }
 impl GraphHost for Services {fn kind(&mut self,_:&mut Memory,_:u32)->Result<u32>{Ok(u32::from(self.dynamic))}}
 impl super::super::ai_traversal_exclusion::Services for Services {
  fn class20(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["class20",p]));Ok(0x6af44)}
 }
 impl super::super::ai_traversal_policy_release::Services for Services {
  fn class18(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["class18",p]));Ok(0x6af6c)}
 }
 impl super::super::ai_traversal_policy_release::Host for Services {
  fn register18(&mut self,m:&mut Memory,p:u32)->Result<()>{Host::register18(self,m,p)}
  fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>{Host::initialize48(self,m,p)}
  fn marker_slot(&mut self,m:&mut Memory)->Result<u32>{Host::marker_slot(self,m)}
  fn bind50(&mut self,m:&mut Memory,p:u32,a:&Endpoint,b:&Endpoint)->Result<()>{Host::bind50(self,m,p,a,b)}
  fn kind(&mut self,m:&mut Memory,p:u32)->Result<u32>{GraphHost::kind(self,m,p)}
 }
 impl super::super::ai_navigation_world_registration::Host for Services {
  fn callback(&mut self,m:&mut Memory,world:u32,p:u32,function:u32,data:u32)->Result<()> {
   assert_eq!(world,0x71000000);
   self.trace.push(json!(["world_callback",function,data,p,w(m,p,20)?,w(m,p,40)?,m.word(0x72900014)?]));
   if self.world_mutate {m.write(world+0x50,1)?;}Ok(())
  }
  fn inserted24(&mut self,m:&mut Memory,p:u32)->Result<()> {self.trace.push(json!(["inserted24",p,w(m,p,20)?,w(m,p,40)?,m.word(0x72900014)?]));Ok(())}
 }
 impl super::super::ai_navigation_world_registration::Services for Services {
  fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>{Host::initialize48(self,m,p)}
  fn marker_slot(&mut self,m:&mut Memory)->Result<u32>{Host::marker_slot(self,m)}
  fn bind50(&mut self,m:&mut Memory,p:u32,a:&Endpoint,b:&Endpoint)->Result<()>{Host::bind50(self,m,p,a,b)}
  fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32>{GraphHost::kind(self,m,r)}
 }
 impl super::super::ai_traversal_world_callbacks::Host for Services {
  fn unbind_graph(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32)->Result<()>{anyhow::bail!("Actor-add callback unexpectedly unbound graph geometry")}
  fn toggle28(&mut self,_:&mut Memory,_:u32,_:u32)->Result<()>{anyhow::bail!("Actor-add callback unexpectedly toggled a path object")}
 }
 fn world_callbacks(h:Services)->super::super::ai_traversal_world_callbacks::Native<Services>{
  super::super::ai_traversal_world_callbacks::Native{context:super::super::ai_traversal_world_callbacks::Context{world:release_world(),path_object_class:0x672c4,edge_object_class:0x67314},added:0x4869d4,removed:0x486a40,callbacks:h}
 }
 fn release_world()->super::super::ai_traversal_policy_release::World{super::super::ai_traversal_policy_release::World{root_slot:0x5a108,manager_class:0x6af44,pathfinder_class:0x6af6c}}
 #[test] fn original_lookup_binding_and_edge_ownership_match()->Result<()> {
  let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_registration.json"))?;
  for case in f["cases"].as_array().unwrap() {
   let input=&case["input"];let c=&input["config"];
   let mut m:Memory=serde_json::from_value(input["memory"].clone())?;
   let op=c["op"].as_str().unwrap();
   let mut h=Services{trace:vec![],mutate:c["mutate"].as_bool().unwrap(),slot:c["marker_slot"].as_u64().unwrap() as u32,markers:0,dynamic:op.ends_with("dynamic"),world_mutate:c["world_mutate"].as_bool().unwrap()};
   let id=c["id"].as_u64().unwrap() as u32;
   let result=match op {
    "add"=>add_edge(&mut m,0x6a000000,101,202,G)?,
    "link"=>link_edge(&mut m,0x6a000000,101,202,G,0x6a001000)?,
    "lookup"=>u32::from(lookup(&mut m,S,G,id,&mut h)?.is_some()),
    op if op.starts_with("finalize")=>{
     use super::super::{ai_graph_admission::Admission,ai_graph_handles::{State,Resource},ai_graph_loading::{Loader,Record},ai_graph_resource_group::Group,ai_graph_finalize::{self,Context,Mesh}};
     use std::collections::BTreeMap;
     let mut s=State{phase:0,serial:0,pool:vec![],nodes:BTreeMap::new(),resources:BTreeMap::from([(10,Resource{size18:None,byte8:0,priority:0,state:4,word1c:1})]),current:None,tracked_resource:None,tracked158:0,tracked160:0,deferred_enabled:0};
     let mut l=Loader{source:1,direct_mode:1,name:vec![],extra_records:0,directory:vec![1],records:BTreeMap::from([(1,Record{id:10,word1c:2,size20:0,offset24:0,word28:0,word2c:0})]),bad:vec![0],groups:[Some(Group{resources:vec![10]}),Some(Group{resources:vec![]})]};
     let mut a=Admission{index154:0,index15c:0,word82e4:0,changed14:0,listeners:vec![]};
     let result=if c["real_callbacks"].as_bool().unwrap(){
      let mut registered=Registered{host:super::super::ai_navigation_world_registration::Native{world:release_world(),callbacks:world_callbacks(h)}};
      let result=ai_graph_finalize::finalize(&mut m,&mut a,&mut l,&mut s,&mut Context{manager:G},10,&mut registered)?;h=registered.host.callbacks.callbacks;result
     }else if c["real_world"].as_bool().unwrap(){
      let mut registered=Registered{host:super::super::ai_navigation_world_registration::Native{world:release_world(),callbacks:h}};
      let result=ai_graph_finalize::finalize(&mut m,&mut a,&mut l,&mut s,&mut Context{manager:G},10,&mut registered)?;h=registered.host.callbacks;result
     }else if c["real_release"].as_bool().unwrap(){
      let mut registered=Registered{host:super::super::ai_traversal_policy_release::Native{world:release_world(),callbacks:h}};
      let result=ai_graph_finalize::finalize(&mut m,&mut a,&mut l,&mut s,&mut Context{manager:G},10,&mut registered)?;h=registered.host.callbacks;result
     }else{
      let mut registered=Registered{host:h};
      let result=ai_graph_finalize::finalize(&mut m,&mut a,&mut l,&mut s,&mut Context{manager:G},10,&mut registered)?;h=registered.host;result
     };
     assert_eq!(result,Mesh::NotRequested);
     assert_eq!(s.resources[&10].state,4);
     0
    }
    op=>if c["real_callbacks"].as_bool().unwrap(){let mut native=super::super::ai_navigation_world_registration::Native{world:release_world(),callbacks:world_callbacks(h)};let result=bind(&mut m,S,G,id,10,E,op=="dynamic",&mut native)?;h=native.callbacks.callbacks;result}else if c["real_world"].as_bool().unwrap(){let mut native=super::super::ai_navigation_world_registration::Native{world:release_world(),callbacks:h};let result=bind(&mut m,S,G,id,10,E,op=="dynamic",&mut native)?;h=native.callbacks;result}else if c["real_release"].as_bool().unwrap(){let mut native=super::super::ai_traversal_policy_release::Native{world:release_world(),callbacks:h};let result=bind(&mut m,S,G,id,10,E,op=="dynamic",&mut native)?;h=native.callbacks;result}else{bind(&mut m,S,G,id,10,E,op=="dynamic",&mut h)?},
   };
   if !op.starts_with("finalize") {assert_eq!(result,case["result"].as_u64().unwrap() as u32,"result: {c}");}
   assert_eq!(json!(h.trace),case["trace"],"trace: {c}");
   assert_eq!(serde_json::to_value(m)?,case["memory"],"memory: {c}");
  }Ok(())
 }
 #[test] fn malformed_ownership_cycles_are_reported()->Result<()> {
  let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_registration.json"))?;
  let case=f["cases"].as_array().unwrap().iter().find(|c|c["input"]["config"]["op"]=="link" && c["input"]["config"]["first"]==101 && c["input"]["config"]["next"]==1).unwrap();
  let mut m:Memory=serde_json::from_value(case["input"]["memory"].clone())?;
  m.write(0x6b001010,0x6a000000)?;
  assert!(link_edge(&mut m,0x6a000000,101,202,G,0x6a003000).unwrap_err().to_string().contains("Cyclic"));Ok(())
 }
}
