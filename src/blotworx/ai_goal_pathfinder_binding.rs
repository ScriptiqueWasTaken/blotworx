//! Goal virtual services over canonical CPathFinder objects. Simple native
//! getters and499abc execute here; missing traversal/clock state is diagnosed.
use std::collections::BTreeMap;
use anyhow::{Context,Result};
use super::{ai_goal_detection::{self,State},ai_graph_link_passes::Memory,ai_pathfinder_constructor};
pub trait Backend:super::ai_pathfinder_can_go::Host {
    fn manager_time(&mut self,goal:&mut State,memory:&mut Memory)->Result<f32>;
    ///Actual3d8 state is not yet modeled by the enclosing search loop.
    fn byte_3d8(&mut self,parent:&mut ai_pathfinder_constructor::State,memory:&mut Memory)->Result<u8>;
}
pub struct Binding<'a,B> {pub pathfinders:&'a mut BTreeMap<usize,ai_pathfinder_constructor::State>,pub backend:&'a mut B}
impl<B> Binding<'_,B> {
    fn parent(&mut self,identity:usize)->Result<&mut ai_pathfinder_constructor::State> {
        let parent=self.pathfinders.get_mut(&identity).context("Goal detector has no canonical pathfinder")?;
        anyhow::ensure!(parent.identity==identity,"Goal pathfinder registry identity differs");Ok(parent)
    }
}
impl<B:Backend> ai_goal_detection::RuntimeHost for Binding<'_,B> {
    fn radius(&mut self,_:&mut State,_:&mut Memory,parent:usize,retained:bool)->Result<f32> {
        self.parent(parent)?.base.scales_14_18_1c[if retained {2}else{1}].context("Goal radius is uninitialized")
    }
    fn query_54(&mut self,_:&mut State,_:&mut Memory,parent:usize)->Result<u32> {self.parent(parent)?.reset_fields.word_36c.context("Pathfinder195170 status is unwritten")}
    fn word_f0(&mut self,_:&mut State,_:&mut Memory,parent:usize)->Result<u32> {Ok(u32::try_from(self.parent(parent)?.references_b0_b4_c8_d0_d4_f0[5].unwrap_or(0))?)}
    fn byte_3d8(&mut self,_:&mut State,memory:&mut Memory,parent:usize)->Result<u8> {
        let state=self.pathfinders.get_mut(&parent).context("Goal clock callback removed its pathfinder")?;
        anyhow::ensure!(state.identity==parent,"Goal pathfinder registry identity differs");self.backend.byte_3d8(state,memory)
    }
    fn manager_time(&mut self,goal:&mut State,memory:&mut Memory)->Result<f32> {self.backend.manager_time(goal,memory)}
    fn query_58(&mut self,_:&mut State,memory:&mut Memory,parent:usize,start:u32,end:u32)->Result<u32> {
        let state=self.pathfinders.get_mut(&parent).context("Goal CanGo has no canonical pathfinder")?;
        anyhow::ensure!(state.identity==parent,"Goal pathfinder registry identity differs");super::ai_pathfinder_can_go::query(state,memory,start,end,self.backend)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_goal_detection::{Class,Fields},ai_pathfinder_modifiers::Kind};
    struct Services{calls:Vec<(usize,u32,u32,u32)>,diameters:usize,units:usize,now:f32,redirect:Option<usize>}
    impl super::super::ai_pathfinder_can_go::Host for Services {
        fn entity_diameter(&mut self,state:&mut ai_pathfinder_constructor::State,_:&mut Memory)->Result<Option<f32>> {
            self.diameters+=1;state.tables_fc_1fc[Kind(3).table()].selected=Some(Some(200));Ok(Some(1.6))
        }
        fn unit_scale(&mut self,_:&mut ai_pathfinder_constructor::State,_:&mut Memory)->Result<Option<f32>> {self.units+=1;Ok(None)}
        fn query_1c(&mut self,_:&mut ai_pathfinder_constructor::State,memory:&mut Memory,selected:usize,start:u32,end:u32,radius:f32)->Result<u32> {
            self.calls.push((selected,start,end,radius.to_bits()));memory.write(start+0x10,123)?;Ok(257)
        }
    }
    impl Backend for Services {
        fn manager_time(&mut self,goal:&mut State,_:&mut Memory)->Result<f32> {if let Some(parent)=self.redirect{goal.pathfinder=parent;}Ok(self.now)}
        fn byte_3d8(&mut self,parent:&mut ai_pathfinder_constructor::State,_:&mut Memory)->Result<u8> {Ok(u8::from(parent.identity==51))}
    }
    fn setup()->Result<(BTreeMap<usize,ai_pathfinder_constructor::State>,Memory)> {
        let mut parent=super::super::ai_pathfinder_modifiers::tests::state()?;
        parent.base.scales_14_18_1c[1]=Some(0.5);parent.base.scales_14_18_1c[2]=Some(1.);
        parent.reset_fields.word_36c=Some(2);parent.references_b0_b4_c8_d0_d4_f0[5]=Some(2);
        parent.tables_fc_1fc[Kind(3).table()].selected=Some(Some(100));
        let mut memory=Memory{regions:BTreeMap::from([(1000,vec![0;0x200])]),resources:Default::default(),stride:0,journal:Default::default()};
        super::super::ai_refine_goal::set_position(&mut memory,1100,[0.25,2.,0.])?;
        Ok((BTreeMap::from([(parent.identity,parent)]),memory))
    }
    #[test]
    fn arrival_uses_canonical_radii_real_can_go_wrapper_and_retained_modifier()->Result<()> {
        let (mut parents,mut memory)=setup()?;
        let mut goal=State::construct(Class::Reached2D5,1,50,b"goal");goal.fields=Fields::Reached2D5{flag:Some(0),height:Some(0.5)};
        let mut backend=Services{calls:vec![],diameters:0,units:0,now:0.,redirect:None};
        assert_eq!(goal.query(&mut memory,1000,1100,&mut Binding{pathfinders:&mut parents,backend:&mut backend})?,1);
        assert_eq!(backend.calls,vec![(100,1000,1100,0.8f32.to_bits())]);
        assert_eq!(memory.word(1016)?,123,"CanGo point-cache writes did not reach canonical point storage");
        assert_eq!(parents[&50].tables_fc_1fc[Kind(3).table()].selected,Some(Some(200)));
        assert_eq!((backend.diameters,backend.units),(1,0));
        //Retained arrival uses the second native radius and no second CanGo.
        super::super::ai_refine_goal::set_position(&mut memory,1100,[0.75,2.,0.])?;
        assert_eq!(goal.query(&mut memory,1000,1100,&mut Binding{pathfinders:&mut parents,backend:&mut backend})?,1);
        assert_eq!(backend.calls.len(),1);Ok(())
    }
    #[test]
    fn goal_change_getters_read_live_parent_and_do_not_invent_ready_status()->Result<()> {
        let (mut parents,mut memory)=setup()?;
        super::super::ai_refine_goal::set_position(&mut memory,1100,[0.25,0.,0.])?;
        let mut second=super::super::ai_pathfinder_modifiers::tests::state()?;second.identity=51;parents.insert(51,second);
        let mut goal=State::construct(Class::Changed3D,1,50,b"goal");goal.fields=Fields::Changed3D{distance:Some(1.),last_time:Some(0.)};
        let mut backend=Services{calls:vec![],diameters:0,units:0,now:0.9,redirect:Some(51)};
        assert_eq!(goal.query(&mut memory,1000,1100,&mut Binding{pathfinders:&mut parents,backend:&mut backend})?,1);
        let Fields::Changed3D{last_time,..}=goal.fields else {unreachable!()};assert_eq!(last_time,Some(0.9));assert_eq!(goal.pathfinder,51);
        goal.pathfinder=50;parents.get_mut(&50).unwrap().reset_fields.word_36c=None;
        assert!(goal.query(&mut memory,1000,1100,&mut Binding{pathfinders:&mut parents,backend:&mut backend}).is_err());
        assert!(backend.calls.is_empty());Ok(())
    }
}
