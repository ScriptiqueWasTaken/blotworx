//! Retail base spatial24 endpoint recovery1c48f8 and selector wrapper34
//! 1c5160. Geometry hooks and virtual24 dispatch are required native boundaries.
use super::{ai_graph_link_passes::Memory,ai_spatial_walk::Filter,ai_spatial_service};
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn scalar(m:&Memory,p:u32,o:u32)->Result<f32>{Ok(f32::from_bits(word(m,p,o)?))}
fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([word(m,p,0)?,word(m,p,4)?,word(m,p,8)?])}
fn put(m:&mut Memory,p:u32,v:[u32;3])->Result<()>{for (i,v) in v.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
fn clear_cache(m:&mut Memory,p:u32)->Result<()>{for (i,v) in [u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,0].into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
fn create_point(m:&mut Memory,p:u32,kind:u32,position:[u32;3])->Result<()>{
 m.write(p,kind)?;put(m,p.wrapping_add(4),position)?;
 for i in 0..16{m.write(p.wrapping_add(0x10+i*4),0)?;}Ok(())
}
#[derive(Clone,Copy)]
pub struct Context{pub spatial:ai_spatial_service::Context,pub point_kind:u32,pub cache_kind:u32,pub recovery_callback_slot:u32}
pub trait Host:super::ai_navigation_sweep::InterfaceHost+super::ai_cell_trace::InterfaceHost{
 /// Raw arena ownership is the native data implementation. Typed canonical
 /// pathfinders override these two accessors instead of maintaining mirrors.
 fn owner_provider(&mut self,m:&mut Memory,service:u32)->Result<u32>{
  let owner=word(m,service,4)?;let preferred=word(m,owner,0x54)?;
  let record=if preferred!=0{preferred}else{word(m,owner,0x50)?};
  if record==0{Ok(0)}else{word(m,record,0x4c)}
 }
 fn owner_entity(&mut self,m:&mut Memory,service:u32)->Result<u32>{word(m,word(m,word(m,service,4)?,4)?,0x14)}
 fn select_filters(&mut self,m:&mut Memory,service:u32,output:u32,mode:u32)->Result<()>{ai_spatial_service::select_filters(m,service,output,mode)}
 fn recovery(&mut self,m:&mut Memory,callback:u32,start:u32,recovered:u32,mode:u32,context_owner:u32,radius:u32)->Result<u32>;
 fn query24(&mut self,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>)->Result<u32>;
}
/// Caller maps a distinct0x800-byte scratch frame. Virtual dispatch owns nested
/// frames, so recovery is not limited by an invented recursion/route cap.
pub fn query24(m:&mut Memory,c:Context,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>,host:&mut impl Host)->Result<u32>{
 let provider=host.owner_provider(m,service)?;if provider==0{return Ok(0)}
 let frame=c.spatial.scratch;let local_cache=frame.wrapping_add(0x64);clear_cache(m,local_cache)?;
 let cache=ai_spatial_service::attribute(m,end,c.spatial.point_cache_metadata)?;
 let cache=if cache!=0{cache.wrapping_add(4)}else{local_cache};
 let recovered=frame.wrapping_add(0x1c);let normal=frame.wrapping_add(0x10);let output_cache=frame.wrapping_add(0x48);
 put(m,recovered,point(m,start.wrapping_add(4))?)?;put(m,normal,[0;3])?;clear_cache(m,output_cache)?;
 super::ai_navigation_sweep::provider_sweep(m,c.spatial.globals,c.spatial.vectors,frame.wrapping_add(0x200),provider,end.wrapping_add(4),start.wrapping_add(4),radius,recovered,normal,Some(cache),Some(output_cache),&mut None,None,6,host)?;
 if word(m,cache,0x18)?==0{
  let dx=scalar(m,end,4)?-scalar(m,start,4)?;let dz=scalar(m,end,12)?-scalar(m,start,12)?;
  let squared=dz.mul_add(dz,dx*dx);let r=f32::from_bits(radius);
  return Ok(u32::from(!(squared>r*r)))
 }
 if scalar(m,start,4)?==scalar(m,recovered,0)? && scalar(m,start,12)?==scalar(m,recovered,8)?{return Ok(0)}
 if word(m,output_cache,0x18)?==0{return Ok(0)}
 let slot=word(m,output_cache,0x14)?;let table=word(m,provider,8)?;let sector=word(m,table,slot.wrapping_mul(8).wrapping_add(4))?;
 let bound=2.*(scalar(m,sector,0x2c)?+scalar(m,sector,0x10)?);
 let dx=scalar(m,recovered,0)?-scalar(m,start,4)?;let dz=scalar(m,recovered,8)?-scalar(m,start,12)?;
 if dz.mul_add(dz,dx*dx)>bound*bound{return Ok(0)}
 let probe=frame.wrapping_add(0x80);create_point(m,probe,c.point_kind,point(m,recovered)?)?;
 let callback=m.word(c.recovery_callback_slot)?;
 let context=host.owner_entity(m,service)?;
 if callback!=0 && host.recovery(m,callback,start,probe,4,context,(2.*f32::from_bits(radius)).to_bits())?&255!=0{return Ok(0)}
 let retry=frame.wrapping_add(0xd0);create_point(m,retry,c.point_kind,point(m,recovered)?)?;
 let saved=std::array::from_fn::<_,7,_>(|i|word(m,output_cache,i as u32*4));let saved=saved.into_iter().collect::<Result<Vec<_>>>()?;
 let cache_attribute=frame.wrapping_add(0x28);m.write(cache_attribute,c.cache_kind)?;
 for (i,value) in saved.into_iter().enumerate(){m.write(cache_attribute.wrapping_add(4+i as u32*4),value)?;}
 // Cache class4ec160's virtual0c is2b7650, which returns41d64.
 let slot=word(m,c.spatial.point_cache_metadata,8)?;
 m.write(retry.wrapping_add(0x10).wrapping_add(slot.wrapping_mul(4)),cache_attribute)?;
 host.query24(m,service,retry,end,radius,filter)
}
///1c5160 selects a live filter-set word before virtual24; it does not dispatch
/// to the distinct3c sweep operation or normalize its virtual return value.
pub fn query34(m:&mut Memory,scratch:u32,service:u32,start:u32,end:u32,radius:u32,mode:u32,host:&mut impl Host)->Result<u32>{
 let selected=scratch.wrapping_add(8);m.write(selected,0)?;
 host.select_filters(m,service,selected,mode)?;
 host.query24(m,service,start,end,radius,&mut Some(Filter::Address(selected)))
}

/// Compose native geometry/acceptance with the remaining explicitly supplied
/// traversal services. Function tags are data identities supplied by the class
/// registry; they are never called as Wii/host executable addresses.
pub struct Bound<H>{pub context:Context,pub argument_frame:u32,pub selector34_tag:u32,pub host:H}
pub trait Services:Host+super::ai_spatial_queries::Host{}
impl<H:Host+super::ai_spatial_queries::Host> Services for H{}
impl<H:Services> super::ai_traversal_exclusion::Services for Bound<H>{
 fn class20(&mut self,m:&mut Memory,service:u32)->Result<u32>{self.host.class20(m,service)}
}
impl<H:Services> super::ai_traversal_cost::Host for Bound<H>{
 fn exclusion_world(&self)->super::ai_traversal_exclusion::World{self.host.exclusion_world()}
 fn class18(&mut self,m:&mut Memory,interface:u32)->Result<u32>{self.host.class18(m,interface)}
 fn query40(&mut self,m:&mut Memory,service:u32,start:&mut super::ai_segment_query::WalkPoint,end:&mut[u32;3],radius:u32,provider:&mut u32,pool:u32)->Result<u32>{self.host.query40(m,service,start,end,radius,provider,pool)}
 fn contribution2c(&mut self,m:&mut Memory,interface:u32,context_owner:u32,start:&mut super::ai_segment_query::WalkPoint,end:&mut super::ai_segment_query::WalkPoint,value:u32,cost:&mut u32)->Result<u32>{self.host.contribution2c(m,interface,context_owner,start,end,value,cost)}
}
impl<H:Services> super::ai_spatial_walk::Host for Bound<H>{
 fn trace42e46c(&mut self,m:&mut Memory,cell:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>,flags:u32)->Result<u32>{
  let mut filters=super::ai_cell_trace::Filters{interfaces:&mut self.host};
  super::ai_cell_trace::trace(m,self.context.spatial.globals,self.context.spatial.vectors,self.context.spatial.scratch,cell,start,end,radius,filter,flags,&mut filters)
 }
}
fn publish_point(m:&mut Memory,p:u32,kind:u32,value:&super::ai_segment_query::WalkPoint)->Result<()>{
 m.write(p,kind)?;put(m,p.wrapping_add(4),value.position)?;
 for (i,v) in value.auxiliary.into_iter().enumerate(){m.write(p.wrapping_add(0x10+i as u32*4),v)?;}Ok(())
}
fn receive_point(m:&Memory,p:u32,value:&mut super::ai_segment_query::WalkPoint)->Result<()>{
 value.position=point(m,p.wrapping_add(4))?;
 for i in 0..16{value.auxiliary[i]=word(m,p,0x10+i as u32*4)?;}Ok(())
}
impl<H:Services> super::ai_spatial_queries::Host for Bound<H>{
 fn cost_classes(&self)->super::ai_traversal_cost::Classes{self.host.cost_classes()}
 fn owner_tag3c(&mut self,m:&mut Memory,owner:u32)->Result<u32>{self.host.owner_tag3c(m,owner)}
 fn sweep34(&mut self,m:&mut Memory,service:u32,start:&mut super::ai_segment_query::WalkPoint,end:&mut super::ai_segment_query::WalkPoint,mode:u32,radius:u32)->Result<u32>{
  let vtable=m.word(service)?;
  if word(m,vtable,0x34)?!=self.selector34_tag{return self.host.sweep34(m,service,start,end,mode,radius)}
  let first=self.argument_frame;let second=first.wrapping_add(0x50);
  publish_point(m,first,self.context.point_kind,start)?;publish_point(m,second,self.context.point_kind,end)?;
  let result=query34(m,self.context.spatial.scratch,service,first,second,radius,mode,&mut self.host);
  receive_point(m,first,start)?;receive_point(m,second,end)?;result
 }
 fn direct1c(&mut self,m:&mut Memory,service:u32,owner:u32,start:&mut super::ai_segment_query::WalkPoint,end:&mut super::ai_segment_query::WalkPoint,tag:u32,cost:&mut u32)->Result<u32>{self.host.direct1c(m,service,owner,start,end,tag,cost)}
 fn adjust20(&mut self,m:&mut Memory,service:u32,owner:u32,start:&mut super::ai_segment_query::WalkPoint,end:&mut super::ai_segment_query::WalkPoint,tag:u32,cost:&mut u32)->Result<()>{self.host.adjust20(m,service,owner,start,end,tag,cost)}
}

#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};
 fn object(m:&Memory,p:u32)->Result<Value>{
  let mut attributes=Vec::new();
  for i in 0..16{let attribute=word(m,p,0x10+i*4)?;
   attributes.push(if attribute==0{Value::Null}else{json!({"kind":m.word(attribute)?,"words":(0..7).map(|j|word(m,attribute,4+j*4)).collect::<Result<Vec<_>>>()?})});
  }
  Ok(json!({"kind":m.word(p)?,"position":point(m,p.wrapping_add(4))?,"attributes":attributes}))
 }
 struct Interfaces<'a>{input:&'a Value,context:Context,next_frame:u32,trace:Vec<Value>}
 impl super::super::ai_cell_trace::InterfaceHost for Interfaces<'_>{
  fn accept1c(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Acceptance floor lookup unexpectedly invoked trace filters")}
 }
 impl super::super::ai_navigation_sweep::InterfaceHost for Interfaces<'_>{
  fn sweep20(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Base acceptance unexpectedly invoked sweep filters")}
 }
 impl Host for Interfaces<'_>{
  fn recovery(&mut self,m:&mut Memory,callback:u32,start:u32,recovered:u32,mode:u32,context_owner:u32,radius:u32)->Result<u32>{
   self.trace.push(json!({"kind":"recovery","callback":callback,"start":object(m,start)?,"recovered":object(m,recovered)?,"mode":mode,"context":context_owner,"radius":radius}));
   if self.input["mutate"].as_bool().unwrap(){m.write(recovered.wrapping_add(4),99f32.to_bits())?;}
   Ok(self.input["callback"].as_u64().unwrap() as u32)
  }
  fn query24(&mut self,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>)->Result<u32>{
   self.trace.push(json!({"kind":"query","service":service,"start":object(m,start)?,"end":object(m,end)?,"radius":radius,"filter":filter.as_ref().map(|f|f.word(m)).transpose()?}));
   let original=if self.input["op"]=="backend"{0x66000000}else{0x6d000000};
   if start==original{if let Some(result)=self.input["initial_override"].as_u64(){
    m.write(start.wrapping_add(4),9f32.to_bits())?;m.write(end.wrapping_add(12),11f32.to_bits())?;
    m.write(end.wrapping_add(0x10).wrapping_add(self.input["slot"].as_u64().unwrap() as u32*4),0)?;return Ok(result as u32)
   }}
   if start!=original{if let Some(result)=self.input["child"].as_u64(){return Ok(result as u32)}}
   let frame=self.next_frame;self.next_frame=self.next_frame.checked_add(0x1000).ok_or_else(||anyhow::anyhow!("Test frame domain exhausted"))?;
   anyhow::ensure!(m.regions.insert(frame,vec![0;0x800]).is_none(),"Test frame overlaps live data");
   let mut c=self.context;c.spatial.scratch=frame;
   let result=query24(m,c,service,start,end,radius,filter,self);m.regions.remove(&frame);result
  }
 }

 impl super::super::ai_traversal_exclusion::Services for Interfaces<'_>{
  fn class20(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Unexpected exclusion dispatch in acceptance fixture")}
 }
 impl super::super::ai_traversal_cost::Host for Interfaces<'_>{
  fn exclusion_world(&self)->super::super::ai_traversal_exclusion::World{unreachable!("Unexpected cost world in acceptance fixture")}
  fn class18(&mut self,m:&mut Memory,interface:u32)->Result<u32>{anyhow::ensure!(word(m,m.word(interface)?,0x18)?==0x2b7b30,"Unknown fixture class getter");Ok(0x11864)}
  fn query40(&mut self,_:&mut Memory,_:u32,_:&mut super::super::ai_segment_query::WalkPoint,_:&mut[u32;3],_:u32,_:&mut u32,_:u32)->Result<u32>{anyhow::bail!("Unexpected cost geometry in acceptance fixture")}
  fn contribution2c(&mut self,_:&mut Memory,_:u32,_:u32,_:&mut super::super::ai_segment_query::WalkPoint,_:&mut super::super::ai_segment_query::WalkPoint,_:u32,_:&mut u32)->Result<u32>{anyhow::bail!("Unexpected cost contribution in acceptance fixture")}
 }
 impl super::super::ai_spatial_walk::Host for Interfaces<'_>{
  fn trace42e46c(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:&mut Option<Filter>,_:u32)->Result<u32>{anyhow::bail!("Bound adapter must supply native geometry")}
 }
 impl super::super::ai_spatial_queries::Host for Interfaces<'_>{
  fn cost_classes(&self)->super::super::ai_traversal_cost::Classes{unreachable!("Unexpected cost classes in acceptance fixture")}
  fn owner_tag3c(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Unexpected owner cost tag in acceptance fixture")}
  fn sweep34(&mut self,_:&mut Memory,_:u32,_:&mut super::super::ai_segment_query::WalkPoint,_:&mut super::super::ai_segment_query::WalkPoint,_:u32,_:u32)->Result<u32>{anyhow::bail!("Bound adapter must dispatch verified base34")}
  fn direct1c(&mut self,_:&mut Memory,_:u32,_:u32,_:&mut super::super::ai_segment_query::WalkPoint,_:&mut super::super::ai_segment_query::WalkPoint,_:u32,_:&mut u32)->Result<u32>{anyhow::bail!("Unexpected direct cost query in acceptance fixture")}
  fn adjust20(&mut self,_:&mut Memory,_:u32,_:u32,_:&mut super::super::ai_segment_query::WalkPoint,_:&mut super::super::ai_segment_query::WalkPoint,_:u32,_:&mut u32)->Result<()>{anyhow::bail!("Unexpected adjustment query in acceptance fixture")}
 }
 fn walk_point(m:&Memory,p:u32)->Result<super::super::ai_segment_query::WalkPoint>{
  let mut point=super::super::ai_segment_query::WalkPoint{position:point(m,p.wrapping_add(4))?,auxiliary:[0;16]};
  for i in 0..16{point.auxiliary[i]=word(m,p,0x10+i as u32*4)?;}Ok(point)
 }
 fn walk_snapshot(m:&Memory,p:&super::super::ai_segment_query::WalkPoint)->Result<Value>{
  let mut attributes=Vec::new();
  for &a in &p.auxiliary{attributes.push(if a==0{Value::Null}else{json!({"kind":m.word(a)?,"words":(0..7).map(|i|word(m,a,4+i*4)).collect::<Result<Vec<_>>>()?})});}
  Ok(json!({"kind":0x4ec1b0u32,"position":p.position,"attributes":attributes}))
 }
 #[test]
 fn retail_spatial_acceptance_recovers_endpoints_and_preserves_virtual_dispatch()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_acceptance.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x62000000,vec![0;0x1800]),(0x63000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20]),(0x564d0,vec![0;0x40]),(0x64000000,vec![0;0x40]),(0x65000000,vec![0;0x800]),(0x67000000,vec![0;16]),(0x68000000,vec![0;64]),(0x69000000,vec![0;64]),(0x6a000000,vec![0;64]),(0x6b000000,vec![0;0x500]),(0x6c000000,vec![0;64]),(0x6d000000,vec![0;0x300]),(0x6e000000,vec![0;0x1100]),(0x41d64,vec![0;16]),(0x6f000000,vec![0;32]),(0x4e14c,vec![0;4]),(0x66000000,vec![0;0xa0]),(0x74000000,vec![0;0x100]),(0x73000000,vec![0;0x300]),(0x361b4,vec![0;16])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let c=Context{spatial:ai_spatial_service::Context{globals:super::super::ai_edge_crossing::Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0},vectors:super::super::ai_cell_crossing::Vectors{up:0x564f4,zero:0x564d0},point_cache_metadata:0x41d64,scratch:0x65000000},point_kind:0x4ec1b0,cache_kind:0x4ec160,recovery_callback_slot:0x4e14c};
   let input=&case["input"];let mut host=Interfaces{input,context:c,next_frame:0x70000000,trace:vec![]};let radius=input["radius"].as_u64().unwrap() as u32;
   let mut filter=(!input["filter"].is_null()).then_some(Filter::Address(0x64000000));
   let (result,start_after,end_after)=if input["op"]=="backend"{
    let mut start=walk_point(&m,0x6d000000)?;let mut end=walk_point(&m,0x6d000200)?;
    let bound=Bound{context:c,argument_frame:0x66000000,selector34_tag:0x1c5160,host};
    let mut native=super::super::ai_spatial_queries::Native{globals:c.spatial.globals,spatial_class:0x11864,radius_metadata:0x361b4,host:bound};
    let result=u32::from(native.accept_points(&mut m,0x74000000,&mut start,&mut end)?);host=native.host.host;
    (result,walk_snapshot(&m,&start)?,walk_snapshot(&m,&end)?)
   }else{
    let result=if input["op"]=="wrapper"{query34(&mut m,0x65000000,0x6e000000,0x6d000000,0x6d000200,radius,input["mode"].as_u64().unwrap() as u32,&mut host)?}
     else{query24(&mut m,c,0x6e000000,0x6d000000,0x6d000200,radius,&mut filter,&mut host)?};
    (result,object(&m,0x6d000000)?,object(&m,0x6d000200)?)
   };
   let actual=json!({"result":result,"trace":host.trace,"start_after":start_after,"end_after":end_after,"cache":(0..7).map(|i|m.word(0x6d000104+i*4)).collect::<Result<Vec<_>>>()?,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }
}
