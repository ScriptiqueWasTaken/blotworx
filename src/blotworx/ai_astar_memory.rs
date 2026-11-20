//! Astar workspace resize4734a8, cleanup473904, node473850 and pool473984.
//! Allocation/free and final virtual54 reset are required external services.
use anyhow::{Context as _,Result};
use std::collections::BTreeMap;
use super::{ai_goto_initialization::{state,state_mut},ai_traversal_instances::Instance};

#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct Node {
    pub word8:i32,pub values_c_14:[u32;3],pub word18:u32,pub word1c:i32,
    pub values20_24:[u32;2],pub scalar2c:u32,pub word30:u32,
    pub values34_3c:[u32;3],pub word44:u32,pub word4c:u32,
    pub previous:Option<usize>,pub next:Option<usize>,pub byte58:u8,
}
impl Node {
    pub(super) fn construct()->Self {
        Self{word8:-1,values_c_14:[0;3],word18:0,word1c:-1,
            values20_24:[0x7f7fffff;2],scalar2c:0,word30:0,
            values34_3c:[0x7f7fffff;3],word44:0,word4c:0,
            previous:None,next:None,byte58:0}
    }
}
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct Pool {
    pub allocation:Option<usize>,pub active_head:Option<usize>,pub active_tail:Option<usize>,
    pub free_head:Option<usize>,pub word14:u32,pub count:u32,pub nodes:Vec<Node>,
}
#[derive(Default,Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct Store {pub pools:BTreeMap<usize,Pool>}
pub trait Allocator {
    /// Includes the native optional allocation observer. It may change the
    /// instance; caller publication order and fresh reads remain significant.
    fn allocate(&mut self,instance:&mut Instance,bytes:u32)->Result<Option<usize>>;
    fn free(&mut self,instance:&mut Instance,allocation:usize)->Result<()>;
}
pub trait Host:Allocator {
    fn reset(&mut self,store:&mut Store,instance:&mut Instance)->Result<()>;
}
impl Store {
    fn destroy(&mut self,instance:&mut Instance,id:usize,host:&mut impl Host)->Result<()> {
        let pool=self.pools.get(&id).context("Astar pool is not in its owned arena")?;
        if pool.count!=0 {
            if let Some(allocation)=pool.allocation {
                //473804 with destructor flag -1 has no field/free effects.
                host.free(instance,allocation)?;
            }
        }
        host.free(instance,id)?;
        self.pools.remove(&id);
        Ok(())
    }
    pub fn clear(&mut self,instance:&mut Instance,host:&mut impl Host)->Result<()> {
        for index in 0..2 {
            if let Some(id)=state(instance)?.references_54_58[index] {self.destroy(instance,id,host)?;}
            state_mut(instance)?.references_54_58[index]=None;
        }
        Ok(())
    }
    fn create(&mut self,instance:&mut Instance,count:u32,host:&mut impl Host)->Result<Option<usize>> {
        let Some(id)=host.allocate(instance,0x1c)? else {return Ok(None)};
        anyhow::ensure!(!self.pools.contains_key(&id),"Allocator returned a live Astar pool identity");
        self.pools.insert(id,Pool{allocation:None,active_head:None,active_tail:None,free_head:None,word14:0,count,nodes:Vec::new()});
        if count!=0 {
            let bytes=count.checked_mul(0x5c).and_then(|n|n.checked_add(0x10)).context("Astar node array overflows its native allocation domain")?;
            let allocation=host.allocate(instance,bytes)?;
            // Native8000d334 returns null, followed by an invalid write through
            // that null array. Preserve the partial allocation, report failure.
            let allocation=allocation.context("Required Astar node-array allocation unavailable")?;
            let pool=self.pools.get_mut(&id).unwrap();pool.allocation=Some(allocation);
            pool.nodes.try_reserve_exact(count as usize)?;
            for _ in 0..count {pool.nodes.push(Node::construct());}
            pool.free_head=Some(0);
            for (index,node) in pool.nodes.iter_mut().enumerate() {
                node.previous=index.checked_sub(1);
                node.next=(index+1<count as usize).then_some(index+1);
            }
        }
        Ok(Some(id))
    }
    pub fn resize(&mut self,instance:&mut Instance,requested:u32,host:&mut impl Host)->Result<()> {
        let count=if requested==0 {1} else {requested};
        if count>state(instance)?.word_50 {
            self.clear(instance,host)?;
            state_mut(instance)?.word_50=count;
            for index in 0..2 {
                let pool=self.create(instance,count,host)?;
                state_mut(instance)?.references_54_58[index]=pool;
            }
        }
        host.reset(self,instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,allocation_index:usize}
    impl Allocator for Fixture<'_> {
        fn allocate(&mut self,_:&mut Instance,bytes:u32)->Result<Option<usize>> {
            let index=self.allocation_index;self.allocation_index+=1;
            let result=if self.case["fail_allocation"].as_u64()==Some(index as u64) {None} else {Some(100+index)};
            self.trace.push(json!(["allocate",bytes,result]));
            if self.case["observer"].as_bool().unwrap_or(false){self.trace.push(json!(["observer",result,bytes,123]));}
            Ok(result)
        }
        fn free(&mut self,instance:&mut Instance,id:usize)->Result<()> {
            self.trace.push(json!(["free",id,state(instance)?.references_54_58]));Ok(())
        }
    }
    impl Host for Fixture<'_> {
        fn reset(&mut self,_:&mut Store,instance:&mut Instance)->Result<()> {
            let s=state(instance)?;self.trace.push(json!(["reset",s.word_50,s.references_54_58]));Ok(())
        }
    }
    #[test]
    fn original_astar_workspace_resize_cleanup_nodes_and_lists_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_memory.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut instance=super::super::ai_goto_traversal::construct(1);
            let mut store=Store::default();let mut host=Fixture{case:c,trace:Vec::new(),allocation_index:0};
            if let Some(initial)=c["initial"].as_u64(){store.resize(&mut instance,initial as u32,&mut host)?;host.trace.clear();}
            store.resize(&mut instance,c["requested"].as_u64().unwrap() as u32,&mut host)?;
            assert_eq!(host.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            let s=state(&instance)?;
            assert_eq!(json!({"count":s.word_50,"pools":s.references_54_58}),case["state"],"{c}");
            assert_eq!(serde_json::to_value(&store)?,case["store"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn missing_node_arrays_fail_without_reset_or_false_pool_publication()->Result<()> {
        for failure in [1,3] {
            let case=json!({"fail_allocation":failure});
            let mut host=Fixture{case:&case,trace:Vec::new(),allocation_index:0};
            let mut instance=super::super::ai_goto_traversal::construct(1);
            let mut store=Store::default();
            let error=store.resize(&mut instance,2,&mut host).unwrap_err();
            assert!(error.to_string().contains("node-array allocation"));
            assert_eq!(state(&instance)?.word_50,2);
            assert_eq!(state(&instance)?.references_54_58,if failure==1 {[None,None]} else {[Some(100),None]});
            assert!(host.trace.iter().all(|entry|entry[0]!="reset"));
            let pending=store.pools.get(&(99+failure)).unwrap();
            assert!(pending.allocation.is_none() && pending.nodes.is_empty());
        }
        Ok(())
    }
}
