//! Resource admission478674/47873c and incremental link scheduling477edc.
//! Geometry linking, registered listeners and context clock are mandatory services.
use anyhow::{Context as _,Result};
use super::{ai_graph_handles::State,ai_graph_loading::Loader,ai_graph_resource_group::Group};
#[derive(Clone,Copy,Debug,serde::Serialize,serde::Deserialize)]
pub struct Listener {pub function:usize,pub data:u32}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct Admission {pub index154:u32,pub index15c:u32,pub word82e4:u32,pub changed14:u8,pub listeners:Vec<Listener>}
pub trait Host {
    fn adjacent(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,a:Option<u32>,b:u32)->Result<u8>;
    fn link(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,a:Option<u32>,b:u32)->Result<()>;
    fn finalize(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>)->Result<()>;
    fn arrived(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State,resource:Option<u32>,listener:Listener)->Result<()>;
    fn clock_bits(&mut self,admission:&mut Admission,loader:&mut Loader,state:&mut State)->Result<u32>;
}
fn group(loader:&Loader,index:usize)->Result<&Group>{loader.groups[index].as_ref().context("Admission group pointer is null")}
fn count(loader:&Loader,index:usize)->Result<u32>{u32::try_from(group(loader,index)?.resources.len()).context("Admission group exceeds native count domain")}
impl Admission {
    pub fn select(&mut self,loader:&Loader,state:&State)->Result<Option<u32>> {
        let mut index=0;
        if let Some(id)=group(loader,0)?.first_state3_unprioritized(state,&mut index)? {
            self.index15c=0;self.index154=index.wrapping_sub(1);
            if (self.index154 as i32)<0 {self.index154=self.index154.wrapping_add(count(loader,0)?);}
            return Ok(Some(id))
        }
        let resource=group(loader,1)?.first_state3_unprioritized(state,&mut index)?;
        if let Some(id)=resource {
            let key=state.resources.get(&id).context("Admission resource is absent")?.word1c;
            if loader.records.get(&key).context("Admission directory record is absent")?.word1c>1 {self.index154=count(loader,0)?.wrapping_sub(1);}
            self.index15c=index.wrapping_sub(1);
            if (self.index15c as i32)<0 {self.index15c=self.index15c.wrapping_add(count(loader,1)?);}
        }
        Ok(resource)
    }
    pub fn tick(&mut self,loader:&mut Loader,state:&mut State,host:&mut impl Host)->Result<u32> {
        if state.tracked_resource.is_some() {self.advance(loader,state,host)?;return Ok(0)}
        state.tracked_resource=self.select(loader,state)?;
        if let Some(id)=state.tracked_resource {
            state.resources.get_mut(&id).context("Selected admission resource is absent")?.state=4;
            self.word82e4=0;
        }
        Ok(1)
    }
    fn candidate(&mut self,loader:&mut Loader,state:&mut State,host:&mut impl Host,id:u32)->Result<bool> {
        if state.tracked_resource==Some(id) {return Ok(false)}
        let status=state.resources.get(&id).context("Admission neighbour is absent")?.state;
        if status!=5 && status!=6 {return Ok(false)}
        if host.adjacent(self,loader,state,state.tracked_resource,id)?!=0 {
            host.link(self,loader,state,state.tracked_resource,id)?;
            return Ok(true)
        }
        Ok(false)
    }
    pub fn advance(&mut self,loader:&mut Loader,state:&mut State,host:&mut impl Host)->Result<()> {
        if state.tracked158!=count(loader,0)? {
            loop {
                let id=*group(loader,0)?.resources.get(self.index154 as usize).context("Admission group3c index is invalid")?;
                let linked=self.candidate(loader,state,host,id)?;
                state.tracked158=state.tracked158.wrapping_add(1);
                self.index154=self.index154.wrapping_sub(1);
                if (self.index154 as i32)<0 {self.index154=self.index154.wrapping_add(count(loader,0)?);}
                if state.tracked158==count(loader,0)? || linked {return Ok(())}
            }
        }
        loop {
            let linked=if count(loader,1)?!=0 {
                let id=*group(loader,1)?.resources.get(self.index15c as usize).context("Admission group38 index is invalid")?;
                self.candidate(loader,state,host,id)?
            }else{false};
            state.tracked160=state.tracked160.wrapping_add(1);
            self.index15c=self.index15c.wrapping_sub(1);
            if (self.index15c as i32)<0 {self.index15c=self.index15c.wrapping_add(count(loader,1)?);}
            if (state.tracked160 as i32)<(count(loader,1)? as i32) {
                if linked {return Ok(())}
                continue
            }
            host.finalize(self,loader,state,state.tracked_resource)?;
            let resource=state.tracked_resource.context("Finalization removed the tracked graph resource")?;
            state.resources.get_mut(&resource).context("Finalized resource is absent")?.state=5;
            let mut index=0usize;
            while index<self.listeners.len() {
                let listener=self.listeners[index];host.arrived(self,loader,state,state.tracked_resource,listener)?;
                index+=1;
            }
            self.changed14=1;state.tracked160=0;state.tracked158=0;
            let mut index=0;
            while index<state.pool.len() {
                let id=state.pool[index];let node=state.nodes.get(&id).context("Admission pool node is absent")?;
                if node.free==0 && (if node.word14==0 {None}else{Some(node.word14)})==state.tracked_resource {
                    let bits=host.clock_bits(self,loader,state)?;
                    // Original reloads the pool base after the clock callback.
                    let id=*state.pool.get(index).context("Clock callback truncated graph pool")?;
                    state.nodes.get_mut(&id).context("Clock callback removed graph node")?.scalar2c=bits;
                }
                index+=1;
            }
            state.tracked_resource=None;
            return Ok(())
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a> {c:&'a Value,trace:Vec<Value>}
    impl Fixture<'_> {
        fn snapshot(a:&Admission,s:&State)->Value {json!([s.tracked_resource,s.tracked_resource.map(|id|s.resources[&id].state),s.tracked158,s.tracked160,a.index154,a.index15c])}
        fn mutate(&self,kind:&str,a:&mut Admission,s:&mut State)->Result<()> {
            let m=&self.c["mutations"][kind];
            for f in ["index154","index15c","word82e4","tracked158","tracked160"] {
                if let Some(v)=m[f].as_u64() {let v=v as u32;match f {"index154"=>a.index154=v,"index15c"=>a.index15c=v,"word82e4"=>a.word82e4=v,"tracked158"=>s.tracked158=v,"tracked160"=>s.tracked160=v,_=>unreachable!()}}
            }
            if m.get("tracked_resource").is_some(){s.tracked_resource=serde_json::from_value(m["tracked_resource"].clone())?;}
            if let Some(states)=m["resource_states"].as_object(){for (k,v) in states{s.resources.get_mut(&k.parse()?).unwrap().state=v.as_u64().unwrap() as u32;}}
            if m["listeners"].is_array(){a.listeners=serde_json::from_value(m["listeners"].clone())?;}
            if m["pool"].is_array(){s.pool=serde_json::from_value(m["pool"].clone())?;}
            if let Some(v)=m["clock_node"].as_u64(){s.nodes.get_mut(&100).unwrap().word14=v as u32;}
            Ok(())
        }
    }
    impl Host for Fixture<'_> {
        fn adjacent(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,x:Option<u32>,y:u32)->Result<u8>{self.trace.push(json!(["adjacent",x,y,Self::snapshot(a,s)]));self.mutate("adjacent",a,s)?;Ok(self.c["adjacent"].as_u64().unwrap_or(0) as u8)}
        fn link(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,x:Option<u32>,y:u32)->Result<()>{self.trace.push(json!(["link",x,y,Self::snapshot(a,s)]));self.mutate("link",a,s)}
        fn finalize(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,x:Option<u32>)->Result<()>{self.trace.push(json!(["finalize",x,Self::snapshot(a,s)]));self.mutate("finalize",a,s)}
        fn arrived(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State,x:Option<u32>,listener:Listener)->Result<()>{self.trace.push(json!(["arrived",x,listener,Self::snapshot(a,s)]));self.mutate("arrived",a,s)}
        fn clock_bits(&mut self,a:&mut Admission,_:&mut Loader,s:&mut State)->Result<u32>{self.trace.push(json!(["clock",Self::snapshot(a,s)]));self.mutate("clock",a,s)?;Ok(self.c["clock"].as_u64().unwrap_or(0x40000000) as u32)}
    }
    #[test] fn original_admission_selection_incremental_linking_and_publication_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_admission.json"))?;
        for case in fixtures["cases"].as_array().unwrap() {
            let c=&case["input"];let mut a:Admission=serde_json::from_value(c["admission"].clone())?;
            let mut state:State=serde_json::from_value(c["state"].clone())?;let mut loader:Loader=serde_json::from_value(c["loader"].clone())?;
            let mut host=Fixture{c,trace:Vec::new()};
            let result=match c["op"].as_str().unwrap(){"select"=>json!(a.select(&loader,&state)?),"tick"=>json!(a.tick(&mut loader,&mut state,&mut host)?),"advance"=>{a.advance(&mut loader,&mut state,&mut host)?;Value::Null},_=>unreachable!()};
            assert_eq!(result,case["result"],"{c}");assert_eq!(host.trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(a)?,case["admission"],"{c}");assert_eq!(serde_json::to_value(state)?,case["state"],"{c}");
        }
        Ok(())
    }
}
