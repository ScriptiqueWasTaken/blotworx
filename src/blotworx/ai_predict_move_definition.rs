use anyhow::Result;
use super::{ai_predict_move::{State,DefinitionBackend},ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
impl State{
 pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{
  self.ratio=Some(0.5);
  if !definition::initialize(self,root,&mut Definitions{tree,backend})?{return Ok(false)}
  self.reset();Ok(true)
 }
}
struct Definitions<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Definitions<'_,B>{
 fn child_count(&mut self,_:&mut State,r:usize)->Result<u32>{Ok(u32::try_from(self.tree.node(r)?.children.len())?)}
 fn child(&mut self,_:&mut State,r:usize,i:u32)->Result<Option<usize>>{Ok(self.tree.node(r)?.children.get(i as usize).copied())}
 fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
 fn initialize_attribute(&mut self,s:&mut State,n:usize)->Result<bool>{
  let row=self.tree.node(n)?;
  if row.name.as_deref().is_some_and(|name|super::ai_traversal_registry::names_equal(name,b"AccidentRatio")){
   let value=self.backend.number(s,row.value.as_deref())? as f32;if 0.<value{s.ratio=Some(value);}return Ok(true)
  }
  Ok(row.name.as_ref().is_some_and(|name|name.first()==Some(&b'_')))
 }
}
#[cfg(test)]
mod tests {
 use super::*;use serde_json::{Value,json};use super::super::ai_navigation_definitions::Node;
 struct Runtime{number:f64,trace:Vec<Value>}
 impl DefinitionBackend for Runtime{fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{self.trace.push(json!(["number"]));Ok(self.number)}}
 #[test]
 fn original_prediction_definitions_preserve_default_ratio_and_reset_after_success_only()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_predict_move.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let input=&case["input"];if input["op"]!="init"{continue}
   let mut state=super::super::ai_predict_move::tests::from_fields(case);let mut tree=Tree::default();let root=if let Some(attrs)=input["attrs"].as_array(){let root=tree.append(None,Node::default())?;for row in attrs{tree.append(Some(root),Node{name:Some(row[0].as_str().unwrap().as_bytes().to_vec()),value:Some(row[1].as_str().unwrap().as_bytes().to_vec()),..Node::default()})?;}Some(root)}else{None};
   let mut host=Runtime{number:f32::from_bits(input["number"].as_u64().unwrap() as u32) as f64,trace:vec![]};let result=state.initialize_definition(&mut tree,root,&mut host)?;
   assert_eq!(json!(u32::from(result)),case["result"]);assert_eq!(json!(super::super::ai_predict_move::tests::fields(&state)),case["fields"],"{input}");assert_eq!(json!(host.trace),case["trace"]);
  }Ok(())
 }
}
