//! Common198b3c execution against canonical World task ownership. Concrete
//! behavior and completion callbacks are mandatory, never guessed defaults.
use super::{assets::World,ai_task_runtime::{self,Context,LiveHost,State}};
use anyhow::{ensure,Context as _,Result};
pub(super) type Update = fn(&mut World,usize,Context)->Result<u32>;
pub(super) type Complete = fn(&mut World,usize,Context)->Result<()>;
impl World {
    pub(super) fn update_ai_behavior_task(&mut self,owner:usize,slot:usize,delta:f32,update:Update,complete:Complete)->Result<u32> {
        ensure!(self.ai_brains.get(&owner).is_some_and(|brain|brain.selector.current==Some(slot)),"Common AI update requires its selected behavior");
        ensure!(!self.removed_entities.contains(&owner),"AI update owner was retired");
        ai_task_runtime::update_live(Context {owner,delta},&mut Binding {world:self,owner,slot,update,complete})
    }
}
struct Binding<'a> {world:&'a mut World,owner:usize,slot:usize,update:Update,complete:Complete}
impl Binding<'_> {
    fn state(&self)->Result<&State> {self.world.ai_brains.get(&self.owner).and_then(|brain|brain.behavior_tasks.get(&self.slot)).context("AI callback removed the canonical task owner")}
    fn state_mut(&mut self)->Result<&mut State> {self.world.ai_brains.get_mut(&self.owner).and_then(|brain|brain.behavior_tasks.get_mut(&self.slot)).context("AI callback removed the canonical task owner")}
}
impl LiveHost for Binding<'_> {
    fn child(&self)->Result<Option<usize>> {Ok(self.state()?.child)}
    fn set_child(&mut self,value:Option<usize>)->Result<()> {self.state_mut()?.child=value;Ok(())}
    fn set_previous_kind(&mut self,value:u32)->Result<()> {self.state_mut()?.previous_kind=value;Ok(())}
    fn mode(&self)->Result<u32> {Ok(self.state()?.mode)}
    fn set_mode(&mut self,value:u32)->Result<()> {self.state_mut()?.mode=value;Ok(())}
    fn timer(&self)->Result<f32> {Ok(self.state()?.timer)}
    fn set_timer(&mut self,value:f32)->Result<()> {self.state_mut()?.timer=value;Ok(())}
    fn kind(&mut self,child:usize)->Result<u32> {self.world.ai_child_kind(self.owner,child)}
    fn stop(&mut self,child:usize,owner:usize)->Result<()> {ensure!(owner==self.owner,"AI child owner mismatch");self.world.stop_ai_child(owner,child)}
    fn destroy(&mut self,child:usize)->Result<()> {self.world.destroy_ai_child(self.owner,child)}
    fn update_task(&mut self,child:usize,context:Context)->Result<u32> {
        let brain=self.world.ai_brains.get(&self.owner).context("AI brain missing")?;
        if brain.animation_waits.contains_key(&child) {self.world.update_ai_animation_wait(self.owner,child,context.delta)}
        else if brain.turns.contains_key(&child) {self.world.update_ai_turn(self.owner,child)}
        else {anyhow::bail!("AI update requires a constructed concrete child task")}
    }
    fn child_completed(&mut self,context:Context)->Result<()> {(self.complete)(self.world,self.slot,context)}
    fn update_behavior(&mut self,context:Context)->Result<u32> {(self.update)(self.world,self.slot,context)}
}
