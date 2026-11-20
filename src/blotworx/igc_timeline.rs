//! PC af4dc0 scene clock. Event receivers and package transitions are separate.
use anyhow::{ensure,Result};
#[derive(Default,Debug,serde::Serialize)]
pub(super) struct Clock {
    pub elapsed:f32,pub frames:u32,pub horizon:f32,pub complete:bool,
}
impl Clock {
    pub fn update(&mut self,delta:f32,disabled:bool,blocking:bool)->Result<bool> {
        ensure!(delta.is_finite(),"Nonfinite IGC delta");
        if disabled {self.complete=true;return Ok(false);}
        if blocking||self.complete{return Ok(false);}
        if delta>0. {self.frames=self.frames.wrapping_add(1);self.elapsed+=delta;}
        // PC retains elapsed in float32, computes the half-delta lookahead in
        // x87, then stores that threshold in float32 before comparisons.
        self.horizon=(self.elapsed as f64+delta as f64*0.5)as f32;
        ensure!(self.elapsed.is_finite()&&self.horizon.is_finite(),"IGC clock overflow");
        Ok(true)
    }
    pub fn due(&self,time_based:bool,frame:u32,time:f32)->bool {
        if time_based {self.horizon>=time}else{self.frames>=frame}
    }
    pub fn finish(&mut self,time_based:bool,frames:u32,duration:f32) {
        // Native clamps the duration's upper bound to90 seconds, not its lower
        // bound. Frame-mode completion still uses the authored frame count.
        if if time_based {self.horizon>=duration.min(90.)}else{self.frames>=frames} {self.complete=true;}
    }
}
