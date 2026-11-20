//! Native segment intersection459cbc and polygon crossing441fa4.
//! Query acceptance24, path4425c0, child volumes and spatial walk are required.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::{ai_graph_link_passes::Memory,ai_segment_query::{Context,WalkPoint},
    ai_region_segment::{self,Endpoint},ai_region_geometry::{self,Children,vertex,select},
    ai_actor_queries::{Runtime,Filter},ai_astar_graph_services::GraphContext,ai_traversal_instances::Instance};
fn word(m:&Memory,b:u32,o:u32)->Result<u32>{m.word(b.wrapping_add(o))}
fn scalar(m:&Memory,b:u32,o:u32)->Result<f32>{Ok(f32::from_bits(word(m,b,o)?))}
fn put(m:&mut Memory,b:u32,o:u32,v:f32)->Result<()>{m.write(b.wrapping_add(o),v.to_bits())}
#[derive(Clone,Copy,Debug)]
pub struct Globals {pub root_slot:u32,pub intersections:u32,pub polygon_visits:u32}
impl Globals {
    pub fn context(self,m:&Memory)->Result<u32>{word(m,m.word(self.root_slot)?,0x4c)}
    pub fn scale(self,m:&Memory)->Result<Option<f32>>{let root=m.word(self.root_slot)?;if root==0{Ok(None)}else{Ok(Some(scalar(m,root,12)?))}}
    pub fn clock(self,m:&Memory)->Result<u32>{word(m,self.context(m)?,0x50)}
    fn intersection(self,m:&mut Memory)->Result<()>{
        let counter=m.word(self.intersections)?.wrapping_add(1);m.write(self.intersections,counter)?;
        let pending=scalar(m,self.intersections,16)?;let cost=scalar(m,self.intersections,8)?;put(m,self.intersections,16,pending+cost)
    }
    fn polygons(self,m:&mut Memory,count:u32,region:u32)->Result<()>{
        let visits=m.word(self.polygon_visits)?.wrapping_add(count);m.write(self.polygon_visits,visits)?;
        let config=word(m,region,4)?;let index=word(m,config,0xb0)? as i32;
        self.charge(m,count,index)
    }
    pub(super) fn charge(self,m:&mut Memory,count:u32,index:i32)->Result<()>{
        if index<0{return Ok(())}
        let context=self.context(m)?;
        let cost=if (index as u32)<word(m,context,12)? {let table=word(m,context,0x40)?;scalar(m,table,(index as u32).wrapping_mul(4))?}else{0.};
        let charge=(count as f32)*cost;let old=scalar(m,context,0x7c)?;put(m,context,0x7c,old+charge)
    }
    pub(super) fn finish(self,m:&mut Memory)->Result<()>{
        let context=self.context(m)?;let old=scalar(m,context,0x7c)?;let pending=scalar(m,self.intersections,16)?;
        put(m,context,0x7c,old+pending)?;put(m,self.intersections,16,0.)
    }
    pub(super) fn radius_intersection(self,m:&mut Memory)->Result<()>{
        let counter=word(m,self.intersections,4)?.wrapping_add(1);m.write(self.intersections.wrapping_add(4),counter)?;
        let pending=scalar(m,self.intersections,16)?;let cost=scalar(m,self.intersections,12)?;
        put(m,self.intersections,16,pending+cost)
    }
}

///459cbc is directional in XZ. Its two Y outputs interpolate DIFFERENT
/// segments. Independent optional outputs match441fa4's actual call contract.
pub fn intersection(m:&mut Memory,g:Globals,a:[u32;3],b:[u32;3],c:[u32;3],d:[u32;3],first:Option<&mut[u32;3]>,second:Option<&mut[u32;3]>)->Result<bool>{
    g.intersection(m)?;
    let mut first_value=[0.;3];let mut second_value=[0.;3];
    let hit=super::native_segment_intersection::intersection(a.map(f32::from_bits),b.map(f32::from_bits),c.map(f32::from_bits),d.map(f32::from_bits),first.as_ref().map(|_|&mut first_value),second.as_ref().map(|_|&mut second_value));
    if hit{if let Some(first)=first{*first=first_value.map(f32::to_bits);}if let Some(second)=second{*second=second_value.map(f32::to_bits);}}Ok(hit)
}

pub trait Acceptance {
    /// Query virtual24 receives the two native point objects with zeroed
    /// auxiliary fields. The result is tested as a low byte, not bit0.
    fn accept(&mut self,c:&mut Context<'_,'_>,query:u32,start:[u32;3],crossing:[u32;3])->Result<u8>;
}
pub fn crossing(c:&mut Context<'_,'_>,g:Globals,region:u32,polygon:u32,start:[u32;3],end:Endpoint,query:u32,point:&mut[u32;3],edge:&mut u32,special:&mut u8,host:&mut impl Acceptance)->Result<u8>{
    let outline=word(c.graph.tables,polygon,4)?;let count=word(c.graph.tables,outline,16)?;
    if count<2{return Ok(0)}
    let config=word(c.graph.tables,region,4)?;
    let vertical=c.graph.tables.byte(config.wrapping_add(0x58))?;
    let radius=scalar(c.graph.tables,config,0x5c)?;let radius2=radius*radius;
    let height=scalar(c.graph.tables,config,0x60)?;
    let mut current=word(c.graph.tables,outline,8)?;let mut previous=vertex(c.graph.tables,current)?;
    let endpoint=match end{Endpoint::Aliased=>*point,Endpoint::Separate(p)=>p};let mut target=endpoint;
    *special=0;
    // The accounting write precedes the optional per-polygon cost lookup.
    let count=word(c.graph.tables,word(c.graph.tables,polygon,4)?,16)?;
    g.polygons(c.graph.tables,count,region)?;
    let outline=word(c.graph.tables,polygon,4)?;
    let scale=g.scale(c.graph.tables)?;let inside=ai_region_geometry::polygon(c.graph.tables,outline,start,scale)?;
    current=word(c.graph.tables,current,0x30)?;
    let s=start.map(f32::from_bits);let e=endpoint.map(f32::from_bits);
    let mut found=false;let mut visited=BTreeSet::new();
    while current!=0 {
        ensure!(visited.insert(current),"Cyclic navigation crossing vertices");
        let next=vertex(c.graph.tables,current)?;
        let offset=[s[0]-previous[0],s[1]-previous[1],s[2]-previous[2]];
        let direction=[next[0]-previous[0],next[1]-previous[1],next[2]-previous[2]];
        let side=direction[2]*offset[0]-direction[0]*offset[2];
        if inside && side<=0. {
            let within_height=if vertical==0 {true}else{
                let difference=previous[1]-next[1];let low=select(difference,next[1],previous[1])-0.5*height;
                let high=0.5*height+select(difference,previous[1],next[1]);s[1]>low && s[1]<high
            };
            if within_height {
                let endpoint_side=direction[2]*(e[0]-s[0])-direction[0]*(e[2]-s[2]);
                let length2=direction[0]*direction[0]+direction[2]*direction[2];let limit=radius2*length2;
                if endpoint_side<=0. && side*side<=limit {
                    let projection=offset[2]*direction[2]+offset[0]*direction[0];
                    let mut near=if projection>=0. {projection<=length2}else{projection*projection<=limit};
                    if !near {
                        near=offset[0]*offset[0]+offset[2]*offset[2]<radius2 ||
                            (s[2]-next[2])*(s[2]-next[2])+(s[0]-next[0])*(s[0]-next[0])<radius2;
                    }
                    if near{*edge=current;target=start;found=true;*special=1;break}
                }
            }
        }else{
            let mut first=[0;3];let mut second=[0;3];
            if intersection(c.graph.tables,g,start,target,previous.map(f32::to_bits),next.map(f32::to_bits),Some(&mut first),Some(&mut second))? &&
                (vertical==0 || host.accept(c,query,start,first)?!=0) {
                target=first;*edge=current;found=true;
            }
        }
        previous=next;current=word(c.graph.tables,current,0x30)?;
    }
    g.finish(c.graph.tables)?;
    if found{*point=target;Ok(1)}else{Ok(0)}
}

pub trait Services:Children+Acceptance {
    fn path(&mut self,c:&mut Context<'_,'_>,region:u32,polygon:&mut u32,first:&mut u32,last:&mut u32,start:[u32;3],end:[u32;3],query:u32,pool:u32,scalar:&mut u32)->Result<u32>;
    fn invalidate(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,f:Filter)->Result<u8>;
    fn walk(&mut self,c:&mut Context<'_,'_>,query:u32,p:&mut WalkPoint,end:[u32;3],iterator:&mut u32,output:&mut[u32;2])->Result<u8>;
}
pub struct Native<H>{pub globals:Globals,pub services:H}
impl<H:Services> Children for Native<H>{fn contains(&mut self,m:&Memory,child:u32,p:[u32;3],flag:u32)->Result<u8>{self.services.contains(m,child,p,flag)}}
impl<H:Services> ai_region_segment::Geometry for Native<H>{
    fn context_scale(&self,c:&Context<'_,'_>)->Result<Option<f32>>{self.globals.scale(c.graph.tables)}
    fn crossing(&mut self,c:&mut Context<'_,'_>,r:u32,p:&mut u32,s:[u32;3],e:Endpoint,q:u32,out:&mut[u32;3],edge:&mut u32,special:&mut u8)->Result<u8>{crossing(c,self.globals,r,*p,s,e,q,out,edge,special,&mut self.services)}
    fn path(&mut self,c:&mut Context<'_,'_>,r:u32,p:&mut u32,first:&mut u32,last:&mut u32,s:[u32;3],e:[u32;3],q:u32,pool:u32,scalar:&mut u32)->Result<u32>{self.services.path(c,r,p,first,last,s,e,q,pool,scalar)}
}
impl<H:Services> ai_region_segment::Bindings for Native<H>{
    fn clock_bits(&self,g:&GraphContext<'_,Memory>,_:&Instance)->Result<u32>{self.globals.clock(g.tables)}
    fn invalidate(&mut self,r:&mut Runtime,g:&mut GraphContext<'_,Memory>,i:&mut Instance,f:Filter)->Result<u8>{self.services.invalidate(r,g,i,f)}
    fn walk(&mut self,c:&mut Context<'_,'_>,q:u32,p:&mut WalkPoint,e:[u32;3],i:&mut u32,o:&mut[u32;2])->Result<u8>{self.services.walk(c,q,p,e,i,o)}
}

#[cfg(test)] mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use serde_json::{Value,json};
    use super::super::{ai_graph_handles,ai_graph_queries::Metadata,ai_goto_traversal,ai_astar_memory::Node};
    const SLOT:u32=0x5f000000;const METRICS:u32=0x5f001000;const VISITS:u32=0x5f002000;const ROOT:u32=0x6a000000;const FRAME:u32=0x6f000000;const RATES:u32=0x6f001000;
    const REGION:u32=0x61000000;const NODES:u32=0x64000000;const QUERY:u32=0x6b000000;const POOL:u32=0x6d000000;const PNODES:u32=0x75000000;
    fn bits(v:&Value)->u32{v.as_u64().unwrap() as u32}
    fn vector(v:&Value)->[u32;3]{std::array::from_fn(|i|bits(&v[i]))}
    pub(super) fn globals(m:&mut Memory,c:&Value)->Result<Globals>{
        m.regions.insert(SLOT,vec![0;0x2004]);m.regions.insert(ROOT,vec![0;0x50]);m.regions.insert(FRAME,vec![0;0x80]);
        let rates=c["rates"].as_array().unwrap();m.regions.insert(RATES,vec![0;rates.len()*4]);
        for (at,v) in [(SLOT,ROOT),(ROOT+12,bits(&c["geometry"]["scale"])),(ROOT+0x4c,FRAME),(FRAME+12,rates.len() as u32),(FRAME+0x40,RATES),(FRAME+0x50,2f32.to_bits()),(FRAME+0x7c,bits(&c["budget"])),(METRICS,bits(&c["counter"])),(METRICS+8,bits(&c["cost"])),(METRICS+16,bits(&c["pending"])),(VISITS,bits(&c["visits"]))]{m.write(at,v)?;}
        for (i,v) in rates.iter().enumerate(){m.write(RATES+i as u32*4,bits(v))?;}
        Ok(Globals{root_slot:SLOT,intersections:METRICS,polygon_visits:VISITS})
    }
    fn graph()->ai_graph_handles::State{ai_graph_handles::State{phase:0,serial:0,pool:Vec::new(),nodes:BTreeMap::new(),resources:BTreeMap::new(),current:None,tracked_resource:None,tracked158:0,tracked160:0,deferred_enabled:0}}
    fn actors()->Runtime{Runtime{pathfinder_class:0,sentinel:[0;3],constraints:BTreeMap::new(),owners:BTreeMap::new(),services:BTreeMap::new(),classes:BTreeMap::new(),queries:BTreeMap::new(),pools:BTreeMap::new(),entries:BTreeMap::new()}}
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Acceptance for Fixture<'_>{fn accept(&mut self,_:&mut Context<'_,'_>,q:u32,s:[u32;3],p:[u32;3])->Result<u8>{self.trace.push(json!(["accept",q,s,p]));Ok(bits(&self.case["accept"]) as u8)}}
    impl Children for Fixture<'_>{fn contains(&mut self,_:&Memory,child:u32,p:[u32;3],flag:u32)->Result<u8>{self.trace.push(json!(["child",child,p,flag]));Ok(bits(&self.case["geometry"]["children"][((child-0x67000000)/0x100) as usize]) as u8)}}
    impl Services for Fixture<'_>{
        fn path(&mut self,c:&mut Context<'_,'_>,r:u32,p:&mut u32,first:&mut u32,last:&mut u32,s:[u32;3],e:[u32;3],q:u32,pool:u32,scalar:&mut u32)->Result<u32>{
            self.trace.push(json!(["path",r,*p,*first,*last,s,e,q,pool,*scalar]));*scalar=bits(&self.case["path_scalar"]);c.graph.tables.write(pool+20,bits(&self.case["path_count"]))?;Ok(bits(&self.case["path_status"]))
        }
        fn invalidate(&mut self,_:&mut Runtime,_:&mut GraphContext<'_,Memory>,_:&mut Instance,_:Filter)->Result<u8>{anyhow::bail!("No invalidation in crossing fixture")}
        fn walk(&mut self,_:&mut Context<'_,'_>,_:u32,_:&mut WalkPoint,_:[u32;3],_:&mut u32,_:&mut[u32;2])->Result<u8>{anyhow::bail!("Outer spatial walk is not part of crossing fixture")}
    }
    fn verify_globals(m:&Memory,expected:&Value)->Result<()>{
        for (at,key) in [(METRICS,"counter"),(VISITS,"visits"),(METRICS+16,"pending"),(FRAME+0x7c,"budget")]{assert_eq!(json!(m.word(at)?),expected[key],"{key}: {}",expected["input"]);}Ok(())
    }
    #[test] fn original_crossings_intersections_and_work_budgets_match()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_edge_crossing.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let mut memory=ai_region_geometry::fixture_memory(&c["geometry"])?;let globals=globals(&mut memory,c)?;
            let mut host=Fixture{case:c,trace:Vec::new()};
            let result=if c["op"]=="intersection"{
                let mut first=vector(&c["initial"]);let mut second=first;
                let result=intersection(&mut memory,globals,vector(&c["a"]),vector(&c["b"]),vector(&c["c"]),vector(&c["d"]),(c["first_output"]==true).then_some(&mut first),(c["second_output"]==true).then_some(&mut second))?;
                assert_eq!(json!(first),expected["first"],"{c}");assert_eq!(json!(second),expected["second"],"{c}");u8::from(result)
            }else{
                let mut graph=graph();let mut metadata=Metadata::default();let mut actors=actors();let mut instance=ai_goto_traversal::construct(1);
                let mut gc=GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata};let mut context=Context{actors:&mut actors,graph:&mut gc,instance:&mut instance};
                let mut point=vector(&c[if c["alias"]==true{"end"}else{"initial"}]);let mut edge=bits(&c["edge"]);let mut special=bits(&c["special"]) as u8;
                let endpoint=if c["alias"]==true{Endpoint::Aliased}else{Endpoint::Separate(vector(&c["end"]))};
                let result=crossing(&mut context,globals,REGION,NODES,vector(&c["start"]),endpoint,QUERY,&mut point,&mut edge,&mut special,&mut host)?;
                assert_eq!(json!(point),expected["point"],"{c}");assert_eq!(json!(edge),expected["edge"],"{c}");assert_eq!(json!(special),expected["special"],"{c}");result
            };
            assert_eq!(json!(result),expected["result"],"{c}");assert_eq!(host.trace,*expected["trace"].as_array().unwrap(),"{c}");verify_globals(&memory,expected)?;
            assert_eq!(globals.clock(&memory)?,2f32.to_bits());
        }Ok(())
    }
    #[test] fn region_segment_runs_actual_crossings_containment_and_budget_accounting()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_crossing_segment.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let (mut memory,mut graph)=ai_region_segment::fixture(c)?;let globals=globals(&mut memory,&c["edge_globals"])?;
            let mut metadata=Metadata::default();let mut actors=actors();let mut instance=ai_goto_traversal::construct(1);
            let mut gc=GraphContext{graph:&mut graph,tables:&mut memory,metadata:&mut metadata};let mut context=Context{actors:&mut actors,graph:&mut gc,instance:&mut instance};
            let mut native=ai_region_segment::Native{bindings:Native{globals,services:Fixture{case:c,trace:Vec::new()}}};
            let mut scalar=bits(&c["scalar"]);let mut seed:Node=serde_json::from_value(c["seed"].clone())?;
            let result=super::super::ai_segment_query::Services::segment(&mut native,&mut context,QUERY,vector(&c["start"]),vector(&c["end"]),REGION,POOL,&mut scalar,&mut seed)?;
            assert_eq!(json!(result),expected["result"],"{c}");assert_eq!(json!(scalar),expected["scalar"],"{c}");assert_eq!(serde_json::to_value(seed)?,expected["seed"],"{c}");
            assert_eq!(native.bindings.services.trace,*expected["trace"].as_array().unwrap(),"{c}");verify_globals(&memory,expected)?;
            assert_eq!(json!([memory.word(POOL+8)?,memory.word(POOL+12)?,memory.word(POOL+16)?,memory.word(POOL+20)?]),expected["pool"]);
            let bytes=(0..(bits(&c["pool_count"])+1)*0x20).map(|i|memory.byte(PNODES+i)).collect::<Result<Vec<_>>>()?;assert_eq!(json!(bytes),expected["nodes"]);
        }Ok(())
    }
}
#[cfg(test)]
pub(super) fn fixture_globals(memory:&mut Memory,case:&serde_json::Value)->Result<Globals>{tests::globals(memory,case)}
