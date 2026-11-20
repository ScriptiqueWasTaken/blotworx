//! Concrete JPSGotoTraversal construction196a64 through196274/471790/4834a8.
//! Factory1969ec allocation is mandatory. Initialization and traversal execution
//! are separate required stages, not successful no-op methods on this type.
use anyhow::Result;
use super::ai_traversal_instances::{Concrete,Instance};

#[derive(Debug,serde::Serialize)]
///Common196274 Astar fields. Concrete retains the actual derived class identity.
pub struct State {
    pub strategy:Option<usize>,
    pub reference_8:Option<usize>,
    pub word_c:i32,
    pub flag_14:bool,
    pub flag_15:bool,
    pub word_18:i32,
    pub values_30_34:[f32;2],
    pub references_38_3c:[Option<usize>;2],
    pub word_40:i32,
    pub vector_44:[f32;3],
    pub word_50:u32,
    pub references_54_58:[Option<usize>;2],
    pub reference_5c:Option<usize>,
    pub word_60:u32,
    pub reference_64:Option<usize>,
    pub word_68:u32,
    pub reference_6c:Option<usize>,
    pub node_70:u32,
    pub position_74:[f32;3],
    pub values_80_8c:[f32;4],
}
pub trait AllocationHost {
    /// Native allocation plus its optional allocation observer. None retains
    /// the factory's null result; it must not construct a substitute instance.
    fn allocate(&mut self,bytes:u32)->Result<Option<usize>>;
}
pub fn create(host:&mut impl AllocationHost)->Result<Option<Instance>> {
    Ok(host.allocate(0x90)?.map(construct))
}
pub fn construct(handle:usize)->Instance {
    super::ai_traversal_instances::construct_astar(handle,Concrete::Goto(construct_fields()))
}
///196274 constructs these fields for both Goto and Wander traversals.
pub(super) fn construct_fields()->State {
    State {
        strategy:None,reference_8:None,word_c:-1,flag_14:true,flag_15:true,word_18:-1,
        values_30_34:[f32::MAX;2],references_38_3c:[None;2],word_40:-1,vector_44:[0.;3],
        word_50:0,references_54_58:[None;2],reference_5c:None,word_60:0x80,reference_64:None,
        word_68:6,reference_6c:None,node_70:u32::MAX,position_74:[0.;3],values_80_8c:[0.;4],
    }
}

impl State {
    ///196b84 copies the node ID and three coordinates without normalization,
    /// finite-value checks or alteration of the other retained state.
    pub fn set_target(&mut self,node:u32,position:[f32;3]) {self.node_70=node;self.position_74=position;}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::collections::BTreeMap;
    struct Allocator<'a>{case:&'a Value,trace:Vec<Value>}
    impl AllocationHost for Allocator<'_> {
        fn allocate(&mut self,bytes:u32)->Result<Option<usize>> {
            self.trace.push(json!(["allocate",bytes]));let result=self.case["allocated"].as_bool().unwrap_or(true).then_some(100);
            if self.case["observer"].as_bool().unwrap_or(false){self.trace.push(json!(["observer",result,bytes,123]));}
            Ok(result)
        }
    }
    impl super::super::ai_traversal_release::Host for Allocator<'_> {
        fn budget_mode0(&mut self,instance:&mut Instance)->Result<u32>{self.release_budget(instance,"budget0")}
        fn budget_mode1(&mut self,instance:&mut Instance)->Result<u32>{self.release_budget(instance,"budget1")}
    }
    impl Allocator<'_> {
        fn release_budget(&mut self,instance:&Instance,kind:&str)->Result<u32> {
            let state=(instance.concrete.as_ref().unwrap().as_ref()).astar();
            self.trace.push(json!(["context"]));
            self.trace.push(json!([kind,state.word_c as u32,instance.lease.as_ref().unwrap().owner_key]));
            Ok(self.case["clear"]["budget_result"].as_u64().unwrap() as u32)
        }
    }
    fn words(instance:&Instance,state:&State)->BTreeMap<String,u32> {
        let lease=instance.lease.as_ref().unwrap();let mut words=BTreeMap::new();
        let mut put=|off:u32,value:u32|{words.insert(format!("{off:x}"),value);};
        put(4,state.strategy.unwrap_or(0) as u32);put(8,state.reference_8.unwrap_or(0) as u32);put(0xc,state.word_c as u32);
        put(0x10,instance.traversal_id.unwrap());put(0x18,state.word_18 as u32);
        put(0x20,lease.field_20.unwrap());put(0x24,lease.owner_key);put(0x28,lease.reuse_count as u32);put(0x2c,lease.clock_bits);
        for (i,v) in state.values_30_34.into_iter().enumerate(){put(0x30+i as u32*4,v.to_bits());}
        for (i,v) in state.references_38_3c.into_iter().enumerate(){put(0x38+i as u32*4,v.unwrap_or(0) as u32);}
        put(0x40,state.word_40 as u32);
        for (i,v) in state.vector_44.into_iter().enumerate(){put(0x44+i as u32*4,v.to_bits());}
        put(0x50,state.word_50);
        for (i,v) in state.references_54_58.into_iter().enumerate(){put(0x54+i as u32*4,v.unwrap_or(0) as u32);}
        put(0x5c,state.reference_5c.unwrap_or(0) as u32);put(0x60,state.word_60);put(0x64,state.reference_64.unwrap_or(0) as u32);
        put(0x68,state.word_68);put(0x6c,state.reference_6c.unwrap_or(0) as u32);put(0x70,state.node_70);
        for (i,v) in state.position_74.into_iter().enumerate(){put(0x74+i as u32*4,v.to_bits());}
        for (i,v) in state.values_80_8c.into_iter().enumerate(){put(0x80+i as u32*4,v.to_bits());}
        words
    }
    #[test]
    fn original_complete_goto_constructor_factory_and_target_words_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goto_traversal.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let mut allocator=Allocator{case:c,trace:vec![]};
            let mut instance=create(&mut allocator)?;
            assert_eq!(json!(instance.is_some()),expected["available"],"factory result: {c}");
            if let Some(instance)=&mut instance {
                {
                    let state=(instance.concrete.as_mut().unwrap().as_mut()).astar_mut();
                    if !c["target"].is_null(){
                        let position=std::array::from_fn(|i|f32::from_bits(c["target"]["position"][i].as_u64().unwrap() as u32));
                        state.set_target(c["target"]["node"].as_u64().unwrap() as u32,position);
                    }
                    if !c["clear"].is_null(){state.word_c=c["clear"]["service"].as_i64().unwrap() as i32;}
                }
                if !c["clear"].is_null(){
                    let clear=&c["clear"];let lease=instance.lease.as_mut().unwrap();
                    lease.field_20=Some(clear["tag"].as_u64().unwrap() as u32);lease.owner_key=clear["owner"].as_u64().unwrap() as u32;
                    lease.reuse_count=clear["count"].as_i64().unwrap() as i32;lease.clock_bits=clear["clock"].as_u64().unwrap() as u32;
                    instance.clear_lease(&mut allocator)?;
                }
                let state=(instance.concrete.as_ref().unwrap().as_ref()).astar();
                let flags=[state.flag_14,state.flag_15];
                assert_eq!(json!(words(instance,state)),expected["words"],"constructor/target words: {c}");
                assert_eq!(json!(flags),expected["flags"],"constructor flags: {c}");
                // These native bytes are not initialized by the constructor;
                // the portable typed object does not assign them a fake value.
                let poison=c["poison"].as_u64().unwrap_or(0xa5);
                assert_eq!(expected["untouched"],json!([poison,poison]));
            }
            assert_eq!(json!(allocator.trace),expected["trace"],"allocation/budget order: {c}");
        }
        Ok(())
    }
    #[test]
    fn concrete_constructor_supplies_the_recovered_lease_to_pool_acquisition()->Result<()> {
        use super::super::{ai_traversal_instances::Pool,ai_traversal_acquisition::Host};
        struct Context;
        impl Host for Context {
            fn prepare_mode0(&mut self,pool:&mut Pool,first:usize,_:u32)->Result<u32> {
                let instance=pool.slots.iter().filter_map(|s|s.instance.as_ref()).find(|i|i.handle==first).unwrap();
                let state=(instance.concrete.as_ref().unwrap().as_ref()).astar();
                assert_eq!(state.word_c,-1);
                //46a1f0 rejects the constructor's unsigned FFFFFFFF task index.
                Ok(0)
            }
            fn prepare_mode1(&mut self,_:&mut Pool,_:usize,_:u32)->Result<u32>{unreachable!()}
            fn clock_50(&mut self,_:&mut Pool)->Result<f32>{Ok(2.)}
            fn clock_58(&mut self,_:&mut Pool)->Result<f32>{Ok(1.5)}
            fn post_acquire(&mut self,_:&mut Pool,_:usize,_:u32,_:f32)->Result<u32>{unreachable!()}
        }
        // Supplied context-clock boundary; this composes real construction
        // with the recovered pool/selector, not concrete initialization/AI.
        let mut pool=Pool::new(1)?;assert!(pool.insert(construct(100))?.is_none());
        let selected=pool.acquire_kernel(77,0,&mut Context)?.unwrap();
        assert_eq!(selected.handle,100);
        let instance=pool.slots[selected.slot].instance.as_ref().unwrap();
        let lease=instance.lease.as_ref().unwrap();
        assert_eq!((lease.owner_key,lease.reuse_count,lease.clock_bits,lease.field_20),(77,-1,2f32.to_bits(),Some(2)));
        let state=(instance.concrete.as_ref().unwrap().as_ref()).astar();
        assert_eq!(state.node_70,u32::MAX);
        Ok(())
    }
}
