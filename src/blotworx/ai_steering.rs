//! Retail SimpleBiped steering1cc8e8..1ccefc. Requests are retained slot
//! identities, not copied action scalars. This does not register a World agent.
use anyhow::{Context,Result};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Request{Heading,Speed,Steering,Acceleration,TorsoRotate}
impl Request{pub fn index(self)->usize{match self{Self::Heading=>0,Self::Speed=>1,Self::Steering=>2,Self::Acceleration=>3,Self::TorsoRotate=>4}}}
pub struct State{pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub radius:Option<f32>,pub angle:Option<f32>,pub strafe:u8}
pub trait DefinitionBackend{
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
 fn number(&mut self,s:&mut State,text:Option<&[u8]>)->Result<f64>;
}
pub trait Host{
 fn position(&mut self,s:&mut State,entity:usize)->Result<[f32;3]>;
 fn point(&mut self,s:&mut State,point:usize)->Result<[f32;3]>;
 fn heading(&mut self,s:&mut State,entity:usize)->Result<f32>;
 fn request(&mut self,s:&mut State,action:usize,kind:Request)->Result<Option<usize>>;
 fn torso_orientation(&mut self,s:&mut State,entity:usize)->Result<Option<[f32;3]>>;
 fn forward(&mut self,s:&mut State,entity:usize)->Result<[f32;3]>;
 fn target(&mut self,s:&mut State,parent:usize)->Result<[f32;3]>;
 fn current_speed(&mut self,s:&mut State,entity:usize)->Result<f32>;
 fn zero(&mut self,s:&mut State)->Result<[f32;3]>;
 fn write(&mut self,s:&mut State,request:usize,offset:u32,bits:u32)->Result<()>;
 fn changed(&mut self,s:&mut State,request:usize,value:u8)->Result<()>;
}
pub fn cosine(value:f32)->Result<f32>{Ok(super::native_angle::sin_cos_medium(value as f64)?.1 as f32)}
impl State{
 pub const CLASS:&'static [u8]=b"Mpd::CSteering_SimpleBiped";
 pub fn construct(identity:usize,pathfinder:usize,name:&[u8],host:&mut impl DefinitionBackend)->Result<Self>{
  let mut s=Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),radius:None,angle:None,strafe:0};let scale=host.unit_scale(&mut s)?.unwrap_or(1.);s.radius=Some(2.*scale);s.angle=Some(cosine(f32::from_bits(0x3db2b8c2))?);Ok(s)
 }
 pub fn reset(&mut self){self.strafe=0;}
 pub fn probe20(&self)->u32{0}
 pub fn query(&mut self,entity:usize,target:usize,mut speed:f32,action:usize,host:&mut impl Host)->Result<u32>{
  let destination=host.point(self,target)?;let source=host.position(self,entity)?;
  if destination.iter().zip(source).all(|(a,b)|*a==b){return Ok(1)}
  let [dx,dy,dz]=std::array::from_fn(|i|destination[i]-source[i]);
  let distance=super::cursor::native_sqrt(dz.mul_add(dz,dx.mul_add(dx,dy*dy)));
  let desired=super::native_direction::angles([dx,0.,dz],||host.zero(self))?[1];
  let mut error=desired-host.heading(self,entity)?;if error>180.{error-=360.}else if error< -180.{error+=360.}
  speed*=cosine(f32::from_bits(0x3c8efa35)*(f32::from_bits(0x3eef9db2)*error))?;
  if let Some(torso)=host.request(self,action,Request::TorsoRotate)?{
   let multiplier=if self.strafe!=0{f32::from_bits(0x3f8ccccd)}else{1.};let radius=self.radius.context("Steering radius is unwritten")?*multiplier;let angle=self.angle.context("Steering angle is unwritten")?*multiplier;self.strafe=0;
   if distance>0. && distance<radius{
    let direction=if let Some(v)=host.torso_orientation(self,entity)?{v}else{host.forward(self,entity)?};
    if dz.mul_add(direction[2],dx*direction[0])<angle*distance{
     self.strafe=1;let parent=self.pathfinder;let target=host.target(self,parent)?;let source=host.position(self,entity)?;
     let angles=super::native_direction::angles(std::array::from_fn(|i|target[i]-source[i]),||host.zero(self))?;
     host.write(self,torso,12,angles[1].to_bits())?;host.changed(self,torso,1)?;host.write(self,torso,8,angles[0].to_bits())?;host.changed(self,torso,1)?;
    }
   }
  }
  if let Some(heading)=host.request(self,action,Request::Heading)?{host.write(self,heading,8,desired.to_bits())?;host.changed(self,heading,1)?;}
  else{let Some(steering)=host.request(self,action,Request::Steering)? else{return Ok(0)};let value=if error>0.{1f32}else{-1.};host.write(self,steering,8,value.to_bits())?;host.changed(self,steering,1)?;}
  if let Some(request)=host.request(self,action,Request::Speed)?{host.write(self,request,8,speed.to_bits())?;host.changed(self,request,1)?;}
  else{let Some(acceleration)=host.request(self,action,Request::Acceleration)? else{return Ok(0)};let value=if speed>host.current_speed(self,entity)?{1f32}else{-1.};host.write(self,acceleration,8,value.to_bits())?;host.changed(self,acceleration,1)?;}
  Ok(1)
 }
}
#[cfg(test)]pub(super) mod tests{
 use super::*;use serde_json::{Value,json};
 pub struct Services<'a>{pub input:&'a Value,pub trace:Vec<Value>,pub requests:[[u32;3];5]}
 impl Services<'_>{pub fn scalar(&self,key:&str)->f32{f32::from_bits(self.input[key].as_u64().unwrap() as u32)}pub fn vector(&self,key:&str)->[f32;3]{std::array::from_fn(|i|f32::from_bits(self.input[key][i].as_u64().unwrap() as u32))}}
 impl DefinitionBackend for Services<'_>{fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["scale"]));Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{self.trace.push(json!(["number"]));Ok(f64::from_bits(self.input["number"].as_u64().unwrap()))}}
 impl Host for Services<'_>{
 fn position(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(self.vector("position"))}
 fn point(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(self.vector("target"))}
 fn heading(&mut self,_:&mut State,_:usize)->Result<f32>{Ok(self.scalar("heading"))}
 fn request(&mut self,_:&mut State,_:usize,r:Request)->Result<Option<usize>>{let i=r.index();self.trace.push(json!(["lookup",i]));Ok(self.input["present"][i].as_bool().unwrap().then_some(i))}
 fn torso_orientation(&mut self,_:&mut State,_:usize)->Result<Option<[f32;3]>>{self.trace.push(json!(["direction"]));Ok(self.input["direction"].as_array().map(|_|self.vector("direction")))}
 fn forward(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(self.vector("forward"))}
 fn target(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(self.vector("parent_target"))}
 fn current_speed(&mut self,_:&mut State,_:usize)->Result<f32>{Ok(self.scalar("current_speed"))}
 fn zero(&mut self,_:&mut State)->Result<[f32;3]>{Ok([0.;3])}
 fn write(&mut self,_:&mut State,r:usize,o:u32,b:u32)->Result<()>{self.trace.push(json!(["write",r,o,b]));self.requests[r][if o==8{1}else{2}]=b;Ok(())}
 fn changed(&mut self,_:&mut State,r:usize,b:u8)->Result<()>{self.trace.push(json!(["write",r,4,b]));self.requests[r][0]=b as u32;Ok(())}
 }
 pub fn state(input:&Value)->State{State{identity:1,pathfinder:2,name:b"biped".to_vec(),radius:Some(f32::from_bits(input["radius"].as_u64().unwrap() as u32)),angle:Some(f32::from_bits(input["angle"].as_u64().unwrap() as u32)),strafe:input["flag"].as_u64().unwrap() as u8}}
 pub fn host(input:&Value)->Services<'_>{Services{input,trace:vec![],requests:[[2,0x40400000,0x40800000];5]}}
 #[test]fn simple_biped_runtime_matches_original_steering_and_math()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_steering.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];if c["op"]!="query"{continue}let mut s=state(c);let mut h=host(c);let result=s.query(10,20,h.scalar("speed"),30,&mut h)?;
   assert_eq!(result,case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.strafe,case["flag"].as_u64().unwrap() as u8,"{c}");assert_eq!(json!(h.requests),case["requests"],"{c}");assert_eq!(json!(h.trace),case["trace"],"{c}");
  }Ok(())
 }
}

