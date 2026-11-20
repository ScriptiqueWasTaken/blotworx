//! PCacb010/acb290 and9a9300 publication gates. Query scheduling, spatial
//! source selection and ownership belong to the asynchronous scene manager.
use anyhow::{ensure,Context,Result};
use std::sync::{Arc,Mutex};
use super::d3d9_sh::Snapshot;
#[derive(Debug,Clone,Default)]
pub(super) struct Query {
    pub status:u32,pub requested_frame:u32,
    pub coefficients:Option<Arc<[[f32;3];9]>>,
}
/// Stable native probe identity. All clients sharing a probe observe its
/// current request stamp and task completion, without per-frame handle copies.
#[derive(Debug,Clone,Default)]
pub(super) struct Handle(Arc<Mutex<Query>>);
impl Handle {
    pub fn new(query:Query)->Self {Self(Arc::new(Mutex::new(query)))}
    pub fn read(&self)->Result<Query>{Ok(self.0.lock().map_err(|_|anyhow::anyhow!("PC SH query state poisoned"))?.clone())}
    pub fn replace(&self,query:Query)->Result<()>{*self.0.lock().map_err(|_|anyhow::anyhow!("PC SH query state poisoned"))?=query;Ok(())}
    pub fn same(&self,other:&Self)->bool{Arc::ptr_eq(&self.0,&other.0)}
}
#[derive(Debug,Clone,Copy)]
pub(super) struct Request {pub position:[f32;3],pub mode:u32}
impl Request {pub fn words(self)->[u32;4] {[self.position[0].to_bits(),self.position[1].to_bits(),self.position[2].to_bits(),self.mode]}}
pub(super) trait Manager {
    /// Native virtual slot9. Output handle/token writes survive a false return.
    fn request(&mut self,request:Request,query:&mut Option<Handle>,token:&mut u32)->Result<bool>;
}
#[derive(Debug,Clone)]
pub(super) struct Client {pub position:[f32;3],pub query:Option<Handle>,pub token:u32}
#[derive(Debug,Clone,Copy)]
/// `capability` is the opaque renderer+154 virtual-slot10 signed result;
/// its high-level meaning is not inferred as a shader-model version.
pub(super) struct Publication {pub resource_kind:u32,pub capability:Option<i32>}
impl Publication {
    pub fn admits(self)->bool {
        match self.resource_kind {0x8000=>true,0x8040=>self.capability.is_none_or(|v|v>9),_=>false}
    }
}
/// PCacb290 uses wrapping 32-bit subtraction, not elapsed simulation time.
pub(super) fn frame_age(current:u32,updated:u32)->u32 {current.wrapping_sub(updated)}
impl Client {
    pub fn request(&mut self,manager:&mut dyn Manager,urgent:bool)->Result<bool> {
        let accepted=manager.request(Request {position:self.position,mode:if urgent{4}else{2}},&mut self.query,&mut self.token)?;
        if !accepted {return Ok(false);}
        Ok(self.query.as_ref().context("Native SH manager accepted a request without a query")?.read()?.status==2)
    }
    /// Returns whether coefficients were copied, unlike the native wrapper's
    /// general resource-handled return. The position service is lazy, preserving
    /// native gate/callback ordering. Failed/pending queries keep the last
    /// published mesh coefficients; shared authored material data is untouched.
    pub fn publish(&mut self,policy:Publication,frame:u32,manager:Option<&mut dyn Manager>,
        position:impl FnOnce([f32;3])->Result<[f32;3]>,snapshot:&mut Snapshot)->Result<bool> {
        if !policy.admits() {return Ok(false);}
        let Some(query)=&self.query else{return Ok(false)};
        if frame_age(frame,query.read()?.requested_frame)!=0 {return Ok(false);}
        let Some(manager)=manager else{return Ok(false)};
        self.position=position(self.position)?;
        if !self.request(manager,true)? {return Ok(false);}
        let coefficients=self.query.as_ref().unwrap().read()?.coefficients.context("Ready native SH query has no coefficient data")?;
        ensure!(coefficients.iter().flatten().all(|v|v.is_finite()),"Nonfinite native SH query coefficients");
        snapshot.coefficients=*coefficients;Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Receiver {accepted:bool,query:Handle,calls:Vec<[u32;4]>}
    impl Manager for Receiver {
        fn request(&mut self,request:Request,query:&mut Option<Handle>,token:&mut u32)->Result<bool> {
            self.calls.push(request.words());*query=Some(self.query.clone());*token=77;Ok(self.accepted)
        }
    }
    fn snapshot()->Snapshot {Snapshot {coefficients:[[0.;3];9],specular_color:Some([0.1;3]),specular_direction:Some([0.,1.,0.])}}
    #[test]
    fn original_native_request_age_and_publication_gates()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_query_publication.json"))?;
        let words=reference["input_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
        let coefficients=Arc::new(std::array::from_fn(|i|std::array::from_fn(|j|f32::from_bits(words[i*3+j]))));
        for row in reference["ages"].as_array().unwrap() {assert_eq!(frame_age(row["frame"].as_u64().unwrap() as u32,row["stamp"].as_u64().unwrap() as u32),row["age"].as_u64().unwrap() as u32);}
        for row in reference["requests"].as_array().unwrap() {
            let query=Handle::new(Query {status:row["status"].as_u64().unwrap() as u32,requested_frame:100,coefficients:Some(coefficients.clone())});
            let mut receiver=Receiver {accepted:row["accepted"].as_bool().unwrap(),query,calls:vec![]};
            let mut client=Client {position:[1.,-0.,3.],query:None,token:41};
            assert_eq!(client.request(&mut receiver,row["urgent"].as_bool().unwrap())?,row["ready"].as_bool().unwrap(),"{row}");
            assert_eq!(serde_json::json!(receiver.calls),row["requests"]);assert_eq!(client.token,row["token"].as_u64().unwrap() as u32);assert!(client.query.is_some());
        }
        for row in reference["cases"].as_array().unwrap() {
            let query=Handle::new(Query {status:row["status"].as_u64().unwrap() as u32,requested_frame:row["stamp"].as_u64().unwrap_or(0) as u32,coefficients:Some(coefficients.clone())});
            let mut receiver=Receiver {accepted:row["accepted"].as_bool().unwrap(),query:query.clone(),calls:vec![]};
            let mut client=Client {position:[11.,12.,13.],query:row["stamp"].as_u64().map(|_|query),token:41};
            let policy=Publication {resource_kind:row["resource_kind"].as_u64().unwrap() as u32,capability:row["capability"].as_i64().map(|v|v as i32)};
            let mut snapshot=snapshot();let manager=if row["manager"].as_bool().unwrap(){Some(&mut receiver as &mut dyn Manager)}else{None};
            let published=client.publish(policy,row["frame"].as_u64().unwrap() as u32,manager,|_|Ok([1.,-0.,3.]),&mut snapshot)?;
            assert_eq!(published,!row["published"].as_array().unwrap().is_empty(),"{row}");
            assert_eq!(serde_json::json!(receiver.calls),row["requests"],"{row}");assert_eq!(client.token,row["token"].as_u64().unwrap() as u32);
            assert_eq!(snapshot.coefficients.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),row["output_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
            assert_eq!(snapshot.specular_color,Some([0.1;3]));assert_eq!(snapshot.specular_direction,Some([0.,1.,0.]));
        }Ok(())
    }
    #[test]
    fn malformed_ready_result_preserves_published_data()->Result<()> {
        let query=Handle::new(Query {status:2,requested_frame:7,coefficients:None});
        let mut receiver=Receiver {accepted:true,query:query.clone(),calls:vec![]};let mut client=Client {position:[0.;3],query:Some(query),token:0};let mut snapshot=snapshot();
        assert!(client.publish(Publication {resource_kind:0x8000,capability:None},7,Some(&mut receiver),Ok,&mut snapshot).is_err());assert_eq!(snapshot.coefficients,[[0.;3];9]);Ok(())
    }
}
