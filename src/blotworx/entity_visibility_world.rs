//! Ordered entity visibility using live transform ancestry (REL299778).
use anyhow::{Result,ensure};
use super::{assets::World,entity_visibility};
struct Binding<'a> { world:&'a World, hits:&'a [(usize,u32)] }
impl entity_visibility::Host for Binding<'_> {
    fn entity(&mut self,hit:usize)->Result<Option<usize>> {
        Ok(self.world.collision_entity_index(self.hits[hit].0))
    }
    fn root(&mut self,entity:Option<usize>)->Result<Option<usize>> {
        let Some(mut current)=entity else {return Ok(None)};
        let mut visited=std::collections::HashSet::new();
        loop {
            ensure!(visited.insert(current),"Cycle in visibility transform ancestry");
            match self.world.current_transform_parent(current)? {
                Some(parent)=>current=parent,
                None=>return Ok(Some(current)),
            }
        }
    }
    fn filter(&mut self,hit:usize)->Result<u32> {Ok(self.hits[hit].1)}
}
impl World {
    pub(super) fn ordered_entity_visibility(&self,source:usize,target:usize,hits:&[(usize,u32)])->Result<bool> {
        entity_visibility::visible(source,target,hits.len(),1,0,&mut Binding{world:self,hits})
    }
}
