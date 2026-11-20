//! Base navigation World virtual18 (46b37c, vtable51d570). Actor pool and
//! callback records share canonical storage; derived actor24 remains required.
use super::{ai_graph_link_passes::Memory,ai_spatial_construction,ai_traversal_policy_release as release,ai_traversal_registration as registration};
use anyhow::Result;
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
pub trait Host {
    fn actor_slot(&mut self,m:&Memory,actor:u32)->Result<u32>{w(m,actor,0x14)}
    fn bind_actor(&mut self,m:&mut Memory,actor:u32,slot:u32,world:u32)->Result<()>{
        m.write(actor.wrapping_add(0x14),slot)?;m.write(actor.wrapping_add(0x28),world)
    }
    fn callback(&mut self,m:&mut Memory,world:u32,actor:u32,function:u32,data:u32)->Result<()>;
    fn inserted24(&mut self,m:&mut Memory,actor:u32)->Result<()>;
}
pub fn register(m:&mut Memory,world:u32,actor:u32,h:&mut impl Host)->Result<u32>{
    let pool=w(m,world,0x18)?;
    if w(m,pool,0x14)?>=w(m,world,0x14)? {return Ok(0)}
    let slot=w(m,pool,0x10)?;
    ai_spatial_construction::append(m,pool,actor)?;
    // A missing free node does not reject this native branch: it publishes
    // null14, binds the World, and still delivers callbacks/actor virtual24.
    h.bind_actor(m,actor,slot,world)?;
    let mut i=0u32;
    while (i as i32)<(w(m,world,0x50)? as i32) {
        let data=w(m,w(m,world,0x5c)?,i.wrapping_mul(4))?;
        let function=w(m,w(m,world,0x58)?,i.wrapping_mul(4))?;
        h.callback(m,world,actor,function,data)?;
        i=i.wrapping_add(1);
    }
    h.inserted24(m,actor)?;Ok(1)
}
pub trait RemovalHost:Host {
    fn first_dependency(&mut self,m:&Memory,actor:u32)->Result<Option<u32>>{
        let node=w(m,w(m,actor,0x2c)?,8)?;
        Ok(if node==0{None}else{Some(w(m,node,4)?)})
    }
    fn detach14(&mut self,m:&mut Memory,child:u32,actor:u32)->Result<()>;
    fn removed20(&mut self,m:&mut Memory,actor:u32)->Result<()>;
}
///Base World virtual1c46b520. Callback/child/derived operations precede
///the fresh pool/node reads; no forced dependency-list clearing or readiness.
pub fn unregister(m:&mut Memory,world:u32,actor:u32,h:&mut impl RemovalHost)->Result<u32>{
    if h.actor_slot(m,actor)?==0{return Ok(0)}
    let mut i=0u32;
    while (i as i32)<(w(m,world,0x60)? as i32){
        let data=w(m,w(m,world,0x6c)?,i.wrapping_mul(4))?;
        let function=w(m,w(m,world,0x68)?,i.wrapping_mul(4))?;
        h.callback(m,world,actor,function,data)?;i=i.wrapping_add(1);
    }
    while let Some(child)=h.first_dependency(m,actor)?{
        h.detach14(m,child,actor)?;
    }
    h.removed20(m,actor)?;
    let pool=w(m,world,0x18)?;let slot=h.actor_slot(m,actor)?;
    ai_spatial_construction::unlink(m,pool,slot)?;
    h.bind_actor(m,actor,0,0)?;Ok(1)
}
pub trait Services:Host+release::Services {
    fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>;
    fn marker_slot(&mut self,m:&mut Memory)->Result<u32>;
    fn bind50(&mut self,m:&mut Memory,p:u32,a:&registration::Endpoint,b:&registration::Endpoint)->Result<()>;
    fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32>;
}
/// Use after identifying the base World dispatch. Capacity rejection is
///ignored by tagged registration exactly as its native caller does.
pub struct Native<H> {pub world:release::World,pub callbacks:H}
impl<H:Services+RemovalHost> Native<H>{
    pub fn unregister_actor(&mut self,m:&mut Memory,p:u32)->Result<u32>{let root=m.word(self.world.root_slot)?;unregister(m,root,p,&mut self.callbacks)}
}
impl<H:Services> registration::Host for Native<H> {
    fn release_usage(&mut self,m:&mut Memory,p:u32)->Result<()>{release::release(m,self.world,p,false,&mut self.callbacks)}
    fn release_pool(&mut self,m:&mut Memory,p:u32)->Result<()>{release::release(m,self.world,p,true,&mut self.callbacks)}
    fn register18(&mut self,m:&mut Memory,p:u32)->Result<()>{let root=m.word(self.world.root_slot)?;let _=register(m,root,p,&mut self.callbacks)?;Ok(())}
    fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>{self.callbacks.initialize48(m,p)}
    fn marker_slot(&mut self,m:&mut Memory)->Result<u32>{self.callbacks.marker_slot(m)}
    fn bind50(&mut self,m:&mut Memory,p:u32,a:&registration::Endpoint,b:&registration::Endpoint)->Result<()>{self.callbacks.bind50(m,p,a,b)}
}
impl<H:Services> registration::GraphHost for Native<H>{fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32>{self.callbacks.kind(m,r)}}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::{Value,json};
    const W:u32=0x60000000;const A:u32=0x61000000;const A2:u32=A+0x1000;const POOL:u32=0x62000000;const FUN:u32=0x64000000;const DATA:u32=0x65000000;
    struct Spy<'a>{config:&'a Value,trace:Vec<Value>,calls:u32,removing:bool}
    impl Host for Spy<'_>{
        fn callback(&mut self,m:&mut Memory,world:u32,actor:u32,function:u32,data:u32)->Result<()> {
            assert_eq!(world,W);
            self.trace.push(json!(["callback",function,data,actor,w(m,actor,20)?,w(m,actor,40)?,w(m,POOL,20)?]));self.calls+=1;
            if self.calls==1 {
                let mutation=self.config["mutation"].as_str().unwrap().to_owned();
                let count=if self.removing{96}else{80};let functions=if self.removing{104}else{88};let data=if self.removing{108}else{92};
                match mutation.as_str(){
                    "count"=>m.write(W+count,0)?,"grow"=>m.write(W+count,3)?,"arrays"=>{m.write(W+functions,FUN+0x1000)?;m.write(W+data,DATA+0x1000)?;},"actor"=>m.write(A+20,0x77777777)?,"pool"=>m.write(POOL+20,u32::MAX)?,
                    "nested"=>{let result=register(m,W,A2,self)?;self.trace.push(json!(["nested",result]));},_=>{}
                }
            }Ok(())
        }
        fn inserted24(&mut self,m:&mut Memory,a:u32)->Result<()> {self.trace.push(json!(["inserted",a,w(m,a,20)?,w(m,a,40)?,w(m,POOL,20)?]));Ok(())}
    }
    impl RemovalHost for Spy<'_>{
        fn detach14(&mut self,m:&mut Memory,child:u32,actor:u32)->Result<()>{
            let list=w(m,actor,44)?;let node=w(m,list,8)?;assert_eq!(w(m,node,4)?,child);
            self.trace.push(json!(["detach14",child,actor,node]));let next=w(m,node,12)?;m.write(list+8,next)
        }
        fn removed20(&mut self,m:&mut Memory,actor:u32)->Result<()>{
            self.trace.push(json!(["removed20",actor,w(m,actor,20)?,w(m,actor,40)?,w(m,POOL,20)?]));
            if self.config["swap"].as_bool().unwrap_or(false){m.write(W+24,POOL+4096)?;m.write(actor+20,0x63000000+4096+20)?;}Ok(())
        }
    }
    #[test] fn original_world_pool_registration_callbacks_and_reentrancy_match()->Result<()> {
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_world_registration.json"))?;
        for case in f["cases"].as_array().unwrap(){let input=&case["input"];let mut m:Memory=serde_json::from_value(input["memory"].clone())?;let mut h=Spy{config:&input["config"],trace:vec![],calls:0,removing:false};
            let result=register(&mut m,W,A,&mut h)?;
            assert_eq!(json!(result),case["result"],"result: {}",input["config"]);
            assert_eq!(json!(h.trace),case["trace"],"callbacks: {}",input["config"]);
            assert_eq!(serde_json::to_value(m)?,case["memory"],"memory: {}",input["config"]);
        }Ok(())
    }
    #[test] fn original_world_unregistration_children_callbacks_and_roundtrips_match()->Result<()> {
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_world_unregistration.json"))?;
        for case in f["cases"].as_array().unwrap(){let input=&case["input"];let c=&input["config"];let mut m:Memory=serde_json::from_value(input["memory"].clone())?;let mut h=Spy{config:c,trace:vec![],calls:0,removing:c["op"]=="remove"};
            if c["op"]=="roundtrip" {let result=register(&mut m,W,A,&mut h)?;h.trace.push(json!(["registered",result]));h.removing=true;}
            let result=unregister(&mut m,W,A,&mut h)?;
            assert_eq!(json!(result),case["result"],"result: {c}");assert_eq!(json!(h.trace),case["trace"],"trace: {c}");assert_eq!(serde_json::to_value(m)?,case["memory"],"memory: {c}");
        }Ok(())
    }
}
