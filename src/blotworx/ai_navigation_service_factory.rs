//! Concrete construction from resolved Service/Class data. Initialization and
//! successful brain publication remain separate mandatory stages.
use super::{
    ai_navigation_action::Registry,
    ai_navigation_construction::Constructed,
    ai_navigation_definitions::Tree,
    ai_pathfinder_constructor::{self, Counter, Host},
};
use anyhow::Result;
pub enum Owned {
    Pathfinder(Box<ai_pathfinder_constructor::State>),
}
impl Owned {
    /// Verified51fb58 slot24 ->4597ac is blr. This is specific to the
    /// constructed CPathFinder class, not a fallback for unsupported services.
    pub fn world_inserted24(&mut self){match self{Self::Pathfinder(_)=>{}}}
    pub fn world_removed20<H:super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host>(
        &mut self,memory:&mut super::ai_graph_link_passes::Memory,
        modifiers:&mut std::collections::BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,host:&mut H)->Result<()> {
        match self{Self::Pathfinder(state)=>super::ai_pathfinder_world_lifecycle::removed20_concrete(state,memory,modifiers,host)}
    }
    /// Enclosing48fec4, using actual LPF/definition/modifier bindings. A real
    /// backend is required; World brain registration is a later operation.
    pub fn initialize_definition(&mut self,tree:&mut Tree,root:usize,
        shared:&mut super::ai_pathfinder_initialization::Shared,
        memory:&mut super::ai_graph_link_passes::Memory,
        backend:&mut impl super::ai_pathfinder_initialization::Backend)->Result<bool> {
        match self {Self::Pathfinder(state)=>super::ai_pathfinder_initialization::initialize(state,tree,root,shared,memory,backend)}
    }
    /// Actual default-modifier phase of48fec4; surrounding search allocations,
    ///reset and global registration are still separate required operations.
    pub fn initialize_defaults(&mut self,backend:&mut impl super::ai_pathfinder_modifiers::DefaultsHost)->Result<()> {
        match self {Self::Pathfinder(state)=>super::ai_pathfinder_modifiers::initialize_defaults(state,backend)}
    }
    /// Owned LPF portion of48fec4. This does not initialize the remaining
    /// pathfinder search storage/default modifiers or register an agent.
    pub fn initialize_lpf_definition(&mut self,tree:&mut Tree,root:usize,backend:&mut impl super::ai_lpf_definition_binding::Backend)->Result<bool> {
        match self {Self::Pathfinder(state)=>state.initialize_lpf_definition(tree,root,backend)}
    }
    /// Concrete tree/scalar/modifier pass. The derived48fec4 LPF and runtime
    /// allocations must still surround this pass before publishing a service.
    pub fn configure_definition(&mut self,tree:&mut Tree,root:usize,backend:&mut impl super::ai_pathfinder_definition_binding::Backend)->Result<bool> {
        match self {
            Self::Pathfinder(state)=>super::ai_pathfinder_definition_binding::Binding::new(tree,backend).initialize(state,root),
        }
    }
    ///492440 scalar/resource dispatch. The supplied host must implement the
    ///modifier continuation; unsupported modifiers may not be accepted silently.
    pub fn initialize_attribute(
        &mut self,
        node: usize,
        host: &mut impl super::ai_pathfinder_attributes::Host,
    ) -> Result<bool> {
        match self {
            Self::Pathfinder(state) => {
                super::ai_pathfinder_attributes::initialize(state.as_mut(), node, host)
            }
        }
    }
    /// The base465d74 pass, called by the derived initializer after its LPF
    /// setup. This does not replace48fec4 or publish a ready brain service.
    pub fn initialize_attributes(
        &mut self,
        root: usize,
        host: &mut impl super::ai_navigation_service_init::Host<ai_pathfinder_constructor::State>,
    ) -> Result<bool> {
        match self {
            Self::Pathfinder(state) => {
                super::ai_navigation_service_init::initialize(state.as_mut(), root, host)
            }
        }
    }
}
pub fn construct(
    tree: &Tree,
    node: usize,
    identity: usize,
    owner: &Constructed,
    action_identity: usize,
    registry: &Registry,
    counter: &mut Counter,
    host: &mut impl Host,
) -> Result<Option<Owned>> {
    let Some(class) = tree.first_value(node, b"Class")? else {
        return Ok(None);
    };
    let class = &class[..class.iter().position(|&b| b == 0).unwrap_or(class.len())];
    //459948 uses80019224 strcmp, unlike case-insensitive tree tag lookup.
    if class == b"Mpd::CPathFinder" {
        return Ok(Some(Owned::Pathfinder(Box::new(
            ai_pathfinder_constructor::State::construct(
                identity,
                owner,
                action_identity,
                registry,
                counter,
                host,
            )?,
        ))));
    }
    anyhow::bail!(
        "Navigation service Class is not implemented: {}",
        String::from_utf8_lossy(class)
    )
}
