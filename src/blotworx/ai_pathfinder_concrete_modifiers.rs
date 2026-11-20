//! Actual concrete modifier/default dispatch shared by definition and default
//! passes. Metadata is supplied in original registration order. Objects have
//! one owner; arena spatial objects are not mirrored as typed records.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_pathfinder_constructor::State,ai_pathfinder_modifiers::{Kind,Storage},
 ai_graph_link_passes::Memory,ai_navigation_definitions::Tree,ai_pathfinder_modifier_factory::{Owned,DefinitionBackend},
 ai_pathfinder_definition_binding as definitions,ai_pathfinder_spatial_construction as spatial};
pub struct Class {pub identity:usize,pub kind:Kind,pub name:Vec<u8>,pub parent:Option<usize>,pub vtable:Option<u32>}
pub struct Metadata {pub classes:Vec<Class>,pub defaults:[Option<usize>;13]}
impl Metadata {
 fn get(&self,id:usize)->Result<&Class>{self.classes.iter().find(|c|c.identity==id).context("Modifier class metadata is absent")}
 fn named(&self,kind:Kind,name:&[u8])->Option<&Class>{let name=&name[..name.iter().position(|&b|b==0).unwrap_or(name.len())];self.classes.iter().find(|c|c.kind==kind && c.name.as_slice()==name)}
}
pub trait Backend:DefinitionBackend+spatial::Allocator {
 fn parent_entity(&mut self,parent:&mut State,memory:&mut Memory)->Result<usize>;
 fn parent_height(&mut self,parent:&mut State,memory:&mut Memory,entity:usize)->Result<Option<f32>>;
 fn pathfinder_integer(&mut self,state:&mut State,text:Option<&[u8]>)->Result<i32>;
 fn pathfinder_number(&mut self,state:&mut State,text:&[u8])->Result<f64>;
 fn pathfinder_scale(&mut self,state:&mut State)->Result<Option<f32>>;
 fn find_database(&mut self,state:&mut State,manager:usize,name:&[u8])->Result<Option<usize>>;
 fn register_database_callback(&mut self,state:&mut State,database:usize)->Result<()>;
 fn allocate_typed(&mut self,state:&mut State,memory:&mut Memory,class:&Class)->Result<usize>;
 fn allocate_table(&mut self,state:&mut State,memory:&mut Memory,kind:Kind,capacity:u32)->Result<Storage>;
 fn free_table(&mut self,state:&mut State,memory:&mut Memory,kind:Kind,storage:usize)->Result<()>;
}
pub struct Binding<'a,B>{pub metadata:&'a Metadata,pub objects:&'a mut BTreeMap<usize,Owned>,pub memory:&'a mut Memory,pub spatial:super::ai_spatial_construction::Context,pub backend:&'a mut B}
impl<B:Backend> Binding<'_,B>{
 pub fn configure(&mut self,state:&mut State,tree:&mut Tree,root:usize)->Result<bool>{definitions::Binding::new(tree,self).initialize(state,root)}
 pub fn initialize_defaults(&mut self,state:&mut State,tree:&mut Tree)->Result<()>{super::ai_pathfinder_modifiers::initialize_defaults(state,&mut definitions::Binding::new(tree,self))}

 fn validate(&self,state:&State,kind:Kind,id:usize)->Result<&Owned>{
  let o=self.objects.get(&id).context("Concrete modifier object is absent")?;
  ensure!(o.identity()==id && o.kind()==kind && o.pathfinder(self.memory)?==state.identity,"Concrete modifier identity, class or owner differs");Ok(o)
 }
 fn construct(&mut self,state:&mut State,kind:Kind,class:usize,name:&[u8],default:bool)->Result<usize>{
  let descriptor=self.metadata.get(class)?;ensure!(descriptor.kind==kind,"Modifier metadata belongs to another interface");
  let object=if descriptor.kind==Kind(3) && descriptor.name==Owned::SPATIAL_CLASS {
   // The real constructor never invokes configure; Definitions is supplied so
   // its Host still has actual configuration if initialization is called.
   let mut tree=Tree::default();let mut host=spatial::Definitions{tree:&mut tree,root:None,context:super::ai_spatial_modifier::Context{root_slot:self.spatial.root_slot,manager_class:self.spatial.manager_class},backend:self.backend};
   Owned::construct_spatial_text(state,self.memory,self.spatial,name,default,&mut host)?
  }else{
   let identity=self.backend.allocate_typed(state,self.memory,descriptor)?;ensure!(identity!=0,"Typed modifier allocation is null");
   let parent=state.identity;
   let mut scope=Scoped{parent:state,memory:self.memory,backend:self.backend};
   if kind==Kind(9) && descriptor.name==super::ai_steering::State::CLASS{Owned::Steering(Box::new(super::ai_steering::State::construct(identity,parent,name,&mut scope)?))}else if kind==Kind(12) && descriptor.name==super::ai_edge_awareness::State::CLASS{Owned::EdgeAwareness(Box::new(super::ai_edge_awareness::State::construct(identity,parent,name,&mut scope)?))}else{Owned::construct_typed(kind,&descriptor.name,identity,parent,name,&mut scope)?}
  };
  let id=object.identity();ensure!(id!=0,"Modifier constructor returned a null identity");ensure!(!self.objects.contains_key(&id),"Modifier allocation overlaps an existing object");self.objects.insert(id,object);Ok(id)
 }
 fn initialize(&mut self,state:&mut State,kind:Kind,id:usize,tree:&mut Tree,node:Option<usize>)->Result<bool>{
  self.validate(state,kind,id)?;
  let object=self.objects.get_mut(&id).context("Modifier owner disappeared")?;
  if matches!(object,Owned::Spatial{..}){object.initialize_spatial(state,self.memory,self.spatial,tree,node,self.backend)}else{object.initialize_definition(tree,node,&mut Scoped{parent:state,memory:self.memory,backend:self.backend})}
 }
}
impl<B:Backend> definitions::Backend for Binding<'_,B>{
 fn integer(&mut self,s:&mut State,t:Option<&[u8]>)->Result<i32>{self.backend.pathfinder_integer(s,t)}
 fn number(&mut self,s:&mut State,t:&[u8])->Result<f64>{self.backend.pathfinder_number(s,t)}
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>{self.backend.pathfinder_scale(s)}
 fn find_database(&mut self,s:&mut State,m:usize,n:&[u8])->Result<Option<usize>>{self.backend.find_database(s,m,n)}
 fn register_database_callback(&mut self,s:&mut State,d:usize)->Result<()>{self.backend.register_database_callback(s,d)}
 fn class(&mut self,_:&mut State,k:Kind,n:&[u8])->Result<Option<usize>>{Ok(self.metadata.named(k,n).map(|c|c.identity))}
 fn construct(&mut self,s:&mut State,k:Kind,c:usize,n:&[u8])->Result<usize>{self.construct(s,k,c,n,false)}
 fn initialize_modifier(&mut self,s:&mut State,k:Kind,o:usize,t:&mut Tree,n:usize)->Result<bool>{self.initialize(s,k,o,t,Some(n))}
 fn object_name(&mut self,s:&mut State,k:Kind,o:usize)->Result<Vec<u8>>{Ok(self.validate(s,k,o)?.name(self.memory)?.into_owned())}
 fn allocate_table(&mut self,s:&mut State,k:Kind,c:u32)->Result<Storage>{self.backend.allocate_table(s,self.memory,k,c)}
 fn free_table(&mut self,s:&mut State,k:Kind,o:usize)->Result<()>{self.backend.free_table(s,self.memory,k,o)}
}
impl<B:Backend> definitions::DefaultsBackend for Binding<'_,B>{
 fn default_class(&self,k:Kind)->Result<usize>{let id=self.metadata.defaults.get(k.0).copied().flatten().context("Default modifier class metadata is absent")?;ensure!(self.metadata.get(id)?.kind==k,"Default modifier metadata interface differs");Ok(id)}
 fn object_class(&mut self,s:&mut State,k:Kind,o:usize)->Result<usize>{
  let object=self.validate(s,k,o)?;
  let descriptor=match object{
   Owned::Spatial{address}=>{let vtable=self.memory.word(*address)?;self.metadata.classes.iter().find(|c|c.kind==k && c.vtable==Some(vtable)).context("Spatial virtual class getter is not implemented for this vtable")?},
   Owned::FindNodes(_)=>self.metadata.named(k,super::ai_find_nodes::State::CLASS).context("Nearest reachable class metadata absent")?,
   Owned::ComputeTarget(_)=>self.metadata.named(k,super::ai_compute_target::State::CLASS).context("Queue target class metadata absent")?,
   Owned::Steering(_)=>self.metadata.named(k,super::ai_steering::State::CLASS).context("Biped steering class metadata absent")?,
   Owned::EdgeAwareness(_)=>self.metadata.named(k,super::ai_edge_awareness::State::CLASS).context("Edge awareness class metadata absent")?,
   Owned::NodeReached(s)=>self.metadata.named(k,s.class.name()).context("Node arrival class metadata absent")?,
   Owned::NodeCandidate(_)=>self.metadata.named(k,super::ai_node_candidate::State::CLASS).context("Candidate class metadata absent")?,
   Owned::Accident(_)=>self.metadata.named(k,super::ai_predict_move::State::CLASS).context("Accident class metadata absent")?,
   Owned::GoalDetection(s)=>self.metadata.named(k,s.class().name()).context("Goal class metadata absent")?,
   Owned::GapAvoidance(_)=>self.metadata.named(k,Owned::GAP_CLASS).context("Gap class metadata absent")?,
   Owned::RefineGoal(_)=>self.metadata.named(k,Owned::REFINE_CLASS).context("Refinement class metadata absent")?,
  };Ok(descriptor.identity)
 }
 fn parent_class(&mut self,_:&mut State,c:usize)->Result<Option<usize>>{Ok(self.metadata.get(c)?.parent)}
 fn construct_default(&mut self,s:&mut State,k:Kind,c:usize,n:&[u8])->Result<Option<usize>>{Ok(Some(self.construct(s,k,c,n,true)?))}
 fn initialize_default(&mut self,s:&mut State,k:Kind,o:usize,t:&mut Tree)->Result<bool>{self.initialize(s,k,o,t,None)}
}

struct Reset<'a,H>{objects:&'a mut BTreeMap<usize,Owned>,host:&'a mut H}
impl<H:super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host> super::ai_pathfinder_reset::Host for Reset<'_,H>{
 fn detach_goal(&mut self,s:&mut State,m:&mut Memory,g:u32)->Result<()>{self.host.detach_goal(s,m,g)}
 fn release(&mut self,s:&mut State,m:&mut Memory,manager:usize,reference:u32)->Result<()>{self.host.release(s,m,manager,reference)}
 fn reset_modifier(&mut self,s:&mut State,m:&mut Memory,k:Kind,id:usize)->Result<()>{
  let object=self.objects.get_mut(&id).context("Concrete reset modifier is absent")?;
  ensure!(object.identity()==id && object.kind()==k && object.pathfinder(m)?==s.identity,"Concrete reset modifier identity, class or owner differs");
  object.reset(self.host)
 }
}
///495d40 uses the same registry populated by construction, while goal detach
/// and graph release still dispatch to the actual owning runtime services.
pub fn reset_owned<H:super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host>(state:&mut State,memory:&mut Memory,objects:&mut BTreeMap<usize,Owned>,host:&mut H)->Result<()>{
 super::ai_pathfinder_reset::reset(state,memory,&mut Reset{objects,host})
}

/// Borrowed for a constructor/initializer only; no parent is removed from its
/// owning runtime and no duplicate identity-to-owner map is maintained.
struct Scoped<'a,B>{parent:&'a mut State,memory:&'a mut Memory,backend:&'a mut B}
impl<B:Backend> super::ai_goto_gap::Host for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_goto_gap::State)->Result<Option<f32>>{super::ai_goto_gap::Host::unit_scale(self.backend,s)}
 fn owner_entity(&mut self,s:&mut super::ai_goto_gap::State)->Result<usize>{ensure!(s.pathfinder==self.parent.identity,"Steering constructor parent differs");self.backend.parent_entity(self.parent,self.memory)}
 fn profiling_enabled(&mut self,s:&mut super::ai_goto_gap::State)->Result<bool>{super::ai_goto_gap::Host::profiling_enabled(self.backend,s)}
 fn register_budget(&mut self,s:&mut super::ai_goto_gap::State,n:&str)->Result<i32>{super::ai_goto_gap::Host::register_budget(self.backend,s,n)}
}
impl<B:Backend> super::ai_goto_gap_setup::Host for Scoped<'_,B>{
 fn world_service(&mut self,s:&mut super::ai_goto_gap::State,k:super::ai_goto_gap_setup::Service)->Result<Option<usize>>{super::ai_goto_gap_setup::Host::world_service(self.backend,s,k)}
 fn entity_attribute(&mut self,s:&mut super::ai_goto_gap::State,e:usize,k:super::ai_goto_gap_setup::Attribute)->Result<Option<usize>>{super::ai_goto_gap_setup::Host::entity_attribute(self.backend,s,e,k)}
 fn cache34(&mut self,s:&mut super::ai_goto_gap::State,n:u32)->Result<Option<super::ai_goto_gap::Pool<super::ai_goto_gap::Cache34>>>{super::ai_goto_gap_setup::Host::cache34(self.backend,s,n)}
 fn cache24(&mut self,s:&mut super::ai_goto_gap::State,n:u32)->Result<Option<super::ai_goto_gap::Pool<super::ai_goto_gap::Cache24>>>{super::ai_goto_gap_setup::Host::cache24(self.backend,s,n)}
}
impl<B:Backend> super::ai_goto_gap_definition_binding::Backend for Scoped<'_,B>{
 fn number(&mut self,s:&mut super::ai_goto_gap::State,t:Option<&[u8]>)->Result<f64>{super::ai_goto_gap_definition_binding::Backend::number(self.backend,s,t)}
 fn integer(&mut self,s:&mut super::ai_goto_gap::State,t:Option<&[u8]>)->Result<i32>{super::ai_goto_gap_definition_binding::Backend::integer(self.backend,s,t)}
 fn diameter_attribute(&mut self,s:&mut super::ai_goto_gap::State,e:usize)->Result<Option<usize>>{super::ai_goto_gap_definition_binding::Backend::diameter_attribute(self.backend,s,e)}
 fn diameter_value(&mut self,s:&mut super::ai_goto_gap::State,a:usize)->Result<f32>{super::ai_goto_gap_definition_binding::Backend::diameter_value(self.backend,s,a)}
}
impl<B:Backend> super::ai_goal_detection::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_goal_detection::State)->Result<Option<f32>>{super::ai_goal_detection::DefinitionBackend::unit_scale(self.backend,s)}
 fn number(&mut self,s:&mut super::ai_goal_detection::State,t:Option<&[u8]>)->Result<f64>{super::ai_goal_detection::DefinitionBackend::number(self.backend,s,t)}
 fn entity_height(&mut self,s:&mut super::ai_goal_detection::State)->Result<Option<f32>>{
  ensure!(s.pathfinder==self.parent.identity,"Goal detector parent differs");let entity=self.backend.parent_entity(self.parent,self.memory)?;
  self.backend.parent_height(self.parent,self.memory,entity)
 }
}
impl<B:Backend> super::ai_predict_move::DefinitionBackend for Scoped<'_,B>{
 fn number(&mut self,s:&mut super::ai_predict_move::State,t:Option<&[u8]>)->Result<f64>{super::ai_predict_move::DefinitionBackend::number(self.backend,s,t)}
}
impl<B:Backend> super::ai_node_candidate::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_node_candidate::State)->Result<Option<f32>>{ensure!(s.pathfinder==self.parent.identity,"Candidate parent differs");super::ai_node_candidate::DefinitionBackend::unit_scale(self.backend,s)}
 fn integer(&mut self,s:&mut super::ai_node_candidate::State,t:Option<&[u8]>)->Result<i32>{ensure!(s.pathfinder==self.parent.identity,"Candidate parent differs");super::ai_node_candidate::DefinitionBackend::integer(self.backend,s,t)}
 fn number(&mut self,s:&mut super::ai_node_candidate::State,t:Option<&[u8]>)->Result<f64>{ensure!(s.pathfinder==self.parent.identity,"Candidate parent differs");super::ai_node_candidate::DefinitionBackend::number(self.backend,s,t)}
}
impl<B:Backend> super::ai_node_reached::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_node_reached::State)->Result<Option<f32>>{super::ai_node_reached::DefinitionBackend::unit_scale(self.backend,s)}
 fn number(&mut self,s:&mut super::ai_node_reached::State,t:Option<&[u8]>)->Result<f64>{super::ai_node_reached::DefinitionBackend::number(self.backend,s,t)}
 fn entity_height(&mut self,s:&mut super::ai_node_reached::State)->Result<Option<f32>>{ensure!(s.pathfinder==self.parent.identity,"Node detector parent differs");let entity=self.backend.parent_entity(self.parent,self.memory)?;self.backend.parent_height(self.parent,self.memory,entity)}
}
impl<B:Backend> super::ai_edge_awareness::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_edge_awareness::State)->Result<Option<f32>>{super::ai_edge_awareness::DefinitionBackend::unit_scale(self.backend,s)}
 fn number(&mut self,s:&mut super::ai_edge_awareness::State,t:Option<&[u8]>)->Result<f64>{super::ai_edge_awareness::DefinitionBackend::number(self.backend,s,t)}
}
impl<B:Backend> super::ai_steering::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_steering::State)->Result<Option<f32>>{super::ai_steering::DefinitionBackend::unit_scale(self.backend,s)}
 fn number(&mut self,s:&mut super::ai_steering::State,t:Option<&[u8]>)->Result<f64>{super::ai_steering::DefinitionBackend::number(self.backend,s,t)}
}
impl<B:Backend> super::ai_compute_target::DefinitionBackend for Scoped<'_,B>{
 fn unit_scale(&mut self,s:&mut super::ai_compute_target::State)->Result<Option<f32>>{super::ai_compute_target::DefinitionBackend::unit_scale(self.backend,s)}
 fn profiles(&mut self,s:&mut super::ai_compute_target::State)->Result<Option<usize>>{super::ai_compute_target::DefinitionBackend::profiles(self.backend,s)}
 fn number(&mut self,s:&mut super::ai_compute_target::State,t:Option<&[u8]>)->Result<f64>{super::ai_compute_target::DefinitionBackend::number(self.backend,s,t)}
}
#[cfg(test)]
mod tests{
 use super::*;use super::super::{ai_navigation_entity as entity,ai_navigation_construction::{Constructed,ActionClass,ActionFactory,Layout},ai_navigation_action::{Registry as Actions,Class as Action},ai_goal_detection as goal};
 use serde_json::Value;
 struct Shape;
 impl entity::Host for Shape{
  fn radius(&mut self,_:&mut entity::Owned)->Result<f32>{Ok(0.4)}
  fn height(&mut self,_:&mut entity::Owned)->Result<f32>{Ok(2.4)}
  fn maximum_speed(&mut self,_:&mut entity::Owned)->Result<f32>{Ok(1.)}
  fn unit_scale(&mut self,_:&mut entity::Owned)->Result<Option<f32>>{Ok(Some(1.))}
 }
 fn registry()->entity::Registry{entity::Registry{count:3,diameter:entity::Class{index:0,constructor:Some(entity::Kind::Diameter)},height:entity::Class{index:1,constructor:Some(entity::Kind::Height)},maximum_speed:entity::Class{index:2,constructor:Some(entity::Kind::MaximumSpeed)}}}
 struct Runtime{brains:BTreeMap<usize,Constructed>,entities:BTreeMap<usize,entity::Owned>,registry:entity::Registry,scale:Option<f32>,next:usize,seen:Vec<(usize,usize)>}
 impl Runtime{
  fn new()->Result<Self>{
   let registry=registry();let mut brains=BTreeMap::new();let mut entities=BTreeMap::new();
   for (brain,owner) in [(20,42),(21,43)]{
    let actions=Actions{count:2,heading:Action{index:0,constructor:true},speed:Action{index:1,constructor:true}};
    brains.insert(brain,Constructed::new(brain,Some(owner),Some(brain+100),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},&Layout{agent_classes:0,service_classes:1,attributes:actions})?);
    let mut owned=entity::Owned::new(owner,Some(b"fixture actor"),[0;2],&registry,&mut Shape)?;owned.entity.height=999.;entities.insert(owner,owned);
   }
   Ok(Self{brains,entities,registry,scale:Some(1.),next:500,seen:vec![]})
  }
 }
 impl super::super::ai_goto_gap::Host for Runtime{
  fn unit_scale(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<Option<f32>>{Ok(self.scale)}
  fn owner_entity(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<usize>{anyhow::bail!("Must use scoped canonical parent")}
  fn profiling_enabled(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<bool>{Ok(false)}
  fn register_budget(&mut self,_:&mut super::super::ai_goto_gap::State,_:&str)->Result<i32>{unreachable!()}
 }
 impl super::super::ai_goto_gap_setup::Host for Runtime{
  fn world_service(&mut self,_:&mut super::super::ai_goto_gap::State,_:super::super::ai_goto_gap_setup::Service)->Result<Option<usize>>{anyhow::bail!("No gap setup in constructor/goal fixture")}
  fn entity_attribute(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize,_:super::super::ai_goto_gap_setup::Attribute)->Result<Option<usize>>{unreachable!()}
 }
 impl super::super::ai_goto_gap_definition_binding::Backend for Runtime{
  fn number(&mut self,_:&mut super::super::ai_goto_gap::State,_:Option<&[u8]>)->Result<f64>{unreachable!()}
  fn integer(&mut self,_:&mut super::super::ai_goto_gap::State,_:Option<&[u8]>)->Result<i32>{unreachable!()}
  fn diameter_attribute(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize)->Result<Option<usize>>{unreachable!()}
  fn diameter_value(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize)->Result<f32>{unreachable!()}
 }
 impl super::super::ai_predict_move::DefinitionBackend for Runtime{
  fn number(&mut self,_:&mut super::super::ai_predict_move::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 }
 impl super::super::ai_node_candidate::DefinitionBackend for Runtime{
  fn unit_scale(&mut self,_:&mut super::super::ai_node_candidate::State)->Result<Option<f32>>{Ok(self.scale)}
  fn integer(&mut self,_:&mut super::super::ai_node_candidate::State,t:Option<&[u8]>)->Result<i32>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
  fn number(&mut self,_:&mut super::super::ai_node_candidate::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 }
 impl super::super::ai_node_reached::DefinitionBackend for Runtime{
 fn unit_scale(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{Ok(self.scale)}
 fn number(&mut self,_:&mut super::super::ai_node_reached::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 fn entity_height(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{anyhow::bail!("Node height must use scoped owner")}
 }
 impl super::super::ai_edge_awareness::DefinitionBackend for Runtime{
 fn unit_scale(&mut self,_:&mut super::super::ai_edge_awareness::State)->Result<Option<f32>>{Ok(self.scale)}
 fn number(&mut self,_:&mut super::super::ai_edge_awareness::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 }
 impl super::super::ai_steering::DefinitionBackend for Runtime{
 fn unit_scale(&mut self,_:&mut super::super::ai_steering::State)->Result<Option<f32>>{Ok(self.scale)}
 fn number(&mut self,_:&mut super::super::ai_steering::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 }
 impl super::super::ai_compute_target::DefinitionBackend for Runtime{
 fn unit_scale(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<f32>>{Ok(self.scale)}
 fn profiles(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<usize>>{Ok(Some(900))}
 fn number(&mut self,_:&mut super::super::ai_compute_target::State,t:Option<&[u8]>)->Result<f64>{Ok(std::str::from_utf8(t.context("Missing fixture value")?)?.parse()?)}
 }
 impl goal::DefinitionBackend for Runtime{
  fn unit_scale(&mut self,_:&mut goal::State)->Result<Option<f32>>{Ok(self.scale)}
  fn number(&mut self,_:&mut goal::State,_:Option<&[u8]>)->Result<f64>{unreachable!()}
  fn entity_height(&mut self,_:&mut goal::State)->Result<Option<f32>>{anyhow::bail!("Must use scoped parent height")}
 }
 impl spatial::Allocator for Runtime{
  fn class20(&mut self,_:&mut State,_:&mut Memory,_:u32)->Result<u32>{unreachable!()}
  fn allocate(&mut self,_:&mut State,_:&mut Memory,_:u32)->Result<super::super::ai_spatial_construction::Allocation>{unreachable!()}
  fn observe(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32,_:u32,_:u32)->Result<()>{unreachable!()}
 }
 impl Backend for Runtime{
  fn parent_entity(&mut self,p:&mut State,_:&mut Memory)->Result<usize>{p.word_30+=1;let e=super::super::ai_navigation_owner_queries::entity(p,&self.brains)?;self.seen.push((p.base.owner,e));Ok(e)}
  fn parent_height(&mut self,p:&mut State,_:&mut Memory,e:usize)->Result<Option<f32>>{p.word_38+=1;super::super::ai_navigation_owner_queries::scalar(e,&self.entities,&self.registry,entity::Kind::Height)}
  fn pathfinder_integer(&mut self,_:&mut State,_:Option<&[u8]>)->Result<i32>{unreachable!()}
  fn pathfinder_number(&mut self,_:&mut State,_:&[u8])->Result<f64>{unreachable!()}
  fn pathfinder_scale(&mut self,_:&mut State)->Result<Option<f32>>{Ok(self.scale)}
  fn find_database(&mut self,_:&mut State,_:usize,_:&[u8])->Result<Option<usize>>{unreachable!()}
  fn register_database_callback(&mut self,_:&mut State,_:usize)->Result<()>{unreachable!()}
  fn allocate_typed(&mut self,_:&mut State,_:&mut Memory,_:&Class)->Result<usize>{self.next+=1;Ok(self.next)}
  fn allocate_table(&mut self,_:&mut State,_:&mut Memory,_:Kind,_:u32)->Result<Storage>{unreachable!()}
  fn free_table(&mut self,_:&mut State,_:&mut Memory,_:Kind,_:usize)->Result<()>{unreachable!()}
 }
 fn metadata()->Metadata{
  let mut defaults=[None;13];defaults[1]=Some(0x11930);defaults[8]=Some(0x6fb1c);
  Metadata{classes:vec![Class{identity:0x11930,kind:Kind(1),name:goal::Class::Reached2D5.name().to_vec(),parent:None,vtable:None},Class{identity:0x6fb1c,kind:Kind(8),name:Owned::GAP_CLASS.to_vec(),parent:None,vtable:None}],defaults}
 }
 fn spatial_context()->super::super::ai_spatial_construction::Context{super::super::ai_spatial_construction::Context{root_slot:0,manager_class:0,observer_slot:0,service_kind:0,parent_kind:0,interface_kind:0,pool_kind:0,node_kind:0,service_metadata:0,registry_flag:0,registry:0}}
 #[test]
 fn scoped_goal_height_replays_original_defaults_through_canonical_entity_attributes()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goal_detection.json"))?;let mut count=0;
  for case in fixture["cases"].as_array().unwrap(){let input=&case["input"];if input["op"]!="init" || input["class"]!=0 || input["attrs"].as_array().is_none_or(|a|!a.is_empty()){continue}
   let mut host=Runtime::new()?;host.scale=input["scale"].as_u64().map(|v|f32::from_bits(v as u32));
   let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;state.base.owner=21;
   let entity=host.entities.get_mut(&43).unwrap();let slot=entity.entity.attributes[1].unwrap();
   entity.scalars.get_mut(&slot).unwrap().value_bits=input["entity_height"].as_u64().map(|v|v as u32).unwrap_or(2.4f32.to_bits());
   if input["attribute"]==false{entity.entity.attributes[1]=None;}
   let metadata=metadata();let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();
   let id={let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut host};
    let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(1),0x11930,b"default")?.unwrap();
    assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(1),id,&mut tree)?);id};
   let Owned::GoalDetection(g)=&objects[&id] else{unreachable!()};let goal::Fields::Reached2D5{flag,height}=g.fields else{unreachable!()};
   assert_eq!(height.unwrap().to_bits(),case["word_10"].as_u64().unwrap() as u32,"{input}");assert_eq!(flag,Some(0));assert_eq!(host.seen,[(21,43)]);assert_eq!((state.word_30,state.word_38),(65,33));count+=1;
  }assert!(count>1);Ok(())
 }
 #[test]
 fn scoped_steering_construction_reads_current_brain_without_parent_mirror()->Result<()>{
  let mut host=Runtime::new()?;let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let metadata=metadata();let mut objects=BTreeMap::new();let mut memory=Memory::default();
  for brain in [20,21]{state.base.owner=brain;
   let id={let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut host};definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(8),0x6fb1c,b"default")?.unwrap()};
   let Owned::GapAvoidance(g)=&objects[&id] else{unreachable!()};assert_eq!(g.entity_58,Some(if brain==20{42}else{43}));assert_eq!(g.pathfinder,state.identity);
  }
  assert_eq!(host.seen,[(20,42),(21,43)]);assert_eq!(state.word_30,66);Ok(())
 }
 #[test]
 fn optional_scalar_lookup_matches_original_direct_slot_pointer_semantics()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_entity_direct_attribute.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let mut registry=registry();registry.height.index=case["index"].as_i64().unwrap() as i32;
   let mut entity=entity::Owned::new(42,None,[0;2],&self::registry(),&mut Shape)?;
   entity.entity.attributes=vec![case["slot"].as_bool().unwrap().then_some(7);case["count"].as_u64().unwrap() as usize];
   entity.scalars.clear();entity.scalars.insert(7,entity::Scalar{kind:entity::Kind::Diameter,value_bits:case["value"].as_u64().unwrap() as u32});
   let value=entity.optional_scalar(entity::Kind::Height,&registry)?;
   assert_eq!(value.is_some(),case["result"].as_u64().unwrap()!=0,"{case}");if let Some(v)=value{assert_eq!(v.value_bits,case["value"].as_u64().unwrap() as u32);}
  }Ok(())
 }
 #[test]
 fn prediction_default_and_named_configuration_use_concrete_factory_and_reset()->Result<()>{
  use super::super::{ai_predict_move::State as Predict,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x118e0,kind:Kind(4),name:Predict::CLASS.to_vec(),parent:None,vtable:None});metadata.defaults[4]=Some(0x118e0);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();
  let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(4),0x118e0,b"default")?.unwrap();
  let Owned::Accident(p)=&binding.objects[&id] else{unreachable!()};assert!(p.ratio.is_none());assert!(p.remaining.is_none());assert_eq!(p.predicted,[0.;3]);
  assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(4),id,&mut tree)?);
  let Owned::Accident(p)=&binding.objects[&id] else{unreachable!()};assert_eq!(p.ratio,Some(0.5));assert_eq!(p.predicted,[f32::MAX;3]);
  assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(4),id)?,0x118e0);
  let root=tree.append(None,Node::default())?;tree.append(Some(root),Node{name:Some(b"AccidentRatio".to_vec()),value:Some(b"0.25".to_vec()),..Node::default()})?;
  assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(4),id,&mut tree,root)?);
  let owned=binding.objects.get_mut(&id).unwrap();let Owned::Accident(p)=owned else{unreachable!()};assert_eq!(p.ratio,Some(0.25));p.predicted=[1.,2.,3.];
  owned.reset(binding.backend)?;let Owned::Accident(p)=owned else{unreachable!()};assert_eq!(p.ratio,Some(0.25));assert_eq!(p.predicted,[f32::MAX;3]);Ok(())
 }

 #[test]fn candidate_default_definitions_and_reset_use_same_owned_object()->Result<()>{
  use super::super::{ai_node_candidate::State as Candidate,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x11a98,kind:Kind(5),name:Candidate::CLASS.to_vec(),parent:None,vtable:None});metadata.defaults[5]=Some(0x11a98);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();
  let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(5),0x11a98,b"default")?.unwrap();
  assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(5),id,&mut tree)?);
  assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(5),id)?,0x11a98);
  let Owned::NodeCandidate(p)=&binding.objects[&id] else{unreachable!()};assert_eq!(p.frame_limit,2);assert_eq!(p.skip_limit,4);assert_eq!(p.previous,u32::MAX);
  let root=tree.append(None,Node::default())?;
  for (name,value) in [("MaxCandidatePerFrame","3"),("MaxDistPathNode","12.5"),("NbNodesToSkip","7")]{tree.append(Some(root),Node{name:Some(name.as_bytes().to_vec()),value:Some(value.as_bytes().to_vec()),..Node::default()})?;}
  assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(5),id,&mut tree,root)?);
  let owned=binding.objects.get_mut(&id).unwrap();let Owned::NodeCandidate(p)=owned else{unreachable!()};assert_eq!((p.frame_limit,p.distance,p.skip_limit),(3,12.5,7));p.previous=99;p.frame_count=2;p.skipped=3;
  owned.reset(binding.backend)?;let Owned::NodeCandidate(p)=owned else{unreachable!()};assert_eq!((p.frame_limit,p.distance,p.skip_limit,p.previous),(3,12.5,7,99));assert_eq!((p.frame_count,p.skipped),(0,0));Ok(())
 }

 #[test]fn path_node_arrival_factory_initializes_actual_parent_height_and_preserves_reset()->Result<()>{
  use super::super::{ai_node_reached::Class as NodeClass,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();
  for (identity,class) in [(0x11980,NodeClass::Distance2D5),(0x119a8,NodeClass::Distance3D)]{metadata.classes.push(Class{identity,kind:Kind(6),name:class.name().to_vec(),parent:None,vtable:None});}metadata.defaults[6]=Some(0x11980);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();
  let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(6),0x11980,b"default")?.unwrap();
  assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(6),id,&mut tree)?);
  assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(6),id)?,0x11980);
  let Owned::NodeReached(p)=&binding.objects[&id] else{unreachable!()};assert_eq!((p.distance,p.height),(Some(0.5),Some(1.2)));
  let root=tree.append(None,Node::default())?;for (name,value) in [("MaxDist","1.5"),("MaxDeltaHeight","3.5")]{tree.append(Some(root),Node{name:Some(name.as_bytes().to_vec()),value:Some(value.as_bytes().to_vec()),..Node::default()})?;}
  assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(6),id,&mut tree,root)?);
  let object=binding.objects.get_mut(&id).unwrap();object.reset(binding.backend)?;let Owned::NodeReached(p)=object else{unreachable!()};assert_eq!((p.distance,p.height),(Some(1.5),Some(3.5)));
  let id3=definitions::Backend::construct(&mut binding,&mut state,Kind(6),0x119a8,b"3d")?;assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(6),id3,&mut tree)?);assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(6),id3)?,0x119a8);
  let Owned::NodeReached(p)=&binding.objects[&id3] else{unreachable!()};assert_eq!((p.distance,p.height),(Some(0.5),None));assert_eq!(binding.backend.seen,[(20,42),(20,42)]);Ok(())
 }

 #[test]fn edge_awareness_default_and_named_definitions_keep_constructor_defaults()->Result<()>{
  use super::super::{ai_edge_awareness::State as Edge,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;backend.scale=Some(2.);let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x6fb44,kind:Kind(12),name:Edge::CLASS.to_vec(),parent:None,vtable:None});metadata.defaults[12]=Some(0x6fb44);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(12),0x6fb44,b"default")?.unwrap();assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(12),id,&mut tree)?);assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(12),id)?,0x6fb44);
  let Owned::EdgeAwareness(p)=&binding.objects[&id] else{unreachable!()};assert_eq!((p.distance,p.duration),(Some(50.),Some(10.)));
  let root=tree.append(None,Node::default())?;for (name,value) in [("AwarenessDistance","3"),("MemoryDuration","4")]{tree.append(Some(root),Node{name:Some(name.as_bytes().to_vec()),value:Some(value.as_bytes().to_vec()),..Node::default()})?;}
  assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(12),id,&mut tree,root)?);binding.backend.scale=Some(7.);assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(12),id,&mut tree)?);
  let object=binding.objects.get_mut(&id).unwrap();object.reset(binding.backend)?;let Owned::EdgeAwareness(p)=object else{unreachable!()};assert_eq!((p.distance,p.duration),(Some(6.),Some(4.)));Ok(())
 }

 #[test]fn simple_biped_default_factory_configuration_and_reset_match_constructor()->Result<()>{
  use super::super::{ai_steering::State as Steering,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;backend.scale=Some(2.);let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x11ae8,kind:Kind(9),name:Steering::CLASS.to_vec(),parent:None,vtable:None});metadata.defaults[9]=Some(0x11ae8);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(9),0x11ae8,b"default")?.unwrap();assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(9),id,&mut tree)?);assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(9),id)?,0x11ae8);
  let Owned::Steering(p)=&binding.objects[&id] else{unreachable!()};assert_eq!(p.radius,Some(4.));assert_eq!(p.angle.unwrap().to_bits(),0x3f7f069e);
  let root=tree.append(None,Node::default())?;tree.append(Some(root),Node{name:Some(b"StrafeRadius".to_vec()),value:Some(b"3".to_vec()),..Node::default()})?;assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(9),id,&mut tree,root)?);
  let o=binding.objects.get_mut(&id).unwrap();let Owned::Steering(p)=o else{unreachable!()};p.strafe=7;o.reset(binding.backend)?;let Owned::Steering(p)=o else{unreachable!()};assert_eq!((p.radius,p.strafe),(Some(6.),0));Ok(())
 }

 #[test]fn queue_target_default_factory_definition_and_reset_preserve_native_fields()->Result<()>{
  use super::super::{ai_compute_target::State as Queue,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;backend.scale=Some(2.);let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x118b4,kind:Kind(7),name:Queue::CLASS.to_vec(),parent:None,vtable:None});metadata.defaults[7]=Some(0x118b4);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(7),0x118b4,b"default")?.unwrap();let Owned::ComputeTarget(p)=&binding.objects[&id] else{unreachable!()};assert!(p.maxdist.is_none());assert_eq!(p.axis,[f32::MAX;3]);assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(7),id,&mut tree)?);assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(7),id)?,0x118b4);
  let Owned::ComputeTarget(p)=&binding.objects[&id] else{unreachable!()};assert_eq!((p.maxdist,p.extra,p.entitydist,p.ratio,p.profiles),(Some(20.),Some(0.5),Some(5.),Some(1.),Some(Some(900))));
  let root=tree.append(None,Node::default())?;tree.append(Some(root),Node{name:Some(b"MaxDist".to_vec()),value:Some(b"0".to_vec()),..Node::default()})?;assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(7),id,&mut tree,root)?);
  let object=binding.objects.get_mut(&id).unwrap();let Owned::ComputeTarget(p)=object else{unreachable!()};p.set_predicate(Some(77));p.actor=Some(42);p.axis=[1.,2.,3.];object.reset(binding.backend)?;let Owned::ComputeTarget(p)=object else{unreachable!()};assert_eq!(p.maxdist,Some(0.));assert_eq!(p.predicate,Some(77));assert_eq!(p.actor,Some(42));assert_eq!(p.axis,[1.,2.,3.]);Ok(())
 }

 #[test]fn nearest_reachable_default_factory_definitions_and_reset_preserve_native_flags()->Result<()>{
  use super::super::{ai_find_nodes::State as FindNodes,ai_navigation_definitions::Node};
  let mut backend=Runtime::new()?;let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;let mut metadata=metadata();metadata.classes.push(Class{identity:0x119d0,kind:Kind(10),name:FindNodes::CLASS.to_vec(),parent:None,vtable:Some(0x4eba78)});metadata.defaults[10]=Some(0x119d0);
  let mut objects=BTreeMap::new();let mut memory=Memory::default();let mut tree=Tree::default();let mut binding=Binding{metadata:&metadata,objects:&mut objects,memory:&mut memory,spatial:spatial_context(),backend:&mut backend};
  let id=definitions::DefaultsBackend::construct_default(&mut binding,&mut state,Kind(10),0x119d0,b"default")?.unwrap();assert!(definitions::DefaultsBackend::initialize_default(&mut binding,&mut state,Kind(10),id,&mut tree)?);assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut state,Kind(10),id)?,0x119d0);
  let object=&binding.objects[&id];assert_eq!(object.kind(),Kind(10));assert_eq!(object.pathfinder(binding.memory)?,state.identity);assert_eq!(object.name(binding.memory)?.as_ref(),b"default");let Owned::FindNodes(p)=object else{unreachable!()};assert_eq!([p.start_flags,p.goal_flags],[0,0]);
  let root=tree.append(None,Node::default())?;for name in [b"Class".as_slice(),b"_metadata"]{tree.append(Some(root),Node{name:Some(name.to_vec()),..Node::default()})?;}assert!(definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(10),id,&mut tree,root)?);
  tree.append(Some(root),Node{name:Some(b"Unknown".to_vec()),..Node::default()})?;assert!(!definitions::Backend::initialize_modifier(&mut binding,&mut state,Kind(10),id,&mut tree,root)?);
  let object=binding.objects.get_mut(&id).unwrap();let Owned::FindNodes(p)=object else{unreachable!()};p.start_flags=0x1234;p.goal_flags=0xabcd;object.reset(binding.backend)?;let Owned::FindNodes(p)=object else{unreachable!()};assert_eq!([p.start_flags,p.goal_flags],[0x1234,0xabcd]);Ok(())
 }

}
