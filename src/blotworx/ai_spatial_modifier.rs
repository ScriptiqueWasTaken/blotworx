//! Complete base ICanGo1c32f4 layer attributes and2b7ac0 filter forwarding.
//! Service data stays in the canonical navigation arena.
use anyhow::{Context as _,Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_traversal_exclusion::{self,Services},ai_spatial_walk::Filter};
#[derive(Clone,Copy)]
pub struct Context {pub root_slot:u32,pub manager_class:u32}
pub trait Attributes:Services {
    fn name(&mut self,memory:&mut Memory,node:u32)->Result<Option<u32>>;
    fn value(&mut self,memory:&mut Memory,node:u32)->Result<u32>;
    /// Definition-backed implementations resolve handles against their owning
    /// tree; native arena strings retain checked raw byte access by default.
    fn text(&mut self,memory:&mut Memory,handle:u32)->Result<Vec<u8>>{cstring(memory,handle)}
    fn first(&mut self,memory:&mut Memory,handle:u32)->Result<u8>{memory.byte(handle)}
}
fn cstring(memory:&Memory,mut p:u32)->Result<Vec<u8>> {
    ensure!(p!=0,"CanGo layer name is null");let mut bytes=Vec::new();
    loop {let b=memory.byte(p)?;if b==0 {return Ok(bytes)}bytes.push(b);p=p.wrapping_add(1);}
}
fn named(memory:&mut Memory,node:u32,name:&[u8],host:&mut impl Attributes)->Result<bool> {
    if host.name(memory,node)?.is_none() {return Ok(false)}
    let p=host.name(memory,node)?.context("CanGo property name disappeared")?;
    Ok(super::ai_traversal_registry::names_equal(&host.text(memory,p)?,name))
}
pub fn attribute(memory:&mut Memory,c:Context,service:u32,node:u32,host:&mut impl Attributes)->Result<u32> {
    if host.name(memory,node)?.is_some() {
        let p=host.name(memory,node)?.context("CanGo metadata name disappeared")?;
        if host.first(memory,p)?==b'_' {return Ok(1)}
    }
    let offsets:&[u32]=if named(memory,node,b"BlockingLayer",host)? {&[12,20,24,28]}
        else if named(memory,node,b"PathObjectLayer",host)? {&[16,20,28]}else{return Ok(0)};
    let manager=ai_traversal_exclusion::find_service(memory,memory.word(c.root_slot)?,c.manager_class,host)?;
    if manager==0 {return Ok(1)}
    let value=host.value(memory,node)?;
    let config=memory.word(manager.wrapping_add(4))?;
    if config==0 {return Ok(1)}
    let mut at=memory.word(config.wrapping_add(8))?;
    let mut layer=0;
    while at!=0 {
        let item=memory.word(at.wrapping_add(4))?;
        let name=memory.word(item.wrapping_add(4))?;
        if super::ai_traversal_registry::names_equal(&host.text(memory,value)?,&cstring(memory,name)?) {layer=memory.word(at.wrapping_add(4))?;break;}
        at=memory.word(at.wrapping_add(12))?;
    }
    if layer==0 {return Ok(1)}
    for &offset in offsets {
        let pool=memory.word(service.wrapping_add(offset))?;
        super::ai_spatial_construction::append(memory,pool,layer)?;
    }
    Ok(1)
}
/// The caller supplies a distinct shim frame, retaining the local filter word
/// through virtual24. Reentrant virtual calls must supply their own frames.
pub fn forward1c(memory:&mut Memory,frame:u32,service:u32,start:u32,end:u32,radius:u32,
    mut query24:impl FnMut(&mut Memory,u32,u32,u32,u32,&mut Option<Filter>)->Result<u32>)->Result<u32> {
    let slot=frame.wrapping_add(8);memory.write(slot,memory.word(service.wrapping_add(24))?)?;
    query24(memory,service,start,end,radius,&mut Some(Filter::Address(slot)))
}
/// Concrete BASE virtual24; derived classes must supply their actual dispatch
/// to forward1c rather than pretending this base algorithm implements them.
pub fn query_base1c(memory:&mut Memory,frame:u32,c:super::ai_spatial_acceptance::Context,service:u32,start:u32,end:u32,radius:u32,
    host:&mut impl super::ai_spatial_acceptance::Host)->Result<u32> {
    forward1c(memory,frame,service,start,end,radius,|memory,service,start,end,radius,filter|
        super::ai_spatial_acceptance::query24(memory,c,service,start,end,radius,filter,host))
}
#[cfg(test)]
mod tests {
 use super::*;
 use serde_json::{Value,json};
 const BASE:u32=0x60000000;
 struct Runtime {present:bool,manager:bool,trace:Vec<Value>}
 impl Services for Runtime {
  fn class20(&mut self,_:&mut Memory,_:u32)->Result<u32>{self.trace.push(json!("class"));Ok(if self.manager{0x41d2c}else{BASE+0x900})}
 }
 impl Attributes for Runtime {
  fn name(&mut self,_:&mut Memory,_:u32)->Result<Option<u32>>{self.trace.push(json!("name"));Ok(self.present.then_some(BASE+0x600))}
  fn value(&mut self,_:&mut Memory,_:u32)->Result<u32>{self.trace.push(json!("value"));Ok(BASE+0x700)}
 }
 #[test]
 fn retail_layer_attributes_preserve_pool_aliases_and_capacity()->Result<()> {
  let data:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_modifier.json"))?;
  for (index,c) in data["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(BASE,vec![0;0x3000]),(0x5a108,vec![0;4])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   m.write(0x5a108,BASE+0x100)?;
   for (offset,byte) in c["initial"].as_object().unwrap(){m.write_byte(BASE+offset.parse::<u32>()?,byte.as_u64().unwrap() as u8)?;}
   let mut host=Runtime{present:c["input"]["name_present"].as_bool().unwrap(),manager:c["input"]["manager"].as_bool().unwrap(),trace:vec![]};
   let result=attribute(&mut m,Context{root_slot:0x5a108,manager_class:0x41d2c},BASE,BASE+0x500,&mut host)?;
   assert_eq!(json!(result),c["result"],"case {index}");assert_eq!(json!(host.trace),c["trace"],"case {index}");
   let actual:serde_json::Map<String,Value>=(0..0x3000u32).filter_map(|i|match m.byte(BASE+i){Ok(0)=>None,Ok(b)=>Some((i.to_string(),json!(b))),Err(e)=>panic!("{e}")}).collect();
   assert_eq!(json!(actual),c["final"],"case {index}");
  }
  Ok(())
 }
 #[test]
 fn forwarding_retains_live_stack_filter_and_full_return()->Result<()> {
  let mut m=Memory{regions:[(BASE,vec![0;0x100])].into(),resources:Default::default(),stride:0,journal:Default::default()};
  m.write(BASE+24,0x1234)?;
  let result=forward1c(&mut m,BASE+0x40,BASE,11,22,0x3f000000,|m,service,start,end,radius,filter|{
   assert_eq!((service,start,end,radius),(BASE,11,22,0x3f000000));
   let slot=match filter{Some(Filter::Address(slot))=>*slot,_=>panic!("stack filter")};
   assert_eq!(slot,BASE+0x48);assert_eq!(m.word(slot)?,0x1234);
   m.write(BASE+24,0x5678)?;assert_eq!(m.word(slot)?,0x1234);m.write(slot,0xabcd)?;Ok(0x80000101)
  })?;
  assert_eq!(result,0x80000101);assert_eq!(m.word(BASE+0x48)?,0xabcd);assert_eq!(m.word(BASE+24)?,0x5678);Ok(())
 }
}
