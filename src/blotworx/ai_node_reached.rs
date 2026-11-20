//! EM1 path-node arrival1c833c..1c88c0, distinct from goal arrival.
//! These detectors have no arrival latch: virtual14 is the original no-op.
use anyhow::{Context,Result};
use super::{ai_graph_link_passes::Memory,ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Class{Distance2D5,Distance3D}
impl Class{
 pub fn name(self)->&'static [u8]{match self{Self::Distance2D5=>b"Mpd::CDetectPathNodeReached_Distance2D5",Self::Distance3D=>b"Mpd::CDetectPathNodeReached_Distance3D"}}
}
pub struct State{pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub class:Class,pub distance:Option<f32>,pub height:Option<f32>}
pub trait DefinitionBackend{
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
 fn number(&mut self,s:&mut State,text:Option<&[u8]>)->Result<f64>;
 fn entity_height(&mut self,s:&mut State)->Result<Option<f32>>;
}
impl State{
 pub fn construct(class:Class,identity:usize,pathfinder:usize,name:&[u8])->Self{Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),class,distance:None,height:None}}
 pub fn reset(&mut self){} // actual2b7c00, including before initialization
 pub fn query(&self,m:&Memory,start:u32,end:u32)->Result<u32>{
  self.query_positions(super::ai_refine_goal::position(m,start)?,super::ai_refine_goal::position(m,end)?)
 }
 /// Same virtual1c arithmetic for canonical inline parent points, without a
 /// serialized mirror of the owning pathfinder.
 pub fn query_positions(&self,source:[f32;3],target:[f32;3])->Result<u32>{
  let dx=target[0]-source[0];let dz=target[2]-source[2];let distance=dz.mul_add(dz,dx*dx);let radius=self.distance.context("Path-node arrival distance is unwritten")?;
  if self.class==Class::Distance3D{let dy=target[1]-source[1];return Ok(u32::from(dy.mul_add(dy,distance)<radius*radius))}
  if !(distance<radius*radius){return Ok(0)}
  Ok(u32::from((source[1]-target[1]).abs()<self.height.context("Path-node arrival height is unwritten")?))
 }
 pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{
  if self.class==Class::Distance2D5{self.height=Some(if let Some(v)=backend.entity_height(self)?{0.5*v}else{f32::from_bits(0x3f59999a)*backend.unit_scale(self)?.unwrap_or(1.)});}
  self.distance=Some(0.5*backend.unit_scale(self)?.unwrap_or(1.));
  definition::initialize(self,root,&mut Binding{tree,backend})
 }
}
struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Binding<'_,B>{
 fn child_count(&mut self,_:&mut State,r:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(r)?.children.len())?)}
 fn child(&mut self,_:&mut State,r:usize,i:u32)->Result<Option<usize>>{Ok(self.tree.node(r)?.children.get(i as usize).copied())}
 fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
 fn initialize_attribute(&mut self,s:&mut State,n:usize)->Result<bool>{
  let row=self.tree.node(n)?;let name=row.name.as_deref();
  let distance=name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"MaxDist"));
  let height=s.class==Class::Distance2D5 && name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"MaxDeltaHeight"));
  if distance||height{let scale=self.backend.unit_scale(s)?.unwrap_or(1.);let value=self.backend.number(s,row.value.as_deref())? as f32*scale;if 0.<value{if distance{s.distance=Some(value)}else{s.height=Some(value)}}return Ok(true)}
  Ok(name.is_some_and(|v|v.first()==Some(&b'_')))
 }
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::{Value,json};use super::super::ai_navigation_definitions::Node;
 struct Host<'a>{input:&'a Value,trace:Vec<Value>}
 impl DefinitionBackend for Host<'_>{
 fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["scale"]));Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
 fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{self.trace.push(json!(["number"]));Ok(f64::from_bits(self.input["number"].as_u64().unwrap_or(1.25f64.to_bits())))}
 fn entity_height(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["height"]));Ok((self.input["attribute"]==true).then(||f32::from_bits(self.input["entity_height"].as_u64().unwrap_or(2.4f32.to_bits() as u64) as u32)))}
 }
 #[test]fn path_node_arrival_and_definitions_match_original()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_node_reached.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];let class=if c["class"]==0{Class::Distance2D5}else{Class::Distance3D};let mut s=State::construct(class,1,2,b"node\0ignored");assert_eq!(s.name,b"node");assert!(s.distance.is_none());assert!(s.height.is_none());s.reset();assert!(s.distance.is_none());
   s.distance=Some(f32::from_bits(c["distance"].as_u64().unwrap_or(0.5f32.to_bits() as u64) as u32));s.height=Some(f32::from_bits(c["height"].as_u64().unwrap_or(0.5f32.to_bits() as u64) as u32));
   let mut h=Host{input:c,trace:vec![]};let mut m=Memory::default();m.regions.insert(0x74000000,vec![0;0x200]);
   for (ptr,key,defaults) in [(0x74000000,"source",[0;3]),(0x74000100,"target",[0.25f32.to_bits(),0,0])]{for i in 0..3{m.write(ptr+4+i*4,c[key].get(i as usize).and_then(Value::as_u64).unwrap_or(defaults[i as usize] as u64) as u32)?;}}
   let result=if c["op"]=="query"{s.query(&m,0x74000000,0x74000100)?}else{let mut tree=Tree::default();let root=tree.append(None,Node::default())?;for attr in c["attrs"].as_array().unwrap(){tree.append(Some(root),Node{name:attr[0].as_str().map(|v|v.as_bytes().to_vec()),value:attr[1].as_str().map(|v|v.as_bytes().to_vec()),..Node::default()})?;}u32::from(s.initialize_definition(&mut tree,Some(root),&mut h)?)};
   assert_eq!(result,case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.distance.unwrap().to_bits(),case["distance"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.height.unwrap().to_bits(),case["height"].as_u64().unwrap() as u32,"{c}");assert_eq!(h.trace,case["trace"].as_array().unwrap().clone(),"{c}");
   let before=(s.distance.unwrap().to_bits(),s.height.unwrap().to_bits());s.reset();assert_eq!((s.distance.unwrap().to_bits(),s.height.unwrap().to_bits()),before);
  }Ok(())
 }
}
