//! Navigation agent base4eba08: constructor1965b4, bind196690,
//! action cache19680c and speed setter1968bc. Services are required boundaries.
use anyhow::{Context as _,Result};
#[derive(Debug,serde::Serialize)]
pub struct Agent {
    /// Action pointer at +4; may alias the owner brain action at +18.
    pub action_04:Option<usize>, pub owner:Option<usize>,
    pub world_service_61d14:Option<usize>, pub owner_service_6af6c:Option<usize>,
    pub speed:f32, pub action_vector:[f32;3], pub action_scalar:f32,
    pub bound_owner:Option<usize>, pub blackboard_head:usize,
    pub world_service_45dcc:Option<usize>,
}
pub trait Host {
    fn owner_is_112a0(&mut self,owner:usize)->Result<bool>;
    /// Historical name:196724 reads owner entity+20 user_data[0], the gameplay
    /// actor handle. None means absent entity; Some(0) is a readable zero handle.
    fn blackboard_head(&mut self,owner:usize)->Result<Option<usize>>;
    fn owner_service_6af6c(&mut self,owner:usize)->Result<Option<usize>>;
    fn world_service_61d14(&mut self)->Result<Option<usize>>;
    fn world_service_45dcc(&mut self)->Result<Option<usize>>;
    fn initialize_attributes(&mut self,agent:&mut Agent,attributes:usize)->Result<bool>;
    fn speed_value_360e4(&mut self,owner:usize)->Result<Option<f32>>;
    ///18df00 leaves the caller's zero vector untouched when action+30 is null.
    fn action_vector(&mut self,owner:usize)->Result<Option<[f32;3]>>;
    ///18de98 supplies zero for a suppressed action, and leaves zero on absence.
    fn action_scalar(&mut self,owner:usize)->Result<Option<f32>>;
}
/// Keeps derived virtual attribute callbacks on the canonical complete agent.
pub trait Owner {
    fn base(&self)->&Agent;
    fn base_mut(&mut self)->&mut Agent;
}
impl Owner for Agent {
    fn base(&self)->&Agent {self}
    fn base_mut(&mut self)->&mut Agent {self}
}
pub trait BindingHost<S:Owner>:Host {
    fn initialize_owned_attributes(&mut self,agent:&mut S,attributes:usize)->Result<bool>;
}
impl<H:Host> BindingHost<Agent> for H {
    fn initialize_owned_attributes(&mut self,agent:&mut Agent,attributes:usize)->Result<bool> {
        self.initialize_attributes(agent,attributes)
    }
}
/// Native196690 with derived virtual10, preserving callback ordering and fresh
/// owner reads. No parent copy, temporary replacement, or split initialization.
pub fn bind_owned<S:Owner,H:BindingHost<S>>(agent:&mut S,attributes:usize,host:&mut H)->Result<bool> {
        let Some(owner)=agent.base_mut().owner else {return Ok(false)};
        if !host.owner_is_112a0(owner)? {return Ok(false)}
        agent.base_mut().bound_owner=Some(owner);
        let Some(head)=host.blackboard_head(owner)? else {return Ok(false)};
        agent.base_mut().blackboard_head=head;
        agent.base_mut().owner_service_6af6c=host.owner_service_6af6c(owner)?;
        if agent.base_mut().owner_service_6af6c.is_none() {return Ok(false)}
        agent.base_mut().world_service_61d14=host.world_service_61d14()?;
        if agent.base_mut().world_service_61d14.is_none() {return Ok(false)}
        agent.base_mut().world_service_45dcc=host.world_service_45dcc()?;
        if !host.initialize_owned_attributes(agent,attributes)? {return Ok(false)}
        let owner=agent.base_mut().owner.context("Agent attribute callback removed its owner")?;
        let Some(speed)=host.speed_value_360e4(owner)? else {return Ok(false)};
        agent.base_mut().speed=speed;
        Ok(true)
}
impl Agent {
    pub fn new(owner:Option<usize>)->Self {
        Self {action_04:None,owner,world_service_61d14:None,owner_service_6af6c:None,
            speed:-1.,action_vector:[0.;3],action_scalar:0.,bound_owner:None,
            blackboard_head:0,world_service_45dcc:None}
    }
    /// Preserve partial writes on each failed stage. Do not collapse services
    /// into a single preflight or claim a successful agent after profile lookup.
    pub fn bind(&mut self,attributes:usize,host:&mut impl Host)->Result<bool> {
        bind_owned(self,attributes,host)
    }
    pub fn cache_action(&mut self,host:&mut impl Host)->Result<bool> {
        let Some(owner)=self.bound_owner else {return Ok(false)};
        if self.owner_service_6af6c.is_none() {return Ok(false)}
        self.action_vector=host.action_vector(owner)?.unwrap_or([0.;3]);
        self.action_scalar=host.action_scalar(owner)?.unwrap_or(0.);
        Ok(true)
    }
    pub fn set_speed(&mut self,speed:f32,host:&mut impl Host)->Result<()> {
        // cror EQ,LT,EQ followed by beq skips ordered nonpositive values.
        // An unordered request reaches the service and is stored as in native.
        if speed<=0. {return Ok(())}
        let owner=self.owner.context("Agent speed setter has no owner")?;
        self.speed=match host.speed_value_360e4(owner)? {
            Some(limit) if speed>limit=>limit,_=>speed,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Fixture<'_> {
        fn flag(&self,key:&str)->bool{self.case[key].as_bool().unwrap_or(true)}
    }
    impl Host for Fixture<'_> {
        fn owner_is_112a0(&mut self,owner:usize)->Result<bool>{self.trace.push(json!(["class",owner]));Ok(self.flag("class"))}
        fn blackboard_head(&mut self,owner:usize)->Result<Option<usize>>{assert_eq!(owner,7);Ok(self.flag("blackboard").then(||self.case["head"].as_u64().unwrap_or(82) as usize))}
        fn owner_service_6af6c(&mut self,owner:usize)->Result<Option<usize>>{self.trace.push(json!(["owner_service",owner]));Ok(self.flag("owner_service").then_some(101))}
        fn world_service_61d14(&mut self)->Result<Option<usize>>{self.trace.push(json!(["required_world"]));Ok(self.flag("required_world").then_some(102))}
        fn world_service_45dcc(&mut self)->Result<Option<usize>>{self.trace.push(json!(["optional_world"]));Ok(self.flag("optional_world").then_some(103))}
        fn initialize_attributes(&mut self,agent:&mut Agent,attributes:usize)->Result<bool>{self.trace.push(json!(["attributes",attributes]));agent.action_04=Some(99);Ok(self.flag("attributes"))}
        fn speed_value_360e4(&mut self,owner:usize)->Result<Option<f32>>{self.trace.push(json!(["speed_value",owner]));Ok(self.flag("speed_record").then(||f32::from_bits(self.case["limit"].as_u64().unwrap_or(5f32.to_bits() as u64) as u32)))}
        fn action_vector(&mut self,owner:usize)->Result<Option<[f32;3]>>{self.trace.push(json!(["vector",owner]));Ok(self.flag("vector").then_some([1.25,-2.5,3.75]))}
        fn action_scalar(&mut self,owner:usize)->Result<Option<f32>>{self.trace.push(json!(["scalar",owner]));Ok(self.flag("scalar").then_some(0.75))}
    }
    #[test]
    fn original_agent_base_instructions_match_partial_binding_cache_and_speed()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_base.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let flag=|key:&str|c[key].as_bool().unwrap_or(true);
            let mut agent=Agent::new(flag("owner").then_some(7));
            let op=c["op"].as_str().unwrap();
            if op!="new" {
                agent.action_04=Some(90);agent.world_service_61d14=Some(91);
                agent.owner_service_6af6c=flag("path").then_some(92);agent.speed=7.;
                agent.action_vector=[1.,-2.,3.];agent.action_scalar=4.;
                agent.bound_owner=flag("bound").then_some(8);agent.blackboard_head=93;
                agent.world_service_45dcc=Some(94);
            }
            let mut host=Fixture{case:c,trace:vec![]};
            let result=match op {
                "new"=>None,"bind"=>Some(agent.bind(88,&mut host)?),
                "cache"=>Some(agent.cache_action(&mut host)?),
                "speed"=>{agent.set_speed(f32::from_bits(c["request"].as_u64().unwrap() as u32),&mut host)?;None},_=>unreachable!(),
            };
            let actual=json!({"trace":host.trace,"configuration":agent.action_04,"owner":agent.owner,
                "world_service_61d14":agent.world_service_61d14,"owner_service_6af6c":agent.owner_service_6af6c,
                "speed":agent.speed.to_bits(),"action_vector":agent.action_vector.map(f32::to_bits),
                "action_scalar":agent.action_scalar.to_bits(),"bound_owner":agent.bound_owner,
                "blackboard_head":agent.blackboard_head,"world_service_45dcc":agent.world_service_45dcc,"result":result});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],expected[key],"{key}: {c}");}
        }
        Ok(())
    }
    struct Derived {base:Agent,attribute_writes:usize}
    impl Owner for Derived {
        fn base(&self)->&Agent{&self.base}
        fn base_mut(&mut self)->&mut Agent{&mut self.base}
    }
    impl BindingHost<Derived> for Fixture<'_> {
        fn initialize_owned_attributes(&mut self,a:&mut Derived,attributes:usize)->Result<bool>{
            self.trace.push(json!(["derived_attributes",attributes]));a.attribute_writes+=1;
            a.base.owner=Some(19);a.base.action_04=Some(99);Ok(self.flag("attributes"))
        }
    }
    #[test]
    fn derived_binding_retains_complete_owner_and_fresh_reads_after_attributes()->Result<()> {
        for success in [false,true] {
            let c=json!({"attributes":success});let mut h=Fixture{case:&c,trace:vec![]};
            let mut a=Derived{base:Agent::new(Some(7)),attribute_writes:0};
            assert_eq!(bind_owned(&mut a,88,&mut h)?,success);
            assert_eq!(a.attribute_writes,1);assert_eq!(a.base.action_04,Some(99));
            assert_eq!(a.base.bound_owner,Some(7));assert_eq!(a.base.owner,Some(19));
            assert_eq!(h.trace.iter().any(|v|*v==json!(["speed_value",19])),success);
            assert_eq!(a.base.speed,if success {5.}else{-1.});
        }
        Ok(())
    }

}
