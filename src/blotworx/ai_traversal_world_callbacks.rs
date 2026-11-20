//!4869d4/486a40 World callbacks and their canonical actor/link cleanup.
//! Graph edge unbinding47581c and derived virtual28 remain required dispatch.
use super::{ai_graph_link_passes::Memory,ai_traversal_exclusion as exclusion,ai_traversal_policy_release as release,ai_actor_queries,ai_navigation_world_registration as admission};
use anyhow::{ensure,Result};
use std::collections::BTreeSet;
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn row(p:u32,i:u32,stride:u32)->u32{p.wrapping_add(i.wrapping_mul(stride))}
#[derive(Clone,Copy)]
pub struct Context{pub world:release::World,pub path_object_class:u32,pub edge_object_class:u32}
pub trait Host:release::Services {
    fn unbind_graph(&mut self,m:&mut Memory,graph:u32,a:u32,b:u32,tag:u32)->Result<()>;
    fn toggle28(&mut self,m:&mut Memory,policy:u32,actor:u32)->Result<()>;
}
fn compatible(m:&Memory,mut class:u32,target:u32)->Result<bool>{
    let mut seen=BTreeSet::new();loop{if class==target{return Ok(true)}ensure!(seen.insert(class),"Cyclic path-object class ancestry");class=w(m,class,20)?;if class==0{return Ok(false)}}
}
///4869f4: first empty slot, no duplicate suppression or pool-index inference.
pub fn track(m:&mut Memory,c:Context,manager:u32,actor:u32)->Result<()> {
    let count=w(m,m.word(c.world.root_slot)?,20)?;
    for i in 0..count {let table=w(m,manager,20)?;let at=row(table,i,4);if m.word(at)?==0 {m.write(at,actor)?;break}}
    Ok(())
}
pub fn added(m:&mut Memory,c:Context,actor:u32,manager:u32)->Result<()> {
    if w(m,actor,24)?!=0 {track(m,c,manager,actor)?;}Ok(())
}
///486bd8: remove first matching actor, then clear its two masks in each
///active nonnull pooled path object. Counts/tables are read at native points.
pub fn forget(m:&mut Memory,c:Context,manager:u32,actor:u32)->Result<()> {
    let index=exclusion::actor_index(m,c.world.root_slot,manager,actor)?;
    if index==u32::MAX{return Ok(())}
    let table=w(m,manager,20)?;m.write(row(table,index,4),0)?;
    let mut i=0;while i<w(m,manager,12)? {
        let pool=row(w(m,manager,8)?,i,12);let mut j=0;
        while j<w(m,pool,4)? {
            let p=w(m,w(m,pool,0)?,j.wrapping_mul(4))?;
            if p!=0 && m.byte(p.wrapping_add(0x7c))?!=0 {
                let bits=w(m,p,0x8c)?;exclusion::set_bit(m,bits,index,false)?;
                let bits=w(m,p,0x88)?;exclusion::set_bit(m,bits,index,false)?;
            }j=j.wrapping_add(1);
        }i=i.wrapping_add(1);
    }Ok(())
}
///486cb0 deliberately leaves policy slot0 untouched and compares IDs, not
///object identity. It scans all captured graphs, regardless of their phase.
pub fn clear_slots(m:&mut Memory,manager:u32,p:u32,h:&mut impl Host)->Result<()> {
    let header=w(m,manager,24)?;let count=if header==0{0}else{w(m,header,8)?};
    for i in 0..count {
        let header=w(m,manager,24)?;
        let graph=if i<w(m,header,8)?{row(w(m,header,4)?,i,0x8630)}else{0};
        let slots=w(m,graph,0x2c0)?.wrapping_add(w(m,manager,4)?).wrapping_sub(0x4000);
        for j in 1..slots {let table=w(m,graph,0x2cc)?;let other=w(m,table,j.wrapping_mul(4))?;if other!=0 && w(m,other,0x78)?==w(m,p,0x78)?{m.write(row(table,j,4),0)?;}}
    }h.toggle28(m,p,0)
}
fn edge(m:&Memory,p:u32,a:u32,b:u32,g:u32)->Result<Option<u32>>{
    let count=w(m,p,0x84)?;
    for i in 0..count {let e=row(w(m,p,0x80)?,i,24);if w(m,e,0)?&0x7fffffff==a&0x7fffffff && w(m,e,4)?&0x7fffffff==b&0x7fffffff && w(m,e,8)?==g {return Ok(Some(e))}}
    Ok(None)
}
///484e2c: unbind graph, notify/clear pool users, then repair cached segments
///to the next linked owner. The node IDs retain their original mask semantics.
pub fn drop_edge(m:&mut Memory,c:Context,p:u32,a:u32,b:u32,g:u32,h:&mut impl Host)->Result<u32>{
    if edge(m,p,a,b,g)?.is_none(){return Ok(0)}
    let tag=w(m,p,0x78)?;h.unbind_graph(m,g,a,b,tag)?;
    release::notify(m,c.world,p,true,h)?;
    let bits=w(m,p,0x88)?;let data=w(m,bits,0)?;let size=w(m,bits,8)?;m.fill(data,size,0)?;
    let count=w(m,m.word(c.world.root_slot)?,20)?;
    for i in 0..count {
        let bits=w(m,p,0x8c)?;if exclusion::bit(m,bits,i)?&255==0{continue}
        let actor=w(m,w(m,w(m,p,0x90)?,20)?,i.wrapping_mul(4))?;
        if actor==0{continue}
        let owner=w(m,actor,24)?;
        let service=ai_actor_queries::find_in_memory(m,owner,c.world.pathfinder_class,&mut |m,s|h.class18(m,s))?;
        if service==0{continue}
        let path=w(m,service,0x60)?;if path==0{continue}
        let mut at=w(m,path,0x1bc)?;let end=w(m,path,0x1b8)?;let mut previous=0;let mut seen=BTreeSet::new();
        while at!=end {
            ensure!(seen.insert(at),"Cyclic cached path segment range");
            if previous!=0 && w(m,previous,0)?&0x7fffffff==a&0x7fffffff && w(m,at,0)?&0x7fffffff==b&0x7fffffff && w(m,at,16)?==p {
                let next=if let Some(e)=edge(m,p,a,b,g)?{w(m,e,16)?}else{0};m.write(at.wrapping_add(16),next)?;
            }previous=at;at=at.wrapping_add(20);
        }
    }Ok(1)
}
pub fn removed(m:&mut Memory,c:Context,p:u32,manager:u32,h:&mut impl Host)->Result<()> {
    let class=h.class18(m,p)?;
    if compatible(m,class,c.path_object_class)? {
        release::release(m,c.world,p,true,h)?;
        let class=h.class18(m,p)?;
        if compatible(m,class,c.edge_object_class)? {
            let mut i=0;while i<w(m,p,0x84)? {let e=row(w(m,p,0x80)?,i,24);let a=w(m,e,0)?;let b=w(m,e,4)?;let g=w(m,e,8)?;if a!=0 && b!=0 && g!=0 {drop_edge(m,c,p,a,b,g,h)?;}i=i.wrapping_add(1);}
        }else{
            let mut i=0;while i<w(m,p,0x84)? {
                let e=row(w(m,p,0x80)?,i,24);
                if w(m,e,0)?!=0 && w(m,e,4)?!=0 {
                    let next=w(m,e,16)?;
                    if next!=0 {let mut j=0;while j<w(m,next,0x84)? {let e=row(w(m,p,0x80)?,i,24);let other=row(w(m,next,0x80)?,j,24);if w(m,other,0)?&0x7fffffff==w(m,e,0)?&0x7fffffff && w(m,other,4)?&0x7fffffff==w(m,e,4)?&0x7fffffff {m.write(other.wrapping_add(20),0)?;}j=j.wrapping_add(1);}}
                }i=i.wrapping_add(1);
            }
        }clear_slots(m,manager,p,h)?;
    }forget(m,c,manager,p)
}
///Source484b78 base virtual28; derived dispatch must select it explicitly.
pub fn base_toggle(m:&mut Memory,c:Context,p:u32,actor:u32,h:&mut impl Host)->Result<()> {
    let mark=|m:&mut Memory,owner:u32,h:&mut dyn FnMut(&mut Memory,u32)->Result<u32>|->Result<()>{
        let service=ai_actor_queries::find_in_memory(m,owner,c.world.pathfinder_class,&mut |m,s|h(m,s))?;
        if service!=0{m.write_byte(service.wrapping_add(0x48),1)?;m.write_byte(service.wrapping_add(0xf5),1)?;}Ok(())
    };
    if actor!=0 {let owner=w(m,actor,24)?;return mark(m,owner,&mut |m,s|h.class18(m,s))}
    let count=w(m,m.word(c.world.root_slot)?,20)?;
    for i in 0..count {
        let actor=w(m,w(m,w(m,p,0x90)?,20)?,i.wrapping_mul(4))?;
        let usage=w(m,p,0x8c)?;
        let used=if exclusion::bit(m,usage,i)?&255!=0{true}else{let bits=w(m,p,0x88)?;exclusion::bit(m,bits,i)?&255!=0};
        if used && actor!=0 {let owner=w(m,actor,24)?;if owner!=0{mark(m,owner,&mut |m,s|h.class18(m,s))?;}}
    }Ok(())
}
pub struct Native<H>{pub context:Context,pub added:u32,pub removed:u32,pub callbacks:H}
impl<H:Host+admission::Host> admission::Host for Native<H>{
    fn actor_slot(&mut self,m:&Memory,p:u32)->Result<u32>{self.callbacks.actor_slot(m,p)}
    fn bind_actor(&mut self,m:&mut Memory,p:u32,slot:u32,world:u32)->Result<()>{self.callbacks.bind_actor(m,p,slot,world)}
    fn callback(&mut self,m:&mut Memory,w:u32,p:u32,f:u32,data:u32)->Result<()>{
        if f==self.added{added(m,self.context,p,data)}else if f==self.removed{removed(m,self.context,p,data,&mut self.callbacks)}else{self.callbacks.callback(m,w,p,f,data)}
    }
    fn inserted24(&mut self,m:&mut Memory,p:u32)->Result<()>{self.callbacks.inserted24(m,p)}
}
impl<H:Host+admission::RemovalHost> admission::RemovalHost for Native<H>{
    fn first_dependency(&mut self,m:&Memory,p:u32)->Result<Option<u32>>{self.callbacks.first_dependency(m,p)}
    fn detach14(&mut self,m:&mut Memory,child:u32,p:u32)->Result<()>{self.callbacks.detach14(m,child,p)}
    fn removed20(&mut self,m:&mut Memory,p:u32)->Result<()>{self.callbacks.removed20(m,p)}
}
impl<H:Host+admission::Services> exclusion::Services for Native<H>{fn class20(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.callbacks.class20(m,p)}}
impl<H:Host+admission::Services> release::Services for Native<H>{fn class18(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.callbacks.class18(m,p)}}
impl<H:Host+admission::Services> admission::Services for Native<H>{
    fn initialize48(&mut self,m:&mut Memory,p:u32)->Result<()>{self.callbacks.initialize48(m,p)}
    fn marker_slot(&mut self,m:&mut Memory)->Result<u32>{self.callbacks.marker_slot(m)}
    fn bind50(&mut self,m:&mut Memory,p:u32,a:&super::ai_traversal_registration::Endpoint,b:&super::ai_traversal_registration::Endpoint)->Result<()>{self.callbacks.bind50(m,p,a,b)}
    fn kind(&mut self,m:&mut Memory,p:u32)->Result<u32>{self.callbacks.kind(m,p)}
}
#[cfg(test)] mod tests{
    use super::*;
    use serde_json::{Value,json};
    const P:u32=0x73000000;const MAN:u32=0x74000000;const OTHER:u32=0x74100000;const ACTOR:u32=0x7a100000;const OWNER:u32=0x7a200000;const SERVICE:u32=0x7a400000;const CLASS:u32=0x77f00000;const POOLS:u32=0x6f000000;const TABLES:u32=0x6f001000;const EDGE:u32=0x6f100000;const NEXT:u32=0x6f200000;const NEXT_EDGE:u32=0x6f300000;const INACTIVE:u32=0x6f400000;const PATH:u32=0x6f500000;const NODES:u32=0x6f600000;const CHILD:u32=0x6f700000;const EXTRA:u32=0x6f800000;const OTHER_P:u32=0x73100000;const G:u32=0x61000000;
    fn context()->Context{Context{world:release::World{root_slot:0x5a108,manager_class:0x6af44,pathfinder_class:0x6af6c},path_object_class:0x672c4,edge_object_class:0x67314}}
    fn n(v:&Value)->u32{v.as_u64().unwrap() as u32}
    fn region(m:&mut Memory,p:u32,n:usize){assert!(m.regions.insert(p,vec![0;n]).is_none());}
    fn kind(c:&Value)->u32{match c["kind"].as_str().unwrap(){"path"=>0x672c4,"edge"=>0x67314,_=>CLASS}}
    fn setup(c:&Value)->Result<Memory>{
        let mut m=super::super::ai_traversal_policy_release::tests::setup(c)?;
        for(p,len)in[(OTHER_P,160),(POOLS,24),(TABLES,20),(EDGE,72),(NEXT,160),(NEXT_EDGE,48),(INACTIVE,160),(PATH,448),(NODES,60),(CHILD,32),(0x672c4,32),(0x67314,32)]{region(&mut m,p,len);}
        m.write(0x7c100008,8)?;m.write(0x7c100018,8)?;
        m.write(P+24,if c["brain"].as_bool().unwrap(){OWNER}else{0})?;m.write(P+0x78,77)?;m.write_byte(P+0x7c,n(&c["active"]) as u8)?;m.write(P+0x80,EDGE)?;m.write(P+0x84,n(&c["edge_count"]))?;m.write(0x67314+20,0x672c4)?;m.write(CHILD+20,kind(c))?;
        for(i,p)in[OTHER_P,NEXT,INACTIVE].into_iter().enumerate(){let base=EXTRA+(i as u32+1)*256;region(&mut m,base,64);m.write(p+0x78,if p==OTHER_P{77}else{88})?;m.write_byte(p+0x7c,if p==INACTIVE{0}else if p==NEXT{2}else{1})?;m.write(p+0x8c,base)?;m.write(p+0x88,base+12)?;m.write(base,base+32)?;m.write(base+8,8)?;m.write(base+12,base+48)?;m.write(base+20,8)?;m.write_byte(base+32,0xe0)?;m.write_byte(base+48,0xe0)?;m.write(p+0x80,NEXT_EDGE)?;m.write(p+0x84,2)?;}
        for(off,v)in[(8,POOLS),(12,2)]{m.write(MAN+off,v)?;}
        for(off,v)in[(0,TABLES),(4,4),(12,TABLES+16),(16,1)]{m.write(POOLS+off,v)?;}
        for(i,p)in[P,OTHER_P,0,NEXT,INACTIVE].iter().enumerate(){m.write(TABLES+i as u32*4,*p)?;}
        for(i,p)in c["slots"].as_array().unwrap().iter().enumerate(){m.write(0x7a000000+i as u32*4,n(p))?;}
        for(i,(a,b,g))in[(n(&c["first"]),0x800000ca,G),(101,202,0),(0,202,G)].into_iter().enumerate(){for(off,v)in[(0,a),(4,b),(8,g),(12,1),(16,if c["linked"].as_bool().unwrap(){NEXT}else{0}),(20,0)]{m.write(EDGE+i as u32*24+off,v)?;}}
        for(i,(a,b))in[(101,202),(102,202)].into_iter().enumerate(){for(off,v)in[(0,a),(4,b),(8,G+4096),(12,2),(16,0),(20,P)]{m.write(NEXT_EDGE+i as u32*24+off,v)?;}}
        m.write(SERVICE+0x60,PATH)?;m.write(PATH+0x1bc,NODES)?;m.write(PATH+0x1b8,NODES+60)?;
        for(i,id)in[0x80000065,202,303].iter().enumerate(){m.write(NODES+i as u32*20,*id)?;m.write(NODES+i as u32*20+16,P)?;}
        if c["op"]=="world_remove" {
            for(p,len)in[(0x69000040,48),(0x6fa00000,28),(0x6fb00000,40),(0x6fc00000,4),(0x6fc00010,4),(0x6fd00000,28),(0x6fe00000,40),(0x6ff00000,32),(0x6ff01000,32)]{region(&mut m,p,len);}
            for(off,v)in[(24,0x6fa00000),(96,1),(104,0x6fc00000),(108,0x6fc00010)]{m.write(0x69000000+off,v)?;}
            m.write(0x6fc00000,0x486a40)?;m.write(0x6fc00010,MAN)?;m.write(P+20,if c["world_bound"].as_bool().unwrap(){0x6fb00000}else{0})?;m.write(P+40,0x69000000)?;m.write(P+44,0x6fd00000)?;
            for(off,v)in[(4,0x6fb00000),(8,0x6fb00000),(12,0x6fb00000),(16,0x6fb00014),(20,1),(24,2)]{m.write(0x6fa00000+off,v)?;}
            m.write(0x6fb00004,P)?;m.write_byte(0x6fb00010,1)?;let children=n(&c["world_children"]);m.write(0x6fd00008,if children>0{0x6fe00000}else{0})?;
            for i in 0..2 {m.write(0x6fe00004+i*20,0x6ff00000+i*4096)?;m.write(0x6fe0000c+i*20,if i+1<children{0x6fe00000+(i+1)*20}else{0})?;}
        }
        Ok(m)
    }
    struct Spy<'a>{c:&'a Value,trace:Vec<Value>}
    impl exclusion::Services for Spy<'_>{fn class20(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["class20",p]));Ok(if p==OTHER{CLASS}else{0x6af44})}}
    impl release::Services for Spy<'_>{fn class18(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["class18",p]));Ok(if p==P{if self.c["derived"].as_bool().unwrap(){CHILD}else{kind(self.c)}}else if ((p-SERVICE)%0x1000)/256==0{CLASS}else{0x6af6c})}}
    impl Host for Spy<'_>{
        fn unbind_graph(&mut self,m:&mut Memory,g:u32,a:u32,b:u32,tag:u32)->Result<()>{self.trace.push(json!(["unbind",g,a,b,tag]));if self.c["unbind_mutate"].as_bool().unwrap(){m.write(EDGE+16,OTHER_P)?;}Ok(())}
        fn toggle28(&mut self,m:&mut Memory,p:u32,a:u32)->Result<()>{self.trace.push(json!(["toggle28",p,a]));base_toggle(m,context(),p,a,self)}
    }
    impl admission::Host for Spy<'_>{
        fn callback(&mut self,_:&mut Memory,_:u32,_:u32,_:u32,_:u32)->Result<()>{anyhow::bail!("Unknown traversal World callback")}
        fn inserted24(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected actor24")}
    }
    impl admission::RemovalHost for Spy<'_>{
        fn detach14(&mut self,m:&mut Memory,child:u32,p:u32)->Result<()>{let list=w(m,p,44)?;let node=w(m,list,8)?;assert_eq!(w(m,node,4)?,child);self.trace.push(json!(["detach14",child,p,node]));let next=w(m,node,12)?;m.write(list+8,next)}
        fn removed20(&mut self,m:&mut Memory,p:u32)->Result<()>{self.trace.push(json!(["removed20",p,w(m,p,20)?,w(m,p,40)?,m.word(0x6fa00014)?]));Ok(())}
    }
    impl admission::Services for Spy<'_>{
        fn initialize48(&mut self,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected activation during unregistration")}
        fn marker_slot(&mut self,_:&mut Memory)->Result<u32>{anyhow::bail!("Unexpected marker during unregistration")}
        fn bind50(&mut self,_:&mut Memory,_:u32,_:&super::super::ai_traversal_registration::Endpoint,_:&super::super::ai_traversal_registration::Endpoint)->Result<()>{anyhow::bail!("Unexpected binding during unregistration")}
        fn kind(&mut self,_:&mut Memory,_:u32)->Result<u32>{anyhow::bail!("Unexpected graph kind during unregistration")}
    }
    fn snapshot(m:&Memory,c:&Value)->Result<Value>{
        let bytes=|p:u32,n:u32|->Result<Vec<u8>>{(0..n).map(|i|m.byte(p+i)).collect()};
        let actors=(0..3).map(|i|m.word(0x7a000000+i*4)).collect::<Result<Vec<_>>>()?;
        let masks=[P,OTHER_P,NEXT,INACTIVE].into_iter().flat_map(|p|[p+0x8c,p+0x88]).map(|p|bytes(m.word(m.word(p)?)?,8)).collect::<Result<Vec<_>>>()?;
        let refs=(0..2).map(|g|(0..3).map(|i|m.word(0x79000000+g*0x1000+i*4)).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;
        let states=(0..2).map(|g|bytes(0x78000000+g*0x1000,36)).collect::<Result<Vec<_>>>()?;
        let flags=(0..3).map(|i|Ok([m.word(SERVICE+i*0x1000+256+0x48)?,m.word(SERVICE+i*0x1000+256+0xf4)?])).collect::<Result<Vec<_>>>()?;
        let world=if c["op"]=="world_remove"{json!({"owner":[m.word(P+20)?,m.word(P+40)?],"pool":bytes(0x6fa00000,28)?,"nodes":bytes(0x6fb00000,40)?,"dependents":bytes(0x6fd00000,28)?})}else{Value::Null};
        Ok(json!({"actors":actors,"cache":m.word(P+0x90)?,"masks":masks,"refs":refs,"states":states,"edges":bytes(EDGE,72)?,"next_edges":bytes(NEXT_EDGE,48)?,"nodes":bytes(NODES,60)?,"flags":flags,"world":world}))
    }
    #[test] fn original_world_callbacks_cleanup_and_cached_paths_match()->Result<()> {
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_world_callbacks.json"))?;
        for case in f["cases"].as_array().unwrap(){let c=&case["input"];let mut m=setup(c)?;let mut native=Native{context:context(),added:0x4869d4,removed:0x486a40,callbacks:Spy{c,trace:vec![]}};
            let result=match c["op"].as_str().unwrap(){
                "added"=>{admission::Host::callback(&mut native,&mut m,0x69000000,P,0x4869d4,MAN)?;None},
                "removed"=>{admission::Host::callback(&mut native,&mut m,0x69000000,P,0x486a40,MAN)?;None},
                "track"=>{track(&mut m,context(),MAN,P)?;None},"forget"=>{forget(&mut m,context(),MAN,P)?;None},
                "clear"=>{clear_slots(&mut m,MAN,P,&mut native.callbacks)?;None},
                "drop"=>Some(drop_edge(&mut m,context(),P,101,202,G,&mut native.callbacks)?),
                "toggle"=>{base_toggle(&mut m,context(),P,n(&c["actor_argument"]),&mut native.callbacks)?;None},
                "world_remove"=>{let mut world=admission::Native{world:context().world,callbacks:native};let result=world.unregister_actor(&mut m,P)?;native=world.callbacks;Some(result)},
                _=>unreachable!()
            };
            assert_eq!(json!(result),case["result"],"result: {c}");assert_eq!(json!(native.callbacks.trace),case["trace"],"trace: {c}");assert_eq!(snapshot(&m,c)?,case["output"],"state: {c}");
        }Ok(())
    }
}
