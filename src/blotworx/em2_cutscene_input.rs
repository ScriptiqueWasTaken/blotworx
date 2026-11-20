//! PC f15420/f15160 counters and a45f40 stacking semantics for the diagnostic
//! main-game/cutscene path. Full GameState boot and state-entry event receivers
//! are separate work; this must not stand in for the complete game FSM.
use anyhow::{ensure,Result};

#[derive(Default,Debug,serde::Serialize)]
pub(super) struct Slot {
    pub count:u32,
    pub input_locks:i32,
    pub deferred_updates:u32,
}
#[derive(Debug,serde::Serialize)]
pub(super) struct State {
    pub slots:[Slot;2],
    pub context_stack:Vec<u32>,
}
impl State {
    pub fn main_playtest()->Self {Self {slots:Default::default(),context_stack:vec![0xa017]}}
    pub fn current(&self)->u32 {*self.context_stack.last().expect("GameState stack cannot be empty")}
    pub fn restricted(&self,index:usize)->bool {self.current()==3||self.slots[index].input_locks>0}
    pub fn apply(&mut self,index:usize,restrict:bool)->Result<()> {
        ensure!(index<2,"Invalid PC player input slot {index}");
        let mode=self.current();
        if restrict {
            if mode==1||mode==7{return Ok(());}
            ensure!(mode==0xa017||mode==3,"PC restriction transition from GameState {mode:#x} is not connected");
            let slot=&mut self.slots[index];
            if mode!=3 {slot.input_locks=slot.input_locks.wrapping_add(1);}
            slot.count=slot.count.wrapping_add(1);
            // Native InGameCutSceneState::ShouldStack returns true. Repeated
            // pushes of3 are retained; releasing an inner call pops only one.
            self.context_stack.push(3);
        } else {
            let slot=&mut self.slots[index];
            if slot.count==0{return Ok(());}
            if slot.count==1 {slot.input_locks=slot.input_locks.wrapping_sub(1).max(0);}
            slot.count-=1;
            if mode==3 {
                ensure!(self.context_stack.len()>1,"PC cutscene release has no previous GameState");
                self.context_stack.pop();
            } else {slot.deferred_updates=slot.deferred_updates.wrapping_add(1);}
        }
        Ok(())
    }
}
