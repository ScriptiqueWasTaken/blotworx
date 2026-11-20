//! Shared player-slot constraint ownership. Version adapters supply component
//! admission, initial handles and concrete changed-handle receivers.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone,Copy,Debug,PartialEq,Eq,serde::Serialize)]
pub(super) enum Kind { Buddy, Player }
#[derive(Clone,Copy,Debug,serde::Serialize)]
pub(super) struct Slot { pub active:bool, pub handle:u32 }
#[derive(Debug,serde::Serialize)]
struct Cache { kind:Kind, handle:u32 }
#[derive(Debug,serde::Serialize)]
struct Pending { slot:usize, active:bool, owner:Option<usize>, handle:u32, cursor:usize }
#[derive(Debug,serde::Serialize)]
pub(super) struct State {
    pub slots:Vec<Slot>,
    owners:BTreeMap<usize,Vec<Cache>>,
    pending:Option<Pending>,
    pub dispatches:u64,
}
impl State {
    pub fn new(slots:Vec<Slot>)->Self { Self {slots,owners:BTreeMap::new(),pending:None,dispatches:0} }
    pub fn register(&mut self,owner:usize,components:&[(Kind,u32)])->Result<()> {
        ensure!(!self.owners.contains_key(&owner),"Constraint component owner already admitted");
        for (index,(kind,_)) in components.iter().enumerate() {
            ensure!(!components[..index].iter().any(|(previous,_)|previous==kind),"Duplicate constraint component");
        }
        self.owners.insert(owner,components.iter().map(|&(kind,handle)|Cache {kind,handle}).collect());Ok(())
    }
    pub fn set_active(&mut self,slot:usize,active:bool,owner:Option<usize>,mut receive:impl FnMut(usize,Kind,u32,u32)->Result<()>)->Result<()> {
        let state=self.slots.get_mut(slot).context("Invalid player constraint slot")?;
        if let Some(pending)=self.pending.as_ref() {
            ensure!(pending.slot==slot&&pending.active==active&&pending.owner==owner,"A different player constraint update is pending");
        }else{
            // Native slot write precedes entity lookup and component calls.
            state.active=active;
            self.pending=Some(Pending {slot,active,owner,handle:if active {state.handle}else{0},cursor:0});
        }
        let pending=self.pending.as_mut().unwrap();
        if let Some(owner)=pending.owner {
            let components=self.owners.get_mut(&owner).context("Player constraint component lifecycle is not admitted")?;
            while let Some(component)=components.get_mut(pending.cursor) {
                receive(owner,component.kind,component.handle,pending.handle)?;
                component.handle=pending.handle;
                pending.cursor+=1;self.dispatches+=1;
            }
        }
        self.pending=None;Ok(())
    }
}

#[cfg(test)]
#[path="../../tools/blotworx/tests/player_constraint_cases.rs"]
mod tests;
