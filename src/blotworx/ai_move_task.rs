//! Concrete native move task REL19814c..1986bc. Navigation and animation
//! services are mandatory boundaries, not guessed local steering fallbacks.
use anyhow::Result;
pub trait Host {
    fn subscribe(&mut self, owner:usize, kind:u16)->Result<()>;
    fn agent(&mut self, owner:usize)->Result<usize>;
    fn speed(&mut self, agent:usize, speed:f32)->Result<()>;
    fn reset(&mut self, agent:usize)->Result<()>;
    fn resolve(&mut self, reference:usize)->Result<Option<usize>>;
    fn target_agent(&mut self, entity:Option<usize>)->Result<Option<usize>>;
    fn attach(&mut self, agent:usize, target:usize)->Result<()>;
    fn has_animation(&mut self, owner:usize)->Result<bool>;
    fn quick_turns(&mut self, owner:usize, value:f32)->Result<()>;
    fn position(&mut self, entity:usize)->Result<[f32;4]>;
    fn target_reference(&mut self, agent:usize, reference:usize, radius:f32)->Result<()>;
    fn project(&mut self, owner:usize, point:[f32;4])->Result<[f32;4]>;
    fn target_point(&mut self, agent:usize, point:[f32;4], radius:f32)->Result<()>;
    fn adjust(&mut self, agent:usize, point:[f32;4], radius:f32)->Result<Option<[f32;4]>>;
    fn retarget(&mut self, agent:usize, point:[f32;4])->Result<()>;
    fn detach(&mut self, agent:usize, target:usize)->Result<()>;
    fn stop(&mut self, agent:usize)->Result<()>;
}
#[derive(Debug,serde::Serialize)]
pub struct Move {
    pub reference:Option<usize>, pub point:[f32;4], pub radius:f32, pub speed:f32,
    pub complete:bool, pub submitted:bool, pub changed:bool,
    pub attached:Option<usize>, pub timeout:f32, pub kind:u32,
}
impl Move {
    pub fn new(reference:Option<usize>, point:[f32;4],radius:f32,speed:f32,timeout:f32)->Self {
        Self {reference,point,radius,speed,timeout,complete:false,submitted:false,changed:false,attached:None,kind:0}
    }
    pub fn activate(&mut self,owner:usize,host:&mut impl Host)->Result<()> {
        host.subscribe(owner,0x6f)?;host.subscribe(owner,0x70)?;
        let agent=host.agent(owner)?;
        host.speed(agent,self.speed)?;host.reset(agent)?;
        self.submitted=false;
        let target=match self.reference {Some(reference)=>host.resolve(reference)?,None=>None};
        self.attached=host.target_agent(target)?;
        if let Some(target)=self.attached {
            let agent=host.agent(owner)?;host.attach(agent,target)?;
        }
        if host.has_animation(owner)? {
            // Native ble tests the GT bit: unordered speed takes the zero branch.
            host.quick_turns(owner,if self.speed>1. {1.} else {0.})?;
        }
        Ok(())
    }
    pub fn receive(&mut self,kind:u16) {
        match kind {0x6f=>{self.complete=true;self.kind=2;},0x70=>{self.complete=true;self.kind=1;},_=>{}}
    }
    pub fn retarget(&mut self,point:[f32;4]) {self.point=point;self.changed=true;}
    pub fn update(&mut self,owner:usize,delta:f32,host:&mut impl Host)->Result<u32> {
        if let Some(reference)=self.reference {
            if host.resolve(reference)?.is_none() {
                self.submitted=true;self.complete=true;self.kind=2;return Ok(1);
            }
        }
        if !self.submitted {
            let agent=host.agent(owner)?;
            let target=match self.reference {Some(reference)=>host.resolve(reference)?,None=>None};
            let target_agent=if target.is_some() {host.target_agent(target)?} else {None};
            if target_agent.is_some() {
                host.target_reference(agent,self.reference.unwrap(),self.radius)?;
            } else {
                if let Some(target)=target {self.point=host.position(target)?;}
                else if self.reference.is_some() {return Ok(1);}
                self.point=host.project(owner,self.point)?;
                host.target_point(agent,self.point,self.radius)?;
            }
            self.submitted=true;
        } else if self.changed {
            let agent=host.agent(owner)?;
            self.point=host.project(owner,self.point)?;
            if let Some(point)=host.adjust(agent,self.point,self.radius)? {
                self.point=point;host.retarget(agent,self.point)?;
            } else {self.kind=2;self.complete=true;}
        }
        self.changed=false;
        if self.timeout>0. {
            self.timeout-=delta;
            if self.timeout<0. {self.kind=2;self.complete=true;}
        }
        Ok(u32::from(self.complete))
    }
    pub fn stop(&mut self,owner:usize,host:&mut impl Host)->Result<()> {
        let agent=host.agent(owner)?;
        if let Some(target)=self.attached {
            let current=host.agent(owner)?;host.detach(current,target)?;self.attached=None;
        }
        host.stop(agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,resolves:usize}
    impl Fixture<'_> {
        fn flag(&self,key:&str,default:bool)->bool {self.case[key].as_bool().unwrap_or(default)}
        fn point()->[f32;4] {[2.,3.,4.,-0.]}
        fn projected()->[f32;4] {[2.,5.,4.,-0.]}
        fn adjusted()->[f32;4] {[6.,7.,8.,0.]}
    }
    impl Host for Fixture<'_> {
        fn subscribe(&mut self,owner:usize,kind:u16)->Result<()> {assert_eq!(owner,7);self.trace.push(json!(["subscribe",kind]));Ok(())}
        fn agent(&mut self,owner:usize)->Result<usize> {assert_eq!(owner,7);self.trace.push(json!(["agent"]));Ok(100)}
        fn speed(&mut self,agent:usize,speed:f32)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["speed",speed.to_bits()]));Ok(())}
        fn reset(&mut self,agent:usize)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["reset"]));Ok(())}
        fn resolve(&mut self,reference:usize)->Result<Option<usize>> {assert_eq!(reference,42);self.trace.push(json!(["resolve",42]));let found=self.flag("found",true)&&!(self.flag("second_missing",false)&&self.resolves>0);self.resolves+=1;Ok(found.then_some(43))}
        fn target_agent(&mut self,entity:Option<usize>)->Result<Option<usize>> {self.trace.push(json!(["target_agent",entity]));Ok((entity.is_some()&&self.flag("target_agent",false)).then_some(200))}
        fn attach(&mut self,agent:usize,target:usize)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["attach",target]));Ok(())}
        fn has_animation(&mut self,owner:usize)->Result<bool> {assert_eq!(owner,7);self.trace.push(json!(["animation"]));Ok(self.flag("animation",true))}
        fn quick_turns(&mut self,owner:usize,value:f32)->Result<()> {assert_eq!(owner,7);self.trace.push(json!(["quick_turns",value.to_bits()]));Ok(())}
        fn position(&mut self,entity:usize)->Result<[f32;4]> {self.trace.push(json!(["position",entity]));Ok(Self::point())}
        fn target_reference(&mut self,agent:usize,reference:usize,radius:f32)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["target_reference",reference,radius.to_bits()]));Ok(())}
        fn project(&mut self,owner:usize,point:[f32;4])->Result<[f32;4]> {assert_eq!(owner,7);self.trace.push(json!(["project",point.map(f32::to_bits)]));Ok(Self::projected())}
        fn target_point(&mut self,agent:usize,point:[f32;4],radius:f32)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["target_point",point.map(f32::to_bits),radius.to_bits()]));Ok(())}
        fn adjust(&mut self,agent:usize,point:[f32;4],radius:f32)->Result<Option<[f32;4]>> {assert_eq!(agent,100);self.trace.push(json!(["adjust",point.map(f32::to_bits),radius.to_bits()]));Ok(self.flag("adjust",true).then_some(Self::adjusted()))}
        fn retarget(&mut self,agent:usize,point:[f32;4])->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["retarget",point.map(f32::to_bits)]));Ok(())}
        fn detach(&mut self,agent:usize,target:usize)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["detach",target]));Ok(())}
        fn stop(&mut self,agent:usize)->Result<()> {assert_eq!(agent,100);self.trace.push(json!(["stop"]));Ok(())}
    }
    #[test]
    fn original_move_instructions_match_task_state_and_service_order()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_move_task.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let case=&expected["input"];
            let bits=|name:&str,default:f32|f32::from_bits(case[name].as_u64().map_or(default.to_bits(),|v|v as u32));
            let flag=|name:&str|case[name].as_bool().unwrap_or(false);
            let mut task=Move::new(flag("reference").then_some(42),Fixture::point(),0.5,bits("speed",-1.),bits("timeout",0.));
            task.complete=flag("complete");task.submitted=flag("submitted");task.changed=flag("changed");task.attached=flag("attached").then_some(200);task.kind=7;
            let mut host=Fixture{case,trace:vec![],resolves:0};
            let result=match case["op"].as_str().unwrap() {
                "activate"=>{task.activate(7,&mut host)?;None},
                "update"=>Some(task.update(7,0.1,&mut host)?),
                "stop"=>{task.stop(7,&mut host)?;None},
                "receive"=>{task.receive(case["event"].as_u64().unwrap() as u16);None},
                "retarget"=>{task.retarget(Fixture::adjusted());None},_=>unreachable!(),
            };
            let actual=json!({"trace":host.trace,"kind":task.kind,"point":task.point.map(f32::to_bits),"complete":task.complete,"submitted":task.submitted,"changed":task.changed,"attached":task.attached,"timeout":task.timeout.to_bits(),"result":result});
            for key in ["trace","kind","point","complete","submitted","changed","attached","timeout","result"] {assert_eq!(actual[key],expected[key],"{key}: {case}");}
        }
        Ok(())
    }
}
