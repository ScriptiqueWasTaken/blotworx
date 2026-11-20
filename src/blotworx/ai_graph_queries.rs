//! Graph-chain lookup47fd10 and bound ID47fc74 using concrete point tables.
use anyhow::{Context as _,Result,ensure};
use std::collections::{BTreeMap,BTreeSet};
use super::{ai_graph_handles::State,ai_graph_points::{Data,Layout,Location}};
#[derive(Clone,Copy,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct Point {pub resource:u32,pub location:Location}
#[derive(Default,serde::Serialize,serde::Deserialize)]
pub struct Tables {pub resources:BTreeMap<u32,Data>}
pub trait Resources {
    fn kind(&mut self,state:&mut State,resource:u32)->Result<u32>;
    fn count(&mut self,state:&mut State,resource:u32,data:&mut Data)->Result<u32>;
}
#[derive(Clone,Copy,serde::Serialize,serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Class {Abstract,Concrete}
#[derive(serde::Serialize,serde::Deserialize)]
pub struct ResourceMetadata {pub class:Class,pub header:Option<usize>}
#[derive(Default,serde::Serialize,serde::Deserialize)]
pub struct Metadata {pub resources:BTreeMap<u32,ResourceMetadata>,pub headers:BTreeMap<usize,u32>}
impl Resources for Metadata {
    fn kind(&mut self,_:&mut State,resource:u32)->Result<u32> {
        // Vtables51ea08/51ea58 ->482eb4/482eac.
        Ok(match self.resources.get(&resource).context("Graph resource class metadata is absent")?.class {Class::Abstract=>0,Class::Concrete=>1})
    }
    fn count(&mut self,_:&mut State,resource:u32,_:&mut Data)->Result<u32> {
        //475108/47505c both load resource28 -> word0.
        let header=self.resources.get(&resource).context("Graph resource count metadata is absent")?.header.context("Graph resource has no count header")?;
        self.headers.get(&header).copied().context("Graph resource count header is absent")
    }
}
/// Shared chain traversal preserves fresh links after lookup callbacks.
pub(super) fn find_in_chain(state:&mut State,root:Option<usize>,lookup:&mut impl FnMut(&mut State,u32)->Result<Option<Location>>)->Result<bool> {
        let Some(root)=root else{return Ok(false)};let mut handle=Some(root);let mut visited=BTreeSet::new();
        while let Some(id)=handle {
            ensure!(visited.insert(id),"Cyclic graph lookup chain");
            let resource=state.nodes.get(&id).context("Graph lookup handle is absent")?.word14;
            if resource!=0 && state.resources.get(&resource).context("Graph lookup resource is absent")?.state==5 {
                let location=lookup(state,resource)?;
                if let Some(location)=location {
                    let root=state.nodes.get_mut(&root).context("Graph lookup callback removed its destination handle")?;
                    root.point=Some(Point{resource,location});root.graph=Some(resource);
                    return Ok(true)
                }
            }
            handle=state.nodes.get(&id).context("Graph lookup callback removed its chain handle")?.next;
        }
        Ok(false)
}
/// Point queries can read either fixture tables or the canonical shared buffers.
pub trait QueryStore {
    fn find_point(&mut self,state:&mut State,root:Option<usize>,node:u32,metadata:&mut Metadata)->Result<bool>;
    fn point_position(&self,point:Point)->Result<[u32;3]>;
}
impl QueryStore for Tables {
    fn find_point(&mut self,state:&mut State,root:Option<usize>,node:u32,metadata:&mut Metadata)->Result<bool>{self.find(state,root,node,metadata)}
    fn point_position(&self,point:Point)->Result<[u32;3]>{self.position(point)}
}
impl Tables {
    pub fn find(&mut self,state:&mut State,root:Option<usize>,node:u32,host:&mut impl Resources)->Result<bool> {
        find_in_chain(state,root,&mut |state,resource| {
            let kind=host.kind(state,resource)?;
            let data=self.resources.get_mut(&resource).context("Graph lookup resource has no point tables")?;
            data.find(if kind==0 {Layout::Fixed32} else {Layout::Dynamic},node,&mut |data|host.count(state,resource,data))
        })
    }

    pub fn node_id(&self,state:&State,handle:usize)->Result<u32> {
        let point=state.nodes.get(&handle).context("Bound graph handle is absent")?.point;
        match point {None=>Ok(u32::MAX),Some(p)=>self.resources.get(&p.resource).context("Bound graph point resource is absent")?.node_id(p.location)}
    }
    pub fn position(&self,point:Point)->Result<[u32;3]> {
        self.resources.get(&point.resource).context("Graph point position resource is absent")?.position(point.location)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>}
    impl Resources for Fixture<'_> {
        fn kind(&mut self,state:&mut State,resource:u32)->Result<u32> {
            self.trace.push(json!(["kind",resource]));
            if let Some(value)=self.c["kind_state"].as_u64(){state.resources.get_mut(&resource).unwrap().state=value as u32;}
            Ok(self.c["kinds"][resource.to_string()].as_u64().unwrap() as u32)
        }
        fn count(&mut self,state:&mut State,resource:u32,_:&mut Data)->Result<u32> {
            self.trace.push(json!(["count",resource]));
            if self.c["clear_next"].as_bool().unwrap_or(false){state.nodes.get_mut(&100).unwrap().next=None;}
            Ok(self.c["totals"][resource.to_string()].as_u64().unwrap() as u32)
        }
    }
    #[test]
    fn original_graph_chain_queries_bindings_and_concrete_point_search_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_queries.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut state:State=serde_json::from_value(c["state"].clone())?;
            let mut tables:Tables=serde_json::from_value(c["tables"].clone())?;
            let root=c["root"].as_u64().map(|n|n as usize);let mut host=Fixture{c,trace:Vec::new()};
            let result=match c["op"].as_str().unwrap(){
                "ready"=>state.ready(root)?,"marked"=>state.marked(root)?,
                "find"=>tables.find(&mut state,root,c["node"].as_u64().unwrap() as u32,&mut host)?,_=>unreachable!()
            };
            assert_eq!(json!(result),case["result"],"{c}");
            assert_eq!(host.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            let id=root.map(|root|tables.node_id(&state,root)).transpose()?;
            assert_eq!(json!(id),case["node_id"],"{c}");
            assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn original_graph_queries_with_concrete_kind_and_count_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut state:State=serde_json::from_value(c["state"].clone())?;
            let mut tables:Tables=serde_json::from_value(c["tables"].clone())?;
            let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let root=c["root"].as_u64().map(|n|n as usize);
            let result=match c["op"].as_str().unwrap(){"ready"=>state.ready(root)?,"marked"=>state.marked(root)?,"find"=>tables.find(&mut state,root,c["node"].as_u64().unwrap() as u32,&mut metadata)?,_=>unreachable!()};
            assert_eq!(json!(result),case["result"],"{c}");
            assert_eq!(json!(root.map(|root|tables.node_id(&state,root)).transpose()?),case["node_id"],"{c}");
            assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
        }
        Ok(())
    }
}
