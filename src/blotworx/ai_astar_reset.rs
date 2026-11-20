//! Goto reset196358 ->471d68 and ordered node insertion4732ac.
//! Graph/actor/query services are mandatory; no guessed graph positions.
use anyhow::{Context as _,Result,ensure};
use std::collections::BTreeSet;
use super::{ai_astar_memory::{Allocator,Host,Node,Pool,Store},ai_goto_initialization::{state,state_mut},ai_traversal_instances::Instance};

impl Pool {
    fn node(&self,index:usize)->Result<&Node>{self.nodes.get(index).context("Astar list node is outside its pool")}
    fn node_mut(&mut self,index:usize)->Result<&mut Node>{self.nodes.get_mut(index).context("Astar list node is outside its pool")}
    fn reset_nodes(&mut self)->Result<()> {
        let mut current=self.active_head;let mut visited=BTreeSet::new();
        while let Some(index)=current {
            ensure!(visited.insert(index),"Cyclic active Astar list");
            let n=self.node_mut(index)?;
            n.word8=-1;n.word18=0;n.values_c_14=[0;3];n.word1c=-1;
            n.values20_24=[0x7f7fffff;2];n.word30=0;n.values34_3c=[0x7f7fffff;3];
            current=n.next;
        }
        while let Some(index)=self.active_head {
            let old_free=self.free_head;
            let node=self.node_mut(index)?;node.byte58=0;
            let next=node.next;node.next=old_free;
            self.free_head=Some(index);self.active_head=next;
        }
        self.active_tail=None;self.word14=0;
        Ok(())
    }
    pub fn insert(&mut self,source:&Node)->Result<bool> {
        let mut position=self.active_head;let mut visited=BTreeSet::new();
        while let Some(index)=position {
            ensure!(visited.insert(index),"Cyclic ordered Astar list");
            let node=self.node(index)?;
            if f32::from_bits(node.values20_24[1])>f32::from_bits(source.values20_24[1]) {break;}
            position=node.next;
        }
        let Some(index)=self.free_head else{return Ok(false)};
        self.word14=self.word14.wrapping_add(1);
        let old=self.node(index)?;self.free_head=old.next;
        let old=self.node(index)?;
        let mut new=source.clone();new.previous=old.previous;new.next=old.next;new.byte58=1;
        *self.node_mut(index)?=new;
        match position {
            None=>match self.active_tail {
                None=>{self.active_head=Some(index);self.active_tail=Some(index);let node=self.node_mut(index)?;node.next=None;node.previous=None;}
                Some(tail)=>{let node=self.node_mut(index)?;node.previous=Some(tail);node.next=None;self.node_mut(tail)?.next=Some(index);self.active_tail=Some(index);}
            },
            Some(position)=>{
                let previous=self.node(position)?.previous;
                self.node_mut(index)?.next=Some(position);self.node_mut(position)?.previous=Some(index);
                self.node_mut(index)?.previous=previous;
                if let Some(previous)=previous {self.node_mut(previous)?.next=Some(index);} else {self.active_head=Some(index);}
            }
        }
        Ok(true)
    }
}
pub trait Services {
    type Point:Copy;
    fn handle_ready(&mut self,instance:&mut Instance,handle:usize)->Result<bool>;
    fn handle_point(&self,handle:usize)->Result<Option<Self::Point>>;
    fn find_point(&mut self,instance:&mut Instance,handle:usize,node:u32)->Result<u32>;
    fn point_position(&self,point:Self::Point)->Result<[u32;3]>;
    fn restore_point_and_clear_graph(&mut self,handle:usize,point:Option<Self::Point>)->Result<()>;
    fn constraint_owner(&self,constraint:usize)->Result<Option<usize>>;
    fn sentinel_position(&self)->Result<[u32;3]>;
    fn actor_for_constraint(&mut self,instance:&mut Instance,owner:usize)->Result<Option<usize>>;
    fn actor_query(&self,actor:usize)->Result<Option<usize>>;
    fn prepare_query(&mut self,instance:&mut Instance,query:usize)->Result<()>;
    fn segment_query(&mut self,instance:&mut Instance,query:usize,start:[u32;3],end:[u32;3],seed:&mut Node)->Result<u32>;
}
fn different(a:[u32;3],b:[u32;3])->bool {a.into_iter().zip(b).any(|(a,b)|f32::from_bits(a)!=f32::from_bits(b))}
impl Store {
    pub fn reset_goto(&mut self,instance:&mut Instance,services:&mut impl Services)->Result<()> {
        state_mut(instance)?.node_70=u32::MAX;
        // Reset second pool first; resolve the first pool only afterward.
        for index in [1,0] {
            let id=state(instance)?.references_54_58[index].context("Astar reset requires both node pools")?;
            self.pools.get_mut(&id).context("Astar reset pool is not in its arena")?.reset_nodes()?;
        }
        let node_id=state(instance)?.word_40 as u32;
        if node_id==u32::MAX {return Ok(())}
        let mut position=state(instance)?.vector_44.map(f32::to_bits);
        if let Some(handle)=state(instance)?.reference_8 {
            if services.handle_ready(instance,handle)? {
                let handle=state(instance)?.reference_8.context("Ready graph handle was removed")?;
                let saved=services.handle_point(handle)?;
                let current_node=state(instance)?.word_40 as u32;
                if services.find_point(instance,handle,current_node)?==1 {
                    let handle=state(instance)?.reference_8.context("Graph lookup removed its handle")?;
                    let point=services.handle_point(handle)?.context("Graph lookup succeeded without a point")?;
                    position=services.point_position(point)?;
                }
                let handle=state(instance)?.reference_8.context("Graph lookup removed its restoration handle")?;
                services.restore_point_and_clear_graph(handle,saved)?;
            }
        }
        let mut seed=Node::construct();
        seed.word8=state(instance)?.word_40;seed.values_c_14=position;seed.values20_24=[0;2];
        if let Some(constraint)=state(instance)?.reference_6c {
            if let Some(owner)=services.constraint_owner(constraint)? {
                let start=state(instance)?.vector_44.map(f32::to_bits);
                if different(start,services.sentinel_position()?) && different(start,position) {
                    if let Some(actor)=services.actor_for_constraint(instance,owner)? {
                        if let Some(query)=services.actor_query(actor)? {
                            services.prepare_query(instance,query)?;
                            let query=services.actor_query(actor)?.context("Prepared graph actor lost its query interface")?;
                            let start=state(instance)?.vector_44.map(f32::to_bits);
                            if services.segment_query(instance,query,start,position,&mut seed)?==1 {return Ok(())}
                        }
                    }
                }
            }
        }
        let id=state(instance)?.references_54_58[0].context("Astar seed insertion requires its node pool")?;
        self.pools.get_mut(&id).context("Astar seed pool is not in its arena")?.insert(&seed)?;
        Ok(())
    }
}
/// Allocation callbacks plus the real goto reset, usable by workspace resize.
pub struct ResettingHost<A,S> {pub allocator:A,pub services:S}
impl<A:Allocator,S> Allocator for ResettingHost<A,S> {
    fn allocate(&mut self,i:&mut Instance,n:u32)->Result<Option<usize>>{self.allocator.allocate(i,n)}
    fn free(&mut self,i:&mut Instance,p:usize)->Result<()>{self.allocator.free(i,p)}
}
impl<A:Allocator,S:Services> Host for ResettingHost<A,S> {
    fn reset(&mut self,store:&mut Store,instance:&mut Instance)->Result<()> {store.reset_goto(instance,&mut self.services)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    fn bits(c:&Value,key:&str,default:[u32;3])->[u32;3] {
        c[key].as_array().map(|a|std::array::from_fn(|i|a[i].as_u64().unwrap() as u32)).unwrap_or(default)
    }
    struct Fixture<'a> {c:&'a Value,trace:Vec<Value>,point:Option<usize>,graph:u32,prepared:bool}
    impl Services for Fixture<'_> {
        type Point=usize;
        fn handle_ready(&mut self,_:&mut Instance,_:usize)->Result<bool>{self.trace.push(json!(["ready",44]));Ok(self.c["ready"].as_bool().unwrap_or(true))}
        fn handle_point(&self,_:usize)->Result<Option<usize>>{Ok(self.point)}
        fn find_point(&mut self,instance:&mut Instance,_:usize,node:u32)->Result<u32> {
            self.trace.push(json!(["find",44,node]));let result=self.c["found"].as_u64().unwrap_or(1) as u32;
            if result==1 {self.point=Some(2);}
            if let Some(node)=self.c["find_node"].as_u64(){state_mut(instance)?.word_40=node as i32;}
            Ok(result)
        }
        fn point_position(&self,_:usize)->Result<[u32;3]>{Ok(bits(self.c,"position",[0x40000000,0,0]))}
        fn restore_point_and_clear_graph(&mut self,_:usize,point:Option<usize>)->Result<()> {self.point=point;self.graph=0;Ok(())}
        fn constraint_owner(&self,_:usize)->Result<Option<usize>>{Ok(self.c["owner"].as_bool().unwrap_or(true).then_some(55))}
        fn sentinel_position(&self)->Result<[u32;3]>{Ok(bits(self.c,"sentinel",[0;3]))}
        fn actor_for_constraint(&mut self,_:&mut Instance,owner:usize)->Result<Option<usize>> {self.trace.push(json!(["actor",owner]));Ok(self.c["actor"].as_bool().unwrap_or(true).then_some(77))}
        fn actor_query(&self,_:usize)->Result<Option<usize>>{Ok(self.c["query"].as_bool().unwrap_or(true).then_some(if self.prepared && self.c["change_query"].as_bool().unwrap_or(false){67}else{66}))}
        fn prepare_query(&mut self,instance:&mut Instance,query:usize)->Result<()> {
            self.trace.push(json!(["prepare",query]));self.prepared=true;
            if self.c["prepare_vector"].is_array(){state_mut(instance)?.vector_44=bits(self.c,"prepare_vector",[0;3]).map(f32::from_bits);}
            Ok(())
        }
        fn segment_query(&mut self,_:&mut Instance,query:usize,start:[u32;3],end:[u32;3],seed:&mut Node)->Result<u32> {
            self.trace.push(json!(["segment",query,start,end]));seed.scalar2c=0x3f000000;Ok(self.c["segment"].as_u64().unwrap_or(0) as u32)
        }
    }
    impl<T> super::super::ai_astar_graph_services::Actors<T> for Fixture<'_> {
        fn constraint_owner(&self,c:usize)->Result<Option<usize>>{<Self as Services>::constraint_owner(self,c)}
        fn sentinel_position(&self)->Result<[u32;3]>{<Self as Services>::sentinel_position(self)}
        fn actor_for_constraint(&mut self,i:&mut Instance,o:usize)->Result<Option<usize>>{<Self as Services>::actor_for_constraint(self,i,o)}
        fn actor_query(&self,a:usize)->Result<Option<usize>>{<Self as Services>::actor_query(self,a)}
        fn prepare_query(&mut self,i:&mut Instance,_:&mut super::super::ai_astar_graph_services::GraphContext<'_,T>,q:usize)->Result<()>{<Self as Services>::prepare_query(self,i,q)}
        fn segment_query(&mut self,i:&mut Instance,_:&mut super::super::ai_astar_graph_services::GraphContext<'_,T>,q:usize,s:[u32;3],e:[u32;3],n:&mut Node)->Result<u32>{<Self as Services>::segment_query(self,i,q,s,e,n)}
    }
    #[test]
    fn original_goto_reset_with_concrete_graph_metadata_and_search_matches()->Result<()> {
        use super::super::{ai_astar_graph_services::GraphServices,ai_graph_queries::{Tables,Metadata},ai_graph_handles::State as Graph};
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_graph_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut store:Store=serde_json::from_value(c["store"].clone())?;
            let mut graph:Graph=serde_json::from_value(c["graph"].clone())?;
            let mut tables:Tables=serde_json::from_value(c["graph_tables"].clone())?;
            let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let mut instance=super::super::ai_goto_traversal::construct(1);let s=state_mut(&mut instance)?;
            s.references_54_58=[Some(100),Some(102)];s.word_40=c["selected"].as_u64().unwrap() as i32;s.node_70=99;
            s.vector_44=[1.,0.,0.];s.reference_8=Some(100);s.reference_6c=c["constraint"].as_bool().unwrap_or(true).then_some(33);
            let actors=Fixture{c,trace:Vec::new(),point:Some(1),graph:99,prepared:false};
            let mut services=GraphServices{graph:&mut graph,tables:&mut tables,metadata:&mut metadata,actors};
            store.reset_goto(&mut instance,&mut services)?;
            let actor_trace=case["trace"].as_array().unwrap().iter().filter(|e|e[0]!="ready" && e[0]!="find").cloned().collect::<Vec<_>>();
            assert_eq!(services.actors.trace,actor_trace,"{c}");
            assert_eq!(serde_json::to_value(store)?,case["store"],"{c}");
            let binding=&services.graph.nodes[&100];
            assert_eq!(json!({"point":binding.point,"graph":binding.graph}),case["graph_binding"],"{c}");
            let s=state(&instance)?;
            assert_eq!(json!({"selected":s.word_40 as u32,"target":s.node_70,"vector":s.vector_44.map(f32::to_bits),"point":1,"graph":binding.graph.unwrap_or(0)}),case["state"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn original_goto_reset_with_canonical_linked_buffers_matches()->Result<()> {
        use super::super::{ai_astar_graph_services::GraphServices,ai_graph_queries::{Tables,Metadata},ai_graph_handles::State as Graph};
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_graph_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut store:Store=serde_json::from_value(c["store"].clone())?;
            let mut graph:Graph=serde_json::from_value(c["graph"].clone())?;
            let tables:Tables=serde_json::from_value(c["graph_tables"].clone())?;
            let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let mut memory=super::super::ai_graph_memory_queries::fixture_memory(tables,&metadata)?;
            let mut instance=super::super::ai_goto_traversal::construct(1);let s=state_mut(&mut instance)?;
            s.references_54_58=[Some(100),Some(102)];s.word_40=c["selected"].as_u64().unwrap() as i32;s.node_70=99;
            s.vector_44=[1.,0.,0.];s.reference_8=Some(100);s.reference_6c=c["constraint"].as_bool().unwrap_or(true).then_some(33);
            let actors=Fixture{c,trace:Vec::new(),point:Some(1),graph:99,prepared:false};
            let mut services=GraphServices{graph:&mut graph,tables:&mut memory,metadata:&mut metadata,actors};
            store.reset_goto(&mut instance,&mut services)?;
            let actor_trace=case["trace"].as_array().unwrap().iter().filter(|e|e[0]!="ready" && e[0]!="find").cloned().collect::<Vec<_>>();
            assert_eq!(services.actors.trace,actor_trace,"{c}");
            assert_eq!(serde_json::to_value(store)?,case["store"],"{c}");
            let binding=&services.graph.nodes[&100];
            assert_eq!(json!({"point":binding.point,"graph":binding.graph}),case["graph_binding"],"{c}");
            let s=state(&instance)?;
            assert_eq!(json!({"selected":s.word_40 as u32,"target":s.node_70,"vector":s.vector_44.map(f32::to_bits),"point":1,"graph":binding.graph.unwrap_or(0)}),case["state"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn original_goto_reset_and_ordered_insertion_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut store:Store=serde_json::from_value(c["store"].clone())?;
            let mut instance=super::super::ai_goto_traversal::construct(1);
            let s=state_mut(&mut instance)?;s.references_54_58=[Some(100),Some(102)];s.word_40=c["selected"].as_u64().unwrap_or(7) as i32;
            s.node_70=99;s.vector_44=bits(c,"vector",[0x3f800000,0,0]).map(f32::from_bits);
            s.reference_8=c["handle"].as_bool().unwrap_or(true).then_some(44);s.reference_6c=c["constraint"].as_bool().unwrap_or(true).then_some(33);
            let mut services=Fixture{c,trace:Vec::new(),point:Some(1),graph:99,prepared:false};
            let result=if c["op"]=="insert" {
                let seed=serde_json::from_value(c["seed"].clone())?;json!(store.pools.get_mut(&100).unwrap().insert(&seed)?)
            } else {store.reset_goto(&mut instance,&mut services)?;Value::Null};
            assert_eq!(result,case["result"],"{c}");
            assert_eq!(services.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(store)?,case["store"],"{c}");
            let s=state(&instance)?;
            assert_eq!(json!({"selected":s.word_40 as u32,"target":s.node_70,"vector":s.vector_44.map(f32::to_bits),"point":services.point,"graph":services.graph}),case["state"],"{c}");
        }
        Ok(())
    }
    struct MemoryAllocator<'a>{c:&'a Value,index:usize,trace:Vec<Value>}
    impl Allocator for MemoryAllocator<'_> {
        fn allocate(&mut self,_:&mut Instance,bytes:u32)->Result<Option<usize>> {
            let id=100+self.index;self.index+=1;self.trace.push(json!(["allocate",bytes,id]));
            if self.c["observer"].as_bool().unwrap_or(false){self.trace.push(json!(["observer",id,bytes,123]));}
            Ok(Some(id))
        }
        fn free(&mut self,instance:&mut Instance,id:usize)->Result<()> {self.trace.push(json!(["free",id,state(instance)?.references_54_58]));Ok(())}
    }
    #[test]
    fn original_workspace_with_concrete_goto_reset_matches()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_workspace_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut instance=super::super::ai_goto_traversal::construct(1);state_mut(&mut instance)?.node_70=99;
            let mut store=Store::default();
            let allocator=MemoryAllocator{c,index:0,trace:Vec::new()};
            let services=Fixture{c,trace:Vec::new(),point:Some(1),graph:99,prepared:false};
            let mut host=ResettingHost{allocator,services};
            if let Some(initial)=c["initial"].as_u64(){store.resize(&mut instance,initial as u32,&mut host)?;host.allocator.trace.clear();}
            store.resize(&mut instance,c["requested"].as_u64().unwrap() as u32,&mut host)?;
            let trace=case["trace"].as_array().unwrap().iter().filter(|e|e[0]!="reset").cloned().collect::<Vec<_>>();
            assert_eq!(host.allocator.trace,trace,"{c}");assert!(host.services.trace.is_empty());
            assert_eq!(serde_json::to_value(store)?,case["store"],"{c}");let s=state(&instance)?;
            assert_eq!(json!({"count":s.word_50,"pools":s.references_54_58,"target":s.node_70}),case["state"],"{c}");
        }
        Ok(())
    }
}
