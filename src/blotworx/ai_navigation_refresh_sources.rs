//!18a6cc/18a99c over the actual scene-group hash registry. Existing paired
//!weak-handle and component registries supply canonical lookups; no visible
//!entity scan or rebuilt/sorted list substitutes for native registry order.
use anyhow::{Context,Result,ensure};use std::collections::BTreeSet;
use super::{ai_graph_link_passes::Memory,ai_navigation_refresh::{self,Pass,Metadata},
 entity_handle_runtime::{Registry,Allocator},entity_component_lookup::Storage};
fn r(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn w(m:&mut Memory,p:u32,o:u32,v:u32)->Result<()>{m.write(p.wrapping_add(o),v)}
pub trait Lifecycle<A:Allocator> {
 fn enter(&mut self,m:&mut Memory)->Result<u32>;
 fn leave(&mut self,m:&mut Memory,enabled:bool)->Result<()>;
 fn destroy_group(&mut self,m:&mut Memory,group:u32,handles:&mut Registry<A>,components:&mut Storage)->Result<()>;
}
///Sources41798/1bd00 take actor+8 interfaces. Their component interfaces
///differ: BasicAI subtracts8; the dynamic-body getter subtracts14hex.
pub fn component(m:&Memory,components:&Storage,actor:u32,pass:Pass)->Result<Option<u32>> {
 if actor==0{return Ok(None)}let index=m.byte(if pass==Pass::Actors{0x27b30}else{0x2d2b0})?;
 Ok(components.component(actor.wrapping_sub(8),index)?.map(|p|if pass==Pass::Actors{p}else{p.wrapping_sub(12)}).filter(|&p|p!=0))
}
fn hash(m:&Memory,mut name:u32)->Result<u32>{let mut h=0u32;loop{let b=m.byte(name)?;if b==0{return Ok(h)}h=h.wrapping_mul(33).wrapping_add((b as i8 as i32) as u32);name=name.checked_add(1).context("Scene-group name address overflow")?;}}
fn next(m:&Memory,manager:u32,node:u32)->Result<u32>{
 let chained=r(m,node,0)?;if chained!=0{return Ok(chained)}
 let count=r(m,manager,0x58)?;ensure!(count!=0,"Scene-group bucket count became zero during traversal");
 let mut index=(hash(m,r(m,node,4)?)?%count).wrapping_add(1);
 while index<count {let p=r(m,r(m,manager,0x5c)?,index.wrapping_mul(4))?;if p!=0{return Ok(p)}index=index.wrapping_add(1);}Ok(0)
}
pub(super) fn retain<A:Allocator>(m:&mut Memory,group:u32,h:&mut impl Lifecycle<A>)->Result<()> {
 let state=h.enter(m)?;let count=r(m,group,4)?;w(m,group,4,count.wrapping_add(1))?;h.leave(m,state!=0)
}
pub(super) fn release<A:Allocator>(m:&mut Memory,group:u32,handles:&mut Registry<A>,components:&mut Storage,h:&mut impl Lifecycle<A>)->Result<()> {
 let state=h.enter(m)?;let count=r(m,group,4)?.wrapping_sub(1);w(m,group,4,count)?;h.leave(m,state!=0)?;
 if count==0{h.destroy_group(m,group,handles,components)?;}Ok(())
}
pub fn collect<A:Allocator>(m:&mut Memory,manager:u32,pass:Pass,handles:&mut Registry<A>,components:&mut Storage,h:&mut impl Lifecycle<A>)->Result<Vec<u32>> {
 let mut node=0;let count=r(m,manager,0x58)?;
 for i in 0..count{node=r(m,r(m,manager,0x5c)?,i.wrapping_mul(4))?;if node!=0{break}}
 let mut result=Vec::new();let mut seen=BTreeSet::new();
 while node!=0 {ensure!(seen.insert(node),"Cyclic scene-group registry");let group=r(m,node,8)?;
  if group!=0{retain::<A>(m,group,h)?;}
  //The source advances its hash iterator BEFORE visiting the retained group.
  node=next(m,manager,node)?;
  if group==0{continue}let count=r(m,group,0x14)?;
  for i in 0..count{let actor=r(m,r(m,group,0xc)?,i.wrapping_mul(4))?;
   if component(m,components,actor,pass)?.is_some(){let handle=handles.handle(actor)?;result.try_reserve(1)?;result.push(handle);}
  }
  release(m,group,handles,components,h)?;
 }Ok(result)
}
pub trait Backend<A:Allocator>:Lifecycle<A>+super::ai_lpf_context::Host {
 fn graph_service(&mut self,m:&mut Memory)->Result<Option<u32>>;
 fn object_class(&mut self,m:&mut Memory,object:u32)->Result<usize>;
 fn parent_class(&mut self,m:&mut Memory,class:usize)->Result<Option<usize>>;
 fn refresh_metadata(&self)->Metadata;
}
pub struct Native<'a,A:Allocator,H>{pub manager:u32,pub handles:&'a mut Registry<A>,pub components:&'a mut Storage,pub backend:&'a mut H}
impl<A:Allocator,H:Backend<A>> ai_navigation_refresh::Sources for Native<'_,A,H>{
 fn collect(&mut self,m:&mut Memory,p:Pass)->Result<Vec<u32>>{collect(m,self.manager,p,self.handles,self.components,self.backend)}
 fn resolve(&mut self,_:&mut Memory,h:u32)->Result<Option<u32>>{self.handles.entity(h)?.map(u32::try_from).transpose().map(|v|v.filter(|&p|p!=0)).map_err(Into::into)}
 fn component(&mut self,m:&mut Memory,a:u32,p:Pass)->Result<Option<u32>>{component(m,self.components,a,p)}
 fn graph_service(&mut self,m:&mut Memory)->Result<Option<u32>>{self.backend.graph_service(m)}
 fn object_class(&mut self,m:&mut Memory,o:u32)->Result<usize>{self.backend.object_class(m,o)}
 fn parent_class(&mut self,m:&mut Memory,c:usize)->Result<Option<usize>>{self.backend.parent_class(m,c)}
}
impl<A:Allocator,H:Backend<A>> super::ai_lpf_context::Host for Native<'_,A,H>{
 fn child_reset(&mut self,m:&mut Memory,p:u32,c:u32)->Result<()>{self.backend.child_reset(m,p,c)}
 fn group_clear(&mut self,m:&mut Memory,g:u32)->Result<()>{self.backend.group_clear(m,g)}
}
impl<A:Allocator,H:Backend<A>> super::ai_navigation_region_world::Runtime for Native<'_,A,H>{fn refresh_metadata(&self)->Metadata{self.backend.refresh_metadata()}}
#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};use std::collections::BTreeMap;
 struct Nodes{next:usize}
 impl Allocator for Nodes{fn node(&mut self,_:super::super::entity_handle_runtime::Direction,_:&mut super::super::entity_handle_lookup::Table,_:&mut u32)->Result<usize>{self.next+=1;Ok(self.next)}}
 struct Host<'a>{row:&'a Value,trace:Vec<Value>}
 impl Lifecycle<Nodes> for Host<'_>{
  fn enter(&mut self,_:&mut Memory)->Result<u32>{let n=self.row["input"]["enter"].as_u64().unwrap() as u32;self.trace.push(json!(["enter",n]));Ok(n)}
  fn leave(&mut self,_:&mut Memory,e:bool)->Result<()>{self.trace.push(json!(["leave",u8::from(e)]));Ok(())}
  fn destroy_group(&mut self,_:&mut Memory,g:u32,h:&mut Registry<Nodes>,_:&mut Storage)->Result<()>{self.trace.push(json!(["destroy",g]));if self.row["input"]["destroy_remove"].as_bool().unwrap_or(false){h.remove(self.row["actors"][1].as_u64().unwrap() as u32)?;}Ok(())}
 }
 impl super::super::ai_lpf_context::Host for Host<'_>{
  fn child_reset(&mut self,_:&mut Memory,_:u32,_:u32)->Result<()>{anyhow::bail!("No LPF child reset in source collector fixture")}
  fn group_clear(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("No LPF group reset in source collector fixture")}
 }
 impl Backend<Nodes> for Host<'_>{
  fn graph_service(&mut self,m:&mut Memory)->Result<Option<u32>>{ensure!(m.word(0x5a108)?==0,"Collector fixture has no global system");Ok(None)}
  fn object_class(&mut self,_:&mut Memory,_:u32)->Result<usize>{anyhow::bail!("No final object in source collector fixture")}
  fn parent_class(&mut self,_:&mut Memory,_:usize)->Result<Option<usize>>{anyhow::bail!("No final class in source collector fixture")}
  fn refresh_metadata(&self)->Metadata{Metadata{outline:super::super::ai_navigation_outline::Class{identity:0x3613c,index:3,has_factory:true},final_class:0x11368}}
 }
 fn bytes(s:&str)->Vec<u8>{s.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect()}
 #[test]
 fn original_registry_collection_typed_interfaces_and_complete_refresh_composition_match()->Result<()> {
  use super::super::entity_component_lookup::Entity;
  use super::super::ai_navigation_refresh::Sources;
  let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_refresh_sources.json"))?;
  for row in capture["cases"].as_array().unwrap(){let c=&row["input"];let mut m=Memory::default();for span in row["initial"].as_array().unwrap(){m.regions.insert(span[0].as_u64().unwrap() as u32,bytes(span[1].as_str().unwrap()));}m.regions.insert(0x27b30,vec![7]);m.regions.insert(0x2d2b0,vec![9]);m.regions.insert(0x5a108,vec![0;4]);
   let mut components=Storage{entities:Default::default(),metadata:Default::default()};
   for d in row["components"].as_array().unwrap(){components.entities.insert(d["base"].as_u64().unwrap() as u32,Entity{metadata:0,slots:Some(d["slots"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u8).collect()),interfaces:d["interfaces"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect()});}
   let mut handles=Registry::initialized(0,Nodes{next:0})?;for a in row["actors"].as_array().unwrap().iter().take(4){handles.register(a.as_u64().unwrap() as u32)?;}
   let mut h=Host{row,trace:vec![]};let mut native=Native{manager:0x72000000,handles:&mut handles,components:&mut components,backend:&mut h};
   if c["whole"].as_bool().unwrap_or(false){let metadata=super::super::ai_navigation_region_world::Runtime::refresh_metadata(&native);ai_navigation_refresh::refresh(&mut m,&BTreeMap::new(),metadata,&mut native)?;}
   else{let pass=if c["pass"]=="actors"{Pass::Actors}else{Pass::Groups};let output=native.collect(&mut m,pass)?;assert_eq!(json!(output),row["output"],"{c}");}
   for i in 1..=4u32{assert_eq!(json!(native.resolve(&mut m,i)?),row["resolved"].get(i.to_string()).cloned().unwrap_or(Value::Null),"resolved {c} handle{i}");}
   assert_eq!(json!(h.trace),row["trace"],"{c}");for span in row["final"].as_array().unwrap(){let p=span[0].as_u64().unwrap() as u32;assert_eq!(m.regions[&p],bytes(span[1].as_str().unwrap()),"{c} at{p:x}");}
  }eprintln!("{} original source collector/composed refresh cases",capture["cases"].as_array().unwrap().len());Ok(())
 }
}
