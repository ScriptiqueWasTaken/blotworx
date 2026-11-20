//! Flee virtual10, REL1bef20..1bf278. Preserve the distinct missing-value
//! branches, raw boolean handle, unit conversion and native speed limit.
use anyhow::{Context,Result};
use super::{ai_flee_agent::State,ai_traversal_registry::names_equal};
pub trait Host {
    fn name(&mut self,s:&mut State,node:usize)->Result<Option<Vec<u8>>>;
    fn value(&mut self,s:&mut State,node:usize)->Result<Option<usize>>;
    fn text(&mut self,s:&mut State,value:usize)->Result<Vec<u8>>;
    fn integer(&mut self,s:&mut State,value:usize)->Result<i32>;
    fn number(&mut self,s:&mut State,value:usize)->Result<f64>;
    fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>;
    fn traversal(&mut self,s:&mut State,value:Option<usize>)->Result<i32>;
    fn task_manager(&mut self,s:&mut State)->Result<usize>;
    fn safety_task(&mut self,s:&mut State,manager:usize,value:Option<usize>)->Result<i32>;
    fn maximum_speed(&mut self,s:&mut State)->Result<Option<f32>>;
}
fn tag(s:&mut State,node:usize,name:&[u8],h:&mut impl Host)->Result<bool>{
    Ok(names_equal(&h.name(s,node)?.context("Flee attribute has no name")?,name))
}
pub fn initialize(s:&mut State,node:Option<usize>,h:&mut impl Host)->Result<bool>{
    let Some(node)=node else{return Ok(false)};
    if tag(s,node,b"TraversalConfig",h)?{
        let value=h.value(s,node)?;let index=h.traversal(s,value)?;s.word_48=index as u32;return Ok(index!=-1)
    }
    if tag(s,node,b"MaxDangerousPoints",h)?{
        if let Some(v)=h.value(s,node)?{s.capacity_5c=h.integer(s,v)? as u32;}return Ok(true)
    }
    if tag(s,node,b"MaxDangerousEntities",h)?{
        if let Some(v)=h.value(s,node)?{s.capacity_6c=h.integer(s,v)? as u32;return Ok(true)}
        //1bf018 falls through to the following name tests on a null value.
    }
    if tag(s,node,b"Speed",h)?{
        if let Some(v)=h.value(s,node)?{
            let scale=h.unit_scale(s)?.unwrap_or(1.);
            let speed=(h.number(s,v)? as f32)*scale;
            //1bfb70: ordered <=0 declines, unordered proceeds to the query.
            if !(speed<=0.){
                s.speed_8c=match h.maximum_speed(s)?{Some(limit) if speed>limit=>limit,_=>speed};
            }
        }return Ok(true)
    }
    if tag(s,node,b"CheckSafetyTask",h)?{
        let value=h.value(s,node)?;let manager=h.task_manager(s)?;
        let index=h.safety_task(s,manager,value)?;s.index_94=index;return Ok(index>=0)
    }
    if tag(s,node,b"StopWhenSafe",h)?{
        let Some(v)=h.value(s,node)? else{return Ok(true)};
        //1bf108 retains one raw pointer across every comparison. Do not reuse
        //the pathfinder boolean parser, which re-queries the node's value.
        for (word,value) in [(b"yes".as_slice(),1),(b"true",1),(b"1",1),(b"no",0),(b"false",0),(b"0",0)]{
            if names_equal(&h.text(s,v)?,word){s.flags_90_91[1]=value;return Ok(true)}
        }return Ok(false)
    }
    if tag(s,node,b"FindSafePlaceInterval",h)?{
        if let Some(v)=h.value(s,node)?{
            let value=h.number(s,v)? as f32;
            //190d38(1) is the verified zero-returning helper: one setter pass.
            if value>0.{s.scalars_74_78_7c_80_84[4]=value;}
        }return Ok(true)
    }
    if h.name(s,node)?.is_none(){return Ok(false)}
    let name=h.name(s,node)?.context("Flee attribute name disappeared before its prefix read")?;
    Ok(name.first()==Some(&b'_'))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Seed;
    impl super::super::ai_flee_agent::Host for Seed{
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut State)->Result<usize>{anyhow::bail!("Unexpected constructor entity query")}
        fn release_reservation(&mut self,_:&mut State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected reservation")}
        fn detach_service(&mut self,_:&mut State,_:&mut super::super::ai_graph_link_passes::Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected service detach")}
    }
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>,names:u32,value_reads:usize,value:Option<Vec<u8>>}
    impl Fixture<'_>{
        fn flag(&self,n:&str)->bool{self.c[n].as_bool().unwrap_or(true)}
        fn bits(&self,n:&str,d:u32)->u32{self.c[n].as_i64().map(|n|n as u32).or_else(||self.c[n].as_u64().map(|n|n as u32)).unwrap_or(d)}
        fn current(&self)->Option<Vec<u8>>{self.value.as_ref().map(|v|v[..v.iter().position(|&b|b==0).unwrap_or(v.len())].to_vec())}
        fn string(&self)->Option<String>{self.current().map(|v|String::from_utf8(v).unwrap())}
    }
    impl Host for Fixture<'_>{
        fn name(&mut self,_:&mut State,_:usize)->Result<Option<Vec<u8>>>{self.names+=1;Ok(Some(self.c["name"].as_str().unwrap_or("Unknown").as_bytes().to_vec()))}
        fn value(&mut self,_:&mut State,_:usize)->Result<Option<usize>>{
            let v=if let Some(values)=self.c["values"].as_array(){&values[self.value_reads.min(values.len()-1)]}else{&self.c["value"]};
            self.value_reads+=1;self.value=v.as_str().map(|v|v.as_bytes().to_vec());
            self.trace.push(json!(["value",self.string()]));Ok(self.value.as_ref().map(|_|1))
        }
        fn text(&mut self,_:&mut State,v:usize)->Result<Vec<u8>>{anyhow::ensure!(v==1,"Unknown raw string");self.current().context("Raw string absent")}
        fn integer(&mut self,_:&mut State,_:usize)->Result<i32>{self.trace.push(json!(["integer",self.string()]));Ok(self.bits("integer",0) as i32)}
        fn number(&mut self,_:&mut State,_:usize)->Result<f64>{self.trace.push(json!(["number",self.string()]));Ok(f64::from_bits(self.c["number"].as_u64().unwrap_or(2f64.to_bits())))}
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>>{
            let bits=self.flag("world").then(||self.bits("scale",2f32.to_bits()));self.trace.push(json!(["scale",bits]));
            if self.c["mutation"]=="scale_value"{self.value=Some(b"2".to_vec());}Ok(bits.map(f32::from_bits))
        }
        fn traversal(&mut self,s:&mut State,_:Option<usize>)->Result<i32>{self.trace.push(json!(["traversal",s.references_34_38_3c_40_44[1].unwrap_or(0),self.string()]));Ok(self.bits("index",5) as i32)}
        fn task_manager(&mut self,_:&mut State)->Result<usize>{
            self.trace.push(json!(["task_manager"]));if self.c["mutation"]=="task_value"{self.value=Some(b"replacement".to_vec());}Ok(0x75000100)
        }
        fn safety_task(&mut self,_:&mut State,m:usize,_:Option<usize>)->Result<i32>{self.trace.push(json!(["task",m,self.string()]));Ok(self.bits("index",5) as i32)}
        fn maximum_speed(&mut self,_:&mut State)->Result<Option<f32>>{self.trace.push(json!(["maximum_speed",0x74000100u32]));Ok(self.flag("speed_record").then(||f32::from_bits(self.bits("limit",3f32.to_bits()))))}
    }
    #[test]
    fn flee_attributes_match_original_missing_values_raw_boolean_speed_and_callbacks()->Result<()>{
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_flee_attributes.json"))?;
        let mut failures=Vec::new();
        for case in f["cases"].as_array().context("Flee attribute cases absent")?{
            let c=&case["input"];let mut s=State::new(Some(0x74000000),&mut Seed)?;
            s.references_34_38_3c_40_44[1]=Some(0x75000000);s.word_48=17;s.capacity_5c=3;s.capacity_6c=4;s.speed_8c=9.;s.scalars_74_78_7c_80_84[4]=15.;s.index_94=19;s.flags_90_91[1]=7;
            let mut h=Fixture{c,trace:vec![],names:0,value_reads:0,value:None};
            let result=initialize(&mut s,h.flag("node").then_some(1),&mut h)?;
            let actual=json!({"result":result,"trace":h.trace,"name_reads":h.names,"words":[s.word_48,s.capacity_5c,s.capacity_6c,s.speed_8c.to_bits(),s.scalars_74_78_7c_80_84[4].to_bits(),s.index_94 as u32],"flag":s.flags_90_91[1]});
            for key in actual.as_object().unwrap().keys(){if actual[key]!=case[key]{failures.push(format!("{key}: {c}: {} != {}",actual[key],case[key]));}}
        }
        anyhow::ensure!(failures.is_empty(),"{} failing comparisons:\n{}",failures.len(),failures.join("\n"));Ok(())
    }
}
