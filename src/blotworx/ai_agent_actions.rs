//! Canonical storage for private agent actions. Shared +4 references alias the
//! parent brain's actual +18 action; no clone or second action image is made.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_navigation_action::{Action,Registry},
    ai_navigation_construction::{ActionClass,ActionFactory,Constructed}};
pub struct Private {pub owner:usize,pub class:usize,pub action:Action}
#[derive(Default)]
pub struct Store {pub objects:BTreeMap<usize,Private>}
impl Store {
    pub fn has_owner(&self,owner:usize)->bool{self.objects.values().any(|a|a.owner==owner)}
}
/// Borrow the existing parent fields even during base-brain initialization.
/// In particular this does not require moving/rebuilding Constructed around an
/// active call to State.initialize.
pub struct Parent<'a> {
    pub brain:&'a super::ai_navigation_brain::Brain,
    pub base:&'a mut super::ai_navigation_brain_base::State,
    pub action:&'a mut Option<Action>,
}
impl<'a> From<&'a mut Constructed> for Parent<'a> {
    fn from(p:&'a mut Constructed)->Self{Self{brain:&p.brain,base:&mut p.base,action:&mut p.action}}
}
pub trait ConstructionHost {
    ///18cdb0..18ce18 publishes the constructed vtable once to class metadata
    /// and the shared class registry. This is required, not implied by new().
    fn action_class_constructed(&mut self,actions:&mut View<'_>,identity:usize)->Result<()>;
}
pub struct View<'a> {
    pub parent:Parent<'a>,
    pub private:&'a mut Store,
    pub class:&'a ActionClass,
    pub registry:&'a Registry,
    pub next_identity:&'a mut usize,
}
impl View<'_> {
    pub fn reborrow(&mut self)->View<'_> {
        View{parent:Parent{brain:self.parent.brain,base:self.parent.base,action:self.parent.action},
            private:self.private,class:self.class,registry:self.registry,next_identity:self.next_identity}
    }
    pub fn owner_action(&self,owner:usize)->Result<Option<usize>> {
        ensure!(owner==self.parent.brain.identity,"Agent action owner differs from the canonical brain");
        Ok(self.parent.base.action)
    }
    pub fn action_mut(&mut self,id:usize)->Result<&mut Action> {
        if self.parent.base.action==Some(id) {
            return self.parent.action.as_mut().context("Shared brain action handle has no canonical object")
        }
        let a=self.private.objects.get_mut(&id).context("Private agent action is not owned")?;
        ensure!(a.owner==self.parent.brain.identity,"Private agent action belongs to a different brain");
        Ok(&mut a.action)
    }
    pub fn action_class(&self,id:usize)->Result<usize> {
        if self.parent.base.action==Some(id) {
            ensure!(self.parent.action.is_some(),"Shared action metadata has no canonical object");
            return Ok(self.class.metadata)
        }
        let a=self.private.objects.get(&id).context("Agent action metadata is not owned")?;
        ensure!(a.owner==self.parent.brain.identity,"Agent action metadata belongs to a different brain");
        Ok(a.class)
    }
    pub fn construct(&mut self,class:usize,host:&mut impl ConstructionHost)->Result<usize> {
        ensure!(class==self.class.metadata,"Agent action factory metadata is not implemented");
        match self.class.factory.as_ref().context("Agent action class has no factory")? {
            ActionFactory::BaseAi=>{},
            ActionFactory::Unsupported(name)=>anyhow::bail!("Agent action factory is not implemented: {name}"),
        }
        let id=self.next_identity.checked_add(1).context("Navigation identities exhausted")?;
        ensure!(self.parent.base.action!=Some(id) && !self.private.objects.contains_key(&id),"Agent action identity is already owned");
        *self.next_identity=id;
        let action=Action::new(self.registry)?;
        self.private.objects.insert(id,Private{owner:self.parent.brain.identity,class,action});
        host.action_class_constructed(self,id)?;
        Ok(id)
    }
    pub fn acknowledge(&mut self,id:usize)->Result<()> {self.action_mut(id)?.acknowledge();Ok(())}
    pub fn destroy_private(&mut self,id:usize)->Result<()> {
        ensure!(self.parent.base.action!=Some(id),"Agent cannot delete its shared brain action");
        let action=self.private.objects.get(&id).context("Destroyed private action is not owned")?;
        ensure!(action.owner==self.parent.brain.identity,"Destroyed private action belongs to a different brain");
        self.private.objects.remove(&id);Ok(())
    }
}
