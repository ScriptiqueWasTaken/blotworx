//! Concrete graph services for goto reset. Actor/segment services remain required.
use anyhow::{Context as _,Result};
use super::{ai_astar_memory::Node,ai_astar_reset::Services,ai_graph_handles::State,ai_graph_queries::{Point,Tables,Metadata,QueryStore},ai_traversal_instances::Instance};
pub struct GraphContext<'a,T> {pub graph:&'a mut State,pub tables:&'a mut T,pub metadata:&'a mut Metadata}
pub trait Actors<T=Tables> {
    fn constraint_owner(&self,constraint:usize)->Result<Option<usize>>;
    fn sentinel_position(&self)->Result<[u32;3]>;
    fn actor_for_constraint(&mut self,instance:&mut Instance,owner:usize)->Result<Option<usize>>;
    fn actor_query(&self,actor:usize)->Result<Option<usize>>;
    fn prepare_query(&mut self,instance:&mut Instance,context:&mut GraphContext<'_,T>,query:usize)->Result<()>;
    fn segment_query(&mut self,instance:&mut Instance,context:&mut GraphContext<'_,T>,query:usize,start:[u32;3],end:[u32;3],seed:&mut Node)->Result<u32>;
}
pub struct GraphServices<'a,A,T=Tables> {
    pub graph:&'a mut State,pub tables:&'a mut T,pub metadata:&'a mut Metadata,pub actors:A,
}
impl<A:Actors<T>,T:QueryStore> Services for GraphServices<'_,A,T> {
    type Point=Point;
    fn handle_ready(&mut self,_:&mut Instance,handle:usize)->Result<bool>{self.graph.ready(Some(handle))}
    fn handle_point(&self,handle:usize)->Result<Option<Point>>{Ok(self.graph.nodes.get(&handle).context("Astar graph handle is absent")?.point)}
    fn find_point(&mut self,_:&mut Instance,handle:usize,node:u32)->Result<u32>{Ok(u32::from(self.tables.find_point(self.graph,Some(handle),node,self.metadata)?))}
    fn point_position(&self,point:Point)->Result<[u32;3]>{self.tables.point_position(point)}
    fn restore_point_and_clear_graph(&mut self,handle:usize,point:Option<Point>)->Result<()> {
        let handle=self.graph.nodes.get_mut(&handle).context("Astar restore handle is absent")?;handle.point=point;handle.graph=None;Ok(())
    }
    fn constraint_owner(&self,constraint:usize)->Result<Option<usize>>{self.actors.constraint_owner(constraint)}
    fn sentinel_position(&self)->Result<[u32;3]>{self.actors.sentinel_position()}
    fn actor_for_constraint(&mut self,i:&mut Instance,owner:usize)->Result<Option<usize>>{self.actors.actor_for_constraint(i,owner)}
    fn actor_query(&self,actor:usize)->Result<Option<usize>>{self.actors.actor_query(actor)}
    fn prepare_query(&mut self,i:&mut Instance,query:usize)->Result<()>{self.actors.prepare_query(i,&mut GraphContext{graph:self.graph,tables:self.tables,metadata:self.metadata},query)}
    fn segment_query(&mut self,i:&mut Instance,query:usize,start:[u32;3],end:[u32;3],seed:&mut Node)->Result<u32>{self.actors.segment_query(i,&mut GraphContext{graph:self.graph,tables:self.tables,metadata:self.metadata},query,start,end,seed)}
}
