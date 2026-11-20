//! Path-agent protocol4eb9ac:194c98/194e14/194e88/194ed0.
//! Derived virtual methods and pathfinder services remain mandatory.
use anyhow::{Context as _,Result};
#[derive(Debug,serde::Serialize)]
pub struct Point {
    pub position:[f32;3],
    /// Native point+10..4c: sixteen reflected attribute pointers. These are
    /// portable handles, never Wii or host addresses stored in asset bytes.
    pub attributes:[Option<usize>;16],
}
#[derive(Debug,serde::Serialize)]
pub struct Agent {
    pub base:super::ai_agent_base::Agent,
    pub point:Point,
    pub request_state:i32,
    pub pathfinder:Option<usize>,
}
impl super::ai_agent_base::Owner for Agent {
    fn base(&self)->&super::ai_agent_base::Agent {&self.base}
    fn base_mut(&mut self)->&mut super::ai_agent_base::Agent {&mut self.base}
}
///194e14 invokes derived virtuals on the complete object, including Goto.
pub trait Owner:super::ai_agent_base::Owner {
    fn path(&self)->&Agent;
    fn path_mut(&mut self)->&mut Agent;
}
impl Owner for Agent {
    fn path(&self)->&Agent{self}
    fn path_mut(&mut self)->&mut Agent{self}
}
pub trait BindingHost<S:Owner> {
    fn bind_base_owned(&mut self,agent:&mut S,attributes:usize)->Result<bool>;
    fn create_pathfinder_owned(&mut self,agent:&mut S)->Result<Option<usize>>;
    fn initialize_owned(&mut self,agent:&mut S)->Result<()>;
}
impl<H:Host> BindingHost<Agent> for H {
    fn bind_base_owned(&mut self,a:&mut Agent,n:usize)->Result<bool>{self.bind_base(a,n)}
    fn create_pathfinder_owned(&mut self,a:&mut Agent)->Result<Option<usize>>{self.create_pathfinder_54(a)}
    fn initialize_owned(&mut self,a:&mut Agent)->Result<()>{self.initialize_14(a)}
}
pub fn bind_owned<S:Owner,H:BindingHost<S>>(agent:&mut S,attributes:usize,host:&mut H)->Result<bool> {
    if !host.bind_base_owned(agent,attributes)?{return Ok(false)}
    // Store even a null factory result before invoking derived virtual14.
    // Its required dereferences must diagnose failure, not fabricate readiness.
    agent.path_mut().pathfinder=host.create_pathfinder_owned(agent)?;
    host.initialize_owned(agent)?;
    Ok(true)
}
pub trait Host {
    fn point_observer(&mut self,agent:&mut Agent)->Result<(usize,usize)>;
    /// Delegate to the recovered base bind with complete derived attribute
    /// effects. This explicit boundary permits callbacks to modify this owner.
    fn bind_base(&mut self,agent:&mut Agent,attributes:usize)->Result<bool>;
    fn create_pathfinder_54(&mut self,agent:&mut Agent)->Result<Option<usize>>;
    fn initialize_14(&mut self,agent:&mut Agent)->Result<()>;
    fn reset_path_4c(&mut self,agent:&mut Agent)->Result<()>;
    fn motion(&mut self,agent:&mut Agent,path:usize,vector:[f32;3],scalar:f32)->Result<()>;
    fn ready(&mut self,agent:&mut Agent,path:usize)->Result<bool>;
    fn reconcile(&mut self,agent:&mut Agent,path:usize)->Result<u32>;
    fn status(&mut self,agent:&mut Agent,path:usize)->Result<u32>;
}
impl Agent {
    pub fn new(owner:Option<usize>,host:&mut impl Host)->Result<Self> {
        let mut result=Self {base:super::ai_agent_base::Agent::new(owner),
            point:Point{position:[0.;3],attributes:[None;16]},request_state:-1,pathfinder:None};
        let (slot,observer)=host.point_observer(&mut result)?;
        *result.point.attributes.get_mut(slot).context("Point observer class slot exceeds the native16-slot point layout")?=Some(observer);
        Ok(result)
    }
    pub fn bind(&mut self,attributes:usize,host:&mut impl Host)->Result<bool> {
        bind_owned(self,attributes,host)
    }
    pub fn initialize(&mut self,host:&mut impl Host)->Result<()> {
        host.reset_path_4c(self)?;
        self.request_state=-1;
        //196808 is the original empty base virtual14, not action-cache19680c.
        Ok(())
    }
    pub fn update_path(&mut self,host:&mut impl Host)->Result<u32> {
        let path=self.pathfinder.context("Path-agent motion has no pathfinder")?;
        let vector=self.base.action_vector;let scalar=self.base.action_scalar;
        host.motion(self,path,vector,scalar)?;
        let path=self.pathfinder.context("Path-agent motion callback removed its pathfinder")?;
        if host.ready(self,path)? {
            let path=self.pathfinder.context("Path-agent ready callback removed its pathfinder")?;
            host.reconcile(self,path)
        } else {
            let path=self.pathfinder.context("Path-agent ready callback removed its pathfinder")?;
            Ok(u32::from(host.status(self,path)?==0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Fixture<'_>{fn flag(&self,key:&str,default:bool)->bool{self.case[key].as_bool().unwrap_or(default)}}
    impl Host for Fixture<'_> {
        fn point_observer(&mut self,a:&mut Agent)->Result<(usize,usize)>{
            self.trace.push(json!(["observer",a.base.speed.to_bits(),a.request_state as u32,a.pathfinder.unwrap_or(0)]));
            if self.flag("observer_mutation",false){a.request_state=5;}
            Ok((self.case["slot"].as_u64().unwrap_or(4) as usize,900))
        }
        fn bind_base(&mut self,a:&mut Agent,attributes:usize)->Result<bool>{self.trace.push(json!(["base",attributes]));a.base.speed=11.;a.base.action_04=Some(89);Ok(self.flag("base",true))}
        fn create_pathfinder_54(&mut self,a:&mut Agent)->Result<Option<usize>>{self.trace.push(json!(["create"]));if self.flag("factory_mutation",false){a.pathfinder=Some(104);}Ok(self.flag("created",true).then_some(101))}
        fn initialize_14(&mut self,a:&mut Agent)->Result<()>{self.trace.push(json!(["initialize",a.pathfinder]));if self.flag("initialize_mutation",false){a.pathfinder=Some(105);}Ok(())}
        fn reset_path_4c(&mut self,a:&mut Agent)->Result<()>{self.trace.push(json!(["reset",a.request_state as u32]));a.request_state=10;a.pathfinder=self.flag("reset_path",true).then_some(106);Ok(())}
        fn motion(&mut self,a:&mut Agent,path:usize,vector:[f32;3],scalar:f32)->Result<()>{self.trace.push(json!(["motion",path,vector.map(f32::to_bits),scalar.to_bits()]));if self.flag("motion_mutation",false){a.pathfinder=Some(102);}Ok(())}
        fn ready(&mut self,a:&mut Agent,path:usize)->Result<bool>{self.trace.push(json!(["ready",path]));if self.flag("ready_mutation",false){a.pathfinder=Some(103);}Ok(self.flag("ready",false))}
        fn reconcile(&mut self,_:&mut Agent,path:usize)->Result<u32>{self.trace.push(json!(["reconcile",path]));Ok(self.case["reconcile"].as_u64().unwrap_or(7) as u32)}
        fn status(&mut self,_:&mut Agent,path:usize)->Result<u32>{self.trace.push(json!(["status",path]));Ok(self.case["status"].as_u64().unwrap_or(0) as u32)}
    }
    #[test]
    fn original_path_agent_constructor_bind_reset_and_motion_order_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_path_agent.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let mut host=Fixture{case:c,trace:vec![]};
            let op=c["op"].as_str().unwrap();
            let mut a=if op=="new" {Agent::new(Some(7),&mut host)?} else {
                let mut base=super::super::ai_agent_base::Agent::new(Some(7));base.speed=7.;base.action_vector=[1.,-2.,3.];base.action_scalar=0.75;
                let mut point=Point{position:[4.,5.,6.],attributes:[None;16]};point.attributes[2]=Some(77);
                Agent{base,point,request_state:9,pathfinder:Some(100)}
            };
            let result=match op {"new"=>None,"bind"=>Some(u32::from(a.bind(88,&mut host)?)),
                "initialize"=>{a.initialize(&mut host)?;None},"update"=>Some(a.update_path(&mut host)?),_=>unreachable!()};
            let actual=json!({"trace":host.trace,"speed":a.base.speed.to_bits(),"configuration":a.base.action_04,
                "vector":a.base.action_vector.map(f32::to_bits),"scalar":a.base.action_scalar.to_bits(),
                "position":a.point.position.map(f32::to_bits),"attributes":a.point.attributes,
                "request_state":a.request_state as u32,"pathfinder":a.pathfinder,"result":result});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],expected[key],"{key}: {c}");}
        }
        Ok(())
    }
}
