//! Goto construction1c04f4, reset1c0a8c and path-request1c08d0.
//! Base path request, pathfinder and World registration remain required.
use anyhow::{Context as _,Result};
#[derive(Debug,serde::Serialize)]
pub struct Agent {
    pub path:super::ai_path_agent::Agent,
    pub vector_90:[f32;3], //90..98
    pub field_9c:i32, //9c
    pub reference:Option<usize>, //a0
    pub value_a4:f32, //a4
    /// Constructor does not initialize a8; parser/reset supply the value.
    pub flag_a8:Option<bool>,
}
impl super::ai_agent_base::Owner for Agent {
    fn base(&self)->&super::ai_agent_base::Agent {&self.path.base}
    fn base_mut(&mut self)->&mut super::ai_agent_base::Agent {&mut self.path.base}
}
impl super::ai_path_agent::Owner for Agent {
    fn path(&self)->&super::ai_path_agent::Agent{&self.path}
    fn path_mut(&mut self)->&mut super::ai_path_agent::Agent{&mut self.path}
}
pub trait Host:super::ai_path_agent::Host {
    fn global_vector_564d0(&mut self)->Result<[f32;3]>;
    fn service_speed(&mut self,service:usize)->Result<f32>;
    fn path_value_8a8(&mut self,path:usize)->Result<f32>;
    fn request_base_path(&mut self,agent:&mut Agent)->Result<u32>;
    fn set_service_speed(&mut self,service:usize,speed:f32)->Result<()>;
}
fn separation_squared(point:[f32;3],vector_90:[f32;3])->f32 {
    let dx=point[0]-vector_90[0];let dz=point[2]-vector_90[2];let dy=point[1]-vector_90[1];
    dy.mul_add(dy,dz.mul_add(dz,dx*dx))
}
impl Agent {
    pub fn new<H:super::ai_path_agent::Host+super::ai_goto_configuration::Host>(
        owner:Option<usize>,shared:&mut super::ai_goto_configuration::Shared,host:&mut H,
    )->Result<Self> {
        let path=super::ai_path_agent::Agent::new(owner,host)?;
        let result=Self {path,vector_90:[0.;3],field_9c:-1,reference:None,value_a4:1.,flag_a8:None};
        shared.construct(host)?;
        Ok(result)
    }
    pub fn bind(&mut self,attributes:usize,host:&mut impl super::ai_path_agent::BindingHost<Self>)->Result<bool> {
        super::ai_path_agent::bind_owned(self,attributes,host)
    }
    pub fn initialize(&mut self,host:&mut impl Host)->Result<()> {
        self.vector_90=host.global_vector_564d0()?;
        self.field_9c=-1;self.reference=None;self.flag_a8=Some(false);
        self.path.initialize(host)
    }
    pub fn request_path(&mut self,host:&mut impl Host)->Result<u32> {
        let point=self.path.point.position;
        let squared=separation_squared(point,self.vector_90);
        let service=self.path.base.owner_service_6af6c.context("Goto request has no owner service")?;
        let path=self.path.pathfinder.context("Goto request has no pathfinder")?;
        let speed=host.service_speed(service)?;
        let limit=host.path_value_8a8(path)?;
        // Native ble tests GT clear, preserving the projected point on equality
        // and unordered comparisons. No epsilon or nonnegative clamp is added.
        if squared>limit {self.path.point.position=self.vector_90;}
        let result=host.request_base_path(self)?;
        let service=self.path.base.owner_service_6af6c.context("Goto base request removed its owner service")?;
        host.set_service_speed(service,speed)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::collections::BTreeMap;
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,speeds:BTreeMap<usize,f32>}
    impl Fixture<'_>{fn flag(&self,key:&str)->bool{self.case[key].as_bool().unwrap_or(false)}}
    impl super::super::ai_path_agent::Host for Fixture<'_> {
        fn point_observer(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<(usize,usize)>{unreachable!()}
        fn bind_base(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<bool>{unreachable!()}
        fn create_pathfinder_54(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<Option<usize>>{unreachable!()}
        fn initialize_14(&mut self,_:&mut super::super::ai_path_agent::Agent)->Result<()>{unreachable!()}
        fn reset_path_4c(&mut self,a:&mut super::super::ai_path_agent::Agent)->Result<()>{self.trace.push(json!(["reset",a.request_state as u32]));a.request_state=10;Ok(())}
        fn motion(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize,_:[f32;3],_:f32)->Result<()>{unreachable!()}
        fn ready(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<bool>{unreachable!()}
        fn reconcile(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<u32>{unreachable!()}
        fn status(&mut self,_:&mut super::super::ai_path_agent::Agent,_:usize)->Result<u32>{unreachable!()}
    }
    impl Host for Fixture<'_> {
        fn global_vector_564d0(&mut self)->Result<[f32;3]>{self.trace.push(json!(["global_vector"]));Ok(vector(self.case,"global_vector",[f32::MAX;3]))}
        fn service_speed(&mut self,service:usize)->Result<f32>{self.trace.push(json!(["speed_read",service]));Ok(self.speeds[&service])}
        fn path_value_8a8(&mut self,path:usize)->Result<f32>{self.trace.push(json!(["limit_read",path]));Ok(f32::from_bits(self.case["limit"].as_u64().unwrap_or(14f32.to_bits() as u64) as u32))}
        fn request_base_path(&mut self,a:&mut Agent)->Result<u32>{self.trace.push(json!(["request",a.path.point.position.map(f32::to_bits)]));self.speeds.insert(100,99.);if self.flag("switch_service"){a.path.base.owner_service_6af6c=Some(101);}Ok(self.case["result"].as_u64().unwrap_or(7) as u32)}
        fn set_service_speed(&mut self,service:usize,speed:f32)->Result<()>{self.trace.push(json!(["speed_write",service,speed.to_bits()]));self.speeds.insert(service,speed);Ok(())}
    }
    fn vector(case:&Value,name:&str,default:[f32;3])->[f32;3]{case[name].as_array().map_or(default,|v|std::array::from_fn(|i|f32::from_bits(v[i].as_u64().unwrap() as u32)))}
    #[test]
    fn original_goto_reset_request_fma_and_speed_restore_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goto_agent.json"))?;
        for e in fixture["cases"].as_array().unwrap() {
            let c=&e["input"];let point=vector(c,"point",[1.,2.,3.]);let vector_90=vector(c,"vector_90",[0.;3]);
            let mut base=super::super::ai_agent_base::Agent::new(Some(7));base.owner_service_6af6c=Some(100);base.action_vector=[1.,-2.,3.];base.action_scalar=0.75;
            let path=super::super::ai_path_agent::Agent{base,point:super::super::ai_path_agent::Point{position:point,attributes:[None;16]},request_state:9,pathfinder:Some(200)};
            let mut a=Agent{path,vector_90,field_9c:6,reference:Some(88),value_a4:2.5,flag_a8:Some(true)};
            let mut host=Fixture{case:c,trace:vec![],speeds:BTreeMap::from([(100,f32::from_bits(c["speed"].as_u64().unwrap_or(1.25f32.to_bits() as u64) as u32)),(101,22.)])};
            let result=if c["op"]=="reset" {a.initialize(&mut host)?;None} else {
                assert_eq!(Some(separation_squared(point,vector_90).to_bits()),e["comparison"].as_u64().map(|v|v as u32),"FMA words: {c}");
                Some(a.request_path(&mut host)?)
            };
            let actual=json!({"trace":host.trace,"point":a.path.point.position.map(f32::to_bits),"vector_90":a.vector_90.map(f32::to_bits),
                "request_state":a.path.request_state as u32,"field_9c":a.field_9c as u32,"reference":a.reference,
                "flag_a8":a.flag_a8,"service":a.path.base.owner_service_6af6c,
                "speeds":[host.speeds[&100].to_bits(),host.speeds[&101].to_bits()],"result":result});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],e[key],"{key}: {c}");}
        }
        Ok(())
    }
}
