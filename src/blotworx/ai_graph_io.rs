//! Game graph IO providers 1c2b80/1c2be4 and four-slot sector lookup185dc8.
//! Request completion remains an explicit file-system service (28e884).
use anyhow::{bail,Context as _,Result};
use std::collections::BTreeMap;
use super::{ai_graph_buffers::{Pointer,Bindings},ai_graph_handles::State,
    ai_graph_resource_group::Inputs,ai_graph_loading::{self,Loader,Direct,Packed}};
#[derive(Clone,Debug,serde::Deserialize)]
pub struct Sector {pub active:u8,pub id:u32,pub data:Pointer,pub request:u32}
#[derive(Debug,serde::Deserialize)]
pub struct Source {pub sectors:[Sector;4],pub packed:Pointer,pub size:u32}
/// The original queries request+14 with flag0; zero means finished.
pub trait Completion {fn pending(&mut self,request:u32)->Result<u32>;}
pub struct Provider<C> {pub sources:BTreeMap<usize,Source>,pub allocations:BTreeMap<usize,Vec<u8>>,pub completion:C}
impl<C:Completion> Provider<C> {
    pub fn direct_read(&mut self,source:usize,id:u32,output:&mut Pointer,token:&mut u32)->Result<u32> {
        *token=0;
        if source==0 {return Ok(u32::MAX)}
        let sector=self.sources.get(&source).context("Graph source is absent")?.sectors.iter()
            .find(|s|s.id==id && s.active==1).context("Original graph cache lookup returned a null sector")?.clone();
        if self.completion.pending(sector.request)?!=0 {return Ok(u32::MAX)}
        *output=sector.data;*token=1;Ok(1)
    }
    pub fn packed_read(&mut self,source:usize,offset:u32,bytes:u32,destination:Pointer,token:&mut u32)->Result<u32> {
        *token=0;
        if source==0 {return Ok(u32::MAX)}
        let s=self.sources.get(&source).context("Packed graph source is absent")?;
        if s.packed.is_null() || offset>=s.size {return Ok(u32::MAX)}
        let remaining=s.size-offset;
        let count=if (remaining as i32)<(bytes as i32) {remaining}else{bytes};
        if count==0 {return Ok(u32::MAX)}
        // Negative memcpy sizes are invalid memory operations, not recoverable IO misses.
        if (count as i32)<0 {bail!("Original packed graph read requests a negative memcpy size")}
        let from=s.packed.add(offset);let n=count as usize;
        let source_bytes=self.allocations.get(&from.allocation.context("Unmapped graph source pointer")?).context("Graph source allocation is absent")?;
        let start=from.offset as usize;let end=start.checked_add(n).context("Graph read range overflow")?;
        let copy=source_bytes.get(start..end).context("Packed graph source range is truncated")?.to_vec();
        let target=self.allocations.get_mut(&destination.allocation.context("Unmapped graph destination pointer")?).context("Graph destination allocation is absent")?;
        let start=destination.offset as usize;let end=start.checked_add(n).context("Graph write range overflow")?;
        target.get_mut(start..end).context("Packed graph destination range is truncated")?.copy_from_slice(&copy);
        *token=1;Ok(1)
    }
}
impl<C:Completion> ai_graph_loading::Io for Provider<C> {
    fn direct(&mut self,_:&mut Loader,_:&mut State,inputs:&mut Inputs,_:&mut Bindings,r:Direct,output:&mut Pointer)->Result<u32> {
        self.direct_read(r.source,r.id,output,&mut inputs.resources.get_mut(&r.resource).context("Direct IO token is absent")?.word24)
    }
    fn packed(&mut self,_:&mut Loader,_:&mut State,inputs:&mut Inputs,_:&mut Bindings,r:Packed)->Result<u32> {
        self.packed_read(r.source,r.offset,r.bytes,r.destination,&mut inputs.resources.get_mut(&r.resource).context("Packed IO token is absent")?.word24)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::Value;
    struct Pending {value:u32,trace:Vec<u32>}
    impl Completion for Pending {fn pending(&mut self,r:u32)->Result<u32>{self.trace.push(r);Ok(self.value)}}
    #[test] fn original_cache_and_packed_provider_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_io.json"))?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let mut p=Provider{sources:BTreeMap::from([(1,serde_json::from_value(c["source"].clone())?)]),allocations:BTreeMap::from([(100,(0..64).collect()),(200,vec![238;64])]),completion:Pending{value:c["pending"].as_u64().unwrap() as u32,trace:Vec::new()}};
            let mut output=Pointer{allocation:Some(200),offset:7};let mut token=99;
            let source=c["handle"].as_u64().unwrap() as usize;
            let result=if c["op"]=="direct" {p.direct_read(source,c["id"].as_u64().unwrap() as u32,&mut output,&mut token)?}
            else {p.packed_read(source,c["offset"].as_u64().unwrap() as u32,c["bytes"].as_u64().unwrap() as u32,Pointer{allocation:Some(200),offset:3},&mut token)?};
            assert_eq!(serde_json::json!({"result":result,"token":token,"output":output,"trace":p.completion.trace,"destination":p.allocations[&200]}),case["expected"],"{c}");
        }
        Ok(())
    }
    #[test] fn original_provider_through_real_request_table_matches()->Result<()> {
        use super::super::file_request_status::{Request,Requests};
        use std::sync::{Arc,Mutex};
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_request_completion.json"))?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let t=&c["request_table"];
            let mut slots=vec![Request{generation:0,status:99,result:88};1200];
            slots[t["slot"].as_u64().unwrap() as usize]=Request{generation:t["generation"].as_u64().unwrap() as u16,status:t["status"].as_u64().unwrap() as u32,result:0};
            let mut p=Provider{sources:BTreeMap::from([(1,serde_json::from_value(c["source"].clone())?)]),allocations:BTreeMap::from([(100,(0..64).collect()),(200,vec![238;64])]),completion:Requests{slots:Arc::new(Mutex::new(slots))}};
            let mut output=Pointer{allocation:Some(200),offset:7};let mut token=99;
            let result=p.direct_read(1,10,&mut output,&mut token)?;
            assert_eq!(serde_json::json!({"result":result,"token":token,"output":output,"trace":[p.sources[&1].sectors[0].request],"destination":p.allocations[&200]}),case["expected"],"{c}");
        }
        Ok(())
    }

}
