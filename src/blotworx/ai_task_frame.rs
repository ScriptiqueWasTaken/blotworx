//!468d54 frame reset and its exact actor-order sort468ea8/4693e4.
//! Actor budgets and task entries remain in their owning context throughout.
use anyhow::{Context,Result,ensure};
#[derive(Debug,serde::Serialize)]
pub struct Actor {pub elapsed:f32,pub weight:f32}
#[derive(Debug,serde::Serialize)]
pub struct Frame {
    pub previous:f32,pub current:f32,pub delta:f32,pub unaccounted:f32,
    pub work:u32,pub selected:i32,pub actors:Vec<Actor>,pub order:Vec<u32>,
}
#[derive(Debug,serde::Serialize)]
pub struct Globals {pub elapsed:f32,pub phases:[i32;2]}
impl Default for Globals {fn default()->Self{Self{elapsed:0.,phases:[-4,-4]}}}
pub trait Host:super::ai_task_manager::Host {
    fn frame(&mut self,manager:usize)->Result<&mut Frame>;
    fn task_globals(&mut self)->Result<&mut Globals>;
    fn world_actor_count(&mut self)->Result<u32>;
    ///Invoke the installed4e128+c callback, or preserve its actual absence.
    fn reset_task_clock(&mut self)->Result<()>;
}
fn key(h:&mut impl Host,id:usize,index:usize)->Result<u32>{h.frame(id)?.order.get(index).copied().context("Actor sort pointer is outside its order table")}
fn swap(h:&mut impl Host,id:usize,a:usize,b:usize)->Result<()>{
    let frame=h.frame(id)?;ensure!(a<frame.order.len()&&b<frame.order.len(),"Actor sort swap is outside its order table");frame.order.swap(a,b);Ok(())
}
///468cac re-queries the GLOBAL manager, even when sorting a captured manager.
fn less(h:&mut impl Host,id:usize,a:usize,b:usize)->Result<bool>{
    let left=key(h,id,a)?;let right=key(h,id,b)?;
    let current=super::ai_task_manager::current_identity(h)?;let frame=h.frame(current)?;
    if left as i32==frame.selected{return Ok(true)}
    if right as i32==frame.selected{return Ok(false)}
    let a=frame.actors.get(left as usize).context("Actor sort left record is absent")?;
    let b=frame.actors.get(right as usize).context("Actor sort right record is absent")?;
    Ok(a.elapsed*a.weight>b.elapsed*b.weight)
}
///4692b8 median selection. Equal/unordered comparisons retain native swaps.
fn median(h:&mut impl Host,id:usize,a:usize,b:usize,c:usize)->Result<()>{
    let first=!less(h,id,c,a)?;let second=!less(h,id,b,c)?;
    if first&&second{return Ok(())}
    if !first&&!second{return swap(h,id,a,b)}
    if less(h,id,b,a)?{swap(h,id,a,b)?;}
    if first{swap(h,id,b,c)}else{swap(h,id,a,c)}
}
fn previous(index:usize)->Result<usize>{index.checked_sub(1).context("Actor sort crossed its lower sentinel")}
fn at(base:usize,offset:isize)->Result<usize>{base.checked_add_signed(offset).context("Actor sort pivot address overflow")}
///Native selection-sort threshold20 and two independent global pivot phases.
///Smaller partition recursion uses phase1; tail iteration retains its phase.
fn sort(h:&mut impl Host,id:usize,mut begin:usize,mut end:usize,phase:usize)->Result<()>{
    loop {
        let count=end.checked_sub(begin).context("Actor sort range is reversed")?;
        if count<=1{return Ok(())}
        if count<=20 {
            for pos in begin..end-1 {
                let mut best=pos;
                for candidate in pos+1..end {if less(h,id,candidate,best)?{best=candidate;}}
                if best!=pos{swap(h,id,best,pos)?;}
            }
            return Ok(())
        }
        let old=h.task_globals()?.phases[phase];
        let a=at(begin,(count/4) as isize+(old%5) as isize)?;
        let mut next=old.wrapping_add(1);if next>=5{next=-4;}
        let b=at(begin,(count*3/4) as isize+(next%5) as isize)?;
        next=next.wrapping_add(1);if next>=5{next=-4;}
        h.task_globals()?.phases[phase]=next;
        let last=end-1;median(h,id,a,b,last)?;
        let mut left=begin;let mut right=last;
        while less(h,id,left,last)?{left+=1;}
        loop {right=previous(right)?;if left==right||less(h,id,right,last)?{break}}
        while left<right {
            swap(h,id,left,right)?;left+=1;
            while less(h,id,left,last)?{left+=1;}
            loop {right=previous(right)?;if less(h,id,right,last)?{break}}
        }
        if left==begin {
            swap(h,id,left,last)?;left+=1;right=last;
            if !less(h,id,begin,last)? {
                while left!=end&&!less(h,id,begin,left)?{left+=1;}
                if left<right{swap(h,id,left,right)?;}
            }
            while left<right {
                while !less(h,id,begin,left)?{left+=1;}
                loop {right=previous(right)?;if !less(h,id,begin,right)?{break}}
                if left<right{swap(h,id,left,right)?;left+=1;}
            }
            begin=left;
        } else if left-begin<end-left {
            sort(h,id,begin,left,1)?;begin=left;
        } else {
            sort(h,id,left,end,1)?;end=left;
        }
    }
}
pub fn begin(h:&mut impl Host,id:usize,delta:f32)->Result<()>{
    let delta=if delta<f32::from_bits(0x3a83126f){f32::from_bits(0x3a83126f)}else{delta};
    h.reset_task_clock()?;
    {
        let frame=h.frame(id)?;frame.delta=delta;frame.unaccounted=0.;frame.previous=frame.current;frame.current+=delta;
    }
    h.task_globals()?.elapsed=0.;
    for task in &mut h.tasks(Some(id))?.entries{task.elapsed=0.;task.calls=0;}
    let count=h.world_actor_count()? as usize;
    if count!=0 {
        ensure!(count<=i32::MAX as usize/4,"Actor sort exceeds its native signed pointer domain");
        ensure!(h.frame(id)?.order.len()>=count,"Actor order table is smaller than the World count");
        sort(h,id,0,count,0)?;
        let mut i=0;
        while i<h.world_actor_count()? as usize {
            h.frame(id)?.actors.get_mut(i).context("Actor frame record is absent")?.elapsed=0.;i+=1;
        }
    }
    let frame=h.frame(id)?;frame.selected=-1;frame.work=0;
    h.tasks(Some(id))?.exhausted=0;Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ai_task_manager::{State,Task};
    use serde_json::{Value,json};
    const M:usize=0x72000000;
    struct Fixture<'a>{input:&'a Value,state:State,frame:Frame,globals:Globals,trace:Vec<Value>}
    impl super::super::ai_task_manager::Host for Fixture<'_>{
        fn tasks(&mut self,id:Option<usize>)->Result<&mut State>{if let Some(id)=id{assert_eq!(id,M)}Ok(&mut self.state)}
        fn task_clock(&mut self)->Result<Option<f32>>{anyhow::bail!("Frame reset must not sample clock")}
    }
    impl Host for Fixture<'_>{
        fn frame(&mut self,id:usize)->Result<&mut Frame>{assert_eq!(id,M);Ok(&mut self.frame)}
        fn task_globals(&mut self)->Result<&mut Globals>{Ok(&mut self.globals)}
        fn world_actor_count(&mut self)->Result<u32>{Ok(self.input["count"].as_u64().unwrap() as u32)}
        fn reset_task_clock(&mut self)->Result<()>{
            if !self.input["clock"].as_bool().unwrap_or(true){return Ok(())}
            self.trace.push(json!(["reset",self.frame.current.to_bits(),self.frame.selected as u32,self.state.entries[0].calls,self.state.entries[0].elapsed.to_bits(),self.globals.elapsed.to_bits()]));
            match self.input["mutation"].as_str(){Some("time")=>self.frame.current=100.,Some("selected")=>self.frame.selected=self.frame.actors.len() as i32-1,_=>{}}
            Ok(())
        }
    }
    #[test]
    fn original_full_frame_cases()->Result<()>{
        let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_task_frame.json"))?;
        let rows=capture["cases"].as_array().unwrap();
        for row in rows {
            let input=&row["input"];let count=input["count"].as_u64().unwrap() as usize;let mode=input["mode"].as_str().unwrap();
            let actors=(0..count).map(|i|Actor{
                elapsed:match mode{"equal"=>1.,"ascending"=>(i+1) as f32,_=>(i%7+1) as f32},
                weight:if mode=="nan"&&i%5==0{f32::from_bits(0x7fc00000)}else if matches!(mode,"equal"|"ascending"){1.}else{((i*13+7)%11) as f32*0.25},
            }).collect();
            let mut order:Vec<u32>=(0..count as u32).collect();order.rotate_left(count/3);
            let mut h=Fixture{input,state:State{identity:M,limit:2.,exhausted:254,entries:(0..2).map(|i|Task{name:vec![i],maximum_time:3.,maximum_calls:4,calls:17+i as u32,started:9.,elapsed:5.+i as f32}).collect()},
                frame:Frame{previous:10.,current:11.,delta:12.,unaccounted:13.,work:9,selected:input["selected"].as_i64().unwrap() as i32,actors,order},
                globals:Globals{elapsed:44.,phases:[input["phase0"].as_i64().unwrap_or(-4) as i32,input["phase1"].as_i64().unwrap_or(-4) as i32]},trace:vec![]};
            begin(&mut h,M,f32::from_bits(input["delta"].as_u64().unwrap_or(0.016f32.to_bits() as u64) as u32)).with_context(||format!("{input}"))?;
            let f=&h.frame;
            assert_eq!(json!([f.previous.to_bits(),f.current.to_bits(),f.delta.to_bits(),f.unaccounted.to_bits(),f.work,f.selected as u32]),row["frame"],"{input}");
            assert_eq!(json!(h.trace),row["trace"],"{input}");
            assert_eq!(json!(h.state.exhausted),row["exhausted"],"{input}");
            assert_eq!(json!(h.state.entries.iter().map(|t|[t.calls,t.elapsed.to_bits()]).collect::<Vec<_>>()),row["tasks"],"{input}");
            assert_eq!(json!(f.actors.iter().map(|a|[a.elapsed.to_bits(),a.weight.to_bits()]).collect::<Vec<_>>()),row["actors"],"{input}");
            assert_eq!(json!(f.order),row["order"],"{input}");
            assert_eq!(json!([h.globals.elapsed.to_bits(),h.globals.phases[0] as u32,h.globals.phases[1] as u32]),row["globals"],"{input}");
            for t in &h.state.entries{assert_eq!((t.maximum_time,t.maximum_calls,t.started),(3.,4,9.));}
        }
        eprintln!("{} original full-frame cases",rows.len());Ok(())
    }
}
