//!1bec1c ->196690 ->456530 ->1bef20 on canonical owned records.
//! Local pathfinder, gameplay actor and speed queries read the real objects.
//! World, traversal/task policy, allocation/observation and cleanup stay
//! required services until the manager owns their concrete implementations.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_flee_agent::{self,State,Acceptance},ai_agent_actions::{self,View},
    ai_navigation_definitions::Tree,ai_navigation_entity::{self,Owned,Registry},
    ai_navigation_service_factory::Owned as Service,ai_graph_link_passes::Memory};
pub trait Services:ai_agent_actions::ConstructionHost+super::ai_task_manager::Host {
    fn unit_scale(&mut self)->Result<Option<f32>>;
    fn world_service(&mut self,class:u32)->Result<Option<usize>>;
    fn number(&mut self,text:&[u8])->Result<f64>;
    fn errno(&mut self)->&mut i32;
    fn traversal(&mut self,manager:usize,text:Option<&[u8]>)->Result<i32>;
    fn allocate(&mut self,s:&mut State,m:&mut Memory,bytes:u32)->Result<u32>;
    fn release_reservation(&mut self,s:&mut State,m:&mut Memory,reservation:usize)->Result<()>;
    fn detach_service(&mut self,s:&mut State,m:&mut Memory,service:usize,entity:usize)->Result<()>;
    fn motion_vector(&mut self,owner:usize)->Result<Option<[f32;3]>>;
    fn motion_scalar(&mut self,owner:usize)->Result<Option<f32>>;
}
pub struct Binding<'a,'s,H> {
    pub tree:&'a mut Tree,pub actions:View<'s>,
    pub services:&'a BTreeMap<usize,Service>,
    pub entity:&'a Owned,pub entity_registry:&'a Registry,pub backend:&'a mut H,
}
fn speed(entity:&Owned,registry:&Registry)->Result<Option<f32>> {
    Ok(entity.optional_scalar(ai_navigation_entity::Kind::MaximumSpeed,registry)?.map(|a|f32::from_bits(a.value_bits)))
}
impl<H> Binding<'_, '_, H> {
    fn owner(&self,id:usize)->Result<()> {ensure!(id==self.actions.parent.brain.identity,"Flee query owner is not the canonical brain");Ok(())}
    fn entity(&self,owner:usize)->Result<usize> {
        self.owner(owner)?;
        let id=self.actions.parent.base.owner.context("Flee brain has no entity")?;
        ensure!(id==self.entity.entity.identity,"Flee brain references a different canonical entity");Ok(id)
    }
    fn pathfinder(&self,owner:usize)->Result<Option<usize>> {
        self.owner(owner)?;
        super::ai_navigation_owned_factory::find_pathfinder(self.actions.parent.base,self.services)
    }
    fn service(&self,id:usize)->Result<&super::ai_pathfinder_constructor::State> {
        let Service::Pathfinder(s)=self.services.get(&id).context("Flee pathfinder is not owned")?;
        ensure!(s.identity==id && s.base.owner==self.actions.parent.brain.identity,"Flee pathfinder identity or owner differs");Ok(s)
    }
}
impl<H:Services> ai_flee_agent::Host for Binding<'_, '_, H> {
    fn global_unit_scale(&mut self)->Result<Option<f32>>{self.backend.unit_scale()}
    fn owner_entity(&mut self,s:&mut State)->Result<usize>{self.entity(s.base.owner.context("Flee agent has no owner")?)}
    fn release_reservation(&mut self,s:&mut State,m:&mut Memory,r:usize)->Result<()>{self.backend.release_reservation(s,m,r)}
    fn detach_service(&mut self,s:&mut State,m:&mut Memory,v:usize,e:usize)->Result<()>{self.backend.detach_service(s,m,v,e)}
}
impl<H:Services> ai_flee_agent::BindingHost for Binding<'_, '_, H> {
    fn owner_service(&mut self,s:&mut State)->Result<Option<usize>>{self.pathfinder(s.base.owner.context("Flee agent has no owner")?)}
    fn world_service(&mut self,_:&mut State,class:u32)->Result<Option<usize>>{self.backend.world_service(class)}
    fn service_3c(&mut self,_:&mut State,id:usize)->Result<Option<Acceptance>> {
        use super::ai_pathfinder_constructor::Reference;
        let value=self.service(id)?.base.references_c_10[0].context("Pathfinder acceptance field is uninitialized")?;
        Ok(value.map(|v|match v {Reference::External(id)=>Acceptance::External(id),Reference::InlineAcceptance=>Acceptance::PathfinderInline(id)}))
    }
    fn entity_speed(&mut self,_:&mut State,id:usize)->Result<Option<f32>> {
        ensure!(id==self.entity.entity.identity,"Flee speed query entity differs");speed(self.entity,self.entity_registry)
    }
    fn bind_base(&mut self,s:&mut State,_:&mut Memory,root:usize)->Result<bool>{super::ai_agent_base::bind_owned(s,root,self)}
    fn service_field_50(&mut self,_:&mut State,id:usize)->Result<Option<usize>>{Ok(self.service(id)?.references_3c_4c_50_54_58_60_6c[2])}
    fn allocate(&mut self,s:&mut State,m:&mut Memory,bytes:u32)->Result<u32>{self.backend.allocate(s,m,bytes)}
}
impl<H:Services> super::ai_agent_base::Host for Binding<'_, '_, H> {
    fn owner_is_112a0(&mut self,id:usize)->Result<bool>{self.owner(id)?;Ok(true)} //Typed JPSBaseAIBrain getter2b7784.
    fn blackboard_head(&mut self,owner:usize)->Result<Option<usize>> {
        self.entity(owner)?;
        //196724 dereferences entity+20, the190774-owned user-data pair,
        //then its first word. It is the gameplay actor, not a blackboard head.
        Ok(Some(self.entity.entity.user_data[0]))
    }
    fn owner_service_6af6c(&mut self,id:usize)->Result<Option<usize>>{self.pathfinder(id)}
    fn world_service_61d14(&mut self)->Result<Option<usize>>{self.backend.world_service(0x61d14)}
    fn world_service_45dcc(&mut self)->Result<Option<usize>>{self.backend.world_service(0x45dcc)}
    fn initialize_attributes(&mut self,_:&mut super::ai_agent_base::Agent,_:usize)->Result<bool>{anyhow::bail!("Flee bind lost its complete derived object")}
    fn speed_value_360e4(&mut self,owner:usize)->Result<Option<f32>>{self.entity(owner)?;speed(self.entity,self.entity_registry)}
    fn action_vector(&mut self,owner:usize)->Result<Option<[f32;3]>>{self.owner(owner)?;self.backend.motion_vector(owner)}
    fn action_scalar(&mut self,owner:usize)->Result<Option<f32>>{self.owner(owner)?;self.backend.motion_scalar(owner)}
}
struct Attributes<'a,H> {brain:usize,entity:&'a Owned,registry:&'a Registry,backend:&'a mut H}
impl<H:Services> ai_agent_actions::ConstructionHost for Attributes<'_,H> {
    fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{self.backend.action_class_constructed(a,id)}
}
impl<H:Services> super::ai_flee_definition_binding::Backend for Attributes<'_,H> {
    fn integer(&mut self,_:&mut State,_:&mut Tree,t:&[u8])->Result<i32>{Ok(super::native_text_number::atoi(t,self.backend.errno()))}
    fn number(&mut self,_:&mut State,_:&mut Tree,t:&[u8])->Result<f64>{self.backend.number(t)}
    fn unit_scale(&mut self,_:&mut State,_:&mut Tree)->Result<Option<f32>>{self.backend.unit_scale()}
    fn traversal(&mut self,s:&mut State,_:&mut Tree,t:Option<&[u8]>)->Result<i32>{self.backend.traversal(s.references_34_38_3c_40_44[1].context("Flee graph manager is absent")?,t)}
    fn task_manager(&mut self,_:&mut State,_:&mut Tree)->Result<usize>{super::ai_task_manager::current_identity(self.backend)}
    fn safety_task(&mut self,_:&mut State,_:&mut Tree,m:usize,t:Option<&[u8]>)->Result<i32>{super::ai_task_manager::lookup(self.backend,m,t)}
    fn maximum_speed(&mut self,s:&mut State,_:&mut Tree)->Result<Option<f32>> {
        ensure!(s.base.owner==Some(self.brain),"Flee attribute changed to an unowned brain");speed(self.entity,self.registry)
    }
}
impl<H:Services> super::ai_agent_base::BindingHost<State> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,s:&mut State,root:usize)->Result<bool> {
        self.entity(s.base.owner.context("Flee attribute owner absent")?)?;
        let mut attributes=Attributes{brain:self.actions.parent.brain.identity,entity:self.entity,registry:self.entity_registry,backend:self.backend};
        super::ai_agent_definition_binding::Binding{tree:self.tree,actions:self.actions.reborrow(),backend:&mut attributes}.initialize(s,root)
    }
}
/// Normal child-initializer entry: every touched record remains in its owning
/// map. Construction, service publication and World admission are separate.
pub fn bind_owned<H:Services>(parent:ai_agent_actions::Parent<'_>,children:&mut super::ai_navigation_owned_factory::Children<'_>,
    entity:&Owned,entity_registry:&Registry,tree:&mut Tree,agent:usize,root:usize,
    class:&super::ai_navigation_construction::ActionClass,registry:&super::ai_navigation_action::Registry,m:&mut Memory,backend:&mut H)->Result<bool> {
    let (agents,services,actions)=children.agent_action_parts(parent,class,registry);
    let record=agents.get_mut(&agent).context("Bound Flee agent is not owned")?;
    ensure!(record.identity==agent && record.owner()==Some(actions.parent.brain.identity),"Bound Flee agent identity or owner differs");
    let super::ai_navigation_agent_factory::Owned::Flee(s)=&mut record.object else{anyhow::bail!("Flee initializer received a different agent class")};
    s.bind(root,m,&mut Binding{tree,actions,services,entity,entity_registry,backend})
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_action as action,ai_navigation_construction::{Constructed,ActionClass,ActionFactory,Layout},ai_navigation_definitions::Node,ai_pathfinder_constructor as path};
    #[derive(Default)]
    struct Backend {missing_world:bool,errno:i32,next:u32,allocations:Vec<(u32,u32)>,trace:Vec<String>,tasks:Option<super::super::ai_task_manager::State>}
    impl ai_agent_actions::ConstructionHost for Backend {
        fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{assert_eq!(a.action_class(id)?,40);self.trace.push("action".into());Ok(())}
    }
    impl super::super::ai_task_manager::Host for Backend {
        fn tasks(&mut self,id:Option<usize>)->Result<&mut super::super::ai_task_manager::State>{self.trace.push(if id.is_none(){"task_manager"}else{"safety_task"}.into());if let Some(id)=id{assert_eq!(id,70);}self.tasks.as_mut().context("Task manager not registered")}
        fn task_clock(&mut self)->Result<Option<f32>>{anyhow::bail!("Binding must not run task clock")}
    }
    impl Services for Backend {
        fn unit_scale(&mut self)->Result<Option<f32>>{self.trace.push("scale".into());Ok(Some(2.))}
        fn world_service(&mut self,c:u32)->Result<Option<usize>>{self.trace.push(format!("world:{c:x}"));match c{0x61d14=>Ok((!self.missing_world).then_some(60)),0x45dcc=>Ok(None),_=>anyhow::bail!("Unexpected class")}}
        fn number(&mut self,t:&[u8])->Result<f64>{assert_eq!(t,b"2");self.trace.push("number".into());Ok(2.)}
        fn errno(&mut self)->&mut i32{&mut self.errno}
        fn traversal(&mut self,m:usize,t:Option<&[u8]>)->Result<i32>{assert_eq!(m,60);assert_eq!(t,Some(b"FleeTraversal".as_slice()));self.trace.push("traversal".into());Ok(4)}
        fn allocate(&mut self,_:&mut State,m:&mut Memory,n:u32)->Result<u32>{let p=self.next;self.next=p.checked_add(n+16).unwrap();m.regions.insert(p,vec![0xaa;n as usize]);self.allocations.push((p,n));Ok(p)}
        fn release_reservation(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected reservation")}
        fn detach_service(&mut self,_:&mut State,_:&mut Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected detach")}
        fn motion_vector(&mut self,_:usize)->Result<Option<[f32;3]>>{anyhow::bail!("Unexpected cache")}
        fn motion_scalar(&mut self,_:usize)->Result<Option<f32>>{anyhow::bail!("Unexpected cache")}
    }
    struct Shape;
    impl ai_navigation_entity::Host for Shape {
        fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(0.3)}
        fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(1.5)}
        fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(5.)}
        fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(Some(1.))}
    }
    struct Constructor;
    impl path::BaseHost for Constructor {
        fn unit_scale(&mut self,_:&mut path::Base)->Result<Option<f32>>{Ok(None)}
        fn profiling_enabled(&mut self,_:&mut path::Base)->Result<bool>{Ok(false)}
        fn register_budget(&mut self,_:&mut path::Base,_:&str)->Result<i32>{anyhow::bail!("Unexpected profiling")}
    }
    impl path::Host for Constructor {
        fn world_service(&mut self,_:&mut path::State,_:path::Service)->Result<Option<usize>>{Ok(None)}
        fn point_attribute_index(&mut self,_:&mut path::State)->Result<i32>{Ok(3)}
    }
    fn registry()->Registry {
        use ai_navigation_entity::{Class,Kind};
        Registry{count:3,diameter:Class{index:0,constructor:Some(Kind::Diameter)},height:Class{index:1,constructor:Some(Kind::Height)},maximum_speed:Class{index:2,constructor:Some(Kind::MaximumSpeed)}}
    }
    #[test]
    fn complete_outer_bind_uses_canonical_entity_pathfinder_actions_and_buffers()->Result<()> {
        for mode in [0,1,2,3] {
            let shared=mode==1;let published=mode!=2;let mut backend=Backend{missing_world:mode==3,next:0x60000000,..Default::default()};
            backend.tasks=Some(super::super::ai_task_manager::State{identity:70,limit:0.,exhausted:0,entries:[b"first".as_slice(),b"second".as_slice(),b"SafetyTask".as_slice()].into_iter().map(|name|super::super::ai_task_manager::Task{name:name.to_vec(),maximum_time:0.,maximum_calls:0,calls:0,started:0.,elapsed:0.}).collect()});
            let class=ActionClass{metadata:40,factory:Some(ActionFactory::BaseAi)};
            let layout=Layout{agent_classes:3,service_classes:1,attributes:action::Registry{count:2,heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}}};
            let mut parent=Constructed::new(7,Some(50),Some(20),&class,&layout)?;
            let registry=registry();let entity=Owned::new(50,Some(b"Gus fixture"),[123,9],&registry,&mut Shape)?;
            let mut counter=path::Counter::default();let mut p=path::State::construct(30,&parent,31,&layout.attributes,&mut counter,&mut Constructor)?;
            // These are supplied initialized service fields/publication, not
            // evidence that the constructor completed48fec4 on its own.
            p.base.references_c_10[0]=Some(Some(path::Reference::InlineAcceptance));p.references_3c_4c_50_54_58_60_6c[2]=Some(90);
            let services=BTreeMap::from([(30,Service::Pathfinder(Box::new(p)))]);
            if published{parent.base.services.push(30);}
            let mut s=State::new(Some(7),&mut InitialScale)?;
            let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
            for (n,v) in [("Class","Mpd::JPSFleeAgent"),("ShareBrainAction",if shared{"true"}else{"false"}),("TraversalConfig","FleeTraversal"),("CheckSafetyTask","SafetyTask"),("MaxDangerousPoints","2"),("MaxDangerousEntities","3"),("Speed","2")] {
                tree.append(Some(root),Node{name:Some(n.as_bytes().to_vec()),value:Some(v.as_bytes().to_vec()),..Default::default()})?;
            }
            let mut store=ai_agent_actions::Store::default();let mut next=40;let mut memory=Memory::default();
            let mut b=Binding{tree:&mut tree,actions:View{parent:(&mut parent).into(),private:&mut store,class:&class,registry:&layout.attributes,next_identity:&mut next},services:&services,entity:&entity,entity_registry:&registry,backend:&mut backend};
            let result=s.bind(root,&mut memory,&mut b)?;assert_eq!(result,mode<2);
            assert_eq!(s.references_34_38_3c_40_44[0],published.then_some(30));
            assert_eq!(s.reference_4c,if published{Some(Acceptance::PathfinderInline(30))}else{None});
            if mode>=2 {assert_eq!(s.base.action_04,None);assert!(b.backend.allocations.is_empty());assert_eq!(b.backend.trace,["world:61d14","world:45dcc"]);continue}
            assert_eq!(s.base.blackboard_head,123);assert_eq!(s.base.speed,5.);assert_eq!(s.speed_8c,4.);
            assert_eq!((s.word_48,s.index_94),(4,2));assert_eq!(s.references_34_38_3c_40_44[3],Some(90));
            assert_eq!(s.base.action_04,Some(if shared{20}else{41}));assert_eq!(b.actions.private.objects.len(),usize::from(!shared));
            assert_eq!(b.backend.allocations.iter().map(|(_,n)|*n).collect::<Vec<_>>(),[12,40,12,8]);
            let p=b.backend.allocations[1].0;assert_eq!(memory.word(p)?,12);assert_eq!(memory.word(p+4)?,2);
            assert_eq!(s.arrays_50_54[0],Some((p+16) as usize));for offset in (16..40).step_by(4){assert_eq!(memory.word(p+offset)?,0);}
            assert_eq!(s.capacity_5c,2);assert_eq!(s.capacity_6c,3);assert_eq!(b.backend.errno,0);
        }Ok(())
    }
    struct InitialScale;
    impl ai_flee_agent::Host for InitialScale {
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut State)->Result<usize>{anyhow::bail!("Unexpected ctor owner")}
        fn release_reservation(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected ctor reservation")}
        fn detach_service(&mut self,_:&mut State,_:&mut Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected ctor detach")}
    }
}
