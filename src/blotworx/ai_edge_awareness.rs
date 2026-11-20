//! Retail edge status awareness4ab20c..4ab5fc. Rebinding restores only the
//! saved head point and clears its graph reference, matching the original.
use anyhow::{Context,Result};
use super::{ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
pub struct State{pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub distance:Option<f32>,pub duration:Option<f32>}
pub trait DefinitionBackend{
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
 fn number(&mut self,s:&mut State,text:Option<&[u8]>)->Result<f64>;
}
pub trait Host{
 type Point:Copy;
 fn clock(&mut self,s:&mut State)->Result<f32>;
 fn service(&mut self,s:&mut State,entity:usize)->Result<Option<usize>>;
 fn handle64(&mut self,s:&mut State,service:usize)->Result<Option<usize>>;
 fn ready(&mut self,s:&mut State,handle:usize)->Result<bool>;
 fn point(&mut self,s:&mut State,handle:usize)->Result<Option<Self::Point>>;
 fn bind(&mut self,s:&mut State,handle:usize,node:u32)->Result<u32>;
 fn position(&mut self,s:&mut State,point:Self::Point)->Result<[f32;3]>;
 fn restore(&mut self,s:&mut State,handle:usize,point:Option<Self::Point>)->Result<()>;
 fn entity_position(&mut self,s:&mut State,entity:usize)->Result<[f32;3]>;
}
impl State{
 pub const CLASS:&'static [u8]=b"Mpd::CEdgeStatusAwareness_Distance2dAndTime";
 pub fn construct(identity:usize,pathfinder:usize,name:&[u8],host:&mut impl DefinitionBackend)->Result<Self>{
  let mut s=Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),distance:None,duration:None};let scale=host.unit_scale(&mut s)?.unwrap_or(1.);s.distance=Some(25.*scale);s.duration=Some(10.);Ok(s)
 }
 pub fn reset(&mut self){} //2b7c00
 pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{definition::initialize(self,root,&mut Definitions{tree,backend})}
 pub fn query(&mut self,entity:usize,start:u32,end:u32,time:f32,enabled:u32,host:&mut impl Host)->Result<u32>{
  if enabled==0{return Ok(0)}
  let now=host.clock(self)?;if time+self.duration.context("Edge awareness duration is unwritten")?<now{return Ok(1)}
  let Some(service)=host.service(self,entity)? else{return Ok(1)};
  let Some(handle)=host.handle64(self,service)? else{return Ok(1)};
  if !host.ready(self,handle)?{return Ok(1)}
  let old=host.point(self,handle)?;
  if host.bind(self,handle,start)?&255==0{host.restore(self,handle,old)?;return Ok(1)}
  let p=host.point(self,handle)?.context("Successful edge start binding has no point")?;let a=host.position(self,p)?;
  if host.bind(self,handle,end)?&255==0{host.restore(self,handle,old)?;return Ok(1)}
  let p=host.point(self,handle)?.context("Successful edge end binding has no point")?;let b=host.position(self,p)?;
  host.restore(self,handle,old)?;
  let p=host.entity_position(self,entity)?;let radius=self.distance.context("Edge awareness distance is unwritten")?;
  Ok(u32::from(near(p,a,b,radius)))
 }
}
fn near(p:[f32;3],a:[f32;3],b:[f32;3],radius:f32)->bool{
 let dx=b[0]-a[0];let dz=b[2]-a[2];let ox=p[0]-a[0];let oz=p[2]-a[2];
 let length=dx*dx+dz*dz;let cross=dz*ox-dx*oz;let r2=radius*radius;
 if cross*cross>length*r2{return false}
 let dot=dz*oz+dx*ox;
 // Preserve4ab56c's squared-dot/length comparison; do not normalize the edge.
 if dot>0. && dot*dot<length{return true}
 let z=a[2]-p[2];let x=a[0]-p[0];if z*z+x*x<r2{return true}
 let z=b[2]-p[2];let x=b[0]-p[0];z*z+x*x<r2
}
struct Definitions<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Definitions<'_,B>{
 fn child_count(&mut self,_:&mut State,r:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(r)?.children.len())?)}
 fn child(&mut self,_:&mut State,r:usize,i:u32)->Result<Option<usize>>{Ok(self.tree.node(r)?.children.get(i as usize).copied())}
 fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
 fn initialize_attribute(&mut self,s:&mut State,n:usize)->Result<bool>{
  let row=self.tree.node(n)?;let name=row.name.as_deref();let distance=name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"AwarenessDistance"));let duration=name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"MemoryDuration"));
  if distance||duration{let Some(text)=row.value.as_deref() else{return Ok(false)};let scale=if distance{self.backend.unit_scale(s)?.unwrap_or(1.)}else{1.};let value=self.backend.number(s,Some(text))? as f32*scale;if 0.<value{if distance{s.distance=Some(value)}else{s.duration=Some(value)}}return Ok(true)}
  Ok(name.is_some_and(|v|v.first()==Some(&b'_')))
 }
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::{Value,json};use super::super::ai_navigation_definitions::Node;
 struct Services<'a>{input:&'a Value,trace:Vec<Value>,point:Option<u32>,graph:u32,binds:usize}
 impl Services<'_>{fn scalar(&self,key:&str,default:f32)->f32{f32::from_bits(self.input[key].as_u64().unwrap_or(default.to_bits() as u64) as u32)}fn vector(&self,key:&str)->[f32;3]{std::array::from_fn(|i|f32::from_bits(self.input[key][i].as_u64().unwrap() as u32))}}
 impl DefinitionBackend for Services<'_>{
 fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["scale"]));Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
 fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{self.trace.push(json!(["number"]));Ok(f64::from_bits(self.input["number"].as_u64().unwrap()))}
 }
 impl Host for Services<'_>{type Point=u32;
 fn clock(&mut self,_:&mut State)->Result<f32>{self.trace.push(json!(["clock"]));Ok(self.scalar("now",5.))}
 fn service(&mut self,_:&mut State,_:usize)->Result<Option<usize>>{self.trace.push(json!(["service"]));Ok((self.input["service"]==true).then_some(1))}
 fn handle64(&mut self,_:&mut State,_:usize)->Result<Option<usize>>{self.trace.push(json!(["handle"]));Ok((self.input["handle"]==true).then_some(2))}
 fn ready(&mut self,_:&mut State,_:usize)->Result<bool>{self.trace.push(json!(["ready"]));Ok(self.input["ready"]==true)}
 fn point(&mut self,_:&mut State,_:usize)->Result<Option<u32>>{Ok(self.point)}
 fn bind(&mut self,_:&mut State,_:usize,node:u32)->Result<u32>{self.trace.push(json!(["bind",node]));self.point=Some(self.binds as u32);self.graph=0x66000000;let result=self.input["bind"][self.binds].as_u64().unwrap() as u32;self.binds+=1;Ok(result)}
 fn position(&mut self,_:&mut State,p:u32)->Result<[f32;3]>{Ok(self.vector(if p==0{"start"}else{"end"}))}
 fn restore(&mut self,_:&mut State,_:usize,point:Option<u32>)->Result<()>{self.point=point;self.graph=0;Ok(())}
 fn entity_position(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(self.vector("position"))}
 }
 #[test]fn edge_awareness_query_and_definitions_match_original()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_edge_awareness.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];let mut h=Services{input:c,trace:vec![],point:Some(99),graph:0x123456,binds:0};let mut s=State::construct(1,2,b"edge\0ignored",&mut h)?;assert_eq!(s.name,b"edge");assert_eq!(s.duration,Some(10.));assert_eq!(s.distance.unwrap().to_bits(),(25.*c["scale"].as_u64().map(|v|f32::from_bits(v as u32)).unwrap_or(1.)).to_bits());h.trace.clear();s.distance=Some(h.scalar("radius",2.));s.duration=Some(h.scalar("duration",10.));
   let result=if c["op"]=="init"{let mut tree=Tree::default();let root=tree.append(None,Node::default())?;for attr in c["attrs"].as_array().unwrap(){tree.append(Some(root),Node{name:attr[0].as_str().map(|v|v.as_bytes().to_vec()),value:attr[1].as_str().map(|v|v.as_bytes().to_vec()),..Node::default()})?;}u32::from(s.initialize_definition(&mut tree,Some(root),&mut h)?)}else{s.query(10,7,8,h.scalar("time",0.),c["enabled"].as_u64().unwrap() as u32,&mut h)?};
   assert_eq!(result,case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(h.trace,case["trace"].as_array().unwrap().clone(),"{c}");
   if c["op"]=="init"{assert_eq!(s.distance.unwrap().to_bits(),case["radius"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.duration.unwrap().to_bits(),case["duration"].as_u64().unwrap() as u32,"{c}");}else{assert_eq!(h.point==Some(99),case["point_restored"].as_bool().unwrap(),"{c}");assert_eq!(h.graph,case["graph"].as_u64().unwrap() as u32,"{c}");}
   let before=(s.distance.unwrap().to_bits(),s.duration.unwrap().to_bits());s.reset();assert_eq!((s.distance.unwrap().to_bits(),s.duration.unwrap().to_bits()),before);
  }Ok(())
 }
}
