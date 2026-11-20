use anyhow::Result;use super::{ai_compute_target::{State,DefinitionBackend},ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
impl State{
 pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{
  self.maxdist=Some(10.*backend.unit_scale(self)?.unwrap_or(1.));self.extra=Some(0.25*backend.unit_scale(self)?.unwrap_or(1.));self.ratio=Some(1.);self.entitydist=Some(2.5*backend.unit_scale(self)?.unwrap_or(1.));self.profiles=Some(backend.profiles(self)?);definition::initialize(self,root,&mut Binding{tree,backend})
 }
}
struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Binding<'_,B>{
 fn child_count(&mut self,_:&mut State,r:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(r)?.children.len())?)}
 fn child(&mut self,_:&mut State,r:usize,i:u32)->Result<Option<usize>>{Ok(self.tree.node(r)?.children.get(i as usize).copied())}
 fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
 fn initialize_attribute(&mut self,s:&mut State,n:usize)->Result<bool>{
  let row=self.tree.node(n)?;let name=row.name.as_deref();
  for (i,key) in [b"MaxDist".as_slice(),b"ExtraGap",b"MaxDeltaHeightRatio",b"MaxEntityDist"].into_iter().enumerate(){
   if name.is_some_and(|v|super::ai_traversal_registry::names_equal(v,key)){let scale=if i==2{1.}else{self.backend.unit_scale(s)?.unwrap_or(1.)};let value=self.backend.number(s,row.value.as_deref())? as f32*scale;
    if 0.<=value{match i{0=>s.maxdist=Some(value),1=>s.extra=Some(value),2=>s.ratio=Some(value),_=>s.entitydist=Some(value)}}return Ok(true)
   }
  }Ok(name.is_some_and(|v|v.first()==Some(&b'_')))
 }
}
#[cfg(test)]mod tests{
 use super::*;use super::super::{ai_compute_target::tests as runtime,ai_navigation_definitions::Node};use serde_json::{Value,json};
 #[test]fn queue_definitions_defaults_units_service_lookup_and_unknowns_match_original()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_compute_target.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];if c["op"]!="init"{continue}let mut s=runtime::state(c);let mut h=runtime::host(c);let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
   for attr in c["attrs"].as_array().unwrap(){tree.append(Some(root),Node{name:attr[0].as_str().map(|v|v.as_bytes().to_vec()),value:attr[1].as_str().map(|v|v.as_bytes().to_vec()),..Node::default()})?;}
   let result=s.initialize_definition(&mut tree,Some(root),&mut h)?;assert_eq!(u32::from(result),case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(json!(runtime::image(&s)),case["fields"],"{c}");assert_eq!(json!(h.trace),case["trace"],"{c}");assert_eq!(s.profiles.flatten().is_some(),case["profiles"].as_bool().unwrap());
  }Ok(())
 }
}
