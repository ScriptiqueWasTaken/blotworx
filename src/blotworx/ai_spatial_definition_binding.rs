//! Concrete49b274 ->1c32f4 definition configuration for arena-owned spatial
//! objects. Definition strings stay owned by Tree; no second arena string copy.
use anyhow::{Context as _,Result};
use super::{ai_graph_link_passes::Memory,ai_navigation_definitions::Tree,
 ai_definition_strings::{Strings,Field},ai_spatial_modifier::{self,Attributes},ai_traversal_exclusion::Services};
struct Binding<'a,H>{memory:&'a mut Memory,tree:&'a mut Tree,services:&'a mut H,strings:Strings,context:ai_spatial_modifier::Context}
impl<H:Services> Services for Binding<'_,H>{
 fn class20(&mut self,m:&mut Memory,service:u32)->Result<u32>{self.services.class20(m,service)}
}
/// Attribute bridge borrows the same tree and handles without borrowing the
/// outer binding's Memory twice during native attribute callbacks.
struct Attribute<'a,H>{tree:&'a mut Tree,services:&'a mut H,strings:&'a mut Strings}
impl<H:Services> Services for Attribute<'_,H>{
 fn class20(&mut self,m:&mut Memory,service:u32)->Result<u32>{self.services.class20(m,service)}
}
impl<H:Services> Attributes for Attribute<'_,H>{
 fn name(&mut self,_:&mut Memory,node:u32)->Result<Option<u32>>{self.strings.capture(self.tree,usize::try_from(node)?,Field::Name)?.map(u32::try_from).transpose().map_err(Into::into)}
 fn value(&mut self,_:&mut Memory,node:u32)->Result<u32>{Ok(u32::try_from(self.strings.capture(self.tree,usize::try_from(node)?,Field::Value)?.context("Spatial layer value is null")?)?)}
 fn text(&mut self,_:&mut Memory,handle:u32)->Result<Vec<u8>>{self.strings.text(self.tree,usize::try_from(handle)?)}
 fn first(&mut self,m:&mut Memory,handle:u32)->Result<u8>{Ok(self.text(m,handle)?.first().copied().unwrap_or(0))}
}
impl<H:Services> super::ai_navigation_modifier_init::Host<u32> for Binding<'_,H>{
 fn child_count(&mut self,_:&mut u32,root:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(root)?.children.len())?)}
 fn child(&mut self,_:&mut u32,root:usize,index:u32)->Result<Option<usize>>{Ok(self.tree.node(root)?.children.get(index as usize).copied())}
 fn name(&mut self,_:&mut u32,node:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(node)?.name.clone())}
 fn initialize_attribute(&mut self,service:&mut u32,node:usize)->Result<bool>{
  Ok(ai_spatial_modifier::attribute(self.memory,self.context,*service,u32::try_from(node)?,&mut Attribute{tree:self.tree,services:self.services,strings:&mut self.strings})?&255!=0)
 }
}
pub fn configure(memory:&mut Memory,tree:&mut Tree,root:Option<usize>,service:u32,context:ai_spatial_modifier::Context,services:&mut impl Services)->Result<u32>{
 let mut service=service;
 Ok(u32::from(super::ai_navigation_modifier_init::initialize(&mut service,root,&mut Binding{memory,tree,services,strings:Strings::default(),context})?))
}
#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};use super::super::ai_navigation_definitions::Node;
 struct Manager{present:bool,calls:usize}
 impl Services for Manager{fn class20(&mut self,_:&mut Memory,_:u32)->Result<u32>{self.calls+=1;Ok(if self.present{0x41d2c}else{0x60000900})}}
 #[test]
 fn tree_configuration_reuses_native_layer_algorithm_and_canonical_strings()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_modifier.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let input=&case["input"];if input["name_present"]!=true{continue}
   let mut m=Memory{regions:[(0x60000000,vec![0;0x3000]),(0x5a108,vec![0;4])].into(),resources:Default::default(),stride:0,journal:Default::default()};m.write(0x5a108,0x60000100)?;
   for (offset,byte) in case["initial"].as_object().unwrap(){m.write_byte(0x60000000+offset.parse::<u32>()?,byte.as_u64().unwrap() as u8)?;}
   let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
   tree.append(Some(root),Node{name:Some(b"Class".to_vec()),value:Some(b"Mpd::CCanGo".to_vec()),..Node::default()})?;
   tree.append(Some(root),Node{name:Some(input["name"].as_str().unwrap().as_bytes().to_vec()),value:Some(input["value"].as_str().unwrap().as_bytes().to_vec()),..Node::default()})?;
   let mut manager=Manager{present:input["manager"].as_bool().unwrap(),calls:0};
   let result=configure(&mut m,&mut tree,Some(root),0x60000000,ai_spatial_modifier::Context{root_slot:0x5a108,manager_class:0x41d2c},&mut manager)?;
   assert_eq!(json!(result),case["result"],"case {index}");
   let actual:serde_json::Map<String,Value>=(0..0x3000u32).filter_map(|i|match m.byte(0x60000000+i){Ok(0)=>None,Ok(b)=>Some((i.to_string(),json!(b))),Err(e)=>panic!("{e}")}).collect();assert_eq!(json!(actual),case["final"],"case {index}");
   let expected=case["trace"].as_array().unwrap().iter().filter(|v|v.as_str()==Some("class")).count();assert_eq!(manager.calls,expected);
  }Ok(())
 }
}
