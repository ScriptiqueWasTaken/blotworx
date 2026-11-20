//! Concrete entity/brain ownership for4577b0. Service initialization and manager
//! admission are required backend operations, never inferred from construction.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{assets::World,ai_navigation_factory::{self,Host,Kind,Record},
    ai_navigation_entity::{Owned,Registry},ai_navigation_construction::{Constructed,Layout,ActionClass},
    ai_navigation_definitions::{Tree,ProfileServices}};
pub struct Metadata {
    pub entity_class:usize,
    pub brain_class:usize,
    pub action_class:usize,
    pub entity_attributes:Registry,
    pub brain_layout:Layout,
    pub action:ActionClass,
}
pub trait Lifecycle:ProfileServices {
    fn enabled(&mut self)->Result<bool>;
    fn world(&mut self)->Result<Option<usize>>;
    /// Canonical global4e1a8 slot and its owned pool memory. Factory membership
    /// is implemented here, independently of navigation World admission.
    fn factory_pool(&mut self)->Result<(&mut super::ai_graph_link_passes::Memory,u32)>;
    fn unit_scale(&mut self)->Result<Option<f32>>;
    fn destroy_brain(&mut self,entity:&Owned,brain:&mut Constructed,children:&mut Children<'_>)->Result<()>;
    fn destroy_entity(&mut self,entity:&mut Owned)->Result<()>;
}
pub trait Backend:Lifecycle {
    fn initialize_brain(&mut self,world:&World,tree:&mut Tree,definition:usize,entities:&BTreeMap<usize,Owned>,entity:usize,brain:&mut Constructed,children:&mut Children<'_>)->Result<bool>;
}
#[derive(Default)]
pub struct Storage {
    pub entities:BTreeMap<usize,Owned>,
    pub brains:BTreeMap<usize,Constructed>,
    /// Canonical service objects referenced by Constructed.base.services.
    pub services:BTreeMap<usize,super::ai_navigation_service_factory::Owned>,
    pub agents:BTreeMap<usize,super::ai_navigation_agent_factory::Record>,
    pub agent_actions:super::ai_agent_actions::Store,
    next_identity:usize,
}
/// Disjoint child access while parent records stay in their canonical maps.
pub struct Children<'a> {
    pub services:&'a mut BTreeMap<usize,super::ai_navigation_service_factory::Owned>,
    pub agents:&'a mut BTreeMap<usize,super::ai_navigation_agent_factory::Record>,
    pub agent_actions:&'a mut super::ai_agent_actions::Store,
    next_identity:&'a mut usize,
}
impl Children<'_> {
    /// Disjoint ownership allows a derived agent callback to access its real
    /// brain action without removing the live agent from the canonical map.
    pub fn agent_action_parts<'b>(&'b mut self,parent:super::ai_agent_actions::Parent<'b>,
        class:&'b ActionClass,registry:&'b super::ai_navigation_action::Registry)
        ->(&'b mut BTreeMap<usize,super::ai_navigation_agent_factory::Record>,
           &'b mut BTreeMap<usize,super::ai_navigation_service_factory::Owned>,super::ai_agent_actions::View<'b>) {
        (self.agents,self.services,super::ai_agent_actions::View{parent,private:self.agent_actions,class,registry,next_identity:self.next_identity})
    }
    pub fn find_agent(&self,base:&super::ai_navigation_brain_base::State,class:super::ai_navigation_agent_factory::Class)->Result<Option<usize>>{
        super::ai_navigation_agent_factory::find(base,self.agents,class)
    }
    ///4595a0 for the supported final CPathFinder class. Consult only the
    ///published ordered list; constructed but unbound objects are not visible.
    pub fn find_pathfinder(&self,base:&super::ai_navigation_brain_base::State)->Result<Option<usize>>{
        find_pathfinder(base,self.services)
    }
    pub fn construct_agent<H:super::ai_navigation_agent_factory::ConstructionHost>(
        &mut self,owner:&super::ai_navigation_brain::Brain,class:super::ai_navigation_agent_factory::Class,
        shared:&mut super::ai_goto_configuration::Shared,host:&mut H)->Result<usize>{
        let id=self.identity()?;
        ensure!(!self.agents.contains_key(&id),"Navigation agent identity already owned");
        let record=super::ai_navigation_agent_factory::Record::construct(id,Some(owner.identity),class,shared,host)?;
        self.agents.insert(id,record);Ok(id)
    }
    pub fn identity(&mut self)->Result<usize>{
        let id=self.next_identity.checked_add(1).context("Navigation identities exhausted")?;
        *self.next_identity=id;Ok(id)
    }
    pub fn construct_pathfinder(&mut self,owner:super::ai_pathfinder_constructor::Owner<'_>,
        registry:&super::ai_navigation_action::Registry,
        counter:&mut super::ai_pathfinder_constructor::Counter,
        host:&mut impl super::ai_pathfinder_constructor::Host)->Result<usize> {
        let id=self.identity()?;let action=self.identity()?;
        ensure!(!self.services.contains_key(&id),"Navigation service identity already owned");
        let state=super::ai_pathfinder_constructor::State::construct_for(id,owner,action,registry,counter,host)?;
        self.services.insert(id,super::ai_navigation_service_factory::Owned::Pathfinder(Box::new(state)));
        Ok(id)
    }
    /// Construction does not publish the service as initialized. The derived
    /// initializer must succeed before the base initializer appends its handle.
    pub fn construct_service(&mut self,tree:&Tree,node:usize,owner:&Constructed,
        registry:&super::ai_navigation_action::Registry,
        counter:&mut super::ai_pathfinder_constructor::Counter,
        host:&mut impl super::ai_pathfinder_constructor::Host)->Result<Option<usize>> {
        if tree.first_value(node,b"Class")?.is_none(){return Ok(None)}
        let id=self.identity()?;let action=self.identity()?;
        let Some(service)=super::ai_navigation_service_factory::construct(tree,node,id,owner,action,registry,counter,host)? else{return Ok(None)};
        ensure!(!self.services.contains_key(&id),"Navigation service identity already owned");
        self.services.insert(id,service);Ok(Some(id))
    }
}
/// Pure final CPathFinder getter497fc8 over the published ordered service list.
pub fn find_pathfinder(base:&super::ai_navigation_brain_base::State,
    services:&BTreeMap<usize,super::ai_navigation_service_factory::Owned>)->Result<Option<usize>> {
    i32::try_from(base.services.len()).context("Navigation service count exceeds native signed domain")?;
    for &id in &base.services {
        let super::ai_navigation_service_factory::Owned::Pathfinder(s)=services.get(&id).context("Brain service list references an unowned object")?;
        ensure!(s.identity==id,"Brain service object identity differs");return Ok(Some(id))
    }Ok(None)
}
impl Storage {
    pub fn gameplay_navigation_brain(&self, wrapper: usize) -> Result<Option<usize>> {
        let entity = self.entities.get(&wrapper).context("Gameplay navigation wrapper is not owned")?;
        let brain = entity.entity.common.brain;
        if let Some(id) = brain {
            let owned = self.brains.get(&id).context("Gameplay navigation brain is not owned")?;
            ensure!(owned.brain.identity == id && owned.base.owner == Some(wrapper), "Gameplay navigation brain owner differs");
        }
        Ok(brain)
    }
    pub fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()> {
        self.brains.get_mut(&brain).context("Gameplay navigation brain is not owned")?.brain.gameplay_brain_2c = Some(gameplay);
        Ok(())
    }
    /// Borrow canonical disjoint records for child initialization. Objects
    /// remain discoverable in their maps throughout nested callbacks.
    pub fn child_initialization_parts(&mut self,brain:usize)->Result<(&Owned,&mut Constructed,Children<'_>)> {
        let parent=self.brains.get_mut(&brain).context("Child initialization brain is not owned")?;
        ensure!(parent.brain.identity==brain,"Child initialization brain identity differs");
        let entity=self.entities.get(&parent.base.owner.context("Child initialization brain has no entity")?).context("Child initialization entity is not owned")?;
        Ok((entity,parent,Children{services:&mut self.services,agents:&mut self.agents,agent_actions:&mut self.agent_actions,next_identity:&mut self.next_identity}))
    }
    fn identity(&mut self)->Result<usize> {
        let id=self.next_identity.checked_add(1).context("Navigation identities exhausted")?;
        self.next_identity=id;Ok(id)
    }
}
pub struct Binding<'a,B> {
    pub world:&'a World,
    pub tree:&'a mut Tree,
    pub metadata:&'a Metadata,
    pub storage:&'a mut Storage,
    /// Caller-owned inline pairs indexed by the factory's opaque user handle.
    pub users:&'a BTreeMap<usize,[usize;2]>,
    pub backend:&'a mut B,
}
impl<B:Backend> Host for Binding<'_,B> {
    fn record_mut<'a>(&'a mut self,_:&'a mut ai_navigation_factory::Runtime,entity:usize)->Result<&'a mut Record> {
        Ok(&mut self.storage.entities.get_mut(&entity).context("Owned navigation entity absent")?.entity.common)
    }
    fn contains_entity(&self,_:&ai_navigation_factory::Runtime,entity:usize)->bool {self.storage.entities.contains_key(&entity)}
    fn enabled_45e6bc(&mut self,_:&mut ai_navigation_factory::Runtime)->Result<bool> {self.backend.enabled()}
    fn world_context(&mut self,_:&mut ai_navigation_factory::Runtime)->Result<Option<usize>> {self.backend.world()}
    fn pool_has_free(&mut self,_:&mut ai_navigation_factory::Runtime)->Result<bool> {
        let (memory,slot)=self.backend.factory_pool()?;
        super::ai_navigation_factory_pool::available(memory,slot)
    }
    fn class_name(&mut self,_:&mut ai_navigation_factory::Runtime,node:usize,key:&str)->Result<Option<Vec<u8>>> {
        Ok(self.tree.first_value(node,key.as_bytes())?.map(<[u8]>::to_vec))
    }
    fn class_metadata(&mut self,_:&mut ai_navigation_factory::Runtime,kind:Kind,name:Option<&[u8]>)->Result<Option<usize>> {
        Ok(match (kind,name) {
            (Kind::Entity,Some(b"JPSBaseAIEntity"))=>Some(self.metadata.entity_class),
            (Kind::Brain,Some(b"JPSBaseAIBrain"))=>Some(self.metadata.brain_class),
            (Kind::Action,Some(b"JPSBaseAIAction"))=>Some(self.metadata.action_class),
            (_,None)=>None,
            (_,Some(name))=>anyhow::bail!("Owned navigation class metadata is not implemented: {}",String::from_utf8_lossy(name)),
        })
    }
    fn class_factory(&mut self,_:&mut ai_navigation_factory::Runtime,kind:Kind,metadata:usize)->Result<Option<usize>> {
        Ok(match kind {
            Kind::Entity if metadata==self.metadata.entity_class=>Some(self.metadata.entity_class),
            Kind::Brain if metadata==self.metadata.brain_class=>Some(self.metadata.brain_class),
            _=>None,
        })
    }
    fn construct_entity(&mut self,_:&mut ai_navigation_factory::Runtime,factory:usize,name:&[u8],user:Option<usize>)->Result<Option<usize>> {
        ensure!(factory==self.metadata.entity_class,"Unsupported owned navigation entity factory");
        let pair=*self.users.get(&user.context("AI entity requires its inline user pair")?).context("Navigation user pair absent")?;
        let id=self.storage.identity()?;
        let scale=self.backend.unit_scale()?;
        let entity=self.world.construct_navigation_entity(id,Some(name),pair,&self.metadata.entity_attributes,scale)?;
        ensure!(self.storage.entities.insert(id,entity).is_none(),"Duplicate navigation entity identity");
        Ok(Some(id))
    }
    fn initialize_entity(&mut self,_:&mut ai_navigation_factory::Runtime,entity:usize,definition:usize)->Result<bool> {
        self.storage.entities.get_mut(&entity).context("Navigation initialization owner absent")?.entity.initialize_definition(
            definition,&mut super::ai_navigation_definitions::Binding{tree:self.tree,profiles:self.backend})
    }
    fn brain_definition(&mut self,_:&mut ai_navigation_factory::Runtime,definition:usize)->Result<Option<usize>> {self.tree.find_tag(definition,b"Brain")}
    fn construct_brain(&mut self,_:&mut ai_navigation_factory::Runtime,factory:usize,entity:usize,action_class:usize)->Result<Option<usize>> {
        ensure!(factory==self.metadata.brain_class && action_class==self.metadata.action_class && self.metadata.action.metadata==action_class,"Owned navigation brain factory metadata differs");
        ensure!(self.storage.entities.contains_key(&entity),"Constructed brain entity absent");
        let id=self.storage.identity()?;let action=self.storage.identity()?;
        let brain=Constructed::new(id,Some(entity),Some(action),&self.metadata.action,&self.metadata.brain_layout)?;
        ensure!(self.storage.brains.insert(id,brain).is_none(),"Duplicate navigation brain identity");Ok(Some(id))
    }
    fn initialize_brain(&mut self,_:&mut ai_navigation_factory::Runtime,brain:usize,definition:usize)->Result<bool> {
        let constructed=self.storage.brains.get_mut(&brain).context("Owned navigation brain absent")?;
        let entity=constructed.base.owner.context("Navigation brain owner absent")?;
        ensure!(self.storage.entities.contains_key(&entity),"Navigation brain entity absent");
        self.backend.initialize_brain(self.world,self.tree,definition,&self.storage.entities,entity,constructed,
            &mut Children{services:&mut self.storage.services,agents:&mut self.storage.agents,agent_actions:&mut self.storage.agent_actions,next_identity:&mut self.storage.next_identity})
    }
    fn destroy_brain(&mut self,_:&mut ai_navigation_factory::Runtime,brain:usize)->Result<()> {
        let constructed=self.storage.brains.get_mut(&brain).context("Destroyed navigation brain absent")?;
        let entity=self.storage.entities.get(&constructed.base.owner.context("Destroyed brain owner absent")?).context("Destroyed brain entity absent")?;
        self.backend.destroy_brain(entity,constructed,
            &mut Children{services:&mut self.storage.services,agents:&mut self.storage.agents,agent_actions:&mut self.storage.agent_actions,next_identity:&mut self.storage.next_identity})?;
        ensure!(!self.storage.services.values().any(|s|match s {
            super::ai_navigation_service_factory::Owned::Pathfinder(s)=>s.base.owner==brain,
        }),"Navigation brain destructor retained an owned service");
        ensure!(!self.storage.agents.values().any(|a|a.owner()==Some(brain)),"Navigation brain destructor retained an owned agent");
        ensure!(!self.storage.agent_actions.has_owner(brain),"Navigation brain destructor retained a private agent action");
        self.storage.brains.remove(&brain);Ok(())
    }
    fn destroy_entity(&mut self,runtime:&mut ai_navigation_factory::Runtime,entity:usize)->Result<()> {
        let owned=self.storage.entities.get(&entity).context("Destroyed navigation entity absent")?;
        if let Some(brain)=owned.entity.common.brain {self.destroy_brain(runtime,brain)?;}
        let owned=self.storage.entities.get_mut(&entity).context("Navigation entity destructor removed its owner")?;
        self.backend.destroy_entity(owned)?;self.storage.entities.remove(&entity);Ok(())
    }
    fn register_pool(&mut self,_:&mut ai_navigation_factory::Runtime,entity:usize)->Result<Option<usize>> {
        self.storage.entities.get(&entity).context("Navigation registration owner absent")?;
        let (memory,slot)=self.backend.factory_pool()?;
        super::ai_navigation_factory_pool::append(memory,slot,entity)
    }
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use super::super::{ai_navigation_entity as entity,ai_navigation_action as action,
        ai_navigation_construction::ActionFactory,ai_navigation_resolver as resolver};
    pub(in crate::blotworx) struct Constructor;
    #[test]
    fn gameplay_brain_binding_uses_canonical_wrapper_and_brain_ownership() -> Result<()> {
        struct Shape;
        impl entity::Host for Shape {
            fn radius(&mut self, _: &mut Owned) -> Result<f32> { Ok(0.5) }
            fn height(&mut self, _: &mut Owned) -> Result<f32> { Ok(1.5) }
            fn maximum_speed(&mut self, _: &mut Owned) -> Result<f32> { Ok(3.0) }
            fn unit_scale(&mut self, _: &mut Owned) -> Result<Option<f32>> { Ok(None) }
        }
        let mut storage = Storage::default();
        let attributes = Registry { count: 3,
            diameter: entity::Class { index: 0, constructor: Some(entity::Kind::Diameter) },
            height: entity::Class { index: 1, constructor: Some(entity::Kind::Height) },
            maximum_speed: entity::Class { index: 2, constructor: Some(entity::Kind::MaximumSpeed) } };
        storage.entities.insert(10, Owned::new(10, Some(b"Gus"), [7, 0], &attributes, &mut Shape)?);
        assert_eq!(storage.gameplay_navigation_brain(10)?, None);
        assert!(storage.gameplay_navigation_brain(99).is_err());
        storage.brains.insert(20, Constructed::new(20, Some(10), Some(30),
            &ActionClass { metadata: 88, factory: Some(ActionFactory::BaseAi) },
            &Layout { agent_classes: 3, service_classes: 1, attributes: action::Registry {
                count: 2, heading: action::Class { index: 0, constructor: true }, speed: action::Class { index: 1, constructor: true } } })?);
        storage.entities.get_mut(&10).unwrap().entity.common.brain = Some(20);
        let brain = storage.gameplay_navigation_brain(10)?.unwrap();
        storage.bind_gameplay_brain(brain, 70)?;
        assert_eq!(storage.brains[&20].brain.gameplay_brain_2c, Some(70));
        storage.bind_gameplay_brain(brain, 71)?;
        assert_eq!(storage.brains[&20].brain.gameplay_brain_2c, Some(71));
        assert!(storage.brains[&20].base.services.is_empty());
        assert!(storage.brains[&20].base.agents.is_empty(), "Linking claimed agent initialization");
        storage.brains.get_mut(&20).unwrap().base.owner = Some(11);
        assert!(storage.gameplay_navigation_brain(10).is_err());
        assert!(storage.bind_gameplay_brain(99, 70).is_err());
        Ok(())
    }
    impl super::super::ai_pathfinder_constructor::BaseHost for Constructor {
        fn unit_scale(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base)->Result<Option<f32>>{Ok(None)}
        fn profiling_enabled(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base)->Result<bool>{Ok(false)}
        fn register_budget(&mut self,_:&mut super::super::ai_pathfinder_constructor::Base,_:&str)->Result<i32>{anyhow::bail!("Profiling was not enabled")}
    }
    impl super::super::ai_pathfinder_constructor::Host for Constructor {
        fn world_service(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:super::super::ai_pathfinder_constructor::Service)->Result<Option<usize>>{Ok(None)}
        fn point_attribute_index(&mut self,_:&mut super::super::ai_pathfinder_constructor::State)->Result<i32>{Ok(3)}
    }
    impl super::super::ai_flee_agent::Host for Constructor {
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut super::super::ai_flee_agent::State)->Result<usize>{anyhow::bail!("Unbound Flee entity query")}
        fn release_reservation(&mut self,_:&mut super::super::ai_flee_agent::State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected Flee reservation")}
        fn detach_service(&mut self,_:&mut super::super::ai_flee_agent::State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected Flee service")}
    }
    impl super::super::ai_path_agent::Host for Constructor {
        fn point_observer(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<(usize,usize)>{Ok((3,771))}
        fn bind_base(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<bool>{anyhow::bail!("Unexpected path binding")}
        fn create_pathfinder_54(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<Option<usize>>{anyhow::bail!("Unexpected path creation")}
        fn initialize_14(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<()>{anyhow::bail!("Unexpected derived initialization")}
        fn reset_path_4c(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<()>{anyhow::bail!("Unbound path reset")}
        fn motion(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize,_:[f32;3],_:f32)->Result<()>{anyhow::bail!("Unexpected path motion")}
        fn ready(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<bool>{anyhow::bail!("Unbound path readiness")}
        fn reconcile(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<u32>{anyhow::bail!("Unbound path reconciliation")}
        fn status(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<u32>{anyhow::bail!("Unbound path status")}
    }
    impl super::super::ai_goto_configuration::Host for Constructor {
        fn parameter(&mut self,_:&mut super::super::ai_goto_configuration::Shared,n:&str,_:&str,_:f32,_:f32)->Result<Option<usize>>{
            Ok(Some(match n{"AIAvoidMickeyRadius"=>1010,"AIAvoidMickeyLookaheadTime"=>1020,_=>anyhow::bail!("Unexpected Goto parameter")}))
        }
        fn retain(&mut self,_:&mut super::super::ai_goto_configuration::Shared,_:usize)->Result<()>{Ok(())}
        fn release(&mut self,_:&mut super::super::ai_goto_configuration::Shared,_:usize)->Result<()>{anyhow::bail!("Unexpected Goto parameter release")}
    }
    impl super::super::ai_goto_agent::Host for Constructor {
        fn global_vector_564d0(&mut self)->Result<[f32;3]>{anyhow::bail!("Unexpected Goto reset")}
        fn service_speed(&mut self,_:usize)->Result<f32>{anyhow::bail!("Unbound Goto speed")}
        fn path_value_8a8(&mut self,_:usize)->Result<f32>{anyhow::bail!("Unbound Goto path")}
        fn request_base_path(&mut self,_:&mut super::super::ai_goto_agent::Agent)->Result<u32>{anyhow::bail!("Unbound Goto request")}
        fn set_service_speed(&mut self,_:usize,_:f32)->Result<()>{anyhow::bail!("Unbound Goto speed write")}
    }
    impl super::super::ai_navigation_agent_factory::ConstructionHost for Constructor {
        fn class_constructed(&mut self,r:&super::super::ai_navigation_agent_factory::Record)->Result<()>{
            // Registration is an explicit supplied boundary in this ownership
            // fixture, not evidence of a working global manager registry.
            ensure!(r.owner().is_some(),"Fixture agent owner missing");Ok(())
        }
    }
    #[test]
    fn parent_initialization_constructs_canonical_services_with_shared_identity_allocation()->Result<()> {
        let mut storage=Storage::default();
        let parent=storage.identity()?;let parent_action=storage.identity()?;
        let make_registry=||action::Registry{count:2,heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}};
        let registry=make_registry();
        storage.brains.insert(parent,Constructed::new(parent,None,Some(parent_action),
            &ActionClass{metadata:88,factory:Some(ActionFactory::BaseAi)},
            &Layout{agent_classes:3,service_classes:1,attributes:make_registry()})?);
        let mut tree=Tree::default();
        let node=tree.append(None,super::super::ai_navigation_definitions::Node::default())?;
        tree.append(Some(node),super::super::ai_navigation_definitions::Node{name:Some(b"Class".to_vec()),value:Some(b"Mpd::CPathFinder".to_vec()),..Default::default()})?;
        let mut counter=super::super::ai_pathfinder_constructor::Counter::default();
        let owner=&storage.brains[&parent];
        let mut children=Children{services:&mut storage.services,agents:&mut storage.agents,agent_actions:&mut storage.agent_actions,next_identity:&mut storage.next_identity};
        let service=children.construct_service(&tree,node,owner,&registry,&mut counter,&mut Constructor)?.unwrap();
        let super::super::ai_navigation_service_factory::Owned::Pathfinder(state)=&children.services[&service];
        assert_eq!((service,state.identity,state.base.owner,state.base.action_identity),(3,3,parent,4));
        assert_eq!(state.base.action_identity==parent_action,false);
        assert!(owner.base.services.is_empty(),"Construction published an uninitialized service");
        assert_eq!(state.mode_28,None);assert_eq!(counter.created,1);
        let missing=tree.append(None,Default::default())?;
        assert!(children.construct_service(&tree,missing,owner,&registry,&mut counter,&mut Constructor)?.is_none());
        assert_eq!(children.identity()?,5,"Missing class consumed identities");
        let direct=children.construct_pathfinder(owner.into(),&registry,&mut counter,&mut Constructor)?;
        let super::super::ai_navigation_service_factory::Owned::Pathfinder(state)=&children.services[&direct];
        assert_eq!((direct,state.identity,state.base.owner,state.base.action_identity),(6,6,parent,7));
        assert_eq!(counter.created,2);
        assert!(owner.base.services.is_empty(),"Direct class factory published an uninitialized service");
        use super::super::ai_navigation_agent_factory::{Class,Owned as Agent};
        let mut shared=super::super::ai_goto_configuration::Shared::default();
        let flee=children.construct_agent(&owner.brain,Class::named(b"Mpd::JPSFleeAgent")?,&mut shared,&mut Constructor)?;
        let wander=children.construct_agent(&owner.brain,Class::named(b"JPSWanderAgent\0unused")?,&mut shared,&mut Constructor)?;
        let goto=children.construct_agent(&owner.brain,Class::named(b"JPSGotoAgent")?,&mut shared,&mut Constructor)?;
        assert_eq!((flee,wander,goto),(8,9,10));
        assert!(Class::named(b"jpsgotoagent").is_err(),"Agent class lookup became case insensitive");
        assert!(owner.base.agents.is_empty(),"Construction published unbound agents");
        assert_eq!(children.find_agent(&owner.base,Class::Goto)?,None);
        assert_eq!(children.find_pathfinder(&owner.base)?,None);
        assert!(matches!(children.agents[&flee].object,Agent::Flee(_)));
        let Agent::Wander(a)=&children.agents[&wander].object else{anyhow::bail!("Wrong Wander class")};
        assert_eq!(a.point.attributes[3],Some(771));assert_eq!(a.pathfinder,None);
        let Agent::Goto(a)=&children.agents[&goto].object else{anyhow::bail!("Wrong Goto class")};
        assert_eq!(a.flag_a8,None);assert_eq!(shared.counter,1);
        assert!(storage.brains.contains_key(&parent),"Construction removed its live parent");
        assert_eq!(storage.identity()?,11,"Child allocation used a different identity namespace");
        let mut m=super::super::ai_graph_link_passes::Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        super::super::ai_navigation_agent_factory::transition(&mut m,&mut storage,parent,flee,true,&mut Constructor)?;
        super::super::ai_navigation_agent_factory::transition(&mut m,&mut storage,parent,flee,false,&mut Constructor)?;
        let Agent::Flee(a)=&storage.agents[&flee].object else{unreachable!()};
        assert_eq!(a.scalars_74_78_7c_80_84[3],-1.);
        assert_eq!(a.base.owner,Some(parent));
        assert!(super::super::ai_navigation_agent_factory::transition(&mut m,&mut storage,parent,wander,false,&mut Constructor).unwrap_err().to_string().contains("Unbound path reset"));
        // Supply initialized-list contents for this query fixture only. The
        // constructors above have deliberately not claimed successful binding.
        let f:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_owned_agent_query.json"))?;
        let mut bank=BTreeMap::new();
        {
            let owner=&storage.brains[&parent].brain;
            let mut children=Children{services:&mut storage.services,agents:&mut storage.agents,agent_actions:&mut storage.agent_actions,next_identity:&mut storage.next_identity};
            for class in [Class::Goto,Class::Wander,Class::Flee]{for i in 0..6{
                let id=children.construct_agent(owner,class,&mut shared,&mut Constructor)?;bank.insert((format!("{class:?}"),i),id);
            }}
        }
        for case in f["cases"].as_array().context("Owned agent query cases absent")?{
            let labels=case["classes"].as_array().unwrap();let mut ids=Vec::new();
            for (i,n) in labels.iter().enumerate(){let class=Class::named(n.as_str().unwrap().as_bytes())?;ids.push(bank[&(format!("{class:?}"),i)]);}
            storage.brains.get_mut(&parent).unwrap().base.agents=ids.clone();
            let base=&storage.brains[&parent].base;
            let result=super::super::ai_navigation_agent_factory::find(base,&storage.agents,Class::named(case["wanted"].as_str().unwrap().as_bytes())?)?;
            let index=result.map(|id|ids.iter().position(|&v|v==id).unwrap());
            assert_eq!(serde_json::json!(index),case["result_index"],"{case}");
        }
        Ok(())
    }
    struct Services {calls:Vec<&'static str>,owner:usize,shape:Option<[u32;2]>,pool:super::super::ai_graph_link_passes::Memory}
    impl ProfileServices for Services {
        fn manager(&mut self,_:&mut super::super::ai_navigation_entity_base::Entity)->Result<Option<usize>> {unreachable!()}
        fn find(&mut self,_:&mut super::super::ai_navigation_entity_base::Entity,_:usize,_:&[u8])->Result<Option<usize>> {unreachable!()}
        fn attach(&mut self,_:&mut super::super::ai_navigation_entity_base::Entity,_:usize)->Result<bool> {unreachable!()}
        fn detach(&mut self,_:&mut super::super::ai_navigation_entity_base::Entity,_:usize)->Result<()> {unreachable!()}
    }
    impl Lifecycle for Services {
        fn enabled(&mut self)->Result<bool> {Ok(true)}
        fn world(&mut self)->Result<Option<usize>> {Ok(Some(7))}
        fn factory_pool(&mut self)->Result<(&mut super::super::ai_graph_link_passes::Memory,u32)> {Ok((&mut self.pool,0x50000000))}
        fn unit_scale(&mut self)->Result<Option<f32>> {Ok(None)}
        fn destroy_brain(&mut self,_:&Owned,_:&mut Constructed,_:&mut Children<'_>)->Result<()> {self.calls.push("brain_destroy");Ok(())}
        fn destroy_entity(&mut self,entity:&mut Owned)->Result<()> {self.calls.push("entity_destroy");assert_eq!(entity.entity.common.brain,None);Ok(())}
    }
    impl Backend for Services {
        fn initialize_brain(&mut self,_:&World,tree:&mut Tree,definition:usize,entities:&BTreeMap<usize,Owned>,entity:usize,brain:&mut Constructed,_:&mut Children<'_>)->Result<bool> {
            let entity=entities.get(&entity).context("Fixture brain entity absent")?;
            self.calls.push("brain_init");
            assert_eq!(entity.entity.user_data,[self.owner,0]);
            assert_eq!(entity.entity.common.world,Some(7));assert_eq!(entity.entity.common.word_28,0);
            assert_eq!(entity.entity.common.brain,Some(brain.brain.identity));
            assert_eq!(brain.base.owner,Some(entity.entity.identity));
            assert!(brain.action.is_some());assert_eq!(entity.scalars.len(),3);
            let service=tree.find_tag(definition,b"Service")?.context("Resolved native pathfinder definition absent")?;
            assert_eq!(tree.first_value(service,b"Class")?,Some(b"Mpd::CPathFinder".as_slice()));
            self.shape=Some([entity.entity.radius.to_bits(),entity.entity.height.to_bits()]);
            // Deliberately reject at the service boundary. This fixture cannot
            // pretend that pathfinder/global-service initialization succeeded.
            Ok(false)
        }

    }
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_owned_factory_constructs_real_records_and_cleans_uninitialized_services()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let owner=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
        let catalog=world.biped_navigation_catalog()?;
        let mut resolution=resolver::Runtime{world:None,allocator:1,fallback_allocator:2};
        let manager=resolver::Manager{identity:7,allocator:None,definition_root:Some(catalog.root)};
        
        // Metadata layout and manager services are supplied fixture boundaries;
        // all entity/brain constructors, property reads and ownership run live.
        let metadata=Metadata{entity_class:101,brain_class:102,action_class:103,
            entity_attributes:Registry{count:3,diameter:entity::Class{index:0,constructor:Some(entity::Kind::Diameter)},height:entity::Class{index:1,constructor:Some(entity::Kind::Height)},maximum_speed:entity::Class{index:2,constructor:Some(entity::Kind::MaximumSpeed)}},
            brain_layout:Layout{agent_classes:3,service_classes:1,attributes:action::Registry{count:2,heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}}},
            action:ActionClass{metadata:103,factory:Some(ActionFactory::BaseAi)}};
        let mut storage=Storage::default();
        let mut pool=super::super::ai_graph_link_passes::Memory::default();
        pool.regions.insert(0x50000000,0x51000000u32.to_be_bytes().to_vec());
        pool.regions.insert(0x51000000,vec![0;0x1c]);pool.regions.insert(0x52000000,vec![0;0x14]);
        pool.write(0x51000010,0x52000000)?;pool.write(0x51000018,1)?;
        let mut services=Services{calls:vec![],owner,shape:None,pool};let mut factory=ai_navigation_factory::Runtime::default();
        let mut wrapper=Some(99);
        world.initialize_owned_navigation_wrapper(owner,&mut wrapper,&mut super::super::ai_controller_navigation_factory::Factory{
            world:&world,source:&catalog.tree,manager:Some(&manager),resolution:&mut resolution,runtime:&mut factory,
            metadata:&metadata,storage:&mut storage,backend:&mut services,registered_flags:1,
        })?;
        assert_eq!(wrapper,None);assert!(storage.entities.is_empty() && storage.brains.is_empty());
        assert!(factory.records.is_empty(),"Factory duplicated inline entity state");
        assert_eq!(services.calls,["brain_init","brain_destroy","entity_destroy"]);
        let shape=services.shape.context("Owned shape construction was not reached")?;
        assert_eq!(shape,[super::super::motion_ground::radius(0,&world.runtime_settings)?.to_bits(),super::super::character_physics::height(0,&world.runtime_settings)?.to_bits()]);
        //A missing manager or root must take the actual null return branch,
        //without entering constructors or retaining the controller's old slot.
        let missing_root=resolver::Manager{identity:8,allocator:None,definition_root:None};
        for manager in [None,Some(&missing_root)]{
            let before=services.calls.len();wrapper=Some(99);
            world.initialize_owned_navigation_wrapper(owner,&mut wrapper,&mut super::super::ai_controller_navigation_factory::Factory{
                world:&world,source:&catalog.tree,manager,resolution:&mut resolution,runtime:&mut factory,
                metadata:&metadata,storage:&mut storage,backend:&mut services,registered_flags:1,
            })?;
            assert_eq!(wrapper,None);assert_eq!(services.calls.len(),before);
            assert!(storage.entities.is_empty()&&storage.brains.is_empty());
        }
        assert_eq!(resolution.world,Some(8));assert_eq!(resolution.allocator,2);
        // A controlled Brain-free JPSBaseAIEntity definition verifies real
        // successful factory ownership without claiming a normal initialized
        // NPC agent, navigation World admission, or derived service success.
        assert_eq!(services.pool.word(0x51000014)?,0,"Rejected initialization must not consume a factory slot");
        let mut definition_tree=Tree::default();
        let definition=definition_tree.append(None,super::super::ai_navigation_definitions::Node::default())?;
        definition_tree.append(Some(definition),super::super::ai_navigation_definitions::Node{
            name:Some(b"Class".to_vec()),value:Some(b"JPSBaseAIEntity".to_vec()),..Default::default()})?;
        let users=BTreeMap::from([(1,[owner,0])]);
        let created=factory.create(Some(b"Factory membership check"),Some(definition),0,Some(1),
            &mut Binding{world:&world,tree:&mut definition_tree,metadata:&metadata,storage:&mut storage,users:&users,backend:&mut services})?.context("Brain-free entity factory rejected")?;
        let common=&storage.entities[&created].entity.common;
        assert_eq!(common.pool_node,Some(0x52000000));assert_eq!(common.word_14,0);
        assert_eq!(common.word_28,0);assert_eq!(common.world,Some(7));assert_eq!(common.brain,None);
        assert_eq!(common.flags,0);assert_eq!(common.definition,None);
        assert_eq!(services.pool.word(0x51000014)?,1);
        assert_eq!(services.pool.word(0x52000004)?,u32::try_from(created)?);
        let count=storage.entities.len();
        assert_eq!(factory.create(Some(b"Full ownership pool"),Some(definition),0,Some(1),
            &mut Binding{world:&world,tree:&mut definition_tree,metadata:&metadata,storage:&mut storage,users:&users,backend:&mut services})?,None);
        assert_eq!(storage.entities.len(),count);assert!(factory.records.is_empty());
        Ok(())
    }
}
