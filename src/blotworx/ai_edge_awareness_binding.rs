//! Canonical actor-service lookup and graph cursor services for4ab38c.
//! Clock and the concrete actor service's virtual64 require owning runtime data.
use anyhow::{Context,Result};use std::collections::BTreeMap;
use super::{ai_edge_awareness::{self,State},ai_astar_graph_services::GraphContext,ai_actor_queries::Runtime,ai_navigation_entity::Owned,ai_graph_queries::{Point,QueryStore}};
pub trait Backend{
 fn clock(&mut self,s:&mut State,graph:&mut super::ai_graph_handles::State)->Result<f32>;
 fn handle64(&mut self,s:&mut State,graph:&mut super::ai_graph_handles::State,service:usize)->Result<Option<usize>>;
}
pub struct Binding<'a,'b,B,T>{pub graph:GraphContext<'a,T>,pub actors:&'b Runtime,pub entities:&'b BTreeMap<usize,Owned>,pub backend:&'b mut B}
impl<B:Backend,T:QueryStore> ai_edge_awareness::Host for Binding<'_,'_,B,T>{type Point=Point;
 fn clock(&mut self,s:&mut State)->Result<f32>{self.backend.clock(s,self.graph.graph)}
 fn service(&mut self,_:&mut State,entity:usize)->Result<Option<usize>>{let owner=self.entities.get(&entity).context("Edge awareness entity is absent")?.entity.common.brain.context("Edge awareness entity has no context owner")?;self.actors.find(owner,0x6af6c)}
 fn handle64(&mut self,s:&mut State,service:usize)->Result<Option<usize>>{self.backend.handle64(s,self.graph.graph,service)}
 fn ready(&mut self,_:&mut State,h:usize)->Result<bool>{self.graph.graph.ready(Some(h))}
 fn point(&mut self,_:&mut State,h:usize)->Result<Option<Point>>{Ok(self.graph.graph.nodes.get(&h).context("Edge awareness graph handle is absent")?.point)}
 fn bind(&mut self,_:&mut State,h:usize,node:u32)->Result<u32>{Ok(u32::from(self.graph.tables.find_point(self.graph.graph,Some(h),node,self.graph.metadata)?))}
 fn position(&mut self,_:&mut State,p:Point)->Result<[f32;3]>{Ok(self.graph.tables.point_position(p)?.map(f32::from_bits))}
 fn restore(&mut self,_:&mut State,h:usize,point:Option<Point>)->Result<()>{let node=self.graph.graph.nodes.get_mut(&h).context("Edge awareness restore handle is absent")?;node.point=point;node.graph=None;Ok(())}
 fn entity_position(&mut self,_:&mut State,entity:usize)->Result<[f32;3]>{Ok(self.entities.get(&entity).context("Edge awareness entity position is absent")?.entity.values_34_58[..3].try_into()?)}
}
#[cfg(test)]mod tests{
 use super::*;use super::super::{ai_graph_handles as handles,ai_graph_queries as query,ai_graph_points as points,ai_navigation_entity as entity,ai_actor_queries as actor};
 struct Services;impl Backend for Services{fn clock(&mut self,_:&mut State,_:&mut handles::State)->Result<f32>{Ok(5.)}fn handle64(&mut self,_:&mut State,_:&mut handles::State,service:usize)->Result<Option<usize>>{assert_eq!(service,30);Ok(Some(100))}}
 impl ai_edge_awareness::DefinitionBackend for Services{fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{Ok(None)}fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{unreachable!()}}
 impl entity::Host for Services{fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(1.)}fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(2.)}fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(1.)}fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(None)}}
 #[test]fn edge_awareness_uses_real_actor_lookup_graph_readiness_and_point_binding()->Result<()>{
  let mut backend=Services;let registry=entity::Registry{count:3,diameter:entity::Class{index:0,constructor:Some(entity::Kind::Diameter)},height:entity::Class{index:1,constructor:Some(entity::Kind::Height)},maximum_speed:entity::Class{index:2,constructor:Some(entity::Kind::MaximumSpeed)}};
  let mut entity=Owned::new(10,None,[0;2],&registry,&mut backend)?;entity.entity.common.brain=Some(20);entity.entity.values_34_58[..3].copy_from_slice(&[0.,0.,0.]);let mut entities=BTreeMap::from([(10,entity)]);
  let actors=Runtime{pathfinder_class:0,sentinel:[0;3],constraints:Default::default(),owners:BTreeMap::from([(20,actor::Owner{word14:0,services:vec![30]})]),services:BTreeMap::from([(30,actor::Service{class:0x6af6c,query:None,context:20,filter:None})]),classes:BTreeMap::from([(0x6af6c,None)]),queries:Default::default(),pools:Default::default(),entries:Default::default()};
  let old=Point{resource:7,location:points::Location{table:points::Table::Internal,offset:64}};
  let node=handles::Node{connection:None,traversal:None,point:Some(old),graph:Some(99),free:0,word14:7,word18:0,manager_1c:None,word20:0,byte24:0,scalar28:0,scalar2c:0,byte30:0,word34:0,byte38:0,byte39:0,word3c:0,next:None,serial:0};
  let mut graph=handles::State{phase:0,serial:0,pool:vec![],nodes:BTreeMap::from([(100,node)]),resources:BTreeMap::from([(7,handles::Resource{size18:None,byte8:0,priority:0,state:5,word1c:0})]),current:None,tracked_resource:None,tracked158:0,tracked160:0,deferred_enabled:0};
  let mut bytes=vec![0;96];for (i,(id,x)) in [(7u32,-1f32),(8,1.),(9,0.)].into_iter().enumerate(){bytes[i*32..i*32+4].copy_from_slice(&id.to_be_bytes());bytes[i*32+4..i*32+8].copy_from_slice(&x.to_bits().to_be_bytes());}
  let mut tables=query::Tables{resources:BTreeMap::from([(7,points::Data{external_count:0,stride:32,internal:bytes,external:None})])};let mut metadata=query::Metadata{resources:BTreeMap::from([(7,query::ResourceMetadata{class:query::Class::Abstract,header:Some(90)})]),headers:BTreeMap::from([(90,3)])};
  let mut s=State::construct(1,2,b"edge",&mut backend)?;s.distance=Some(2.);
  for (position,start,ready,expected) in [([0.,0.,0.],7,5,1),([0.,0.,3.],7,5,0),([0.,0.,3.],99,5,1),([0.,0.,3.],7,4,1)]{
   entities.get_mut(&10).unwrap().entity.values_34_58[..3].copy_from_slice(&position);graph.resources.get_mut(&7).unwrap().state=ready;
   let mut binding=Binding{graph:GraphContext{graph:&mut graph,tables:&mut tables,metadata:&mut metadata},actors:&actors,entities:&entities,backend:&mut backend};assert_eq!(s.query(10,start,8,0.,1,&mut binding)?,expected);assert_eq!(graph.nodes[&100].point,Some(old));assert_eq!(graph.nodes[&100].graph,None);
  }Ok(())
 }
}
