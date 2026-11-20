//! JPSRefineGoal virtual20, retail REL194458..194bb4, and virtual24's
//!194c50 active-region cost. Graph/spatial/attribute providers remain required;
//! absence is never replaced by clear space or an invented candidate.
use anyhow::{ensure, Context, Result};
use super::{ai_graph_link_passes::Memory, ai_refine_goal::{State, Units, PointHost, position, set_position, word},
    ai_segment_query::WalkPoint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribute { Diameter, Height }
pub trait Host: Units {
    fn graph_handle_64(&mut self, state:&mut State, memory:&mut Memory)->Result<Option<usize>>;
    fn graph_ready(&mut self, state:&mut State, memory:&mut Memory, graph:usize)->Result<u8>;
    /// Read actual pathfinder+50 and its +4c provider, preserving null checks.
    fn spatial_provider(&mut self, state:&mut State, memory:&mut Memory)->Result<Option<usize>>;
    ///45f994 exact lookup, followed by the attribute's scalar at +4.
    fn actor_attribute(&mut self, state:&mut State, memory:&mut Memory, attribute:Attribute)->Result<Option<f32>>;
    fn graph_radius(&mut self, state:&mut State, memory:&mut Memory, graph:usize)->Result<f32>;
    fn fallback_radius(&mut self, state:&mut State, memory:&mut Memory)->Result<f32>;
    ///47cde0, mode1, native capacity256 and null optional arguments. Returned
    ///order and data addresses are retained; do not sort/deduplicate them.
    fn candidates(&mut self, state:&mut State, memory:&mut Memory, graph:usize,
        minimum:[f32;3], maximum:[f32;3], mode:u32, capacity:u32)->Result<Vec<u32>>;
    fn bind_graph_point(&mut self, state:&mut State, memory:&mut Memory, graph:usize, point:u32)->Result<()>;
    fn lpf_context(&mut self, state:&mut State, memory:&mut Memory)->Result<usize>;
    fn context_contains(&mut self, state:&mut State, memory:&mut Memory, context:usize, point:u32)->Result<u8>;
    fn spatial_service_184(&mut self, state:&mut State, memory:&mut Memory)->Result<usize>;
    fn query34(&mut self, state:&mut State, memory:&mut Memory, service:usize,
        start:&mut WalkPoint, end:&mut WalkPoint, radius:f32, mode:u32)->Result<u8>;
    fn query20(&mut self, state:&mut State, memory:&mut Memory, service:usize,
        start:&mut WalkPoint, end:&mut WalkPoint, output:&mut WalkPoint, normal:&mut[f32;3], radius:f32)->Result<()>;
    fn lpf_module(&mut self, state:&mut State, memory:&mut Memory)->Result<usize>;
    ///441fa4. Iterator, endpoint, crossing, edge and optional special byte
    ///are distinct retained outputs, matching the native stack objects.
    fn cross_region(&mut self, state:&mut State, memory:&mut Memory, region:u32, iterator:&mut u32,
        start:u32, end:&mut[f32;3], lpf:usize, crossing:&mut[f32;3], edge:&mut u32, special:&mut Option<u8>)->Result<u8>;
    fn base_cost(&mut self, state:&mut State, memory:&mut Memory, graph:usize,
        point:[f32;3], goal:u32, radius:f32)->Result<f64>;
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::collections::BTreeMap;
    const BASE:usize=0x62000000;
    const PATH:usize=0x63000000;
    const GRAPH:usize=0x66000000;
    const SPATIAL:usize=0x68000000;
    const LPF:usize=0x69000000;
    const CONTEXT:usize=0x69001000;
    const REGION:u32=0x6a000000;
    const REGION_OWNER:u32=0x6a001000;
    const POLYGON:u32=0x6b000000;
    const SOURCE:u32=0x6c000000;
    const OUTPUT:u32=0x6d000000;
    const POINTS:u32=0x6e000000;
    const CACHE:u32=0x6f100000;
    fn bits(values:[f32;3])->[u32;3] {values.map(f32::to_bits)}
    pub(super) fn decoded(value:&Value)->[f32;3] {serde_json::from_value::<[u32;3]>(value.clone()).unwrap().map(f32::from_bits)}
    pub(in crate::blotworx) fn summary(point:&WalkPoint)->Value {
        json!({"position":point.position,"auxiliary":point.auxiliary.iter().enumerate()
            .filter(|(_,v)|**v!=0).map(|(i,v)|(i,*v)).collect::<Vec<_>>()})
    }
    pub(in crate::blotworx) fn point(memory:&Memory,address:u32)->Result<WalkPoint> {
        let mut auxiliary=[0;16];for (i,value) in auxiliary.iter_mut().enumerate(){*value=word(memory,address,0x10+i as u32*4)?;}
        Ok(WalkPoint{position:bits(position(memory,address)?),auxiliary})
    }
    fn make_point(memory:&mut Memory,address:u32,position:[f32;3],auxiliary:u32)->Result<()> {
        memory.regions.insert(address,vec![0xa5;0x50]);memory.write(address,0x4ec1b0)?;
        set_position(memory,address,position)?;
        for i in 0..16 {memory.write(address+0x10+i*4,if auxiliary==0 {0}else{auxiliary+i})?;}
        Ok(())
    }
    pub(in crate::blotworx) struct Boundary<'a>{pub(in crate::blotworx) input:&'a Value,pub(in crate::blotworx) trace:Vec<Value>,pub(in crate::blotworx) current:usize}
    impl Boundary<'_> {
        fn row(&self)->&Value {&self.input["rows"][self.current]}
    }
    impl Units for Boundary<'_> {
        fn unit_context(&mut self)->Result<Option<usize>> {
            let present=!self.input["scale"].is_null();self.trace.push(json!(["unit_context",present]));Ok(present.then_some(80))
        }
        fn context_scale(&mut self,context:usize)->Result<f32> {
            assert_eq!(context,80);
            let index=self.trace.iter().filter(|row|row[0].as_str()==Some("scale")).count();
            let value=if let Some(values)=self.input["outer"]["scales"].as_array(){&values[index.min(values.len()-1)]}else{&self.input["scale"]};
            self.trace.push(json!(["scale",value]));Ok(f32::from_bits(value.as_u64().context("Missing scale")? as u32))
        }
    }
    impl Host for Boundary<'_> {
        fn graph_handle_64(&mut self,state:&mut State,_:&mut Memory)->Result<Option<usize>> {
            assert_eq!(state.pathfinder,PATH);let present=self.input["handle"].as_bool().unwrap();
            self.trace.push(json!(["handle",present]));Ok(present.then_some(GRAPH))
        }
        fn graph_ready(&mut self,_:&mut State,_:&mut Memory,graph:usize)->Result<u8> {
            assert_eq!(graph,GRAPH);self.trace.push(json!(["ready",self.input["ready"]]));Ok(self.input["ready"].as_u64().unwrap() as u8)
        }
        fn spatial_provider(&mut self,_:&mut State,_:&mut Memory)->Result<Option<usize>> {
            let present=self.input["record"].as_bool().unwrap();self.trace.push(json!(["provider_record",present]));
            if !present {return Ok(None)}
            let present=self.input["provider"].as_bool().unwrap();self.trace.push(json!(["provider",present]));Ok(present.then_some(SPATIAL))
        }
        fn actor_attribute(&mut self,state:&mut State,_:&mut Memory,attribute:Attribute)->Result<Option<f32>> {
            let name=if attribute==Attribute::Diameter {"diameter"}else{"height"};
            let value=self.input[name].as_u64().map(|v|v as u32);
            self.trace.push(json!(["attribute",name,value]));
            if attribute==Attribute::Height && self.input["activate_on_height"].as_bool()==Some(true) {state.word_40=REGION;}
            Ok(value.map(f32::from_bits))
        }
        fn graph_radius(&mut self,_:&mut State,_:&mut Memory,graph:usize)->Result<f32> {
            assert_eq!(graph,GRAPH);self.trace.push(json!(["radius",self.input["radius"]]));Ok(f32::from_bits(self.input["radius"].as_u64().unwrap() as u32))
        }
        fn fallback_radius(&mut self,_:&mut State,_:&mut Memory)->Result<f32> {
            self.trace.push(json!(["fallback",self.input["fallback"]]));Ok(f32::from_bits(self.input["fallback"].as_u64().unwrap() as u32))
        }
        fn candidates(&mut self,_:&mut State,_:&mut Memory,graph:usize,minimum:[f32;3],maximum:[f32;3],mode:u32,capacity:u32)->Result<Vec<u32>> {
            assert_eq!(graph,GRAPH);self.trace.push(json!(["candidates",bits(minimum),bits(maximum),mode,capacity]));
            Ok((0..self.input["rows"].as_array().unwrap().len()).map(|i|POINTS+i as u32*0x100).collect())
        }
        fn bind_graph_point(&mut self,_:&mut State,memory:&mut Memory,graph:usize,point:u32)->Result<()> {
            assert_eq!(graph,GRAPH);self.current=((point-POINTS)/0x100) as usize;self.trace.push(json!(["bind",self.current]));
            memory.write(GRAPH as u32+4,point)?;memory.write(GRAPH as u32+8,0)
        }
        fn lpf_context(&mut self,_:&mut State,_:&mut Memory)->Result<usize>{self.trace.push(json!(["context"]));Ok(if self.input["outer"]["context_null"].as_bool()==Some(true){0}else{CONTEXT})}
        fn context_contains(&mut self,_:&mut State,_:&mut Memory,context:usize,point:u32)->Result<u8> {
            assert_eq!(context,CONTEXT);assert_eq!(point,POINTS+self.current as u32*0x100+4);
            let result=self.row()["contains"].as_u64().unwrap() as u8;self.trace.push(json!(["contains",self.current,result]));Ok(result)
        }
        fn spatial_service_184(&mut self,_:&mut State,_:&mut Memory)->Result<usize>{self.trace.push(json!(["service"]));Ok(if self.input["outer"]["service"].as_bool()==Some(false){0}else{SPATIAL})}
        fn query34(&mut self,_:&mut State,_:&mut Memory,service:usize,start:&mut WalkPoint,end:&mut WalkPoint,radius:f32,mode:u32)->Result<u8> {
            assert_eq!(service,SPATIAL);self.trace.push(json!(["query34",self.current,radius.to_bits(),summary(start),summary(end),mode]));
            Ok(self.row()["accept"].as_u64().unwrap() as u8)
        }
        fn query20(&mut self,_:&mut State,memory:&mut Memory,service:usize,start:&mut WalkPoint,end:&mut WalkPoint,output:&mut WalkPoint,normal:&mut[f32;3],radius:f32)->Result<()> {
            assert_eq!(service,SPATIAL);self.trace.push(json!(["query20",self.current,radius.to_bits(),summary(start),summary(end),summary(output),bits(*normal)]));
            output.position=serde_json::from_value(self.row()["proposal"].clone())?;end.auxiliary[0]=0x12340000+self.current as u32;
            if self.input["mutate_candidate"].as_bool()==Some(true) {set_position(memory,POINTS+self.current as u32*0x100,output.position.map(f32::from_bits))?;}
            Ok(())
        }
        fn lpf_module(&mut self,_:&mut State,_:&mut Memory)->Result<usize>{Ok(LPF)}
        fn cross_region(&mut self,_:&mut State,memory:&mut Memory,region:u32,iterator:&mut u32,start:u32,end:&mut[f32;3],lpf:usize,crossing:&mut[f32;3],edge:&mut u32,special:&mut Option<u8>)->Result<u8> {
            assert_eq!(region,REGION);assert_eq!(lpf,LPF);let index=((*iterator-POLYGON)/0x100) as usize;
            let raw=[word(memory,start,0)?,word(memory,start,4)?,word(memory,start,8)?];
            self.trace.push(json!(["cross",self.current,index,raw,bits(*end),bits(*crossing),*edge]));
            let row=&self.input["regions"][index];*crossing=decoded(&row["hit"]);*edge=100+index as u32;
            if let Some(value)=row["special"].as_u64(){*special=Some(value as u8);}
            if !row["end"].is_null(){*end=decoded(&row["end"]);}
            Ok(row["result"].as_u64().unwrap() as u8)
        }
        fn base_cost(&mut self,_:&mut State,memory:&mut Memory,graph:usize,values:[f32;3],goal:u32,radius:f32)->Result<f64> {
            assert_eq!(graph,GRAPH);self.trace.push(json!(["base_cost",self.current,bits(values),radius.to_bits(),summary(&point(memory,goal)?)]));
            Ok(f64::from_bits(self.row()["cost"].as_u64().unwrap()))
        }
    }
    pub(in crate::blotworx) fn prepare(input:&Value)->Result<(State,Memory,u32)> {
            let mut state=State::construct(BASE,PATH,b"Refine");
            state.word_40=if input["active"].as_bool().unwrap(){REGION}else{0};
            let mut memory=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride:0,journal:BTreeMap::new()};
            make_point(&mut memory,SOURCE,decoded(&input["source"]),0x1000)?;
            make_point(&mut memory,OUTPUT,decoded(&input["output"]),0x2000)?;
            for (i,row) in input["rows"].as_array().unwrap().iter().enumerate(){make_point(&mut memory,POINTS+i as u32*0x100,decoded(&row["point"]),0x3000+i as u32*0x100)?;}
            memory.regions.insert(GRAPH as u32,vec![0xa5;0x50]);memory.regions.insert(REGION,vec![0xa5;0x20]);
            memory.regions.insert(REGION_OWNER,vec![0xa5;0x10]);memory.write(REGION+8,REGION_OWNER)?;
            let regions=input["regions"].as_array().unwrap();memory.write(REGION_OWNER+8,if regions.is_empty(){0}else{POLYGON})?;
            for i in 0..regions.len(){let p=POLYGON+i as u32*0x100;memory.regions.insert(p,vec![0;16]);memory.write(p+12,if i+1<regions.len(){p+0x100}else{0})?;}
            let output=if input["alias"].as_bool().unwrap(){SOURCE}else{OUTPUT};
            memory.regions.insert(CACHE,vec![0;0x1c]);
            for (i,value) in [u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,0].into_iter().enumerate(){memory.write(CACHE+i as u32*4,value)?;}
            Ok((state,memory,output))
    }
    #[test]
    fn refine_projection_full_candidate_control_flow_matches_original()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_refine_projection.json"))?;
        for (index,case) in fixture["cases"].as_array().context("Missing cases")?.iter().enumerate() {
            let input=&case["input"];let (mut state,mut memory,output)=prepare(input)?;
            let mut host=Boundary{input,trace:Vec::new(),current:0};
            if input["entry"].as_bool()==Some(true) {state.initialize_projected_point(&mut memory,SOURCE,output,0,CACHE,&mut host)?;}
            else {state.project_point(&mut memory,SOURCE,output,&mut host)?;}
            assert_eq!(json!(host.trace),case["trace"],"case{index} {input}");
            let words=|p:u32|->Result<Vec<u32>>{(0..20).map(|i|word(&memory,p,i*4)).collect()};
            assert_eq!(json!(words(SOURCE)?),case["source"],"source case{index}");
            assert_eq!(json!(words(output)?),case["output"],"output case{index}");
            let points:Vec<_>=(0..input["rows"].as_array().unwrap().len()).map(|i|words(POINTS+i as u32*0x100)).collect::<Result<_>>()?;
            assert_eq!(json!(points),case["points"],"points case{index}");assert_eq!(json!(state.word_40!=0),case["active"],"case{index}");
            assert_eq!(json!(state.base_vectors.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>()),case["base_vectors"],"cache case{index}");
        }
        Ok(())
    }
}
fn fresh_point(position:[f32;3])->WalkPoint {WalkPoint{position:position.map(f32::to_bits),auxiliary:[0;16]}}
fn squared(delta:[f32;3])->f32 {delta[1].mul_add(delta[1],delta[2].mul_add(delta[2],delta[0]*delta[0]))}
struct Projection<'a,H>{host:&'a mut H}
impl<H:Host> Units for Projection<'_,H> {
    fn unit_context(&mut self)->Result<Option<usize>> {self.host.unit_context()}
    fn context_scale(&mut self,context:usize)->Result<f32> {self.host.context_scale(context)}
}
impl<H:Host> PointHost for Projection<'_,H> {
    fn refine_20(&mut self,state:&mut State,memory:&mut Memory,source:u32,output:u32)->Result<()> {
        state.project_point(memory,source,output,self.host)
    }
}
impl State {
    /// Connect193d54 to THIS concrete class's actual194458 implementation.
    pub fn initialize_projected_point(&mut self,memory:&mut Memory,source:u32,output:u32,
        classified:u8,cache:u32,host:&mut impl Host)->Result<()> {
        self.initialize_point(memory,source,output,classified,cache,&mut Projection{host})
    }
    pub fn goal_cost(&mut self,memory:&mut Memory,graph:usize,point:[f32;3],goal:u32,
        radius:f32,host:&mut impl Host)->Result<f64> {
        if self.word_40==0 {return host.base_cost(self,memory,graph,point,goal,radius);}
        let goal=position(memory,goal)?;
        Ok(f64::from(squared(std::array::from_fn(|i|goal[i]-point[i]))))
    }
    pub fn project_point(&mut self,memory:&mut Memory,source:u32,output:u32,host:&mut impl Host)->Result<()> {
        let Some(graph)=host.graph_handle_64(self,memory)? else {return Ok(())};
        if host.graph_ready(self,memory,graph)?==0 {return Ok(())}
        if host.spatial_provider(self,memory)?.is_none() {return Ok(())}
        let radius=host.actor_attribute(self,memory,Attribute::Diameter)?.map_or(0.,|v|0.5*v);
        let graph_radius=host.graph_radius(self,memory,graph)?;
        let distance=if graph_radius == -1. {host.fallback_radius(self,memory)?}else{graph_radius};
        let extent=distance*1.5;
        // This goal object survives all candidate queries, including their
        //writes to its auxiliary data; it is not recreated every iteration.
        let goal=position(memory,source)?;
        let mut query_goal=fresh_point(goal);
        let candidates=host.candidates(self,memory,graph,goal.map(|v|v-extent),goal.map(|v|v+extent),1,256)?;
        ensure!(candidates.len()<=256,"Graph candidate query exceeded its actual native output capacity");
        let context=host.unit_context()?;
        let scale=if let Some(context)=context {host.context_scale(context)?} else {1.};
        let margin=f32::from_bits(0x3dcccccd)*scale;
        let mut best=f64::from(f32::MAX);
        // Native creates this storage even for a zero result; keep it outside
        //the loop so callback mutations have the same lifetime.
        let mut crossing=[0.;3];
        for candidate in candidates {
            host.bind_graph_point(self,memory,graph,candidate)?;
            let mut start=fresh_point(position(memory,candidate)?);
            let mut point=position(memory,source)?;
            let mut projected=fresh_point(point);
            if self.word_40!=0 {
                let context=host.lpf_context(self,memory)?;
                if host.context_contains(self,memory,context,candidate.wrapping_add(4))?!=0 {continue;}
                let service=host.spatial_service_184(self,memory)?;
                let mut reverse_start=fresh_point(position(memory,candidate)?);
                let mut reverse_end=fresh_point(position(memory,source)?);
                if host.query34(self,memory,service,&mut reverse_start,&mut reverse_end,radius,5)?==0 {continue;}
                crossing=point;
                let mut edge=0;
                let owner=word(memory,self.word_40,8)?;
                let mut iterator=word(memory,owner,8)?;
                let mut special=None;
                while iterator!=0 {
                    let region=self.word_40;
                    let lpf=host.lpf_module(self,memory)?;
                    let result=host.cross_region(self,memory,region,&mut iterator,candidate.wrapping_add(4),
                        &mut point,lpf,&mut crossing,&mut edge,&mut special)?;
                    if result!=0 && special.context("Successful region crossing did not initialize its special byte")?!=0 {break;}
                    iterator=word(memory,iterator,12)?;
                }
                let source_position=position(memory,candidate)?;
                point=std::array::from_fn(|i|{
                    let difference=crossing[i]-source_position[i];
                    let step=f32::from_bits(0x3f7d70a4)*difference;
                    source_position[i]+step
                });
                crossing=point;
            } else {
                let service=host.spatial_service_184(self,memory)?;
                let mut normal=[0.;3];
                host.query20(self,memory,service,&mut start,&mut query_goal,&mut projected,&mut normal,radius)?;
                point=projected.position.map(f32::from_bits);
            }
            if let Some(height)=host.actor_attribute(self,memory,Attribute::Height)? {
                let source_position=position(memory,source)?;
                let vertical=(source_position[1]-point[1]).abs();
                if vertical>height {
                    let dx=source_position[0]-point[0];let dz=source_position[2]-point[2];
                    let horizontal=super::cursor::native_sqrt(dz.mul_add(dz,dx*dx));
                    if horizontal<vertical {continue;}
                }
            }
            let candidate_position=position(memory,candidate)?;
            let delta=std::array::from_fn(|i|point[i]-candidate_position[i]);
            let length=super::cursor::native_sqrt(squared(delta));
            if length<=margin {point=candidate_position;} else {
                let fraction=(length-margin)/length;
                point=std::array::from_fn(|i|candidate_position[i]+fraction*(point[i]-candidate_position[i]));
                // Source194b14 restores candidate height only on this branch.
                point[1]=candidate_position[1];
            }
            let score=self.goal_cost(memory,graph,point,source,radius,host)?;
            if score<best {
                best=score;set_position(memory,output,point)?;
            }
        }
        // Retained crossing storage has no externally published result.
        let _=crossing;
        Ok(())
    }
}
