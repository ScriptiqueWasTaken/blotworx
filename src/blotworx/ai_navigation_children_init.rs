//! Concrete child construction/initialization inside459080. Services are owned
//! by the factory throughout; only successful derived initialization publishes
//! into the base brain's ordered list. World admission remains a later stage.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_navigation_brain_base::{self,State,Kind},ai_navigation_brain::Brain,
    ai_navigation_action::{Action,Registry},ai_navigation_construction::Constructed,
    ai_navigation_definitions::Tree,ai_navigation_owned_factory::Children,
    ai_pathfinder_constructor::{self,Counter},ai_pathfinder_initialization as pathfinder,
    ai_pathfinder_concrete_modifiers as modifiers,ai_navigation_service_factory::Owned,
    ai_graph_link_passes::Memory};
use super::ai_navigation_agent_factory::{Class as AgentKind,ConstructionHost};

pub struct ServiceClass {pub identity:usize,pub has_factory:bool}
pub struct AgentClass {pub identity:usize,pub class:AgentKind,pub has_factory:bool}
/// References into the caller's one manager lifetime. In particular modifiers
/// and shared search arrays are not rebuilt for each actor/brain callback.
pub struct Resources<'a> {
    pub service_class:&'a ServiceClass,
    pub agent_classes:&'a [AgentClass],
    pub goto_shared:&'a mut super::ai_goto_configuration::Shared,
    pub entity:&'a super::ai_navigation_entity::Owned,
    pub entity_registry:&'a super::ai_navigation_entity::Registry,
    pub action_class:&'a super::ai_navigation_construction::ActionClass,
    pub action_registry:&'a Registry,
    pub counter:&'a mut Counter,
    pub shared:&'a mut pathfinder::Shared,
    pub memory:&'a mut Memory,
    pub modifier_metadata:&'a modifiers::Metadata,
    pub modifiers:&'a mut BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,
    pub path_context:super::ai_agent_path::Context,
    pub spatial:super::ai_spatial_construction::Context,
}
impl Resources<'_> {
    pub fn reborrow(&mut self)->Resources<'_>{Resources{
        service_class:self.service_class,agent_classes:self.agent_classes,goto_shared:&mut *self.goto_shared,
        entity:self.entity,entity_registry:self.entity_registry,action_class:self.action_class,action_registry:self.action_registry,
        counter:&mut *self.counter,shared:&mut *self.shared,memory:&mut *self.memory,
        modifier_metadata:self.modifier_metadata,modifiers:&mut *self.modifiers,path_context:self.path_context,spatial:self.spatial,
    }}
}
/// Derived agent factories and destructors must be real class operations.
/// Returning null is reserved for a verified absent registration/factory;
/// unsupported implementations must return an error, never a ready placeholder.
pub trait Backend:ai_pathfinder_constructor::Host+pathfinder::Backend+modifiers::Backend+ConstructionHost+super::ai_path_agent_binding::Services {
    fn destroy_agent(&mut self,parent:super::ai_agent_actions::Parent<'_>,children:&mut Children<'_>,agent:usize)->Result<()>;
    fn destroy_service(&mut self,brain:&Brain,base:&mut State,children:&mut Children<'_>,memory:&mut Memory,service:usize)->Result<()>;
    fn attribute(&mut self,brain:&Brain,base:&mut State,tree:&mut Tree,node:usize)->Result<bool>;
}
pub fn initialize(owner:&mut Constructed,children:&mut Children<'_>,tree:&mut Tree,root:Option<usize>,
    resources:Resources<'_>,backend:&mut impl Backend)->Result<bool> {
    initialize_parts(&owner.brain,&mut owner.base,&mut owner.action,children,tree,root,resources,backend)
}
pub fn initialize_parts(brain:&Brain,base:&mut State,action:&mut Option<Action>,children:&mut Children<'_>,tree:&mut Tree,root:Option<usize>,resources:Resources<'_>,backend:&mut impl Backend)->Result<bool>{
    base.initialize(root,&mut Binding{brain,action,children,tree,resources,backend})
}
struct Binding<'a,'c,H> {
    brain:&'a Brain,action:&'a mut Option<Action>,children:&'a mut Children<'c>,tree:&'a mut Tree,
    resources:Resources<'a>,backend:&'a mut H,
}
impl<H> Binding<'_, '_, H> {
    fn agent_class(&self,identity:usize)->Result<&AgentClass>{
        let mut classes=self.resources.agent_classes.iter().filter(|c|c.identity==identity);
        let class=classes.next().context("Navigation agent class metadata is absent")?;
        ensure!(classes.next().is_none(),"Duplicate navigation agent class identity");Ok(class)
    }
}
impl<H:Backend> ai_navigation_brain_base::Host for Binding<'_, '_, H> {
    fn child_count(&mut self,_:&mut State,root:usize)->Result<u32>{
        u32::try_from(self.tree.node(root)?.children.len()).context("Navigation child count exceeds native word domain")
    }
    fn child(&mut self,_:&mut State,root:usize,index:u32)->Result<usize>{
        self.tree.node(root)?.children.get(index as usize).copied().context("Navigation child disappeared")
    }
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(node)?.name.clone())}
    fn class_name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.first_value(node,b"Class")?.map(<[u8]>::to_vec))}
    fn class_metadata(&mut self,_:&mut State,kind:Kind,name:&[u8])->Result<Option<usize>>{
        match kind {
            Kind::Service=>{
                ensure!(name==b"Mpd::CPathFinder","Unimplemented navigation service Class: {}",String::from_utf8_lossy(name));
                Ok(Some(self.resources.service_class.identity))
            }
            Kind::Agent=>{
                let kind=AgentKind::named(name)?;
                let mut classes=self.resources.agent_classes.iter().filter(|c|c.class==kind);
                let class=classes.next();
                ensure!(classes.next().is_none(),"Duplicate navigation agent class registration");
                if let Some(class)=class {
                    ensure!(class.identity!=self.resources.service_class.identity,"Agent and service class identities collide");
                    self.agent_class(class.identity)?;
                }
                Ok(class.map(|c|c.identity))
            }
        }
    }
    fn class_factory(&mut self,_:&mut State,metadata:usize)->Result<Option<usize>>{
        if metadata==self.resources.service_class.identity {
            Ok(self.resources.service_class.has_factory.then_some(metadata))
        }else{Ok(self.agent_class(metadata)?.has_factory.then_some(metadata))}
    }
    fn construct(&mut self,base:&mut State,kind:Kind,factory:usize)->Result<usize>{
        match kind {
            Kind::Service=>{
                ensure!(factory==self.resources.service_class.identity,"Wrong service factory identity");
                self.children.construct_pathfinder(ai_pathfinder_constructor::Owner{brain:self.brain,base,action:self.action.as_ref()},
                    self.resources.action_registry,self.resources.counter,self.backend)
            }
            Kind::Agent=>{
                let class=self.agent_class(factory)?.class;
                self.children.construct_agent(self.brain,class,self.resources.goto_shared,self.backend)
            }
        }
    }
    fn initialize(&mut self,base:&mut State,kind:Kind,instance:usize,node:usize)->Result<bool>{
        match kind {
            Kind::Service=>{
                let Owned::Pathfinder(state)=self.children.services.get_mut(&instance).context("Initialized service is not owned")?;
                ensure!(state.identity==instance && state.base.owner==self.brain.identity,"Initialized service has a different owner");
                pathfinder::initialize_concrete(state,self.tree,node,self.resources.shared,self.resources.memory,
                    self.resources.modifier_metadata,self.resources.modifiers,self.resources.spatial,self.backend)
            }
            Kind::Agent=>{
                let record=self.children.agents.get(&instance).context("Initialized agent is not owned")?;
                ensure!(record.identity==instance && record.owner()==Some(self.brain.identity),"Initialized agent has a different owner");
                let class=record.class();
                let parent=super::ai_agent_actions::Parent{brain:self.brain,base,action:self.action};
                if class==AgentKind::Flee {
                    super::ai_flee_binding::bind_owned(parent,self.children,self.resources.entity,self.resources.entity_registry,
                        self.tree,instance,node,self.resources.action_class,self.resources.action_registry,self.resources.memory,self.backend)
                } else {
                    super::ai_path_agent_binding::bind_owned(parent,self.children,self.resources.entity,self.resources.entity_registry,
                        self.tree,instance,node,self.resources.action_class,self.resources.action_registry,self.resources.modifiers,
                        self.resources.memory,self.resources.path_context,self.backend)
                }
            }
        }
    }
    fn destroy(&mut self,base:&mut State,kind:Kind,instance:usize)->Result<()>{
        match kind {
            Kind::Service=>{
                self.backend.destroy_service(self.brain,base,self.children,self.resources.memory,instance)?;
                ensure!(!self.children.services.contains_key(&instance),"Service destructor retained its owned object");Ok(())
            }
            Kind::Agent=>{
                self.backend.destroy_agent(super::ai_agent_actions::Parent{brain:self.brain,base,action:self.action},self.children,instance)?;
                ensure!(!self.children.agents.contains_key(&instance),"Agent destructor retained its owned object");Ok(())
            }
        }
    }
    fn attribute(&mut self,base:&mut State,node:usize)->Result<bool>{self.backend.attribute(self.brain,base,self.tree,node)}
}
