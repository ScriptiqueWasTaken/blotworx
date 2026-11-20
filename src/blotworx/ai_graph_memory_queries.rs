//! Graph-chain queries and Astar seeding over the canonical linked buffers.
use anyhow::{Context as _,Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_graph_queries::{find_in_chain,Point,QueryStore,Metadata,Resources},ai_graph_handles::State,ai_graph_points::Table};
impl Memory {
    fn point_address(&self,point:Point)->Result<u32> {
        let view=self.view(point.resource)?;
        let base=match point.location.table {Table::Internal=>view.points,Table::External=>{ensure!(view.external!=0,"Graph point refers to an absent external table");view.external}};
        Ok(base.wrapping_add(point.location.offset))
    }
    pub fn bound_node_id(&self,state:&State,handle:usize)->Result<u32> {
        let point=state.nodes.get(&handle).context("Bound graph handle is absent")?.point;
        match point {None=>Ok(u32::MAX),Some(p)=>Ok(self.word(self.point_address(p)?)?&0x7fffffff)}
    }
}
impl QueryStore for Memory {
    fn find_point(&mut self,state:&mut State,root:Option<usize>,node:u32,metadata:&mut Metadata)->Result<bool> {
        find_in_chain(state,root,&mut |state,resource| {
            let kind=metadata.kind(state,resource)?;
            // Both concrete count getters read the live header word. Metadata's
            // fixture count cache is deliberately not a second source of truth.
            self.locate(resource,node,kind!=0)
        })
    }
    fn point_position(&self,point:Point)->Result<[u32;3]> {
        let at=self.point_address(point)?;
        Ok([self.word(at.wrapping_add(4))?,self.word(at.wrapping_add(8))?,self.word(at.wrapping_add(12))?])
    }
}
#[cfg(test)]
pub(super) fn fixture_memory(tables:super::ai_graph_queries::Tables,metadata:&Metadata)->Result<Memory> {
    use std::collections::BTreeMap;
    use super::ai_graph_link_passes::View;
    let mut memory=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride:36,journal:BTreeMap::new()};
    let mut headers=BTreeMap::new();let mut cursor=0x70000000u32;
    for (&key,&value) in &metadata.headers {headers.insert(key,cursor);memory.regions.insert(cursor,value.to_be_bytes().to_vec());cursor=cursor.checked_add(16).context("Fixture header address overflow")?;}
    cursor=0x73000000;
    for (id,data) in tables.resources {
        let points=cursor;
        cursor=cursor.checked_add(u32::try_from(data.internal.len())?.checked_add(16).context("Fixture buffer size overflow")?).context("Fixture address overflow")?;
        memory.regions.insert(points,data.internal);
        let external=if let Some(bytes)=data.external {
            let at=cursor;cursor=cursor.checked_add(u32::try_from(bytes.len())?.checked_add(16).context("Fixture external size overflow")?).context("Fixture external address overflow")?;
            memory.regions.insert(at,bytes);at
        }else{0};
        let header=metadata.resources.get(&id).and_then(|r|r.header).and_then(|key|headers.get(&key).copied()).unwrap_or(0);
        memory.resources.insert(id,View{auxiliary:None,payload:None,edges:None,edge_stride:None,header,points,external,external_count:data.external_count,stride:data.stride});
    }
    Ok(memory)
}
#[cfg(test)] mod tests {
    use super::*;
    use super::super::ai_graph_queries::{Tables,Class,ResourceMetadata};
    use serde_json::{Value,json};
    #[test] fn original_concrete_queries_match_using_canonical_buffers()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut state:State=serde_json::from_value(c["state"].clone())?;
            let tables:Tables=serde_json::from_value(c["tables"].clone())?;let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let mut memory=fixture_memory(tables,&metadata)?;let root=c["root"].as_u64().map(|n|n as usize);
            let result=match c["op"].as_str().unwrap(){"ready"=>state.ready(root)?,"marked"=>state.marked(root)?,"find"=>memory.find_point(&mut state,root,c["node"].as_u64().unwrap() as u32,&mut metadata)?,_=>unreachable!()};
            assert_eq!(json!(result),case["result"],"{c}");
            assert_eq!(json!(root.map(|root|memory.bound_node_id(&state,root)).transpose()?),case["node_id"],"{c}");
            assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
        }
        Ok(())
    }
    #[test] fn linked_buffer_writes_and_header_counts_are_immediately_visible()->Result<()> {
        use std::collections::BTreeMap;
        use super::super::ai_graph_link_passes::View;
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        let base=fixture["cases"].as_array().unwrap().iter().find(|c|c["input"]["state"]["nodes"].get("100").is_some()).unwrap();
        let mut state:State=serde_json::from_value(base["input"]["state"].clone())?;
        state.resources.get_mut(&1).unwrap().state=5;let root=state.nodes.get_mut(&100).unwrap();root.word14=1;root.next=None;root.point=None;root.graph=None;
        let mut bytes=vec![0;64];bytes[..4].copy_from_slice(&7u32.to_be_bytes());bytes[32..36].copy_from_slice(&9u32.to_be_bytes());
        let mut memory=Memory{regions:BTreeMap::from([(100,1u32.to_be_bytes().to_vec()),(200,bytes)]),resources:BTreeMap::from([(1,View{auxiliary:None,payload:None,edges:None,edge_stride:None,header:100,points:200,external:0,external_count:0,stride:32})]),stride:32,journal:BTreeMap::new()};
        let mut metadata=Metadata::default();metadata.resources.insert(1,ResourceMetadata{class:Class::Concrete,header:None});
        assert!(memory.find_point(&mut state,Some(100),7,&mut metadata)?);
        memory.write(204,0x7f812345)?;
        assert_eq!(memory.point_position(state.nodes[&100].point.unwrap())?,[0x7f812345,0,0]);
        assert!(!memory.find_point(&mut state,Some(100),9,&mut metadata)?);
        assert_eq!(memory.bound_node_id(&state,100)?,7);
        memory.write(100,2)?;
        assert!(memory.find_point(&mut state,Some(100),9,&mut metadata)?);
        memory.write(232,11)?;assert_eq!(memory.bound_node_id(&state,100)?,11);
        Ok(())
    }
}
#[cfg(test)] mod owned_tests {
    use super::*;
    use anyhow::ensure;
    use super::super::{ai_graph_admission::Admission,ai_graph_handles::Resource,ai_graph_link_passes::View,ai_graph_linking::Pass,ai_graph_normalization::{convert,Layout},ai_graph_queries::{Class,ResourceMetadata}};
    use std::{collections::BTreeMap,path::PathBuf};
    use serde_json::{Value,json};
    fn hash(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|v|format!("{v:02x}")).collect()}
    #[test]
    #[ignore="Requires immutable imported Epic Mickey data via BLOTWORX_TEST_DATA"]
    fn original_owned_gv_chain_queries_use_live_validated_buffers()->Result<()> {
        let root=PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("BLOTWORX_TEST_DATA is required")?);
        let source:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_rings_owned.json"))?;
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_memory_queries_owned.json"))?;
        let stride=source["stride"].as_u64().unwrap() as u32;
        let mut memory=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride,journal:BTreeMap::new()};
        for r in source["records"].as_array().unwrap() {
            let original=std::fs::read(root.join(r["path"].as_str().unwrap()))?;
            ensure!(hash(&original)==r["source_sha256"].as_str().unwrap(),"Owned graph source changed");
            let normalized=convert(&original[16..],Layout::Dynamic{node_stride:stride,edge_stride:12})?;
            let id=r["id"].as_u64().unwrap() as u32;let header=r["header"].as_u64().unwrap() as u32;
            memory.resources.insert(id,View{auxiliary:None,payload:None,edges:None,edge_stride:None,header,points:header+8,external:normalized.external.map(|o|header.wrapping_add(o)).unwrap_or(0),external_count:normalized.external_count,stride});
            memory.regions.insert(header,normalized.bytes);
        }
        let order:Vec<u32>=serde_json::from_value(source["runs"][0]["order"].clone())?;
        let attributes=serde_json::from_value(source["attributes"].clone())?;
        let mut admission=Admission{index154:0,index15c:0,word82e4:0,changed14:0,listeners:Vec::new()};
        memory.splice_external(order[0],true)?;memory.convert_vertex_triplets(order[0],&attributes)?;
        memory.run(&mut admission,order[1],order[0],Pass::DynamicPeers)?;
        memory.splice_external(order[1],true)?;memory.convert_vertex_triplets(order[1],&attributes)?;
        let template:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        let base=template["cases"].as_array().unwrap().iter().find(|c|c["input"]["state"]["nodes"].get("100").is_some() && c["input"]["state"]["nodes"].get("101").is_some()).unwrap();
        let mut graph:State=serde_json::from_value(base["input"]["state"].clone())?;
        graph.resources.clear();let mut metadata=Metadata::default();let chain:Vec<u32>=serde_json::from_value(fixture["chain"].clone())?;
        for (i,&id) in chain.iter().enumerate() {
            graph.resources.insert(id,Resource{size18:None,byte8:0,priority:0,state:5,word1c:id});
            metadata.resources.insert(id,ResourceMetadata{class:Class::Concrete,header:None});
            let node=graph.nodes.get_mut(&(100+i)).unwrap();node.word14=id;node.point=None;node.graph=None;node.next=if i+1<chain.len(){Some(101+i)}else{None};
        }
        for q in fixture["queries"].as_array().unwrap() {
            let found=memory.find_point(&mut graph,Some(100),q["node"].as_u64().unwrap() as u32,&mut metadata)?;
            assert_eq!(json!(found),q["found"],"{q}");
            let binding=&graph.nodes[&100];assert_eq!(json!({"point":binding.point,"graph":binding.graph}),q["binding"],"{q}");
            assert_eq!(json!(memory.bound_node_id(&graph,100)?),q["node_id"],"{q}");
            let position=binding.point.map(|p|memory.point_position(p)).transpose()?.map(|values|{let mut bytes=[0;12];for (i,v) in values.iter().enumerate(){bytes[i*4..i*4+4].copy_from_slice(&v.to_be_bytes());}hash(&bytes)});
            assert_eq!(json!(position),q["position_sha256"],"{q}");
        }
        for r in source["records"].as_array().unwrap() {
            let header=r["header"].as_u64().unwrap() as u32;let count=r["point_count"].as_u64().unwrap() as usize;
            assert_eq!(hash(&memory.regions[&header][8..8+count*stride as usize]),r["live_points_sha256"].as_str().unwrap());
            assert_eq!(hash(&std::fs::read(root.join(r["path"].as_str().unwrap()))?),r["source_sha256"].as_str().unwrap());
        }
        Ok(())
    }
}
