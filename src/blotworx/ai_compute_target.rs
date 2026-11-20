//! Complete Queue target projection1c5ca8..1c6a68. Actor/services are live
//! references; iteration is bounded only by the native world actor count.
use anyhow::{Context,Result};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Attribute{Height,Diameter,Length,TorsoOrientation}
impl Attribute{pub fn index(self)->usize{match self{Self::Height=>0,Self::Diameter=>1,Self::Length=>2,Self::TorsoOrientation=>3}}}
pub struct State{pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub predicate:Option<usize>,pub profiles:Option<Option<usize>>,pub maxdist:Option<f32>,pub entitydist:Option<f32>,pub extra:Option<f32>,pub ratio:Option<f32>,pub actor:Option<usize>,pub axis:[f32;3]}
pub trait DefinitionBackend{fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;fn profiles(&mut self,s:&mut State)->Result<Option<usize>>;fn number(&mut self,s:&mut State,text:Option<&[u8]>)->Result<f64>;}
pub trait Host{
 type Attribute:Copy;
 fn read(&mut self,s:&mut State,point:usize,offset:u32)->Result<u32>;
 fn write(&mut self,s:&mut State,point:usize,offset:u32,value:u32)->Result<()>;
 fn parent_target(&mut self,s:&mut State,parent:usize)->Result<[f32;3]>;
 fn parent_entity(&mut self,s:&mut State,parent:usize)->Result<usize>;
 fn position(&mut self,s:&mut State,actor:usize)->Result<[f32;3]>;
 fn attribute(&mut self,s:&mut State,actor:usize,kind:Attribute)->Result<Option<Self::Attribute>>;
 fn scalar(&mut self,s:&mut State,attribute:Self::Attribute)->Result<f32>;
 fn vector(&mut self,s:&mut State,attribute:Self::Attribute)->Result<[f32;3]>;
 fn forward(&mut self,s:&mut State,actor:usize)->Result<[f32;3]>;
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
 fn up(&mut self,s:&mut State)->Result<[f32;3]>;
 fn zero(&mut self,s:&mut State)->Result<[f32;3]>;
 fn actor_limit(&mut self,s:&mut State)->Result<i32>;
 fn tolerance(&mut self,s:&mut State)->Result<f32>;
 fn first(&mut self,s:&mut State,profiles:usize,position:[f32;3],distance:f32)->Result<Option<usize>>;
 fn next(&mut self,s:&mut State,profiles:usize)->Result<Option<usize>>;
 fn all_first(&mut self,s:&mut State)->Result<Option<usize>>;
 fn all_actor(&mut self,s:&mut State,node:usize)->Result<usize>;
 fn all_next(&mut self,s:&mut State,node:usize)->Result<Option<usize>>;
 fn filter(&mut self,s:&mut State,predicate:usize,current:usize,other:usize)->Result<u32>;
 fn intersection(&mut self,s:&mut State,a:[f32;3],b:[f32;3],c:[f32;3],d:[f32;3],out:&mut[f32;3])->Result<bool>;
 fn flush(&mut self,s:&mut State)->Result<()>;
}
fn dot(a:[f32;3],b:[f32;3])->f32{a[2].mul_add(b[2],a[0].mul_add(b[0],a[1]*b[1]))}
fn sub(a:[f32;3],b:[f32;3])->[f32;3]{std::array::from_fn(|i|a[i]-b[i])}
fn norm(a:[f32;3])->f32{super::cursor::native_sqrt(dot(a,a))}
#[derive(Clone,Copy)]struct Projection{radius:f32,height:f32,previous:Option<usize>,start:[f32;3],distance:f32,direction:[f32;3]}
impl State{
 pub const CLASS:&'static [u8]=b"Mpd::CComputeTargetPoint_Queue";
 pub fn construct(identity:usize,pathfinder:usize,name:&[u8])->Self{Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),predicate:None,profiles:None,maxdist:None,entitydist:None,extra:None,ratio:None,actor:None,axis:[f32::MAX;3]}}
 pub fn reset(&mut self){} // actual2b7c00
 pub fn set_predicate(&mut self,predicate:Option<usize>){self.predicate=predicate;}
 fn point(&mut self,p:usize,h:&mut impl Host)->Result<[f32;3]>{Ok([f32::from_bits(h.read(self,p,4)?),f32::from_bits(h.read(self,p,8)?),f32::from_bits(h.read(self,p,12)?)])}
 fn actor_position(&mut self,h:&mut impl Host)->Result<[f32;3]>{let actor=self.actor.context("Queue target has no current actor")?;h.position(self,actor)}
 pub fn accept(&mut self,input:usize,h:&mut impl Host)->Result<bool>{let source=self.point(input,h)?;let parent=self.pathfinder;let target=h.parent_target(self,parent)?;if source.iter().zip(target).any(|(a,b)|*a!=b){return Ok(true)}let parent=self.pathfinder;let actor=h.parent_entity(self,parent)?;let p=h.position(self,actor)?;let delta=sub(p,self.point(input,h)?);let radius=self.maxdist.context("Queue MaxDist is unwritten")?;Ok(delta[1].mul_add(delta[1],delta[2].mul_add(delta[2],delta[0]*delta[0]))>radius*radius)}
 pub fn query(&mut self,input:usize,output:usize,h:&mut impl Host)->Result<u32>{
  for i in 0..19{let offset=4+i*4;let v=h.read(self,input,offset)?;h.write(self,output,offset,v)?;}
  if self.accept(input,h)?{return Ok(1)}
  let parent=self.pathfinder;self.actor=Some(h.parent_entity(self,parent)?);let delta=sub(self.point(input,h)?,self.actor_position(h)?);let squared=dot(delta,delta);if squared==0.{return Ok(1)}let length=super::cursor::native_sqrt(squared);let d=delta.map(|v|v/length);
  let tolerance=h.tolerance(self)?;self.axis=[0f32.mul_add(d[2],-(d[0]*d[1])),d[0].mul_add(d[0],-((-d[2])*d[2])),(-d[2]).mul_add(d[1],-(0f32*d[0]))];
  let mut current=self.point(input,h)?;let mut projected=dot(sub(current,self.actor_position(h)?),d);let mut budget=h.actor_limit(self)?;let mut previous=None;
  while budget>0{
   let mut next=current;let found=self.find(current,&mut next,previous,h)?;previous=found;if found.is_none(){break}
   let mut p=dot(sub(next,self.actor_position(h)?),d);let maximum=projected-tolerance;
   if p>maximum{if p<0. || projected<=tolerance{next=self.actor_position(h)?;p=0.;}else{next=std::array::from_fn(|i|current[i]-tolerance*d[i]);p=maximum;}}
   current=next;projected=p;for i in 0..16{h.write(self,output,16+i*4,0)?;}
   let position=self.actor_position(h)?;if current.iter().zip(position).all(|(a,b)|*a==b){break}budget=budget.wrapping_sub(1);
  }
  let old=self.point(output,h)?;if old.iter().zip(current).any(|(a,b)|*a!=b){for i in 0..3{h.write(self,output,4+i as u32*4,current[i].to_bits())?;}}
  Ok(1)
 }
 fn find(&mut self,start:[f32;3],out:&mut[f32;3],previous:Option<usize>,h:&mut impl Host)->Result<Option<usize>>{
  if self.actor.is_none(){return Ok(None)}*out=start;let delta=sub(start,self.actor_position(h)?);let length=norm(delta);if length==0.{return Ok(None)}let direction=delta.map(|v|v/length);
  let actor=self.actor.unwrap();let diameter=h.attribute(self,actor,Attribute::Diameter)?;let actor=self.actor.context("Queue diameter lookup removed its actor")?;let height=h.attribute(self,actor,Attribute::Height)?;
  let radius=if let Some(a)=diameter{0.5*h.scalar(self,a)?}else{0.5*h.unit_scale(self)?.unwrap_or(1.)};
  let halfheight=if let Some(a)=height{0.5*h.scalar(self,a)?}else{h.unit_scale(self)?.unwrap_or(1.)};let context=Projection{radius,height:halfheight,previous,start,distance:length,direction};
  if let Some(profiles)=self.profiles.context("Queue profiles binding is unwritten")?{
   let position=self.actor_position(h)?;let distance=self.entitydist.context("Queue MaxEntityDist is unwritten")?;let mut actor=h.first(self,profiles,position,distance)?;
   while let Some(other)=actor{if self.candidate(other,out,context,h)?{return Ok(Some(other))}let profiles=self.profiles.flatten().context("Queue candidate removed its profile manager")?;actor=h.next(self,profiles)?;}
  }else{let mut node=h.all_first(self)?;while let Some(current)=node{let actor=h.all_actor(self,current)?;if self.candidate(actor,out,context,h)?{return Ok(Some(actor))}node=h.all_next(self,current)?;}}
  Ok(None)
 }
 fn candidate(&mut self,other:usize,out:&mut[f32;3],c:Projection,h:&mut impl Host)->Result<bool>{
  let halfheight=if let Some(a)=h.attribute(self,other,Attribute::Height)?{0.5*h.scalar(self,a)?}else{h.unit_scale(self)?.unwrap_or(1.)};
  let actor=self.actor.context("Queue candidate has no current actor")?;let offset=sub(h.position(self,other)?,h.position(self,actor)?);let width=self.ratio.context("Queue MaxDeltaHeightRatio is unwritten")?*(c.height+halfheight);
  if dot(offset,self.axis).abs()>width{return Ok(false)}if other==actor || c.previous==Some(other){return Ok(false)}
  if let Some(predicate)=self.predicate{if h.filter(self,predicate,actor,other)?&255==0{return Ok(false)}}
  let p=h.position(self,other)?;let center=[p[0],(c.height+p[1])-halfheight,p[2]];
  let diameter=h.attribute(self,other,Attribute::Diameter)?;let length=h.attribute(self,other,Attribute::Length)?;
  let radius=if let Some(a)=diameter{0.5*h.scalar(self,a)?}else{0.5*h.unit_scale(self)?.unwrap_or(1.)};
  if let Some(length)=length{
   let length=h.scalar(self,length)?;let side=self.extra.context("Queue ExtraGap is unwritten")?+(c.radius+radius);let forward_radius=self.extra.unwrap()+0.5f32.mul_add(length,c.radius);
   let forward=if let Some(a)=h.attribute(self,other,Attribute::TorsoOrientation)?{h.vector(self,a)?}else{h.forward(self,other)?};let up=h.up(self)?;
   let cross=[up[1].mul_add(forward[2],-(up[2]*forward[1])),up[2].mul_add(forward[0],-(up[0]*forward[2])),up[0].mul_add(forward[1],-(up[1]*forward[0]))];let length=norm(cross);let right=if length==0.{h.zero(self)?}else{cross.map(|v|v/length)};
   let offset=sub(c.start,h.position(self,other)?);let distance=dot(forward,offset);if distance>forward_radius || distance< -forward_radius{return Ok(false)}let distance=dot(right,offset);if distance>side || distance< -side{return Ok(false)}
   let front=forward.map(|v|v*forward_radius);let side=right.map(|v|v*side);let plus=std::array::from_fn::<_,3,_>(|i|center[i]+front[i]);let minus=std::array::from_fn::<_,3,_>(|i|center[i]-front[i]);
   let vertices=[std::array::from_fn(|i|plus[i]+side[i]),std::array::from_fn(|i|plus[i]-side[i]),std::array::from_fn(|i|minus[i]-side[i]),std::array::from_fn(|i|minus[i]+side[i])];
   for i in 0..4{let actor=self.actor_position(h)?;if h.intersection(self,actor,c.start,vertices[i],vertices[(i+1)&3],out)?{h.flush(self)?;return Ok(true)}}h.flush(self)?;*out=self.actor_position(h)?;Ok(true)
  }else{
   let radius=self.extra.context("Queue ExtraGap is unwritten")?+(c.radius+radius);let offset=sub(*out,center);let dot=c.direction[2].mul_add(offset[2],offset[0]*c.direction[0]);
   if dot< -radius || !(dot<radius){return Ok(false)}let cross=offset[2].mul_add(c.direction[0],-(offset[0]*c.direction[2]));if cross< -radius || !(cross<radius){return Ok(false)}
   let extent=dot+radius;let amount=if c.distance-extent>=0.{extent}else{c.distance};*out=std::array::from_fn(|i|out[i]-amount*c.direction[i]);Ok(true)
  }
 }
}

#[cfg(test)]pub(super) mod tests{
 use super::*;use serde_json::{Value,json};use std::collections::BTreeMap;
 pub struct Services<'a>{pub input:&'a Value,pub trace:Vec<Value>,pub words:BTreeMap<usize,u32>,pub cursor:usize,pub counter:u32,pub pending:f32,pub budget:f32}
 impl Services<'_>{
 pub fn value(&self,key:&str)->f32{f32::from_bits(self.input[key].as_u64().unwrap() as u32)}
 fn vector_value(v:&Value)->[f32;3]{std::array::from_fn(|i|f32::from_bits(v[i].as_u64().unwrap() as u32))}
 fn key(kind:Attribute)->&'static str{match kind{Attribute::Height=>"height",Attribute::Diameter=>"diameter",Attribute::Length=>"length",Attribute::TorsoOrientation=>"torso"}}
 fn world(&mut self)->Option<f32>{self.trace.push(json!(["world"]));self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32))}
 }
 impl DefinitionBackend for Services<'_>{
 fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{Ok(self.world())}
 fn profiles(&mut self,_:&mut State)->Result<Option<usize>>{self.trace.push(json!(["world"]));self.trace.push(json!(["profiles"]));Ok((self.input["profiles"]==true).then_some(1))}
 fn number(&mut self,_:&mut State,_:Option<&[u8]>)->Result<f64>{self.trace.push(json!(["number"]));Ok(f64::from_bits(self.input["number"].as_u64().unwrap()))}
 }
 impl Host for Services<'_>{type Attribute=(usize,Attribute);
 fn read(&mut self,_:&mut State,p:usize,o:u32)->Result<u32>{Ok(*self.words.get(&(p+o as usize)).unwrap_or(&0))}
 fn write(&mut self,_:&mut State,p:usize,o:u32,v:u32)->Result<()>{self.words.insert(p+o as usize,v);Ok(())}
 fn parent_target(&mut self,_:&mut State,_:usize)->Result<[f32;3]>{Ok(Self::vector_value(&self.input["parent_target"]))}
 fn parent_entity(&mut self,_:&mut State,_:usize)->Result<usize>{Ok(0)}
 fn position(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{Ok(Self::vector_value(&self.input["actors"][e]["position"]))}
 fn attribute(&mut self,_:&mut State,e:usize,a:Attribute)->Result<Option<Self::Attribute>>{self.trace.push(json!(["attribute",e,a.index()]));Ok((!self.input["actors"][e][Self::key(a)].is_null()).then_some((e,a)))}
 fn scalar(&mut self,_:&mut State,(e,a):Self::Attribute)->Result<f32>{Ok(f32::from_bits(self.input["actors"][e][Self::key(a)].as_u64().unwrap() as u32))}
 fn vector(&mut self,_:&mut State,(e,a):Self::Attribute)->Result<[f32;3]>{Ok(Self::vector_value(&self.input["actors"][e][Self::key(a)]))}
 fn forward(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{Ok(Self::vector_value(&self.input["actors"][e]["forward"]))}
 fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{Ok(self.world())}
 fn up(&mut self,_:&mut State)->Result<[f32;3]>{Ok([0.,1.,0.])}
 fn zero(&mut self,_:&mut State)->Result<[f32;3]>{Ok([0.;3])}
 fn actor_limit(&mut self,_:&mut State)->Result<i32>{self.trace.push(json!(["world"]));Ok(self.input["limit"].as_u64().unwrap() as u32 as i32)}
 fn tolerance(&mut self,_:&mut State)->Result<f32>{Ok(self.value("tolerance"))}
 fn first(&mut self,_:&mut State,_:usize,p:[f32;3],distance:f32)->Result<Option<usize>>{self.trace.push(json!(["first",p.map(f32::to_bits),distance.to_bits()]));self.cursor=0;Ok((!self.input["actors"].as_array().unwrap().is_empty()).then_some(0))}
 fn next(&mut self,_:&mut State,_:usize)->Result<Option<usize>>{self.trace.push(json!(["next"]));self.cursor+=1;Ok((self.cursor<self.input["actors"].as_array().unwrap().len()).then_some(self.cursor))}
 fn all_first(&mut self,_:&mut State)->Result<Option<usize>>{self.trace.push(json!(["world"]));Ok((!self.input["actors"].as_array().unwrap().is_empty()).then_some(0))}
 fn all_actor(&mut self,_:&mut State,node:usize)->Result<usize>{Ok(node)}
 fn all_next(&mut self,_:&mut State,node:usize)->Result<Option<usize>>{Ok((node+1<self.input["actors"].as_array().unwrap().len()).then_some(node+1))}
 fn filter(&mut self,_:&mut State,_:usize,a:usize,b:usize)->Result<u32>{self.trace.push(json!(["predicate",a,b]));Ok(self.input["predicate"].as_u64().unwrap() as u32)}
 fn intersection(&mut self,_:&mut State,a:[f32;3],b:[f32;3],c:[f32;3],d:[f32;3],out:&mut[f32;3])->Result<bool>{self.trace.push(json!(["intersection",a.map(f32::to_bits),b.map(f32::to_bits),c.map(f32::to_bits),d.map(f32::to_bits)]));self.counter=self.counter.wrapping_add(1);self.pending+=0.125;Ok(super::super::native_segment_intersection::intersection(a,b,c,d,Some(out),None))}
 fn flush(&mut self,_:&mut State)->Result<()>{self.trace.push(json!(["flush"]));self.budget+=self.pending;self.pending=0.;Ok(())}
 }
 pub fn state(c:&Value)->State{let mut s=State::construct(1,2,b"queue");s.predicate=c["predicate"].as_u64().map(|_|1);s.profiles=Some((c["profiles"]==true).then_some(1));s.maxdist=Some(f32::from_bits(c["maxdist"].as_u64().unwrap() as u32));s.entitydist=Some(f32::from_bits(c["entitydist"].as_u64().unwrap() as u32));s.extra=Some(f32::from_bits(c["extra"].as_u64().unwrap() as u32));s.ratio=Some(f32::from_bits(c["ratio"].as_u64().unwrap() as u32));s}
 pub fn host(c:&Value)->Services<'_>{let mut words=BTreeMap::new();words.insert(0x63000000,0x12345678);words.insert(0x63000100,0x87654321);for i in 0..3{words.insert(0x63000004+i*4,c["point"][i].as_u64().unwrap() as u32);}for i in 0..16{words.insert(0x63000010+i*4,0x101+i as u32);}Services{input:c,trace:vec![],words,cursor:0,counter:0,pending:0.,budget:1.}}
 pub fn image(s:&State)->Vec<u32>{vec![s.maxdist.unwrap().to_bits(),s.entitydist.unwrap().to_bits(),s.extra.unwrap().to_bits(),s.ratio.unwrap().to_bits(),s.actor.map_or(0,|e|0x64000000+e as u32*0x1000),s.axis[0].to_bits(),s.axis[1].to_bits(),s.axis[2].to_bits()]}
 #[test]fn complete_queue_projection_matches_original_actors_geometry_and_budget()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_compute_target.json"))?;
  for (i,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let c=&case["input"];if c["op"]!="query"{continue}let mut s=state(c);let mut h=host(c);let alias=c["alias"].as_u64().unwrap() as usize;let out=if alias==0{0x63000100}else{0x63000000+alias};let result=s.query(0x63000000,out,&mut h)?;
   let output=(0..20).map(|j|h.read(&mut s,out,j*4)).collect::<Result<Vec<_>>>()?;
   let actual=json!({"result":result,"output":output,"fields":image(&s),"actor":s.actor,"profiles":s.profiles.flatten().is_some(),"trace":h.trace,"counters":[h.counter,h.pending.to_bits(),h.budget.to_bits()]});for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],case[key],"case {i} {key}: {c}");}
  }Ok(())
 }
}

