//! Definition-tree binding for the concrete gap-avoidance modifier. Global
//! service and entity-attribute lookup remain mandatory actual runtime services.
use anyhow::Result;
use super::{ai_navigation_definitions::Tree,ai_definition_strings::{Strings,Field},
    ai_goto_gap::{self,State,Pool,Cache34,Cache24},ai_goto_gap_attributes as attributes,
    ai_goto_gap_setup as setup,ai_navigation_modifier_init as definition};
pub trait Backend:ai_goto_gap::Host+setup::Host {
    fn number(&mut self,state:&mut State,text:Option<&[u8]>)->Result<f64>;
    fn integer(&mut self,state:&mut State,text:Option<&[u8]>)->Result<i32>;
    fn diameter_attribute(&mut self,state:&mut State,entity:usize)->Result<Option<usize>>;
    fn diameter_value(&mut self,state:&mut State,attribute:usize)->Result<f32>;
}
pub struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B,strings:Strings}
impl<'a,B:Backend> Binding<'a,B> {
    pub fn new(tree:&'a mut Tree,backend:&'a mut B)->Self {Self{tree,backend,strings:Strings::default()}}
    pub fn initialize(&mut self,state:&mut State,root:Option<usize>)->Result<bool> {state.initialize_definition(root,self)}
}
impl<B:Backend> ai_goto_gap::Host for Binding<'_,B> {
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>> {self.backend.unit_scale(state)}
    fn owner_entity(&mut self,state:&mut State)->Result<usize> {self.backend.owner_entity(state)}
    fn profiling_enabled(&mut self,state:&mut State)->Result<bool> {self.backend.profiling_enabled(state)}
    fn register_budget(&mut self,state:&mut State,name:&str)->Result<i32> {self.backend.register_budget(state,name)}
}
impl<B:Backend> setup::Host for Binding<'_,B> {
    fn world_service(&mut self,state:&mut State,kind:setup::Service)->Result<Option<usize>> {self.backend.world_service(state,kind)}
    fn entity_attribute(&mut self,state:&mut State,entity:usize,kind:setup::Attribute)->Result<Option<usize>> {self.backend.entity_attribute(state,entity,kind)}
    fn cache34(&mut self,state:&mut State,capacity:u32)->Result<Option<Pool<Cache34>>> {self.backend.cache34(state,capacity)}
    fn cache24(&mut self,state:&mut State,capacity:u32)->Result<Option<Pool<Cache24>>> {self.backend.cache24(state,capacity)}
}
impl<B:Backend> attributes::Host for Binding<'_,B> {
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.strings.capture(self.tree,node,Field::Name)}
    fn value(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.strings.capture(self.tree,node,Field::Value)}
    fn text(&mut self,_:&mut State,value:usize)->Result<Vec<u8>> {self.strings.text(self.tree,value)}
    fn number(&mut self,state:&mut State,value:Option<usize>)->Result<f64> {
        let text=value.map(|id|self.strings.text(self.tree,id)).transpose()?;self.backend.number(state,text.as_deref())
    }
    fn integer(&mut self,state:&mut State,value:Option<usize>)->Result<i32> {
        let text=value.map(|id|self.strings.text(self.tree,id)).transpose()?;self.backend.integer(state,text.as_deref())
    }
    fn diameter_attribute(&mut self,state:&mut State,entity:usize)->Result<Option<usize>> {self.backend.diameter_attribute(state,entity)}
    fn diameter_value(&mut self,state:&mut State,attribute:usize)->Result<f32> {self.backend.diameter_value(state,attribute)}
}
impl<B:Backend> definition::Host<State> for Binding<'_,B> {
    fn child_count(&mut self,_:&mut State,root:usize)->Result<u32> {Ok(u32::try_from(self.tree.node(root)?.children.len())?)}
    fn child(&mut self,_:&mut State,root:usize,index:u32)->Result<Option<usize>> {Ok(self.tree.node(root)?.children.get(index as usize).copied())}
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn initialize_attribute(&mut self,state:&mut State,node:usize)->Result<bool> {attributes::initialize(state,node,self)}
}
impl<B:Backend> definition::GapHost for Binding<'_,B> {
    fn setup(&mut self,state:&mut State)->Result<bool> {setup::initialize(state,self)}
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    use serde_json::{Value,json};
    use super::super::ai_navigation_definitions::Node;
    struct Services {available:bool,setups:u32,owner:usize}
    impl ai_goto_gap::Host for Services {
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {Ok(None)}
        fn owner_entity(&mut self,_:&mut State)->Result<usize> {Ok(self.owner)}
        fn profiling_enabled(&mut self,_:&mut State)->Result<bool> {Ok(false)}
        fn register_budget(&mut self,_:&mut State,_:&str)->Result<i32> {anyhow::bail!("Unexpected profiler")}
    }
    impl setup::Host for Services {
        fn world_service(&mut self,_:&mut State,kind:setup::Service)->Result<Option<usize>> {
            match kind {setup::Service::Profiles=>{self.setups+=1;Ok(Some(80))},setup::Service::Gap=>Ok(self.available.then_some(90))}
        }
        fn entity_attribute(&mut self,_:&mut State,_:usize,_:setup::Attribute)->Result<Option<usize>> {Ok(None)}
    }
    impl Backend for Services {
        fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64> {anyhow::bail!("No numeric parser supplied for this boolean fixture")}
        fn integer(&mut self,_:&mut State,_:Option<&[u8]>)->Result<i32> {anyhow::bail!("No integer parser supplied for this boolean fixture")}
        fn diameter_attribute(&mut self,_:&mut State,_:usize)->Result<Option<usize>> {anyhow::bail!("No diameter lookup expected")}
        fn diameter_value(&mut self,_:&mut State,_:usize)->Result<f32> {anyhow::bail!("No diameter lookup expected")}
    }
    #[test]
    fn full_definition_entry_matches_original_parse_then_service_order()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goto_gap_definition.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];let mut services=Services{available:input["setup"].as_bool().unwrap(),setups:0,owner:20};
            let mut state=State::construct(1,50,b"IGoto",&mut services)?;
            let mut tree=Tree::default();let root=if let Some(rows)=input["nodes"].as_array() {
                let root=tree.append(None,Node::default())?;
                for row in rows {tree.append(Some(root),Node{name:Some(row["name"].as_str().context("Missing tag")?.as_bytes().to_vec()),value:row["value"].as_str().map(|s|s.as_bytes().to_vec()),..Node::default()})?;}
                Some(root)
            } else {None};
            let result=Binding::new(&mut tree,&mut services).initialize(&mut state,root)?;
            assert_eq!(json!({"result":result,"flag":state.flag_24.unwrap(),"setups":services.setups}),case["output"],"{input}");
            if result {
                assert_eq!(state.pool_d4.as_ref().context("Steering cache absent")?.nodes.len(),61);
                assert_eq!(state.pool_118.as_ref().context("Steering cache absent")?.nodes.len(),36);
            }
        }
        Ok(())
    }
    impl super::super::ai_goal_detection::DefinitionBackend for Services {
        fn unit_scale(&mut self,_:&mut super::super::ai_goal_detection::State)->Result<Option<f32>> {Ok(None)}
        fn number(&mut self,_:&mut super::super::ai_goal_detection::State,_:Option<&[u8]>)->Result<f64> {anyhow::bail!("No goal numeric provider in this fixture")}
        fn entity_height(&mut self,_:&mut super::super::ai_goal_detection::State)->Result<Option<f32>> {Ok(self.available.then_some(2.4))}
    }
    impl super::super::ai_pathfinder_constructor::BaseHost for Services {
        fn unit_scale(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base)->Result<Option<f32>> {Ok(None)}
        fn profiling_enabled(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base)->Result<bool> {Ok(false)}
        fn register_budget(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base,_:&str)->Result<i32> {anyhow::bail!("Unexpected profiler")}
    }
    impl super::super::ai_pathfinder_constructor::Host for Services {
        fn world_service(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,kind:super::super::ai_pathfinder_constructor::Service)->Result<Option<usize>> {
            Ok(Some(match kind{super::super::ai_pathfinder_constructor::Service::Graph=>5,super::super::ai_pathfinder_constructor::Service::PathObjects=>6}))
        }
        fn point_attribute_index(&mut self,_:&mut super::super::ai_pathfinder_constructor::State)->Result<i32> {Ok(0)}
    }
    #[test]
    fn source_biped_gap_class_constructs_configures_and_requires_real_services()->Result<()> {
        use super::super::{ai_biped_catalog::{Catalog,Database},ai_navigation_resolver as resolver,
            ai_navigation_action::{Class,Registry},ai_navigation_construction::{Constructed,Layout,ActionClass,ActionFactory},
            ai_pathfinder_constructor::{State as Pathfinder,Counter},ai_pathfinder_modifier_factory::Owned,ai_pathfinder_modifiers::Kind};
        let registry=||Registry{count:2,heading:Class{index:0,constructor:true},speed:Class{index:1,constructor:true}};
        let brain=Constructed::new(20,Some(10),Some(30),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},&Layout{agent_classes:3,service_classes:1,attributes:registry()})?;
        for available in [false,true] {
            let mut backend=Services{available,setups:0,owner:10};
            let pathfinder=Pathfinder::construct(50,&brain,40,&registry(),&mut Counter::default(),&mut backend)?;
            let catalog=Catalog::new(&[Database{radius:0,height:0,path:Some(b"AI/ShortThinBiped/example.hpd".to_vec())}])?;
            let mut resolution=resolver::Runtime{world:None,allocator:1,fallback_allocator:2};
            let manager=resolver::Manager{identity:7,allocator:None,definition_root:Some(catalog.root)};
            let mut resolved=resolution.resolve(Some(&manager),&catalog.tree,Some(b"ShortThinBiped"))?.context("Biped not resolved")?;
            let brain_node=resolved.tree.find_tag(resolved.root,b"Brain")?.context("Brain missing")?;
            let service=resolved.tree.find_tag(brain_node,b"Service")?.context("Service missing")?;
            let group=resolved.tree.find_tag(service,b"Modifiers_IGoto")?.context("Goto group missing")?;
            let node=resolved.tree.find_tag(group,b"IGoto")?.context("Goto modifier missing")?;
            let class=resolved.tree.first_value(node,b"Class")?.context("Goto Class missing")?.to_vec();
            let mut modifier=Owned::construct(Kind(8),&class,100,&pathfinder,b"IGoto",&mut backend)?;
            assert_eq!(modifier.name(&super::super::ai_graph_link_passes::Memory::default())?.as_ref(),b"IGoto");
            assert_eq!(modifier.initialize_definition(&mut resolved.tree,Some(node),&mut backend)?,available);
            let Owned::GapAvoidance(state)=modifier else {anyhow::bail!("Goto factory constructed the wrong concrete class")};
            assert_eq!(state.pathfinder,50);assert_eq!(state.entity_58,Some(10));assert_eq!(state.flag_24,Some(1));
            assert_eq!(state.pool_d4.is_some(),available);assert_eq!(state.pool_118.is_some(),available);
            let group=resolved.tree.find_tag(service,b"Modifiers_IRefineGoal")?.context("Refine group missing")?;
            let node=resolved.tree.find_tag(group,b"IRefineGoal")?.context("Refine modifier missing")?;
            let class=resolved.tree.first_value(node,b"Class")?.context("Refine Class missing")?.to_vec();
            let mut modifier=Owned::construct(Kind(0),&class,101,&pathfinder,b"IRefineGoal",&mut backend)?;
            assert_eq!(modifier.name(&super::super::ai_graph_link_passes::Memory::default())?.as_ref(),b"IRefineGoal");
            let setups=backend.setups;
            assert!(modifier.initialize_definition(&mut resolved.tree,Some(node),&mut backend)?);
            assert_eq!(backend.setups,setups,"Refine initialization invoked unrelated gap setup");
            let Owned::RefineGoal(state)=modifier else {anyhow::bail!("Refine factory constructed the wrong concrete class")};
            assert_eq!(state.pathfinder,pathfinder.identity);
            assert_eq!(state.base_vectors,[[f32::MAX;3];2]);
            assert_eq!(state.word_40,0);
        }
        Ok(())
    }
    #[test]
    fn owned_distance_detectors_initialize_and_reset_through_source_class_factory()->Result<()> {
        use super::super::{ai_goal_detection::{Class,Fields},ai_pathfinder_modifier_factory::Owned};
        let pathfinder=super::super::ai_pathfinder_modifiers::tests::state()?;
        for class in [Class::Reached2D5,Class::Reached3D,Class::Changed3D] {
            let mut backend=Services{available:true,setups:0,owner:10};
            let mut owned=Owned::construct(class.kind(),class.name(),100,&pathfinder,b"default",&mut backend)?;
            assert_eq!(owned.identity(),100);assert_eq!(owned.pathfinder(&super::super::ai_graph_link_passes::Memory::default())?,pathfinder.identity);assert_eq!(owned.kind(),class.kind());
            let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
            tree.append(Some(root),Node{name:Some(b"Class".to_vec()),value:Some(class.name().to_vec()),..Node::default()})?;
            assert!(owned.initialize_definition(&mut tree,Some(root),&mut backend)?);
            let Owned::GoalDetection(goal)=&mut owned else {anyhow::bail!("Detector class factory returned the wrong concrete object")};
            match &mut goal.fields {
                Fields::Reached2D5{flag,height}=>{assert_eq!(*height,Some(1.2));assert_eq!(*flag,Some(0));*flag=Some(255);},
                Fields::Reached3D{flag}=>{assert_eq!(*flag,Some(0));*flag=Some(255);},
                Fields::Changed3D{distance,last_time}=>{assert_eq!(*distance,Some(1.));assert_eq!(*last_time,Some(0.));*last_time=Some(12.);},
            }
            owned.reset(&mut backend)?;
            let Owned::GoalDetection(goal)=&owned else {unreachable!()};
            match goal.fields {
                Fields::Reached2D5{flag,height}=>{assert_eq!(flag,Some(0));assert_eq!(height,Some(1.2));},
                Fields::Reached3D{flag}=>assert_eq!(flag,Some(0)),
                Fields::Changed3D{distance,last_time}=>{assert_eq!(distance,Some(1.));assert_eq!(last_time,Some(0.));},
            }
            assert_eq!(backend.setups,0,"Goal detection invoked unrelated steering setup");
        }Ok(())
    }

 impl super::super::ai_predict_move::DefinitionBackend for Services{
  fn number(&mut self,_:&mut super::super::ai_predict_move::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("No accident numeric parser expected in this fixture")}
 }

 impl super::super::ai_node_candidate::DefinitionBackend for Services{
  fn unit_scale(&mut self,_:&mut super::super::ai_node_candidate::State)->Result<Option<f32>>{anyhow::bail!("Candidate scale not expected in this fixture")}
  fn integer(&mut self,_:&mut super::super::ai_node_candidate::State,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("Candidate integer not expected in this fixture")}
  fn number(&mut self,_:&mut super::super::ai_node_candidate::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Candidate number not expected in this fixture")}
 }

 impl super::super::ai_node_reached::DefinitionBackend for Services{
 fn unit_scale(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{anyhow::bail!("Node detector not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_node_reached::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Node detector not expected in this fixture")}
 fn entity_height(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{anyhow::bail!("Node detector not expected in this fixture")}
 }

 impl super::super::ai_edge_awareness::DefinitionBackend for Services{
 fn unit_scale(&mut self,_:&mut super::super::ai_edge_awareness::State)->Result<Option<f32>>{anyhow::bail!("Edge awareness not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_edge_awareness::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Edge awareness not expected in this fixture")}
 }

 impl super::super::ai_steering::DefinitionBackend for Services{
 fn unit_scale(&mut self,_:&mut super::super::ai_steering::State)->Result<Option<f32>>{anyhow::bail!("Biped steering not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_steering::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Biped steering not expected in this fixture")}
 }

 impl super::super::ai_compute_target::DefinitionBackend for Services{
 fn unit_scale(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<f32>>{anyhow::bail!("Queue target not expected in this fixture")}
 fn profiles(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<usize>>{anyhow::bail!("Queue target not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_compute_target::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Queue target not expected in this fixture")}
 }

}
