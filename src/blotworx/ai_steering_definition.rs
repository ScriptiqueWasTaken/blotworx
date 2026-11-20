use anyhow::{Context,Result};
use super::{ai_steering::{State,DefinitionBackend,cosine},ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
impl State{pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{definition::initialize(self,root,&mut Binding{tree,backend})}}
struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Binding<'_,B>{
 fn child_count(&mut self,_:&mut State,r:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(r)?.children.len())?)}
 fn child(&mut self,_:&mut State,r:usize,i:u32)->Result<Option<usize>>{Ok(self.tree.node(r)?.children.get(i as usize).copied())}
 fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
 fn initialize_attribute(&mut self,s:&mut State,n:usize)->Result<bool>{
  let row=self.tree.node(n)?;let name=row.name.as_deref();
  if name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"StrafeRadius")){let scale=self.backend.unit_scale(s)?.unwrap_or(1.);let value=self.backend.number(s,row.value.as_deref())? as f32*scale;if 0.<=value{s.radius=Some(value)}return Ok(true)}
  if name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,b"StrafeAngle")){
   let value=self.backend.number(s,row.value.as_deref())? as f32;let radians=f32::from_bits(0x3c8efa35);s.angle=Some((super::native_angle::acos(s.angle.context("Steering angle is unwritten")? as f64) as f32)/radians);
   if -180.<=value && value<=180.{s.angle=Some(value)}
   let value=cosine(radians*value)?;s.angle=Some(if value>1.{1.}else if value< -1.{-1.}else{value});return Ok(true)
  }
  Ok(name.is_some_and(|v|v.first()==Some(&b'_')))
 }
}
#[cfg(test)]mod tests{
 use super::*;use super::super::{ai_navigation_definitions::Node,ai_steering::tests as runtime};use serde_json::{Value,json};
 #[test]fn simple_biped_definitions_preserve_authored_angle_radius_and_flag()->Result<()>{
 let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_steering.json"))?;
 for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];if c["op"]!="init"{continue}let mut s=runtime::state(c);let mut h=runtime::host(c);let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
 for attr in c["attrs"].as_array().unwrap(){tree.append(Some(root),Node{name:attr[0].as_str().map(|v|v.as_bytes().to_vec()),value:attr[1].as_str().map(|v|v.as_bytes().to_vec()),..Node::default()})?;}
 let result=s.initialize_definition(&mut tree,Some(root),&mut h)?;assert_eq!(u32::from(result),case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.radius.unwrap().to_bits(),case["radius"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.angle.unwrap().to_bits(),case["angle"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.strafe,case["flag"].as_u64().unwrap() as u8);assert_eq!(json!(h.trace),case["trace"],"{c}");let old=(s.radius,s.angle);s.reset();assert_eq!(s.strafe,0);assert_eq!((s.radius,s.angle),old);
 }Ok(())}
}
