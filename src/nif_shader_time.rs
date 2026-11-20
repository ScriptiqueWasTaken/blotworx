//! Runtime-only mesh time synchronization. Native PC NiTimeSyncController
//! publishes the model update's raw time, without scaling or wall-clock reads.
use anyhow::{ensure,Result};
#[derive(Debug,Clone,Copy)]
pub(crate) struct Clock {value:Option<f32>,flags:u16,last_time:f32}
impl Clock {
    pub fn new(existing:Option<f32>)->Result<Self> {
        ensure!(existing.is_none_or(f32::is_finite),"Nonfinite existing mesh shader time");
        Ok(Self {value:Some(existing.unwrap_or(0.)),flags:12,last_time:-f32::MAX})
    }
    pub fn value(&self)->Option<f32> {self.value}
    pub fn update(&mut self,time:f32,target_exists:bool)->Result<()> {
        ensure!(time.is_finite(),"Nonfinite mesh model update time");
        if !target_exists||self.value.is_none()||self.flags&8==0{return Ok(())}
        ensure!(self.flags&64==0,"Scaled mesh-time controller requires its controller adapter");
        if time==self.last_time&&self.flags&128==0{return Ok(())}
        self.flags&=!128;self.value=Some(time);Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_time_sync_matches_original_constructor_and_update()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../tools/blotworx/fixtures/pc_time_sync.json"))?;
        let initial=Clock::new(None)?;
        assert_eq!(initial.flags,reference["initial"]["flags"].as_u64().unwrap() as u16);
        assert_eq!(initial.last_time.to_bits(),reference["initial"]["last_time"].as_u64().unwrap() as u32);
        assert_eq!(initial.value(),Some(0.));
        for case in reference["cases"].as_array().unwrap() {
            let mut clock=Clock::new(Some(-11.))?;
            if !case["active"].as_bool().unwrap(){clock.flags&=!8;}
            if !case["extra"].as_bool().unwrap(){clock.value=None;}
            if let Some(time)=case["last_time"].as_f64(){clock.last_time=time as f32;}
            clock.update(case["time"].as_f64().unwrap() as f32,case["target"].as_bool().unwrap())?;
            if case["extra"].as_bool().unwrap() {assert_eq!(clock.value().unwrap().to_bits(),case["word"].as_u64().unwrap() as u32,"{case}");}
            else {assert!(clock.value().is_none());}
        }Ok(())
    }
}
