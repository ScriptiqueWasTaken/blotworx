//! Base brain World removal459658 and insertion459704, inherited by the
//! actual JPSBaseAIBrain vtable4ebf88. Lists stay in canonical Constructed.base
//! storage throughout callbacks. Derived service/agent virtuals are mandatory.
use super::{ai_graph_link_passes::Memory,ai_navigation_owned_factory::Storage,
    ai_navigation_profile_lifecycle as profile};
use anyhow::{Context,Result};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ChildCall {Service20,Service24,Agent28,Agent2c}
pub trait Children {
    fn child(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize,child:usize,call:ChildCall)->Result<()>;
}
/// Dispatch actual owned services. Agent classes remain required derived
/// callbacks; an unknown class must not be treated as an empty callback.
pub trait Agents {
    fn agent(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize,child:usize,inserted:bool)->Result<()>;
}
pub struct OwnedChildren<H> {
    pub callbacks:H,
    pub modifiers:std::collections::BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,
}
impl<H:Agents+super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host> Children for OwnedChildren<H> {
    fn child(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize,child:usize,call:ChildCall)->Result<()> {
        match call {
            ChildCall::Service20|ChildCall::Service24=>{
                let service=actors.services.get_mut(&child).context("World callback service is not owned")?;
                match service {
                    super::ai_navigation_service_factory::Owned::Pathfinder(state)=>{
                        anyhow::ensure!(state.identity==child && state.base.owner==brain,
                            "World callback service identity or owning brain differs");
                    }
                }
                if call==ChildCall::Service24 {service.world_inserted24();Ok(())}
                else {service.world_removed20(m,&mut self.modifiers,&mut self.callbacks)}
            }
            ChildCall::Agent28|ChildCall::Agent2c=>self.callbacks.agent(m,actors,brain,child,call==ChildCall::Agent2c),
        }
    }
}
impl<H:Agents+super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host+profile::Services> profile::Services for OwnedChildren<H> {
    fn brain24(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{transition(m,a,b,false,self)}
    fn brain28(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{transition(m,a,b,true,self)}
    fn metric28(&mut self,m:&mut Memory,a:&mut Storage,s:u32)->Result<u32>{self.callbacks.metric28(m,a,s)}
    fn inserted30(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.callbacks.inserted30(m,a,s,p)}
    fn removed34(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.callbacks.removed34(m,a,s,p)}
}
impl<H:Agents+super::ai_pathfinder_reset::Host+super::ai_goto_gap::Host+profile::Host> profile::Host for OwnedChildren<H> {
    fn world_callback(&mut self,m:&mut Memory,a:&mut Storage,w:u32,p:usize,f:u32,d:u32)->Result<()>{self.callbacks.world_callback(m,a,w,p,f,d)}
    fn dependency_detach14(&mut self,m:&mut Memory,a:&mut Storage,c:usize,p:usize)->Result<()>{self.callbacks.dependency_detach14(m,a,c,p)}
}
pub fn transition(m:&mut Memory,actors:&mut Storage,brain:usize,inserted:bool,h:&mut impl Children)->Result<()>{
    // Both native functions finish the service pass before the agent pass.
    // A callback may replace/grow/shrink either list or reenter this function.
    for services in [true,false] {
        let mut i=0usize;
        loop {
            let base=&actors.brains.get(&brain).context("World callback navigation brain is not owned")?.base;
            let list=if services{&base.services}else{&base.agents};
            i32::try_from(list.len()).context("Navigation brain list exceeds its native signed count")?;
            let Some(&child)=list.get(i) else{break};
            let call=match (services,inserted){
                (true,false)=>ChildCall::Service20,(true,true)=>ChildCall::Service24,
                (false,false)=>ChildCall::Agent28,(false,true)=>ChildCall::Agent2c,
            };
            h.child(m,actors,brain,child,call)?;
            i+=1;
        }
    }Ok(())
}
/// Compose inside profile::Native to provide actual inherited brain24/28
/// dispatch. Other profile/World operations retain their concrete backend.
pub struct Native<H>{pub callbacks:H}
impl<H:Children+profile::Services> profile::Services for Native<H>{
    fn brain24(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{transition(m,a,b,false,&mut self.callbacks)}
    fn brain28(&mut self,m:&mut Memory,a:&mut Storage,b:usize)->Result<()>{transition(m,a,b,true,&mut self.callbacks)}
    fn metric28(&mut self,m:&mut Memory,a:&mut Storage,s:u32)->Result<u32>{self.callbacks.metric28(m,a,s)}
    fn inserted30(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.callbacks.inserted30(m,a,s,p)}
    fn removed34(&mut self,m:&mut Memory,a:&mut Storage,s:u32,p:usize)->Result<()>{self.callbacks.removed34(m,a,s,p)}
}
impl<H:Children+profile::Host> profile::Host for Native<H>{
    fn world_callback(&mut self,m:&mut Memory,a:&mut Storage,w:u32,p:usize,f:u32,d:u32)->Result<()>{self.callbacks.world_callback(m,a,w,p,f,d)}
    fn dependency_detach14(&mut self,m:&mut Memory,a:&mut Storage,c:usize,p:usize)->Result<()>{self.callbacks.dependency_detach14(m,a,c,p)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_action as action,ai_navigation_construction::{Constructed,ActionClass,ActionFactory,Layout}};
    use serde_json::{json,Value};
    const A:u32=0x61000000;const B:u32=0x68000000;const P:u32=0x60000000;
    const OUT:u32=0x69000000;const WORLD:u32=0x6d000000;const SERIAL:u32=0x51ef4;
    const CS:usize=0x6e000000;const CA:usize=0x6f000000;const SV:u32=0x71000000;const AV:u32=SV+4096;
    fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p+o)}
    fn link(v:u32)->Option<usize>{(v!=0).then_some(v as usize)}
    fn list(m:&Memory,table:u32,count:u32)->Result<Vec<usize>>{
        let table=w(m,B,table)?;
        (0..w(m,B,count)?).map(|i|Ok(w(m,table,i*4)? as usize)).collect()
    }
    fn constructed_brain()->Result<Constructed>{
        Constructed::new(B as usize,Some(A as usize),Some(91),
            &ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},
            &Layout{agent_classes:3,service_classes:3,attributes:action::Registry{count:2,
                heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}}})
    }
    struct Derived;
    impl Agents for Derived {
        fn agent(&mut self,_:&mut Memory,a:&mut Storage,b:usize,c:usize,inserted:bool)->Result<()>{
            assert!(inserted);assert_eq!(c,91);a.brains.get_mut(&b).unwrap().base.agents.clear();Ok(())
        }
    }
    impl super::super::ai_pathfinder_reset::Host for Derived {
        fn detach_goal(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected goal detach")}
        fn reset_modifier(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:super::super::ai_pathfinder_modifiers::Kind,_:usize)->Result<()>{anyhow::bail!("Unexpected modifier fallback")}
        fn release(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:usize,_:u32)->Result<()>{anyhow::bail!("Unexpected graph release")}
    }
    impl super::super::ai_goto_gap::Host for Derived {
        fn unit_scale(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<Option<f32>>{anyhow::bail!("Unexpected gap construction")}
        fn owner_entity(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<usize>{anyhow::bail!("Unexpected gap construction")}
        fn profiling_enabled(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<bool>{anyhow::bail!("Unexpected gap construction")}
        fn register_budget(&mut self,_:&mut super::super::ai_goto_gap::State,_:&str)->Result<i32>{anyhow::bail!("Unexpected gap construction")}
    }
    #[test]
    fn owned_service_dispatch_preserves_single_owner_and_live_agent_lists()->Result<()>{
        let mut actors=Storage::default();let mut brain=constructed_brain()?;
        brain.base.services=vec![77];brain.base.agents=vec![91,92];actors.brains.insert(B as usize,brain);
        let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;
        state.identity=77;state.base.owner=B as usize;state.flags_9c_9d=[7,8];
        actors.services.insert(77,super::super::ai_navigation_service_factory::Owned::Pathfinder(Box::new(state)));
        let mut h=OwnedChildren{callbacks:Derived,modifiers:Default::default()};
        let mut m=Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        transition(&mut m,&mut actors,B as usize,true,&mut h)?;
        assert!(actors.brains[&(B as usize)].base.agents.is_empty());
        let super::super::ai_navigation_service_factory::Owned::Pathfinder(s)=&mut actors.services.get_mut(&77).unwrap();
        assert_eq!(s.flags_9c_9d,[7,8]);s.base.owner=99;
        assert!(transition(&mut m,&mut actors,B as usize,true,&mut h).unwrap_err().to_string().contains("owning brain"));
        actors.services.clear();
        assert!(transition(&mut m,&mut actors,B as usize,true,&mut h).unwrap_err().to_string().contains("not owned"));
        Ok(())
    }
    struct Spy<'a>{config:&'a Value,trace:Vec<Value>,fired:bool}
    impl Spy<'_>{
        fn record(&mut self,actors:&Storage,kind:&str,child:usize)->Result<()>{
            let base=&actors.brains.get(&(B as usize)).context("Trace brain disappeared")?.base;
            let e=&actors.entities[&(A as usize)].entity;
            self.trace.push(json!([kind,child,base.services.len(),base.agents.len(),e.word_5c as u32,
                e.profile_68.unwrap_or(0),e.profile_node_70.unwrap_or(0)]));Ok(())
        }
    }
    impl Children for Spy<'_>{
        fn child(&mut self,m:&mut Memory,actors:&mut Storage,brain:usize,child:usize,call:ChildCall)->Result<()>{
            assert_eq!(brain,B as usize);
            let (name,phase)=match call {ChildCall::Service20=>("service20","service"),ChildCall::Service24=>("service24","service"),
                ChildCall::Agent28=>("agent28","agent"),ChildCall::Agent2c=>("agent2c","agent")};
            self.record(actors,name,child)?;
            if !self.fired && self.config["phase"]==phase {
                self.fired=true;
                match self.config["mutation"].as_str().unwrap(){
                    "services_shrink"=>actors.brains.get_mut(&brain).unwrap().base.services.clear(),
                    "services_grow"=>actors.brains.get_mut(&brain).unwrap().base.services=vec![CS,CS+256,CS+512],
                    "service_array"=>actors.brains.get_mut(&brain).unwrap().base.services=vec![CS+512,CS+256,CS],
                    "agents_shrink"=>actors.brains.get_mut(&brain).unwrap().base.agents.clear(),
                    "agents_grow"=>actors.brains.get_mut(&brain).unwrap().base.agents=vec![CA,CA+256,CA+512],
                    "agent_array"=>actors.brains.get_mut(&brain).unwrap().base.agents=vec![CA+256,CA+512,CA],
                    "profile_clear"=>actors.entities.get_mut(&(A as usize)).unwrap().entity.profile_68=None,
                    "profile_swap"=>actors.entities.get_mut(&(A as usize)).unwrap().entity.profile_68=Some((P+4096) as usize),
                    "nested"=>{
                        let inserted=matches!(call,ChildCall::Service20|ChildCall::Agent28);
                        transition(m,actors,brain,inserted,self)?;self.trace.push(json!(["nested",inserted]));
                    },_=>{}
                }
            }Ok(())
        }
    }
    impl profile::Services for Spy<'_>{
        fn brain24(&mut self,_:&mut Memory,_:&mut Storage,_:usize)->Result<()>{anyhow::bail!("Inherited brain removal was bypassed")}
        fn brain28(&mut self,_:&mut Memory,_:&mut Storage,_:usize)->Result<()>{anyhow::bail!("Inherited brain insertion was bypassed")}
        fn metric28(&mut self,m:&mut Memory,a:&mut Storage,s:u32)->Result<u32>{self.record(a,"metric",s as usize)?;w(m,s,4)}
        fn inserted30(&mut self,_:&mut Memory,a:&mut Storage,s:u32,_:usize)->Result<()>{self.record(a,"insert30",s as usize)}
        fn removed34(&mut self,_:&mut Memory,a:&mut Storage,s:u32,_:usize)->Result<()>{self.record(a,"remove34",s as usize)}
    }
    impl profile::Host for Spy<'_>{
        fn world_callback(&mut self,_:&mut Memory,_:&mut Storage,_:u32,_:usize,_:u32,_:u32)->Result<()>{anyhow::bail!("Fixture has no ordinary World callbacks")}
        fn dependency_detach14(&mut self,_:&mut Memory,_:&mut Storage,_:usize,_:usize)->Result<()>{anyhow::bail!("Fixture has no entity dependencies")}
    }
    #[test]
    fn canonical_brain_lists_and_actual_world_actor_profile_composition_match_original()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_brain_lifecycle.json"))?;
        for case in fixture["cases"].as_array().unwrap(){
            let c=&case["input"]["config"];let op=c["op"].as_str().unwrap();
            let mut m:Memory=serde_json::from_value(case["input"]["memory"].clone())?;
            let mut expected:Memory=serde_json::from_value(case["memory"].clone())?;
            let mut actors=Storage::default();let mut actor_expected=Vec::new();
            for id in [A,A+4096]{
                let mut actor=super::super::ai_navigation_world_owned::tests::constructed_actor(id as usize)?;
                let e=&mut actor.entity;e.common.word_14=w(&m,id,20)?;e.common.brain=link(w(&m,id,24)?);
                e.common.word_28=w(&m,id,40)?;e.word_5c=w(&m,id,92)? as i32;
                e.profile_68=link(w(&m,id,104)?);e.profile_node_70=link(w(&m,id,112)?);
                actor_expected.push((id,[w(&expected,id,20)?,w(&expected,id,24)?,w(&expected,id,40)?,w(&expected,id,92)?,w(&expected,id,104)?,w(&expected,id,112)?]));
                actors.entities.insert(id as usize,actor);
            }
            let mut brain=constructed_brain()?;brain.base.services=list(&m,12,16)?;brain.base.agents=list(&m,4,8)?;
            let lists_expected=(list(&expected,12,16)?,list(&expected,4,8)?);
            actors.brains.insert(B as usize,brain);
            // Neither the actor nor the brain/list snapshots remain readable.
            // Nested callbacks must consult the same canonical constructed owner.
            for address in [A,A+4096,B,OUT,SV,AV,SV+8192,AV+8192]{m.regions.remove(&address);expected.regions.remove(&address);}
            if op=="world_roundtrip"{m.regions.remove(&(WORLD+12288));expected.regions.remove(&(WORLD+12288));}
            let mut native=profile::Native{serial_slot:SERIAL,callbacks:Native{callbacks:Spy{config:c,trace:vec![],fired:false}}};
            let result=match op {
                "brain_insert"|"brain_remove"=>{transition(&mut m,&mut actors,B as usize,op=="brain_insert",&mut native.callbacks.callbacks)?;Value::Null},
                "actor_insert"=>{profile::inserted24(&mut m,&mut actors,A as usize,SERIAL,&mut native.callbacks)?;Value::Null},
                "actor_remove"=>{profile::removed20(&mut m,&mut actors,A as usize,&mut native.callbacks)?;Value::Null},
                "world_roundtrip"=>{
                    let mut binding=super::super::ai_navigation_world_owned::Binding{actors:&mut actors,callbacks:&mut native};
                    let registered=binding.register(&mut m,WORLD,A as usize)?;let removed=binding.unregister(&mut m,WORLD,A as usize)?;json!([registered,removed])
                },_=>anyhow::bail!("Unknown brain lifecycle reference operation")
            };
            assert_eq!(result,case["result"],"result {c}");
            assert_eq!(json!(native.callbacks.callbacks.trace),case["trace"],"callbacks {c}");
            assert_eq!(serde_json::to_value(&m)?,serde_json::to_value(&expected)?,"non-owner memory {c}");
            let base=&actors.brains[&(B as usize)].base;
            assert_eq!((&base.services,&base.agents),(&lists_expected.0,&lists_expected.1),"live lists {c}");
            assert_eq!((base.owner,base.action),(Some(A as usize),Some(91)));
            for (id,expected) in actor_expected {
                let e=&actors.entities[&(id as usize)].entity;
                assert_eq!([e.common.word_14,e.common.brain.unwrap_or(0) as u32,e.common.word_28,e.word_5c as u32,
                    e.profile_68.unwrap_or(0) as u32,e.profile_node_70.unwrap_or(0) as u32],expected,"canonical actor {c}");
            }
        }Ok(())
    }
}
