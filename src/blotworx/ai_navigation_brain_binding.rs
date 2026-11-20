//!Combined18d69c over the canonical459080 child initializer and owned records.
//!Observer/motion records and the shared context cache are owned; derived
//!allocation, context and region service operations require runtime services; a successful profile lookup never substitutes for these operations.
use anyhow::{Context,Result,ensure};
use std::collections::BTreeMap;
use super::{ai_navigation_brain::{self as brain,Brain,Observer,AgentClass,WorldService},
    ai_navigation_brain_base::State,ai_navigation_action::Action,ai_navigation_construction::Constructed,
    ai_navigation_children_init::{self as children,Resources},ai_navigation_owned_factory::Children,
    ai_navigation_definitions::Tree,ai_graph_link_passes::Memory,
    ai_navigation_service_factory::Owned as Service,ai_pathfinder_modifier_factory::Owned as Modifier,
    ai_pathfinder_modifiers::Kind,ai_pathfinder_concrete_modifiers::Metadata};
pub const OUTPUT_CALLBACK:usize=0x18d620;
pub trait Backend:children::Backend+super::ai_navigation_brain_resources::Allocator+super::ai_navigation_region_world::Runtime {
    fn brain_region_service(&mut self,brain:&mut Brain)->Result<Option<usize>>;
}
///No copied pathfinder, modifier or LPF state is used by these source accesses.
struct Canonical<'a>{parent:usize,services:&'a mut BTreeMap<usize,Service>,objects:&'a mut BTreeMap<usize,Modifier>,metadata:&'a Metadata,memory:&'a mut Memory}
impl Canonical<'_>{
    fn service(&self,id:usize)->Result<&super::ai_pathfinder_constructor::State>{
        let Service::Pathfinder(s)=self.services.get(&id).context("Brain pathfinder is not owned")?;
        ensure!(s.identity==id&&s.base.owner==self.parent,"Brain pathfinder identity or owner differs");Ok(s)
    }
    fn selected(&self,service:usize,kind:Kind)->Result<Option<usize>>{self.service(service)?.tables_fc_1fc[kind.table()].selected.context("Brain modifier selection is uninitialized")}
    fn object(&self,id:usize,kind:Kind)->Result<&Modifier>{
        let o=self.objects.get(&id).context("Brain modifier is not owned")?;
        ensure!(o.identity()==id&&o.kind()==kind,"Brain modifier identity or interface differs");self.service(o.pathfinder(self.memory)?)?;Ok(o)
    }
    fn gap_callback(&mut self,id:usize)->Result<()>{
        self.object(id,Kind(8))?;
        let Modifier::GapAvoidance(s)=self.objects.get_mut(&id).unwrap() else{anyhow::bail!("Brain IGoto callback layout is unsupported")};
        s.reference_54=Some(Some(OUTPUT_CALLBACK));Ok(())
    }
    fn queue_class(&self)->Result<usize>{
        let mut classes=self.metadata.classes.iter().filter(|c|c.kind==Kind(7)&&c.name==super::ai_compute_target::State::CLASS);
        let class=classes.next().context("Brain Queue output class is not registered")?;
        ensure!(classes.next().is_none(),"Brain Queue output class registration is ambiguous");Ok(class.identity)
    }
    fn output_class(&self,id:usize)->Result<usize>{
        ensure!(matches!(self.object(id,Kind(7))?,Modifier::ComputeTarget(_)),"Brain output class getter is unsupported");self.queue_class()
    }
    fn queue_callback(&mut self,id:usize)->Result<()>{
        self.output_class(id)?;
        let Modifier::ComputeTarget(s)=self.objects.get_mut(&id).unwrap() else{unreachable!()};s.set_predicate(Some(OUTPUT_CALLBACK));Ok(())
    }
    fn context(&self,service:usize)->Result<usize>{
        let s=self.service(service)?;let id=s.reference_f8.context("Brain LPF reference is uninitialized")?.context("Brain LPF context is null")?;
        let lpf=s.lpf.as_ref().context("Brain LPF context is not owned")?;
        ensure!(lpf.identity==id&&lpf.pathfinder==Some(service),"Brain LPF identity or parent differs");Ok(id)
    }
    fn context_mut(&mut self,id:usize)->Result<&mut super::ai_lpf_module::State>{
        let ids=self.services.iter().filter_map(|(&service,s)|{let Service::Pathfinder(s)=s;(s.base.owner==self.parent&&s.lpf.as_ref().is_some_and(|l|l.identity==id)).then_some(service)}).collect::<Vec<_>>();
        ensure!(ids.len()==1,"Brain LPF context ownership is absent or ambiguous");ensure!(self.context(ids[0])?==id,"Brain LPF reference differs");
        let Service::Pathfinder(s)=self.services.get_mut(&ids[0]).unwrap();Ok(s.lpf.as_mut().unwrap())
    }
}
pub fn initialize(world:&super::assets::World,entities:&BTreeMap<usize,super::ai_navigation_entity::Owned>,owner:&mut Constructed,children:&mut Children<'_>,tree:&mut Tree,root:Option<usize>,resources:Resources<'_>,records:&mut super::ai_navigation_brain_resources::Store,regions:&mut super::ai_navigation_region_cache::Cache,backend:&mut impl Backend)->Result<bool>{
    let Constructed{brain,base,action}=owner;
    brain.initialize(root,&mut Binding{world,entities,base,action,children,tree,resources,records,regions,backend})
}
struct Binding<'a,'c,H>{world:&'a super::assets::World,entities:&'a BTreeMap<usize,super::ai_navigation_entity::Owned>,base:&'a mut State,action:&'a mut Option<Action>,children:&'a mut Children<'c>,tree:&'a mut Tree,resources:Resources<'a>,records:&'a mut super::ai_navigation_brain_resources::Store,regions:&'a mut super::ai_navigation_region_cache::Cache,backend:&'a mut H}
impl<H> Binding<'_, '_,H>{
    fn canonical(&mut self,brain:&Brain)->Canonical<'_>{Canonical{parent:brain.identity,services:self.children.services,objects:self.resources.modifiers,metadata:self.resources.modifier_metadata,memory:self.resources.memory}}
}
impl<H> brain::Construction for Binding<'_, '_,H>{
    fn construct_base(&mut self,_:usize,_:Option<usize>,_:usize)->Result<()>{anyhow::bail!("Brain initialization binding cannot reconstruct an owned parent")}
}
impl<H:Backend> brain::Host for Binding<'_, '_,H>{
    fn output_target_class(&mut self)->Result<usize>{
        let mut classes=self.resources.modifier_metadata.classes.iter().filter(|c|c.kind==Kind(7)&&c.name==super::ai_compute_target::State::CLASS);
        let class=classes.next().context("Brain Queue output target is not registered")?;ensure!(classes.next().is_none(),"Ambiguous Queue output target");Ok(class.identity)
    }
    fn initialize_base(&mut self,b:&mut Brain,root:Option<usize>)->Result<bool>{children::initialize_parts(b,self.base,self.action,self.children,self.tree,root,self.resources.reborrow(),self.backend)}
    fn owner_service_6af6c(&mut self,_:&mut Brain)->Result<Option<usize>>{self.children.find_pathfinder(self.base)}
    fn agent(&mut self,_:&mut Brain,class:AgentClass)->Result<Option<usize>>{self.children.find_agent(self.base,class)}
    fn world_service(&mut self,b:&mut Brain,_:WorldService)->Result<Option<usize>>{self.backend.brain_region_service(b)}
    fn construct_observer(&mut self,b:&mut Brain,kind:Observer,name:&str)->Result<usize>{
        let id=self.records.construct_observer(b,self.resources.memory,kind,name.as_bytes(),self.backend)?;
        let service=self.records.observers[&id].service;self.canonical(b).service(service)?;Ok(id)
    }
    fn initialize_observer(&mut self,b:&mut Brain,id:usize)->Result<()>{self.records.initialize_observer(b.identity,id)}
    fn construct_action(&mut self,b:&mut Brain)->Result<Option<usize>>{self.records.construct_motion(b,self.resources.memory,self.backend)}
    fn filter_1ac(&mut self,b:&mut Brain,service:usize)->Result<Option<usize>>{self.canonical(b).selected(service,Kind(8))}
    fn install_filter_callback_18d620(&mut self,b:&mut Brain,id:usize)->Result<()>{self.canonical(b).gap_callback(id)}
    fn construct_traversal_1c1854(&mut self,_:&mut Brain)->Result<usize>{
        let p=self.backend.allocate_path_object(self.resources.memory,0x8b4)?;
        super::ai_agent_path::construct(self.resources.memory,self.resources.path_context,p,super::ai_agent_path::Kind::Goto)?;Ok(p as usize)
    }
    fn set_traversal_flag_8b0(&mut self,_:&mut Brain,id:usize)->Result<()>{self.resources.memory.write_byte(u32::try_from(id)?.wrapping_add(0x8b0),1)}
    fn initialize_traversal(&mut self,b:&mut Brain,id:usize,name:&str)->Result<bool>{
        super::ai_path_agent_binding::bind_brain_path(super::ai_agent_actions::Parent{brain:b,base:self.base,action:self.action},self.children,self.resources.action_class,self.resources.action_registry,self.resources.modifiers,self.resources.memory,u32::try_from(id)?,name.as_bytes(),self.backend)
    }
    fn register_region_186a7c(&mut self,b:&mut Brain,service:usize)->Result<Option<usize>>{self.regions.register(b.identity,service,&mut super::ai_lpf_context::Native{memory:self.resources.memory,backend:&mut super::ai_navigation_region_world::Binding{world:self.world,entities:Some(self.entities),brain:b.identity,base:self.base,entity:self.resources.entity,backend:self.backend}})}
    fn route_context_f8(&mut self,b:&mut Brain,service:usize)->Result<usize>{self.canonical(b).context(service)}
    fn set_route_owner(&mut self,b:&mut Brain,id:usize,owner:usize)->Result<()>{self.canonical(b).context_mut(id)?.context=Some(owner);Ok(())}
    fn refresh_route(&mut self,b:&mut Brain,id:usize)->Result<()>{self.canonical(b).context_mut(id)?.reset()}
    fn output_198(&mut self,b:&mut Brain,service:usize)->Result<usize>{self.canonical(b).selected(service,Kind(7))?.context("Brain output modifier is null")}
    fn output_class(&mut self,b:&mut Brain,id:usize)->Result<usize>{self.canonical(b).output_class(id)}
    fn parent_class(&mut self,id:usize)->Result<Option<usize>>{Ok(self.resources.modifier_metadata.classes.iter().find(|c|c.identity==id).context("Brain output class ancestry is unregistered")?.parent)}
    fn install_output_callback_18d620(&mut self,b:&mut Brain,id:usize)->Result<()>{self.canonical(b).queue_callback(id)}
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json,Value};
    struct Construction;
    impl super::super::ai_goto_gap::Host for Construction{
        fn unit_scale(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<usize>{Ok(1)}
        fn profiling_enabled(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<bool>{Ok(false)}
        fn register_budget(&mut self,_:&mut super::super::ai_goto_gap::State,_:&str)->Result<i32>{anyhow::bail!("No profiling clock installed in constructor fixture")}
    }
    impl super::super::ai_lpf_module::Host for Construction{
        fn unit_scale(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<f32>>{Ok(None)}
        fn manager(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<usize>>{unreachable!()}
        fn child_count(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<u32>{unreachable!()}
        fn child(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize,_:u32)->Result<Option<usize>>{unreachable!()}
        fn name(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<Option<Vec<u8>>>{unreachable!()}
        fn value(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<Option<usize>>{unreachable!()}
        fn integer(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<i32>{unreachable!()}
        fn number(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<f64>{unreachable!()}
        fn context(&mut self,_:&mut super::super::ai_lpf_module::State,_:Option<usize>,_:usize)->Result<Option<usize>>{unreachable!()}
    }
    #[test]
    fn selected_owned_modifiers_and_lpf_match_original_brain_setup()->Result<()>{
        let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_brain_binding.json"))?;
        for row in capture["cases"].as_array().unwrap(){
            let c=&row["input"];let p=row["service"].as_u64().unwrap() as usize;
            let mut service=super::super::ai_pathfinder_modifiers::tests::state()?;service.identity=p;service.base.owner=0x72000000;
            service.tables_fc_1fc[8].selected=Some(c["filter"].as_bool().unwrap().then_some(0x74000000));service.tables_fc_1fc[7].selected=Some(Some(0x75000000));
            service.reference_f8=Some(Some(0x76000000));service.lpf=Some(Box::new(super::super::ai_lpf_module::State::construct(0x76000000,p,&mut Construction)?));
            let gap=super::super::ai_goto_gap::State::construct(0x74000000,p,b"goto",&mut Construction)?;
            let queue=super::super::ai_compute_target::State::construct(0x75000000,p,b"queue");
            let mut services=BTreeMap::from([(p,Service::Pathfinder(Box::new(service)))]);
            let mut objects=BTreeMap::from([(0x74000000,Modifier::GapAvoidance(Box::new(gap))),(0x75000000,Modifier::ComputeTarget(Box::new(queue)))]);
            let metadata=Metadata{classes:vec![super::super::ai_pathfinder_concrete_modifiers::Class{identity:0x118b4,kind:Kind(7),name:super::super::ai_compute_target::State::CLASS.to_vec(),parent:None,vtable:None}],defaults:[None;13]};
            let mut memory=Memory::default();
            {
                let mut owned=Canonical{parent:0x72000000,services:&mut services,objects:&mut objects,metadata:&metadata,memory:&mut memory};
                if let Some(id)=owned.selected(p,Kind(8))?{owned.gap_callback(id)?;}
                if c["region"].as_bool().unwrap(){let id=owned.context(p)?;owned.context_mut(id)?.context=Some(0x77000000);owned.context_mut(id)?.reset()?;}
                let id=owned.selected(p,Kind(7))?.context("Source output absent")?;assert_eq!(owned.output_class(id)?,owned.queue_class()?);owned.queue_callback(id)?;
            }
            let Modifier::GapAvoidance(gap)=&objects[&0x74000000] else{unreachable!()};let Modifier::ComputeTarget(queue)=&objects[&0x75000000] else{unreachable!()};
            assert_eq!(json!(gap.reference_54.flatten().unwrap_or(0)),row["gap_callback"],"{c}");
            assert_eq!(json!(queue.predicate.unwrap_or(0)),row["queue_callback"],"{c}");
            let Service::Pathfinder(service)=&services[&p];let lpf=service.lpf.as_ref().unwrap();
            assert_eq!(json!(lpf.context.unwrap_or(0)),row["lpf_context"],"{c}");assert_eq!(json!(lpf.position.map(f32::to_bits)),row["lpf_position"],"{c}");
            //Reject dangling ownership instead of consulting an arena copy.
            let mut owned=Canonical{parent:1,services:&mut services,objects:&mut objects,metadata:&metadata,memory:&mut memory};assert!(owned.selected(p,Kind(7)).is_err());
        }
        Ok(())
    }
}

///Shared manager resources survive individual temporary actor definitions.
///The parent entity and class registries are supplied from the actual factory.
pub struct Shared<'a>{
    pub records:&'a mut super::ai_navigation_brain_resources::Store,
    pub regions:&'a mut super::ai_navigation_region_cache::Cache,
    pub service_class:&'a children::ServiceClass,pub agent_classes:&'a [children::AgentClass],
    pub goto_shared:&'a mut super::ai_goto_configuration::Shared,pub counter:&'a mut super::ai_pathfinder_constructor::Counter,
    pub shared:&'a mut super::ai_pathfinder_initialization::Shared,pub memory:&'a mut Memory,
    pub modifier_metadata:&'a Metadata,pub modifiers:&'a mut BTreeMap<usize,Modifier>,
    pub path_context:super::ai_agent_path::Context,pub spatial:super::ai_spatial_construction::Context,
}
///Concrete owned-factory initializer. Lifecycle operations cannot replace the
///derived brain initializer; all service/agent selection uses owned records.
pub struct FactoryBackend<'a,H>{pub world:&'a super::assets::World,pub metadata:&'a super::ai_navigation_owned_factory::Metadata,pub resources:Shared<'a>,pub backend:&'a mut H}
impl<H:super::ai_navigation_owned_factory::Lifecycle> super::ai_navigation_definitions::ProfileServices for FactoryBackend<'_,H>{
    fn manager(&mut self,e:&mut super::ai_navigation_entity_base::Entity)->Result<Option<usize>>{super::ai_navigation_definitions::ProfileServices::manager(self.backend,e)}
    fn find(&mut self,e:&mut super::ai_navigation_entity_base::Entity,m:usize,n:&[u8])->Result<Option<usize>>{super::ai_navigation_definitions::ProfileServices::find(self.backend,e,m,n)}
    fn attach(&mut self,e:&mut super::ai_navigation_entity_base::Entity,p:usize)->Result<bool>{super::ai_navigation_definitions::ProfileServices::attach(self.backend,e,p)}
    fn detach(&mut self,e:&mut super::ai_navigation_entity_base::Entity,p:usize)->Result<()>{super::ai_navigation_definitions::ProfileServices::detach(self.backend,e,p)}
}
impl<H:super::ai_navigation_owned_factory::Lifecycle+super::ai_navigation_region_world::Runtime> super::ai_navigation_owned_factory::Lifecycle for FactoryBackend<'_,H>{
    fn enabled(&mut self)->Result<bool>{super::ai_navigation_owned_factory::Lifecycle::enabled(self.backend)}
    fn world(&mut self)->Result<Option<usize>>{super::ai_navigation_owned_factory::Lifecycle::world(self.backend)}
    fn factory_pool(&mut self)->Result<(&mut Memory,u32)>{super::ai_navigation_owned_factory::Lifecycle::factory_pool(self.backend)}
    fn unit_scale(&mut self)->Result<Option<f32>>{super::ai_navigation_owned_factory::Lifecycle::unit_scale(self.backend)}
    fn destroy_brain(&mut self,e:&super::ai_navigation_entity::Owned,b:&mut Constructed,c:&mut Children<'_>)->Result<()>{self.resources.regions.release(b.brain.identity,&mut super::ai_navigation_region_world::Binding{world:self.world,entities:None,brain:b.brain.identity,base:&b.base,entity:e,backend:self.backend})?;super::ai_navigation_owned_factory::Lifecycle::destroy_brain(self.backend,e,b,c)}
    fn destroy_entity(&mut self,e:&mut super::ai_navigation_entity::Owned)->Result<()>{super::ai_navigation_owned_factory::Lifecycle::destroy_entity(self.backend,e)}
}
impl<H:Backend+super::ai_navigation_owned_factory::Lifecycle> super::ai_navigation_owned_factory::Backend for FactoryBackend<'_,H>{
    fn initialize_brain(&mut self,world:&super::assets::World,tree:&mut Tree,definition:usize,entities:&BTreeMap<usize,super::ai_navigation_entity::Owned>,entity:usize,brain:&mut Constructed,children:&mut Children<'_>)->Result<bool>{
        ensure!(std::ptr::eq(world,self.world),"Brain initializer and destructor must share the same gameplay World");
        let entity=entities.get(&entity).context("Initialized brain entity absent")?;
        let r=&mut self.resources;
        let resources=Resources{service_class:r.service_class,agent_classes:r.agent_classes,goto_shared:&mut *r.goto_shared,
            entity,entity_registry:&self.metadata.entity_attributes,action_class:&self.metadata.action,action_registry:&self.metadata.brain_layout.attributes,
            counter:&mut *r.counter,shared:&mut *r.shared,memory:&mut *r.memory,modifier_metadata:r.modifier_metadata,modifiers:&mut *r.modifiers,path_context:r.path_context,spatial:r.spatial};
        initialize(world,entities,brain,children,tree,Some(definition),resources,&mut *r.records,&mut *r.regions,self.backend)
    }
}
