//! CSelectPathNodeCandidate_NextPathNode1cb648..1cc2c8. Pointer arguments
//! remain portable arena references; owner/query services are explicit.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::{ai_graph_link_passes::Memory,ai_segment_query::WalkPoint};
#[derive(Clone,Copy)]pub enum Point{Source200,Target2c4}
///Virtual3c returns an acceptance policy; Refresh496268 returns a source point.
#[derive(Clone,Copy)]pub enum Current{Acceptance3c,Refresh496268}
#[derive(Clone,Copy)]pub struct Context{pub frame:u32,pub point_kind:u32,pub marker_kind:u32,pub marker_metadata:u32,pub spatial_class:u32}
pub struct State{pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub distance:f32,pub frame_limit:u32,pub stamp:f32,pub frame_count:u32,pub skipped:u32,pub skip_limit:u32,pub previous:u32}
pub trait DefinitionBackend{
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
 fn integer(&mut self,s:&mut State,text:Option<&[u8]>)->Result<i32>;
 fn number(&mut self,s:&mut State,text:Option<&[u8]>)->Result<f64>;
}
pub trait Host{
 fn clock(&mut self,s:&mut State,m:&mut Memory)->Result<f32>;
 fn entity(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<usize>;
 fn position(&mut self,s:&mut State,m:&mut Memory,entity:usize)->Result<[f32;3]>;
 fn recovery_flag(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<u8>;
 fn manager50(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<u32>;
 fn validate30(&mut self,s:&mut State,m:&mut Memory,interface:u32,entity:usize,previous:u32,candidate:u32)->Result<u32>;
 fn next485908(&mut self,s:&mut State,m:&mut Memory,interface:u32,previous:u32,candidate:u32,manager:u32)->Result<u32>;
 fn current(&mut self,s:&mut State,m:&mut Memory,parent:usize,kind:Current)->Result<u32>;
 fn goto1ac(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<u32>;
 fn cost20(&mut self,s:&mut State,m:&mut Memory,service:u32,entity:usize,start:u32,end:u32,current:u32,cost:u32)->Result<u32>;
 fn can_go184(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<u32>;
 fn class18(&mut self,s:&mut State,m:&mut Memory,service:u32)->Result<u32>;
 fn diameter(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<Option<f32>>;
 fn unit_scale(&mut self,s:&mut State,m:&mut Memory)->Result<Option<f32>>;
 fn sweep34(&mut self,s:&mut State,m:&mut Memory,service:u32,start:u32,end:u32,mode:u32,radius:f32)->Result<u32>;
 fn can_go58(&mut self,s:&mut State,m:&mut Memory,parent:usize,start:u32,end:u32)->Result<u32>;
 fn point(&mut self,s:&mut State,m:&mut Memory,parent:usize,kind:Point)->Result<WalkPoint>;
 fn path_cost4997f0(&mut self,s:&mut State,m:&mut Memory,parent:usize,start:u32,end:u32,radius:f32,cost:u32)->Result<u32>;
 fn lpf_f8(&mut self,s:&mut State,m:&mut Memory,parent:usize)->Result<u32>;
 fn shortcut49da6c(&mut self,s:&mut State,m:&mut Memory,lpf:u32,node:u32)->Result<u32>;
}
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn point(m:&mut Memory,p:u32,kind:u32,value:WalkPoint)->Result<()>{m.write(p,kind)?;for (i,v) in value.position.into_iter().chain(value.auxiliary).enumerate(){m.write(p.wrapping_add(4+i as u32*4),v)?;}Ok(())}
fn position(m:&Memory,p:u32)->Result<[u32;3]>{Ok([word(m,p,4)?,word(m,p,8)?,word(m,p,12)?])}
impl State{
 pub const CLASS:&'static [u8]=b"Mpd::CSelectPathNodeCandidate_NextPathNode";
 pub fn construct(identity:usize,pathfinder:usize,name:&[u8])->Self{Self{identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),distance:0.,frame_limit:0,stamp:f32::MAX,frame_count:0,skipped:0,skip_limit:4,previous:u32::MAX}}
 pub fn reset(&mut self){self.stamp=f32::MAX;self.frame_count=0;self.skipped=0;}
 ///2b7aa0: inherited virtual28 has a literal zero result.
 pub fn probe28(&self)->u32{0}
 pub fn precheck(&mut self,m:&mut Memory,node:u32,host:&mut impl Host)->Result<bool>{
  if self.skipped>=self.skip_limit{self.skipped=0;return Ok(false)}
  let now=host.clock(self,m)?;if self.stamp!=now{self.stamp=host.clock(self,m)?;self.frame_count=0;}
  if self.frame_count>=self.frame_limit{return Ok(false)}
  let parent=self.pathfinder;let radius=self.distance;let entity=host.entity(self,m,parent)?;let p=host.position(self,m,entity)?;
  let v=position(m,node)?.map(f32::from_bits);let [x,y,z]=std::array::from_fn(|i|v[i]-p[i]);
  Ok(!(y.mul_add(y,z.mul_add(z,x*x))>radius*radius))
 }
 pub fn select(&mut self,m:&mut Memory,c:Context,path:u32,node:u32,host:&mut impl Host)->Result<u32>{
  if !self.precheck(m,node,host)?{return Ok(0)}self.next(m,c,path,node,host)
 }
 pub fn next(&mut self,m:&mut Memory,c:Context,path:u32,node:u32,host:&mut impl Host)->Result<u32>{
  let parent=self.pathfinder;if host.recovery_flag(self,m,parent)?!=0{let r=self.recover(m,c,path,node,host)?;if r!=0{return Ok(r)}}
  if node!=0 && m.word(node)?!=self.previous{self.skipped=0;self.previous=m.word(node)?;}
  self.frame_count=self.frame_count.wrapping_add(1);
  let candidate=node.wrapping_add(0x14).wrapping_add(self.skipped.wrapping_mul(0x14));
  if candidate>=word(m,path,0x1b8)?{self.skipped=0;return Ok(0)}
  let before=candidate.wrapping_sub(0x14);let previous=if before>=word(m,path,0x1bc)?{m.word(before)?}else{u32::MAX};
  let mut interface=word(m,candidate,0x10)?;let parent=self.pathfinder;let manager=host.manager50(self,m,parent)?;
  while interface!=0{
   let parent=self.pathfinder;let entity=host.entity(self,m,parent)?;
   if host.validate30(self,m,interface,entity,previous,m.word(candidate)?)?&255==0{self.skipped=0;return Ok(0)}
   interface=host.next485908(self,m,interface,previous,m.word(candidate)?,manager)?;
  }
  self.skipped=self.skipped.wrapping_add(1);Ok(candidate)
 }
 ///1cbcf4: a missing first cost rejects; unavailable detour legs accept, as
 /// in the original. Unsupported cost implementations must return an error.
 pub fn detour(&mut self,m:&mut Memory,c:Context,start:u32,middle:u32,end:u32,host:&mut impl Host)->Result<u32>{
  let parent=self.pathfinder;let current=host.current(self,m,parent,Current::Acceptance3c)?;
  let parent=self.pathfinder;let goto=host.goto1ac(self,m,parent)?;let entity=host.entity(self,m,parent)?;
  let frame=c.frame.wrapping_add(0x200);let direct=frame.wrapping_add(8);let a=frame.wrapping_add(0x10);let b=frame.wrapping_add(12);
  for p in [direct,a,b]{m.write(p,0)?;}
  if host.cost20(self,m,goto,entity,start,end,current,direct)?&255==0{return Ok(0)}
  if host.cost20(self,m,goto,entity,start,middle,current,a)?&255==0{return Ok(1)}
  if host.cost20(self,m,goto,entity,middle,end,current,b)?&255==0{return Ok(1)}
  let sum=f32::from_bits(m.word(a)?)+f32::from_bits(m.word(b)?);Ok(u32::from(f32::from_bits(m.word(direct)?)<=sum))
 }
 pub fn reachable(&mut self,m:&mut Memory,c:Context,start:u32,end:u32,host:&mut impl Host)->Result<u32>{
  let parent=self.pathfinder;let selected=host.can_go184(self,m,parent)?;let mut class=host.class18(self,m,selected)?;let mut visited=BTreeSet::new();
  let mesh=loop{if class==c.spatial_class{break true}ensure!(visited.insert(class),"Cyclic candidate spatial class ancestry");class=word(m,class,0x14)?;if class==0{break false}};
  if mesh{
   let parent=self.pathfinder;let radius=if let Some(d)=host.diameter(self,m,parent)?{0.5*d}else{f32::from_bits(0x3e99999a)*host.unit_scale(self,m)?.unwrap_or(1.)};
   let parent=self.pathfinder;let selected=host.can_go184(self,m,parent)?;host.sweep34(self,m,selected,start,end,6,radius)
  }else{let parent=self.pathfinder;host.can_go58(self,m,parent,start,end)}
 }
 pub fn recover(&mut self,m:&mut Memory,c:Context,path:u32,node:u32,host:&mut impl Host)->Result<u32>{
  let end=word(m,path,0x1b8)?;let parent=self.pathfinder;let last=end.wrapping_sub(0x14);if last==node{return Ok(0)}
  let first=c.frame.wrapping_add(0x68);let target=c.frame.wrapping_add(0xb8);let start=c.frame.wrapping_add(0x18);let marker=c.frame.wrapping_add(0x10);
  point(m,target,c.point_kind,WalkPoint{position:position(m,last)?,auxiliary:[0;16]})?;
  let source=host.point(self,m,parent,Point::Source200)?;point(m,first,c.point_kind,WalkPoint{position:source.position,auxiliary:[0;16]})?;
  let radius=if let Some(d)=host.diameter(self,m,parent)?{0.5*d}else{0.5*host.unit_scale(self,m)?.unwrap_or(1.)};
  if host.path_cost4997f0(self,m,parent,first,target,radius,c.frame.wrapping_add(8))?&255==0{return Ok(0)}
  let final_point=host.point(self,m,parent,Point::Target2c4)?;point(m,target,c.point_kind,final_point)?;
  m.write(marker,c.marker_kind)?;let id=m.word(node)?;m.write(marker.wrapping_add(4),id)?;
  point(m,start,c.point_kind,WalkPoint{position:position(m,node)?,auxiliary:[0;16]})?;
  if id!=1{let slot=word(m,c.marker_metadata,8)?;m.write(start.wrapping_add(0x10).wrapping_add(slot.wrapping_mul(4)),marker)?;}
  let current=host.current(self,m,parent,Current::Refresh496268)?;
  if self.detour(m,c,current,start,target,host)?&255==0{return Ok(0)}
  let current=host.current(self,m,parent,Current::Refresh496268)?;
  if self.reachable(m,c,current,target,host)?&255==0{return Ok(0)}
  let lpf=host.lpf_f8(self,m,parent)?;
  if lpf!=0 && host.shortcut49da6c(self,m,lpf,last)?&255==0{return Ok(0)}Ok(last)
 }
}

#[cfg(test)]
mod tests{
 use super::*;
 struct Services{clock_calls:u32,now:f32,position:[f32;3],costs:Vec<(u32,f32)>,trace:Vec<(u32,u32)>,valid:u32,next:u32}
 impl Default for Services{fn default()->Self{Self{clock_calls:0,now:1.,position:[0.;3],costs:Vec::new(),trace:Vec::new(),valid:1,next:0}}}
 impl Host for Services{
 fn clock(&mut self,_:&mut State,_:&mut Memory)->Result<f32>{self.clock_calls+=1;Ok(self.now)}
 fn entity(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<usize>{Ok(42)}
 fn position(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<[f32;3]>{Ok(self.position)}
 fn recovery_flag(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<u8>{Ok(0)}
 fn manager50(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<u32>{Ok(77)}
 fn validate30(&mut self,_:&mut State,_:&mut Memory,_:u32,_:usize,p:u32,n:u32)->Result<u32>{self.trace.push((p,n));Ok(self.valid)}
 fn next485908(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32,_:u32,manager:u32)->Result<u32>{assert_eq!(manager,77);Ok(self.next)}
 fn current(&mut self,_:&mut State,_:&mut Memory,_:usize,_:Current)->Result<u32>{Ok(0x8888)}
 fn goto1ac(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<u32>{Ok(0x9999)}
 fn cost20(&mut self,_:&mut State,m:&mut Memory,_:u32,_:usize,start:u32,end:u32,_:u32,out:u32)->Result<u32>{self.trace.push((start,end));let (result,cost)=self.costs.remove(0);m.write(out,cost.to_bits())?;Ok(result)}
 fn can_go184(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<u32>{unreachable!()}
 fn class18(&mut self,_:&mut State,_:&mut Memory,_:u32)->Result<u32>{unreachable!()}
 fn diameter(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<Option<f32>>{unreachable!()}
 fn unit_scale(&mut self,_:&mut State,_:&mut Memory)->Result<Option<f32>>{unreachable!()}
 fn sweep34(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:f32)->Result<u32>{unreachable!()}
 fn can_go58(&mut self,_:&mut State,_:&mut Memory,_:usize,_:u32,_:u32)->Result<u32>{unreachable!()}
 fn point(&mut self,_:&mut State,_:&mut Memory,_:usize,_:Point)->Result<WalkPoint>{unreachable!()}
 fn path_cost4997f0(&mut self,_:&mut State,_:&mut Memory,_:usize,_:u32,_:u32,_:f32,_:u32)->Result<u32>{unreachable!()}
 fn lpf_f8(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<u32>{unreachable!()}
 fn shortcut49da6c(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32)->Result<u32>{unreachable!()}
 }
 fn setup()->(State,Memory,Context,Services){let mut m=Memory::default();m.regions.insert(0x1000,vec![0;0x1000]);(State::construct(1,2,b"next"),m,Context{frame:0x1800,point_kind:0,marker_kind:0,marker_metadata:0,spatial_class:0},Services::default())}
 #[test]fn candidate_budget_distance_links_and_detour_costs()->Result<()>{
  let (mut s,mut m,c,mut h)=setup();let node=0x1400;let path=0x1000;
  s.distance=5.;s.frame_limit=2;m.write(node,7)?;m.write(node+4,3f32.to_bits())?;m.write(node+8,4f32.to_bits())?;
  assert!(s.precheck(&mut m,node,&mut h)?);assert_eq!(h.clock_calls,2);assert_eq!(s.frame_count,0);
  s.frame_count=2;assert!(!s.precheck(&mut m,node,&mut h)?);assert_eq!(h.clock_calls,3);
  h.now=2.;assert!(s.precheck(&mut m,node,&mut h)?);assert_eq!(s.frame_count,0);
  m.write(node+4,f32::NAN.to_bits())?;assert!(s.precheck(&mut m,node,&mut h)?);
  s.skipped=4;let before=h.clock_calls;assert!(!s.precheck(&mut m,node,&mut h)?);assert_eq!(h.clock_calls,before);assert_eq!(s.skipped,0);
  m.write(path+0x1bc,node)?;m.write(path+0x1b8,node+0x3c)?;m.write(node+0x14,8)?;m.write(node+0x24,0x88)?;
  assert_eq!(s.next(&mut m,c,path,node,&mut h)?,node+0x14);assert_eq!(h.trace,[(7,8)]);assert_eq!(s.skipped,1);
  assert_eq!(s.next(&mut m,c,path,node,&mut h)?,node+0x28);assert_eq!(s.skipped,2);
  assert_eq!(s.next(&mut m,c,path,node,&mut h)?,0);assert_eq!(s.skipped,0);
  h.valid=0x100;assert_eq!(s.next(&mut m,c,path,node,&mut h)?,0);assert_eq!(s.skipped,0);
  for (costs,expected,calls) in [(vec![(0,10.)],0,1),(vec![(1,10.),(0,0.)],1,2),(vec![(1,10.),(1,3.),(0,0.)],1,3),(vec![(1,10.),(1,3.),(1,7.)],1,3),(vec![(1,10.),(1,3.),(1,6.)],0,3),(vec![(1,f32::NAN),(1,3.),(1,7.)],0,3)]{
   h.costs=costs;h.trace.clear();assert_eq!(s.detour(&mut m,c,11,22,33,&mut h)?,expected);assert_eq!(h.trace.len(),calls);assert!(h.costs.is_empty());
  }
  s.previous=99;s.skip_limit=9;s.frame_limit=5;s.distance=6.;s.reset();assert_eq!((s.previous,s.skip_limit,s.frame_limit,s.distance),(99,9,5,6.));assert_eq!((s.frame_count,s.skipped,s.stamp),(0,0,f32::MAX));Ok(())
 }
 #[test]fn candidate_precheck_matches_original_instructions()->Result<()>{
  let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_node_candidate_precheck.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let input:Vec<u32>=case["input"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect();
   let (mut s,mut m,_,mut h)=setup();s.skipped=input[0];s.skip_limit=input[1];s.frame_count=input[2];s.frame_limit=input[3];s.stamp=f32::from_bits(input[4]);h.now=f32::from_bits(input[5]);s.distance=f32::from_bits(input[9]);
   for i in 0..3{m.write(0x1404+i*4,input[6+i as usize])?;}
   assert_eq!(u32::from(s.precheck(&mut m,0x1400,&mut h)?),case["result"].as_u64().unwrap() as u32,"{case}");
   assert_eq!(h.clock_calls,case["clock_calls"].as_u64().unwrap() as u32,"{case}");
   assert_eq!(vec![s.stamp.to_bits(),s.frame_count,s.skipped],case["fields"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{case}");
  }Ok(())
 }

}
