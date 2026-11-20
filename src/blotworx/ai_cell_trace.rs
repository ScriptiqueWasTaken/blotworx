//! Concrete spatial trace42e46c, joining native crossings and cell obstacles.
//! Filter430524 is an explicit required boundary, never assumed permissive.
use super::{ai_graph_link_passes::Memory,ai_edge_crossing::Globals,ai_cell_crossing::Vectors,ai_spatial_walk::{Filter,Host as WalkHost}};
use anyhow::Result;
fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([m.word(p)?,m.word(p.wrapping_add(4))?,m.word(p.wrapping_add(8))?])}
fn put(m:&mut Memory,p:u32,v:[u32;3])->Result<()>{for (i,v) in v.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
pub trait FilterHost{
    fn accept430524(&mut self,m:&mut Memory,filter:&mut Filter,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>;
}
pub trait InterfaceHost{
    fn accept1c(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>;
}
impl<H:InterfaceHost+?Sized> InterfaceHost for &mut H{
    fn accept1c(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>{
        (**self).accept1c(m,interface,cell,start,end,radius)
    }
}
///430524 captures the list once, but rereads each node's next link after its
/// callback. Null list/interface records have intrinsic native behavior.
pub struct Filters<H>{pub interfaces:H}
impl<H:InterfaceHost> FilterHost for Filters<H>{
    fn accept430524(&mut self,m:&mut Memory,filter:&mut Filter,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>{
        let list=filter.word(m)?;if list==0{return Ok(1)}
        let mut node=m.word(list.wrapping_add(8))?;
        while node!=0{
            let interface=m.word(node.wrapping_add(4))?;
            if interface!=0 && self.interfaces.accept1c(m,interface,cell,start,end,radius)?&255==0{return Ok(0)}
            node=m.word(node.wrapping_add(12))?;
        }
        Ok(1)
    }
}
/// Scratch denotes a distinct caller-owned mapped0xf0-byte data region, as in
/// the original stack frame. No portable data address is executed as code.
pub struct Native<H>{pub globals:Globals,pub vectors:Vectors,pub scratch:u32,pub filter_host:H}
impl<H:FilterHost> WalkHost for Native<H>{
    fn trace42e46c(&mut self,m:&mut Memory,cell:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>,flags:u32)->Result<u32>{
        trace(m,self.globals,self.vectors,self.scratch,cell,start,end,radius,filter,flags,&mut self.filter_host)
    }
}
pub fn trace(m:&mut Memory,g:Globals,v:Vectors,scratch:u32,mut cell:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>,flags:u32,host:&mut impl FilterHost)->Result<u32>{
    let saved=scratch.wrapping_add(0x58);let hit=scratch.wrapping_add(0x34);let normal=scratch.wrapping_add(0x28);
    let next=scratch.wrapping_add(8);let count=scratch.wrapping_add(12);let history=scratch.wrapping_add(0x68);let candidate=scratch.wrapping_add(0x4c);
    put(m,saved,point(m,start)?)?;
    let margin=f32::from_bits(m.word(cell.wrapping_add(0x10))?);
    let bias=f32::from_bits(0x3a03126f)*g.scale(m)?.unwrap_or(1.);
    let original_radius=f32::from_bits(radius);let mut clearance=bias+(original_radius-margin);
    if clearance<bias{clearance=bias}
    m.write(count,0)?;
    let mut obstacles=super::ai_radius_segment::Native{globals:g};
    while cell!=0{
        put(m,candidate,point(m,end)?)?;m.write(next,0)?;put(m,hit,[0;3])?;put(m,normal,[0;3])?;
        if super::ai_cell_crossing::crossing(m,g,v,cell,saved,end,next,hit,normal,history,count)?&255==0{return Ok(0)}
        if m.word(next)?==0{return Ok(0)}
        put(m,candidate,point(m,hit)?)?;
        if super::ai_cell_obstacles::blocked(m,cell,saved,candidate,clearance.to_bits(),flags,&mut obstacles)?&255!=0{return Ok(0)}
        if flags&8==0{
            if let Some(filter)=filter.as_mut(){if host.accept430524(m,filter,cell,saved,candidate,radius)?&255==0{return Ok(0)}}
        }
        let target=m.word(next)?;
        if target==cell{return Ok(cell)}
        put(m,saved,point(m,hit)?)?;cell=target;
    }
    Ok(0)
}

#[cfg(test)]
mod tests{
 use super::*;
 use serde_json::{Value,json};
 struct Admission<'a>{input:&'a Value,trace:Vec<Value>}
 impl FilterHost for Admission<'_>{
  fn accept430524(&mut self,m:&mut Memory,filter:&mut Filter,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>{
   let value=filter.word(m)?;self.trace.push(json!([value,cell,point(m,start)?,point(m,end)?,radius]));
   if self.input["mutate"].as_bool().unwrap(){filter.set_word(m,value.wrapping_add(1))?;m.write(end.wrapping_add(4),1.25f32.to_bits())?;}
   Ok(self.input["accept"].as_u64().unwrap() as u32)
  }
 }
 #[test]
 fn retail_cell_trace_joins_real_geometry_and_preserves_filter_mutations()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_cell_trace.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){for address_filter in [false,true]{
   let mut m=Memory{regions:[(0x62000000,vec![0;0x1800]),(0x63000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20]),(0x564d0,vec![0;0x40]),(0x64000000,vec![0;4]),(0x65000000,vec![0;0xf0])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let input=&case["input"];
   let mut filter=input["filter"].as_u64().map(|v|if address_filter{Filter::Address(0x64000000)}else{Filter::Value(v as u32)});
   let mut native=Native{globals:Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0},vectors:Vectors{up:0x564f4,zero:0x564d0},scratch:0x65000000,filter_host:Admission{input,trace:vec![]}};
   let result=native.trace42e46c(&mut m,0x62000000,0x63000000,0x63000010,(input["radius"].as_f64().unwrap() as f32).to_bits(),&mut filter,input["flags"].as_u64().unwrap() as u32)?;
   let actual=json!({"result":result,"filter":filter.map(|f|f.word(&m)).transpose()?.unwrap_or(0),"trace":native.filter_host.trace,"start":point(&m,0x63000000)?,"end":point(&m,0x63000010)?,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}, address_filter={address_filter}: {input}");}
  }}
  Ok(())
 }
}

#[cfg(test)]
mod filter_tests{
 use super::*;use serde_json::{Value,json};
 struct Interfaces<'a>{input:&'a Value,trace:Vec<Value>}
 impl InterfaceHost for Interfaces<'_>{
  fn accept1c(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,_:u32)->Result<u32>{
   self.trace.push(json!([interface,cell,start,end]));let i=(interface-0x69000000)/16;
   if self.input["mutate"].as_bool().unwrap() && i==0{m.write(0x6800000c,0x68000020)?;m.write(0x64000000,0)?;}
   Ok(self.input["results"][i as usize].as_u64().unwrap() as u32)
  }
 }
 #[test]
 fn retail_cell_filter_walk_observes_callback_link_changes()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_cell_filter.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x63000000,vec![0;32]),(0x64000000,vec![0;4]),(0x67000000,vec![0;16]),(0x68000000,vec![0;64]),(0x69000000,vec![0;64]),(0x6a000000,vec![0;32])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let mut filters=Filters{interfaces:Interfaces{input:&case["input"],trace:vec![]}};
   let result=filters.accept430524(&mut m,&mut Filter::Address(0x64000000),123,0x63000000,0x63000010,0.5f32.to_bits())?;
   let actual=json!({"result":result,"trace":filters.interfaces.trace,"filter":m.word(0x64000000)?,"first_next":if case["input"]["nodes"].as_array().unwrap().is_empty(){0}else{m.word(0x6800000c)?}});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}");}
  }
  Ok(())
 }
}
