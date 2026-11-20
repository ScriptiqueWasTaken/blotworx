//! Goto/Astar configuration4719fc/4835fc, dispatcher4837f8 and init4718d8.
//! Numeric conversion, strategy/budget registries, Astar memory and allocation
//! remain mandatory services. There are no successful default implementations.
use anyhow::{Context as _,Result};
use super::{ai_goto_traversal::State,ai_traversal_instances::Instance};

#[derive(Debug,serde::Serialize)]
pub struct ExtractionEntry {
    pub word_0:Option<u32>,
    pub position:[f32;3],
    pub word_10:Option<u32>,
}
///4719e4 initializes coordinates only; the other two words remain unknown.
pub fn extraction_entries(count:u32)->Result<Vec<ExtractionEntry>> {
    count.checked_mul(0x14).and_then(|n|n.checked_add(0x10))
        .context("Extraction array overflows its native allocation domain")?;
    let mut entries=Vec::new();entries.try_reserve_exact(count as usize)?;
    entries.resize_with(count as usize,||ExtractionEntry{word_0:None,position:[0.;3],word_10:None});
    Ok(entries)
}

pub trait Host {
    fn child_count(&mut self,root:usize)->Result<u32>;
    fn child(&mut self,root:usize,index:u32)->Result<usize>;
    fn name(&self,node:usize)->Result<Vec<u8>>;
    fn integer_value(&mut self,node:usize)->Result<u32>;
    /// C double conversion before the original frsp operation.
    fn double_value(&mut self,node:usize)->Result<f64>;
    fn world_scale(&mut self)->Result<Option<f32>>;
    fn resize_astar_memory(&mut self,instance:&mut Instance,size:u32)->Result<()>;
    fn budget_task(&mut self,node:usize)->Result<u32>;
    fn estimated_tasks_enabled(&mut self)->Result<bool>;
    fn estimated_task(&mut self,node:usize)->Result<u32>;
    fn strategy_class(&mut self,node:usize)->Result<Option<Vec<u8>>>;
    fn strategy_metadata(&mut self,name:&[u8])->Result<Option<usize>>;
    fn create_strategy(&mut self,instance:&mut Instance,metadata:usize)->Result<Option<usize>>;
    fn initialize_strategy(&mut self,instance:&mut Instance,strategy:usize,node:usize)->Result<u32>;
    fn touch_context(&mut self)->Result<()>;
    fn allocate(&mut self,instance:&mut Instance,bytes:u32)->Result<Option<usize>>;
    fn construct_extraction_array(&mut self,instance:&mut Instance,allocation:Option<usize>,count:u32)->Result<Option<usize>>;
}
pub(super) fn state(instance:&Instance)->Result<&State> {
    let state=(instance.concrete.as_ref().context("Goto initialization has no concrete state")?.as_ref()).astar();Ok(state)
}
pub(super) fn state_mut(instance:&mut Instance)->Result<&mut State> {
    let state=(instance.concrete.as_mut().context("Goto initialization has no concrete state")?.as_mut()).astar_mut();Ok(state)
}
fn equal(left:&[u8],right:&[u8])->bool {super::ai_traversal_registry::names_equal(left,right)}

pub fn attribute(instance:&mut Instance,node:usize,host:&mut impl Host)->Result<bool> {
    let name=host.name(node)?;
    if equal(&name,b"TraversalAstarMemory") {
        let size=host.integer_value(node)?;host.resize_astar_memory(instance,size)?;return Ok(true);
    }
    if equal(&name,b"MaxDistance") {
        let scale=host.world_scale()?.unwrap_or(1.);
        let value=host.double_value(node)? as f32;
        state_mut(instance)?.values_30_34[0]=scale*value;return Ok(true);
    }
    if equal(&name,b"MaxCost") {
        state_mut(instance)?.values_30_34[1]=host.double_value(node)? as f32;return Ok(true);
    }
    if equal(&name,b"ExtractionBufferSize") {
        state_mut(instance)?.word_60=host.integer_value(node)?;return Ok(true);
    }
    if name.first()==Some(&b'_')||equal(&name,b"InstanceCount") {return Ok(true)};
    if equal(&name,b"AperiodicTask") {
        state_mut(instance)?.word_c=host.budget_task(node)? as i32;return Ok(true);
    }
    if equal(&name,b"EstimatedTask") {
        if host.estimated_tasks_enabled()? {state_mut(instance)?.word_18=host.estimated_task(node)? as i32;}
        return Ok(true);
    }
    if equal(&name,b"HandleStrategy") {
        let Some(class)=host.strategy_class(node)? else {return Ok(false)};
        let Some(metadata)=host.strategy_metadata(&class)? else {return Ok(false)};
        let strategy=host.create_strategy(instance,metadata)?;
        state_mut(instance)?.strategy=strategy;
        let strategy=strategy.context("Required handle strategy allocation unavailable")?;
        //4837d0's boolean result is intentionally ignored by this parser.
        let _=host.initialize_strategy(instance,strategy,node)?;
        return Ok(true);
    }
    Ok(false)
}
pub fn configure(instance:&mut Instance,root:usize,host:&mut impl Host)->Result<bool> {
    let mut index=0;
    while index<host.child_count(root)? {
        let node=host.child(root,index)?;
        if !equal(&host.name(node)?,b"Class") && !attribute(instance,node,host)? {return Ok(false)};
        index+=1;
    }
    if state(instance)?.strategy.is_none() {return Ok(false)};
    host.touch_context()?;
    Ok(true)
}
pub fn initialize(instance:&mut Instance,root:usize,host:&mut impl Host)->Result<bool> {
    if !configure(instance,root,host)? {return Ok(false)};
    if state(instance)?.word_50==0 {return Ok(false)};
    let count=state(instance)?.word_60;
    let allocation=host.allocate(instance,count.wrapping_mul(0x14).wrapping_add(0x10))?;
    let array=host.construct_extraction_array(instance,allocation,count)?;
    state_mut(instance)?.reference_5c=array;
    if array.is_none(){return Ok(false)};
    let state=state(instance)?;
    Ok(state.references_54_58[1].is_some()&&state.references_54_58[0].is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,children:u32,entries:Option<Vec<ExtractionEntry>>,flat:Option<super::super::ai_flat_handle_strategy::State>}
    impl Fixture<'_>{
        fn flag(&self,key:&str,default:bool)->bool {self.case[key].as_bool().unwrap_or(default)}
        fn node(&self,node:usize)->&Value{&self.case["nodes"][node-10]}
    }
    struct StrategyAllocation<'a,'b>{host:&'a mut Fixture<'b>}
    impl super::super::ai_flat_handle_strategy::AllocationHost for StrategyAllocation<'_,'_> {
        fn allocate(&mut self,bytes:u32)->Result<Option<usize>> {
            self.host.trace.push(json!(["strategy_allocate",bytes]));
            if self.host.flag("observer",false){self.host.trace.push(json!(["strategy_observer",201,bytes,123]));}
            Ok(Some(201))
        }
    }
    impl super::super::ai_flat_handle_strategy::Configuration for Fixture<'_> {
        fn child_count(&mut self,root:usize)->Result<u32>{
            Ok(if self.node(root)["class"].is_null(){0}else if self.case["strategy_result"].as_u64().unwrap_or(1)==0 {2}else{1})
        }
        fn child(&mut self,root:usize,index:u32)->Result<usize>{self.trace.push(json!(["strategy_child",root,index]));Ok(10000+(root-10)*2+index as usize)}
        fn name(&self,node:usize)->Result<Vec<u8>>{Ok(if (node-10000)%2==0 {b"Class".to_vec()}else{b"UnsupportedFlatSetting".to_vec()})}
    }
    impl Host for Fixture<'_> {
        fn child_count(&mut self,_:usize)->Result<u32>{Ok(self.children)}
        fn child(&mut self,_:usize,index:u32)->Result<usize>{self.trace.push(json!(["child",index]));Ok(10+index as usize)}
        fn name(&self,node:usize)->Result<Vec<u8>>{Ok(self.node(node)["name"].as_str().unwrap().as_bytes().to_vec())}
        fn integer_value(&mut self,node:usize)->Result<u32>{self.trace.push(json!(["integer",node]));Ok(self.node(node)["integer"].as_i64().unwrap_or(0) as u32)}
        fn double_value(&mut self,node:usize)->Result<f64>{self.trace.push(json!(["double",node]));Ok(f64::from_bits(self.node(node)["double"].as_u64().unwrap_or(2.5f64.to_bits())))}
        fn world_scale(&mut self)->Result<Option<f32>>{
            let value=self.flag("world",true).then_some(self.case["scale"].as_u64().unwrap_or(1.25f32.to_bits() as u64) as u32);
            self.trace.push(json!(["world_scale",value]));Ok(value.map(f32::from_bits))
        }
        fn resize_astar_memory(&mut self,instance:&mut Instance,size:u32)->Result<()> {
            self.trace.push(json!(["resize",size]));let state=state_mut(instance)?;
            state.word_50=self.case["resize_word50"].as_u64().unwrap_or(size.max(1) as u64) as u32;
            state.references_54_58=[self.flag("buffer54",true).then_some(301),self.flag("buffer58",true).then_some(302)];
            if let Some(count)=self.case["resize_child_count"].as_u64(){self.children=count as u32;}
            Ok(())
        }
        fn budget_task(&mut self,node:usize)->Result<u32>{self.trace.push(json!(["context"]));self.trace.push(json!(["budget_task",node]));Ok(self.case["task_index"].as_u64().unwrap_or(7) as u32)}
        fn estimated_tasks_enabled(&mut self)->Result<bool>{self.trace.push(json!(["estimated_enabled"]));Ok(self.flag("estimated",false))}
        fn estimated_task(&mut self,node:usize)->Result<u32>{self.trace.push(json!(["context"]));self.trace.push(json!(["estimated_task",node]));Ok(self.case["task_index"].as_u64().unwrap_or(7) as u32)}
        fn strategy_class(&mut self,node:usize)->Result<Option<Vec<u8>>>{self.trace.push(json!(["strategy_class",node]));Ok(self.node(node)["class"].as_str().map(|s|s.as_bytes().to_vec()))}
        fn strategy_metadata(&mut self,name:&[u8])->Result<Option<usize>>{
            self.trace.push(json!(["metadata",std::str::from_utf8(name)?]));
            Ok((!self.flag("metadata_missing",false)&&super::super::ai_flat_handle_strategy::matches_class(name)).then_some(200))
        }
        fn create_strategy(&mut self,_:&mut Instance,metadata:usize)->Result<Option<usize>>{
            self.trace.push(json!(["strategy_create",metadata]));
            let result=super::super::ai_flat_handle_strategy::create(&mut StrategyAllocation{host:self})?;
            Ok(result.map(|(handle,state)|{self.flat=Some(state);handle}))
        }
        fn initialize_strategy(&mut self,_:&mut Instance,strategy:usize,node:usize)->Result<u32>{
            self.trace.push(json!(["strategy_init",strategy,node]));
            let mut flat=self.flat.take().context("Strategy instance absent")?;
            let result=flat.initialize(node,self)?;self.flat=Some(flat);Ok(u32::from(result))
        }
        fn touch_context(&mut self)->Result<()>{self.trace.push(json!(["context"]));Ok(())}
        fn allocate(&mut self,instance:&mut Instance,bytes:u32)->Result<Option<usize>> {
            self.trace.push(json!(["allocate",bytes]));let allocation=self.flag("allocated",true).then_some(402);
            if self.flag("observer",false){self.trace.push(json!(["observer",allocation,bytes,123]));}
            if let Some(size)=self.case["allocate_word60"].as_u64(){state_mut(instance)?.word_60=size as u32;}
            Ok(allocation)
        }
        fn construct_extraction_array(&mut self,_:&mut Instance,allocation:Option<usize>,count:u32)->Result<Option<usize>> {
            self.trace.push(json!(["construct",allocation,count]));
            if allocation.is_none(){return Ok(None)};
            self.entries=Some(extraction_entries(count)?);Ok(Some(202))
        }
    }
    #[test]
    fn original_goto_configuration_initialization_and_extraction_constructor_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goto_initialization.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let mut instance=super::super::ai_goto_traversal::construct(100);
            let mut host=Fixture{case:c,trace:vec![],children:c["root_count"].as_u64().unwrap_or(c["nodes"].as_array().unwrap().len() as u64) as u32,entries:None,flat:None};
            let s=state_mut(&mut instance)?;s.strategy=host.flag("strategy_initial",false).then_some(201);
            s.word_50=c["memory_initial"].as_u64().unwrap_or(0) as u32;s.reference_5c=host.flag("working_initial",false).then_some(99);
            let result=if c["op"]=="attribute" {attribute(&mut instance,10,&mut host)?}else{initialize(&mut instance,1,&mut host)?};
            let s=state(&instance)?;
            let actual=json!({"strategy":s.strategy,"word_c":s.word_c,"word_18":s.word_18,"values":s.values_30_34.map(f32::to_bits),"word50":s.word_50,"buffers":s.references_54_58,"word60":s.word_60,"working":s.reference_5c});
            assert_eq!(actual,expected["state"],"initialized state: {c}");
            assert_eq!(json!(result),expected["result"],"initialization result: {c}");
            assert_eq!(json!(host.trace),expected["trace"],"service order: {c}");
            let positions=host.entries.as_ref().map(|entries|entries.iter().map(|entry|{
                assert!(entry.word_0.is_none()&&entry.word_10.is_none());entry.position.map(f32::to_bits)
            }).collect::<Vec<_>>());
            assert_eq!(json!(positions),expected["positions"],"working-entry constructor: {c}");
        }
        Ok(())
    }
}
