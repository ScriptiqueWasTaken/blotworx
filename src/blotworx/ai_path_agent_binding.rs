//! Canonical Goto/Wander194e14 ->196690 ->definition callbacks -> per-agent
//! arena path creation/binding/reset. No copied path or parent storage.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_agent_path as path,ai_agent_actions::{self,View},ai_agent_base,
    ai_navigation_definitions::Tree,ai_navigation_entity::{self,Owned,Registry},
    ai_navigation_service_factory::Owned as Service,ai_pathfinder_modifier_factory::Owned as Modifier,
    ai_graph_link_passes::Memory,ai_goto_agent,ai_path_agent};
pub trait Services:super::ai_flee_binding::Services+path::DestructionHost {
    fn allocate_path_object(&mut self,m:&mut Memory,bytes:u32)->Result<u32>;
    fn allocate_path_buffer(&mut self,m:&mut Memory,path:u32,bytes:u32)->Result<u32>;
}
/// Service4892d4/4893b4/4893fc retain their own modifier/route policy.
/// No ready/status callback can bypass the actual mapped path update.
pub trait UpdateServices:Services+super::ai_agent_path_runtime::Host+super::ai_agent_path_initialize::Host {
    fn inline_acceptance(&mut self,service:usize)->Result<u32>;
    fn begin_route(&mut self,m:&mut Memory,service:&mut super::ai_pathfinder_constructor::State)->Result<()>;
    fn append_route(&mut self,m:&mut Memory,service:&mut super::ai_pathfinder_constructor::State,point:u32)->Result<()>;
    fn finish_route(&mut self,m:&mut Memory,service:&mut super::ai_pathfinder_constructor::State,flag:u32)->Result<()>;
}
struct Runtime<'a,H>{entity:&'a Owned,services:&'a BTreeMap<usize,Service>,brain:usize,backend:&'a mut H}
impl<H:UpdateServices> path::ResetHost for Runtime<'_,H>{
    fn reservation(&mut self,m:&mut Memory,a:u32,b:u32,c:u32)->Result<u32>{self.backend.reservation(m,a,b,c)}
    fn release(&mut self,m:&mut Memory,r:u32)->Result<()>{self.backend.release(m,r)}
}
impl<H:UpdateServices> super::ai_agent_path_runtime::Host for Runtime<'_,H>{
    fn reservation_word(&self,m:&Memory,r:u32,o:u32)->Result<u32>{self.backend.reservation_word(m,r,o)}
    fn reservation_byte(&self,m:&Memory,r:u32,o:u32)->Result<u8>{self.backend.reservation_byte(m,r,o)}
    fn owner_position(&mut self,_:&mut Memory,o:u32)->Result<[f32;3]>{ensure!(o as usize==self.entity.entity.identity,"Updated path belongs to a different entity");Ok(std::array::from_fn(|i|self.entity.entity.values_34_58[i]))}
    fn path_virtual(&mut self,m:&mut Memory,p:u32,s:u32,r:u32)->Result<u32>{if s==0x1c{return Ok(u32::from(super::ai_agent_path_request::request(m,p,r,self)?))}if s==0x18{return Ok(u32::from(match m.word(p.wrapping_add(0x868))?{0x4eae38=>super::ai_agent_path_initialize::goto(m,p,r,self)?,0x4eb818=>super::ai_agent_path_initialize::wander(m,p,r,self)?,_=>anyhow::bail!("No concrete path initializer")}))}anyhow::bail!("Unknown path virtual slot")}
    fn refresh(&mut self,m:&mut Memory,r:u32)->Result<()>{self.backend.refresh(m,r)}
    fn accepts_point(&mut self,m:&mut Memory,r:u32,p:u32)->Result<u32>{self.backend.accepts_point(m,r,p)}
    fn result(&mut self,m:&mut Memory,r:u32,i:u32)->Result<(u32,u32,u32)>{self.backend.result(m,r,i)}
    fn sample(&mut self,m:&mut Memory,r:u32,p:u32,i:u32)->Result<()>{self.backend.sample(m,r,p,i)}
    fn acknowledge(&mut self,m:&mut Memory,r:u32)->Result<()>{self.backend.acknowledge(m,r)}
}
impl<H:UpdateServices> super::ai_agent_path_request::Host for Runtime<'_,H>{
    fn request_manager(&mut self,_:&mut Memory)->Result<u32>{Ok(u32::try_from(super::ai_task_manager::current_identity(self.backend)?)?)}
    fn request_task(&mut self,m:&mut Memory,r:u32)->Result<u32>{self.backend.request_task(m,r)}
    fn request_enabled(&mut self,_:&mut Memory,a:u32,t:u32)->Result<u32>{Ok(u32::from(super::ai_task_manager::enabled(self.backend,a as usize,t)?))}
    fn service_provider(&mut self,m:&mut Memory,s:u32)->Result<u32>{
        let Service::Pathfinder(state)=self.services.get(&(s as usize)).context("Request service is not owned")?;
        ensure!(state.identity==s as usize && state.base.owner==self.brain,"Request service identity or owner differs");
        Ok(state.graph_provider(m)?.map(u32::try_from).transpose()?.unwrap_or(0))
    }
    fn provider_ready(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.backend.provider_ready(m,p)}
    fn provider_configuration(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.backend.provider_configuration(m,p)}
    fn configuration_position(&mut self,m:&mut Memory,p:u32)->Result<[f32;3]>{self.backend.configuration_position(m,p)}
    fn reservation_extent(&mut self,m:&mut Memory,r:u32)->Result<f32>{self.backend.reservation_extent(m,r)}
    fn request_bounds(&mut self,m:&mut Memory,r:u32,l:[f32;3],u:[f32;3],f:[u32;2])->Result<u32>{self.backend.request_bounds(m,r,l,u,f)}
    fn set_target(&mut self,m:&mut Memory,r:u32,n:u32,p:[f32;3])->Result<()>{self.backend.set_target(m,r,n,p)}
}
impl<H:UpdateServices> super::ai_agent_path_initialize::Host for Runtime<'_,H>{
    fn random_state(&mut self)->Result<&mut super::game_random::State>{self.backend.random_state()}
    fn traversal_mut(&mut self,r:u32)->Result<&mut super::ai_traversal_instances::Instance>{self.backend.traversal_mut(r)}
    fn strategy_mut(&mut self,s:usize)->Result<&mut super::ai_flat_handle_strategy::State>{self.backend.strategy_mut(s)}
    fn initial_position(&mut self,_:&mut Memory,o:u32)->Result<[f32;3]>{ensure!(o as usize==self.entity.entity.identity,"Initial path belongs to another entity");Ok(std::array::from_fn(|i|self.entity.entity.values_34_58[i]))}
    fn service_graph(&mut self,_:&mut Memory,s:u32)->Result<Option<usize>>{let Service::Pathfinder(state)=self.services.get(&(s as usize)).context("Initial path service is not owned")?;ensure!(state.identity==s as usize && state.base.owner==self.brain,"Initial path service identity or owner differs");Ok(state.references_3c_4c_50_54_58_60_6c[2])}
    fn service_acceptance(&mut self,_:&mut Memory,s:u32)->Result<u32>{
        let Service::Pathfinder(state)=self.services.get(&(s as usize)).context("Acceptance service is not owned")?;
        ensure!(state.identity==s as usize && state.base.owner==self.brain,"Acceptance service identity or owner differs");
        use super::ai_pathfinder_constructor::Reference;
        match state.base.references_c_10[0].context("Acceptance interface is uninitialized")?{None=>Ok(0),Some(Reference::External(id))=>Ok(u32::try_from(id)?),Some(Reference::InlineAcceptance)=>self.backend.inline_acceptance(s as usize)}
    }
    fn task_enter(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{super::ai_task_manager::enter(self.backend,a as usize,t)}
    fn task_leave(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{super::ai_task_manager::leave(self.backend,a as usize,t)}
    fn project_actor(&mut self,m:&mut Memory,p:u32,v:[f32;3],s:u32,f:[u32;4])->Result<super::ai_agent_path_initialize::Projection>{self.backend.project_actor(m,p,v,s,f)}
    fn select_provider_node(&mut self,m:&mut Memory,p:u32,n:u32)->Result<()>{self.backend.select_provider_node(m,p,n)}
    fn configuration_node(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.backend.configuration_node(m,p)}
    fn project_goal(&mut self,m:&mut Memory,p:u32,v:u32,w:u32,n:u32,g:[f32;3])->Result<super::ai_agent_path_initialize::Projection>{self.backend.project_goal(m,p,v,w,n,g)}
}
pub struct Binding<'a,'s,H> {
    pub tree:&'a mut Tree,pub actions:View<'s>,pub services:&'a mut BTreeMap<usize,Service>,
    pub modifiers:&'a BTreeMap<usize,Modifier>,pub entity:&'a Owned,pub entity_registry:&'a Registry,
    pub memory:&'a mut Memory,pub context:path::Context,pub backend:&'a mut H,
}
fn speed(entity:&Owned,r:&Registry)->Result<Option<f32>> {Ok(entity.optional_scalar(ai_navigation_entity::Kind::MaximumSpeed,r)?.map(|a|f32::from_bits(a.value_bits)))}
impl<H> Binding<'_, '_, H> {
    fn owner(&self,id:usize)->Result<()> {ensure!(id==self.actions.parent.brain.identity,"Path agent query owner differs");Ok(())}
    fn entity(&self,id:usize)->Result<()> {self.owner(id)?;ensure!(self.actions.parent.base.owner==Some(self.entity.entity.identity),"Path agent canonical entity differs");Ok(())}
    fn service(&self,id:usize)->Result<Option<usize>> {self.owner(id)?;super::ai_navigation_owned_factory::find_pathfinder(self.actions.parent.base,self.services)}
}
struct Factory<'a,'s,H>{actions:&'a mut View<'s>,services:&'a BTreeMap<usize,Service>,modifiers:&'a BTreeMap<usize,Modifier>,backend:&'a mut H}
impl<H> Factory<'_, '_, H> {
    fn owner(&self,id:usize)->Result<()> {ensure!(id==self.actions.parent.brain.identity,"Path factory parent differs");Ok(())}
    fn service(&self,id:u32)->Result<&super::ai_pathfinder_constructor::State> {
        let Service::Pathfinder(s)=self.services.get(&(id as usize)).context("Path service is not owned")?;
        ensure!(s.identity==id as usize && s.base.owner==self.actions.parent.brain.identity,"Path service identity or parent differs");Ok(s)
    }
}
impl<H:Services> path::BindingHost for Factory<'_, '_, H> {
    fn world_service(&mut self,_:&mut Memory,_:u32)->Result<u32>{Ok(self.backend.world_service(0x61d14)?.map(u32::try_from).transpose()?.unwrap_or(0))}
    fn owner_service(&mut self,_:&mut Memory,_:u32,id:usize)->Result<u32>{self.owner(id)?;Ok(super::ai_navigation_owned_factory::find_pathfinder(self.actions.parent.base,self.services)?.map(u32::try_from).transpose()?.unwrap_or(0))}
    fn owner_word40(&mut self,_:&mut Memory,_:u32,id:usize)->Result<u32>{self.owner(id)?;Ok(self.actions.parent.brain.owner_40.map(u32::try_from).transpose()?.unwrap_or(0))}
    fn traversal(&mut self,_:&mut Memory,_:u32,manager:u32,name:Option<&[u8]>)->Result<u32>{Ok(self.backend.traversal(manager as usize,name)? as u32)}
    fn buffer_size(&mut self,_:&mut Memory,id:u32)->Result<u32>{Ok(self.service(id)?.words_90_94_98[1])}
    fn radius(&mut self,_:&mut Memory,id:u32)->Result<f32>{
        //Service+134 is table2.selected, NOT the character radius.
        let selected=self.service(id)?.tables_fc_1fc[2].selected.context("Goal-change selection is uninitialized")?.context("Goal-change detector is absent")?;
        let Modifier::GoalDetection(d)=self.modifiers.get(&selected).context("Goal-change detector is not owned")? else{anyhow::bail!("Selected goal-change detector class has no distance scalar")};
        ensure!(d.identity==selected && d.pathfinder==id as usize,"Goal-change detector identity or parent differs");
        let super::ai_goal_detection::Fields::Changed3D{distance,..}=d.fields else{anyhow::bail!("Selected goal-change detector has a different native layout")};
        distance.context("Goal-change distance is uninitialized")
    }
    fn allocate(&mut self,m:&mut Memory,p:u32,n:u32)->Result<u32>{self.backend.allocate_path_buffer(m,p,n)}
}
///The brain's independent CanReach path uses the same actual Goto binding,
///published pathfinder and selected goal-change detector as its path agents.
pub fn bind_brain_path(parent:ai_agent_actions::Parent<'_>,children:&mut super::ai_navigation_owned_factory::Children<'_>,
    class:&super::ai_navigation_construction::ActionClass,registry:&super::ai_navigation_action::Registry,
    modifiers:&BTreeMap<usize,Modifier>,memory:&mut Memory,path:u32,name:&[u8],backend:&mut impl Services)->Result<bool>{
    let brain=parent.brain.identity;
    let (_,services,mut view)=children.agent_action_parts(parent,class,registry);
    path::bind_goto(memory,path,Some(name),Some(brain),&mut Factory{actions:&mut view,services,modifiers,backend})
}
impl<H:Services> path::FactoryHost for Factory<'_, '_, H> {
    fn allocate_object(&mut self,m:&mut Memory,n:u32)->Result<u32>{self.backend.allocate_path_object(m,n)}
}
impl<H:Services> path::ResetHost for Factory<'_, '_, H> {
    fn reservation(&mut self,m:&mut Memory,a:u32,b:u32,c:u32)->Result<u32>{self.backend.reservation(m,a,b,c)}
    fn release(&mut self,m:&mut Memory,p:u32)->Result<()>{self.backend.release(m,p)}
}
impl<H:Services> Binding<'_, '_, H> {
    fn create(&mut self,kind:path::Kind,owner:Option<usize>)->Result<Option<usize>> {
        let owner=owner.context("Path factory has no bound owning brain")?;self.owner(owner)?;
        let p=path::create(self.memory,self.context,kind,owner,&mut Factory{actions:&mut self.actions,services:self.services,modifiers:self.modifiers,backend:self.backend})?;
        Ok((p!=0).then_some(p as usize))
    }
    pub fn reset_wander(&mut self,a:&mut ai_path_agent::Agent)->Result<()> {
        let p=u32::try_from(a.pathfinder.context("Path agent reset has a null required path")?)?;
        path::reset(self.memory,p,&mut Factory{actions:&mut self.actions,services:self.services,modifiers:self.modifiers,backend:self.backend})?;
        a.request_state=-1;Ok(())
    }
    pub fn reset_goto(&mut self,a:&mut ai_goto_agent::Agent)->Result<()> {
        for i in 0..3 {a.vector_90[i]=f32::from_bits(self.memory.word(self.context.global_vector.wrapping_add(i as u32*4))?);}
        a.field_9c=-1;a.reference=None;a.flag_a8=Some(false);self.reset_wander(&mut a.path)
    }
}
impl<H:UpdateServices> Binding<'_, '_, H> {
    ///194ed0/195404 over the bound canonical agent and mapped path.
    pub fn update(&mut self,a:&mut ai_path_agent::Agent)->Result<u32>{
        self.entity(a.base.owner.context("Updated agent has no brain")?)?;
        let p=u32::try_from(a.pathfinder.context("Updated agent has no path")?)?;
        super::ai_agent_path_runtime::motion(self.memory,p,a.base.action_vector,a.base.action_scalar)?;
        let ready=super::ai_agent_path_runtime::ready(self.memory,p,&mut Runtime{entity:self.entity,services:self.services,brain:self.actions.parent.brain.identity,backend:self.backend})?;
        if !ready{return Ok(u32::from(super::ai_agent_path_runtime::status(self.memory,p)?==0))}
        //195da4 deliberately ignores begin's boolean return and rereads both
        //sample count and service on every iteration.
        if self.memory.word(p.wrapping_add(0x854))?!=0 {
            let id=self.memory.word(p.wrapping_add(12))? as usize;
            let Service::Pathfinder(service)=self.services.get_mut(&id).context("Route service is not owned")?;
            ensure!(service.base.owner==self.actions.parent.brain.identity,"Route service belongs to another brain");
            self.backend.begin_route(self.memory,service)?;
            let mut index=0u32;
            while index<self.memory.word(p.wrapping_add(0x854))? {
                let id=self.memory.word(p.wrapping_add(12))? as usize;
                let Service::Pathfinder(service)=self.services.get_mut(&id).context("Route append service is not owned")?;
                ensure!(service.base.owner==self.actions.parent.brain.identity,"Route append service belongs to another brain");
                self.backend.append_route(self.memory,service,p.wrapping_add(0x84).wrapping_add(index.wrapping_mul(0x14)))?;
                index=index.wrapping_add(1);
            }
            let id=self.memory.word(p.wrapping_add(12))? as usize;
            let Service::Pathfinder(service)=self.services.get_mut(&id).context("Route finish service is not owned")?;
            ensure!(service.base.owner==self.actions.parent.brain.identity,"Route finish service belongs to another brain");
            self.backend.finish_route(self.memory,service,1)?;
        }
        self.copy_point(a,p)
    }
    fn copy_point(&mut self,a:&mut ai_path_agent::Agent,p:u32)->Result<u32>{
        for i in 0..3{a.point.position[i]=f32::from_bits(self.memory.word(p.wrapping_add(0x18+i as u32*4))?);}
        let slot=self.memory.word(self.context.point_attribute_class.wrapping_add(8))?;
        let source=self.memory.word(p.wrapping_add(0x24).wrapping_add(slot.wrapping_mul(4)))?;
        if source==0{return Ok(0)}
        let index=self.memory.word(source.wrapping_add(4))?;if index==u32::MAX{return Ok(0)}
        let target=a.point.attributes.get(slot as usize).copied().flatten().context("Reconciled agent point has no required observer attribute")?;
        self.memory.write(u32::try_from(target)?.wrapping_add(4),index)?;Ok(1)
    }
}
impl<H:Services> ai_agent_base::Host for Binding<'_, '_, H> {
    fn owner_is_112a0(&mut self,id:usize)->Result<bool>{self.owner(id)?;Ok(true)} //Pure2b7784.
    fn blackboard_head(&mut self,id:usize)->Result<Option<usize>>{self.entity(id)?;Ok(Some(self.entity.entity.user_data[0]))}
    fn owner_service_6af6c(&mut self,id:usize)->Result<Option<usize>>{self.service(id)}
    fn world_service_61d14(&mut self)->Result<Option<usize>>{self.backend.world_service(0x61d14)}
    fn world_service_45dcc(&mut self)->Result<Option<usize>>{self.backend.world_service(0x45dcc)}
    fn initialize_attributes(&mut self,_:&mut ai_agent_base::Agent,_:usize)->Result<bool>{anyhow::bail!("Path bind lost its complete derived object")}
    fn speed_value_360e4(&mut self,id:usize)->Result<Option<f32>>{self.entity(id)?;speed(self.entity,self.entity_registry)}
    fn action_vector(&mut self,id:usize)->Result<Option<[f32;3]>>{self.owner(id)?;self.backend.motion_vector(id)}
    fn action_scalar(&mut self,id:usize)->Result<Option<f32>>{self.owner(id)?;self.backend.motion_scalar(id)}
}
struct Attributes<'a,H>{brain:usize,entity:&'a Owned,registry:&'a Registry,backend:&'a mut H}
impl<H:Services> ai_agent_actions::ConstructionHost for Attributes<'_,H> {
    fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{self.backend.action_class_constructed(a,id)}
}
impl<H:Services> super::ai_path_agent_attribute_binding::Backend for Attributes<'_,H> {
    fn number(&mut self,_:&mut ai_goto_agent::Agent,_:&mut Tree,t:&[u8])->Result<f64>{self.backend.number(t)}
    fn unit_scale(&mut self,_:&mut ai_goto_agent::Agent,_:&mut Tree)->Result<Option<f32>>{self.backend.unit_scale()}
    fn maximum_speed(&mut self,a:&mut ai_goto_agent::Agent,_:&mut Tree,owner:usize)->Result<Option<f32>>{ensure!(owner==self.brain && a.path.base.owner==Some(owner),"Goto speed owner differs");speed(self.entity,self.registry)}
}
impl<H:Services> ai_agent_base::BindingHost<ai_goto_agent::Agent> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,a:&mut ai_goto_agent::Agent,n:usize)->Result<bool>{
        self.entity(a.path.base.owner.context("Goto owner absent")?)?;
        let mut h=Attributes{brain:self.actions.parent.brain.identity,entity:self.entity,registry:self.entity_registry,backend:self.backend};
        super::ai_agent_definition_binding::Binding{tree:self.tree,actions:self.actions.reborrow(),backend:&mut h}.initialize(a,n)
    }
}
impl<H:Services> ai_agent_base::BindingHost<ai_path_agent::Agent> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,a:&mut ai_path_agent::Agent,n:usize)->Result<bool>{
        self.entity(a.base.owner.context("Wander owner absent")?)?;
        let mut h=Attributes{brain:self.actions.parent.brain.identity,entity:self.entity,registry:self.entity_registry,backend:self.backend};
        super::ai_agent_definition_binding::Binding{tree:self.tree,actions:self.actions.reborrow(),backend:&mut h}.initialize(a,n)
    }
}
impl<H:Services> ai_path_agent::BindingHost<ai_goto_agent::Agent> for Binding<'_, '_, H> {
    fn bind_base_owned(&mut self,a:&mut ai_goto_agent::Agent,n:usize)->Result<bool>{ai_agent_base::bind_owned(a,n,self)}
    fn create_pathfinder_owned(&mut self,a:&mut ai_goto_agent::Agent)->Result<Option<usize>>{self.create(path::Kind::Goto,a.path.base.bound_owner)}
    fn initialize_owned(&mut self,a:&mut ai_goto_agent::Agent)->Result<()>{self.reset_goto(a)}
}
impl<H:Services> ai_path_agent::BindingHost<ai_path_agent::Agent> for Binding<'_, '_, H> {
    fn bind_base_owned(&mut self,a:&mut ai_path_agent::Agent,n:usize)->Result<bool>{ai_agent_base::bind_owned(a,n,self)}
    fn create_pathfinder_owned(&mut self,a:&mut ai_path_agent::Agent)->Result<Option<usize>>{self.create(path::Kind::Wander,a.base.bound_owner)}
    fn initialize_owned(&mut self,a:&mut ai_path_agent::Agent)->Result<()>{self.reset_wander(a)}
}
pub fn bind_owned<H:Services>(parent:ai_agent_actions::Parent<'_>,children:&mut super::ai_navigation_owned_factory::Children<'_>,
    entity:&Owned,entity_registry:&Registry,tree:&mut Tree,agent:usize,root:usize,
    class:&super::ai_navigation_construction::ActionClass,registry:&super::ai_navigation_action::Registry,
    modifiers:&BTreeMap<usize,Modifier>,memory:&mut Memory,context:path::Context,backend:&mut H)->Result<bool> {
    let (agents,services,actions)=children.agent_action_parts(parent,class,registry);
    let record=agents.get_mut(&agent).context("Bound path agent is not owned")?;
    ensure!(record.identity==agent && record.owner()==Some(actions.parent.brain.identity),"Path agent identity or parent differs");
    let mut b=Binding{tree,actions,services,modifiers,entity,entity_registry,memory,context,backend};
    match &mut record.object {
        super::ai_navigation_agent_factory::Owned::Goto(a)=>a.bind(root,&mut b),
        super::ai_navigation_agent_factory::Owned::Wander(a)=>ai_path_agent::bind_owned(a.as_mut(),root,&mut b),
        super::ai_navigation_agent_factory::Owned::Flee(_)=>anyhow::bail!("Flee is not a path-agent subclass"),
    }
}
pub fn update_owned<H:UpdateServices>(parent:ai_agent_actions::Parent<'_>,children:&mut super::ai_navigation_owned_factory::Children<'_>,
    entity:&Owned,entity_registry:&Registry,tree:&mut Tree,agent:usize,
    class:&super::ai_navigation_construction::ActionClass,registry:&super::ai_navigation_action::Registry,
    modifiers:&BTreeMap<usize,Modifier>,memory:&mut Memory,context:path::Context,backend:&mut H)->Result<u32> {
    let (agents,services,actions)=children.agent_action_parts(parent,class,registry);
    let record=agents.get_mut(&agent).context("Updated path agent is not owned")?;
    ensure!(record.identity==agent && record.owner()==Some(actions.parent.brain.identity),"Updated path agent identity or parent differs");
    let mut b=Binding{tree,actions,services,modifiers,entity,entity_registry,memory,context,backend};
    match &mut record.object {
        super::ai_navigation_agent_factory::Owned::Goto(a)=>b.update(&mut a.path),
        super::ai_navigation_agent_factory::Owned::Wander(a)=>b.update(a),
        super::ai_navigation_agent_factory::Owned::Flee(_)=>anyhow::bail!("Flee is not a path-agent subclass"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_owned_factory::{Storage,tests::Constructor},ai_navigation_action as action,
        ai_navigation_construction::{Constructed,ActionClass,ActionFactory,Layout},ai_navigation_agent_factory::{Record,Class,Owned as Agent},
        ai_pathfinder_constructor as pf,ai_navigation_definitions::Node,ai_goal_detection::{self,Fields}};
    #[derive(Default)]
    struct Backend {next:u32,traversal_failed:bool,allocations:Vec<(u32,u32)>,errno:i32,updating:bool,reservation:Option<super::super::ai_traversal_instances::Instance>,route:Vec<u32>,strategy:super::super::ai_flat_handle_strategy::State,random:super::super::game_random::State,task_world:super::super::ai_task_context_world::Store,clock:u32}
    impl ai_agent_actions::ConstructionHost for Backend {
        fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{assert_eq!(a.parent.brain.identity,7);assert_eq!(a.action_class(id)?,40);Ok(())}
    }
    impl super::super::ai_task_clock::Backend for u32 {
        fn sample(&mut self,id:usize)->Result<f32>{assert_eq!(id,90);*self+=1;Ok(*self as f32)}
        fn reset(&mut self,id:usize)->Result<()>{assert_eq!(id,91);*self=0;Ok(())}
    }
    impl super::super::ai_task_manager::Host for Backend {
        fn tasks(&mut self,id:Option<usize>)->Result<&mut super::super::ai_task_manager::State>{self.task_world.tasks(id)}
        fn task_clock(&mut self)->Result<Option<f32>>{self.task_world.sample(&mut self.clock)}
    }
    impl super::super::ai_task_startup::Numbers for Backend {
        fn number(&mut self,text:&[u8])->Result<f64>{Ok(std::str::from_utf8(text)?.parse()?)}
        fn errno(&mut self)->&mut i32{&mut self.errno}
    }
    impl super::super::ai_task_frame::Host for Backend {
        fn frame(&mut self,id:usize)->Result<&mut super::super::ai_task_frame::Frame>{Ok(&mut self.task_world.manager_mut(id)?.frame)}
        fn task_globals(&mut self)->Result<&mut super::super::ai_task_frame::Globals>{Ok(&mut self.task_world.globals.frame)}
        fn world_actor_count(&mut self)->Result<u32>{Ok(self.task_world.current()?.actor_count)}
        fn reset_task_clock(&mut self)->Result<()>{self.task_world.reset_clock(&mut self.clock)}
    }
    impl super::super::ai_flee_binding::Services for Backend {
        fn unit_scale(&mut self)->Result<Option<f32>>{Ok(Some(2.))}
        fn world_service(&mut self,c:u32)->Result<Option<usize>>{match c{0x61d14=>Ok(Some(60)),0x45dcc=>Ok(None),_=>anyhow::bail!("Unexpected World class")}}
        fn number(&mut self,t:&[u8])->Result<f64>{assert_eq!(t,b"2");Ok(2.)}
        fn errno(&mut self)->&mut i32{&mut self.errno}
        fn traversal(&mut self,m:usize,t:Option<&[u8]>)->Result<i32>{assert_eq!(m,60);assert!(matches!(t,Some(b"GotoTraversal"|b"WanderTraversal"|b"CanReachTraversal")));Ok(if self.traversal_failed{-1}else{5})}
        fn allocate(&mut self,_:&mut super::super::ai_flee_agent::State,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Unexpected Flee allocation")}
        fn release_reservation(&mut self,_:&mut super::super::ai_flee_agent::State,_:&mut Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected Flee reservation")}
        fn detach_service(&mut self,_:&mut super::super::ai_flee_agent::State,_:&mut Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected Flee detach")}
        fn motion_vector(&mut self,_:usize)->Result<Option<[f32;3]>>{anyhow::bail!("Unexpected motion cache")}
        fn motion_scalar(&mut self,_:usize)->Result<Option<f32>>{anyhow::bail!("Unexpected scalar cache")}
    }
    impl path::ResetHost for Backend {
        fn reservation(&mut self,_:&mut Memory,_:u32,_:u32,_:u32)->Result<u32>{ensure!(self.updating,"Fresh path unexpectedly owns a reservation");Ok(self.reservation.as_ref().map_or(0,|r|r.handle as u32))}
        fn release(&mut self,_:&mut Memory,r:u32)->Result<()>{ensure!(self.reservation.as_ref().is_some_and(|i|i.handle==r as usize),"Unexpected reservation release");Ok(())}
    }
    impl path::DestructionHost for Backend {
        fn free_global(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected free")}
        fn free_cpp(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected C++ free")}
    }
    impl Backend {
        fn allocate(&mut self,m:&mut Memory,n:u32)->Result<u32>{let p=self.next;self.next+=0x1000;m.regions.insert(p,vec![0xcd;n as usize]);self.allocations.push((p,n));Ok(p)}
    }
    impl Services for Backend {
        fn allocate_path_object(&mut self,m:&mut Memory,n:u32)->Result<u32>{self.allocate(m,n)}
        fn allocate_path_buffer(&mut self,m:&mut Memory,_:u32,n:u32)->Result<u32>{self.allocate(m,n)}
    }
    impl super::super::ai_agent_path_runtime::Host for Backend {
        fn reservation_word(&self,_:&Memory,r:u32,o:u32)->Result<u32>{
            let i=self.reservation.as_ref().context("Missing typed reservation")?;ensure!(i.handle==r as usize,"Reservation identity differs");
            let g=(i.concrete.as_deref().context("Missing concrete traversal")?).astar();
            match o{8=>Ok(g.reference_8.unwrap_or(0) as u32),0x28=>Ok(i.lease.as_ref().context("Missing lease")?.reuse_count as u32),0x6c=>Ok(g.reference_6c.unwrap_or(0) as u32),_=>anyhow::bail!("Unexpected reservation word")}
        }
        fn reservation_byte(&self,_:&Memory,r:u32,o:u32)->Result<u8>{
            let i=self.reservation.as_ref().context("Missing typed reservation")?;ensure!(i.handle==r as usize,"Reservation identity differs");
            let g=(i.concrete.as_deref().context("Missing concrete traversal")?).astar();
            match o{0x14=>Ok(u8::from(g.flag_14)),0x15=>Ok(u8::from(g.flag_15)),_=>anyhow::bail!("Unexpected reservation byte")}
        }
        fn owner_position(&mut self,_:&mut Memory,_:u32)->Result<[f32;3]>{anyhow::bail!("Must read canonical entity")}
        fn path_virtual(&mut self,_:&mut Memory,_:u32,s:u32,_:u32)->Result<u32>{ensure!(s==0x18,"Unexpected path virtual");Ok(1)}
        fn refresh(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected refresh")}
        fn accepts_point(&mut self,_:&mut Memory,_:u32,_:u32)->Result<u32>{Ok(1)}
        fn result(&mut self,_:&mut Memory,_:u32,_:u32)->Result<(u32,u32,u32)>{Ok((3,19,1))}
        fn sample(&mut self,m:&mut Memory,_:u32,p:u32,_:u32)->Result<()>{for(o,v)in[(0,1),(4,4f32.to_bits()),(8,5f32.to_bits()),(12,6f32.to_bits()),(16,0)]{m.write(p+o,v)?;}Ok(())}
        fn acknowledge(&mut self,_:&mut Memory,_:u32)->Result<()>{Ok(())}
    }
    impl super::super::ai_agent_path_request::Host for Backend {
        fn request_manager(&mut self,_:&mut Memory)->Result<u32>{Ok(80)}
        fn request_task(&mut self,_:&mut Memory,r:u32)->Result<u32>{let i=self.reservation.as_ref().context("Missing typed reservation")?;ensure!(i.handle==r as usize,"Reservation identity differs");let g=(i.concrete.as_deref().unwrap()).astar();Ok(g.word_c as u32)}
        fn request_enabled(&mut self,_:&mut Memory,a:u32,t:u32)->Result<u32>{assert_eq!(a,80);assert_eq!(t,u32::MAX);Ok(1)}
        fn service_provider(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Must use canonical provider lookup")}
        fn provider_ready(&mut self,_:&mut Memory,p:u32)->Result<u32>{assert_eq!(p,81);Ok(1)}
        fn provider_configuration(&mut self,_:&mut Memory,p:u32)->Result<u32>{assert_eq!(p,81);Ok(82)}
        fn configuration_position(&mut self,_:&mut Memory,p:u32)->Result<[f32;3]>{assert_eq!(p,82);Ok([1.,2.,3.])}
        fn reservation_extent(&mut self,_:&mut Memory,_:u32)->Result<f32>{Ok(2.)}
        fn request_bounds(&mut self,_:&mut Memory,_:u32,l:[f32;3],u:[f32;3],f:[u32;2])->Result<u32>{assert_eq!(l,[1.-2.3,2.-2.3,3.-2.3]);assert_eq!(u,[1.+2.3,2.+2.3,3.+2.3]);assert_eq!(f,[1,0]);Ok(1)}
        fn set_target(&mut self,_:&mut Memory,r:u32,n:u32,p:[f32;3])->Result<()>{let i=self.reservation.as_mut().context("Missing typed reservation")?;ensure!(i.handle==r as usize,"Reservation identity differs");let g=(i.concrete.as_deref_mut().unwrap()).astar_mut();g.set_target(n,p);Ok(())}
    }
    impl super::super::ai_agent_path_initialize::Host for Backend {
        fn random_state(&mut self)->Result<&mut super::super::game_random::State>{Ok(&mut self.random)}
        fn traversal_mut(&mut self,r:u32)->Result<&mut super::super::ai_traversal_instances::Instance>{let i=self.reservation.as_mut().context("Missing typed traversal")?;ensure!(i.handle==r as usize,"Traversal identity differs");Ok(i)}
        fn strategy_mut(&mut self,s:usize)->Result<&mut super::super::ai_flat_handle_strategy::State>{assert_eq!(s,83);Ok(&mut self.strategy)}
        fn initial_position(&mut self,_:&mut Memory,_:u32)->Result<[f32;3]>{anyhow::bail!("Must use canonical initial position")}
        fn service_graph(&mut self,_:&mut Memory,_:u32)->Result<Option<usize>>{anyhow::bail!("Must use canonical service graph")}
        fn service_acceptance(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Goto has its own action profile")}
        fn task_enter(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{assert_eq!(a,80);assert_eq!(t,u32::MAX);Ok(())}
        fn task_leave(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{assert_eq!(a,80);assert_eq!(t,u32::MAX);Ok(())}
        fn project_actor(&mut self,_:&mut Memory,p:u32,v:[f32;3],s:u32,f:[u32;4])->Result<super::super::ai_agent_path_initialize::Projection>{assert_eq!(p,81);assert_eq!(v,[0.;3]);assert_eq!(s,30);assert_eq!(f,[1,1,0,0]);Ok(super::super::ai_agent_path_initialize::Projection{status:0,node:55})}
        fn select_provider_node(&mut self,_:&mut Memory,p:u32,n:u32)->Result<()>{assert_eq!(p,81);assert_eq!(n,55);Ok(())}
        fn configuration_node(&mut self,_:&mut Memory,p:u32)->Result<u32>{assert_eq!(p,82);Ok(99)}
        fn project_goal(&mut self,_:&mut Memory,_:u32,p:u32,_:u32,n:u32,g:[f32;3])->Result<super::super::ai_agent_path_initialize::Projection>{assert_eq!(p,81);assert_eq!(n,99);assert_eq!(g,[1.,-2.,3.]);Ok(super::super::ai_agent_path_initialize::Projection{status:1,node:66})}
    }
    impl UpdateServices for Backend {
        fn inline_acceptance(&mut self,s:usize)->Result<u32>{assert_eq!(s,30);Ok(84)}
        fn begin_route(&mut self,_:&mut Memory,s:&mut pf::State)->Result<()>{assert_eq!(s.identity,30);s.word_64=9;self.route.push(0);Ok(())}
        fn append_route(&mut self,_:&mut Memory,s:&mut pf::State,p:u32)->Result<()>{assert_eq!(s.word_64,9);self.route.push(p);Ok(())}
        fn finish_route(&mut self,_:&mut Memory,s:&mut pf::State,f:u32)->Result<()>{assert_eq!(s.word_64,9);self.route.push(f);Ok(())}
    }
    fn entity_registry()->Registry {
        use ai_navigation_entity::{Class,Kind};
        Registry{count:3,diameter:Class{index:0,constructor:Some(Kind::Diameter)},height:Class{index:1,constructor:Some(Kind::Height)},maximum_speed:Class{index:2,constructor:Some(Kind::MaximumSpeed)}}
    }
    #[test]
    fn canonical_goto_wander_bind_constructs_actual_paths_and_reads_selected_modifier()->Result<()> {
        for class in [Class::Goto,Class::Wander] {for mode in 0..4 {
            let mut store=Storage::default();let mut entity=super::super::ai_navigation_world_owned::tests::constructed_actor(50)?;entity.entity.common.brain=Some(7);store.entities.insert(50,entity);
            let metadata=ActionClass{metadata:40,factory:Some(ActionFactory::BaseAi)};
            let layout=Layout{agent_classes:3,service_classes:1,attributes:action::Registry{count:2,heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}}};
            let mut parent=Constructed::new(7,Some(50),Some(20),&metadata,&layout)?;
            let mut counter=pf::Counter::default();let mut service=pf::State::construct(30,&parent,31,&layout.attributes,&mut counter,&mut Constructor)?;
            // Service publication and initialized modifier state are supplied
            // boundaries. Construction alone is not claimed as48fec4 success.
            service.words_90_94_98[1]=24;service.word_70=81;service.tables_fc_1fc[2].selected=Some(Some(88));
            if mode==0{service.base.references_c_10[0]=Some(Some(pf::Reference::InlineAcceptance));} //48fec4 publication is supplied here, not inferred from construction.
            if mode!=1{parent.base.services.push(30);}
            store.services.insert(30,Service::Pathfinder(Box::new(service)));
            let mut detector=ai_goal_detection::State::construct(ai_goal_detection::Class::Changed3D,88,30,b"GoalChange");
            detector.fields=Fields::Changed3D{distance:if mode==3{None}else{Some(5.)},last_time:Some(0.)};
            let modifiers=BTreeMap::from([(88,Modifier::GoalDetection(Box::new(detector)))]);
            let mut shared=super::super::ai_goto_configuration::Shared::default();let mut record=Record::construct(8,Some(7),class,&mut shared,&mut Constructor)?;
            match &mut record.object {Agent::Goto(a)=>a.path.request_state=99,Agent::Wander(a)=>a.request_state=99,_=>unreachable!()}
            store.agents.insert(8,record);store.brains.insert(7,parent);
            let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
            tree.append(Some(root),Node{name:Some(b"Class".to_vec()),value:Some(if class==Class::Goto{b"JPSGotoAgent".to_vec()}else{b"JPSWanderAgent".to_vec()}),..Default::default()})?;
            if class==Class::Goto{tree.append(Some(root),Node{name:Some(b"Speed".to_vec()),value:Some(b"2".to_vec()),..Default::default()})?;}
            let mut memory=Memory::default();memory.regions.insert(0x6729c,vec![0;16]);memory.write(0x672a4,3)?;
            memory.regions.insert(0x564d0,[1f32,-2.,3.].into_iter().flat_map(|v|v.to_bits().to_be_bytes()).collect());
            let mut backend=Backend{next:0x60000000,traversal_failed:mode==2,..Default::default()};let registry=entity_registry();
            // Use real TimeMgt startup to register this supplied test profile.
            let mut task_tree=Tree::default();let world_root=task_tree.append(None,Node::default())?;let task_root=task_tree.append(Some(world_root),Node{name:Some(b"TimeMgt".to_vec()),..Default::default()})?;
            let mut task_node=Node{name:Some(b"Aperiodic".to_vec()),..Default::default()};task_node.set_identifier(Some(b"Traversal"));
            let task=task_tree.append(Some(task_root),task_node)?;
            task_tree.append(Some(task),Node{name:Some(b"Tpf".to_vec()),value:Some(b"100000".to_vec()),..Default::default()})?;
            task_tree.append(Some(task),Node{name:Some(b"MaxCall".to_vec()),value:Some(b"100".to_vec()),..Default::default()})?;
            let mut tasks=super::super::ai_task_context_world::Store{definitions:task_tree,current_world:Some(70),..Default::default()};
            tasks.worlds.insert(70,super::super::ai_task_context_world::World{identity:70,definition_root:world_root,actor_count:3,other_count:0,task_manager:None});
            tasks.globals.getter=super::super::ai_task_clock::Getter::External(90);tasks.globals.reset=Some(91);
            assert!(tasks.install(70,80,&mut backend)?);tasks.manager_mut(80)?.tasks.limit=100.;
            assert_eq!(tasks.manager(80)?.tasks.find(Some(b"Traversal"))?,0);backend.task_world=tasks;
            let (entity,parent,mut children)=store.child_initialization_parts(7)?;
            let result=bind_owned(parent.into(),&mut children,entity,&registry,&mut tree,8,root,&metadata,&layout.attributes,&modifiers,&mut memory,path::Context{point_attribute_class:0x6729c,global_vector:0x564d0},&mut backend);
            let record=&store.agents[&8];let a=match &record.object{Agent::Goto(a)=>&a.path,Agent::Wander(a)=>a.as_ref(),_=>unreachable!()};
            if mode==1{assert!(!result?);assert!(backend.allocations.is_empty());assert_eq!(a.base.action_04,None);continue;}
            if mode==2 || (mode==3&&class==Class::Goto){assert!(result.is_err());assert_eq!(a.pathfinder,None);assert!(a.base.action_04.is_some());continue;}
            assert!(result?);assert_eq!(a.base.blackboard_head,4);assert_eq!(a.base.speed,3.);assert_eq!(a.request_state,-1);
            let p=a.pathfinder.context("Created path missing")? as u32;
            assert_eq!(memory.word(p+0x7c)?,50,"Path owner was confused with gameplay actor handle4");
            assert_eq!(memory.word(p+12)?,30);assert_eq!(memory.word(p+4)?,60);assert_eq!(memory.word(p)?,5);
            assert_eq!(store.agent_actions.objects.len(),1);assert!(store.brains[&7].base.agents.is_empty(),"Bind itself published the agent");
            if let Agent::Goto(g)=&record.object {
                assert_eq!(g.vector_90,[1.,-2.,3.]);assert_eq!(g.flag_a8,Some(false));assert_eq!(g.reference,None);
                assert_eq!(memory.word(p+0x8a8)?,25f32.to_bits(),"Goto limit must use selected goal detector, not shape radius");
                let buffer=memory.word(p+0x880)?;assert_eq!(memory.word(buffer)?,24);
                assert_eq!(backend.allocations.iter().map(|(_,n)|*n).collect::<Vec<_>>(),[0x8b4,24,24]);
                assert_ne!(memory.word(p+0x8ac)?,0);
            } else {assert_eq!(backend.allocations.iter().map(|(_,n)|*n).collect::<Vec<_>>(),[0x878]);}
            // Drive this same bound agent without a mapped image of the
            // typed reservation. Position likewise comes from owned entity50.
            if class==Class::Goto&&mode==0 {
                let reach=Services::allocate_path_object(&mut backend,&mut memory,0x8b4)?;
                path::construct(&mut memory,path::Context{point_attribute_class:0x6729c,global_vector:0x564d0},reach,path::Kind::Goto)?;
                memory.write_byte(reach+0x8b0,1)?;assert_ne!(reach,p);
                let (_,parent,mut children)=store.child_initialization_parts(7)?;
                parent.brain.traversal_74=Some(reach as usize);
                assert!(bind_brain_path(parent.into(),&mut children,&metadata,&layout.attributes,&modifiers,&mut memory,reach,b"CanReachTraversal",&mut backend)?);
                assert_eq!(memory.word(reach+0x7c)?,50);assert_eq!(memory.word(reach+12)?,30);assert_eq!(memory.word(reach)?,5);
                assert_eq!(memory.word(reach+0x8a8)?,25f32.to_bits());assert_eq!(memory.byte(reach+0x8b0)?,1);
                assert_ne!(memory.word(reach+0x8ac)?,0);let buffer=memory.word(reach+0x880)?;assert_eq!(memory.word(buffer)?,24);
            }
            // Begin the actual manager frame before admitting path work.
            // Exhaustion and stale task totals would otherwise reject requests.
            {
                let tasks=&mut backend.task_world.manager_mut(80)?.tasks;tasks.exhausted=254;
                tasks.entries[0].calls=99;tasks.entries[0].elapsed=99.;
                backend.clock=99;backend.task_world.globals.frame.elapsed=44.;
                backend.task_world.manager_mut(80)?.frame=super::super::ai_task_frame::Frame{
                    previous:10.,current:11.,delta:12.,unaccounted:13.,work:9,selected:0,
                    actors:[1.,3.,2.].into_iter().map(|elapsed|super::super::ai_task_frame::Actor{elapsed,weight:1.}).collect(),order:vec![2,0,1],
                };
                backend.task_world.begin_frame(0.,&mut backend.clock)?;
                let tasks=&backend.task_world.manager(80)?.tasks;assert_eq!(tasks.exhausted,0);
                assert_eq!((tasks.entries[0].calls,tasks.entries[0].elapsed),(0,0.));
                let frame=&backend.task_world.manager(80)?.frame;assert_eq!(frame.order,[0,1,2]);
                assert!(frame.actors.iter().all(|a|a.elapsed==0.));assert_eq!((frame.selected,frame.work),(-1,0));
                assert_eq!(frame.previous,11.);assert_eq!(frame.delta.to_bits(),0x3a83126f);
                assert_eq!(backend.task_world.globals.frame.elapsed,0.);assert_eq!(backend.clock,0);
            }
            backend.updating=true;
            if class==Class::Wander{memory.write(p+0x874,2.5f32.to_bits())?;}
            for ready in [false,true] {
                if ready {
                    let mut reservation=if class==Class::Goto{super::super::ai_goto_traversal::construct(0x76000000)}else{super::super::ai_wander_traversal::construct(0x76000000)};
                    let g=(reservation.concrete.as_deref_mut().unwrap()).astar_mut();g.reference_8=None;g.strategy=Some(83);g.word_c=0;
                    backend.reservation=Some(reservation);
                    memory.regions.insert(771,vec![0;8]);
                }
                let (entity,parent,mut children)=store.child_initialization_parts(7)?;
                let value=update_owned(parent.into(),&mut children,entity,&registry,&mut tree,8,&metadata,&layout.attributes,&modifiers,&mut memory,path::Context{point_attribute_class:0x6729c,global_vector:0x564d0},&mut backend)?;
                assert_eq!(value,1);
                assert!(!memory.regions.contains_key(&0x76000000),"Traversal was mirrored into the arena");
                if ready {
                    assert_eq!(backend.route,[0,p+0x84,p+0x98,1]);assert_eq!(memory.word(775)?,1);
                    let Service::Pathfinder(service)=&store.services[&30];assert_eq!(service.word_64,9);
                    let task=&backend.task_world.manager(80)?.tasks.entries[0];assert_eq!(task.calls,if class==Class::Goto{2}else{1});assert_eq!(task.elapsed,if class==Class::Goto{2.}else{1.});assert_eq!(backend.clock,if class==Class::Goto{7}else{4});
                    if class==Class::Goto {
                        let reservation=backend.reservation.as_ref().unwrap();let g=(reservation.concrete.as_deref().unwrap()).astar();
                        assert_eq!(g.node_70,66);assert_eq!(g.position_74,[1.,-2.,3.]);assert_eq!(g.word_40,55);assert_eq!(g.vector_44,[0.;3]);assert_eq!(backend.strategy.owner,Some(30));assert_eq!(g.references_38_3c[1],Some((p+0x884) as usize));assert_eq!(backend.random.draws,0);
                    }else{
                        let reservation=backend.reservation.as_mut().unwrap();let w=reservation.concrete.as_deref_mut().unwrap().wander_mut()?;
                        assert_eq!(w.scalar_a4,2.5);assert_eq!(w.base.references_38_3c[0],if mode==0{Some(84)}else{None});assert_eq!(w.base.references_38_3c[1],Some((p+0x86c) as usize));assert_eq!(backend.random.draws,1);
                    }

                } else {assert!(backend.route.is_empty());}
            }
        }}Ok(())
    }
}
