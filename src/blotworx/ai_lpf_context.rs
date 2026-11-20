//!Canonical LPF context records445860, lookup450074 and reset449a64.
//!All pointers address owned data in Memory; no original code is executed.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::ai_graph_link_passes::Memory;
fn r(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn w(m:&mut Memory,p:u32,o:u32,v:u32)->Result<()>{m.write(p.wrapping_add(o),v)}
pub trait Host {
 fn child_reset(&mut self,m:&mut Memory,context:u32,child:u32)->Result<()>;
 fn group_clear(&mut self,m:&mut Memory,group:u32)->Result<()>;
}
pub fn construct(m:&mut Memory,p:u32)->Result<()> {
 w(m,p,0,0x51a768)?;
 for o in (4..=0x54).step_by(4).chain([0x90,0x94,0x98,0x9c,0xa0,0xa4,0xbc,0xc0,0xc4,0xc8,0xcc,0xd0,0xd4,0xd8]){w(m,p,o,0)?;}
 for o in [0xa8,0xac,0xb0,0xb4,0xb8]{w(m,p,o,u32::MAX)?;}
 for i in 0..3{let v=r(m,0x564d0,i*4)?;w(m,p,0xdc+i*4,v)?;}Ok(())
}
pub fn lookup(m:&Memory,manager:u32,name:&[u8])->Result<Option<u32>> {
 let mut i=0u32;
 while i<r(m,manager,0x10)? {
  let p=r(m,manager,4)?.wrapping_add(i.wrapping_mul(0x138));let text=r(m,p,0x90)?;
  let mut bytes=Vec::new();let mut at=text;
  loop {let b=m.byte(at)?;if b==0{break}bytes.push(b);at=at.checked_add(1).ok_or_else(||anyhow::anyhow!("Context name address overflow"))?;}
  if super::ai_traversal_registry::names_equal(&bytes,name){return Ok(Some(p))}i=i.wrapping_add(1);
 }Ok(None)
}
//Source pools have two different free-list layouts. Preserve their links and
//node flags, rather than dropping live allocations or recreating empty pools.
fn recycle(m:&mut Memory,p:u32,inline:Option<(u32,u32)>)->Result<()> {
 let mut visited=BTreeSet::new();
 loop {let node=r(m,p,8)?;if node==0{break}ensure!(visited.insert(node),"Cyclic LPF context pool");
  if let Some((next,flag))=inline {m.write_byte(node.wrapping_add(flag),0)?;let old=r(m,node,next)?;let free=r(m,p,0x10)?;w(m,node,next,free)?;w(m,p,0x10,node)?;w(m,p,8,old)?;}
  else {let old=r(m,node,0xc)?;let sentinel=r(m,p,4)?;let free=r(m,sentinel,8)?;w(m,node,0xc,free)?;w(m,sentinel,8,node)?;w(m,p,8,old)?;}
 }
 w(m,p,0xc,0)?;w(m,p,if inline.is_some(){0x14}else{0x10},0)
}
//Append to the original flag10/nextc inline pool. Native exhaustion does
//not grow the pool, but the caller's accounting still takes place.
fn append(m:&mut Memory,p:u32,value:u32)->Result<()> {
 let node=r(m,p,0x10)?;if node==0{return Ok(())}
 let count=r(m,p,0x14)?;w(m,p,0x14,count.wrapping_add(1))?;
 let next=r(m,node,0xc)?;w(m,p,0x10,next)?;w(m,node,4,value)?;m.write_byte(node.wrapping_add(0x10),1)?;
 let tail=r(m,p,0xc)?;
 if tail==0 {w(m,p,8,node)?;w(m,p,0xc,node)?;w(m,node,0xc,0)?;w(m,node,8,0)?;}
 else {w(m,node,8,tail)?;w(m,node,0xc,0)?;w(m,tail,0xc,node)?;w(m,p,0xc,node)?;}
 Ok(())
}
///44db60, including source virtualc before reading the destination pool.
fn remove_group(m:&mut Memory,p:u32,g:u32,h:&mut impl Host)->Result<()> {
 h.group_clear(m,g)?;append(m,r(m,p,0x4c)?,g)?;
 let count=r(m,p,0x108)?;if count!=0{w(m,p,0x108,count.wrapping_sub(1))?;}Ok(())
}
///44ea40: edge next links are at30, not the ordinary list offsetc.
fn clear_edges(m:&mut Memory,p:u32,links:u32)->Result<()> {
 let used=r(m,links,0x10)?;let count=r(m,p,0xf4)?;w(m,p,0xf4,count.saturating_sub(used))?;
 let mut seen=BTreeSet::new();loop {let node=r(m,links,8)?;if node==0{break}ensure!(seen.insert(node),"Cyclic LPF edge list");
  let next=r(m,node,0x30)?;let sentinel=r(m,links,4)?;let free=r(m,sentinel,8)?;w(m,node,0x30,free)?;w(m,sentinel,8,node)?;w(m,links,8,next)?;
 }w(m,links,0xc,0)?;w(m,links,0x10,0)
}
///44e5bc clears one link record, then queues it even when its edge list was empty.
fn release_one(m:&mut Memory,p:u32,links:u32)->Result<()> {
 clear_edges(m,p,links)?;append(m,r(m,p,0x2c)?,links)?;
 let count=r(m,p,0xf8)?;if count!=0{w(m,p,0xf8,count.wrapping_sub(1))?;}Ok(())
}
///44e6f0 preserves node storage and the independently owned pending-link queue.
fn release_links(m:&mut Memory,p:u32,list:u32)->Result<()> {
 let count=r(m,p,0xf8)?;let used=r(m,list,0x10)?;w(m,p,0xf8,count.saturating_sub(used))?;
 let mut node=r(m,list,8)?;let mut seen=BTreeSet::new();
 while node!=0 {ensure!(seen.insert(node),"Cyclic LPF link list");let links=r(m,node,4)?;clear_edges(m,p,links)?;node=r(m,node,0xc)?;}
 let count=r(m,p,0xfc)?;let used=r(m,list,0x10)?;if count>=used{w(m,p,0xfc,count.wrapping_sub(used))?;}
 let queue=r(m,p,0x2c)?;seen.clear();
 loop {let node=r(m,list,8)?;if node==0{break}ensure!(seen.insert(node),"Cyclic LPF link recycling");let value=r(m,node,4)?;append(m,queue,value)?;
  let next=r(m,node,0xc)?;let sentinel=r(m,list,4)?;let free=r(m,sentinel,8)?;w(m,node,0xc,free)?;w(m,sentinel,8,node)?;w(m,list,8,next)?;
 }w(m,list,0xc,0)?;w(m,list,0x10,0)
}
///44d48c maintains the original sorted active/free table. The native caller
///requires membership; preserve its fallback index instead of substituting
///a different binary-search policy or shortening the allocated table.
fn remove_child(m:&mut Memory,p:u32,child:u32)->Result<()> {
 let count=r(m,r(m,p,0xc)?,0xc)?;ensure!(count!=0,"Cannot remove a child from an empty LPF table");
 let wanted=r(m,child,4)?;let mut low=0u32;let mut high=count.wrapping_sub(1);let mut mid;
 loop {mid=low.wrapping_add(high)>>1;if low==mid{break}
  let item=r(m,r(m,p,0x10)?,mid.wrapping_mul(4))?;let key=r(m,item,4)?;
  if key==wanted{low=mid;break}if key>wanted{high=mid}else{low=mid}
 }
 let table=r(m,p,0x10)?;
 if r(m,r(m,table,low.wrapping_mul(4))?,4)?==wanted{mid=low;}
 else if r(m,r(m,table,high.wrapping_mul(4))?,4)?==wanted{mid=high;}
 if mid<count.wrapping_sub(1) {
  for i in mid..count.wrapping_sub(1){let table=r(m,p,0x10)?;let value=r(m,table,i.wrapping_add(1).wrapping_mul(4))?;w(m,table,i.wrapping_mul(4),value)?;}
  let count=r(m,r(m,p,0xc)?,0xc)?;let table=r(m,p,0x10)?;w(m,table,count.wrapping_sub(1).wrapping_mul(4),child)?;
 }
 let last=r(m,r(m,p,0xc)?,0xc)?.wrapping_sub(1);let table=r(m,p,0x10)?;let item=r(m,table,last.wrapping_mul(4))?;w(m,item,4,u32::MAX)?;
 for o in [8,0xc,0x10]{let last=r(m,r(m,p,0xc)?,0xc)?.wrapping_sub(1);let item=r(m,r(m,p,0x10)?,last.wrapping_mul(4))?;w(m,item,o,0)?;}
 let last=r(m,r(m,p,0xc)?,0xc)?.wrapping_sub(1);let item=r(m,r(m,p,0x10)?,last.wrapping_mul(4))?;let links=r(m,item,0x14)?;if links!=0{release_one(m,p,links)?;}
 for o in [0x14,0x18]{let last=r(m,r(m,p,0xc)?,0xc)?.wrapping_sub(1);let item=r(m,r(m,p,0x10)?,last.wrapping_mul(4))?;w(m,item,o,0)?;}
 let count=r(m,p,0xe8)?;if count!=0{w(m,p,0xe8,count.wrapping_sub(1))?;}Ok(())
}
///440ee8, including actual edge/link recycling before clearing the item bounds.
fn clear_item(m:&mut Memory,item:u32)->Result<()> {
 let links=r(m,item,8)?;if links!=0{let context=r(m,item,4)?;release_links(m,context,links)?;}
 let pool=r(m,item,0xc)?;if pool!=0{let context=r(m,item,4)?;let used=r(m,pool,0x10)?;let count=r(m,context,0xec)?;w(m,context,0xec,count.wrapping_sub(used))?;let pool=r(m,item,0xc)?;recycle(m,pool,None)?;}
 w(m,item,0x10,0)?;w(m,item,0x14,0)?;
 for o in [0x18,0x1c,0x20,0x30]{w(m,item,o,0x7f7fffff)?;}
 for o in [0x24,0x28,0x2c]{w(m,item,o,0xff7fffff)?;}Ok(())
}
///44df6c zero14 branch. The context reset sets14zero before this call;
///other (non-reset) entry paths are separate, not silently approximated here.
fn reset_item(m:&mut Memory,p:u32,item:u32,h:&mut impl Host)->Result<()> {
 ensure!(r(m,item,0x14)?==0,"LPF reset item still has its parent group");
 let mut node=r(m,r(m,item,0xc)?,8)?;let mut visited=BTreeSet::new();
 while node!=0 {
  ensure!(visited.insert(node),"Cyclic LPF item children");let child=r(m,node,4)?;w(m,child,0x18,0)?;
  let mut queued=r(m,r(m,p,0xbc)?,8)?;let mut seen=BTreeSet::new();let mut found=false;
  while queued!=0 {ensure!(seen.insert(queued),"Cyclic LPF pending children");if r(m,queued,4)?==child{found=true;break}queued=r(m,queued,0xc)?;}
  if !found{remove_child(m,p,child)?;}node=r(m,node,0xc)?;
 }
 let used=r(m,r(m,item,0xc)?,0x10)?;let count=r(m,p,0xec)?;w(m,p,0xec,count.wrapping_sub(used))?;
 recycle(m,r(m,item,0xc)?,None)?;clear_item(m,item)?;append(m,r(m,p,0x38)?,item)?;
 let count=r(m,p,0x100)?;if count!=0{w(m,p,0x100,count.wrapping_sub(1))?;}Ok(())
}
pub fn reset(m:&mut Memory,p:u32,h:&mut impl Host)->Result<()> {
 let mut i=0u32;while i<r(m,p,0xa0)?{let child=r(m,r(m,p,0x94)?,i.wrapping_mul(4))?;h.child_reset(m,p,child)?;i=i.wrapping_add(1);}
 recycle(m,r(m,p,0x50)?,Some((0x18,0x1c)))?;recycle(m,r(m,p,0x54)?,Some((0x18,0x1c)))?;
 i=0;loop {let table=r(m,p,0x44)?;if i>=r(m,table,0xc)?{w(m,table,0xc,0)?;break}
  let group=r(m,r(m,table,4)?,i.wrapping_mul(4))?;let mut node=r(m,r(m,group,0xc)?,8)?;let mut visited=BTreeSet::new();
  while node!=0 {ensure!(visited.insert(node),"Cyclic LPF context group");let item=r(m,node,4)?;w(m,item,0x14,0)?;reset_item(m,p,item,h)?;node=r(m,node,0xc)?;}
  remove_group(m,p,group,h)?;i=i.wrapping_add(1);
 }
 i=0;loop {let table=r(m,p,0x18)?;if i>=r(m,table,0xc)?{break}
  let array=r(m,p,0x1c)?;let group=r(m,array,i.wrapping_mul(4))?;let pool=r(m,group,0xc)?;let count=r(m,p,0x104)?;let used=r(m,pool,0x10)?;if count>=used{w(m,p,0x104,count.wrapping_sub(used))?;}
  let minimum=u32::MAX;let group=r(m,r(m,p,0x1c)?,i.wrapping_mul(4))?;w(m,group,4,minimum)?;
  let maximum=u32::MAX;let group=r(m,r(m,p,0x1c)?,i.wrapping_mul(4))?;w(m,group,8,maximum)?;recycle(m,r(m,group,0xc)?,None)?;i=i.wrapping_add(1);
 }
 w(m,p,0xd0,0)?;for i in 0..3{let v=r(m,0x564d0,i*4)?;w(m,p,0xdc+i*4,v)?;}
 let pool=r(m,p,0xbc)?;let used=r(m,pool,0x10)?;let count=r(m,p,0xec)?;w(m,p,0xec,count.wrapping_sub(used))?;recycle(m,pool,None)?;
 let pool=r(m,p,0xc0)?;let used=r(m,pool,0x10)?;let count=r(m,p,0x104)?;if count>=used{w(m,p,0x104,count.wrapping_sub(used))?;}recycle(m,pool,None)?;
 recycle(m,r(m,p,0xc4)?,Some((0xc,0x10)))?;
 for o in [0xc8,0xe8,0xec,0xf0,0xf4,0xf8,0xfc,0x100,0x104,0x108]{w(m,p,o,0)?;}Ok(())
}
///186be4..186c08 writes parameters before the context virtual reset.
pub fn initialize(m:&mut Memory,p:u32,extent:f32,scalar:f32,h:&mut impl Host)->Result<()>{w(m,p,0x60,scalar.to_bits())?;w(m,p,0x5c,extent.to_bits())?;reset(m,p,h)}
#[cfg(test)]
mod tests {
 use super::*;use serde_json::{json,Value};
 struct Callbacks{trace:Vec<Value>,shrink:bool}
 impl Host for Callbacks {
  fn child_reset(&mut self,m:&mut Memory,p:u32,c:u32)->Result<()>{self.trace.push(json!(["child",c]));if self.shrink{w(m,p,0xa0,0)?;}Ok(())}
  fn group_clear(&mut self,_:&mut Memory,g:u32)->Result<()>{self.trace.push(json!(["group",g]));Ok(())}
 }
 fn bytes(s:&str)->Vec<u8>{s.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect()}
 #[test]
 fn original_context_construction_lookup_and_populated_reset_match()->Result<()> {
  let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_lpf_context.json"))?;
  for row in capture["cases"].as_array().unwrap(){let c=&row["input"];let mut m=Memory::default();for a in row["initial"].as_array().unwrap(){m.regions.insert(a[0].as_u64().unwrap() as u32,bytes(a[1].as_str().unwrap()));}
   let mut h=Callbacks{trace:vec![],shrink:c["shrink"].as_bool().unwrap_or(false)};
   let result=match c["op"].as_str().unwrap(){"construct"=>{construct(&mut m,0x72000000)?;Value::Null},"reset"=>{reset(&mut m,0x72000000,&mut h)?;Value::Null},"links"=>{release_links(&mut m,0x72000000,row["argument"].as_u64().unwrap() as u32)?;Value::Null},"one_link"=>{release_one(&mut m,0x72000000,row["argument"].as_u64().unwrap() as u32)?;Value::Null},"child"=>{remove_child(&mut m,0x72000000,row["argument"].as_u64().unwrap() as u32)?;Value::Null},"lookup"=>{let n=c["needle"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u8).collect::<Vec<_>>();json!(lookup(&m,0x72000000,&n)?.unwrap_or(0))},_=>unreachable!()};
   assert_eq!(result,row["result"],"{c}");assert_eq!(json!(h.trace),row["trace"],"{c}");
   for a in row["final"].as_array().unwrap(){let p=a[0].as_u64().unwrap() as u32;assert_eq!(m.regions[&p],bytes(a[1].as_str().unwrap()),"{c} at {p:x}");}
  }eprintln!("{} original LPF context cases",capture["cases"].as_array().unwrap().len());Ok(())
 }
}
///Canonical context operations for the brain cache. Getter/extent/world
///refresh services are delegated; named lookup and context reset cannot be
///replaced with successful setup answers by the lower backend.
pub trait Refresh{fn refresh_context(&mut self,m:&mut Memory,c:&mut super::ai_navigation_region_cache::Cache,b:usize)->Result<()>;}
pub struct Native<'a,H>{pub memory:&'a mut Memory,pub backend:&'a mut H}
impl<H:super::ai_navigation_region_cache::Queries> super::ai_navigation_region_cache::Queries for Native<'_,H>{
 fn controller(&mut self,c:&mut super::ai_navigation_region_cache::Cache,b:usize)->Result<usize>{self.backend.controller(c,b)}
 fn primary_key(&mut self,c:&mut super::ai_navigation_region_cache::Cache,b:usize)->Result<u32>{self.backend.primary_key(c,b)}
 fn secondary_key(&mut self,c:&mut super::ai_navigation_region_cache::Cache,b:usize)->Result<u32>{self.backend.secondary_key(c,b)}
 fn extent(&mut self,c:&mut super::ai_navigation_region_cache::Cache,b:usize,p:u32)->Result<f32>{self.backend.extent(c,b,p)}
}
impl<H:super::ai_navigation_region_cache::Queries+Host+Refresh> super::ai_navigation_region_cache::Services for Native<'_,H>{
 fn refresh(&mut self,c:&mut super::ai_navigation_region_cache::Cache,b:usize)->Result<()>{self.backend.refresh_context(self.memory,c,b)}
 fn context(&mut self,_:&mut super::ai_navigation_region_cache::Cache,s:usize,n:&[u8])->Result<usize>{Ok(lookup(self.memory,u32::try_from(s)?,n)?.unwrap_or(0) as usize)}
 fn initialize_context(&mut self,_:&mut super::ai_navigation_region_cache::Cache,p:usize,e:f32,s:f32)->Result<()>{initialize(self.memory,u32::try_from(p)?,e,s,self.backend)}
}
