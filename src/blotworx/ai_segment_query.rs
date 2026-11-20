//! Uncached segment query452724, as invoked by goto reset472124 (r6=0).
//! Region walking and within-region geometry remain required
//! services. This preserves the native result codes; it is not a raycast.
use anyhow::Result;
use super::{ai_actor_queries::{self,Runtime,Filter},ai_astar_graph_services::GraphContext,
    ai_astar_memory::Node,ai_graph_link_passes::Memory,ai_traversal_instances::Instance};

/// Stack88's point payload. The type/vtable word is represented by Rust;
/// the nineteen remaining words retain the native point and auxiliary data.
#[derive(Clone,Debug)]
pub struct WalkPoint {pub position:[u32;3],pub auxiliary:[u32;16]}
impl WalkPoint {fn new(position:[u32;3])->Self{Self{position,auxiliary:[0;16]}}}
pub struct Context<'a,'b> {
    pub actors:&'a mut Runtime,
    pub graph:&'a mut GraphContext<'b,Memory>,
    pub instance:&'a mut Instance,
}
pub trait Services {
    fn clock_bits(&self,graph:&GraphContext<'_,Memory>,instance:&Instance)->Result<u32>;
    fn invalidate(&mut self,actors:&mut Runtime,graph:&mut GraphContext<'_,Memory>,instance:&mut Instance,request:Filter)->Result<u8>;
    /// Query virtual2c. Both output words and the iterator survive each call.
    fn walk(&mut self,c:&mut Context<'_,'_>,query:u32,point:&mut WalkPoint,end:[u32;3],iterator:&mut u32,output:&mut [u32;2])->Result<u8>;
    /// Query virtual40, with a null input cache. Scalar and seed are separate
    /// outputs, as in the original ABI; the caller rereads region/pool fields.
    fn segment(&mut self,c:&mut Context<'_,'_>,query:u32,start:[u32;3],end:[u32;3],region:u32,pool:u32,scalar:&mut u32,seed:&mut Node)->Result<u32>;
}
fn word(c:&Context<'_,'_>,base:u32,offset:u32)->Result<u32>{c.graph.tables.word(base.wrapping_add(offset))}
fn position(c:&Context<'_,'_>,base:u32,offset:u32)->Result<[u32;3]>{Ok([word(c,base,offset)?,word(c,base,offset+4)?,word(c,base,offset+8)?])}

/// REL44d288: ordered region-pointer table. Preserve the native midpoint and
/// endpoint order, including duplicate IDs. Empty/truncated native tables
/// cause a checked data-access error rather than a successful lookup miss.
pub fn region(memory:&Memory,manager:u32,id:u32)->Result<u32>{
    let header=memory.word(manager.wrapping_add(12))?;
    let mut high=memory.word(header.wrapping_add(12))?.wrapping_sub(1);
    let mut low=0u32;
    loop {
        let middle=low.wrapping_add(high)>>1;
        if low==middle {break}
        let table=memory.word(manager.wrapping_add(16))?;
        let candidate=memory.word(table.wrapping_add(middle.wrapping_shl(2)))?;
        let candidate_id=memory.word(candidate.wrapping_add(4))?;
        if candidate_id==id {return Ok(candidate)}
        if candidate_id>id {high=middle}else{low=middle}
    }
    let table=memory.word(manager.wrapping_add(16))?;
    for index in [low,high] {
        let candidate=memory.word(table.wrapping_add(index.wrapping_shl(2)))?;
        if memory.word(candidate.wrapping_add(4))?==id {return Ok(candidate)}
    }
    Ok(0)
}

pub fn uncached(c:&mut Context<'_,'_>,query:u32,start:[u32;3],end:[u32;3],scalar:&mut u32,seed:&mut Node,services:&mut impl Services)->Result<u32> {
    seed.word30=0;
    seed.scalar2c=services.clock_bits(c.graph,c.instance)?;
    *scalar=0; // REL4d582c
    let manager=word(c,query,8)?;
    let list=word(c,manager,0x44)?;
    if word(c,list,0xc)?==0 {return Ok(0)}
    // The concrete query's virtual68 is49d794: owner204/208/20c.
    let actor=word(c,query,0x7c)?;
    let actor_position=position(c,actor,0x204)?.map(f32::from_bits);
    let s=start.map(f32::from_bits);let e=end.map(f32::from_bits);
    let dz=s[2]-actor_position[2];let dx=s[0]-actor_position[0];
    let horizontal=dz*dz+dx*dx;let radius=f32::from_bits(word(c,query,0x68)?);
    // Native ble is !GT, so unordered comparisons continue.
    if horizontal>radius*radius {return Ok(0)}
    let dz=e[2]-s[2];let dx=e[0]-s[0];let dy=e[1]-s[1];
    let squared=dy*dy+(dz*dz+dx*dx);
    let length=super::cursor::native_sqrt(squared);
    let manager=word(c,query,8)?;
    let spacing=f32::from_bits(word(c,manager,0x5c)?);
    let maximum=super::native_float_conversion::unsigned((length/spacing).ceil());
    let mut scalar_part=0;
    let mut iterations=0u32;
    let mut current=start;
    let manager=word(c,query,8)?;
    let mut iterator=word(c,manager,8)?;
    let mut output=[0;2];
    let mut point=WalkPoint::new(current);
    while iterations<maximum {
        if services.walk(c,query,&mut point,end,&mut iterator,&mut output)?==0 || output[1]==0 {break}
        let manager=word(c,query,8)?;let id=word(c,output[1],4)?;
        iterations=iterations.wrapping_add(1);
        let region=region(c.graph.tables,manager,id)?;
        if region==0 {return Ok(1)}
        let object=word(c,region,0x18)?;
        if object==0 {return Ok(1)}
        let handle=word(c,object,0x14)?;
        if handle==0 {return Ok(1)}
        let pool=word(c,query,0x64)?;
        match services.segment(c,query,current,end,handle,pool,&mut scalar_part,seed)? {
            0|1=>{
                // The callback may replace the object's navigation handle.
                let object=word(c,region,0x18)?;seed.word30=word(c,object,0x14)?;return Ok(1)
            }
            2=>{
                let pool=word(c,query,0x64)?;
                if word(c,pool,0x14)?<2 {return Ok(0)}
                *scalar=(f32::from_bits(*scalar)+f32::from_bits(scalar_part)).to_bits();
                let pool=word(c,query,0x64)?;let tail=word(c,pool,0xc)?;
                current=position(c,tail,4)?;point=WalkPoint::new(current);
            }
            3=>return Ok(2),
            _=>return Ok(1),
        }
    }
    Ok(u32::from(iterations>=maximum && maximum!=0))
}

/// Drops directly into the existing actor/cache -> graph-services -> goto-reset
/// composition. The top-level segment callback now executes native Rust code.
pub struct Backend<H>{pub services:H}
impl<H:Services> ai_actor_queries::Backend for Backend<H> {
    fn clock_bits(&self,g:&GraphContext<'_,Memory>,i:&Instance)->Result<u32>{self.services.clock_bits(g,i)}
    fn invalidate(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,f:Filter)->Result<u8>{self.services.invalidate(r,g,i,f)}
    fn segment(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,q:usize,s:[u32;3],e:[u32;3],seed:&mut Node)->Result<u32>{
        let query=u32::try_from(q)?;let mut scalar=0;
        uncached(&mut Context{actors:r,graph:g,instance:i},query,s,e,&mut scalar,seed,&mut self.services)
    }
}

#[cfg(test)] mod tests {
    use super::*;
    use std::{cell::RefCell,collections::BTreeMap};
    use serde_json::{Value,json};
    use super::super::{ai_graph_handles,ai_graph_queries::Metadata,ai_goto_traversal,ai_astar_graph_services::Actors};
    struct Fixture<'a>{case:&'a Value,trace:RefCell<Vec<Value>>,walk:usize,segment:usize}
    impl<'a> Fixture<'a>{fn new(case:&'a Value)->Self{Self{case,trace:RefCell::new(Vec::new()),walk:0,segment:0}}}
    fn bits(value:&Value)->u32{value.as_u64().unwrap() as u32}
    fn vector(value:&Value)->[u32;3]{std::array::from_fn(|i|bits(&value[i]))}
    fn writes(c:&mut Context<'_,'_>,rows:&Value)->Result<()> {
        if let Some(rows)=rows.as_array(){for row in rows {c.graph.tables.write(bits(&row[0]),bits(&row[1]))?;}}Ok(())
    }
    impl Services for Fixture<'_> {
        fn clock_bits(&self,_:&GraphContext<'_,Memory>,_:&Instance)->Result<u32>{self.trace.borrow_mut().push(json!(["clock"]));Ok(bits(&self.case["clock"]))}
        fn invalidate(&mut self,_:&mut Runtime,_:&mut GraphContext<'_,Memory>,_:&mut Instance,_:Filter)->Result<u8>{anyhow::bail!("Unexpected cache invalidation in uncached query")}
        fn walk(&mut self,c:&mut Context<'_,'_>,q:u32,p:&mut WalkPoint,e:[u32;3],iterator:&mut u32,output:&mut[u32;2])->Result<u8>{
            self.trace.borrow_mut().push(json!(["walk",q,p.position,p.auxiliary,e,*iterator,*output,0]));
            let step=&self.case["walks"][self.walk];self.walk+=1;
            if step.is_null(){return Ok(0)}
            if step["output"].is_array(){*output=[bits(&step["output"][0]),bits(&step["output"][1])];}
            if step["iterator"].is_number(){*iterator=bits(&step["iterator"]);}
            if step["position"].is_array(){p.position=vector(&step["position"]);}
            if step["auxiliary"].is_number(){p.auxiliary[0]=bits(&step["auxiliary"]);}
            writes(c,&step["writes"])?;Ok(bits(&step["result"]) as u8)
        }
        fn segment(&mut self,c:&mut Context<'_,'_>,q:u32,s:[u32;3],e:[u32;3],region:u32,pool:u32,scalar:&mut u32,_:&mut Node)->Result<u32>{
            self.trace.borrow_mut().push(json!(["segment",q,s,e,0,region,pool,*scalar]));
            let step=&self.case["segments"][self.segment];self.segment+=1;*scalar=bits(&step["scalar"]);writes(c,&step["writes"])?;Ok(bits(&step["result"]))
        }
    }
    fn memory(c:&Value)->Result<Memory>{
        let mut m=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride:36,journal:BTreeMap::new()};
        // Composed reset fixtures use portable query identity66, while the
        // original-instruction harness relocates it to its native test arena.
        m.regions.insert(0x20,vec![0;0x200]);
        for n in 0x60u32..=0x68{m.regions.insert(n<<24,vec![0;0x300]);}
        for row in c["words"].as_array().unwrap(){m.write(bits(&row[0]),bits(&row[1]))?;}Ok(m)
    }
    fn actors()->Runtime{Runtime{pathfinder_class:0,sentinel:[0;3],constraints:BTreeMap::new(),owners:BTreeMap::new(),services:BTreeMap::new(),classes:BTreeMap::new(),queries:BTreeMap::new(),pools:BTreeMap::new(),entries:BTreeMap::new()}}
    fn graph()->ai_graph_handles::State{ai_graph_handles::State{phase:0,serial:0,pool:Vec::new(),nodes:BTreeMap::new(),resources:BTreeMap::new(),current:None,tracked_resource:None,tracked158:0,tracked160:0,deferred_enabled:0}}
    #[test] fn original_uncached_segment_query_matches_status_outputs_and_service_order()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_segment_query.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let mut memory=memory(c)?;let mut graph=graph();let mut metadata=Metadata::default();
            let mut context=GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata};
            let mut actors=actors();let mut instance=ai_goto_traversal::construct(1);let mut host=Fixture::new(c);let mut seed=Node::construct();
            let before=serde_json::to_value(&seed)?;let mut scalar=0xa5a5a5a5;
            let result=uncached(&mut Context{actors:&mut actors,graph:&mut context,instance:&mut instance},0x60000000,vector(&c["start"]),vector(&c["end"]),&mut scalar,&mut seed,&mut host)?;
            assert_eq!(json!(result),expected["result"],"{c}");assert_eq!(json!(scalar),expected["scalar"],"{c}");
            assert_eq!(json!(seed.scalar2c),expected["seed_clock"]);assert_eq!(json!(seed.word30),expected["seed_handle"]);
            let mut after=serde_json::to_value(&seed)?;after["scalar2c"]=before["scalar2c"].clone();after["word30"]=before["word30"].clone();assert_eq!(after,before);
            assert_eq!(host.trace.into_inner(),*expected["trace"].as_array().unwrap(),"{c}");
            for row in expected["words"].as_array().unwrap(){assert_eq!(memory.word(bits(&row[0]))?,bits(&row[1]));}
        }Ok(())
    }
    #[test] fn actor_segment_adapter_uses_the_same_canonical_query_memory()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_segment_query.json"))?;
        let expected=fixture["cases"].as_array().unwrap().iter().find(|c|c["seed_handle"]==99).unwrap();let c=&expected["input"];
        let mut memory=memory(c)?;let mut graph=graph();let mut metadata=Metadata::default();let mut instance=ai_goto_traversal::construct(1);let mut seed=Node::construct();
        let mut native=ai_actor_queries::Native{runtime:actors(),backend:Backend{services:Fixture::new(c)}};
        let status=native.segment_query(&mut instance,&mut GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata},0x60000000,vector(&c["start"]),vector(&c["end"]),&mut seed)?;
        assert_eq!(json!(status),expected["result"]);assert_eq!(seed.word30,99);assert_eq!(memory.word(0x68000014)?,99);
        memory.regions.remove(&0x61000000);
        assert!(native.segment_query(&mut instance,&mut GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata},0x60000000,[0;3],[0;3],&mut seed).is_err());Ok(())
    }
    #[test] fn original_region_search_preserves_duplicate_and_endpoint_selection()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_segment_query.json"))?;
        for expected in fixture["lookups"].as_array().unwrap(){
            let mut m=Memory{regions:BTreeMap::from([(0x61000000,vec![0;0x300]),(0x62000000,vec![0;0x300]),(0x67000000,vec![0;0x800])]),resources:BTreeMap::new(),stride:36,journal:BTreeMap::new()};
            let ids=expected["ids"].as_array().unwrap();m.write(0x6100000c,0x62000000)?;m.write(0x6200000c,ids.len() as u32)?;m.write(0x61000010,0x61000100)?;
            for (i,id) in ids.iter().enumerate(){m.write(0x61000100+i as u32*4,0x67000000+i as u32*0x100)?;m.write(0x67000004+i as u32*0x100,bits(id))?;}
            assert_eq!(json!(region(&m,0x61000000,bits(&expected["key"]))?),expected["result"],"{expected}");
            m.write(0x6200000c,0)?;assert!(region(&m,0x61000000,0).is_err());
        }Ok(())
    }
    #[test] fn original_goto_reset_executes_the_concrete_uncached_query()->Result<()> {
        use super::super::{ai_astar_memory::Store,ai_astar_graph_services::GraphServices,ai_graph_queries::Tables,ai_goto_initialization::{state,state_mut}};
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_astar_segment_reset.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let mut store:Store=serde_json::from_value(c["store"].clone())?;
            let mut graph:ai_graph_handles::State=serde_json::from_value(c["graph"].clone())?;
            let tables:Tables=serde_json::from_value(c["graph_tables"].clone())?;let mut metadata:Metadata=serde_json::from_value(c["metadata"].clone())?;
            let mut canonical=super::super::ai_graph_memory_queries::fixture_memory(tables,&metadata)?;
            let query_memory=memory(&c["segment_case"])?;canonical.regions.extend(query_memory.regions);
            let mut instance=ai_goto_traversal::construct(1);let s=state_mut(&mut instance)?;
            s.references_54_58=[Some(100),Some(102)];s.word_40=bits(&c["selected"]) as i32;s.node_70=99;s.vector_44=[1.,0.,0.];s.reference_8=Some(100);s.reference_6c=Some(33);
            let actors=ai_actor_queries::Native{runtime:serde_json::from_value(c["actor_runtime"].clone())?,backend:Backend{services:Fixture::new(&c["segment_case"])}};
            let mut services=GraphServices{graph:&mut graph,tables:&mut canonical,metadata:&mut metadata,actors};
            store.reset_goto(&mut instance,&mut services)?;
            assert_eq!(serde_json::to_value(store)?,expected["store"],"{c}");
            let trace=expected["trace"].as_array().unwrap().iter().filter(|v|matches!(v[0].as_str(),Some("clock"|"walk"|"segment"))).cloned().collect::<Vec<_>>();
            assert_eq!(*services.actors.backend.services.trace.borrow(),trace,"{c}");
            let binding=&services.graph.nodes[&100];assert_eq!(json!({"point":binding.point,"graph":binding.graph}),expected["graph_binding"]);
            let s=state(&instance)?;assert_eq!(json!({"selected":s.word_40 as u32,"target":s.node_70,"vector":s.vector_44.map(f32::to_bits),"point":1,"graph":binding.graph.unwrap_or(0)}),expected["state"]);
        }Ok(())
    }
}
