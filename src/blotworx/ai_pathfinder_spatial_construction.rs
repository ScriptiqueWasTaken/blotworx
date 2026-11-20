//! Base spatial construction with access to the canonical pathfinder at every
//! allocator/configuration callback. Raw service/pool data stays in its arena.
use anyhow::Result;
use super::{ai_pathfinder_constructor::State,ai_graph_link_passes::Memory,ai_spatial_construction::{self,Allocation}};
pub trait Allocator {
 fn class20(&mut self,state:&mut State,m:&mut Memory,service:u32)->Result<u32>;
 fn allocate(&mut self,state:&mut State,m:&mut Memory,bytes:u32)->Result<Allocation>;
 fn observe(&mut self,state:&mut State,m:&mut Memory,observer:u32,address:u32,bytes:u32,result:u32)->Result<()>;
}
pub trait Backend:Allocator {
 fn configure(&mut self,state:&mut State,m:&mut Memory,service:u32,root:u32)->Result<u32>;
}
pub struct Binding<'a,B>{pub state:&'a mut State,pub backend:&'a mut B}
impl<B:Backend> super::ai_traversal_exclusion::Services for Binding<'_,B>{
 fn class20(&mut self,m:&mut Memory,service:u32)->Result<u32>{self.backend.class20(self.state,m,service)}
}
impl<B:Backend> ai_spatial_construction::Host for Binding<'_,B>{
 fn owner_filters(&mut self,m:&mut Memory,owner:u32)->Result<u32>{super::ai_pathfinder_spatial_binding::owner_filters(self.state,m,owner)}
 fn allocate(&mut self,m:&mut Memory,bytes:u32)->Result<Allocation>{self.backend.allocate(self.state,m,bytes)}
 fn observe(&mut self,m:&mut Memory,o:u32,a:u32,b:u32,r:u32)->Result<()>{self.backend.observe(self.state,m,o,a,b,r)}
 fn configure(&mut self,m:&mut Memory,service:u32,root:u32)->Result<u32>{self.backend.configure(self.state,m,service,root)}
}

/// Real definition configuration for both named and NULL/default roots. The
/// typed root is retained here; native initialize's opaque root argument is not
/// converted into an invented arena copy of the definition tree.
pub struct Definitions<'a,B>{pub tree:&'a mut super::ai_navigation_definitions::Tree,pub root:Option<usize>,pub context:super::ai_spatial_modifier::Context,pub backend:&'a mut B}
impl<B:Allocator> Allocator for Definitions<'_,B>{
 fn class20(&mut self,s:&mut State,m:&mut Memory,p:u32)->Result<u32>{self.backend.class20(s,m,p)}
 fn allocate(&mut self,s:&mut State,m:&mut Memory,n:u32)->Result<Allocation>{self.backend.allocate(s,m,n)}
 fn observe(&mut self,s:&mut State,m:&mut Memory,o:u32,p:u32,n:u32,r:u32)->Result<()>{self.backend.observe(s,m,o,p,n,r)}
}
struct Services<'a,B>{state:&'a mut State,backend:&'a mut B}
impl<B:Allocator> super::ai_traversal_exclusion::Services for Services<'_,B>{
 fn class20(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.backend.class20(self.state,m,p)}
}
impl<B:Allocator> Backend for Definitions<'_,B>{
 fn configure(&mut self,state:&mut State,m:&mut Memory,service:u32,_root:u32)->Result<u32>{
  super::ai_spatial_definition_binding::configure(m,self.tree,self.root,service,self.context,&mut Services{state,backend:self.backend})
 }
}
