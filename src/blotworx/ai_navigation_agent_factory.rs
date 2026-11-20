//! Registered EM1 agent classes. Goto/Wander use PathAgent; Flee uses Agent.
//! Constructed ownership never implies completed binding or World admission.
use anyhow::{Context,Result,ensure};
use super::{ai_goto_agent,ai_path_agent,ai_flee_agent,ai_goto_configuration,ai_graph_link_passes::Memory};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Class {Goto,Wander,Flee}
impl Class {
    pub fn named(name:&[u8])->Result<Self>{
        let name=&name[..name.iter().position(|&b|b==0).unwrap_or(name.len())];
        match name{b"JPSGotoAgent"=>Ok(Self::Goto),b"JPSWanderAgent"=>Ok(Self::Wander),b"Mpd::JPSFleeAgent"=>Ok(Self::Flee),
            _=>anyhow::bail!("Navigation agent Class is not implemented: {}",String::from_utf8_lossy(name))}
    }
}
pub enum Owned {Goto(Box<ai_goto_agent::Agent>),Wander(Box<ai_path_agent::Agent>),Flee(Box<ai_flee_agent::State>)}
pub struct Record {pub identity:usize,pub object:Owned}
pub trait ConstructionHost:ai_path_agent::Host+ai_goto_configuration::Host+ai_flee_agent::Host {
    /// Enclosing factory tail publishes this class's vtable to its actual
    /// global metadata and registration list. Object construction alone does
    /// not reproduce that global operation or initialize a World service.
    fn class_constructed(&mut self,record:&Record)->Result<()>;
}
impl Record {
    pub fn class(&self)->Class{match &self.object{Owned::Goto(_)=>Class::Goto,Owned::Wander(_)=>Class::Wander,Owned::Flee(_)=>Class::Flee}}
    pub fn construct<H:ConstructionHost>(
        identity:usize,owner:Option<usize>,class:Class,shared:&mut ai_goto_configuration::Shared,host:&mut H)->Result<Self>{
        let object=match class{
            Class::Goto=>Owned::Goto(Box::new(ai_goto_agent::Agent::new(owner,shared,host)?)),
            Class::Wander=>Owned::Wander(Box::new(ai_path_agent::Agent::new(owner,host)?)),
            Class::Flee=>Owned::Flee(Box::new(ai_flee_agent::State::new(owner,host)?)),
        };let record=Self{identity,object};host.class_constructed(&record)?;Ok(record)
    }
    pub fn owner(&self)->Option<usize>{match &self.object{Owned::Goto(a)=>a.path.base.owner,Owned::Wander(a)=>a.base.owner,Owned::Flee(a)=>a.base.owner}}
    pub fn world_inserted2c(&mut self){
        //Actual2b7f18. Explicit variants keep a future class from inheriting
        //a successful empty callback without its own source verification.
        match &mut self.object{Owned::Goto(_)|Owned::Wander(_)|Owned::Flee(_)=>{}}
    }
    pub fn world_removed28<H:ai_goto_agent::Host+ai_flee_agent::Host>(&mut self,m:&mut Memory,h:&mut H)->Result<()>{
        match &mut self.object{
            Owned::Goto(a)=>a.initialize(h),Owned::Wander(a)=>a.initialize(h),Owned::Flee(a)=>a.world_removed28(m,h),
        }
    }
}
///4594e8 specialized to the three supported final EM1 biped classes. None of
///these derives from another requested class; their class getters are constant.
///Borrow the canonical list and records; preserve duplicates/first match.
pub fn find(base:&super::ai_navigation_brain_base::State,
    objects:&std::collections::BTreeMap<usize,Record>,wanted:Class)->Result<Option<usize>> {
    i32::try_from(base.agents.len()).context("Navigation agent count exceeds native signed domain")?;
    for &id in &base.agents{
        let record=objects.get(&id).context("Brain agent list references an unowned object")?;
        ensure!(record.identity==id,"Brain agent object identity differs");
        if record.class()==wanted{return Ok(Some(id))}
    }Ok(None)
}
/// Actual class dispatch for the base-brain agent pass. Callers can bind the
/// host to disjoint canonical brain/entity/service fields before invoking it.
pub fn transition<H:ai_goto_agent::Host+ai_flee_agent::Host>(m:&mut Memory,
    actors:&mut super::ai_navigation_owned_factory::Storage,brain:usize,child:usize,inserted:bool,host:&mut H)->Result<()> {
    ensure!(actors.brains.contains_key(&brain),"Agent callback navigation brain is not owned");
    let record=actors.agents.get_mut(&child).context("World callback agent is not owned")?;
    ensure!(record.identity==child && record.owner()==Some(brain),"World callback agent identity or owning brain differs");
    if inserted{record.world_inserted2c();Ok(())}else{record.world_removed28(m,host)}
}
