//! PC101a8c0 ARFilter. The caller owns TimeManager values and input dispatch.
use anyhow::{ensure,Result};
#[derive(Clone,Debug,serde::Serialize)]
pub(super) struct State {pub ramp_up:f32,pub ramp_down:f32,pub current:f32,pub last_input:f32}
impl State {
 pub fn new(ramp_up:f32,ramp_down:f32)->Result<Self> {
  ensure!(ramp_up.is_finite()&&ramp_down.is_finite(),"Nonfinite PC ARFilter coefficients");
  Ok(Self {ramp_up,ramp_down,current:0.,last_input:0.})
 }
 pub fn filter(&mut self,input:f32,game_delta:f32,fixed_rate:u32)->Result<f32> {
  ensure!(input.is_finite()&&game_delta.is_finite(),"Nonfinite PC ARFilter input/time");
  // Native unsigned frame count converts through x87 before float storage.
  let scale=if fixed_rate==0 {1.}else{(game_delta as f64*fixed_rate as f64) as f32};
  let reversing=self.current!=0.&&(self.current>0.)!=(input>0.);
  let rate=if input==0.||reversing {self.ramp_down}else{self.ramp_up};
  let weight=(rate as f64*scale as f64)as f32;
  let sign=if input==0.{0.}else if input>0.{1.}else{-1.};
  self.current=(self.current as f64+(sign-self.current as f64)*weight as f64)as f32;
  // Zero input keeps the last nonzero amplitude while its sign decays.
  if input!=0. {self.last_input=input;}
  let output=(self.current as f64*self.last_input.abs() as f64)as f32;
  ensure!(output.is_finite(),"PC ARFilter output overflow");Ok(output)
 }
}
