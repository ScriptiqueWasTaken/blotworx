//! Base agent action ownership456530/45675c. Attribute dispatch retains the
//! complete derived agent; action identities refer to canonical stored objects.
use anyhow::{Context,Result};
use super::ai_agent_base::Owner;
pub trait Host<S:Owner> {
    fn child_count(&mut self,root:usize)->Result<u32>;
    fn child(&mut self,root:usize,index:u32)->Result<usize>;
    fn name_matches(&mut self,node:usize,name:&str)->Result<bool>;
    /// Native comparison re-reads both the child and its value for each token.
    fn value_matches(&mut self,node:usize,value:&str)->Result<bool>;
    fn owner_action(&mut self,owner:usize)->Result<Option<usize>>;
    fn action_class(&mut self,action:usize)->Result<usize>;
    fn construct_action(&mut self,class:usize)->Result<Option<usize>>;
    fn attribute(&mut self,agent:&mut S,node:usize)->Result<u8>;
    fn acknowledge_action(&mut self,action:usize)->Result<()>;
}
pub fn initialize<S:Owner,H:Host<S>>(agent:&mut S,root:usize,host:&mut H)->Result<bool> {
    anyhow::ensure!(root!=0,"Agent attributes have no root node");
    let mut index=0u32;
    while index<host.child_count(root)? {
        let node=host.child(root,index)?;
        if host.name_matches(node,"ShareBrainAction")? {
            let mut parsed=None;
            for (token,value) in [("yes",true),("true",true),("1",true),("no",false),("false",false),("0",false)] {
                let node=host.child(root,index)?;
                if host.value_matches(node,token)? {parsed=Some(value);break;}
            }
            match parsed {
                None=>return Ok(false),
                Some(true)=>{
                    let Some(owner)=agent.base().owner else{return Ok(false)};
                    agent.base_mut().action_04=host.owner_action(owner)?;
                }
                Some(false)=>{}, // Native false does not clear an existing action.
            }
        }
        index=index.wrapping_add(1);
    }
    if agent.base().action_04.is_none() {
        if let Some(owner)=agent.base().owner {
            let action=host.owner_action(owner)?.context("Owner brain has no action to supply agent action metadata")?;
            let class=host.action_class(action)?;
            agent.base_mut().action_04=host.construct_action(class)?;
        }
    }
    index=0;
    while index<host.child_count(root)? {
        let node=host.child(root,index)?;
        if !host.name_matches(node,"Class")? && !host.name_matches(node,"ShareBrainAction")? {
            if host.attribute(agent,node)?==0 {return Ok(false)}
        }
        index=index.wrapping_add(1);
    }
    Ok(true)
}
pub fn acknowledge<S:Owner,H:Host<S>>(agent:&S,host:&mut H)->Result<bool> {
    let action=agent.base().action_04;
    let owner=agent.base().owner.context("Agent action acknowledgment has no owner")?;
    if action!=host.owner_action(owner)? {
        host.acknowledge_action(action.context("Agent acknowledgment has no private action")?)?;
    }
    Ok(true)
}
pub trait DestructionHost {
    fn owner_action(&mut self,owner:usize)->Result<Option<usize>>;
    fn destroy_action(&mut self,action:usize,deleting:i32)->Result<()>;
    fn free_agent(&mut self,identity:usize)->Result<()>;
}
///45645c: shared actions survive agent destruction; private ones are deleted.
/// The native object retains its pointer until its owner removes the object.
pub fn destroy<S:Owner,H:DestructionHost>(agent:&S,identity:usize,deleting:i32,host:&mut H)->Result<()> {
    if let Some(action)=agent.base().action_04 {
        let shared=match agent.base().owner {Some(owner)=>host.owner_action(owner)?,None=>None};
        if shared!=Some(action) {host.destroy_action(action,1)?;}
    }
    if deleting>0 {host.free_agent(identity)?;}
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ai_agent_base::Agent;
    #[derive(Default)]
    struct Fixture {nodes:Vec<(String,String)>,trace:Vec<String>,shared:Option<usize>,created:Option<usize>,replace:bool,reads:usize}
    impl Host<Agent> for Fixture {
        fn child_count(&mut self,_:usize)->Result<u32>{Ok(self.nodes.len() as u32)}
        fn child(&mut self,_:usize,i:u32)->Result<usize>{self.trace.push(format!("child:{i}"));Ok(i as usize)}
        fn name_matches(&mut self,n:usize,s:&str)->Result<bool>{Ok(self.nodes[n].0.eq_ignore_ascii_case(s))}
        fn value_matches(&mut self,n:usize,s:&str)->Result<bool>{self.reads+=1;if self.replace && self.reads==2 {self.nodes[n].1="true".into();}Ok(self.nodes[n].1.eq_ignore_ascii_case(s))}
        fn owner_action(&mut self,o:usize)->Result<Option<usize>>{self.trace.push(format!("owner:{o}"));Ok(self.shared)}
        fn action_class(&mut self,a:usize)->Result<usize>{self.trace.push(format!("class:{a}"));Ok(40)}
        fn construct_action(&mut self,c:usize)->Result<Option<usize>>{self.trace.push(format!("construct:{c}"));Ok(self.created)}
        fn attribute(&mut self,a:&mut Agent,n:usize)->Result<u8>{self.trace.push(format!("attribute:{n}"));a.speed=7.;Ok(if self.nodes[n].0=="Fail" {0}else{1})}
        fn acknowledge_action(&mut self,a:usize)->Result<()>{self.trace.push(format!("ack:{a}"));Ok(())}
    }
    #[test]
    fn shared_actions_alias_and_false_does_not_replace_them()->Result<()> {
        let mut a=Agent::new(Some(7));
        let mut h=Fixture{nodes:vec![("ShareBrainAction".into(),"TRUE".into()),("ShareBrainAction".into(),"false".into()),("Speed".into(),"2".into())],shared:Some(20),created:Some(30),..Default::default()};
        assert!(initialize(&mut a,1,&mut h)?);assert_eq!(a.action_04,Some(20));assert_eq!(a.speed,7.);
        assert!(acknowledge(&a,&mut h)?);assert!(!h.trace.iter().any(|t|t.starts_with("construct")||t.starts_with("ack")));
        Ok(())
    }
    #[test]
    fn private_action_factory_and_failed_attributes_keep_native_partial_writes()->Result<()> {
        let mut a=Agent::new(Some(7));
        let mut h=Fixture{nodes:vec![("Class".into(),"Flee".into()),("Fail".into(),"".into())],shared:Some(20),created:Some(30),..Default::default()};
        assert!(!initialize(&mut a,1,&mut h)?);assert_eq!(a.action_04,Some(30));assert_eq!(a.speed,7.);
        acknowledge(&a,&mut h)?;assert_eq!(h.trace.last().unwrap(),"ack:30");
        a.action_04=None;h.created=None;h.nodes.clear();assert!(initialize(&mut a,1,&mut h)?);assert_eq!(a.action_04,None);
        assert!(acknowledge(&a,&mut h).is_err());Ok(())
    }
    #[test]
    fn boolean_comparisons_requery_and_invalid_values_preserve_existing_action()->Result<()> {
        let mut a=Agent::new(Some(7));
        let mut h=Fixture{nodes:vec![("ShareBrainAction".into(),"invalid".into())],shared:Some(20),replace:true,..Default::default()};
        assert!(initialize(&mut a,1,&mut h)?);assert_eq!(h.reads,2);assert_eq!(a.action_04,Some(20));
        h.replace=false;h.nodes[0].1="invalid".into();assert!(!initialize(&mut a,1,&mut h)?);assert_eq!(a.action_04,Some(20));
        a.owner=None;h.nodes[0].1="true".into();assert!(!initialize(&mut a,1,&mut h)?);Ok(())
    }
    struct Original<'a> {case:&'a serde_json::Value,trace:Vec<serde_json::Value>,reads:[usize;3],count:u32}
    impl Host<Agent> for Original<'_> {
        fn child_count(&mut self,_:usize)->Result<u32>{Ok(self.count)}
        fn child(&mut self,_:usize,i:u32)->Result<usize>{self.reads[0]+=1;Ok(i as usize)}
        fn name_matches(&mut self,n:usize,s:&str)->Result<bool>{self.reads[1]+=1;Ok(self.case["nodes"][n][0].as_str().unwrap().eq_ignore_ascii_case(s))}
        fn value_matches(&mut self,n:usize,s:&str)->Result<bool>{self.reads[2]+=1;let value=if self.case["mutate"]=="value" && self.reads[2]>=2 {"true"}else{self.case["nodes"][n][1].as_str().unwrap()};Ok(value.eq_ignore_ascii_case(s))}
        fn owner_action(&mut self,_:usize)->Result<Option<usize>>{Ok(if self.case.get("shared").is_some(){self.case["shared"].as_u64().map(|v|v as usize)}else{Some(20)})}
        fn action_class(&mut self,a:usize)->Result<usize>{self.trace.push(serde_json::json!(["class",a]));Ok(40)}
        fn construct_action(&mut self,c:usize)->Result<Option<usize>>{self.trace.push(serde_json::json!(["construct",c]));Ok(if self.case.get("created").is_some(){self.case["created"].as_u64().map(|v|v as usize)}else{Some(30)})}
        fn attribute(&mut self,a:&mut Agent,n:usize)->Result<u8>{self.trace.push(serde_json::json!(["attribute",n]));if self.case["mutate"]=="owner"{a.owner=None;}if self.case["mutate"]=="truncate"{self.count=0;}Ok(self.case["attribute_result"].as_u64().unwrap_or(1) as u8)}
        fn acknowledge_action(&mut self,a:usize)->Result<()>{self.trace.push(serde_json::json!(["ack",a]));Ok(())}
    }
    impl DestructionHost for Original<'_> {
        fn owner_action(&mut self,o:usize)->Result<Option<usize>>{<Self as Host<Agent>>::owner_action(self,o)}
        fn destroy_action(&mut self,a:usize,d:i32)->Result<()>{self.trace.push(serde_json::json!(["destroy",a,d]));Ok(())}
        fn free_agent(&mut self,i:usize)->Result<()>{self.trace.push(serde_json::json!(["free",i]));Ok(())}
    }
    #[test]
    fn original_action_initialization_acknowledgment_and_destruction_match()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_action_init.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];
            let mut a=Agent::new(c["owner"].as_bool().unwrap_or(true).then_some(7));a.action_04=c["action"].as_u64().map(|v|v as usize);
            let mut h=Original{case:c,trace:vec![],reads:[0;3],count:c["nodes"].as_array().map_or(0,|n|n.len() as u32)};
            let result=match c["op"].as_str().unwrap(){
                "initialize"=>initialize(&mut a,1,&mut h)?,"ack"=>acknowledge(&a,&mut h)?,
                "destroy"=>{destroy(&a,1,c["deleting"].as_i64().unwrap_or(0) as i32,&mut h)?;true},_=>unreachable!(),
            };
            assert_eq!(serde_json::json!({"action":a.action_04,"owner":a.owner,"result":result,"trace":h.trace,"reads":h.reads}),
                serde_json::json!({"action":expected["action"],"owner":expected["owner"],"result":expected["result"],"trace":expected["trace"],"reads":expected["reads"]}),"{c}");
        }
        Ok(())
    }

}
