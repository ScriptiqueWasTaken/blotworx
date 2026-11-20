//! Complete47e5a8 nearest-node control. Collection, graph, scheduling and
//! spatial/LPF services must use their real owners; no ready/clear substitutes.
use anyhow::{Context as _,Result,ensure};use std::collections::BTreeSet;
#[derive(Clone,Copy)]pub struct Context{pub frame:u32,pub point_kind:u32,pub marker_kind:u32,pub spatial_class:usize}
#[derive(Clone,Copy,Default)]pub struct Filter{pub function:u32,pub data:u32}
#[derive(Clone,Copy)]pub struct Args{pub graph:usize,pub point:u32,pub mode:u32,pub output:u32,pub parent:usize,pub direction:u32,pub workspace:u32,pub reset:u32,pub limit:u32,pub filter:Filter}
#[derive(Clone,Copy)]pub struct Collection{pub minimum:[f32;3],pub maximum:[f32;3],pub capacity:u32,pub output:u32,pub filter:Filter}
#[derive(Clone,Copy,Debug)]pub enum Task{Begin,Admit,Enter,Activate,Cancel,Release}
pub trait Host{
 fn read(&mut self,address:u32)->Result<u32>;
 fn write(&mut self,address:u32,value:u32)->Result<()>;
 fn default_workspace(&mut self,graph:usize)->Result<u32>;
 fn graph_radius(&mut self,graph:usize)->Result<f32>;
 fn graph_modes(&mut self,graph:usize)->Result<[Option<u32>;4]>;
 fn collect(&mut self,graph:usize,request:Collection)->Result<u32>;
 fn query_flags(&mut self)->Result<u32>;
 fn traversal_service(&mut self)->Result<Option<usize>>;
 fn task_index(&mut self,graph:usize)->Result<u32>;
 fn entity(&mut self,parent:usize)->Result<usize>;
 fn clock(&mut self)->Result<usize>;
 fn task(&mut self,op:Task,clock:usize,index:u32,entity:usize)->Result<u32>;
 fn marker_index(&mut self)->Result<u32>;
 fn bind(&mut self,graph:usize,node:u32)->Result<u32>;
 fn connections(&mut self,graph:usize)->Result<u32>;
 fn select_connection(&mut self,graph:usize,index:u32)->Result<()>;
 fn allowed(&mut self,graph:usize,entity:usize,mode:u32)->Result<u32>;
 fn base_accept(&mut self,graph:usize,entity:usize)->Result<u32>;
 fn spatial_service(&mut self,parent:usize)->Result<usize>;
 fn class(&mut self,service:usize)->Result<usize>;
 fn class_parent(&mut self,class:usize)->Result<Option<usize>>;
 fn diameter(&mut self,entity:usize)->Result<Option<f32>>;
 fn unit_scale(&mut self)->Result<Option<f32>>;
 fn query34(&mut self,service:usize,start:u32,end:u32,mode:u32,radius:f32)->Result<u32>;
 fn query58(&mut self,parent:usize,start:u32,end:u32)->Result<u32>;
 fn lpf(&mut self,parent:usize)->Result<Option<usize>>;
 fn lpf_ready(&mut self,lpf:usize)->Result<u32>;
 fn lpf_query(&mut self,lpf:usize,start:u32,end:u32,direction:u32,output:u32)->Result<u32>;
}
fn get(h:&mut impl Host,p:u32,o:u32)->Result<u32>{h.read(p.wrapping_add(o))}
fn put(h:&mut impl Host,p:u32,o:u32,v:u32)->Result<()>{h.write(p.wrapping_add(o),v)}
fn flags(h:&mut impl Host,w:u32,value:u32)->Result<()>{let old=get(h,w,20)?;put(h,w,20,old|value)}
fn copy(h:&mut impl Host,source:u32,destination:u32,kind:u32)->Result<()>{put(h,destination,0,kind)?;for i in 0..19{let o=4+i*4;let v=get(h,source,o)?;put(h,destination,o,v)?;}Ok(())}
fn release(h:&mut impl Host,parent:usize,index:u32)->Result<()>{let clock=h.clock()?;let entity=h.entity(parent)?;h.task(Task::Release,clock,index,entity)?;Ok(())}
fn spatial(h:&mut impl Host,c:Context,parent:usize)->Result<bool>{let service=h.spatial_service(parent)?;let mut class=h.class(service)?;let mut visited=BTreeSet::new();loop{if class==c.spatial_class{return Ok(true)}ensure!(visited.insert(class),"Cyclic nearest-search spatial class ancestry");let Some(next)=h.class_parent(class)? else{return Ok(false)};class=next;}}
pub fn query(h:&mut impl Host,c:Context,a:Args)->Result<u32>{
 ensure!(a.mode<=1,"Unsupported nearest-search mode");
 let automatic=a.workspace==0;let w=if automatic{h.default_workspace(a.graph)?}else{a.workspace};ensure!(w!=0,"Nearest-search workspace is null");
 if automatic||a.reset!=0{put(h,w,4,0)?;}let limit=if a.limit==0{u32::MAX}else{a.limit};
 if get(h,w,4)?==0{
  put(h,w,20,0)?;let radius=h.graph_radius(a.graph)?;let position=[f32::from_bits(get(h,a.point,4)?),f32::from_bits(get(h,a.point,8)?),f32::from_bits(get(h,a.point,12)?)];
  if !h.graph_modes(a.graph)?.into_iter().any(|v|v==Some(2)){return get(h,w,4)}
  for offset in [12,8,16]{put(h,w,offset,0)?;}let bytes=get(h,w,0)?;if bytes<0x1c{return get(h,w,4)}
  let count=h.collect(a.graph,Collection{minimum:position.map(|v|v-radius),maximum:position.map(|v|v+radius),capacity:bytes.wrapping_sub(0x18)>>2,output:w.wrapping_add(24),filter:a.filter})?;
  put(h,w,12,count)?;let result=h.query_flags()?;put(h,w,20,result)?;
  if count==0{
   //47ffa0 is a verified literal blr; the notification has no side effects.
   h.write(a.output,u32::MAX)?;put(h,w,4,0)?;return Ok(0)
  }put(h,w,4,1)?;
 }
 if get(h,w,4)?!=1{return Ok(0)}
 let traversal=h.traversal_service()?;let mut processed=0u32;
 loop{
  let index=get(h,w,8)?;let node=h.read(w.wrapping_add(24).wrapping_add(index.wrapping_mul(4)))?;
  if a.mode==0{let id=h.read(node)?&0x7fffffff;h.write(a.output,id)?;put(h,w,4,0)?;return Ok(0)}
  let task=h.task_index(a.graph)?;let default=h.default_workspace(a.graph)?;let mut held=false;
  if w!=default && task!=u32::MAX{
   let entity=h.entity(a.parent)?;let clock=h.clock()?;h.task(Task::Begin,clock,task,entity)?;
   if h.task(Task::Admit,clock,task,entity)?&255!=0{h.task(Task::Enter,clock,task,entity)?;h.task(Task::Activate,clock,task,entity)?;held=true;}
   else{h.task(Task::Cancel,clock,task,entity)?;return get(h,w,4)}
  }
  let candidate=c.frame.wrapping_add(0xe8);let marker=c.frame.wrapping_add(0x18);let start=c.frame.wrapping_add(0x98);let end=c.frame.wrapping_add(0x48);
  put(h,candidate,0,c.point_kind)?;for i in 0..3{let o=4+i*4;let value=get(h,node,o)?;put(h,candidate,o,value)?;}for i in 0..16{put(h,candidate,16+i*4,0)?;}
  let id=h.read(node)?&0x7fffffff;put(h,marker,0,c.marker_kind)?;put(h,marker,4,id)?;let slot=h.marker_index()?;put(h,candidate,16u32.wrapping_add(slot.wrapping_mul(4)),marker)?;
  copy(h,if a.direction!=0{a.point}else{candidate},start,c.point_kind)?;copy(h,if a.direction!=0{candidate}else{a.point},end,c.point_kind)?;
  let raw_id=h.read(node)?;h.bind(a.graph,raw_id)?;let mut valid=true;
  if a.direction!=0{
   let count=h.connections(a.graph)?;if count==0||count==u32::MAX{valid=false;flags(h,w,8)?;}
   if traversal.is_some(){let mut allowed=false;let mut connection=0;while connection<count{h.select_connection(a.graph,connection)?;let entity=h.entity(a.parent)?;if h.allowed(a.graph,entity,0)?&255!=0{allowed=true;break}connection=connection.wrapping_add(1);}if !allowed{valid=false;flags(h,w,0x20)?;}}
  }else{let entity=h.entity(a.parent)?;if h.base_accept(a.graph,entity)?==0{valid=false;flags(h,w,0x10)?;}}
  if valid{
   let accepted=if spatial(h,c,a.parent)?{let entity=h.entity(a.parent)?;let radius=if let Some(diameter)=h.diameter(entity)?{0.5*diameter}else{f32::from_bits(0x3e99999a)*h.unit_scale()?.unwrap_or(1.)};let service=h.spatial_service(a.parent)?;h.query34(service,start,end,6,radius)?}else{h.query58(a.parent,start,end)?};
   if accepted&255==0{valid=false;flags(h,w,0x40)?;}
  }
  if valid{if let Some(lpf)=h.lpf(a.parent)?{if h.lpf_ready(lpf)?!=0{let result=h.lpf_query(lpf,start.wrapping_add(4),end.wrapping_add(4),a.direction,c.frame.wrapping_add(0x14))?;if result!=0{if result==2 && get(h,w,16)?==0{put(h,w,16,node)?;}valid=false;flags(h,w,0x80)?;}}}}
  if valid{let id=h.read(node)?&0x7fffffff;h.write(a.output,id)?;put(h,w,4,0)?;if held{release(h,a.parent,task)?;}return get(h,w,4)}
  let index=get(h,w,8)?.wrapping_add(1);put(h,w,8,index)?;
  if index>=get(h,w,12)?{let fallback=get(h,w,16)?;let id=if fallback!=0{h.read(fallback)?&0x7fffffff}else{u32::MAX};h.write(a.output,id)?;put(h,w,4,0)?;if held{release(h,a.parent,task)?;}return get(h,w,4)}
  if held{release(h,a.parent,task)?;}processed=processed.wrapping_add(1);if processed>=limit{return get(h,w,4)}
 }
}

#[cfg(test)]
pub(super) mod tests {
 use super::*;
 use serde_json::{Value,json};
 use std::collections::BTreeMap;
 const G:usize=0x60000000; const P:usize=0x61000000; const E:usize=P+0x2000;
 const INPUT:u32=0x62000000; const OUT:u32=0x62000100; const W:u32=0x63000000;
 const N:u32=0x64000000; const SPATIAL:usize=0x68000000;
 pub(crate) struct Replay<'a>{input:&'a Value,memory:BTreeMap<u32,u32>,pub(crate) trace:Vec<Value>,current:usize,connection:usize}
 impl Replay<'_>{
  pub(crate) fn new(input:&Value)->Result<Replay<'_>>{
   let mut h=Replay{input,memory:BTreeMap::new(),trace:vec![],current:0,connection:0};
   for i in 0..3{h.write(INPUT+4+i*4,(4.+i as f32).to_bits())?;}
   for i in 0..16{h.write(INPUT+16+i*4,0x100+i)?;}h.write(OUT,0x1234)?;
   let fallback=input["fallback"].as_u64().map_or(0,|v|N+v as u32*0x100);
   for (i,value) in [h.number("capacity"),h.number("phase"),h.number("index"),h.number("count"),fallback,h.number("flags")].into_iter().enumerate(){h.write(W+i as u32*4,value)?;}
   for (i,node) in input["nodes"].as_array().unwrap().iter().enumerate(){
    let p=N+i as u32*0x100;h.write(p,node.as_u64().unwrap() as u32)?;
    for j in 0..3{h.write(p+4+j*4,(i as f32+j as f32+1.).to_bits())?;}h.write(W+24+i as u32*4,p)?;
   }
   Ok(h)
  }
  fn number(&self,key:&str)->u32{self.input[key].as_i64().unwrap() as u32}
  fn item(&self,key:&str)->u32{self.input[key][self.current].as_u64().unwrap() as u32}
  fn point(&self,p:u32)->Value{
   let position:Vec<_>=(0..3).map(|i|self.memory[&(p+4+i*4)]).collect();
   let auxiliary:Vec<_>=(0..16).filter_map(|i|{
    let value=*self.memory.get(&(p+16+i*4)).unwrap_or(&0);
    (value!=0).then(||json!([i,if self.memory.get(&value)==Some(&0x4ebe80){json!(["marker",self.memory[&(value+4)]])}else{json!(value)}]))
   }).collect();json!({"position":position,"auxiliary":auxiliary})
  }
  fn go(&mut self,start:u32,end:u32,radius:Option<f32>)->Result<u32>{
   self.trace.push(json!(["can_go",if radius.is_some(){"mesh"}else{"base"},self.point(start),self.point(end),radius.map(f32::to_bits)]));
   if self.input["mutate"].as_bool().unwrap(){self.write(start+8,9f32.to_bits())?;self.write(end+16,12345)?;}
   Ok(self.item("can_go"))
  }
 }
 impl Host for Replay<'_>{
  fn read(&mut self,p:u32)->Result<u32>{Ok(*self.memory.get(&p).unwrap_or(&0))}
  fn write(&mut self,p:u32,v:u32)->Result<()>{self.memory.insert(p,v);Ok(())}
  fn default_workspace(&mut self,_:usize)->Result<u32>{Ok(if self.input["use_default"].as_bool().unwrap()||self.input["same_default"].as_bool().unwrap(){W}else{W+0x1000})}
  fn graph_radius(&mut self,_:usize)->Result<f32>{Ok(2.)}
  fn graph_modes(&mut self,_:usize)->Result<[Option<u32>;4]>{Ok(std::array::from_fn(|i|Some(self.input["modes"][i].as_u64().unwrap() as u32)))}
  fn collect(&mut self,_:usize,r:Collection)->Result<u32>{
   self.trace.push(json!(["collect",r.minimum.map(f32::to_bits),r.maximum.map(f32::to_bits),r.capacity]));
   let count=self.input["nodes"].as_array().unwrap().len();for i in 0..count{self.write(r.output+i as u32*4,N+i as u32*0x100)?;}Ok(count as u32)
  }
  fn query_flags(&mut self)->Result<u32>{Ok(self.number("query_flags"))}
  fn traversal_service(&mut self)->Result<Option<usize>>{self.trace.push(json!(["traversal"]));Ok(self.input["traversal"].as_bool().unwrap().then_some(0x6c000000))}
  fn task_index(&mut self,_:usize)->Result<u32>{Ok(self.number("task"))}
  fn entity(&mut self,_:usize)->Result<usize>{Ok(E)}
  fn clock(&mut self)->Result<usize>{self.trace.push(json!(["clock"]));Ok(0x6b000000)}
  fn task(&mut self,op:Task,_:usize,index:u32,entity:usize)->Result<u32>{
   let tag=match op{Task::Begin=>"begin",Task::Admit=>"admit",Task::Enter=>"enter",Task::Activate=>"activate",Task::Cancel=>"cancel",Task::Release=>"release"};
   self.trace.push(json!([tag,index,entity]));Ok(if matches!(op,Task::Admit){self.input["admit"][self.memory[&(W+8)] as usize].as_u64().unwrap() as u32}else{0})
  }
  fn marker_index(&mut self)->Result<u32>{Ok(7)}
  fn bind(&mut self,_:usize,node:u32)->Result<u32>{self.current=self.input["nodes"].as_array().unwrap().iter().position(|v|v.as_u64()==Some(node as u64)).unwrap();self.trace.push(json!(["bind",node]));Ok(1)}
  fn connections(&mut self,_:usize)->Result<u32>{self.trace.push(json!(["connections",self.current]));Ok(self.item("connections"))}
  fn select_connection(&mut self,_:usize,index:u32)->Result<()>{self.connection=index as usize;self.trace.push(json!(["select",self.current,index]));Ok(())}
  fn allowed(&mut self,_:usize,_:usize,_:u32)->Result<u32>{self.trace.push(json!(["allowed",self.current,self.connection]));Ok(self.input["allowed"][self.current][self.connection].as_u64().unwrap() as u32)}
  fn base_accept(&mut self,_:usize,_:usize)->Result<u32>{self.trace.push(json!(["base",self.current]));Ok(self.item("base"))}
  fn spatial_service(&mut self,_:usize)->Result<usize>{Ok(SPATIAL)}
  fn class(&mut self,_:usize)->Result<usize>{self.trace.push(json!(["class",self.current]));Ok(match self.input["spatial"].as_str().unwrap(){"mesh"=>0x11864,"derived"=>0x68003000,_=>0x68002000})}
  fn class_parent(&mut self,class:usize)->Result<Option<usize>>{Ok((class==0x68003000).then_some(0x11864))}
  fn diameter(&mut self,_:usize)->Result<Option<f32>>{self.trace.push(json!(["diameter"]));Ok(self.input["diameter"].as_u64().map(|v|f32::from_bits(v as u32)))}
  fn unit_scale(&mut self)->Result<Option<f32>>{Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
  fn query34(&mut self,_:usize,start:u32,end:u32,mode:u32,radius:f32)->Result<u32>{assert_eq!(mode,6);self.go(start,end,Some(radius))}
  fn query58(&mut self,_:usize,start:u32,end:u32)->Result<u32>{self.go(start,end,None)}
  fn lpf(&mut self,_:usize)->Result<Option<usize>>{Ok((!self.input["lpf"].is_null()).then_some(0x69000000))}
  fn lpf_ready(&mut self,_:usize)->Result<u32>{Ok(self.number("lpf_ready"))}
  fn lpf_query(&mut self,_:usize,start:u32,end:u32,direction:u32,output:u32)->Result<u32>{
   let position=|p| (0..3).map(|i|*self.memory.get(&(p+i*4)).unwrap_or(&0)).collect::<Vec<_>>();
   self.trace.push(json!(["lpf",position(start),position(end),direction]));self.write(output,111)?;Ok(self.item("lpf"))
  }
 }
 #[test]
 fn original_nearest_search_preserves_pending_tasks_filters_and_fallback()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_nearest_search.json"))?;
  for (case_index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let input=&case["input"];let mut host=Replay::new(input)?;
   let args=Args{graph:G,point:INPUT,mode:host.number("mode"),output:OUT,parent:P,direction:host.number("direction"),workspace:if input["use_default"].as_bool().unwrap(){0}else{W},reset:host.number("reset"),limit:host.number("limit"),filter:Filter::default()};
   let result=query(&mut host,Context{frame:0x70000000,point_kind:0,marker_kind:0x4ebe80,spatial_class:0x11864},args)?;
   assert_eq!(json!(result),case["result"],"result case {case_index}");assert_eq!(json!(host.read(OUT)?),case["node"],"node case {case_index}");
   let workspace:Vec<_>=(0..6).map(|i|host.read(W+i*4).unwrap()).collect();assert_eq!(json!(workspace),case["workspace"],"workspace case {case_index}");
   assert_eq!(json!(host.trace),case["trace"],"trace case {case_index}");
  }Ok(())
 }
}
