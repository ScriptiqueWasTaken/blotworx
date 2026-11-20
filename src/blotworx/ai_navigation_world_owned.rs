//! Base World46b37c/46b520 admission against the factory's canonical actors.
//! Factory pool+c, World slot14/owner28 and construction context30 are distinct.
//! Derived lifecycle, profile and traversal callbacks remain mandatory services.
use super::{ai_graph_link_passes::Memory, ai_navigation_owned_factory::Storage,
    ai_navigation_world_registration::{self as world, Host, RemovalHost}};
use anyhow::{Context, Result};

pub trait Callbacks {
    fn callback(&mut self,m:&mut Memory,actors:&mut Storage,world:u32,actor:usize,function:u32,data:u32)->Result<()>;
    fn inserted24(&mut self,m:&mut Memory,actors:&mut Storage,actor:usize)->Result<()>;
    fn detach14(&mut self,m:&mut Memory,actors:&mut Storage,child:usize,actor:usize)->Result<()>;
    fn removed20(&mut self,m:&mut Memory,actors:&mut Storage,actor:usize)->Result<()>;
}
pub struct Binding<'a,H> {
    pub actors: &'a mut Storage,
    pub callbacks: &'a mut H,
}
impl<H:Callbacks> Binding<'_,H> {
    pub fn register(&mut self,m:&mut Memory,owner:u32,actor:usize)->Result<u32>{
        self.actors.entities.get(&actor).context("World admission actor is not owned")?;
        world::register(m,owner,u32::try_from(actor)?,self)
    }
    pub fn unregister(&mut self,m:&mut Memory,owner:u32,actor:usize)->Result<u32>{
        self.actors.entities.get(&actor).context("World removal actor is not owned")?;
        world::unregister(m,owner,u32::try_from(actor)?,self)
    }
}
impl<H:Callbacks> Host for Binding<'_,H> {
    fn actor_slot(&mut self,_:&Memory,actor:u32)->Result<u32>{
        Ok(self.actors.entities.get(&(actor as usize)).context("World actor is not owned")?.entity.common.word_14)
    }
    fn bind_actor(&mut self,_:&mut Memory,actor:u32,slot:u32,owner:u32)->Result<()>{
        let common=&mut self.actors.entities.get_mut(&(actor as usize)).context("World actor is not owned")?.entity.common;
        common.word_14=slot;common.word_28=owner;Ok(())
    }
    fn callback(&mut self,m:&mut Memory,owner:u32,actor:u32,function:u32,data:u32)->Result<()>{
        self.callbacks.callback(m,self.actors,owner,actor as usize,function,data)
    }
    fn inserted24(&mut self,m:&mut Memory,actor:u32)->Result<()>{
        self.callbacks.inserted24(m,self.actors,actor as usize)
    }
}
impl<H:Callbacks> RemovalHost for Binding<'_,H> {
    fn first_dependency(&mut self,_:&Memory,actor:u32)->Result<Option<u32>>{
        let pool=&self.actors.entities.get(&(actor as usize)).context("World actor is not owned")?.entity.pool;
        pool.head.map(|head| -> Result<_> {
            let node=pool.nodes.get(head).context("Actor dependency head is out of range")?;
            Ok(u32::try_from(node.child.context("Active dependency has no child identity")?)?)
        }).transpose()
    }
    fn detach14(&mut self,m:&mut Memory,child:u32,actor:u32)->Result<()>{
        self.callbacks.detach14(m,self.actors,child as usize,actor as usize)
    }
    fn removed20(&mut self,m:&mut Memory,actor:u32)->Result<()>{
        self.callbacks.removed20(m,self.actors,actor as usize)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use super::super::{ai_navigation_entity::{self as entity,Class,Kind,Owned,Registry},
        ai_navigation_entity_base::PoolNode};
    use serde_json::{json,Value};
    use std::collections::BTreeMap;
    const W:u32=0x60000000;const A:u32=0x61000000;const A2:u32=A+4096;
    const POOL:u32=0x62000000;const FUN:u32=0x64000000;const DATA:u32=0x65000000;
    struct Shape;
    impl entity::Host for Shape {
        fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(0.3)}
        fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(1.5)}
        fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(3.)}
        fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(None)}
    }
    pub(in crate::blotworx) fn constructed_actor(identity:usize)->Result<Owned>{
        let registry=Registry{count:3,diameter:Class{index:0,constructor:Some(Kind::Diameter)},
            height:Class{index:1,constructor:Some(Kind::Height)},maximum_speed:Class{index:2,constructor:Some(Kind::MaximumSpeed)}};
        Owned::new(identity,Some(b"World actor"),[4,5],&registry,&mut Shape)
    }
    fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p+o)}
    struct Spy<'a>{config:&'a Value,trace:Vec<Value>,calls:u32,removing:bool,
        nodes:BTreeMap<(usize,usize),u32>}
    impl Callbacks for Spy<'_> {
        fn callback(&mut self,m:&mut Memory,actors:&mut Storage,owner:u32,actor:usize,function:u32,data:u32)->Result<()>{
            assert_eq!(owner,W);
            let common=&actors.entities[&actor].entity.common;
            self.trace.push(json!(["callback",function,data,actor,common.word_14,common.word_28,word(m,POOL,20)?]));
            self.calls+=1;
            if self.calls==1 {
                let count=if self.removing{96}else{80};let functions=if self.removing{104}else{88};let data=if self.removing{108}else{92};
                match self.config["mutation"].as_str().unwrap(){
                    "count"=>m.write(W+count,0)?,"grow"=>m.write(W+count,3)?,
                    "arrays"=>{m.write(W+functions,FUN+4096)?;m.write(W+data,DATA+4096)?;},
                    "actor"=>actors.entities.get_mut(&(A as usize)).unwrap().entity.common.word_14=0x77777777,
                    "pool"=>m.write(POOL+20,u32::MAX)?,
                    "nested"=>{let result=Binding{actors,callbacks:self}.register(m,W,A2 as usize)?;self.trace.push(json!(["nested",result]));},
                    _=>{}
                }
            }Ok(())
        }
        fn inserted24(&mut self,m:&mut Memory,actors:&mut Storage,actor:usize)->Result<()>{
            let c=&actors.entities[&actor].entity.common;
            self.trace.push(json!(["inserted",actor,c.word_14,c.word_28,word(m,POOL,20)?]));Ok(())
        }
        fn detach14(&mut self,_:&mut Memory,actors:&mut Storage,child:usize,actor:usize)->Result<()>{
            let pool=&mut actors.entities.get_mut(&actor).unwrap().entity.pool;
            let head=pool.head.context("Dependency callback lost its head")?;
            assert_eq!(pool.nodes[head].child,Some(child));
            self.trace.push(json!(["detach14",child,actor,self.nodes[&(actor,head)]]));
            pool.head=pool.nodes[head].next;Ok(())
        }
        fn removed20(&mut self,m:&mut Memory,actors:&mut Storage,actor:usize)->Result<()>{
            let c=&mut actors.entities.get_mut(&actor).unwrap().entity.common;
            self.trace.push(json!(["removed20",actor,c.word_14,c.word_28,word(m,POOL,20)?]));
            if self.config["swap"].as_bool().unwrap_or(false){m.write(W+24,POOL+4096)?;c.word_14=0x63000000+4096+20;}
            Ok(())
        }
    }
    #[test]
    fn canonical_owned_actors_match_original_world_admission_without_raw_actor_copies()->Result<()>{
        let registry=Registry{count:3,diameter:Class{index:0,constructor:Some(Kind::Diameter)},
            height:Class{index:1,constructor:Some(Kind::Height)},maximum_speed:Class{index:2,constructor:Some(Kind::MaximumSpeed)}};
        for (fixture,removal) in [
            (include_str!("../../tools/blotworx/fixtures/ai_navigation_world_registration.json"),false),
            (include_str!("../../tools/blotworx/fixtures/ai_navigation_world_unregistration.json"),true)] {
            let fixture:Value=serde_json::from_str(fixture)?;
            for case in fixture["cases"].as_array().unwrap(){
                let input=&case["input"];let c=&input["config"];
                let mut m:Memory=serde_json::from_value(input["memory"].clone())?;
                let mut expected:Memory=serde_json::from_value(case["memory"].clone())?;
                let mut actors=Storage::default();let mut nodes=BTreeMap::new();let mut lists=BTreeMap::new();let mut remove=vec![A,A2];
                for actor in [A,A2] {
                    let mut owned=Owned::new(actor as usize,Some(b"World actor"),[4,5],&registry,&mut Shape)?;
                    owned.entity.common.word_14=word(&m,actor,20)?;
                    owned.entity.common.word_28=word(&m,actor,40)?;
                    // These are independent native fields. Admission must not
                    // overwrite factory membership or construction context.
                    owned.entity.common.pool_node=Some(1234);owned.entity.common.world=Some(4321);
                    if removal {
                        let list=word(&m,actor,44)?;lists.insert(actor,list);remove.push(list);
                        let mut node=word(&m,list,8)?;let mut addresses=Vec::new();
                        while node!=0 {addresses.push(node);node=word(&m,node,12)?;}
                        let pool=&mut owned.entity.pool;pool.nodes.clear();
                        for (index,&node) in addresses.iter().enumerate(){
                            nodes.insert((actor as usize,index),node);
                            pool.nodes.push(PoolNode{child:Some(word(&m,node,4)? as usize),
                                previous:index.checked_sub(1),next:(index+1<addresses.len()).then_some(index+1),flag:m.byte(node+16)?});
                            let (&base,_)=m.regions.range(..=node).next_back().unwrap();remove.push(base);
                        }
                        pool.head=(!addresses.is_empty()).then_some(0);
                        pool.tail=addresses.iter().position(|&node|node==word(&m,list,12).unwrap());
                        pool.count=word(&m,list,20)?;
                    }
                    actors.entities.insert(actor as usize,owned);
                }
                remove.sort_unstable();remove.dedup();
                let expected_bindings=[A,A2].map(|actor|(actor,word(&expected,actor,20).unwrap(),word(&expected,actor,40).unwrap()));
                let expected_heads=lists.iter().map(|(&actor,&list)|Ok((actor,word(&expected,list,8)?))).collect::<Result<BTreeMap<_,_>>>()?;
                // Any accidental raw actor/dependency access now fails instead
                // of observing a stale memory image of the same owned object.
                for base in remove {m.regions.remove(&base);expected.regions.remove(&base);}
                let mut callbacks=Spy{config:c,trace:vec![],calls:0,removing:removal && c["op"]=="remove",nodes};
                let mut binding=Binding{actors:&mut actors,callbacks:&mut callbacks};
                let result=if removal {
                    if c["op"]=="roundtrip"{let result=binding.register(&mut m,W,A as usize)?;binding.callbacks.trace.push(json!(["registered",result]));binding.callbacks.removing=true;}
                    binding.unregister(&mut m,W,A as usize)?
                } else {binding.register(&mut m,W,A as usize)?};
                assert_eq!(json!(result),case["result"],"result {c}");
                assert_eq!(json!(callbacks.trace),case["trace"],"callbacks {c}");
                assert_eq!(serde_json::to_value(&m)?,serde_json::to_value(&expected)?,"non-actor memory {c}");
                for (actor,slot,owner) in expected_bindings {
                    let entity=&actors.entities[&(actor as usize)].entity;
                    assert_eq!((entity.common.word_14,entity.common.word_28),(slot,owner),"binding {c}");
                    assert_eq!((entity.common.pool_node,entity.common.world),(Some(1234),Some(4321)));
                    if removal {let head=entity.pool.head.map(|head|callbacks.nodes[&(actor as usize,head)]).unwrap_or(0);assert_eq!(head,expected_heads[&actor],"dependencies {c}");}
                }
            }
        }Ok(())
    }
}
