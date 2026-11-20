//! Per-agent update19579c/195b1c/195e28. The traversal and derived virtual
//! operations are required policies; readiness is never inferred from bind.
use anyhow::{Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_agent_path::{self,ResetHost}};
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn f(m:&Memory,p:u32,o:u32)->Result<f32>{Ok(f32::from_bits(w(m,p,o)?))}
fn put(m:&mut Memory,p:u32,o:u32,v:u32)->Result<()>{m.write(p.wrapping_add(o),v)}
pub trait Host:ResetHost {
    fn reservation_word(&self,m:&Memory,reservation:u32,offset:u32)->Result<u32>;
    fn reservation_byte(&self,m:&Memory,reservation:u32,offset:u32)->Result<u8>;
    fn owner_position(&mut self,m:&mut Memory,owner:u32)->Result<[f32;3]>;
    /// Actual derived path virtuals18/1c; preserve full return words.
    fn path_virtual(&mut self,m:&mut Memory,path:u32,slot:u32,reservation:u32)->Result<u32>;
    fn refresh(&mut self,m:&mut Memory,reservation:u32)->Result<()>;
    fn accepts_point(&mut self,m:&mut Memory,reservation:u32,point:u32)->Result<u32>;
    ///473c90 writes the index and output count before returning its status.
    fn result(&mut self,m:&mut Memory,reservation:u32,index:u32)->Result<(u32,u32,u32)>;
    fn sample(&mut self,m:&mut Memory,reservation:u32,point:u32,index:u32)->Result<()>;
    fn acknowledge(&mut self,m:&mut Memory,reservation:u32)->Result<()>;
}
pub fn motion(m:&mut Memory,p:u32,vector:[f32;3],scalar:f32)->Result<()> {
    for (i,v) in vector.into_iter().enumerate(){put(m,p,0x6c+i as u32*4,v.to_bits())?;}
    put(m,p,0x78,scalar.to_bits())
}
pub fn status(m:&Memory,p:u32)->Result<u32>{Ok(m.byte(p.wrapping_add(8))? as u32)}
fn virtual_call(m:&mut Memory,p:u32,slot:u32,r:u32,h:&mut impl Host)->Result<u32>{
    ensure!([0x4eb980,0x4eb818,0x4eae38].contains(&w(m,p,0x868)?),"Path update derived class is unsupported");
    if slot==0x20{super::ai_agent_path_request::fail(m,p,r,h)?;return Ok(0)}
    h.path_virtual(m,p,slot,r)
}
///195e28; output count and sample content are live after each callback.
pub fn collect(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    let (status,_,count)=h.result(m,r,w(m,p,0x68)?)?;
    if status!=3{return Ok(false)}
    let mut index=0u32;
    while index<count {
        h.sample(m,r,p.wrapping_add(0x84).wrapping_add(index.wrapping_mul(0x14)),index)?;
        index=index.wrapping_add(1);
    }
    let mut append=false;
    if index!=0 {
        let last=index.wrapping_sub(1).wrapping_mul(0x14);
        //Native fcmpu equality: NaNs differ; signed zero compares equal.
        append=f(m,p,0x88u32.wrapping_add(last))?!=f(m,p,0x18)?
            ||f(m,p,0x8cu32.wrapping_add(last))?!=f(m,p,0x1c)?
            ||f(m,p,0x90u32.wrapping_add(last))?!=f(m,p,0x20)?;
    }
    if append {
        let offset=index.wrapping_mul(0x14);
        for (dst,src) in [(0x88u32,0x18),(0x8c,0x1c),(0x90,0x20)] {
            let value=w(m,p,src)?;put(m,p,dst.wrapping_add(offset),value)?;
        }
        put(m,p,0x84u32.wrapping_add(offset),1)?;put(m,p,0x94u32.wrapping_add(offset),0)?;
        put(m,p,0x854,count.wrapping_add(1))?;put(m,p,0x68,1)?;
    } else {put(m,p,0x854,count)?;}
    h.acknowledge(m,r)?;Ok(true)
}
fn complete(m:&Memory,r:u32,h:&impl Host)->Result<bool>{Ok(h.reservation_byte(m,r,0x14)?!=0 && h.reservation_byte(m,r,0x15)?!=0)}
///195b1c. All state reads after policy callbacks are fresh, including vtables.
pub fn ready(m:&mut Memory,p:u32,h:&mut impl Host)->Result<bool>{
    if status(m,p)?!=0{ai_agent_path::reset(m,p,h)?;}
    let limit=f(m,p,0x864)?;
    //Native ble branches also when unordered; only a positive limit proceeds.
    if limit>0. {
        let owner=w(m,p,0x7c)?;let position=h.owner_position(m,owner)?;
        let dx=f(m,p,0x858)?-position[0];let dz=f(m,p,0x860)?-position[2];let dy=f(m,p,0x85c)?-position[1];
        let squared=dy.mul_add(dy,dz.mul_add(dz,dx*dx));
        if squared>limit {
            for (i,value) in position.into_iter().enumerate(){put(m,p,0x858+i as u32*4,value.to_bits())?;}
            ai_agent_path::reset(m,p,h)?;m.write_byte(p.wrapping_add(0x10),0)?;
        }
    }
    m.write_byte(p.wrapping_add(0x80),1)?;
    let manager=w(m,p,4)?;let traversal=w(m,p,0)?;let owner=w(m,p,0x7c)?;
    let r=h.reservation(m,manager,traversal,owner)?;
    if r==0{return Ok(false)}
    if m.byte(p.wrapping_add(0x10))?==0 {
        let value=virtual_call(m,p,0x18,r,h)? as u8;m.write_byte(p.wrapping_add(0x10),value)?;
        if value==0{return Ok(false)}
    }
    if !(h.reservation_word(m,r,8)?!=0 && h.reservation_word(m,r,0x28)?!=u32::MAX) && virtual_call(m,p,0x1c,r,h)?==0{return Ok(false)}
    if !complete(m,r,h)? {
        if h.reservation_word(m,r,0x6c)?==0 {virtual_call(m,p,0x20,r,h)?;return Ok(false)}
        h.refresh(m,r)?;
    }
    if !complete(m,r,h)?{return Ok(false)}
    if h.accepts_point(m,r,p.wrapping_add(0x14))?==0 {virtual_call(m,p,0x20,r,h)?;return Ok(false)}
    if collect(m,p,r,h)? {
        //virtual14 is the known base/Goto release, not reset's reservation lookup.
        ai_agent_path::release(m,p,r,h)?;
        Ok(true)
    }else{virtual_call(m,p,0x20,r,h)?;Ok(false)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::io::Read;
    const P:u32=0x72000000;const O:u32=0x73000000;const R:u32=0x74000000;
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>}
    impl Fixture<'_>{fn n(&self,k:&str,d:u32)->u32{self.c[k].as_u64().unwrap_or(d as u64) as u32}}
    impl ResetHost for Fixture<'_>{
        fn reservation(&mut self,_:&mut Memory,manager:u32,t:u32,o:u32)->Result<u32>{self.trace.push(json!(["reservation",manager,t,o]));Ok(if self.c["reservation"].as_bool().unwrap_or(true){R}else{0})}
        fn release(&mut self,_:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["release",r]));Ok(())}
    }
    impl Host for Fixture<'_>{
        fn reservation_word(&self,m:&Memory,r:u32,o:u32)->Result<u32>{w(m,r,o)}
        fn reservation_byte(&self,m:&Memory,r:u32,o:u32)->Result<u8>{m.byte(r.wrapping_add(o))}
        fn owner_position(&mut self,m:&mut Memory,o:u32)->Result<[f32;3]>{Ok([f(m,o,0x34)?,f(m,o,0x38)?,f(m,o,0x3c)?])}
        fn path_virtual(&mut self,m:&mut Memory,p:u32,slot:u32,r:u32)->Result<u32>{self.trace.push(json!(["virtual",slot,r]));if slot==0x18&&self.c["mutation"]=="class"{put(m,p,0x868,0x4eb980)?;}Ok(self.n(if slot==0x18{"initialize_result"}else{"request_result"},1))}
        fn refresh(&mut self,m:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["refresh",r]));if self.c["refresh_complete"]==true{m.write_byte(r+0x14,1)?;m.write_byte(r+0x15,1)?;}Ok(())}
        fn accepts_point(&mut self,_:&mut Memory,r:u32,p:u32)->Result<u32>{self.trace.push(json!(["accept",r,p]));Ok(self.n("accept",1))}
        fn result(&mut self,_:&mut Memory,r:u32,i:u32)->Result<(u32,u32,u32)>{self.trace.push(json!(["result",r,i]));Ok((self.n("result",3),19,self.n("count",1)))}
        fn sample(&mut self,m:&mut Memory,_:u32,p:u32,i:u32)->Result<()>{self.trace.push(json!(["sample",i]));for(o,v)in[(0,1),(4,if self.c["same"]==true{1f32}else{4.}.to_bits()),(8,2f32.to_bits()),(12,3f32.to_bits()),(16,0)]{put(m,p,o,v)?;}Ok(())}
        fn acknowledge(&mut self,_:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["ack",r]));Ok(())}
    }
    #[test]
    fn original_ready_and_result_collection_match_bytes_and_policy_order()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_path_runtime.json"))?;
        for row in fixture["cases"].as_array().unwrap(){
            let c=&row["input"];let mut h=Fixture{c,trace:vec![]};let mut m=Memory::default();
            for(p,n,fill)in[(P,0x8b4,0xcd),(O,0x100,0),(R,0x100,0)]{m.regions.insert(p,vec![fill;n]);}
            for(o,v)in[(0,5),(4,60),(12,30),(0x68,7),(0x7c,O),(0x854,0),(0x864,(c["limit"].as_f64().unwrap_or(0.) as f32).to_bits()),(0x868,0x4eae38)]{put(&mut m,P,o,v)?;}
            for(o,v)in[(8,h.n("status",0)),(0x10,h.n("initialized",0)),(0x80,h.n("active",0)),(0x8a4,7)]{m.write_byte(P+o,v as u8)?;}
            for(o,values)in[(0x18,[1f32,2.,3.]),(0x858,[0.;3])]{for(i,v)in values.into_iter().enumerate(){put(&mut m,P,o+i as u32*4,v.to_bits())?;}}
            for(i,v)in[2f32,3.,4.].into_iter().enumerate(){put(&mut m,O,0x34+i as u32*4,v.to_bits())?;}
            for(o,v)in[(8,h.n("bound",1)),(0x28,h.n("index",0)),(0x6c,h.n("pending",0))]{put(&mut m,R,o,v)?;}
            for o in[0x14,0x15]{m.write_byte(R+o,h.n("complete",1) as u8)?;}
            let result=if c["op"]=="collect"{collect(&mut m,P,R,&mut h)?}else{ready(&mut m,P,&mut h)?};
            assert_eq!(u32::from(result),row["result"].as_u64().unwrap() as u32,"result {c}");assert_eq!(json!(h.trace),row["trace"],"trace {c}");
            let hex=row["image"].as_str().unwrap();let compressed=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect::<Vec<_>>();
            let mut expected=Vec::new();flate2::read::ZlibDecoder::new(&compressed[..]).read_to_end(&mut expected)?;
            assert_eq!(m.regions[&P],expected,"image {c}");
        }Ok(())
    }
}
