//! Profile462e88/463054/463180 and base actor45fe8c/45fee4 lifecycle.
//! Actors stay in factory Storage. Profile/manager pools have one canonical
//! graph-memory owner; derived brain and manager-service virtuals are required.
use super::{ai_graph_link_passes::Memory, ai_navigation_owned_factory::Storage,
    ai_navigation_world_owned, ai_spatial_construction};
use anyhow::{Context, Result};
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn entity(actors:&Storage,actor:usize)->Result<&super::ai_navigation_entity_base::Entity>{
    Ok(&actors.entities.get(&actor).context("Profile actor is not owned")?.entity)
}
fn entity_mut(actors:&mut Storage,actor:usize)->Result<&mut super::ai_navigation_entity_base::Entity>{
    Ok(&mut actors.entities.get_mut(&actor).context("Profile actor is not owned")?.entity)
}
pub trait Services {
    fn brain24(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize)->Result<()>;
    fn brain28(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize)->Result<()>;
    fn metric28(&mut self,m:&mut Memory,actors:&mut Storage,service:u32)->Result<u32>;
    fn inserted30(&mut self,m:&mut Memory,actors:&mut Storage,service:u32,actor:usize)->Result<()>;
    fn removed34(&mut self,m:&mut Memory,actors:&mut Storage,service:u32,actor:usize)->Result<()>;
}
pub fn recompute(m:&mut Memory,actors:&mut Storage,manager:u32,h:&mut impl Services)->Result<()>{
    m.write(manager.wrapping_add(0x84),0)?;
    let mut i=0u32;
    while i<w(m,manager,0x7c)? {
        let service=w(m,w(m,manager,0x70)?,i.wrapping_mul(4))?;
        let amount=h.metric28(m,actors,service)?;
        m.write(manager.wrapping_add(0x84),w(m,manager,0x84)?.wrapping_add(amount))?;
        i=i.wrapping_add(1);
    }Ok(())
}
pub fn detach(m:&mut Memory,actors:&mut Storage,profile:u32,actor:usize,h:&mut impl Services)->Result<()>{
    let e=entity(actors,actor)?;
    if e.common.word_28==0 || e.profile_node_70.is_none(){return Ok(())}
    let manager=w(m,profile,4)?;
    let mut i=0u32;
    while i<w(m,manager,0x7c)? {
        let service=w(m,w(m,manager,0x70)?,i.wrapping_mul(4))?;
        if service!=0{h.removed34(m,actors,service,actor)?;}
        i=i.wrapping_add(1);
    }
    // Both are reloaded after callbacks. A callback may replace the pool and
    // the actor's node; do not unlink a captured node from the former pool.
    let pool=w(m,profile,8)?;
    let node=u32::try_from(entity(actors,actor)?.profile_node_70.context("Profile detach callback cleared its node")?)?;
    ai_spatial_construction::unlink(m,pool,node)?;
    entity_mut(actors,actor)?.profile_node_70=None;
    // Native detach retains profile68, even after clearing its membership70.
    let manager=w(m,profile,4)?;
    recompute(m,actors,manager,h)
}
pub fn insert(m:&mut Memory,actors:&mut Storage,profile:u32,actor:usize,h:&mut impl Services)->Result<Option<usize>>{
    if let Some(previous)=entity(actors,actor)?.profile_68 {
        detach(m,actors,u32::try_from(previous)?,actor,h)?;
    }
    entity_mut(actors,actor)?.profile_68=None;
    let pool=w(m,profile,8)?;
    //462ecc compares equality, not >=. Preserve malformed/native boundary
    //behavior and the no-free-node branch independently of the count limit.
    if w(m,pool,0x14)?==w(m,pool,0x18)? {
        let manager=w(m,profile,4)?;
        recompute(m,actors,manager,h)?;return Ok(None)
    }
    let node=w(m,pool,0x10)?;
    ai_spatial_construction::append(m,pool,u32::try_from(actor)?)?;
    let e=entity_mut(actors,actor)?;
    e.profile_node_70=(node!=0).then_some(node as usize);
    e.profile_68=Some(profile as usize);
    let manager=w(m,profile,4)?;
    recompute(m,actors,manager,h)?;
    let manager=w(m,profile,4)?;
    let mut i=0u32;
    while i<w(m,manager,0x7c)? {
        let service=w(m,w(m,manager,0x70)?,i.wrapping_mul(4))?;
        h.inserted30(m,actors,service,actor)?;i=i.wrapping_add(1);
    }
    Ok(entity(actors,actor)?.profile_node_70)
}
pub fn inserted24(m:&mut Memory,actors:&mut Storage,actor:usize,serial_slot:u32,h:&mut impl Services)->Result<()>{
    let serial=m.word(serial_slot)?;
    entity_mut(actors,actor)?.word_5c=serial as i32;
    m.write(serial_slot,serial.wrapping_add(1))?;
    if let Some(brain)=entity(actors,actor)?.common.brain{h.brain28(m,actors,brain)?;}
    if let Some(profile)=entity(actors,actor)?.profile_68{insert(m,actors,u32::try_from(profile)?,actor,h)?;}
    Ok(())
}
pub fn removed20(m:&mut Memory,actors:&mut Storage,actor:usize,h:&mut impl Services)->Result<()>{
    if let Some(brain)=entity(actors,actor)?.common.brain{h.brain24(m,actors,brain)?;}
    if let Some(profile)=entity(actors,actor)?.profile_68{detach(m,actors,u32::try_from(profile)?,actor,h)?;}
    Ok(())
}
pub trait Host:Services {
    fn world_callback(&mut self,m:&mut Memory,actors:&mut Storage,world:u32,actor:usize,function:u32,data:u32)->Result<()>;
    fn dependency_detach14(&mut self,m:&mut Memory,actors:&mut Storage,child:usize,actor:usize)->Result<()>;
}
/// Select only for the verified JPSBaseAIEntity base20/24 dispatch. Other
/// entity classes must supply their own concrete lifecycle virtuals.
pub struct Native<H>{pub serial_slot:u32,pub callbacks:H}
impl<H:Host> ai_navigation_world_owned::Callbacks for Native<H>{
    fn callback(&mut self,m:&mut Memory,a:&mut Storage,w:u32,p:usize,f:u32,d:u32)->Result<()>{self.callbacks.world_callback(m,a,w,p,f,d)}
    fn inserted24(&mut self,m:&mut Memory,a:&mut Storage,p:usize)->Result<()>{inserted24(m,a,p,self.serial_slot,&mut self.callbacks)}
    fn detach14(&mut self,m:&mut Memory,a:&mut Storage,c:usize,p:usize)->Result<()>{self.callbacks.dependency_detach14(m,a,c,p)}
    fn removed20(&mut self,m:&mut Memory,a:&mut Storage,p:usize)->Result<()>{removed20(m,a,p,&mut self.callbacks)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json,Value};
    const P:u32=0x60000000;const A:u32=0x61000000;const POOL:u32=0x62000000;
    const NODES:u32=0x63000000;const M:u32=0x64000000;const TABLE:u32=0x65000000;
    const S:u32=0x66000000;const OUT:u32=0x69000000;const SERIAL:u32=0x51ef4;
    const WORLD:u32=0x6d000000;
    fn link(value:u32)->Option<usize>{(value!=0).then_some(value as usize)}
    struct Spy<'a>{config:&'a Value,trace:Vec<Value>,fired:bool}
    impl Spy<'_>{
        fn call(&mut self,m:&mut Memory,actors:&mut Storage,kind:&str,service:usize,actor:usize)->Result<()>{
            let e=entity(actors,A as usize)?;
            self.trace.push(json!([kind,service,actor,e.word_5c as u32,e.profile_68.unwrap_or(0),
                e.profile_node_70.unwrap_or(0),w(m,M,0x84)?]));
            let mutation=self.config["mutation"].as_str().unwrap();
            let when=match mutation {"brain_profile"|"brain_clear"=>"brain","clear_node"=>"insert30","swap"|"null_remove"=>"remove34",_=>"metric"};
            if !self.fired && (when==kind || when=="brain" && kind.starts_with("brain")) {
                self.fired=true;
                match mutation {
                    "total"=>m.write(M+0x84,0xfffffff0)?,"grow"=>m.write(M+0x7c,3)?,"shrink"=>m.write(M+0x7c,0)?,
                    "arrays"=>m.write(M+0x70,TABLE+4096)?,"manager"=>m.write(P+4,M+4096)?,
                    "clear_node"=>entity_mut(actors,A as usize)?.profile_node_70=None,
                    "brain_profile"=>entity_mut(actors,A as usize)?.profile_68=Some((P+4096) as usize),
                    "brain_clear"=>entity_mut(actors,A as usize)?.profile_68=None,
                    "swap"=>{m.write(P+8,POOL+4096)?;entity_mut(actors,A as usize)?.profile_node_70=Some((NODES+4096+20) as usize);},
                    "null_remove"=>m.write(TABLE+4,0)?,_=>{}
                }
            }else if mutation=="null_remove" && kind=="remove34" && service==(S+512) as usize{m.write(TABLE+4,S+256)?;}
            Ok(())
        }
    }
    impl Services for Spy<'_>{
        fn brain24(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{self.call(m,a,"brain24",b,A as usize)}
        fn brain28(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{self.call(m,a,"brain28",b,A as usize)}
        fn metric28(&mut self,m:&mut Memory,a:&mut Storage,s:u32)->Result<u32>{self.call(m,a,"metric",s as usize,A as usize)?;w(m,s,4)}
        fn inserted30(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.call(m,a,"insert30",s as usize,p)}
        fn removed34(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.call(m,a,"remove34",s as usize,p)}
    }
    impl Host for Spy<'_>{
        fn world_callback(&mut self,_:&mut Memory,_:&mut Storage,_:u32,_:usize,_:u32,_:u32)->Result<()>{anyhow::bail!("Fixture has no World callbacks")}
        fn dependency_detach14(&mut self,_:&mut Memory,_:&mut Storage,_:usize,_:usize)->Result<()>{anyhow::bail!("Fixture has no entity dependencies")}
    }
    #[test]
    fn actual_profile_and_actor_lifecycle_match_original_without_actor_memory_copies()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_profile_lifecycle.json"))?;
        for case in fixture["cases"].as_array().unwrap(){
            let c=&case["input"]["config"];let op=c["op"].as_str().unwrap();
            let mut m:Memory=serde_json::from_value(case["input"]["memory"].clone())?;
            let mut expected:Memory=serde_json::from_value(case["memory"].clone())?;
            let mut actors=Storage::default();
            let mut actor_expected=Vec::new();
            for identity in [A,A+4096]{
                let mut actor=super::super::ai_navigation_world_owned::tests::constructed_actor(identity as usize)?;
                let e=&mut actor.entity;
                e.common.word_14=w(&m,identity,0x14)?;e.common.word_28=w(&m,identity,0x28)?;
                e.common.brain=link(w(&m,identity,0x18)?);e.word_5c=w(&m,identity,0x5c)? as i32;
                e.profile_68=link(w(&m,identity,0x68)?);e.profile_node_70=link(w(&m,identity,0x70)?);
                actor_expected.push((identity,[w(&expected,identity,0x14)?,w(&expected,identity,0x18)?,w(&expected,identity,0x28)?,
                    w(&expected,identity,0x5c)?,w(&expected,identity,0x68)?,w(&expected,identity,0x70)?]));
                actors.entities.insert(identity as usize,actor);
            }
            for address in [A,A+4096,OUT]{m.regions.remove(&address);expected.regions.remove(&address);}
            if op=="world_roundtrip"{m.regions.remove(&(WORLD+12288));expected.regions.remove(&(WORLD+12288));}
            let mut native=Native{serial_slot:SERIAL,callbacks:Spy{config:c,trace:vec![],fired:false}};
            let result=match op {
                "insert"=>json!(insert(&mut m,&mut actors,P,A as usize,&mut native.callbacks)?.unwrap_or(0)),
                "detach"=>{detach(&mut m,&mut actors,P,A as usize,&mut native.callbacks)?;Value::Null},
                "recompute"=>{recompute(&mut m,&mut actors,M,&mut native.callbacks)?;Value::Null},
                "actor_insert"=>{inserted24(&mut m,&mut actors,A as usize,SERIAL,&mut native.callbacks)?;Value::Null},
                "actor_remove"=>{removed20(&mut m,&mut actors,A as usize,&mut native.callbacks)?;Value::Null},
                "world_roundtrip"=>{
                    let mut binding=ai_navigation_world_owned::Binding{actors:&mut actors,callbacks:&mut native};
                    let registered=binding.register(&mut m,WORLD,A as usize)?;
                    let removed=binding.unregister(&mut m,WORLD,A as usize)?;json!([registered,removed])
                },_=>anyhow::bail!("Unknown reference operation {op}")
            };
            assert_eq!(result,case["result"],"result {c}");
            assert_eq!(json!(native.callbacks.trace),case["trace"],"callbacks {c}");
            assert_eq!(serde_json::to_value(&m)?,serde_json::to_value(&expected)?,"canonical pools/manager {c}");
            for (identity,expected) in actor_expected {
                let e=entity(&actors,identity as usize)?;
                assert_eq!([e.common.word_14,e.common.brain.unwrap_or(0) as u32,e.common.word_28,e.word_5c as u32,
                    e.profile_68.unwrap_or(0) as u32,e.profile_node_70.unwrap_or(0) as u32],expected,"owned actor {c}");
            }
        }Ok(())
    }
}
