//! Concrete within-region query49ce80, including cached input metadata.
//! Crossing441fa4, path construction4425c0 and child volumes441600 are required.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::{ai_segment_query::{self,Context,WalkPoint},ai_region_geometry::{self,Children},
    ai_graph_link_passes::Memory,ai_astar_memory::Node,ai_actor_queries::{Runtime,Filter},
    ai_astar_graph_services::GraphContext,ai_traversal_instances::Instance};
fn word(c:&Context<'_,'_>,base:u32,offset:u32)->Result<u32>{c.graph.tables.word(base.wrapping_add(offset))}
fn position(c:&Context<'_,'_>,base:u32,offset:u32)->Result<[u32;3]>{Ok([word(c,base,offset)?,word(c,base,offset+4)?,word(c,base,offset+8)?])}

///18bb8c ->482ebc. These are portable data references, not executable pointers.
pub fn owner_handle(memory:&Memory,owner:u32)->Result<Option<usize>>{
    let source=memory.word(owner.wrapping_add(0x50))?;
    let handle=if source!=0 {
        let index=memory.word(owner.wrapping_add(0x3c))?;
        if index>=memory.word(source.wrapping_add(0x30))? {return Ok(None)}
        let records=memory.word(source.wrapping_add(0x28))?;
        let record=records.wrapping_add(index.wrapping_mul(0x28));
        if memory.byte(record.wrapping_add(0x24))?==0 {return Ok(None)}
        memory.word(record.wrapping_add(4))?
    }else{memory.word(owner.wrapping_add(0x70))?};
    Ok((handle!=0).then_some(handle as usize))
}
fn clear_pool(memory:&mut Memory,pool:u32)->Result<()>{
    let mut visited=BTreeSet::new();
    loop {
        let node=memory.word(pool.wrapping_add(8))?;if node==0{break}
        ensure!(visited.insert(node),"Cyclic within-region path pool");
        memory.write_byte(node.wrapping_add(0x18),0)?;
        // Fresh reads preserve aliases between pool/list storage.
        let head=memory.word(pool.wrapping_add(8))?;let next=memory.word(head.wrapping_add(0x14))?;
        let free=memory.word(pool.wrapping_add(0x10))?;memory.write(head.wrapping_add(0x14),free)?;
        let head=memory.word(pool.wrapping_add(8))?;memory.write(pool.wrapping_add(0x10),head)?;memory.write(pool.wrapping_add(8),next)?;
    }
    memory.write(pool.wrapping_add(12),0)?;memory.write(pool.wrapping_add(0x14),0)?;Ok(())
}
/// The first crossing uses the same XYZ buffer for its input endpoint and
/// output. The reverse crossing has separate buffers; preserve that contract.
#[derive(Clone,Copy,Debug)]
pub enum Endpoint {Aliased,Separate([u32;3])}
pub trait Geometry:Children {
    fn context_scale(&self,c:&Context<'_,'_>)->Result<Option<f32>>;
    fn crossing(&mut self,c:&mut Context<'_,'_>,region:u32,polygon:&mut u32,start:[u32;3],end:Endpoint,query:u32,point:&mut[u32;3],edge:&mut u32,special:&mut u8)->Result<u8>;
    fn path(&mut self,c:&mut Context<'_,'_>,region:u32,polygon:&mut u32,first_edge:&mut u32,last_edge:&mut u32,start:[u32;3],end:[u32;3],query:u32,pool:u32,scalar:&mut u32)->Result<u32>;
}
fn contains(c:&Context<'_,'_>,region:u32,p:[u32;3],host:&mut impl Geometry)->Result<bool>{
    let scale=host.context_scale(c)?;ai_region_geometry::region(c.graph.tables,region,p,scale,host)
}
fn publish(seed:&mut Option<&mut Node>,region:u32,point:[u32;3],polygon:u32,edge:u32){
    if let Some(seed)=seed.as_deref_mut(){seed.word30=region;seed.values34_3c=point;seed.word44=polygon;seed.word4c=edge;}
}
pub fn segment(c:&mut Context<'_,'_>,query:u32,start:[u32;3],end:[u32;3],cache:Option<u32>,region:u32,pool:u32,scalar:&mut u32,mut seed:Option<&mut Node>,host:&mut impl Geometry)->Result<u32>{
    clear_pool(c.graph.tables,pool)?;
    let owner=word(c,query,0x7c)?;
    let Some(handle)=owner_handle(c.graph.tables,owner)? else{return Ok(0)};
    if !c.graph.graph.ready(Some(handle))? {return Ok(0)}
    let mut point=[0;3];let mut special=0u8;let mut first_edge=0;let mut selected=0;
    let cached=if let Some(cache)=cache{word(c,cache,8)?!=0}else{false};
    if cached {
        let cache=cache.unwrap();point=position(c,cache,12)?;selected=word(c,cache,0x1c)?;first_edge=word(c,cache,0x24)?;
    } else {
        point=end;
        let list=word(c,region,8)?;let mut cursor=word(c,list,8)?;
        let mut visited=BTreeSet::new();
        while cursor!=0 {
            ensure!(visited.insert(cursor),"Cyclic within-region outline list");
            if host.crossing(c,region,&mut cursor,start,Endpoint::Aliased,query,&mut point,&mut first_edge,&mut special)?!=0 {
                selected=cursor;if special!=0{break}
            }
            cursor=word(c,cursor,12)?;
        }
        if selected==0{return Ok(if contains(c,region,start,host)?{3}else{0})}
    }
    if contains(c,region,end,host)? {
        publish(&mut seed,region,point,selected,first_edge);return Ok(3)
    }
    let mut other_point=[0;3];let mut last_edge=0;
    if host.crossing(c,region,&mut selected,end,Endpoint::Separate(start),query,&mut other_point,&mut last_edge,&mut special)?==0 {
        publish(&mut seed,region,point,selected,first_edge);return Ok(3)
    }
    let result=host.path(c,region,&mut selected,&mut first_edge,&mut last_edge,point,other_point,query,pool,scalar)?;
    if let Some(seed)=seed.as_deref_mut(){seed.word30=0;}
    Ok(if result<=1{result}else{2})
}

/// Cache and outer walk bindings remain shared with the actor-query adapter.
pub trait Bindings:Geometry {
    fn clock_bits(&self,g:&GraphContext<'_,Memory>,i:&Instance)->Result<u32>;
    fn invalidate(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,f:Filter)->Result<u8>;
    fn walk(&mut self,c:&mut Context<'_,'_>,query:u32,point:&mut WalkPoint,end:[u32;3],iterator:&mut u32,output:&mut[u32;2])->Result<u8>;
}
pub struct Native<H>{pub bindings:H}
impl<H:Bindings> ai_segment_query::Services for Native<H>{
    fn clock_bits(&self,g:&GraphContext<'_,Memory>,i:&Instance)->Result<u32>{self.bindings.clock_bits(g,i)}
    fn invalidate(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,f:Filter)->Result<u8>{self.bindings.invalidate(r,g,i,f)}
    fn walk(&mut self,c:&mut Context<'_,'_>,q:u32,p:&mut WalkPoint,e:[u32;3],i:&mut u32,o:&mut[u32;2])->Result<u8>{self.bindings.walk(c,q,p,e,i,o)}
    fn segment(&mut self,c:&mut Context<'_,'_>,q:u32,s:[u32;3],e:[u32;3],r:u32,p:u32,scalar:&mut u32,seed:&mut Node)->Result<u32>{segment(c,q,s,e,None,r,p,scalar,Some(seed),&mut self.bindings)}
}

#[cfg(test)] mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use serde_json::{Value,json};
    use super::super::{ai_graph_handles,ai_graph_queries::Metadata,ai_goto_traversal,ai_actor_queries};
    const QUERY:u32=0x6b000000;const OWNER:u32=0x6c000000;const POOL:u32=0x6d000000;const CACHE:u32=0x6e000000;
    const HANDLE:u32=0x70000000;const RESOURCE:u32=0x72000000;const INFO:u32=0x73000000;const RECORDS:u32=0x74000000;const PNODES:u32=0x75000000;
    const REGION:u32=0x61000000;const NODES:u32=0x64000000;const CHILD:u32=0x67000000;
    fn bits(v:&Value)->u32{v.as_u64().unwrap() as u32}
    fn vector(v:&Value)->[u32;3]{std::array::from_fn(|i|bits(&v[i]))}
    pub(super) fn memory(c:&Value)->Result<Memory>{
        let mut m=ai_region_geometry::fixture_memory(&c["geometry"])?;
        for (at,size) in [(QUERY,0x90),(OWNER,0x220),(POOL,0x20),(CACHE,0x28),(INFO,0x34),(RECORDS,0x50)]{m.regions.insert(at,vec![0;size]);}
        m.write(QUERY+0x7c,OWNER)?;m.write(OWNER+0x70,if c["handle"]==true{HANDLE}else{0})?;
        m.write(OWNER+0x50,if c["indirect"]==true{INFO}else{0})?;m.write(OWNER+0x3c,bits(&c["index"]))?;m.write(INFO+0x28,RECORDS)?;m.write(INFO+0x30,2)?;
        for i in 0..2 {m.write(RECORDS+i*0x28+4,if c["handle"]==true{HANDLE}else{0})?;m.write_byte(RECORDS+i*0x28+0x24,bits(&c["active"]) as u8)?;}
        let count=bits(&c["pool_count"]);m.regions.insert(PNODES,vec![0xa5;(count as usize+1)*0x20]);
        m.write(POOL+8,if count!=0{PNODES}else{0})?;m.write(POOL+12,if count!=0{PNODES+(count-1)*0x20}else{0})?;m.write(POOL+16,PNODES+count*0x20)?;m.write(POOL+20,count)?;
        for i in 0..=count {m.write(PNODES+i*0x20+0x14,if i+1<count{PNODES+(i+1)*0x20}else{0})?;m.write_byte(PNODES+i*0x20+0x18,u8::from(i<count))?;}
        m.write(CACHE+8,if c["cache_handle"]==true{REGION}else{0})?;m.write(CACHE+0x1c,NODES)?;m.write(CACHE+0x24,999)?;
        for i in 0..3 {m.write(CACHE+12+i as u32*4,bits(&c["cache_point"][i]))?;}Ok(m)
    }
    pub(super) fn graph(c:&Value)->ai_graph_handles::State{
        ai_graph_handles::State{phase:0,serial:0,pool:Vec::new(),nodes:BTreeMap::from([(HANDLE as usize,ai_graph_handles::Node{connection:None,traversal:None,point:None,graph:None,free:0,word14:RESOURCE,word18:0,manager_1c:None,word20:0,byte24:0,scalar28:0,scalar2c:0,byte30:0,word34:0,byte38:0,byte39:0,word3c:0,next:None,serial:0})]),
            resources:BTreeMap::from([(RESOURCE,ai_graph_handles::Resource{size18:None,byte8:0,priority:0,state:if c["ready"]==true{5}else{4},word1c:0})]),current:None,tracked_resource:None,tracked158:0,tracked160:0,deferred_enabled:0}
    }
    fn actors()->Runtime{Runtime{pathfinder_class:0,sentinel:[0;3],constraints:BTreeMap::new(),owners:BTreeMap::new(),services:BTreeMap::new(),classes:BTreeMap::new(),queries:BTreeMap::new(),pools:BTreeMap::new(),entries:BTreeMap::new()}}
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,walks:u32}
    impl Children for Fixture<'_>{fn contains(&mut self,_:&Memory,child:u32,p:[u32;3],flag:u32)->Result<u8>{self.trace.push(json!(["child",child,p,flag]));Ok(bits(&self.case["geometry"]["children"][((child-CHILD)/0x100) as usize]) as u8)}}
    impl Geometry for Fixture<'_>{
        fn context_scale(&self,_:&Context<'_,'_>)->Result<Option<f32>>{Ok(self.case["geometry"]["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
        fn crossing(&mut self,_:&mut Context<'_,'_>,r:u32,polygon:&mut u32,s:[u32;3],e:Endpoint,q:u32,p:&mut[u32;3],edge:&mut u32,special:&mut u8)->Result<u8>{
            let (end,alias)=match e{Endpoint::Aliased=>(*p,true),Endpoint::Separate(e)=>(e,false)};
            self.trace.push(json!(["crossing",r,*polygon,s,end,q,*p,*edge,*special,alias]));let step=&self.case[if alias{"first"}else{"reverse"}];
            *p=vector(&step["point"]);*edge=bits(&step["edge"]);*special=bits(&step["special"]) as u8;Ok(bits(&step["result"]) as u8)
        }
        fn path(&mut self,c:&mut Context<'_,'_>,r:u32,polygon:&mut u32,first:&mut u32,last:&mut u32,s:[u32;3],e:[u32;3],q:u32,pool:u32,scalar:&mut u32)->Result<u32>{
            self.trace.push(json!(["path",r,*polygon,*first,*last,s,e,q,pool,*scalar]));*scalar=bits(&self.case["path_scalar"]);c.graph.tables.write(pool+20,bits(&self.case["path_count"]))?;Ok(bits(&self.case["path_status"]))
        }
    }
    impl Bindings for Fixture<'_>{
        fn clock_bits(&self,_:&GraphContext<'_,Memory>,_:&Instance)->Result<u32>{Ok(2f32.to_bits())}
        fn invalidate(&mut self,_:&mut Runtime,_:&mut GraphContext<'_,Memory>,_:&mut Instance,_:Filter)->Result<u8>{anyhow::bail!("No invalidation in region fixture")}
        fn walk(&mut self,_:&mut Context<'_,'_>,_:u32,_:&mut WalkPoint,_:[u32;3],_:&mut u32,output:&mut[u32;2])->Result<u8>{self.walks+=1;*output=[0,0x7f000000];Ok(u8::from(self.walks==1))}
    }
    #[test] fn original_region_segment_matches_ready_pool_metadata_and_geometry_order()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_region_segment.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let mut memory=memory(c)?;let mut graph=graph(c);let mut metadata=Metadata::default();let mut actors=actors();let mut instance=ai_goto_traversal::construct(1);
            let mut gc=GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata};let mut context=Context{actors:&mut actors,graph:&mut gc,instance:&mut instance};
            let mut scalar=bits(&c["scalar"]);let mut seed:Node=serde_json::from_value(c["seed"].clone())?;let mut native=Native{bindings:Fixture{case:c,trace:Vec::new(),walks:0}};
            let result=if c["cache"]==false && c["seed_present"]==true {
                ai_segment_query::Services::segment(&mut native,&mut context,QUERY,vector(&c["start"]),vector(&c["end"]),REGION,POOL,&mut scalar,&mut seed)?
            }else{segment(&mut context,QUERY,vector(&c["start"]),vector(&c["end"]),(c["cache"]==true).then_some(CACHE),REGION,POOL,&mut scalar,(c["seed_present"]==true).then_some(&mut seed),&mut native.bindings)?};
            assert_eq!(json!(result),expected["result"],"{c}");assert_eq!(json!(scalar),expected["scalar"],"{c}");assert_eq!(serde_json::to_value(seed)?,expected["seed"],"{c}");
            assert_eq!(native.bindings.trace,*expected["trace"].as_array().unwrap(),"{c}");
            assert_eq!(json!([memory.word(POOL+8)?,memory.word(POOL+12)?,memory.word(POOL+16)?,memory.word(POOL+20)?]),expected["pool"],"{c}");
            let bytes=(0..(bits(&c["pool_count"])+1)*0x20).map(|i|memory.byte(PNODES+i)).collect::<Result<Vec<_>>>()?;assert_eq!(json!(bytes),expected["nodes"],"{c}");
        }Ok(())
    }
    #[test] fn outer_query_uses_concrete_region_geometry_and_publishes_seed_metadata()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_region_segment.json"))?;
        let expected=fixture["cases"].as_array().unwrap().iter().find(|x|x["input"]["cache"]==false && x["input"]["pool_count"]==3 && x["input"]["end"][0]==2f32.to_bits()).unwrap();let c=&expected["input"];
        let mut memory=memory(c)?;
        for (at,size) in [(0x7c000000,0x80),(0x7d000000,0x20),(0x7e000000,0x100),(0x7f000000,0x10)]{memory.regions.insert(at,vec![0;size]);}
        for (at,v) in [(QUERY+8,0x7c000000),(QUERY+0x64,POOL),(QUERY+0x68,10f32.to_bits()),(0x7c000044,0x7d000000),(0x7c00000c,0x7d000000),(0x7d00000c,1),(0x7c000010,0x7e000000),(0x7c00005c,1f32.to_bits()),(0x7e000000,0x7e000020),(0x7e000024,55),(0x7e000038,0x7e000060),(0x7e000074,REGION),(0x7f000004,55)]{memory.write(at,v)?;}
        let mut graph=graph(c);let mut metadata=Metadata::default();let mut instance=ai_goto_traversal::construct(1);let mut actors=actors();let mut seed:Node=serde_json::from_value(c["seed"].clone())?;
        let mut backend=ai_segment_query::Backend{services:Native{bindings:Fixture{case:c,trace:Vec::new(),walks:0}}};
        let result=ai_actor_queries::Backend::segment(&mut backend,&mut actors,&mut GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata},&mut instance,QUERY as usize,vector(&c["start"]),vector(&c["end"]),&mut seed)?;
        assert_eq!(result,2);let mut expected_seed=expected["seed"].clone();expected_seed["scalar2c"]=json!(2f32.to_bits());assert_eq!(serde_json::to_value(seed)?,expected_seed);
        assert_eq!(backend.services.bindings.trace,*expected["trace"].as_array().unwrap());assert_eq!(backend.services.bindings.walks,1);Ok(())
    }
}
#[cfg(test)]
pub(super) fn fixture(case:&serde_json::Value)->Result<(Memory,super::ai_graph_handles::State)>{Ok((tests::memory(case)?,tests::graph(case)))}
