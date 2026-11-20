//! Retail base spatial service3c, REL1c5268, through real provider/sweep code.
//! Owner/service construction and traversal-interface dispatch remain required.
use super::{ai_graph_link_passes::Memory,ai_edge_crossing::Globals,ai_cell_crossing::Vectors,ai_spatial_walk::Filter};
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([word(m,p,0)?,word(m,p,4)?,word(m,p,8)?])}
fn put(m:&mut Memory,p:u32,v:[u32;3])->Result<()>{for (i,v) in v.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
///46566c: the point layout's attribute slot comes from live class metadata.
pub fn attribute(m:&Memory,point:u32,metadata:u32)->Result<u32>{
 let slot=word(m,metadata,8)?;word(m,point,0x10u32.wrapping_add(slot.wrapping_mul(4)))
}
///4962ac: optional actor-owned traversal filter list.
pub fn owner_filters(m:&Memory,owner:u32)->Result<u32>{
 let record=word(m,owner,0xf8)?;if record==0{return Ok(0)}
 let record=word(m,record,8)?;if record==0{return Ok(0)}
 word(m,record,8)
}
fn recycle(m:&mut Memory,pool:u32)->Result<()>{
 loop{let node=word(m,pool,8)?;if node==0{break}
  m.write_byte(node.wrapping_add(0x10),0)?;
  let next=word(m,node,12)?;m.write(node.wrapping_add(12),word(m,pool,0x10)?)?;
  m.write(pool.wrapping_add(0x10),node)?;m.write(pool.wrapping_add(8),next)?;
 }
 m.write(pool.wrapping_add(12),0)?;m.write(pool.wrapping_add(0x14),0)?;Ok(())
}
fn append_filters(m:&mut Memory,service:u32,list:u32)->Result<()>{
 let mut source=word(m,list,8)?;
 while source!=0{
  let pool=word(m,service,0x20)?;let node=word(m,pool,0x10)?;
  if node!=0{
   m.write(pool.wrapping_add(0x14),word(m,pool,0x14)?.wrapping_add(1))?;
   m.write(pool.wrapping_add(0x10),word(m,node,12)?)?;
   m.write(node.wrapping_add(4),word(m,source,4)?)?;m.write_byte(node.wrapping_add(0x10),1)?;
   let tail=word(m,pool,12)?;
   if tail==0{
    m.write(pool.wrapping_add(8),node)?;m.write(pool.wrapping_add(12),node)?;m.write(node.wrapping_add(12),0)?;
    let head=word(m,pool,8)?;m.write(head.wrapping_add(8),0)?;
    let tail=word(m,pool,12)?;m.write(tail.wrapping_add(8),0)?;
    let tail=word(m,pool,12)?;m.write(tail.wrapping_add(12),0)?;
   }else{
    m.write(node.wrapping_add(8),tail)?;m.write(node.wrapping_add(12),0)?;
    let tail=word(m,pool,12)?;m.write(tail.wrapping_add(12),node)?;m.write(pool.wrapping_add(12),node)?;
   }
  }
  source=word(m,source,12)?;
 }
 Ok(())
}
///1c4df0: select the original filter set. The mode1 union uses the native
/// pool's actual free nodes; it does not allocate or invent additional slots.
pub fn select_filters(m:&mut Memory,service:u32,output:u32,mode:u32)->Result<()>{
 select_filters_with(m,service,output,mode,|m,owner|owner_filters(m,owner))
}
/// Same native selector with canonical actor ownership supplied by the caller.
/// Each lookup remains fresh, including the second lookup after pool recycling.
pub fn select_filters_with(m:&mut Memory,service:u32,output:u32,mode:u32,mut owner_filters:impl FnMut(&mut Memory,u32)->Result<u32>)->Result<()>{
 let field=match mode{0=>Some(0x14),2=>Some(0x18),4=>Some(0x1c),5=>Some(0x10),6=>Some(0xc),_=>None};
 if let Some(field)=field{return m.write(output,word(m,service,field)?)}
 if mode==3{
  let owner=word(m,service,4)?;
  let value=if owner!=0 && owner_filters(m,owner)?!=0{owner_filters(m,word(m,service,4)?)?}else{0};
  return m.write(output,value)
 }
 if mode==1{
  let owner=word(m,service,4)?;
  if owner==0 || owner_filters(m,owner)?==0{return m.write(output,word(m,service,0x10)?)}
  recycle(m,word(m,service,0x20)?)?;
  append_filters(m,service,word(m,service,0x10)?)?;
  let owner=word(m,service,4)?;let list=owner_filters(m,owner)?;
  append_filters(m,service,list)?;
  return m.write(output,word(m,service,0x20)?)
 }
 recycle(m,word(m,service,0x20)?)?;m.write(output,word(m,service,0x20)?)
}

#[derive(Clone,Copy)]
pub struct Context{pub globals:Globals,pub vectors:Vectors,pub point_cache_metadata:u32,pub scratch:u32}
/// Scratch is a distinct mapped0x700-byte region for this frame and nested
/// provider calls. Missing providers return before touching caller outputs.
pub fn sweep3c<H:super::ai_navigation_sweep::InterfaceHost+super::ai_cell_trace::InterfaceHost>(m:&mut Memory,c:Context,service:u32,start_point:u32,end:u32,radius:u32,filter:&mut Option<Filter>,info:u32,output:Option<u32>,host:&mut H)->Result<u32>{
 let owner=word(m,service,4)?;let preferred=word(m,owner,0x54)?;
 let record=if preferred!=0{preferred}else{word(m,owner,0x50)?};
 if record==0{return Ok(0)}
 let provider=word(m,record,0x4c)?;if provider==0{return Ok(0)}
 let cache=attribute(m,start_point,c.point_cache_metadata)?;
 let input_cache=if cache==0{None}else{Some(cache.wrapping_add(4))};
 let returned=c.scratch.wrapping_add(0x1c);let normal=c.scratch.wrapping_add(0x10);let output_cache=c.scratch.wrapping_add(0x28);
 put(m,returned,[0;3])?;put(m,normal,[0;3])?;
 for (i,value) in [u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,0].into_iter().enumerate(){m.write(output_cache.wrapping_add(i as u32*4),value)?;}
 let result=super::ai_navigation_sweep::provider_sweep(m,c.globals,c.vectors,c.scratch.wrapping_add(0x100),provider,start_point.wrapping_add(4),end,radius,returned,normal,input_cache,Some(output_cache),filter,Some(info),7,host)?;
 if let Some(output)=output{put(m,output,point(m,returned)?)?;}
 if result&255!=0{Ok(1)}else{Ok(u32::from(m.word(info)?!=0))}
}

#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};
 struct Interfaces<'a>{input:&'a Value,trace:Vec<Value>}
 impl super::super::ai_cell_trace::InterfaceHost for Interfaces<'_>{
  fn accept1c(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Service floor lookup unexpectedly invoked filter acceptance")}
 }
 impl super::super::ai_navigation_sweep::InterfaceHost for Interfaces<'_>{
  fn sweep20(&mut self,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,info:u32)->Result<u32>{
   self.trace.push(json!([interface,cell,point(m,start)?,point(m,end)?,radius]));
   put(m,output,[(self.input["clip"].as_f64().unwrap() as f32).to_bits(),1f32.to_bits(),2f32.to_bits()])?;
   put(m,normal,[1f32.to_bits(),0,0])?;m.write(info,111)?;m.write(info.wrapping_add(4),222)?;Ok(0)
  }
 }
 #[test]
 fn retail_spatial_service_reaches_real_sweeps_with_native_provider_priority_and_results()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_service.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x62000000,vec![0;0x1800]),(0x63000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20]),(0x564d0,vec![0;0x40]),(0x64000000,vec![0;0x40]),(0x65000000,vec![0;0x700]),(0x67000000,vec![0;16]),(0x68000000,vec![0;64]),(0x69000000,vec![0;64]),(0x6a000000,vec![0;64]),(0x6b000000,vec![0;0x500]),(0x6c000000,vec![0;64]),(0x6d000000,vec![0;0x120]),(0x6e000000,vec![0;0x400]),(0x41d64,vec![0;16])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let input=&case["input"];let mut host=Interfaces{input,trace:vec![]};
   let mut filter=(input["filter"]!="none").then_some(Filter::Address(0x64000000));
   let c=Context{globals:Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0},vectors:Vectors{up:0x564f4,zero:0x564d0},point_cache_metadata:0x41d64,scratch:0x65000000};
   let result=sweep3c(&mut m,c,0x6e000000,0x6d000000,0x63000010,0.4f32.to_bits(),&mut filter,0x64000010,input["output"].as_bool().unwrap().then_some(0x63000030),&mut host)?;
   let actual=json!({"result":result,"point":point(&m,0x63000030)?,"info":[m.word(0x64000010)?,m.word(0x64000014)?],"cache":(0..7).map(|i|m.word(0x6d000104+i*4)).collect::<Result<Vec<_>>>()?,"trace":host.trace,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }
 #[test]
 fn retail_spatial_filter_modes_preserve_native_pool_links_and_capacity()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_filters.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x60000000,vec![0;0x40]),(0x61000000,vec![0;0x100]),(0x62000000,vec![0;0x20]),(0x62100000,vec![0;0x200]),(0x63000000,vec![0;16]),(0x63100000,vec![0;0x100]),(0x64000000,vec![0;16]),(0x64100000,vec![0;0x100]),(0x65000000,vec![0;64]),(0x66000000,vec![0;4])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let input=&case["input"];select_filters(&mut m,0x60000000,0x66000000,input["mode"].as_u64().unwrap() as u32)?;
   let count=input["active"].as_u64().unwrap()+input["free"].as_u64().unwrap();
   let actual=json!({"output":m.word(0x66000000)?,"pool":(0..6).map(|i|m.word(0x62000000+i*4)).collect::<Result<Vec<_>>>()?,"nodes":(0..count as u32).map(|i|(0..5).map(|j|m.word(0x62100000+i*32+j*4)).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }

}
