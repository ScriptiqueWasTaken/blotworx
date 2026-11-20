//!Owned CEntityOutline_MultipleDatabases attributes. Construction419010,
//!initialization4190b0, direct-slot lookup45f994 and refresh418e04.
use anyhow::{Context,Result,ensure};
use std::collections::BTreeMap;
use super::{ai_graph_link_passes::Memory,ai_navigation_entity::Owned};
fn r(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn w(m:&mut Memory,p:u32,o:u32,v:u32)->Result<()>{m.write(p.wrapping_add(o),v)}
#[derive(Clone,Copy)]
pub struct Class{pub identity:usize,pub index:i32,pub has_factory:bool}
#[derive(Debug)]
pub struct Attribute{pub identity:u32,pub owner:usize,pub class:usize}
#[derive(Default)]
pub struct Store{pub records:BTreeMap<u32,Attribute>}
pub trait Host {
 fn allocate(&mut self,m:&mut Memory,entity:&mut Owned,bytes:u32)->Result<u32>;
 fn free(&mut self,m:&mut Memory,entity:&mut Owned,address:u32)->Result<()>;
 ///46c91c(global5a108,41d2c); the actual graph manager, not a supplied count.
 fn graph_manager(&mut self,m:&mut Memory,entity:&mut Owned)->Result<u32>;
}
pub fn construct(m:&mut Memory,p:u32)->Result<()> {
 w(m,p,0,0x516db8)?;for o in [4,8,0xc,0x1c,0x20,0x28,0x2c]{w(m,p,o,0)?;}
 for o in [0x10,0x14]{w(m,p,o,5)?;}for o in [0x18,0x24]{w(m,p,o,0x7f7fffff)?;}Ok(())
}
fn array(m:&mut Memory,e:&mut Owned,count:u32,stride:u32,records:bool,h:&mut impl Host)->Result<u32>{
 let bytes=count.checked_mul(stride).and_then(|n|n.checked_add(16)).context("Outline array exceeds native allocation domain")?;
 let base=h.allocate(m,e,bytes)?;if base==0{return Ok(0)}
 //Actual DOL8000d334 header. The other header bytes remain unwritten.
 w(m,base,0,stride)?;w(m,base,4,count)?;let start=base.wrapping_add(16);
 for i in 0..count{let p=start.wrapping_add(i.wrapping_mul(stride));if records {
  w(m,p,0,0x516d60)?;for o in [4,8,0xc]{w(m,p,o,0)?;}m.write_byte(p.wrapping_add(0x10),0)?;
 }else{for o in [0,4,8]{w(m,p,o,0)?;}}}Ok(start)
}
pub fn initialize(m:&mut Memory,p:u32,e:&mut Owned,h:&mut impl Host)->Result<()> {
 w(m,p,0x14,5)?;w(m,p,0x10,5)?;
 let points=array(m,e,5,12,false,h)?;w(m,p,4,points)?;
 let manager=h.graph_manager(m,e)?;let databases=r(m,manager,4)?;let count=r(m,databases,0x14)?;w(m,p,0xc,count)?;
 let records=array(m,e,count,20,true,h)?;w(m,p,8,records)
}
///Both native branches mark records dirty. The source loop is post-tested;
///zero-count/unmapped storage diagnoses, rather than reporting a ready outline.
pub fn refresh(m:&mut Memory,p:u32)->Result<()> {
 let value=if r(m,p,0x10)?==0{5}else{0};w(m,p,0x10,value)?;
 let mut i=0u32;loop{let records=r(m,p,8)?;m.write_byte(records.wrapping_add(i.wrapping_mul(20)).wrapping_add(16),1)?;
  i=i.wrapping_add(1);if i>=r(m,p,0xc)?{break}
 }Ok(())
}
impl Owned {
 ///45fa78 publishes only after the constructor and virtual14 return. This
 ///does not claim outline geometry/update or normal navigation admission.
 pub fn create_outline(&mut self,m:&mut Memory,c:&Class,h:&mut impl Host)->Result<Option<u32>> {
  let e=self;
  if c.index<0||c.index as usize>=e.entity.attributes.len()||!c.has_factory{return Ok(None)}
  let id=h.allocate(m,e,0x30)?;ensure!(id!=0,"Required outline object allocation is null");ensure!(!e.outlines.records.contains_key(&id),"Outline allocation already owned");
  construct(m,id)?;e.outlines.records.insert(id,Attribute{identity:id,owner:e.entity.identity,class:c.identity});
  initialize(m,id,e,h)?;
  *e.entity.attributes.get_mut(c.index as usize).context("Outline initialization removed its registered slot")?=Some(id as usize);Ok(Some(id))
 }
 ///The source direct-slot lookup is optional and never constructs an attribute.
 pub fn lookup_outline(&self,c:&Class)->Result<Option<u32>> {
  let e=self;
  if c.index<0{return Ok(None)}let Some(id)=e.entity.attributes.get(c.index as usize).copied().flatten().filter(|&v|v!=0)else{return Ok(None)};
  let id=u32::try_from(id)?;let a=e.outlines.records.get(&id).context("Outline attribute slot references an unowned object")?;
  ensure!(a.identity==id&&a.owner==e.entity.identity&&a.class==c.identity,"Outline attribute ownership or class differs");Ok(Some(id))
 }
 pub fn refresh_outline_twice(&self,m:&mut Memory,c:&Class)->Result<bool>{let Some(p)=self.lookup_outline(c)?else{return Ok(false)};refresh(m,p)?;refresh(m,p)?;Ok(true)}
 ///41904c/41864c deleting cleanup. Attribute-slot removal belongs to the
 ///entity lifecycle; it remains visible during the native destructor callbacks.
 pub fn destroy_outline(&mut self,m:&mut Memory,p:u32,h:&mut impl Host)->Result<()> {
  let e=self;
  let a=e.outlines.records.get(&p).context("Destroyed outline is not owned")?;ensure!(a.owner==e.entity.identity,"Destroyed outline owner differs");
  w(m,p,0,0x516d00)?;
  let points=r(m,p,4)?;if points!=0{h.free(m,e,points.wrapping_sub(16))?;}
  let records=r(m,p,8)?;if records!=0{let base=records.wrapping_sub(16);let stride=r(m,base,0)?;let count=r(m,base,4)?;
   for i in (0..count).rev(){let record=records.wrapping_add(i.wrapping_mul(stride));w(m,record,0,0x516d60)?;let points=r(m,record,8)?;if points!=0{h.free(m,e,points.wrapping_sub(16))?;}}
   h.free(m,e,base)?;
  }h.free(m,e,p)?;e.outlines.records.remove(&p);Ok(())
 }
}
#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};
 struct Scalars;
 impl super::super::ai_navigation_entity::Host for Scalars{
  fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(0.5)}fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(1.5)}fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(0.)}fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(None)}
 }
 struct Heap<'a>{row:&'a Value,trace:Vec<Value>,allocation:usize}
 impl Host for Heap<'_>{
  fn allocate(&mut self,m:&mut Memory,_:&mut Owned,n:u32)->Result<u32>{let calls=self.row["trace"].as_array().unwrap().iter().filter(|v|v[0]=="allocate").collect::<Vec<_>>();let call=calls[self.allocation];self.allocation+=1;assert_eq!(json!(n),call[1]);let p=call[2].as_u64().unwrap() as u32;ensure!(m.regions.insert(p,vec![0xa5;n as usize]).is_none(),"Repeated outline allocation");self.trace.push(json!(["allocate",n,p]));Ok(p)}
  fn free(&mut self,_:&mut Memory,_:&mut Owned,p:u32)->Result<()>{self.trace.push(json!(["free",p]));Ok(())}
  fn graph_manager(&mut self,_:&mut Memory,_:&mut Owned)->Result<u32>{self.trace.push(json!(["graph"]));Ok(0x74000000)}
 }
 fn bytes(s:&str)->Vec<u8>{s.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect()}
 fn compare(m:&Memory,spans:&Value,c:&Value){for span in spans.as_array().unwrap(){let p=span[0].as_u64().unwrap() as u32;if p==0x73000000{continue}assert_eq!(m.regions[&p],bytes(span[1].as_str().unwrap()),"{c} at{p:x}");}}
 #[test]
 fn original_owned_outline_creation_refresh_lookup_and_cleanup_match()->Result<()> {
  use super::super::ai_navigation_entity::{Registry,Class as ScalarClass,Kind};
  let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_outline.json"))?;
  for row in capture["cases"].as_array().unwrap(){let c=&row["input"];let registry=Registry{count:4,diameter:ScalarClass{index:0,constructor:Some(Kind::Diameter)},height:ScalarClass{index:1,constructor:Some(Kind::Height)},maximum_speed:ScalarClass{index:2,constructor:Some(Kind::MaximumSpeed)}};
   let mut e=Owned::new(0x72000000,None,[10,11],&registry,&mut Scalars)?;e.entity.attributes[3]=c["previous"].as_u64().filter(|&v|v!=0).map(|v|v as usize);
   let class=Class{identity:0x3613c,index:c["index"].as_i64().unwrap_or(3) as i32,has_factory:c["factory"].as_bool().unwrap_or(true)};
   let mut m=Memory::default();m.regions.insert(0x75000000,vec![0xa5;0x30]);m.regions.insert(0x74000000,vec![0;8]);m.regions.insert(0x74001000,vec![0;0x18]);m.write(0x74000004,0x74001000)?;m.write(0x74001014,c["databases"].as_u64().unwrap_or(1) as u32)?;
   let mut h=Heap{row,trace:vec![],allocation:0};
   let id=if c["op"]=="construct"{construct(&mut m,0x75000000)?;Some(0x75000000)}else{e.create_outline(&mut m,&class,&mut h)?};
   assert_eq!(json!(id.unwrap_or(0)),row["identity"],"{c}");compare(&m,&row["created"],c);
   if let Some(id)=id{if c["op"]!="construct"{for _ in 0..c["refresh"].as_u64().unwrap_or(0){refresh(&mut m,id)?;}}}compare(&m,&row["refreshed"],c);
   if let Some(spans)=row["prepared"].as_array(){for span in spans{let p=span[0].as_u64().unwrap() as u32;if p!=0x73000000{m.regions.insert(p,bytes(span[1].as_str().unwrap()));}}}
   if c["op"]!="construct"{assert_eq!(json!(e.lookup_outline(&class)?.unwrap_or(0)),row["lookup"],"{c}");}
   if c["destroy"].as_bool().unwrap_or(false){if let Some(id)=id{e.destroy_outline(&mut m,id,&mut h)?;assert!(e.outlines.records.is_empty());}}
   compare(&m,&row["final"],c);assert_eq!(json!(e.entity.attributes[3].unwrap_or(0)),row["slot"],"{c}");assert_eq!(json!(h.trace),row["trace"],"{c}");
  }eprintln!("{} original owned outline cases",capture["cases"].as_array().unwrap().len());Ok(())
 }
}
