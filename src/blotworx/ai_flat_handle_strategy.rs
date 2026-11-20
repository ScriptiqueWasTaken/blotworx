//! Mpd::CFlatHandleStrategy: factory474a34, ctor474aac, init474b18,
//! refresh474bc0 and clear474c34. Actual graph/owner operations are required.
use anyhow::{Context as _,Result};

pub const CLASS:&[u8]=b"Mpd::CFlatHandleStrategy";
pub fn matches_class(name:&[u8])->bool {
    &name[..name.iter().position(|&b|b==0).unwrap_or(name.len())]==CLASS
}
#[derive(Default,Debug,serde::Serialize)]
pub struct State {
    pub manager:Option<usize>, //4
    pub owner:Option<usize>, //8
    pub handle:Option<usize>, //c
}
pub trait AllocationHost {fn allocate(&mut self,bytes:u32)->Result<Option<usize>>;}
pub fn create(host:&mut impl AllocationHost)->Result<Option<(usize,State)>> {
    Ok(host.allocate(0x10)?.map(|handle|(handle,State::default())))
}
pub trait Configuration {
    fn child_count(&mut self,root:usize)->Result<u32>;
    fn child(&mut self,root:usize,index:u32)->Result<usize>;
    fn name(&self,node:usize)->Result<Vec<u8>>;
}
pub trait Operations {
    fn release(&mut self,state:&mut State,manager:usize,handle:usize)->Result<()>;
    fn owner_value_64(&mut self,state:&mut State,owner:usize)->Result<Option<usize>>;
    fn acquire(&mut self,state:&mut State,manager:usize,value:usize)->Result<Option<usize>>;
}
/// Concrete graph services; only the owner's virtual64 query remains a
/// required provider. Manager and node identities are portable arena keys.
pub struct GraphOperations<'a,Q> {
    pub graphs:&'a mut std::collections::BTreeMap<usize,super::ai_graph_handles::State>,
    pub query:Q,
}
impl<Q:FnMut(&mut State,usize)->Result<Option<usize>>> Operations for GraphOperations<'_,Q> {
    fn release(&mut self,_:&mut State,manager:usize,handle:usize)->Result<()> {
        self.graphs.get_mut(&manager).context("Flat strategy graph manager is not in its arena")?.release(Some(handle))?;
        Ok(())
    }
    fn owner_value_64(&mut self,state:&mut State,owner:usize)->Result<Option<usize>> {(self.query)(state,owner)}
    fn acquire(&mut self,_:&mut State,manager:usize,value:usize)->Result<Option<usize>> {
        self.graphs.get_mut(&manager).context("Flat strategy graph manager is not in its arena")?.acquire(Some(value))
    }
}
impl State {
    pub fn initialize(&mut self,root:usize,host:&mut impl Configuration)->Result<bool> {
        let mut index=0;
        while index<host.child_count(root)? {
            let node=host.child(root,index)?;
            // Virtual10 is474bb8, which returns zero for every attribute.
            if !super::ai_traversal_registry::names_equal(&host.name(node)?,b"Class") {return Ok(false)};
            index+=1;
        }
        Ok(true)
    }
    pub fn refresh(&mut self,host:&mut impl Operations)->Result<Option<usize>> {
        if let Some(old)=self.handle {
            let manager=self.manager.context("Flat handle release requires its graph manager")?;
            host.release(self,manager,old)?;
            self.handle=None;
        }
        let owner=self.owner.context("Flat handle query requires its bound owner")?;
        if let Some(value)=host.owner_value_64(self,owner)? {
            let manager=self.manager.context("Flat handle acquisition requires its graph manager")?;
            self.handle=host.acquire(self,manager,value)?;
        }
        // A null query result does not overwrite mutations to c during query.
        Ok(self.handle)
    }
    pub fn clear(&mut self,host:&mut impl Operations)->Result<()> {
        if let Some(old)=self.handle {
            let manager=self.manager.context("Flat handle clear requires its graph manager")?;
            host.release(self,manager,old)?;self.handle=None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    #[test]
    fn original_flat_strategy_with_concrete_graph_services_matches()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_flat_graph.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];
            let graph=serde_json::from_value(c["state"].clone())?;
            let mut graphs=std::collections::BTreeMap::from([(88,graph)]);
            let source=c["source"].as_u64().map(|n|n as usize);
            let mut flat=State{manager:Some(88),owner:Some(44),handle:c["flat_handle"].as_u64().map(|n|n as usize)};
            let mut host=GraphOperations{graphs:&mut graphs,query:|_:&mut State,owner|{anyhow::ensure!(owner==44,"Wrong owner query");Ok(source)}};
            let result=if c["op"]=="flat_refresh" {serde_json::to_value(flat.refresh(&mut host)?)?} else {flat.clear(&mut host)?;Value::Null};
            assert_eq!(result,case["result"],"{c}");
            assert_eq!(serde_json::to_value(flat.handle)?,case["flat_handle"],"{c}");
            assert_eq!(serde_json::to_value(&graphs[&88])?,case["state"],"{c}");
        }
        Ok(())
    }
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Fixture<'_> {
        fn mutate(&mut self,state:&mut State,phase:&str) {
            if let Some(fields)=self.case[phase].as_object(){for (key,value) in fields {
                let value=value.as_u64().map(|v|v as usize);
                match key.as_str(){"manager"=>state.manager=value,"owner"=>state.owner=value,"handle"=>state.handle=value,_=>panic!("Unknown fixture field {key}")}
            }}
        }
    }
    impl AllocationHost for Fixture<'_> {
        fn allocate(&mut self,bytes:u32)->Result<Option<usize>> {
            self.trace.push(json!(["allocate",bytes]));let result=self.case["allocated"].as_bool().unwrap_or(true).then_some(100);
            if self.case["observer"].as_bool().unwrap_or(false){self.trace.push(json!(["observer",result,bytes,123]));}
            Ok(result)
        }
    }
    impl Configuration for Fixture<'_> {
        fn child_count(&mut self,_:usize)->Result<u32>{Ok(self.case["names"].as_array().map_or(0,|n|n.len()) as u32)}
        fn child(&mut self,_:usize,index:u32)->Result<usize>{self.trace.push(json!(["child",index]));Ok(10+index as usize)}
        fn name(&self,node:usize)->Result<Vec<u8>>{Ok(self.case["names"][node-10].as_str().unwrap().as_bytes().to_vec())}
    }
    impl Operations for Fixture<'_> {
        fn release(&mut self,state:&mut State,manager:usize,handle:usize)->Result<()> {self.trace.push(json!(["release",manager,handle]));self.mutate(state,"release_mutation");Ok(())}
        fn owner_value_64(&mut self,state:&mut State,owner:usize)->Result<Option<usize>> {self.trace.push(json!(["owner_value",owner]));self.mutate(state,"owner_mutation");Ok(self.case["value"].as_u64().map(|v|v as usize))}
        fn acquire(&mut self,state:&mut State,manager:usize,value:usize)->Result<Option<usize>> {self.trace.push(json!(["acquire",manager,value]));self.mutate(state,"acquire_mutation");Ok(self.case["acquired"].as_u64().map(|v|v as usize))}
    }
    #[test]
    fn original_flat_factory_configuration_refresh_and_clear_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_flat_handle_strategy.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let mut host=Fixture{case:c,trace:vec![]};
            let mut state=State{manager:c["manager"].as_u64().map(|v|v as usize),owner:c["owner"].as_u64().map(|v|v as usize),handle:c["handle"].as_u64().map(|v|v as usize)};
            let mut result=Value::Null;let mut available=true;
            match c["op"].as_str().unwrap(){
                "factory"=>match create(&mut host)? {Some((handle,created))=>{assert_eq!(handle,100);state=created;},None=>available=false},
                "initialize"=>result=json!(state.initialize(1,&mut host)?),
                "refresh"=>result=json!(state.refresh(&mut host)?),
                "clear"=>state.clear(&mut host)?,_=>unreachable!(),
            }
            assert_eq!(json!(host.trace),expected["trace"],"service trace: {c}");
            assert_eq!(json!(available),expected["available"],"allocation result: {c}");
            assert_eq!(if available {json!(state)}else{Value::Null},expected["state"],"flat strategy fields: {c}");
            assert_eq!(result,expected["result"],"result: {c}");
        }
        Ok(())
    }
    #[test]
    fn missing_required_bindings_are_errors_without_false_handle_success()->Result<()> {
        let c=json!({"value":66});let mut host=Fixture{case:&c,trace:vec![]};let mut state=State::default();
        assert!(state.refresh(&mut host).is_err());assert!(host.trace.is_empty());
        state.owner=Some(44);assert!(state.refresh(&mut host).is_err());assert_eq!(state.handle,None);
        state.handle=Some(100);assert!(state.clear(&mut host).is_err());assert_eq!(state.handle,Some(100));
        Ok(())
    }
}
