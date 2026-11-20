//! Mpd::CDetectAccident_PredictMove. Native1c6bf8 attributes,1c6ce8 init,
//!1c6d48 reset,1c6d90 detection and1c7038 movement recording.
use anyhow::{Context,Result};
#[derive(Clone,Copy,Debug)]pub enum Clock{Stamp58,Delta5c,Stamp50}
#[derive(Clone,Copy)]pub struct Request{pub changed:u8,pub value:f32}
pub struct State{
 pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub ratio:Option<f32>,
 pub predicted:[f32;3],pub stamp:f32,pub remaining:Option<f32>,pub travelled:Option<f32>,
 pub target:[f32;3],pub measured:[f32;3],pub predicted_distance:Option<f32>,
}
pub trait DefinitionBackend{fn number(&mut self,state:&mut State,text:Option<&[u8]>)->Result<f64>;}
pub trait Host{
 fn clock(&mut self,state:&mut State,field:Clock)->Result<f32>;
 fn enabled(&mut self,state:&mut State,parent:usize)->Result<u8>;
 fn target(&mut self,state:&mut State,parent:usize)->Result<[f32;3]>;
 fn entity(&mut self,state:&mut State,parent:usize)->Result<usize>;
 fn position(&mut self,state:&mut State,entity:usize)->Result<[f32;3]>;
 fn speed(&mut self,state:&mut State,parent:usize)->Result<Option<Request>>;
 fn heading(&mut self,state:&mut State,parent:usize)->Result<Option<Request>>;
 fn default_speed(&mut self,state:&mut State,parent:usize)->Result<f32>;
 fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>>;
 fn zero(&mut self,state:&mut State)->Result<[f32;3]>;
}
fn distance(a:[f32;3],b:[f32;3])->f32{let [x,y,z]=std::array::from_fn(|i|a[i]-b[i]);super::cursor::native_sqrt(y.mul_add(y,z.mul_add(z,x*x)))}
impl State{
 pub const CLASS:&'static [u8]=b"Mpd::CDetectAccident_PredictMove";
 pub fn construct(identity:usize,pathfinder:usize,name:&[u8])->Self{Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),ratio:None,predicted:[0.;3],stamp:0.,remaining:None,travelled:None,target:[0.;3],measured:[0.;3],predicted_distance:None}}
 pub fn reset(&mut self){self.predicted=[f32::MAX;3];self.stamp=0.;self.remaining=Some(0.);self.travelled=Some(0.);self.target=[f32::MAX;3];self.measured=[f32::MAX;3];self.predicted_distance=Some(0.);}
 fn retarget(&mut self,parent:usize,target:[f32;3],host:&mut impl Host)->Result<()>{
  let entity=host.entity(self,parent)?;let position=host.position(self,entity)?;
  self.travelled=Some(0.);self.remaining=Some(distance(target,position));
  let entity=host.entity(self,parent)?;self.measured=host.position(self,entity)?;self.target=target;Ok(())
 }
 pub fn detect(&mut self,host:&mut impl Host)->Result<u32>{
  let stamp=host.clock(self,Clock::Stamp58)?;if self.stamp!=stamp{self.reset();return Ok(0)}
  let parent=self.pathfinder;if host.enabled(self,parent)?==0{return Ok(0)}
  let target=host.target(self,parent)?;let mut accident=false;
  if self.predicted[0]!=f32::MAX{
   let entity=host.entity(self,parent)?;let position=host.position(self,entity)?;let x=position[0]-self.predicted[0];let z=position[2]-self.predicted[2];
   let error=super::cursor::native_sqrt(z.mul_add(z,x*x));
   let expected=self.predicted_distance.context("Accident prediction distance is unwritten")?*self.ratio.context("Accident ratio is unwritten")?;
   if error>expected{
    let scale=host.unit_scale(self)?.unwrap_or(1.);
    if error>f32::from_bits(0x3dcccccd)*scale{self.retarget(parent,target,host)?;accident=true;}
   }
  }
  if !accident{
   let scale=host.unit_scale(self)?.unwrap_or(1.);
   if self.travelled.context("Accident travelled distance is unwritten")?>4.*(self.remaining.context("Accident target distance is unwritten")?+scale){self.retarget(parent,target,host)?;accident=true;}
  }
  let entity=host.entity(self,parent)?;self.predicted=host.position(self,entity)?;self.predicted_distance=Some(0.);Ok(u32::from(accident))
 }
 pub fn record(&mut self,host:&mut impl Host)->Result<()>{
  let stamp=host.clock(self,Clock::Stamp58)?;if self.stamp!=stamp{self.reset();}
  let parent=self.pathfinder;let entity=host.entity(self,parent)?;
  if self.predicted[0]==f32::MAX{self.predicted=host.position(self,entity)?;}
  if self.measured[0]==f32::MAX{self.measured=host.position(self,entity)?;}
  let enabled=host.enabled(self,parent)?;let target=host.target(self,parent)?;
  if enabled==1 && (target[0]!=self.target[0] || target[1]!=self.target[1] || target[2]!=self.target[2]){
   let e=host.entity(self,parent)?;let p=host.position(self,e)?;let remaining=distance(target,p);
   self.remaining=Some(remaining);self.travelled=Some(0.);self.target=target;
   let e=host.entity(self,parent)?;self.measured=host.position(self,e)?;
  }
  let dt=host.clock(self,Clock::Delta5c)?;
  let speed=match host.speed(self,parent)?{None=>host.default_speed(self,parent)?,Some(r) if r.changed!=0=>r.value,Some(_)=>0.};
  let heading=host.heading(self,parent)?;
  let direction=if let Some(r)=heading.filter(|r|r.changed!=0){
   let radians=r.value*f32::from_bits(0x3c8efa35);let (s,c)=super::native_angle::sin_cos_medium(radians as f64)?;
   [s as f32,0.,c as f32]
  }else{
   let target=host.target(self,parent)?;let p=host.position(self,entity)?;let [x,y,z]=std::array::from_fn(|i|target[i]-p[i]);
   let length=super::cursor::native_sqrt(z.mul_add(z,x.mul_add(x,y*y)));
   if length==0.{host.zero(self)?}else{[x/length,y/length,z/length]}
  };
  let step=dt*speed;let movement=direction.map(|v|step*v);
  self.predicted_distance=Some(self.predicted_distance.context("Accident prediction distance is unwritten")?+step);
  self.predicted=std::array::from_fn(|i|self.predicted[i]+movement[i]);
  let p=host.position(self,entity)?;let travelled=distance(self.measured,p);
  self.travelled=Some(self.travelled.context("Accident travelled distance is unwritten")?+travelled);
  self.measured=host.position(self,entity)?;
  self.stamp=host.clock(self,Clock::Stamp50)?;Ok(())
 }
}
#[cfg(test)]
pub(in crate::blotworx) mod tests {
 use super::*;use serde_json::{Value,json};
 pub(in crate::blotworx) fn from_fields(case:&Value)->State{let words:Vec<f32>=case["input"]["fields"].as_array().unwrap().iter().map(|v|f32::from_bits(v.as_u64().unwrap() as u32)).collect();
  State{identity:1,pathfinder:1,name:b"test".to_vec(),ratio:Some(words[0]),predicted:words[1..4].try_into().unwrap(),stamp:words[4],remaining:Some(words[5]),travelled:Some(words[6]),target:words[7..10].try_into().unwrap(),measured:words[10..13].try_into().unwrap(),predicted_distance:Some(words[13])}
 }
 pub(in crate::blotworx) fn fields(s:&State)->Vec<u32>{[vec![s.ratio.unwrap()],s.predicted.to_vec(),vec![s.stamp,s.remaining.unwrap(),s.travelled.unwrap()],s.target.to_vec(),s.measured.to_vec(),vec![s.predicted_distance.unwrap()]].concat().into_iter().map(f32::to_bits).collect()}
 struct Runtime<'a>{input:&'a Value,trace:Vec<Value>,target:[[f32;3];2],entity:usize,positions:[[f32;3];2],clock:[f32;3]}
 impl Runtime<'_>{fn vector(&self,key:&str)->[f32;3]{std::array::from_fn(|i|f32::from_bits(self.input[key][i].as_u64().unwrap() as u32))}}
 impl Host for Runtime<'_>{
  fn clock(&mut self,s:&mut State,f:Clock)->Result<f32>{let (index,offset)=match f{Clock::Stamp58=>(0,0x58),Clock::Delta5c=>(1,0x5c),Clock::Stamp50=>(2,0x50)};self.trace.push(json!(["clock",offset]));if self.input["mutation"]=="clock_parent" && index==0{s.pathfinder=2;}Ok(self.clock[index])}
  fn enabled(&mut self,_:&mut State,_:usize)->Result<u8>{Ok(self.input["enabled"].as_u64().unwrap() as u8)}
  fn target(&mut self,_:&mut State,p:usize)->Result<[f32;3]>{Ok(self.target[p-1])}
  fn entity(&mut self,_:&mut State,_:usize)->Result<usize>{Ok(self.entity)}
  fn position(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{Ok(self.positions[e-1])}
  fn speed(&mut self,_:&mut State,_:usize)->Result<Option<Request>>{self.trace.push(json!(["speed"]));if self.input["mutation"]=="speed_target"{self.target[0]=[3.,4.,2.];}Ok(self.input["speed"].as_array().map(|r|Request{changed:r[0].as_u64().unwrap() as u8,value:f32::from_bits(r[1].as_u64().unwrap() as u32)}))}
  fn heading(&mut self,_:&mut State,_:usize)->Result<Option<Request>>{self.trace.push(json!(["heading"]));if self.input["mutation"]=="heading_entity"{self.entity=2;}Ok(self.input["heading"].as_array().map(|r|Request{changed:r[0].as_u64().unwrap() as u8,value:f32::from_bits(r[1].as_u64().unwrap() as u32)}))}
  fn default_speed(&mut self,_:&mut State,_:usize)->Result<f32>{Ok(f32::from_bits(self.input["default_speed"].as_u64().unwrap() as u32))}
  fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["scale"]));Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
  fn zero(&mut self,_:&mut State)->Result<[f32;3]>{Ok(self.vector("zero"))}
 }
 #[test]
 fn prediction_and_detection_match_original_actions_math_and_retained_owners()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_predict_move.json"))?;
  for (i,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let input=&case["input"];if input["op"]=="init"{continue}
   let mut state=from_fields(case);let target=std::array::from_fn(|i|f32::from_bits(input["target"][i].as_u64().unwrap() as u32));let position=std::array::from_fn(|i|f32::from_bits(input["position"][i].as_u64().unwrap() as u32));let clock=std::array::from_fn(|i|f32::from_bits(input["clock"][i].as_u64().unwrap() as u32));let mut host=Runtime{input,trace:vec![],target:[target;2],entity:1,positions:[position,[5.,1.,2.]],clock};
   let result=match input["op"].as_str().unwrap(){"detect"=>json!(state.detect(&mut host)?),"record"=>{state.record(&mut host)?;Value::Null},"reset"=>{state.reset();Value::Null},"sequence"=>{state.record(&mut host)?;if input["follow"]==true{host.positions[0]=state.predicted;}host.clock[0]=state.stamp;host.clock[2]=0.2;json!(state.detect(&mut host)?)},_=>unreachable!()};
   assert_eq!(json!(fields(&state)),case["fields"],"case {i}: {input}");assert_eq!(result,case["result"],"case {i}");assert_eq!(json!(host.trace),case["trace"],"case {i}");assert_eq!(json!(state.pathfinder),case["parent"],"case {i}");
  }Ok(())
 }
}
