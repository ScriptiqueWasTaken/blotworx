//!4688d4 task lookup and469bd0/46a0f4/46a53c budget operations.
//! Manager registration and the original optional clock belong to the caller.
use anyhow::{Context,Result,ensure};
#[derive(Debug,serde::Serialize)]
pub struct Task {
    pub name:Vec<u8>,
    pub maximum_time:f32,
    pub maximum_calls:i32,
    pub calls:u32,
    pub started:f32,
    pub elapsed:f32,
}
#[derive(Debug,serde::Serialize)]
pub struct State {pub identity:usize,pub limit:f32,pub exhausted:u8,pub entries:Vec<Task>}
impl State {
    pub fn find(&self,name:Option<&[u8]>)->Result<i32>{
        u32::try_from(self.entries.len()).context("Task count exceeds native word domain")?;
        for (i,task) in self.entries.iter().enumerate(){
            let name=name.context("Task lookup dereferences a null name")?;
            if super::ai_traversal_registry::names_equal(name,&task.name){return Ok(i as u32 as i32)}
        }
        Ok(-1)
    }
}
pub trait Host {
    ///None resolves World+4c; Some resolves that captured manager identity.
    fn tasks(&mut self,identity:Option<usize>)->Result<&mut State>;
    ///None is the verified uninstalled callback, whose source value is0.
    fn task_clock(&mut self)->Result<Option<f32>>;
}
fn manager(h:&mut impl Host,id:usize)->Result<&mut State>{
    ensure!(id!=0,"Required task manager is null");
    let state=h.tasks(Some(id))?;ensure!(state.identity==id,"Task manager identity differs");Ok(state)
}
pub fn current_identity(h:&mut impl Host)->Result<usize>{let id=h.tasks(None)?.identity;ensure!(id!=0,"Required task manager is null");Ok(id)}
pub fn lookup(h:&mut impl Host,id:usize,name:Option<&[u8]>)->Result<i32>{manager(h,id)?.find(name)}
pub fn enabled(h:&mut impl Host,id:usize,task:u32)->Result<bool>{
    {
        let state=manager(h,id)?;
        if state.limit<=0.{return Ok(true)}
        if task as usize>=state.entries.len() || state.exhausted!=0{return Ok(false)}
    }
    let now=h.task_clock()?.unwrap_or(0.);
    let state=manager(h,id)?;state.exhausted=u8::from(now>state.limit);
    if state.exhausted!=0{return Ok(false)}
    let entry=state.entries.get(task as usize).context("Clock invalidated task backing storage")?;
    if entry.calls as i32>=entry.maximum_calls{return Ok(false)}
    //Native ble accepts unordered elapsed/maximum values (GT is clear).
    Ok(!(entry.elapsed>entry.maximum_time))
}
pub fn enter(h:&mut impl Host,id:usize,task:u32)->Result<()>{
    {
        let state=manager(h,id)?;
        let Some(entry)=state.entries.get_mut(task as usize) else{return Ok(())};
        entry.calls=entry.calls.wrapping_add(1);
    }
    let now=h.task_clock()?.unwrap_or(0.);
    manager(h,id)?.entries.get_mut(task as usize).context("Clock invalidated task start storage")?.started=now;Ok(())
}
pub fn leave(h:&mut impl Host,id:usize,task:u32)->Result<()>{
    {
        let state=manager(h,id)?;
        if state.limit<=0. || task as usize>=state.entries.len(){return Ok(())}
    }
    let now=h.task_clock()?.unwrap_or(0.);
    let entry=manager(h,id)?.entries.get_mut(task as usize).context("Clock invalidated task timing storage")?;
    let delta=now-entry.started;entry.elapsed+=delta;Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    const M:usize=0x72000000;
    struct Fixture<'a>{c:&'a Value,state:State,trace:Vec<Value>}
    impl Fixture<'_>{fn n(&self,k:&str,d:u32)->u32{self.c[k].as_u64().unwrap_or(d as u64) as u32}}
    impl Host for Fixture<'_>{
        fn tasks(&mut self,id:Option<usize>)->Result<&mut State>{if let Some(id)=id{assert_eq!(id,M);}Ok(&mut self.state)}
        fn task_clock(&mut self)->Result<Option<f32>>{
            if !self.c["clock"].as_bool().unwrap_or(true){return Ok(None)}
            let i=self.n("task",0) as usize;let e=&self.state.entries[i];self.trace.push(json!(["clock",e.calls,e.started.to_bits(),e.elapsed.to_bits(),self.state.exhausted]));
            match self.c["mutation"].as_str().unwrap_or("none"){
                "limit"=>self.state.limit=1000.,"quota"=>self.state.entries[i].maximum_calls=0,
                "cost"=>self.state.entries[i].elapsed=1000.,"start"=>self.state.entries[i].started=10.,
                "exhausted"=>self.state.exhausted=1,_=>{},
            }
            Ok(Some(f32::from_bits(self.n("now",5f32.to_bits()))))
        }
    }
    #[test]
    fn original_lookup_budget_clock_order_and_mutated_statistics_match()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_task_manager.json"))?;
        for row in fixture["cases"].as_array().unwrap(){let c=&row["input"];let n=|k:&str,d:u32|c[k].as_u64().unwrap_or(d as u64) as u32;
            let names=c["names"].as_array().map_or_else(||vec!["Alpha","Mpd::JPSFleeAgent::CheckSafety","ALPHA"],|a|a.iter().map(|v|v.as_str().unwrap()).collect());
            let entries=names.into_iter().map(|name|Task{name:name.chars().map(|v|v as u8).collect(),maximum_time:f32::from_bits(n("maximum_time",100f32.to_bits())),maximum_calls:n("quota",1) as i32,calls:n("calls",0),started:3.,elapsed:f32::from_bits(n("elapsed",0))}).collect();
            let mut h=Fixture{c,state:State{identity:M,limit:f32::from_bits(n("limit",100f32.to_bits())),exhausted:n("exhausted",0) as u8,entries},trace:vec![]};
            let id=n("task",0);let result=match c["op"].as_str().unwrap(){
                "enabled"=>json!(u32::from(enabled(&mut h,M,id)?)),
                "enter"=>{enter(&mut h,M,id)?;Value::Null},"leave"=>{leave(&mut h,M,id)?;Value::Null},
                "lookup"=>{let query=c["query"].as_str().map(|s|s.chars().map(|v|v as u8).collect::<Vec<_>>());json!(lookup(&mut h,M,query.as_deref())? as u32)},_=>unreachable!(),
            };
            assert_eq!(result,row["result"],"result {c}");assert_eq!(json!(h.trace),row["trace"],"clock order {c}");
            assert_eq!(h.state.limit.to_bits(),row["limit"].as_u64().unwrap() as u32,"limit {c}");assert_eq!(h.state.exhausted,row["exhausted"].as_u64().unwrap() as u8,"flag {c}");
            let entries=h.state.entries.iter().map(|e|[e.maximum_time.to_bits(),e.maximum_calls as u32,e.calls,e.started.to_bits(),e.elapsed.to_bits()]).collect::<Vec<_>>();assert_eq!(json!(entries),row["entries"],"statistics {c}");
        }
        let empty=State{identity:M,limit:0.,exhausted:0,entries:vec![]};assert_eq!(empty.find(None)?,-1);
        Ok(())
    }
}
