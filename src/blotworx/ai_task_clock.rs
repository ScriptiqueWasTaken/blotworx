//!Process-global task clock callbacks and accounting, corresponding to4e128,
//!4a510/14 and the frame-sort globals. They never belong to an individual NPC.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum Getter {#[default]Absent,External(usize),Deterministic}
#[derive(Debug,Default)]
pub struct Globals {
    pub getter:Getter,pub reset:Option<usize>,pub estimates:[f32;2],
    pub frame:super::ai_task_frame::Globals,
}
pub trait Backend {
    fn sample(&mut self,callback:usize)->anyhow::Result<f32>;
    fn reset(&mut self,callback:usize)->anyhow::Result<()>;
}
