//!Shared LPF-context cache186a7c/1869d8. This is a source-defined two-slot
//!resource, not a limit imposed on NPCs. Matching actors share a context.
use anyhow::Result;
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
pub struct Entry {pub key:[u32;2],pub references:u32}
#[derive(Default,Debug)]
pub struct Cache {pub entries:[Entry;2]}
///Required derived operations. The same cache is visible during callbacks;
///source selection after the extent query must see its current contents.
pub trait Queries {
 fn controller(&mut self,cache:&mut Cache,brain:usize)->Result<usize>;
 fn primary_key(&mut self,cache:&mut Cache,controller:usize)->Result<u32>;
 fn secondary_key(&mut self,cache:&mut Cache,controller:usize)->Result<u32>;
 fn extent(&mut self,cache:&mut Cache,brain:usize,primary:u32)->Result<f32>;

}
pub trait Services:Queries {
 fn refresh(&mut self,cache:&mut Cache,brain:usize)->Result<()>;
 fn context(&mut self,cache:&mut Cache,service:usize,name:&[u8])->Result<usize>;
 fn initialize_context(&mut self,cache:&mut Cache,context:usize,extent:f32,scalar60:f32)->Result<()>;
}
impl Cache {
 pub fn register(&mut self,brain:usize,service:usize,h:&mut impl Services)->Result<Option<usize>> {
  let controller=h.controller(self,brain)?;
  let key=[h.primary_key(self,controller)?,h.secondary_key(self,controller)?];
  //Both lookups occur before testing cached keys, even for a cache hit.
  let contexts=[h.context(self,service,b"LpfContext1")?,h.context(self,service,b"LpfContext2")?];
  for i in 0..2 {if self.entries[i].key==key {self.entries[i].references=self.entries[i].references.wrapping_add(1);return Ok((contexts[i]!=0).then_some(contexts[i]));}}
  //The native second pair of zero-count/key checks is redundant without
  //callbacks between them. Preserve its results, including wrapped counts.
  let extent=h.extent(self,brain,key[0])?;
  for i in 0..2 {if self.entries[i].references==0 {
   self.entries[i]=Entry{key,references:1};
   h.initialize_context(self,contexts[i],extent,1.8)?;h.refresh(self,brain)?;
   return Ok((contexts[i]!=0).then_some(contexts[i]));
  }}
  Ok(None)
 }
 pub fn release(&mut self,brain:usize,h:&mut impl Queries)->Result<()> {
  let controller=h.controller(self,brain)?;
  let key=[h.primary_key(self,controller)?,h.secondary_key(self,controller)?];
  for e in &mut self.entries {if e.key==key {e.references=e.references.wrapping_sub(1);break;}}
  Ok(())
 }
}
#[cfg(test)]
mod tests {
 use super::*;
 use serde_json::{json,Value};
 struct Host<'a>{row:&'a Value,trace:Vec<Value>}
 impl Queries for Host<'_>{
  fn controller(&mut self,_:&mut Cache,b:usize)->Result<usize>{assert_eq!(b,0x72000000);self.trace.push(json!(["controller",b]));Ok(0x74000000)}
  fn primary_key(&mut self,_:&mut Cache,c:usize)->Result<u32>{assert_eq!(c,0x74000000);self.trace.push(json!(["key",0]));Ok(self.row["input"]["key"][0].as_u64().unwrap() as u32)}
  fn secondary_key(&mut self,_:&mut Cache,c:usize)->Result<u32>{assert_eq!(c,0x74000000);self.trace.push(json!(["key",1]));Ok(self.row["input"]["key"][1].as_u64().unwrap() as u32)}
  fn extent(&mut self,c:&mut Cache,_:usize,p:u32)->Result<f32>{assert_eq!(p,self.row["input"]["key"][0].as_u64().unwrap() as u32);self.trace.push(json!(["extent"]));if let Some(a)=self.row["input"]["mutation"].as_array(){for i in 0..2{c.entries[i].references=a[i].as_u64().unwrap() as u32;}}Ok(2.5)}
 }
 impl Services for Host<'_>{
  fn refresh(&mut self,_:&mut Cache,_:usize)->Result<()>{self.trace.push(json!(["refresh"]));Ok(())}
  fn context(&mut self,_:&mut Cache,s:usize,n:&[u8])->Result<usize>{assert_eq!(s,0x73000000);let i=if n==b"LpfContext1"{0}else{assert_eq!(n,b"LpfContext2");1};self.trace.push(json!(["context",i]));Ok(0x75000000+i*0x1000)}
  fn initialize_context(&mut self,_:&mut Cache,p:usize,e:f32,z:f32)->Result<()>{self.trace.push(json!(["initialize",(p-0x75000000)/0x1000,e.to_bits(),z.to_bits()]));Ok(())}
 }
 #[test]
 fn original_shared_context_acquire_release_and_reentrant_extent_match()->Result<()> {
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_region_cache.json"))?;
  for row in fixture["cases"].as_array().unwrap(){let c=&row["input"];let mut cache=Cache::default();for i in 0..2{let a=&c["entries"][i];cache.entries[i]=Entry{key:[a[0].as_u64().unwrap() as u32,a[1].as_u64().unwrap() as u32],references:a[2].as_u64().unwrap() as u32};}
   let mut h=Host{row,trace:vec![]};
   let result=if c["op"]=="register"{json!(cache.register(0x72000000,0x73000000,&mut h)?.unwrap_or(0))}else{cache.release(0x72000000,&mut h)?;Value::Null};
   assert_eq!(result,row["result"],"{c}");assert_eq!(json!(cache.entries.map(|e|[e.key[0],e.key[1],e.references])),row["entries"],"{c}");assert_eq!(json!(h.trace),row["trace"],"{c}");
  }eprintln!("{} original LPF context cache cases",fixture["cases"].as_array().unwrap().len());Ok(())
 }
}
