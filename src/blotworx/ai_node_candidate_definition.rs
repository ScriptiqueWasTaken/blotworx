use anyhow::Result;
use super::{ai_node_candidate::{State,DefinitionBackend},ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
impl State{
 pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool>{
  self.frame_limit=2;self.distance=50.*backend.unit_scale(self)?.unwrap_or(1.);self.reset();
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
  if name.is_some_and(|name|super::ai_traversal_registry::names_equal(name,b"MaxCandidatePerFrame")){s.frame_limit=self.backend.integer(s,row.value.as_deref())? as u32;return Ok(true)}
  if name.is_some_and(|name|super::ai_traversal_registry::names_equal(name,b"MaxDistPathNode")){let scale=self.backend.unit_scale(s)?.unwrap_or(1.);let v=self.backend.number(s,row.value.as_deref())? as f32*scale;if 0.<=v{s.distance=v;}return Ok(true)}
  if name.is_some_and(|name|super::ai_traversal_registry::names_equal(name,b"NbNodesToSkip")){let v=self.backend.integer(s,row.value.as_deref())?;if v>=0{s.skip_limit=v as u32;}return Ok(true)}
  Ok(name.is_some_and(|name|name.first()==Some(&b'_')))
 }
}
