//!195f80/1c20c8: source task/provider gates, bounds and Goto publication.
//! Manager/provider and traversal interfaces remain required implementations.
use anyhow::{Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_agent_path::{self,ResetHost}};
pub trait Host:ResetHost {
    fn request_manager(&mut self,m:&mut Memory)->Result<u32>;
    fn request_task(&mut self,m:&mut Memory,reservation:u32)->Result<u32>;
    fn request_enabled(&mut self,m:&mut Memory,manager:u32,task:u32)->Result<u32>;
    fn service_provider(&mut self,m:&mut Memory,service:u32)->Result<u32>;
    fn provider_ready(&mut self,m:&mut Memory,provider:u32)->Result<u32>;
    fn provider_configuration(&mut self,m:&mut Memory,provider:u32)->Result<u32>;
    fn configuration_position(&mut self,m:&mut Memory,configuration:u32)->Result<[f32;3]>;
    fn reservation_extent(&mut self,m:&mut Memory,reservation:u32)->Result<f32>;
    fn request_bounds(&mut self,m:&mut Memory,reservation:u32,lower:[f32;3],upper:[f32;3],flags:[u32;2])->Result<u32>;
    fn set_target(&mut self,m:&mut Memory,reservation:u32,node:u32,position:[f32;3])->Result<()>;
}
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
///195ac0 virtual20, shared by all three path classes. Release uses the live
///class at the call site; the subsequent index/status writes are unconditional.
pub fn fail(m:&mut Memory,p:u32,reservation:u32,h:&mut impl ResetHost)->Result<()> {
    ai_agent_path::release(m,p,reservation,h)?;
    m.write(p.wrapping_add(0x68),u32::MAX)?;m.write_byte(p.wrapping_add(8),1)
}
fn base(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    let manager=h.request_manager(m)?;
    let task=h.request_task(m,r)?;
    if h.request_enabled(m,manager,task)? as u8==0{return Ok(false)}
    let service=w(m,p,12)?;let provider=h.service_provider(m,service)?;
    if provider==0{return Ok(false)}
    if h.provider_ready(m,provider)? as u8==0{return Ok(false)}
    let configuration=h.provider_configuration(m,provider)?;
    if configuration==0{fail(m,p,r,h)?;return Ok(false)}
    let extent=h.reservation_extent(m,r)?;
    //Source4bd05c float bits3f933333. Position is read AFTER extent virtual5c.
    let margin=1.15f32*extent;
    let position=h.configuration_position(m,configuration)?;
    let lower=position.map(|v|v-margin);let upper=position.map(|v|v+margin);
    if h.request_bounds(m,r,lower,upper,[1,0])? as u8==0{fail(m,p,r,h)?;return Ok(false)}
    Ok(true)
}
pub fn request(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    let class=w(m,p,0x868)?;
    ensure!([0x4eb980,0x4eb818,0x4eae38].contains(&class),"Path request class is unsupported");
    if !base(m,p,r,h)?{return Ok(false)}
    if class==0x4eae38 {
        //1c20c8 captures the selected class on entry, but reads target fields
        //fresh after the base request (which may invoke policy callbacks).
        let node=w(m,p,0x87c)?;
        let position=[f32::from_bits(w(m,p,0x870)?),f32::from_bits(w(m,p,0x874)?),f32::from_bits(w(m,p,0x878)?)];
        h.set_target(m,r,node,position)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::io::Read;
    const P:u32=0x72000000;const R:u32=0x73000000;const S:u32=0x74000000;const SPACE:u32=0x75000000;const CFG:u32=0x76000000;
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>}
    impl Fixture<'_>{fn n(&self,k:&str,d:u32)->u32{self.c[k].as_u64().unwrap_or(d as u64) as u32}fn flag(&self,k:&str)->bool{self.c[k].as_bool().unwrap_or(true)}}
    impl ResetHost for Fixture<'_>{
        fn reservation(&mut self,_:&mut Memory,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Request must not reacquire reservation")}
        fn release(&mut self,_:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["release",r]));Ok(())}
    }
    impl Host for Fixture<'_>{
        fn request_manager(&mut self,_:&mut Memory)->Result<u32>{self.trace.push(json!(["manager"]));Ok(80)}
        fn request_task(&mut self,m:&mut Memory,r:u32)->Result<u32>{w(m,r,12)}
        fn request_enabled(&mut self,_:&mut Memory,a:u32,t:u32)->Result<u32>{self.trace.push(json!(["enabled",a,t]));Ok(self.n("enabled",1))}
        fn service_provider(&mut self,_:&mut Memory,s:u32)->Result<u32>{self.trace.push(json!(["provider",s]));Ok(if self.flag("provider"){SPACE}else{0})}
        fn provider_ready(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["provider_ready",p]));Ok(self.n("ready",1))}
        fn provider_configuration(&mut self,m:&mut Memory,p:u32)->Result<u32>{w(m,p,4)}
        fn configuration_position(&mut self,m:&mut Memory,p:u32)->Result<[f32;3]>{Ok([f32::from_bits(w(m,p,4)?),f32::from_bits(w(m,p,8)?),f32::from_bits(w(m,p,12)?)])}
        fn reservation_extent(&mut self,m:&mut Memory,r:u32)->Result<f32>{self.trace.push(json!(["extent",r]));if self.c["mutation"]=="point"{m.write(CFG+4,100f32.to_bits())?;}Ok(self.c["extent"].as_f64().unwrap_or(2.) as f32)}
        fn request_bounds(&mut self,m:&mut Memory,r:u32,l:[f32;3],u:[f32;3],flags:[u32;2])->Result<u32>{self.trace.push(json!(["bounds",r,l.map(f32::to_bits),u.map(f32::to_bits),flags[0],flags[1]]));if self.c["mutation"]=="target"{m.write(P+0x87c,99)?;m.write(P+0x870,100f32.to_bits())?;}Ok(self.n("result",1))}
        fn set_target(&mut self,m:&mut Memory,r:u32,n:u32,v:[f32;3])->Result<()>{m.write(r+0x70,n)?;for(i,v)in v.into_iter().enumerate(){m.write(r+0x74+i as u32*4,v.to_bits())?;}Ok(())}
    }
    fn unpack(hex:&str)->Result<Vec<u8>>{let compressed=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect::<Vec<_>>();let mut out=vec![];flate2::read::ZlibDecoder::new(&compressed[..]).read_to_end(&mut out)?;Ok(out)}
    #[test]
    fn original_request_gates_bounds_failures_and_fresh_goto_target_match()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_path_request.json"))?;
        for row in fixture["cases"].as_array().unwrap(){let c=&row["input"];let mut h=Fixture{c,trace:vec![]};let mut m=Memory::default();
            for(p,n,fill)in[(P,0x8b4,0xcd),(R,0xa8,0xa5),(S,0x100,0),(SPACE,16,0),(CFG,16,0)]{m.regions.insert(p,vec![fill;n]);}
            let class=match c["kind"].as_str().unwrap(){"Base"=>0x4eb980,"Wander"=>0x4eb818,_=>0x4eae38};
            for(o,v)in[(0x868,class),(12,S),(0x68,77),(0x87c,44)]{m.write(P+o,v)?;}
            for(o,v)in[(8,6),(0x10,5),(0x80,7),(0x8a4,9)]{m.write_byte(P+o,v)?;}
            for(i,v)in[1f32,2.,3.].into_iter().enumerate(){m.write(P+0x870+i as u32*4,v.to_bits())?;}
            for(i,v)in[4f32,5.,6.].into_iter().enumerate(){m.write(CFG+4+i as u32*4,v.to_bits())?;}
            m.write(R,0x77000000)?;m.write(R+12,33)?;m.write(S,0x77000100)?;m.write(SPACE+4,if h.flag("config"){CFG}else{0})?;
            assert_eq!(u32::from(request(&mut m,P,R,&mut h)?),row["result"].as_u64().unwrap() as u32,"result {c}");assert_eq!(json!(h.trace),row["trace"],"trace {c}");
            assert_eq!(m.regions[&P],unpack(row["image"].as_str().unwrap())?,"path {c}");assert_eq!(m.regions[&R],unpack(row["reservation"].as_str().unwrap())?,"reservation {c}");
        }Ok(())
    }
}
