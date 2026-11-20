//! Retail navigation sweeps: nearest polyline42c6a4/42c474,
//! cell obstacles42d6d8, unfiltered crossing42e68c and sweep42dbcc.
use super::{ai_graph_link_passes::Memory,ai_edge_crossing::{self,Globals},ai_cell_crossing::{self,Vectors},ai_spatial_walk::Filter};
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn scalar(m:&Memory,p:u32,o:u32)->Result<f32>{Ok(f32::from_bits(word(m,p,o)?))}
fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([word(m,p,0)?,word(m,p,4)?,word(m,p,8)?])}
fn put(m:&mut Memory,p:u32,v:[u32;3])->Result<()>{for (i,v) in v.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
fn count_between(a:u32,b:u32)->u32{((b.wrapping_sub(a) as i32)/12) as u32}
pub fn polyline(m:&mut Memory,g:Globals,v:Vectors,start:u32,end:u32,radius:u32,points:u32,count:u32,output:u32,normal:u32)->Result<u32>{
 if count<2{return Ok(0)}
 let r=f32::from_bits(radius);let mut nearest=f32::MAX;
 for i in 1..count{
  let a=point(m,start)?;let b=point(m,end)?;
  let edge=points.wrapping_add((i-1).wrapping_mul(12));let c=point(m,edge)?;let d=point(m,edge.wrapping_add(12))?;
  let mut hit=[0;3];
  let found=if r<=0.{ai_edge_crossing::intersection(m,g,a,b,c,d,Some(&mut hit),None)?}
   else{let mut p=[0.;3];let found=super::ai_radius_segment::intersection(m,g,a.map(f32::from_bits),b.map(f32::from_bits),c.map(f32::from_bits),d.map(f32::from_bits),r,Some(&mut p))?;hit=p.map(f32::to_bits);found};
  if found{let dz=f32::from_bits(hit[2])-scalar(m,start,8)?;let dx=f32::from_bits(hit[0])-scalar(m,start,0)?;let distance=dz*dz+dx*dx;
   if distance<nearest{nearest=distance;put(m,output,hit)?;ai_cell_crossing::normal(m,v,edge,normal)?;}}
 }
 g.finish(m)?;Ok(u32::from(nearest!=f32::MAX))
}
pub fn obstacles(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,flags:u32)->Result<u32>{
 let destination=scratch.wrapping_add(0x14);put(m,destination,point(m,end)?)?;let mut result=0;
 for (mask,count_offset,table_offset) in [(2,0x38,0x3c),(1,0x40,0x44)]{
  if flags&mask!=0 || word(m,cell,count_offset)?==0{continue}
  let mut table=cell.wrapping_add(word(m,cell,table_offset)?);let mut points=cell.wrapping_add(m.word(table)?);let mut i=0;
  while i<word(m,cell,count_offset)?{
   let next=cell.wrapping_add(word(m,table,4)?);
   if polyline(m,g,v,start,destination,radius,points,count_between(points,next),output,normal)?&255==1{put(m,destination,point(m,output)?)?;result=1}
   points=next;table=table.wrapping_add(4);i+=1;
  }
 }
 if flags&4!=0 || word(m,cell,0x48)?==0{return Ok(result)}
 let sx=scalar(m,start,0)?;let sz=scalar(m,start,8)?;let ex=scalar(m,end,0)?;let ez=scalar(m,end,8)?;
 if sx==ex && sz==ez{return Ok(result)}
 let dx=ex-sx;let dz=ez-sz;let length=super::cursor::native_sqrt(dx*dx+dz*dz);let dx=dx/length;let dz=dz/length;
 let mut table=cell.wrapping_add(word(m,cell,0x4c)?);let mut points=cell.wrapping_add(m.word(table)?);let mut i=0;
 while i<word(m,cell,0x48)?{
  let next=cell.wrapping_add(word(m,table,16)?);
  if !(dz*scalar(m,table,12)?+dx*scalar(m,table,4)?>=0.){
   let count=count_between(points,next);
   if super::ai_cell_obstacles::contains(m,start,points,count)?&255==1{put(m,output,point(m,start)?)?;return Ok(1)}
   if polyline(m,g,v,start,destination,radius,points,count,output,normal)?&255==1{put(m,destination,point(m,output)?)?;result=1}
  }
  points=next;table=table.wrapping_add(16);i+=1;
 }
 Ok(result)
}

///4305f8's polymorphic boundary. Interface records remain portable data;
/// absence of a concrete virtual20 implementation must be an error.
pub trait InterfaceHost{
 fn sweep20(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,info:u32)->Result<u32>;
}
pub fn filter_sweep(m:&mut Memory,filter:&mut Filter,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,info:Option<u32>,scratch:u32,host:&mut impl InterfaceHost)->Result<u32>{
 if let Some(info)=info{m.write(info,0)?;m.write(info.wrapping_add(4),0)?;}
 put(m,output,point(m,end)?)?;
 let list=filter.word(m)?;if list==0{return Ok(1)}
 let candidate=scratch.wrapping_add(0x18);let pair=scratch.wrapping_add(8);
 put(m,candidate,point(m,end)?)?;let mut node=word(m,list,8)?;let mut result=1;
 while node!=0{
  m.write(pair,0)?;m.write(pair.wrapping_add(4),0)?;
  let interface=word(m,node,4)?;
  if interface!=0 && host.sweep20(m,interface,cell,start,candidate,radius,output,normal,pair)?&255==0{
   put(m,candidate,point(m,output)?)?;result=0;
   if let Some(info)=info{m.write(info,m.word(pair)?)?;m.write(info.wrapping_add(4),word(m,pair,4)?)?;}
  }
  node=word(m,node,12)?;
 }
 Ok(result)
}
pub fn cross_unfiltered(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,mut cell:u32,start:u32,end:u32)->Result<u32>{
 let saved=scratch.wrapping_add(0x28);let hit=scratch.wrapping_add(0x1c);let normal=scratch.wrapping_add(0x10);
 let next=scratch.wrapping_add(8);let count=scratch.wrapping_add(12);let history=scratch.wrapping_add(0x38);
 put(m,saved,point(m,start)?)?;m.write(count,0)?;
 loop{
  m.write(next,0)?;put(m,hit,[0;3])?;put(m,normal,[0;3])?;
  if ai_cell_crossing::crossing(m,g,v,cell,saved,end,next,hit,normal,history,count)?&255==0{return Ok(0)}
  let target=m.word(next)?;if target==0 || target==cell{return Ok(cell)}
  put(m,saved,point(m,hit)?)?;cell=target;
 }
}
fn fallback(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,cell:u32,start:u32,end:u32,candidate:u32,result_cell:u32,output:u32,normal:u32)->Result<u32>{
 put(m,normal,point(m,v.up)?)?;
 let dz=scalar(m,candidate,8)?-scalar(m,start,8)?;let dx=scalar(m,candidate,0)?-scalar(m,start,0)?;
 let rz=scalar(m,end,8)?-scalar(m,start,8)?;let rx=scalar(m,end,0)?-scalar(m,start,0)?;
 if rz*dz+rx*dx<0.{put(m,output,point(m,start)?)?;m.write(result_cell,cell)?;return Ok(0)}
 let target=cross_unfiltered(m,g,v,scratch,cell,start,candidate)?;m.write(result_cell,target)?;
 if target==0{put(m,output,point(m,start)?)?;m.write(result_cell,cell)?;}
 else{put(m,output,point(m,candidate)?)?;}
 Ok(0)
}
fn correction(m:&mut Memory,g:Globals,origin:u32,start:u32,end:u32,output:u32)->Result<()>{
 let dx=scalar(m,end,0)?-scalar(m,start,0)?;let dz=scalar(m,end,8)?-scalar(m,start,8)?;
 let length=super::cursor::native_sqrt(dx*dx+dz*dz);let unit=[dx/length,0f32/length,dz/length];
 let scale=g.scale(m)?.unwrap_or(1.);let epsilon=f32::from_bits(0x3a83126f);
 let p=point(m,origin)?.map(f32::from_bits);
 put(m,output,std::array::from_fn(|i|(p[i]-scale*(epsilon*unit[i])).to_bits()))
}
fn forward(m:&Memory,origin:u32,candidate:u32,start:u32,end:u32)->Result<f32>{
 let dz=scalar(m,candidate,8)?-scalar(m,origin,8)?;let dx=scalar(m,candidate,0)?-scalar(m,origin,0)?;
 let rz=scalar(m,end,8)?-scalar(m,start,8)?;let rx=scalar(m,end,0)?-scalar(m,start,0)?;
 Ok(rz*dz+rx*dx)
}
///42dbcc. Caller maps a distinct0x500-byte scratch region for the original
/// frame and nested callees. A partial sweep returns0 with a meaningful cell
/// and point; callers must not collapse the result to a boolean clear test.
pub fn sweep(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,cell:u32,start:u32,end:u32,radius:u32,result_cell:u32,output:u32,normal:u32,filter:&mut Option<Filter>,info:Option<u32>,flags:u32,host:&mut impl InterfaceHost)->Result<u32>{
 if scalar(m,start,0)?==scalar(m,end,0)? && scalar(m,start,8)?==scalar(m,end,8)?{
  let y=scalar(m,end,4)?;let inside=y<=scalar(m,cell,0x18)? && y>=scalar(m,cell,0x1c)?;
  put(m,output,point(m,start)?)?;m.write(result_cell,if inside{cell}else{0})?;return Ok(u32::from(inside))
 }
 let saved=scratch.wrapping_add(0x10c);let desired=scratch.wrapping_add(0x100);let corrected=scratch.wrapping_add(0xf4);let hit=scratch.wrapping_add(0xe8);
 let next=scratch.wrapping_add(8);let count=scratch.wrapping_add(12);let history=scratch.wrapping_add(0x118);
 let obstacle_frame=scratch.wrapping_add(0x200);let fallback_frame=scratch.wrapping_add(0x300);let filter_frame=scratch.wrapping_add(0x400);
 let mut current=cell;put(m,saved,point(m,start)?)?;put(m,output,point(m,end)?)?;
 let margin=scalar(m,cell,0x10)?;let bias=f32::from_bits(0x3a83126f)*g.scale(m)?.unwrap_or(1.);
 let mut clearance=bias+(f32::from_bits(radius)-margin);if clearance<bias{clearance=bias}
 m.write(count,0)?;
 loop{
  put(m,desired,point(m,end)?)?;put(m,corrected,point(m,end)?)?;m.write(next,0)?;put(m,hit,[0;3])?;
  if ai_cell_crossing::crossing(m,g,v,current,saved,desired,next,hit,normal,history,count)?&255==0{
   put(m,output,point(m,start)?)?;m.write(result_cell,cell)?;put(m,normal,point(m,v.up)?)?;return Ok(0)
  }
  put(m,desired,point(m,hit)?)?;let mut partial=false;
  if m.word(next)?==0{
   correction(m,g,hit,start,end,corrected)?;
   if forward(m,saved,corrected,start,end)?<=0.{return fallback(m,g,v,fallback_frame,cell,start,end,corrected,result_cell,output,normal)}
   partial=true;put(m,desired,point(m,corrected)?)?;
  }
  if obstacles(m,g,v,obstacle_frame,current,saved,desired,clearance.to_bits(),corrected,normal,flags)?&255!=0{
   if forward(m,saved,corrected,start,end)?<=0.{
    correction(m,g,saved,start,end,corrected)?;return fallback(m,g,v,fallback_frame,cell,start,end,corrected,result_cell,output,normal)
   }
   partial=true;put(m,desired,point(m,corrected)?)?;
  }
  if flags&8==0{
   if let Some(filter)=filter.as_mut(){
    if filter_sweep(m,filter,current,saved,desired,radius,corrected,normal,info,filter_frame,host)?&255==0{
     if forward(m,saved,corrected,start,end)?<=0.{
      correction(m,g,saved,start,end,corrected)?;return fallback(m,g,v,fallback_frame,cell,start,end,corrected,result_cell,output,normal)
     }
     partial=true;put(m,desired,point(m,corrected)?)?;
    }
   }
  }
  if partial{put(m,output,point(m,corrected)?)?;m.write(result_cell,current)?;return Ok(0)}
  let target=m.word(next)?;
  if target==current{put(m,output,point(m,end)?)?;m.write(result_cell,current)?;return Ok(1)}
  put(m,saved,point(m,hit)?)?;current=target;
 }
}

///4314b0: provider lookup and optional input/output floor caches around42dbcc.
/// Scratch requires0x600 mapped bytes; a failed lookup retains the normal.
pub fn provider_sweep<H:InterfaceHost+super::ai_cell_trace::InterfaceHost>(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,provider:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,input_cache:Option<u32>,output_cache:Option<u32>,filter:&mut Option<Filter>,info:Option<u32>,flags:u32,host:&mut H)->Result<u32>{
 const EMPTY:[u32;7]=[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,0];
 fn clear(m:&mut Memory,p:u32)->Result<()>{for (i,value) in EMPTY.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),value)?}Ok(())}
 let cache=if let Some(cache)=input_cache{cache}else{let cache=scratch.wrapping_add(0x14);clear(m,cache)?;cache};
 {
  let mut lookup=super::ai_cell_trace::Native{globals:g,vectors:v,scratch:scratch.wrapping_add(0x100),filter_host:super::ai_cell_trace::Filters{interfaces:&mut *host}};
  super::ai_spatial_walk::select(m,provider,start,cache,&mut lookup)?;
 }
 let cell=word(m,cache,0x18)?;
 if cell==0{put(m,output,point(m,start)?)?;if let Some(cache)=output_cache{clear(m,cache)?}return Ok(0)}
 let last=scratch.wrapping_add(0x10);m.write(last,0)?;
 let result=sweep(m,g,v,scratch.wrapping_add(0x100),cell,start,end,radius,last,output,normal,filter,info,flags,host)?;
 if let Some(cache)=output_cache{
  let cell=m.word(last)?;
  if cell==0{clear(m,cache)?}
  else{
   let index=super::ai_spatial_walk::sector_index(m,provider,m.word(cell)?)?;
   if index==u32::MAX{clear(m,cache)?}
   else{
    let words=[word(m,cell,0)?,word(m,cell,4)?,word(m,cell,8)?,word(m,cell,12)?,provider,index,cell];
    for (i,value) in words.into_iter().enumerate(){m.write(cache.wrapping_add(i as u32*4),value)?;}
   }
  }
 }
 Ok(result)
}

#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};
 struct Interfaces<'a>{input:&'a Value,trace:Vec<Value>}
 impl InterfaceHost for Interfaces<'_>{
  fn sweep20(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,info:u32)->Result<u32>{
   self.trace.push(json!([interface,cell,point(m,start)?,point(m,end)?,radius,point(m,output)?,point(m,normal)?,[m.word(info)?,word(m,info,4)?]]));
   let i=((interface-0x69000000)/16) as usize;let result=self.input["results"][i].as_u64().unwrap() as u32;
   if result&255==0{
    let p=std::array::from_fn(|j|self.input["clips"][i][j].as_u64().unwrap() as u32);put(m,output,p)?;put(m,normal,[1f32.to_bits(),0,0])?;
    m.write(info,111+i as u32)?;m.write(info.wrapping_add(4),222+i as u32)?;
   }
   Ok(result)
  }
 }
 impl super::super::ai_cell_trace::InterfaceHost for Interfaces<'_>{
  fn accept1c(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Provider floor lookup unexpectedly invoked trace filtering")}
 }
 #[test]
 fn retail_navigation_sweep_family_matches_geometry_outputs_and_filter_clipping()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_sweep.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x62000000,vec![0;0x1800]),(0x63000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20]),(0x564d0,vec![0;0x40]),(0x64000000,vec![0;0x40]),(0x65000000,vec![0;0x600]),(0x67000000,vec![0;16]),(0x68000000,vec![0;64]),(0x69000000,vec![0;64]),(0x6a000000,vec![0;64]),(0x6b000000,vec![0;0x500]),(0x6c000000,vec![0;64])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let input=&case["input"];let output=if input["alias"].as_bool().unwrap(){0x63000010}else{0x63000030};
   let g=Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0};let v=Vectors{up:0x564f4,zero:0x564d0};
   let radius=input["radius"].as_u64().unwrap() as u32;let flags=input["flags"].as_u64().unwrap() as u32;
   let info=input["info"].as_bool().unwrap().then_some(0x64000010);let mut host=Interfaces{input,trace:vec![]};
   let mut filter=(!input["filter"].is_null()).then_some(Filter::Address(0x64000000));
   let result=match input["op"].as_str().unwrap(){
    "provider"=>provider_sweep(&mut m,g,v,0x65000000,0x6b000000,0x63000000,0x63000010,radius,output,0x63000040,(input["cache"]!="none").then_some(0x6c000000),match input["cache_out"].as_str().unwrap(){"none"=>None,"same"=>Some(0x6c000000),_=>Some(0x6c000020)},&mut filter,info,flags,&mut host)?,
    "polyline"=>polyline(&mut m,g,v,0x63000000,0x63000010,radius,0x62000650,input["vertices"].as_u64().unwrap() as u32,output,0x63000040)?,
    "obstacles"=>obstacles(&mut m,g,v,0x65000000,0x62000000,0x63000000,0x63000010,radius,output,0x63000040,flags)?,
    "cross"=>cross_unfiltered(&mut m,g,v,0x65000000,0x62000000,0x63000000,0x63000010)?,
    "fallback"=>fallback(&mut m,g,v,0x65000000,0x62000000,0x63000000,0x63000010,0x64000020,0x63000020,output,0x63000040)?,
    "filter"=>filter_sweep(&mut m,&mut Filter::Address(0x64000000),0x62000000,0x63000000,0x63000010,radius,output,0x63000040,info,0x65000000,&mut host)?,
    "sweep"=>sweep(&mut m,g,v,0x65000000,0x62000000,0x63000000,0x63000010,radius,0x63000020,output,0x63000040,&mut filter,info,flags,&mut host)?,_=>unreachable!(),
   };
   let mut actual=json!({"result":result,"point":point(&m,output)?,"normal":point(&m,0x63000040)?,"next":m.word(0x63000020)?,"info":[m.word(0x64000010)?,m.word(0x64000014)?],"end":point(&m,0x63000010)?,"trace":host.trace,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   if input["op"]=="provider"{actual["cache"]=json!((0..7).map(|i|m.word(0x6c000000+i*4)).collect::<Result<Vec<_>>>()?);actual["cache_out"]=json!((0..7).map(|i|m.word(0x6c000020+i*4)).collect::<Result<Vec<_>>>()?);}
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }
}
