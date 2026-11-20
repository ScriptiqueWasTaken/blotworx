//! Actor service lookup4595a0 and query-cache preparation49fa98.
//! Segment geometry and the invalidation predicate remain mandatory native services.
use anyhow::{Context as _,Result,ensure};
use std::collections::{BTreeMap,BTreeSet};
use super::{ai_astar_graph_services::{Actors,GraphContext},ai_graph_link_passes::Memory,ai_astar_memory::Node as Seed,ai_traversal_instances::Instance};
///4595a0 over canonical service records. Unlike world46c91c, the service
///count is signed, the getter is virtual18, and a matched slot is reloaded.
pub fn find_in_memory(m:&mut Memory,owner:u32,target:u32,class18:&mut impl FnMut(&mut Memory,u32)->Result<u32>)->Result<u32>{
    let mut index=0u32;
    while (index as i32)<(m.word(owner.wrapping_add(0x10))? as i32) {
        let table=m.word(owner.wrapping_add(12))?;
        let service=m.word(table.wrapping_add(index.wrapping_mul(4)))?;
        let mut class=class18(m,service)?;let mut seen=BTreeSet::new();
        loop {
            if class==target {let table=m.word(owner.wrapping_add(12))?;return m.word(table.wrapping_add(index.wrapping_mul(4)))}
            ensure!(seen.insert(class),"Cyclic actor service class ancestry");
            class=m.word(class.wrapping_add(0x14))?;
            if class==0 {break}
        }
        index=index.wrapping_add(1);
    }
    Ok(0)
}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Owner {pub word14:u32,pub services:Vec<usize>}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Service {pub class:usize,pub query:Option<usize>,pub context:usize,pub filter:Option<usize>}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Query {pub word4:u32,pub word8:u32,pub owner:usize,pub pool:usize}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Entry {pub first:u32,pub second:u32,pub handle:Option<usize>,pub time:u32,pub flagged:u8,pub previous:Option<usize>,pub next:Option<usize>,pub active:u8}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Pool {pub head:Option<usize>,pub tail:Option<usize>,pub free:Option<usize>,pub count:u32}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Runtime {
    pub pathfinder_class:usize,pub sentinel:[u32;3],pub constraints:BTreeMap<usize,Option<usize>>,
    pub owners:BTreeMap<usize,Owner>,pub services:BTreeMap<usize,Service>,pub classes:BTreeMap<usize,Option<usize>>,
    pub queries:BTreeMap<usize,Query>,pub pools:BTreeMap<usize,Pool>,pub entries:BTreeMap<usize,Entry>,
}
#[derive(Clone,Copy,Debug,serde::Serialize)]
pub struct Filter {pub interface:usize,pub context_owner:u32,pub first:u32,pub second:u32,pub flagged:u8,pub time:u32}
pub trait Backend {
    fn clock_bits(&self,context:&GraphContext<'_,Memory>,instance:&Instance)->Result<u32>;
    fn invalidate(&mut self,runtime:&mut Runtime,context:&mut GraphContext<'_,Memory>,instance:&mut Instance,request:Filter)->Result<u8>;
    fn segment(&mut self,runtime:&mut Runtime,context:&mut GraphContext<'_,Memory>,instance:&mut Instance,query:usize,start:[u32;3],end:[u32;3],seed:&mut Seed)->Result<u32>;
}
impl Runtime {
    pub fn find(&self,owner:usize,target:usize)->Result<Option<usize>> {
        let services=&self.owners.get(&owner).context("AI actor owner is absent")?.services;
        // Identified class getters return constant metadata; no external callback runs here.
        for &id in services {
            let mut class=self.services.get(&id).context("AI actor service is absent")?.class;let mut visited=BTreeSet::new();
            loop {
                if class==target {return Ok(Some(id))}
                ensure!(visited.insert(class),"Cyclic AI service class ancestry");
                let Some(parent)=*self.classes.get(&class).context("AI service class metadata is absent")? else{break};
                class=parent;
            }
        }
        Ok(None)
    }
    fn query(&self,id:usize)->Result<&Query>{self.queries.get(&id).context("AI segment-query object is absent")}
    fn entry(&self,id:usize)->Result<&Entry>{self.entries.get(&id).context("AI query cache entry is absent")}
    fn entry_mut(&mut self,id:usize)->Result<&mut Entry>{self.entries.get_mut(&id).context("AI query cache entry is absent")}
    fn request(&self,query:usize,entry:usize,flagged:u8,time:u32)->Result<Option<Filter>> {
        let service=self.services.get(&self.query(query)?.owner).context("Query pathfinder owner is absent")?;
        let Some(interface)=service.filter else{return Ok(None)};
        let context_owner=self.owners.get(&service.context).context("Pathfinder context owner is absent")?.word14;
        let e=self.entry(entry)?;
        Ok(Some(Filter{interface,context_owner,first:e.first,second:e.second,flagged,time}))
    }
    fn remove(&mut self,query:usize,id:usize)->Result<Option<usize>> {
        let next=self.entry(id)?.next;self.entry_mut(id)?.active=0;
        let pool=self.query(query)?.pool;
        if self.pools.get(&pool).context("AI query cache pool is absent")?.head==Some(id) {let v=self.entry(id)?.next;self.pools.get_mut(&pool).unwrap().head=v;}
        if self.pools[&pool].tail==Some(id) {let v=self.entry(id)?.previous;self.pools.get_mut(&pool).unwrap().tail=v;}
        if let Some(previous)=self.entry(id)?.previous {let v=self.entry(id)?.next;self.entry_mut(previous)?.next=v;}
        if let Some(next)=self.entry(id)?.next {let v=self.entry(id)?.previous;self.entry_mut(next)?.previous=v;}
        let free=self.pools[&pool].free;self.entry_mut(id)?.next=free;
        let p=self.pools.get_mut(&pool).unwrap();p.free=Some(id);p.count=p.count.wrapping_sub(1);
        Ok(next)
    }
    pub fn prepare(&mut self,query:usize,context:&mut GraphContext<'_,Memory>,instance:&mut Instance,backend:&mut impl Backend)->Result<()> {
        let q=self.query(query)?;if q.word4==0 || q.word8==0 {return Ok(())}
        let clock=backend.clock_bits(context,instance)?;
        let mut cursor=self.pools.get(&self.query(query)?.pool).context("AI query cache pool is absent")?.head;
        let mut visited=BTreeSet::new();
        while let Some(id)=cursor {
            ensure!(visited.insert(id),"Cyclic active AI query cache");
            let e=self.entry(id)?;let flagged=e.flagged;
            let remove=if flagged!=0 {
                let time=e.time;
                if let Some(request)=self.request(query,id,flagged,time)? {backend.invalidate(self,context,instance,request)?!=0}else{true}
            }else {
                let unchanged=if let Some(handle)=e.handle {
                    let time=context.graph.nodes.get(&handle).context("Query cache graph handle is absent")?.scalar2c;
                    f32::from_bits(time)==f32::from_bits(e.time)
                }else{false};
                if unchanged {false}
                else if let Some(request)=self.request(query,id,1,clock)? {
                    if backend.invalidate(self,context,instance,request)?!=0 {true}
                    else {let e=self.entry_mut(id)?;e.handle=None;e.time=clock;e.flagged=1;false}
                }else{true}
            };
            cursor=if remove {self.remove(query,id)?}else{self.entry(id)?.next};
        }
        Ok(())
    }
}
pub struct Native<H>{pub runtime:Runtime,pub backend:H}
impl<H:Backend> Actors<Memory> for Native<H> {
    fn constraint_owner(&self,key:usize)->Result<Option<usize>>{self.runtime.constraints.get(&key).copied().context("AI traversal constraint is absent")}
    fn sentinel_position(&self)->Result<[u32;3]>{Ok(self.runtime.sentinel)}
    fn actor_for_constraint(&mut self,_:&mut Instance,owner:usize)->Result<Option<usize>>{self.runtime.find(owner,self.runtime.pathfinder_class)}
    fn actor_query(&self,actor:usize)->Result<Option<usize>>{Ok(self.runtime.services.get(&actor).context("Pathfinder actor service is absent")?.query)}
    fn prepare_query(&mut self,i:&mut Instance,c:&mut GraphContext<'_,Memory>,query:usize)->Result<()>{self.runtime.prepare(query,c,i,&mut self.backend)}
    fn segment_query(&mut self,i:&mut Instance,c:&mut GraphContext<'_,Memory>,query:usize,start:[u32;3],end:[u32;3],seed:&mut Seed)->Result<u32>{self.backend.segment(&mut self.runtime,c,i,query,start,end,seed)}
}
#[cfg(test)] mod tests {
    use super::*;
    use std::cell::RefCell;
    use serde_json::{Value,json};
    use super::super::{ai_graph_handles::State,ai_graph_queries::Metadata,ai_goto_traversal};
    struct Fixture<'a>{c:&'a Value,trace:RefCell<Vec<Value>>}
    impl Backend for Fixture<'_> {
        fn clock_bits(&self,_:&GraphContext<'_,Memory>,_:&Instance)->Result<u32>{self.trace.borrow_mut().push(json!(["clock"]));Ok(self.c["clock"].as_u64().unwrap_or(0x40000000) as u32)}
        fn invalidate(&mut self,r:&mut Runtime,c:&mut GraphContext<'_,Memory>,instance:&mut Instance,request:Filter)->Result<u8> {
            self.trace.borrow_mut().push(json!(["filter",request]));let m=&self.c["mutation"];
            let node=r.entries.iter().find(|(_,e)|e.first==request.first).map(|(&id,_)|id).unwrap();
            if let Some(v)=m["owner_word"].as_u64(){r.owners.get_mut(&55).unwrap().word14=v as u32;}
            if let Some(v)=m["query_word4"].as_u64(){r.queries.get_mut(&66).unwrap().word4=v as u32;}
            if let Some(v)=m["query_owner"].as_u64(){r.queries.get_mut(&66).unwrap().owner=v as usize;}
            if let Some(v)=m["query_pool"].as_u64(){r.queries.get_mut(&66).unwrap().pool=v as usize;}
            if let Some(v)=m["handle_time"].as_u64(){c.graph.nodes.get_mut(&100).unwrap().scalar2c=v as u32;}
            if m.get("next").is_some(){r.entries.get_mut(&node).unwrap().next=serde_json::from_value(m["next"].clone())?;}
            if self.c["native_actor"].as_bool().unwrap_or(false) {
                if self.c["change_query"].as_bool().unwrap_or(false){r.services.get_mut(&21).unwrap().query=Some(67);}
                if self.c["prepare_vector"].is_array(){let values:[u32;3]=serde_json::from_value(self.c["prepare_vector"].clone())?;super::super::ai_goto_initialization::state_mut(instance)?.vector_44=values.map(f32::from_bits);}
            }
            Ok(self.c["filter_result"].as_u64().unwrap_or(0) as u8)
        }
        fn segment(&mut self,_:&mut Runtime,_:&mut GraphContext<'_,Memory>,_:&mut Instance,query:usize,start:[u32;3],end:[u32;3],seed:&mut Seed)->Result<u32>{
            ensure!(self.c["native_actor"].as_bool().unwrap_or(false),"Segment geometry is outside this cache fixture");
            self.trace.borrow_mut().push(json!(["segment",query,start,end]));seed.scalar2c=0x3f000000;Ok(self.c["segment"].as_u64().unwrap_or(0) as u32)
        }
    }
    fn graph()->Result<State> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        let c=fixture["cases"].as_array().unwrap().iter().find(|c|c["input"]["state"]["nodes"].get("100").is_some()).unwrap();
        Ok(serde_json::from_value(c["input"]["state"].clone())?)
    }
    fn memory()->Memory{Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride:36,journal:BTreeMap::new()}}
    #[test] fn original_actor_service_lookup_and_query_cache_preparation_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_actor_queries.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut runtime:Runtime=serde_json::from_value(c["runtime"].clone())?;
            let mut graph=graph()?;graph.nodes.get_mut(&100).unwrap().scalar2c=c["handle_time"].as_u64().unwrap() as u32;
            let mut memory=memory();let mut metadata=Metadata::default();let mut instance=ai_goto_traversal::construct(1);
            let mut backend=Fixture{c,trace:RefCell::new(Vec::new())};
            let result=if c["op"]=="find" {json!(runtime.find(55,runtime.pathfinder_class)?)}else{runtime.prepare(66,&mut GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata},&mut instance,&mut backend)?;Value::Null};
            assert_eq!(result,case["result"],"{c}");assert_eq!(backend.trace.into_inner(),case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(runtime)?,case["runtime"],"{c}");assert_eq!(json!(graph.nodes[&100].scalar2c),case["handle_time"],"{c}");
        }
        Ok(())
    }
    #[test] fn missing_bindings_and_invalid_cycles_are_explicit_errors()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_actor_queries.json"))?;
        let c=fixture["cases"].as_array().unwrap().iter().find(|c|c["input"]["runtime"]["entries"].get("300").is_some()).unwrap()["input"].clone();
        let mut runtime:Runtime=serde_json::from_value(c["runtime"].clone())?;
        runtime.classes.insert(2,Some(2));assert!(runtime.find(55,999).is_err());assert!(runtime.find(999,3).is_err());
        let mut graph=graph()?;let mut memory=memory();let mut metadata=Metadata::default();let mut instance=ai_goto_traversal::construct(1);let mut backend=Fixture{c:&c,trace:RefCell::new(Vec::new())};
        runtime.entries.get_mut(&300).unwrap().next=Some(300);
        assert!(runtime.prepare(66,&mut GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata},&mut instance,&mut backend).is_err());
        Ok(())
    }
    #[test] fn original_goto_reset_through_actor_lookup_and_cache_matches()->Result<()> {
        use super::super::{ai_astar_memory::Store,ai_astar_graph_services::GraphServices,ai_graph_queries::Tables,ai_goto_initialization::{state,state_mut}};
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_actor_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut store:Store=serde_json::from_value(c["store"].clone())?;
            let mut graph:State=serde_json::from_value(c["graph"].clone())?;
            let tables:Tables=serde_json::from_value(c["graph_tables"].clone())?;let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let mut memory=super::super::ai_graph_memory_queries::fixture_memory(tables,&metadata)?;
            let mut instance=ai_goto_traversal::construct(1);let s=state_mut(&mut instance)?;
            s.references_54_58=[Some(100),Some(102)];s.word_40=c["selected"].as_u64().unwrap() as i32;s.node_70=99;s.vector_44=[1.,0.,0.];s.reference_8=Some(100);s.reference_6c=c["constraint"].as_bool().unwrap_or(true).then_some(33);
            let actors=Native{runtime:serde_json::from_value(c["actor_runtime"].clone())?,backend:Fixture{c,trace:RefCell::new(Vec::new())}};
            let mut services=GraphServices{graph:&mut graph,tables:&mut memory,metadata:&mut metadata,actors};
            store.reset_goto(&mut instance,&mut services)?;
            let trace=case["trace"].as_array().unwrap().iter().filter(|v|v[0]=="clock" || v[0]=="filter" || v[0]=="segment").cloned().collect::<Vec<_>>();
            assert_eq!(*services.actors.backend.trace.borrow(),trace,"{c}");
            assert_eq!(serde_json::to_value(store)?,case["store"],"{c}");
            let binding=&services.graph.nodes[&100];assert_eq!(json!({"point":binding.point,"graph":binding.graph}),case["graph_binding"],"{c}");
            let s=state(&instance)?;assert_eq!(json!({"selected":s.word_40 as u32,"target":s.node_70,"vector":s.vector_44.map(f32::to_bits),"point":1,"graph":binding.graph.unwrap_or(0)}),case["state"],"{c}");
            let runtime=&services.actors.runtime;let p=&runtime.pools[&200];let e=runtime.entries.get(&300);
            let entry=if let Some(e)=e {json!({"handle":e.handle,"time":e.time,"flagged":e.flagged,"active":e.active})}else{json!({"handle":null,"time":0,"flagged":c["cache_flagged"].as_u64().unwrap_or(0),"active":0})};
            assert_eq!(json!({"head":p.head,"tail":p.tail,"free":p.free,"count":p.count,"query":runtime.services[&21].query,"entry":entry}),case["cache"],"{c}");
        }
        Ok(())
    }

}
