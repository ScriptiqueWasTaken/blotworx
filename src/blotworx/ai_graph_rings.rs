//! External ring splice475c20/475cd0 and internal resolution475d78/475eb0.
//! Reference-conversion helpers are identity in the owned REL and observed loaded code.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::ai_graph_link_passes::Memory;
impl Memory {
    ///476b3c: first type2 descriptor controls three halfwords in each vertex.
    pub fn convert_vertex_triplets(&mut self,resource:u32,attributes:&[[u32;3];4])->Result<()> {
        let Some(attribute)=attributes.iter().find(|a|a[1]==2) else{return Ok(())};
        let offset=attribute[2];let view=self.view(resource)?;let mut index=0u32;
        while index<self.word(view.header)? {
            let at=view.points.wrapping_add(index.wrapping_mul(self.stride)).wrapping_add(28).wrapping_add(offset);
            for field in [0,2,4] {self.swap_halfword(at.wrapping_add(field))?;}
            index=index.wrapping_add(1);
        }
        Ok(())
    }
    pub fn splice_external(&mut self,resource:u32,dynamic:bool)->Result<()> {
        let view=self.view(resource)?;let mut at=view.external;
        let field=if dynamic {20}else{24};
        for _ in 0..view.external_count {
            let other=self.word(at.wrapping_add(field))?;
            if other!=0 {
                let next=self.word(other.wrapping_add(field))?;
                self.write(at.wrapping_add(field),next)?;
                if next==0 {self.write(at.wrapping_add(field),other)?;}
                self.write(other.wrapping_add(field),at)?;
            }else {self.write(at.wrapping_add(field),at)?;}
            at=at.wrapping_add(if dynamic {self.stride}else{32});
        }
        Ok(())
    }
    pub fn resolve_internal(&mut self,resource:u32,dynamic:bool)->Result<()> {
        let view=self.view(resource)?;let mut at=view.points;let count=self.word(view.header)?;
        for _ in 0..count {
            let parent=self.word(at.wrapping_add(16))?;
            if parent&0x80000000==0 {
                let link=self.word(parent.wrapping_add(20))?;
                if link&0x80000000!=0 && (link&0x7fffffff)==(self.word(at)?&0x7fffffff) {
                    self.write(parent.wrapping_add(20),at)?;
                    // Both the parent and ring cursor are re-read after writes.
                    let parent=self.word(at.wrapping_add(16))?;
                    let mut cursor=self.word(parent.wrapping_add(24))?;
                    let mut visited=BTreeSet::new();
                    while cursor!=0 && cursor!=self.word(at.wrapping_add(16))? {
                        ensure!(visited.insert(cursor),"Navigation ring cycles without reaching its owner");
                        if self.word(cursor.wrapping_add(20))?&0x80000000!=0 {self.write(cursor.wrapping_add(20),at)?;}
                        cursor=self.word(cursor.wrapping_add(24))?;
                    }
                }
            }
            at=at.wrapping_add(if dynamic {self.stride}else{32});
        }
        Ok(())
    }
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::Value;
    const CASES:&str=include_str!("../../tools/blotworx/fixtures/ai_graph_rings.json");
    #[test] fn original_dynamic_fixed_ring_splicing_and_resolution_match()->Result<()> {
        let fixture:Value=serde_json::from_str(CASES)?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut memory:Memory=serde_json::from_value(c["memory"].clone())?;
            let op=c["op"].as_str().unwrap();
            if op.starts_with("splice") {memory.splice_external(10,op.ends_with("dynamic"))?;}else{memory.resolve_internal(10,op.ends_with("dynamic"))?;}
            assert_eq!(serde_json::to_value(memory)?,case["memory"],"{c}");
        }
        Ok(())
    }
    #[test] fn malformed_ring_reports_a_cycle_instead_of_hanging()->Result<()> {
        let fixture:Value=serde_json::from_str(CASES)?;
        let c=&fixture["cases"].as_array().unwrap().iter().find(|c|c["input"]["op"]=="resolve_dynamic" && c["input"]["memory"]["resources"]["10"]["external_count"]==2).unwrap()["input"];
        let mut memory:Memory=serde_json::from_value(c["memory"].clone())?;
        memory.write(0x14000000+memory.stride+24,0x14000000)?;
        let error=memory.resolve_internal(10,true).unwrap_err();
        assert!(error.to_string().contains("cycles without reaching its owner"));Ok(())
    }
}
#[cfg(test)] mod triplet_tests {
    use super::*;
    use serde_json::Value;
    #[test] fn original_first_descriptor_triplets_offsets_counts_and_zero_stride_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_triplets.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut memory:Memory=serde_json::from_value(c["memory"].clone())?;
            let attributes=serde_json::from_value(c["attributes"].clone())?;
            memory.convert_vertex_triplets(10,&attributes)?;
            assert_eq!(serde_json::to_value(memory)?,case["memory"],"{c}");
        }
        Ok(())
    }
    #[test]
    #[ignore="Requires immutable imported Epic Mickey data via BLOTWORX_TEST_DATA"]
    fn original_owned_gv_normalization_link_and_ring_nodes_match_live_game_hashes()->Result<()> {
        use super::super::{ai_graph_normalization::{convert,Layout},ai_graph_admission::Admission,ai_graph_linking::Pass,ai_graph_link_passes::View};
        use std::{collections::BTreeMap,path::PathBuf};
        use anyhow::{Context as _,ensure};
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_rings_owned.json"))?;
        let root=PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("BLOTWORX_TEST_DATA is required")?);
        let hash=|bytes:&[u8]|ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|v|format!("{v:02x}")).collect::<String>();
        for run in fixture["runs"].as_array().unwrap() {
            let stride=fixture["stride"].as_u64().unwrap() as u32;
            let mut memory=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride,journal:BTreeMap::new()};
            for r in fixture["records"].as_array().unwrap() {
                let source=std::fs::read(root.join(r["path"].as_str().unwrap()))?;
                ensure!(hash(&source)==r["source_sha256"].as_str().unwrap(),"Owned graph source changed");
                let normalized=convert(&source[16..],Layout::Dynamic{node_stride:stride,edge_stride:12})?;
                let id=r["id"].as_u64().unwrap() as u32;let header=r["header"].as_u64().unwrap() as u32;
                let external=normalized.external.map(|offset|header.wrapping_add(offset)).unwrap_or(0);
                assert_eq!(normalized.node_count,r["point_count"].as_u64().unwrap() as u32);
                assert_eq!(external,r["external"].as_u64().unwrap() as u32);
                assert_eq!(normalized.external_count,r["external_count"].as_u64().unwrap() as u32);
                memory.resources.insert(id,View{auxiliary:None,payload:None,edges:None,edge_stride:None,header,points:header+8,external,external_count:normalized.external_count,stride});
                memory.regions.insert(header,normalized.bytes);
            }
            let order:Vec<u32>=serde_json::from_value(run["order"].clone())?;
            let attributes=serde_json::from_value(fixture["attributes"].clone())?;
            let mut admission=Admission{index154:0,index15c:0,word82e4:0,changed14:0,listeners:Vec::new()};
            memory.splice_external(order[0],true)?;
            memory.convert_vertex_triplets(order[0],&attributes)?;
            memory.run(&mut admission,order[1],order[0],Pass::DynamicPeers)?;
            memory.splice_external(order[1],true)?;
            memory.convert_vertex_triplets(order[1],&attributes)?;
            for r in fixture["records"].as_array().unwrap() {
                let header=r["header"].as_u64().unwrap() as u32;let bytes=&memory.regions[&header];
                let count=r["point_count"].as_u64().unwrap() as usize;
                assert_eq!(hash(&bytes[8..8+count*stride as usize]),r["live_points_sha256"].as_str().unwrap(),"{r}");
                assert_eq!(hash(&std::fs::read(root.join(r["path"].as_str().unwrap()))?),r["source_sha256"].as_str().unwrap());
            }
        }
        Ok(())
    }
}
