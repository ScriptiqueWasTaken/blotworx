//! File-request status28e884 and generation-checked lookup28ec94.
//! Worker publication/queueing are separate; no request defaults to completed.
use anyhow::{Context as _,Result};
use std::sync::{Arc,Mutex};
#[derive(Clone,Copy,Debug,serde::Deserialize)]
pub struct Request {pub generation:u16,pub status:u32,pub result:u32}
/// Original table occupies [base+8,base+0xe108), with 0x30-byte slots.
const REQUEST_SLOTS:usize=0xe100/0x30;
#[derive(Clone)]
pub struct Requests {pub slots:Arc<Mutex<Vec<Request>>>}
impl Requests {
    pub fn query(&self,handle:u32,output:Option<&mut u32>)->Result<u32> {
        // Keep the lookup, status and optional result read in one critical section.
        let slots=self.slots.lock().map_err(|_|anyhow::anyhow!("File request table lock is poisoned"))?;
        let index=(handle&0xffff) as usize;
        if handle==u32::MAX || index>=REQUEST_SLOTS {return Ok(u32::MAX)}
        let request=slots.get(index).context("File request table storage is truncated")?;
        if request.generation as u32!=handle>>16 {return Ok(u32::MAX)}
        if let Some(output)=output {*output=if request.status==5 {request.result}else{0};}
        Ok(request.status)
    }
}
impl super::ai_graph_io::Completion for Requests {
    fn pending(&mut self,handle:u32)->Result<u32> {self.query(handle,None)}
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::Value;
    #[test] fn original_file_request_generation_status_and_result_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/file_request_status.json"))?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let mut slots=vec![Request{generation:0,status:99,result:88};REQUEST_SLOTS];
            slots[c["slot"].as_u64().unwrap() as usize]=serde_json::from_value(c["request"].clone())?;
            let table=Requests{slots:Arc::new(Mutex::new(slots))};let mut output=77;
            let result=table.query(c["handle"].as_u64().unwrap() as u32,if c["output"].as_bool().unwrap(){Some(&mut output)}else{None})?;
            assert_eq!(serde_json::json!({"result":result,"output":output}),case["expected"],"{c}");
        }
        Ok(())
    }
}
