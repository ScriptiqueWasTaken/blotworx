//!484ce8/484dc4 traversal-policy release,486250/486330 status updates and
//!48568c/489510 actor notification. Canonical bytes, no supplied result flags.
use super::{ai_actor_queries,ai_graph_link_passes::Memory,ai_traversal_exclusion as exclusion,ai_traversal_registration as registration};
use anyhow::Result;
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
#[derive(Clone,Copy)]
pub struct World {pub root_slot:u32,pub manager_class:u32,pub pathfinder_class:u32}
pub trait Services:exclusion::Services {
    fn class18(&mut self,m:&mut Memory,service:u32)->Result<u32>;
}
///486250/486330 return1 on an already matching status in any ready graph,
///including after previous graphs have been changed. First match per graph.
pub fn status(m:&mut Memory,manager:u32,policy:u32,enabled:bool)->Result<u32>{
    if w(m,manager,0x18)?==0 {return Ok(0)}
    let mut index=0u32;let mut offset=0u32;
    loop {
        let header=w(m,manager,0x18)?;
        if index>=w(m,header,8)? {break}
        let graph=if index<w(m,header,8)? {w(m,header,4)?.wrapping_add(offset)} else {0};
        if graph!=0 && w(m,graph,0x2d4)?==6 {
            let slots=w(m,graph,0x2c0)?.wrapping_add(w(m,manager,4)?).wrapping_sub(0x4000);
            for i in 0..slots {
                let table=w(m,graph,0x2cc)?;
                if w(m,table,i.wrapping_mul(4))?!=policy {continue}
                let state=w(m,graph,0x2c8)?.wrapping_add(i.wrapping_mul(12)).wrapping_add(8);
                let old=m.byte(state)?;
                if old==u8::from(enabled) {return Ok(1)}
                m.write_byte(state,u8::from(enabled))?;break
            }
        }
        offset=offset.wrapping_add(0x8630);index=index.wrapping_add(1);
    }
    Ok(0)
}
fn cached_manager(m:&mut Memory,c:World,p:u32,h:&mut impl Services)->Result<u32>{
    if w(m,p,0x90)?==0 {
        let root=m.word(c.root_slot)?;
        let manager=exclusion::find_service(m,root,c.manager_class,h)?;
        m.write(p.wrapping_add(0x90),manager)?;
    }
    w(m,p,0x90)
}
///48568c captures the count;484ce8 reloads it after every actor callback.
pub fn notify(m:&mut Memory,c:World,p:u32,pool:bool,h:&mut impl Services)->Result<()> {
    let captured=if pool {w(m,m.word(c.root_slot)?,0x14)?} else {0};
    let mut index=0u32;
    while index<if pool {captured}else{w(m,m.word(c.root_slot)?,0x14)?} {
        let bits=w(m,p,if pool{0x88}else{0x8c})?;
        if exclusion::bit(m,bits,index)?&255!=0 {
            let manager=w(m,p,0x90)?;
            let actor=w(m,w(m,manager,0x14)?,index.wrapping_mul(4))?;
            if actor!=0 {
                let owner=w(m,actor,0x18)?;
                let service=ai_actor_queries::find_in_memory(m,owner,c.pathfinder_class,&mut |m,s|h.class18(m,s))?;
                if service!=0 {
                    m.write_byte(service.wrapping_add(0x48),1)?;
                    m.write_byte(service.wrapping_add(0xf5),1)?;
                }
            }
        }
        index=index.wrapping_add(1);
    }
    Ok(())
}
pub fn release(m:&mut Memory,c:World,p:u32,pool:bool,h:&mut impl Services)->Result<()> {
    let manager=cached_manager(m,c,p,h)?;
    if status(m,manager,p,pool)?&255==0 {notify(m,c,p,pool,h)?;}
    Ok(())
}
/// Remaining methods are actual World/derived virtual operations. This
///adapter always supplies concrete release/status/notification behavior.
pub trait Host:Services {
    fn register18(&mut self,m:&mut Memory,p:u32)->Result<()>;
    fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>;
    fn marker_slot(&mut self,m:&mut Memory)->Result<u32>;
    fn bind50(&mut self,m:&mut Memory,p:u32,a:&registration::Endpoint,b:&registration::Endpoint)->Result<()>;
    fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32>;
}
pub struct Native<H> {pub world:World,pub callbacks:H}
impl<H:Host> registration::Host for Native<H> {
    fn release_usage(&mut self,m:&mut Memory,p:u32)->Result<()> {release(m,self.world,p,false,&mut self.callbacks)}
    fn release_pool(&mut self,m:&mut Memory,p:u32)->Result<()> {release(m,self.world,p,true,&mut self.callbacks)}
    fn register18(&mut self,m:&mut Memory,p:u32)->Result<()> {self.callbacks.register18(m,p)}
    fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()> {self.callbacks.initialize48(m,p)}
    fn marker_slot(&mut self,m:&mut Memory)->Result<u32> {self.callbacks.marker_slot(m)}
    fn bind50(&mut self,m:&mut Memory,p:u32,a:&registration::Endpoint,b:&registration::Endpoint)->Result<()> {self.callbacks.bind50(m,p,a,b)}
}
impl<H:Host> registration::GraphHost for Native<H> {
    fn kind(&mut self,m:&mut Memory,r:u32)->Result<u32> {self.callbacks.kind(m,r)}
}
#[cfg(test)] pub(super) mod tests {
    use super::*;
    use serde_json::{Value,json};
    const ROOT:u32=0x69000000;const P:u32=0x73000000;const MAN:u32=0x74000000;const OTHER:u32=0x74100000;const HEADER:u32=0x75000000;const GRAPHS:u32=0x76000000;const META:u32=0x77000000;const CLASS:u32=0x77f00000;const STATES:u32=0x78000000;const REFS:u32=0x79000000;const ACTORS:u32=0x7a000000;const ACTOR:u32=0x7a100000;const OWNER:u32=0x7a200000;const ARRAY:u32=0x7a300000;const SERVICE:u32=0x7a400000;const REPLACEMENT:u32=0x7a500000;const BITS:u32=0x7b000000;const BITSETS:u32=0x7c000000;const MASK:u32=0x7b100000;
    fn n(v:&Value)->u32{v.as_u64().unwrap() as u32}
    fn region(m:&mut Memory,p:u32,n:usize){assert!(!m.regions.contains_key(&p));m.regions.insert(p,vec![0;n]);}
    pub(crate) fn setup(c:&Value)->Result<Memory>{
        let mut m=Memory::default();let graphs=c["graphs"].as_array().unwrap();
        for(p,len)in[(0x5a108,4),(ROOT,64),(ROOT+0x1000,64),(P,160),(MAN,28),(OTHER,28),(HEADER,16),(GRAPHS,graphs.len().max(1)*0x8630),(CLASS,0x300),(ACTORS,16),(0x7d000000,16),(0x7d001000,16),(0x7c100000,12),(0x7c100010,12),(MASK,32),(0x7a600000,16)]{region(&mut m,p,len);}
        m.write(0x5a108,ROOT)?;m.write(ROOT+0x14,c["actor_count"].as_u64().map(|v|v as u32).unwrap_or(c["actors"].as_array().unwrap().len() as u32))?;m.write(ROOT+0x34,0x7d000000)?;m.write(ROOT+0x38,3)?;
        m.write(ROOT+0x1014,1)?;
        for(i,p)in[0,OTHER,MAN].iter().enumerate(){m.write(0x7d000000+i as u32*4,*p)?;m.write(0x7d001000+i as u32*4,if i==0{0}else{OTHER})?;}
        for p in [MAN,OTHER]{m.write(p+4,0x4000u32.wrapping_add(c["bias"].as_i64().unwrap() as u32))?;m.write(p+0x14,ACTORS)?;m.write(p+0x18,if c["header"].as_bool().unwrap(){HEADER}else{0})?;}
        m.write(CLASS+0x114,0x6af44)?;m.write(CLASS+0x214,0x6af6c)?;
        m.write(P+0x90,if c["cached"].as_bool().unwrap(){MAN}else{0})?;m.write(HEADER+4,GRAPHS)?;m.write(HEADER+8,c["graph_count"].as_u64().map(|v|v as u32).unwrap_or(graphs.len() as u32))?;
        for(i,p)in c["actors"].as_array().unwrap().iter().enumerate(){m.write(ACTORS+i as u32*4,n(p))?;}
        for(g,graph)in graphs.iter().enumerate(){
            let off=g as u32*0x1000;let at=GRAPHS+g as u32*0x8630;let entries=graph["entries"].as_array().unwrap();
            for(p,len)in[(META+off,entries.len().max(1)*8),(STATES+off,entries.len().max(1)*12),(REFS+off,(entries.len()+1)*4)]{region(&mut m,p,len);}
            m.write(at+0x2d4,n(&graph["phase"]))?;m.write(at+0x2c0,graph["declared"].as_u64().map(|v|v as u32).unwrap_or(entries.len() as u32))?;m.write(at+0x2c4,META+off)?;m.write(at+0x2c8,STATES+off)?;m.write(at+0x2cc,REFS+off)?;
            for(i,e)in entries.iter().enumerate(){
                let i=i as u32;let bit=BITSETS+off+i*16;let data=BITS+off+i*16;region(&mut m,bit,4);region(&mut m,data,8);
                m.write(REFS+off+i*4,n(&e["interface"]))?;m.write(META+off+i*8+4,n(&e["flags"])<<16)?;m.write(STATES+off+i*12,if e["bitset"].as_bool().unwrap(){bit}else{0})?;m.write_byte(STATES+off+i*12+8,n(&e["enabled"]) as u8)?;m.write(bit,data)?;
                for(j,v)in e["mask"].as_array().unwrap().iter().enumerate(){m.write_byte(data+j as u32,n(v) as u8)?;}
            }
        }
        m.write(P+0x8c,0x7c100000)?;m.write(P+0x88,0x7c100010)?;m.write(0x7c100000,MASK)?;m.write(0x7c100010,MASK+16)?;m.write_byte(MASK,n(&c["usage_mask"]) as u8)?;m.write_byte(MASK+16,n(&c["pool_mask"]) as u8)?;
        for i in 0..3 {let off=i*0x1000;let a=ACTOR+off;let o=OWNER+off;let t=ARRAY+off;region(&mut m,a,32);region(&mut m,o,32);region(&mut m,t,8);m.write(a+0x18,o)?;m.write(o+12,t)?;m.write(o+16,c["service_count"].as_i64().unwrap() as u32)?;
            for j in 0..3 {let s=if j<2{SERVICE+off+j*256}else{REPLACEMENT+off};region(&mut m,s,256);m.write(s+0x48,0x55010203)?;m.write(s+0xf4,0x66778899)?;if j<2{m.write(t+j*4,s)?;}}
        }
        Ok(m)
    }
    struct Spy<'a>{c:&'a Value,trace:Vec<Value>}
    impl exclusion::Services for Spy<'_>{fn class20(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["class20",p]));Ok(if p==OTHER{CLASS}else if n(&self.c["class_kind"])==0{0x6af44}else{CLASS+0x100})}}
    impl Services for Spy<'_>{fn class18(&mut self,m:&mut Memory,p:u32)->Result<u32>{
        self.trace.push(json!(["class18",p]));let i=(p-SERVICE)/0x1000;let j=((p-SERVICE)%0x1000)/256;
        if i==0 && j==1 {match self.c["mutation"].as_str().unwrap(){
            "count"=>m.write(ROOT+0x14,1)?,"root"=>m.write(0x5a108,ROOT+0x1000)?,"slot"=>m.write(ARRAY+4,REPLACEMENT)?,"array"=>{m.write(OWNER+12,0x7a600000)?;m.write(0x7a600004,REPLACEMENT)?;},"mask"=>{m.fill(MASK,8,0)?;m.fill(MASK+16,8,0)?;},_=>{}
        }}
        Ok(if j==0 || n(&self.c["actor_kind"])==2{CLASS}else if n(&self.c["actor_kind"])==0{0x6af6c}else{CLASS+0x200})
    }}
    impl Host for Spy<'_>{
        fn register18(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected registration")}
        fn initialize48(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected initialization")}
        fn marker_slot(&mut self,_:&mut Memory)->Result<u32>{anyhow::bail!("Unexpected marker")}
        fn bind50(&mut self,_:&mut Memory,_:u32,_:&registration::Endpoint,_:&registration::Endpoint)->Result<()>{anyhow::bail!("Unexpected endpoint binding")}
        fn kind(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Unexpected graph kind")}
    }
    fn world()->World{World{root_slot:0x5a108,manager_class:0x6af44,pathfinder_class:0x6af6c}}
    #[test] fn original_release_status_and_actor_notifications_match()->Result<()> {
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_policy_release.json"))?;
        for case in f["cases"].as_array().unwrap(){let c=&case["input"];let mut m=setup(c)?;let mut host=Native{world:world(),callbacks:Spy{c,trace:vec![]}};
            let result=match c["op"].as_str().unwrap(){"usage"=>{registration::Host::release_usage(&mut host,&mut m,P)?;None},"pool"=>{registration::Host::release_pool(&mut host,&mut m,P)?;None},"notify"=>{notify(&mut m,world(),P,true,&mut host.callbacks)?;None},"status_off"=>Some(status(&mut m,MAN,P,false)?),"status_on"=>Some(status(&mut m,MAN,P,true)?),_=>unreachable!()};
            assert_eq!(json!(result),case["result"],"result: {c}");assert_eq!(json!(host.callbacks.trace),case["trace"],"trace: {c}");
            for(key,p)in[("cache",P+0x90),("root",0x5a108),("actor_count",ROOT+0x14)]{assert_eq!(json!(m.word(p)?),case[key],"{key}: {c}");}
            let masks=(0..2).map(|i|(0..8).map(|j|m.byte(MASK+i*16+j)).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;assert_eq!(json!(masks),case["masks"],"masks: {c}");
            let marks=(0..3).map(|i|[SERVICE+i*0x1000,SERVICE+i*0x1000+256,REPLACEMENT+i*0x1000].into_iter().map(|s|Ok([w(&m,s,0x48)?,w(&m,s,0xf4)?])).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;assert_eq!(json!(marks),case["marks"],"marks: {c}");
            let mut states=Vec::new();for(g,graph)in c["graphs"].as_array().unwrap().iter().enumerate(){let mut rows=Vec::new();for i in 0..graph["entries"].as_array().unwrap().len(){rows.push((0..12).map(|j|m.byte(STATES+g as u32*0x1000+i as u32*12+j)).collect::<Result<Vec<_>>>()?);}states.push(rows);}assert_eq!(json!(states),case["states"],"states: {c}");
            for(key,base,off)in[("owner_tables",OWNER,12),("first_slots",ARRAY,4)]{let actual=(0..3).map(|i|m.word(base+i*0x1000+off)).collect::<Result<Vec<_>>>()?;assert_eq!(json!(actual),case[key],"{key}: {c}");}
        }Ok(())
    }
}
