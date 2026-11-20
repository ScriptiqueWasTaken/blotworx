//! Actual ordered definition storage for Flee1bef20. Raw value handles retain
//! buffer identity across unit-scale/task-manager callbacks; no cached strings.
use anyhow::Result;
use super::{ai_flee_agent::State,ai_navigation_definitions::Tree,
    ai_definition_strings::{Strings,Field},ai_flee_attributes as attributes};
pub trait Backend {
    fn integer(&mut self,s:&mut State,tree:&mut Tree,text:&[u8])->Result<i32>;
    fn number(&mut self,s:&mut State,tree:&mut Tree,text:&[u8])->Result<f64>;
    fn unit_scale(&mut self,s:&mut State,tree:&mut Tree)->Result<Option<f32>>;
    fn traversal(&mut self,s:&mut State,tree:&mut Tree,text:Option<&[u8]>)->Result<i32>;
    fn task_manager(&mut self,s:&mut State,tree:&mut Tree)->Result<usize>;
    fn safety_task(&mut self,s:&mut State,tree:&mut Tree,manager:usize,text:Option<&[u8]>)->Result<i32>;
    fn maximum_speed(&mut self,s:&mut State,tree:&mut Tree)->Result<Option<f32>>;
}
pub struct Binding<'a,H>{pub tree:&'a mut Tree,pub backend:&'a mut H,strings:Strings}
impl<'a,H:Backend> Binding<'a,H>{
    pub fn new(tree:&'a mut Tree,backend:&'a mut H)->Self{Self{tree,backend,strings:Strings::default()}}
    pub fn attribute(&mut self,s:&mut State,node:Option<usize>)->Result<bool>{attributes::initialize(s,node,self)}
    fn string(&self,v:usize)->Result<Vec<u8>>{self.strings.text(self.tree,v)}
}
impl<H:Backend> attributes::Host for Binding<'_,H>{
    fn name(&mut self,_:&mut State,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
    fn value(&mut self,_:&mut State,n:usize)->Result<Option<usize>>{self.strings.capture(self.tree,n,Field::Value)}
    fn text(&mut self,_:&mut State,v:usize)->Result<Vec<u8>>{self.string(v)}
    fn integer(&mut self,s:&mut State,v:usize)->Result<i32>{let text=self.string(v)?;self.backend.integer(s,self.tree,&text)}
    fn number(&mut self,s:&mut State,v:usize)->Result<f64>{let text=self.string(v)?;self.backend.number(s,self.tree,&text)}
    fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>{self.backend.unit_scale(s,self.tree)}
    fn traversal(&mut self,s:&mut State,v:Option<usize>)->Result<i32>{let text=v.map(|v|self.string(v)).transpose()?;self.backend.traversal(s,self.tree,text.as_deref())}
    fn task_manager(&mut self,s:&mut State)->Result<usize>{self.backend.task_manager(s,self.tree)}
    fn safety_task(&mut self,s:&mut State,m:usize,v:Option<usize>)->Result<i32>{let text=v.map(|v|self.string(v)).transpose()?;self.backend.safety_task(s,self.tree,m,text.as_deref())}
    fn maximum_speed(&mut self,s:&mut State)->Result<Option<f32>>{self.backend.maximum_speed(s,self.tree)}
}
#[cfg(test)]
mod tests{
    use super::*;
    use super::super::{ai_flee_agent,ai_navigation_definitions::Node};
    struct Services{node:usize,replace:bool}
    impl ai_flee_agent::Host for Services{
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut State)->Result<usize>{anyhow::bail!("Unexpected entity query")}
        fn release_reservation(&mut self,_:&mut State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected reservation")}
        fn detach_service(&mut self,_:&mut State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected detach")}
    }
    impl Backend for Services{
        fn integer(&mut self,_:&mut State,_:&mut Tree,_:&[u8])->Result<i32>{anyhow::bail!("Unexpected integer")}
        fn number(&mut self,_:&mut State,_:&mut Tree,text:&[u8])->Result<f64>{assert_eq!(text,b"2");Ok(2.)}
        fn unit_scale(&mut self,_:&mut State,tree:&mut Tree)->Result<Option<f32>>{
            let value=&mut tree.nodes[self.node].value;
            if self.replace{*value=Some(b"2-reallocated-storage".to_vec());}else{value.as_mut().unwrap()[0]=b'2';}Ok(Some(3.))
        }
        fn traversal(&mut self,_:&mut State,_:&mut Tree,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("Unexpected traversal")}
        fn task_manager(&mut self,_:&mut State,_:&mut Tree)->Result<usize>{anyhow::bail!("Unexpected task manager")}
        fn safety_task(&mut self,_:&mut State,_:&mut Tree,_:usize,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("Unexpected task")}
        fn maximum_speed(&mut self,_:&mut State,_:&mut Tree)->Result<Option<f32>>{Ok(None)}
    }
    #[test]
    fn definition_binding_reads_live_bytes_and_rejects_replaced_storage()->Result<()>{
        for replace in [false,true]{
            let mut tree=Tree::default();let node=tree.append(None,Node{name:Some(b"Speed".to_vec()),value:Some(b"1".to_vec()),..Default::default()})?;
            let mut services=Services{node,replace};let mut state=State::new(None,&mut services)?;let previous=state.speed_8c;
            let result=Binding::new(&mut tree,&mut services).attribute(&mut state,Some(node));
            if replace{assert!(result.unwrap_err().to_string().contains("replaced"));assert_eq!(state.speed_8c,previous);}
            else{assert!(result?);assert_eq!(state.speed_8c,6.);}
        }Ok(())
    }
}
