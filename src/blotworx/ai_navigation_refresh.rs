//!18a4a8 outline refresh and191c3c/191d6c final database invalidation.
//!Target gathering, weak handles and component/class registry access require
//!their owning runtime. Attribute lookup/mutation uses canonical Owned data.
use anyhow::{Context,Result,ensure};use std::collections::{BTreeMap,BTreeSet};
use super::{ai_graph_link_passes::Memory,ai_navigation_entity::Owned,ai_navigation_outline::Class};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Pass{Actors,Groups}
#[derive(Clone,Copy)]
pub struct Metadata{pub outline:Class,pub final_class:usize}
pub trait Sources {
 ///18a6cc/18a99c, preserving source gather order and handle lifetime.
 fn collect(&mut self,m:&mut Memory,pass:Pass)->Result<Vec<u32>>;
 fn resolve(&mut self,m:&mut Memory,handle:u32)->Result<Option<u32>>;
 fn component(&mut self,m:&mut Memory,actor:u32,pass:Pass)->Result<Option<u32>>;
 ///191c3c global system absence ->None; otherwise actual41d2c service.
 fn graph_service(&mut self,m:&mut Memory)->Result<Option<u32>>;
 fn object_class(&mut self,m:&mut Memory,object:u32)->Result<usize>;
 fn parent_class(&mut self,m:&mut Memory,class:usize)->Result<Option<usize>>;
}
fn r(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn name(m:&Memory,mut p:u32)->Result<Vec<u8>>{let mut s=Vec::new();loop{let b=m.byte(p)?;if b==0{return Ok(s)}s.push(b);p=p.checked_add(1).context("Refresh database name address overflow")?;}}
fn outline(m:&mut Memory,e:&BTreeMap<usize,Owned>,class:&Class,id:u32)->Result<()> {
 let entity=e.get(&(id as usize)).context("World refresh references an unowned navigation entity")?;
 entity.refresh_outline_twice(m,class)?;Ok(())
}
fn finish(m:&mut Memory,expected:usize,h:&mut impl Sources)->Result<()> {
 let Some(service)=h.graph_service(m)?else{return Ok(())};
 let list=r(m,service,4)?;let mut node=if list==0{0}else{r(m,list,8)?};let mut seen=BTreeSet::new();let mut database=None;
 while node!=0{ensure!(seen.insert(node),"Cyclic refresh database list");let d=r(m,node,4)?;
  if super::ai_traversal_registry::names_equal(&name(m,r(m,d,4)?)?,b"ThinnedFloors"){database=Some(d);break}node=r(m,node,0xc)?;
 }
 let database=database.context("Required ThinnedFloors refresh database is absent")?;
 let objects=r(m,database,0x14)?;if r(m,objects,0x14)?!=1{return Ok(())}
 let node=r(m,objects,8)?;let object=r(m,node,4)?;let mut class=h.object_class(m,object)?;let mut classes=BTreeSet::new();
 loop{if class==expected{break}ensure!(classes.insert(class),"Cyclic refresh object class ancestry");let Some(parent)=h.parent_class(m,class)?else{return Ok(())};class=parent;}
 //The getter may change the single object's payload; native reads it again.
 let object=r(m,r(m,objects,8)?,4)?;let mut i=0u32;
 while (i as i32)<(r(m,object,0xc)? as i32){let p=r(m,object,8)?.wrapping_add(i.wrapping_mul(0x1c));if r(m,p,0)?!=0{m.write_byte(p.wrapping_add(0x10),1)?;}i=i.wrapping_add(1);}
 Ok(())
}
pub fn refresh(m:&mut Memory,entities:&BTreeMap<usize,Owned>,metadata:Metadata,h:&mut impl Sources)->Result<()> {
 for pass in [Pass::Actors,Pass::Groups]{let handles=h.collect(m,pass)?;
  for handle in handles{if handle==0{continue}let Some(actor)=h.resolve(m,handle)?else{continue};
   let component=h.component(m,actor,pass)?;
   match pass {
    Pass::Actors=>{let component=component.context("BasicAI component disappeared after refresh collection")?;let id=r(m,component,0x40)?;if id!=0{outline(m,entities,&metadata.outline,id)?;}}
    Pass::Groups=>{let Some(component)=component else{continue};let mut i=0u32;
     while i<r(m,component,0x94)?{let id=r(m,component,0x98u32.wrapping_add(i.wrapping_mul(4)))?;outline(m,entities,&metadata.outline,id)?;i=i.wrapping_add(1);}
    }
   }
  }
 }finish(m,metadata.final_class,h)
}
#[cfg(test)]
mod tests {
 use super::*;use serde_json::{Value,json};
 struct Scalars;
 impl super::super::ai_navigation_entity::Host for Scalars{
  fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(0.5)}fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(1.5)}fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(0.)}fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(None)}
 }
 struct Fixture<'a>{row:&'a Value,trace:Vec<Value>}
 impl Sources for Fixture<'_>{
  fn collect(&mut self,_:&mut Memory,pass:Pass)->Result<Vec<u32>>{let key=if pass==Pass::Actors{"actors"}else{"groups"};self.trace.push(json!(["collect",key]));Ok(self.row["input"][key].as_array().map(|a|a.iter().map(|v|v.as_u64().unwrap() as u32).collect()).unwrap_or_else(||if pass==Pass::Actors{vec![0,11,12,13,11]}else{vec![21,22,0]}))}
  fn resolve(&mut self,_:&mut Memory,h:u32)->Result<Option<u32>>{self.trace.push(json!(["resolve",h]));Ok(self.row["resolved"].get(h.to_string()).and_then(Value::as_u64).map(|v|v as u32))}
  fn component(&mut self,_:&mut Memory,a:u32,p:Pass)->Result<Option<u32>>{self.trace.push(json!(["component",a,if p==Pass::Actors{"actors"}else{"groups"}]));Ok(self.row["components"].get(a.to_string()).and_then(Value::as_u64).map(|v|v as u32))}
  fn graph_service(&mut self,_:&mut Memory)->Result<Option<u32>>{if !self.row["input"]["system"].as_bool().unwrap_or(true){return Ok(None)}self.trace.push(json!(["graph"]));Ok(Some(0x72200000))}
  fn object_class(&mut self,m:&mut Memory,o:u32)->Result<usize>{self.trace.push(json!(["class",o]));let kind=self.row["input"]["class"].as_str().unwrap_or("derived");if kind=="mutate"{m.write(self.row["object_node"].as_u64().unwrap() as u32+4,self.row["object_pair"][1].as_u64().unwrap() as u32)?;}Ok(if matches!(kind,"derived"|"other"){0x7e000000}else{0x11368})}
  fn parent_class(&mut self,_:&mut Memory,_:usize)->Result<Option<usize>>{Ok((self.row["input"]["class"].as_str().unwrap_or("derived")=="derived").then_some(0x11368))}
 }
 fn bytes(s:&str)->Vec<u8>{s.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect()}
 #[test]
 fn original_two_pass_world_outline_refresh_and_final_database_marking_match()->Result<()> {
  use super::super::ai_navigation_entity::{Registry,Class as ScalarClass,Kind};
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_refresh.json"))?;
  for row in fixture["cases"].as_array().unwrap(){let c=&row["input"];let mut m=Memory::default();for span in row["initial"].as_array().unwrap(){m.regions.insert(span[0].as_u64().unwrap() as u32,bytes(span[1].as_str().unwrap()));}
   let mut entities=BTreeMap::new();let registry=Registry{count:4,diameter:ScalarClass{index:0,constructor:Some(Kind::Diameter)},height:ScalarClass{index:1,constructor:Some(Kind::Height)},maximum_speed:ScalarClass{index:2,constructor:Some(Kind::MaximumSpeed)}};
   for a in row["entities"].as_array().unwrap(){let id=a[0].as_u64().unwrap() as usize;let p=a[1].as_u64().unwrap() as u32;let mut e=Owned::new(id,None,[0,0],&registry,&mut Scalars)?;if p!=0{e.entity.attributes[3]=Some(p as usize);e.outlines.records.insert(p,super::super::ai_navigation_outline::Attribute{identity:p,owner:id,class:0x3613c});}entities.insert(id,e);}
   let metadata=Metadata{outline:Class{identity:0x3613c,index:c["index"].as_i64().unwrap_or(3) as i32,has_factory:true},final_class:0x11368};
   let mut h=Fixture{row,trace:vec![]};refresh(&mut m,&entities,metadata,&mut h)?;
   assert_eq!(json!(h.trace),row["trace"],"{c}");for span in row["final"].as_array().unwrap(){let p=span[0].as_u64().unwrap() as u32;assert_eq!(m.regions[&p],bytes(span[1].as_str().unwrap()),"{c} at{p:x}");}
  }eprintln!("{} original World refresh cases",fixture["cases"].as_array().unwrap().len());Ok(())
 }
}
