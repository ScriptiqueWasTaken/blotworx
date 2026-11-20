//! Complete JPSRefineGoal virtual1c, REL193fe4..194454. Uses the actual
//! point helper/projection and shared scaled-clock timestamp arithmetic.
//! World brain/LPF/metadata providers and normal scheduling remain required.
use anyhow::{ensure,Context,Result};
use std::collections::BTreeSet;
use super::{ai_refine_goal::{State,position,set_position,word},ai_refine_projection,
    ai_graph_link_passes::Memory,game_clock::Clock,locomotion_actions::ClockStamp};

pub trait Host:ai_refine_projection::Host {
    /// Brain virtual34 receives source+4 (194108), not the saved output
    ///point. The independently initialized cache remains canonical data.
    fn brain_query34(&mut self,state:&mut State,memory:&mut Memory,position:u32,
        classified:&mut u8,cache:u32)->Result<u32>;
    fn clock(&mut self)->Result<Clock>;
    /// Actual LPF context+d0. Never substitute zero for an absent context.
    fn context_scalar_d0(&mut self,state:&mut State,memory:&mut Memory,context:usize)->Result<f32>;
    ///44d1fc writes the class's actual word40 in place, including on failure.
    fn find_region(&mut self,state:&mut State,memory:&mut Memory,context:usize,position:u32)->Result<u8>;
    fn service_class(&mut self,state:&mut State,memory:&mut Memory,service:usize)->Result<usize>;
    fn parent_class(&mut self,state:&mut State,memory:&mut Memory,class:usize)->Result<Option<usize>>;
    /// Registered class corresponding to native BSS11864, not an invented
    ///ready CCanGo instance or a guessed serialized class index.
    fn cango_class(&self)->usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_refine_projection::tests::{Boundary,prepare,point,summary},game_clock::Rate};
    use serde_json::{Value,json};
    use std::num::NonZeroU32;
    const REGION:u32=0x6a000000;
    const SOURCE:u32=0x6c000000;
    const POINTS:u32=0x6e000000;
    const CONTEXT:usize=0x69001000;
    const SCRATCH:u32=0x72000000;
    const CANGO:usize=0x11864;
    impl Host for Boundary<'_> {
        fn brain_query34(&mut self,state:&mut State,memory:&mut Memory,position:u32,classified:&mut u8,cache:u32)->Result<u32> {
            assert_eq!(state.word_40,0);assert_eq!(position,SOURCE+4);assert_eq!(*classified,0);
            let values:Vec<_>=(0..7).map(|i|word(memory,cache,i*4)).collect::<Result<_>>()?;
            self.trace.push(json!(["brain34",summary(&point(memory,position-4)?),values]));
            let config=&self.input["outer"];*classified=config["classified"].as_u64().unwrap() as u8;
            if let Some(values)=config["cache"].as_array(){for (i,value) in values.iter().enumerate(){memory.write(cache+i as u32*4,value.as_u64().unwrap() as u32)?;}}
            Ok(config["query_result"].as_u64().unwrap() as u32)
        }
        fn clock(&mut self)->Result<Clock> {
            let config=&self.input["outer"];let values=config["clock_ticks"].as_array().unwrap();
            let index=self.trace.iter().filter(|row|row[0].as_str()==Some("clock")).count();
            let ticks=values[index.min(values.len()-1)].as_u64().unwrap();self.trace.push(json!(["clock",ticks]));
            let frequency=config["frequency"].as_u64().unwrap() as u32;
            Ok(Clock::new(Rate{ticks_per_second:NonZeroU32::new(frequency).context("Zero fixture frequency")?,
                division_threshold:frequency.wrapping_mul(5) as i32},ticks))
        }
        fn context_scalar_d0(&mut self,_:&mut State,_:&mut Memory,context:usize)->Result<f32> {
            assert_eq!(context,CONTEXT);let bits=self.input["outer"]["context_value"].as_u64().unwrap() as u32;
            self.trace.push(json!(["context_d0",bits]));Ok(f32::from_bits(bits))
        }
        fn find_region(&mut self,state:&mut State,memory:&mut Memory,context:usize,position:u32)->Result<u8> {
            assert_eq!(context,CONTEXT);let config=&self.input["outer"];
            let values=[word(memory,position,0)?,word(memory,position,4)?,word(memory,position,8)?];
            self.trace.push(json!(["find_region",values,config["found"]]));
            state.word_40=if config["region"].as_bool().unwrap(){REGION}else{0};
            Ok(config["found"].as_u64().unwrap() as u8)
        }
        fn service_class(&mut self,_:&mut State,_:&mut Memory,service:usize)->Result<usize> {
            assert_eq!(service,0x68000000);let class=self.input["outer"]["classes"][0].as_u64().unwrap() as usize;
            self.trace.push(json!(["service_class",class]));Ok(class)
        }
        fn parent_class(&mut self,_:&mut State,_:&mut Memory,class:usize)->Result<Option<usize>> {
            let classes=self.input["outer"]["classes"].as_array().unwrap();
            let index=classes.iter().position(|v|v.as_u64()==Some(class as u64)).context("Unknown native metadata")?;
            let parent=classes.get(index+1).and_then(Value::as_u64).map(|v|v as usize);
            self.trace.push(json!(["parent",class,parent]));Ok(parent)
        }
        fn cango_class(&self)->usize {CANGO}
    }
    #[test]
    fn refine_entry_composes_actual_projection_cache_and_native_scaled_timers()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_refine_entry.json"))?;
        for (index,case) in fixture["cases"].as_array().context("Missing cases")?.iter().enumerate() {
            let input=&case["input"];let config=&input["outer"];
            let (mut state,mut memory,output)=prepare(input)?;
            let base:[[u32;3];2]=serde_json::from_value(config["base"].clone())?;
            let derived:[[u32;3];2]=serde_json::from_value(config["derived"].clone())?;
            state.base_vectors=base.map(|v|v.map(f32::from_bits));state.vectors_24_30=derived.map(|v|v.map(f32::from_bits));
            state.scalar_3c=f32::from_bits(config["last"].as_u64().unwrap() as u32);
            let stamp=config["stamp"].as_u64().unwrap();state.timer_words_4c_50_54=[None,Some((stamp>>32) as u32),Some(stamp as u32)];
            memory.regions.insert(SCRATCH,vec![0xa5;0x100]);
            let mut host=Boundary{input,trace:Vec::new(),current:0};state.refine_goal(&mut memory,SOURCE,output,SCRATCH,&mut host)?;
            assert_eq!(json!(host.trace),case["trace"],"trace case{index} {input}");
            let words=|address:u32|->Result<Vec<u32>>{(0..20).map(|i|word(&memory,address,i*4)).collect()};
            assert_eq!(json!(words(SOURCE)?),case["source"],"source case{index}");assert_eq!(json!(words(output)?),case["output"],"output case{index}");
            let points:Vec<_>=(0..input["rows"].as_array().unwrap().len()).map(|i|words(POINTS+i as u32*0x100)).collect::<Result<_>>()?;
            assert_eq!(json!(points),case["points"],"points case{index}");
            assert_eq!(json!(state.base_vectors.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>()),case["base_vectors"],"base case{index}");
            assert_eq!(json!(state.vectors_24_30.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>()),case["derived"],"derived case{index}");
            assert_eq!(json!(state.scalar_3c.to_bits()),case["last"],"scalar case{index}");assert_eq!(json!(state.stamp()?.native_ticks()),case["stamp"],"stamp case{index}");
            assert_eq!(state.timer_words_4c_50_54[0],None,"Unwritten timer word changed");assert_eq!(json!(state.word_40!=0),case["active"],"active case{index}");
        }
        Ok(())
    }
    #[test]
    fn refine_entry_rejects_missing_context_after_native_point_side_effects()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_refine_entry.json"))?;
        let mut input=fixture["cases"][0]["input"].clone();input["outer"]["classified"]=json!(1);
        input["outer"]["query_result"]=json!(0);input["outer"]["context_null"]=json!(true);
        let (mut state,mut memory,output)=prepare(&input)?;memory.regions.insert(SCRATCH,vec![0xa5;0x100]);
        let mut host=Boundary{input:&input,trace:Vec::new(),current:0};
        let error=state.refine_goal(&mut memory,SOURCE,output,SCRATCH,&mut host).unwrap_err();
        assert!(error.to_string().contains("actual initialized LPF context"));
        assert_eq!(position(&memory,output)?,position(&memory,SOURCE)?);
        assert!(!host.trace.iter().any(|row|row[0].as_str()==Some("clock")));
        assert_eq!(state.stamp()?.native_ticks(),0);
        Ok(())
    }
}
fn copy_point(memory:&mut Memory,source:u32,destination:u32)->Result<()> {
    memory.write(destination,0x4ec1b0)?;
    for offset in (4..0x50).step_by(4) {let value=word(memory,source,offset)?;memory.write(destination.wrapping_add(offset),value)?;}
    Ok(())
}
impl State {
    fn stamp(&self)->Result<ClockStamp> {
        let high=self.timer_words_4c_50_54[1].context("Refinement timer high word is uninitialized")?;
        let low=self.timer_words_4c_50_54[2].context("Refinement timer low word is uninitialized")?;
        Ok(ClockStamp::from_native_ticks((u64::from(high)<<32)|u64::from(low)))
    }
    /// Caller owns a distinct mapped0x100-byte scratch frame for this entry,
    ///including nested/reentrant calls; no recursion or entity cap is imposed.
    pub fn refine_goal(&mut self,memory:&mut Memory,source:u32,output:u32,scratch:u32,
        host:&mut impl Host)->Result<()> {
        self.word_40=0;
        let cache=scratch.wrapping_add(0x18);
        let saved_output=scratch.wrapping_add(0x88);
        copy_point(memory,output,saved_output)?;
        for (index,value) in [u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,0].into_iter().enumerate() {
            memory.write(cache.wrapping_add(index as u32*4),value)?;
        }
        let mut classified=0;
        let query=host.brain_query34(self,memory,source.wrapping_add(4),&mut classified,cache)?;
        let unclassified=query!=0 && classified==0;
        let mut refresh_timer=true;
        if unclassified {
            let stamp=self.stamp()?;
            if !stamp.is_running() {self.reset();} else {
                let clock=host.clock()?;
                let elapsed=clock.rate.seconds(stamp.elapsed_ticks(clock.scaled_ticks));
                if elapsed>2. {self.reset();} else {refresh_timer=false;}
            }
        }
        self.initialize_projected_point(memory,source,output,classified,cache,host)?;
        if unclassified && position(memory,source)?==position(memory,output)? {
            set_position(memory,output,[f32::MAX;3])?;
            return Ok(());
        }
        let context=host.lpf_context(self,memory)?;
        //193fe4 reads+d0 BEFORE its null branch. Native null dereference is
        //undefined, so diagnose it after retaining preceding side effects.
        ensure!(context!=0,"Refinement requires the actual initialized LPF context");
        let epoch=host.context_scalar_d0(self,memory,context)?;
        let mut reuse=!(self.scalar_3c<epoch);
        if reuse && self.vectors_24_30[0][0]!=f32::MAX {
            let current=position(memory,output)?;
            let delta:[f32;3]=std::array::from_fn(|i|current[i]-self.vectors_24_30[0][i]);
            let squared=delta[1].mul_add(delta[1],delta[2].mul_add(delta[2],delta[0]*delta[0]));
            let context=host.unit_context()?;
            let first=if let Some(context)=context {host.context_scale(context)?}else{1.};
            let second=if let Some(context)=context {host.context_scale(context)?}else{1.};
            let limit=(f32::from_bits(0x3c23d70a)*first)*second;
            // Native BLE tests NOT-GT, including unordered comparisons.
            reuse=!(squared>limit);
        }
        if reuse {set_position(memory,output,self.vectors_24_30[1])?;} else {
            self.vectors_24_30[0]=position(memory,output)?;
            let context=host.lpf_context(self,memory)?;
            if host.find_region(self,memory,context,output.wrapping_add(4))?!=0 {
                let service=host.spatial_service_184(self,memory)?;
                if service!=0 {
                    let mut class=host.service_class(self,memory,service)?;
                    let wanted=host.cango_class();
                    let mut visited=BTreeSet::new();
                    let matches=loop {
                        if class==wanted {break true;}
                        ensure!(visited.insert(class),"Cyclic refinement service metadata");
                        let Some(parent)=host.parent_class(self,memory,class)? else {break false};
                        class=parent;
                    };
                    if matches {
                        let saved=scratch.wrapping_add(0x38);
                        copy_point(memory,output,saved)?;
                        self.project_point(memory,saved,output,host)?;
                    }
                }
            }
            self.vectors_24_30[1]=position(memory,output)?;
        }
        self.scalar_3c=epoch;
        if refresh_timer {
            let clock=host.clock()?;
            let mut stamp=ClockStamp::default();stamp.reset(clock.scaled_ticks);
            self.timer_words_4c_50_54[1]=Some((stamp.native_ticks()>>32) as u32);
            self.timer_words_4c_50_54[2]=Some(stamp.native_ticks() as u32);
        }
        Ok(())
    }
}
