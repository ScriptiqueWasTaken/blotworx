//!476da4 graph finalization, preserving native stage order and canonical buffers.
use super::{
    ai_graph_admission::Admission, ai_graph_handles::State, ai_graph_link_passes::Memory,
    ai_graph_loading::Loader, ai_mesh_provider, ai_runtime_mesh,
};
use anyhow::{ensure, Context as _, Result};
use std::collections::BTreeSet;
pub struct Context {
    /// The one canonical manager record, shared with traversal registration.
    pub manager: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesh {
    NotRequested,
    Loaded(u32),
    Rejected(u32),
}
pub trait Host {
    fn kind(&mut self, m: &mut Memory, resource: u32) -> Result<u32>;
    ///487b18/48761c are concrete policy registration dispatch, not a no-op.
    fn tag_edge(
        &mut self,
        m: &mut Memory,
        c: &mut Context,
        dynamic: bool,
        resource: u32,
        index: u32,
        edge: u32,
    ) -> Result<()>;
}
fn word(m: &Memory, p: u32, o: u32) -> Result<u32> {
    m.word(p.wrapping_add(o))
}
fn backrefs(m: &mut Memory, r: u32, dynamic_parent: bool) -> Result<()> {
    let view = m.view(r)?;
    let count = m.word(view.header)?;
    let mut at = view.points;
    for _ in 0..count {
        let parent = word(m, at, 20)?;
        if parent & 0x80000000 == 0 {
            m.write(parent.wrapping_add(16), at)?;
            let mut next = word(m, parent, if dynamic_parent { 20 } else { 24 })?;
            let mut seen = BTreeSet::new();
            while next != 0 && next != parent {
                ensure!(
                    seen.insert(next),
                    "Cycle in fixed graph back-reference ring"
                );
                if word(m, next, 16)? & 0x80000000 != 0 {
                    m.write(next.wrapping_add(16), at)?;
                }
                next = word(m, next, if dynamic_parent { 20 } else { 24 })?;
            }
        }
        at = at.wrapping_add(32);
    }
    Ok(())
}
fn tags(m: &mut Memory, c: &mut Context, r: u32, dynamic: bool, h: &mut impl Host) -> Result<()> {
    let view = m.view(r)?;
    let count = m.word(view.header.wrapping_add(4))?;
    let mut edge = view.edges.context("Finalization has no bound edge table")?;
    for _ in 0..count {
        let flags = m.word(edge)?;
        if flags & 0x7fff0000 != 0 {
            h.tag_edge(m, c, dynamic, r, (flags >> 16) & 0x7fff, edge)?;
        }
        let stride = if dynamic { word(m,c.manager,0x21c)? } else { 16 };
        edge = edge.wrapping_add(stride);
    }
    Ok(())
}
pub fn finalize(
    m: &mut Memory,
    a: &mut Admission,
    l: &mut Loader,
    s: &mut State,
    c: &mut Context,
    r: u32,
    h: &mut impl Host,
) -> Result<Mesh> {
    if h.kind(m, r)? == 1 {
        m.stride=word(m,c.manager,0x218)?;
        m.splice_external(r, true)?;
        m.resolve_internal(r, true)?;
        if word(m,word(m,c.manager,0x8534)?,0x1c)?!=0 {
            tags(m, c, r, true, h)?;
        }
        let mut attributes=[[0;3];4];
        for (i,row) in attributes.iter_mut().enumerate() {for (j,value) in row.iter_mut().enumerate(){*value=word(m,c.manager,0x1b4+i as u32*12+j as u32*4)?;}}
        m.stride=word(m,c.manager,0x218)?;
        m.convert_vertex_triplets(r, &attributes)?;
        if m.byte(c.manager.wrapping_add(0x48))?==0 {
            return Ok(Mesh::NotRequested);
        }
        let key = s
            .resources
            .get(&r)
            .context("Finalized graph resource is absent")?
            .word1c;
        let bytes = l
            .records
            .get(&key)
            .context("Finalized directory metadata is absent")?
            .size20;
        let base = m
            .view(r)?
            .payload
            .context("Finalized graph payload is unbound")?;
        let end = base.wrapping_add(bytes);
        let length = m.word(end.wrapping_sub(8))?.swap_bytes();
        let index = l.groups[0]
            .as_ref()
            .context("Dynamic graph group is absent")?
            .resources
            .iter()
            .position(|&id| id == r)
            .context("Finalized resource is outside its dynamic group")?;
        if length == 0 || length == 0x80000000 {
            return Ok(Mesh::NotRequested);
        }
        let source = end.wrapping_sub(8).wrapping_sub(length);
        let sector = ai_runtime_mesh::load(m, source)?;
        let slots=word(m,c.manager,0x44)?;
        m.write(slots.wrapping_add(index as u32*4),sector)?;
        let slots=word(m,c.manager,0x44)?;
        let sector=word(m,slots,index as u32*4)?;
        if sector == 0 {
            return Ok(Mesh::Rejected(source));
        }
        let provider=word(m,c.manager,0x4c)?;
        ai_mesh_provider::begin(m, provider)?;
        let provider=word(m,c.manager,0x4c)?;
        let slots=word(m,c.manager,0x44)?;
        let sector=word(m,slots,index as u32*4)?;
        ai_mesh_provider::set(m, provider, index as u32, sector)?;
        let provider=word(m,c.manager,0x4c)?;
        ai_mesh_provider::finish(m, provider)?;
        Ok(Mesh::Loaded(sector))
    } else {
        m.splice_external(r, false)?;
        m.resolve_internal(r, false)?;
        let key = s
            .resources
            .get(&r)
            .context("Finalized fixed graph is absent")?
            .word1c;
        let level = l
            .records
            .get(&key)
            .context("Fixed graph metadata is absent")?
            .word1c;
        backrefs(m, r, level == 1)?;
        if word(m,word(m,c.manager,0x8534)?,0x1c)?!=0 {
            tags(m, c, r, false, h)?;
        }
        m.apply_journal(a)?;
        Ok(Mesh::NotRequested)
    }
}

#[cfg(test)]
pub(super) fn test_context(m:&mut Memory,attributes:[[u32;3];4],tags:bool,mesh:bool,slot:u32,provider:u32)->Result<Context>{
    let manager=0x61000000;
    for (p,n) in [(manager,0x8600),(0x6e000000,32),(0x6d000000,4)] {ensure!(!m.regions.contains_key(&p),"Test manager overlaps storage");m.regions.insert(p,vec![0;n]);}
    for (i,row) in attributes.iter().enumerate(){for(j,v)in row.iter().enumerate(){m.write(manager+0x1b4+i as u32*12+j as u32*4,*v)?;}}
    for (off,value) in [(0x218,36),(0x21c,12),(0x8534,0x6e000000),(0x44,0x6d000000),(0x4c,provider)] {m.write(manager+off,value)?;}
    m.write_byte(manager+0x48,u8::from(mesh))?;m.write(0x6e00001c,u32::from(tags))?;m.write(0x6d000000,slot)?;Ok(Context{manager})
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_graph_loading::Record,ai_graph_resource_group::Group};
    use serde_json::Value;
    use std::collections::BTreeMap;
    struct Callbacks {kind:u32,change:bool,calls:Vec<(bool,u32,u32)>}
    impl Host for Callbacks {
        fn kind(&mut self,_:&mut Memory,_:u32)->Result<u32>{Ok(self.kind)}
        fn tag_edge(&mut self,m:&mut Memory,c:&mut Context,d:bool,_:u32,i:u32,e:u32)->Result<()> {
            self.calls.push((d,i,e)); if self.change {m.write(c.manager+0x21c,24)?;} Ok(())
        }
    }
    #[test]
    fn original_finalization_layouts_tags_backrefs_and_mesh_branches_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_finalize.json"))?;
        let template:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_metadata.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let mut m:Memory=serde_json::from_value(input["memory"].clone())?;
            let mut s:State=serde_json::from_value(template["cases"][0]["input"]["state"].clone())?;
            s.resources.clear();
            s.resources.insert(10,super::super::ai_graph_handles::Resource{size18:None,byte8:0,priority:0,state:4,word1c:1});
            let mut l=Loader{source:1,direct_mode:1,name:vec![],extra_records:0,directory:vec![1],records:BTreeMap::from([(1,Record{id:10,word1c:input["level"].as_u64().unwrap() as u32,size20:input["size"].as_u64().unwrap() as u32,offset24:0,word28:0,word2c:0})]),bad:vec![0],groups:[Some(Group{resources:vec![10]}),None]};
            let mut a=Admission{index154:0,index15c:0,word82e4:1,changed14:0,listeners:vec![]};
            let mesh_enabled=input["mesh_enabled"].as_bool().unwrap();
            let mut c=test_context(&mut m,serde_json::from_value(input["attributes"].clone())?,input["tags"].as_bool().unwrap(),mesh_enabled,input["slot"].as_u64().unwrap() as u32,0x64000000)?;
            let mut h=Callbacks{kind:input["kind"].as_u64().unwrap() as u32,change:input["change_stride"].as_bool().unwrap(),calls:vec![]};
            let result=finalize(&mut m,&mut a,&mut l,&mut s,&mut c,10,&mut h)?;
            let slot=m.word(0x6d000000)?;let edge_stride=m.word(c.manager+0x21c)?;
            for p in [c.manager,0x6e000000,0x6d000000] {m.regions.remove(&p);}
            assert_eq!(serde_json::to_value(&m)?,case["memory"],"{input}");
            assert_eq!(serde_json::to_value(&h.calls)?,case["calls"],"callbacks: {input}");
            assert_eq!(slot,case["slot"].as_u64().unwrap() as u32);
            assert_eq!(edge_stride,case["edge_stride"].as_u64().unwrap() as u32);
            assert_eq!(a.word82e4,case["journal_count"].as_u64().unwrap() as u32);
            assert_eq!(s.resources[&10].state,4,"Finalization alone cannot publish readiness");
            if input["kind"]==1 && mesh_enabled {
                let length=m.word(0x66000000+l.records[&1].size20-8)?.swap_bytes();
                assert_eq!(result,if length==0 || length==0x80000000 {Mesh::NotRequested} else if slot==0 {Mesh::Rejected(0x66000000)} else {Mesh::Loaded(slot)});
            } else {assert_eq!(result,Mesh::NotRequested);}
        }
        Ok(())
    }
}
