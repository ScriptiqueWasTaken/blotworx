//! Concrete modifier construction and definition initialization. This factory
//! handles gap avoidance, goal refinement and the three distance detectors; others must
//! have their own implementation rather than being treated as missing classes.
use anyhow::{Result,ensure};
use std::borrow::Cow;
use super::ai_graph_link_passes::Memory;
use super::{ai_pathfinder_constructor,ai_pathfinder_modifiers::Kind,ai_navigation_definitions::Tree,
    ai_goto_gap,ai_goto_gap_definition_binding};
pub trait DefinitionBackend:ai_goto_gap_definition_binding::Backend+super::ai_goal_detection::DefinitionBackend+super::ai_predict_move::DefinitionBackend+super::ai_node_candidate::DefinitionBackend+super::ai_node_reached::DefinitionBackend+super::ai_edge_awareness::DefinitionBackend+super::ai_steering::DefinitionBackend+super::ai_compute_target::DefinitionBackend {}
impl<T:ai_goto_gap_definition_binding::Backend+super::ai_goal_detection::DefinitionBackend+super::ai_predict_move::DefinitionBackend+super::ai_node_candidate::DefinitionBackend+super::ai_node_reached::DefinitionBackend+super::ai_edge_awareness::DefinitionBackend+super::ai_steering::DefinitionBackend+super::ai_compute_target::DefinitionBackend> DefinitionBackend for T {}
pub enum Owned { FindNodes(Box<super::ai_find_nodes::State>), ComputeTarget(Box<super::ai_compute_target::State>), Steering(Box<super::ai_steering::State>), EdgeAwareness(Box<super::ai_edge_awareness::State>), NodeReached(Box<super::ai_node_reached::State>), NodeCandidate(Box<super::ai_node_candidate::State>), Accident(Box<super::ai_predict_move::State>), Spatial{address:u32}, GoalDetection(Box<super::ai_goal_detection::State>), GapAvoidance(Box<ai_goto_gap::State>), RefineGoal(Box<super::ai_refine_goal::State>) }
impl Owned {
    pub const SPATIAL_CLASS:&'static [u8]=b"Mpd::CCanGo_AiMesh";
    pub const GAP_CLASS:&'static [u8]=b"Mpd::CGoto_GapDynamicAvoidance_v2";
    pub const REFINE_CLASS:&'static [u8]=b"JPSRefineGoal";
    pub fn construct(kind:Kind,class:&[u8],identity:usize,pathfinder:&ai_pathfinder_constructor::State,name:&[u8],host:&mut impl ai_goto_gap::Host)->Result<Self> {
        Self::construct_typed(kind,class,identity,pathfinder.identity,name,host)
    }
    /// The captured owner is a reference identity, not a copied parent record.
    /// A scoped provider can therefore borrow the real parent during callbacks.
    pub fn construct_typed(kind:Kind,class:&[u8],identity:usize,pathfinder:usize,name:&[u8],host:&mut impl ai_goto_gap::Host)->Result<Self>{
        let class=&class[..class.iter().position(|&b|b==0).unwrap_or(class.len())];
        if kind==Kind(10) && class==super::ai_find_nodes::State::CLASS{return Ok(Self::FindNodes(Box::new(super::ai_find_nodes::State::construct(identity,pathfinder,name))))}
        if kind==Kind(7) && class==super::ai_compute_target::State::CLASS{return Ok(Self::ComputeTarget(Box::new(super::ai_compute_target::State::construct(identity,pathfinder,name))))}
        for candidate in [super::ai_node_reached::Class::Distance2D5,super::ai_node_reached::Class::Distance3D]{if kind==Kind(6)&&class==candidate.name(){return Ok(Self::NodeReached(Box::new(super::ai_node_reached::State::construct(candidate,identity,pathfinder,name))))}}
        if kind==Kind(5) && class==super::ai_node_candidate::State::CLASS{return Ok(Self::NodeCandidate(Box::new(super::ai_node_candidate::State::construct(identity,pathfinder,name))))}
        if kind==Kind(4) && class==super::ai_predict_move::State::CLASS{return Ok(Self::Accident(Box::new(super::ai_predict_move::State::construct(identity,pathfinder,name))))}
        for candidate in [super::ai_goal_detection::Class::Reached2D5,super::ai_goal_detection::Class::Reached3D,super::ai_goal_detection::Class::Changed3D] {
            if kind==candidate.kind() && class==candidate.name() {return Ok(Self::GoalDetection(Box::new(super::ai_goal_detection::State::construct(candidate,identity,pathfinder,name))))}
        }
        if kind==Kind(0) && class==Self::REFINE_CLASS {
            return Ok(Self::RefineGoal(Box::new(super::ai_refine_goal::State::construct(identity,pathfinder,name))));
        }
        ensure!(kind==Kind(8) && class==Self::GAP_CLASS,"Concrete pathfinder modifier is not implemented: {}",String::from_utf8_lossy(class));
        Ok(Self::GapAvoidance(Box::new(ai_goto_gap::State::construct(identity,pathfinder,name,host)?)))
    }
    pub fn identity(&self)->usize {match self{Self::FindNodes(s)=>s.identity,Self::ComputeTarget(s)=>s.identity,Self::Steering(s)=>s.identity,Self::EdgeAwareness(s)=>s.identity,Self::NodeReached(s)=>s.identity,Self::NodeCandidate(s)=>s.identity,Self::Accident(s)=>s.identity,Self::Spatial{address}=>*address as usize,Self::GoalDetection(s)=>s.identity,Self::GapAvoidance(s)=>s.identity,Self::RefineGoal(s)=>s.identity}}
    pub fn pathfinder(&self,memory:&Memory)->Result<usize> {Ok(match self{Self::FindNodes(s)=>s.pathfinder,Self::ComputeTarget(s)=>s.pathfinder,Self::Steering(s)=>s.pathfinder,Self::EdgeAwareness(s)=>s.pathfinder,Self::NodeReached(s)=>s.pathfinder,Self::NodeCandidate(s)=>s.pathfinder,Self::Accident(s)=>s.pathfinder,Self::Spatial{address}=>usize::try_from(memory.word(address.wrapping_add(4))?)?,Self::GoalDetection(s)=>s.pathfinder,Self::GapAvoidance(s)=>s.pathfinder,Self::RefineGoal(s)=>s.pathfinder})}
    pub fn kind(&self)->Kind {match self{Self::FindNodes(_)=>Kind(10),Self::ComputeTarget(_)=>Kind(7),Self::Steering(_)=>Kind(9),Self::EdgeAwareness(_)=>Kind(12),Self::NodeReached(_)=>Kind(6),Self::NodeCandidate(_)=>Kind(5),Self::Accident(_)=>Kind(4),Self::Spatial{..}=>Kind(3),Self::GoalDetection(s)=>s.class().kind(),Self::GapAvoidance(_)=>Kind(8),Self::RefineGoal(_)=>Kind(0)}}
    /// Actual virtual14: the refinement class inherits only its base reset.
    pub fn reset(&mut self,host:&mut impl ai_goto_gap::Host)->Result<()> {
        match self{Self::FindNodes(s)=>{s.reset();Ok(())},Self::ComputeTarget(s)=>{s.reset();Ok(())},Self::Steering(s)=>{s.reset();Ok(())},Self::EdgeAwareness(s)=>{s.reset();Ok(())},Self::NodeReached(s)=>{s.reset();Ok(())},Self::NodeCandidate(s)=>{s.reset();Ok(())},Self::Accident(s)=>{s.reset();Ok(())},Self::Spatial{..}=>Ok(()),/* concrete2b7c00 is a single blr */Self::GoalDetection(s)=>{s.reset();Ok(())},Self::GapAvoidance(s)=>s.reset(host),Self::RefineGoal(s)=>{s.reset();Ok(())}}
    }
    pub fn name<'a>(&'a self,memory:&Memory)->Result<Cow<'a,[u8]>> {Ok(match self{
        Self::Spatial{address}=>{let mut p=memory.word(address.wrapping_add(8))?;ensure!(p!=0,"Spatial modifier has a null owned name");let mut text=Vec::new();loop{let byte=memory.byte(p)?;if byte==0{break}text.push(byte);p=p.wrapping_add(1);}Cow::Owned(text)},
        Self::FindNodes(s)=>Cow::Borrowed(&s.name),Self::ComputeTarget(s)=>Cow::Borrowed(&s.name),Self::Steering(s)=>Cow::Borrowed(&s.name),Self::EdgeAwareness(s)=>Cow::Borrowed(&s.name),Self::NodeReached(s)=>Cow::Borrowed(&s.name),Self::NodeCandidate(s)=>Cow::Borrowed(&s.name),Self::Accident(s)=>Cow::Borrowed(&s.name),Self::GoalDetection(s)=>Cow::Borrowed(&s.name),Self::GapAvoidance(s)=>Cow::Borrowed(&s.name),Self::RefineGoal(s)=>Cow::Borrowed(&s.name)})}
    /// Real1c2e14 factory: allocator returns the identity; owner/name/pools remain
    /// solely in Memory. The supplied source name is a live arena string.
    pub fn construct_spatial(kind:Kind,class:&[u8],pathfinder:&mut ai_pathfinder_constructor::State,memory:&mut Memory,context:super::ai_spatial_construction::Context,name:u32,backend:&mut impl super::ai_pathfinder_spatial_construction::Backend)->Result<Self>{
        let class=&class[..class.iter().position(|&b|b==0).unwrap_or(class.len())];
        ensure!(kind==Kind(3) && class==Self::SPATIAL_CLASS,"Unsupported arena spatial modifier class");
        let owner=u32::try_from(pathfinder.identity)?;
        let address=super::ai_spatial_construction::create(memory,context,owner,name,&mut super::ai_pathfinder_spatial_construction::Binding{state:pathfinder,backend})?;
        Ok(Self::Spatial{address})
    }
    /// The default branch is the inline constructor491370..49146c; named
    /// factory calls additionally register metadata. Do not conflate the two.
    pub fn construct_spatial_text(pathfinder:&mut ai_pathfinder_constructor::State,memory:&mut Memory,context:super::ai_spatial_construction::Context,name:&[u8],default:bool,backend:&mut impl super::ai_pathfinder_spatial_construction::Backend)->Result<Self>{
        let owner=u32::try_from(pathfinder.identity)?;
        let address=super::ai_spatial_construction::create_text(memory,context,owner,name,!default,&mut super::ai_pathfinder_spatial_construction::Binding{state:pathfinder,backend})?;
        Ok(Self::Spatial{address})
    }
    /// Real1c3a14: pool allocation, ordered49b274/1c32f4 definitions and live
    /// canonical LPF filter propagation, including a NULL/default root.
    pub fn initialize_spatial(&mut self,pathfinder:&mut ai_pathfinder_constructor::State,memory:&mut Memory,context:super::ai_spatial_construction::Context,tree:&mut Tree,node:Option<usize>,backend:&mut impl super::ai_pathfinder_spatial_construction::Allocator)->Result<bool>{
        let Self::Spatial{address}=self else{anyhow::bail!("Arena spatial initialization requires an owned spatial modifier")};
        ensure!(usize::try_from(memory.word(address.wrapping_add(4))?)?==pathfinder.identity,"Spatial initialization owner differs");
        let mut definitions=super::ai_pathfinder_spatial_construction::Definitions{tree,root:node,context:super::ai_spatial_modifier::Context{root_slot:context.root_slot,manager_class:context.manager_class},backend};
        Ok(super::ai_spatial_construction::initialize(memory,context,*address,0,&mut super::ai_pathfinder_spatial_construction::Binding{state:pathfinder,backend:&mut definitions})?&255!=0)
    }
    pub fn initialize_definition(&mut self,tree:&mut Tree,node:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool> {
        match self{Self::FindNodes(s)=>s.initialize_definition(tree,node),Self::ComputeTarget(s)=>s.initialize_definition(tree,node,backend),Self::Steering(s)=>s.initialize_definition(tree,node,backend),Self::EdgeAwareness(s)=>s.initialize_definition(tree,node,backend),Self::NodeReached(s)=>s.initialize_definition(tree,node,backend),Self::NodeCandidate(s)=>s.initialize_definition(tree,node,backend),Self::Accident(s)=>s.initialize_definition(tree,node,backend),Self::Spatial{..}=>anyhow::bail!("Spatial definitions require arena-aware initialize_spatial"),Self::GoalDetection(state)=>state.initialize_definition(tree,node,backend),Self::GapAvoidance(state)=>ai_goto_gap_definition_binding::Binding::new(tree,backend).initialize(state,node),
            Self::RefineGoal(state)=>state.initialize_definition(tree,node)}
    }
}
