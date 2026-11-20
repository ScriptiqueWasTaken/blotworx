//! Real Tree/raw value storage for Goto/Wander attribute virtuals.
use anyhow::Result;
use super::{ai_navigation_definitions::Tree,ai_definition_strings::{Strings,Field},
    ai_path_agent_attributes as attributes,ai_goto_agent::Agent,ai_path_agent};
pub trait Backend {
    fn number(&mut self,a:&mut Agent,tree:&mut Tree,text:&[u8])->Result<f64>;
    fn unit_scale(&mut self,a:&mut Agent,tree:&mut Tree)->Result<Option<f32>>;
    fn maximum_speed(&mut self,a:&mut Agent,tree:&mut Tree,owner:usize)->Result<Option<f32>>;
}
pub struct Binding<'a,H>{pub tree:&'a mut Tree,pub backend:&'a mut H,strings:Strings}
impl<'a,H> Binding<'a,H> {
    pub fn new(tree:&'a mut Tree,backend:&'a mut H)->Self{Self{tree,backend,strings:Strings::default()}}
    pub fn goto(&mut self,a:&mut Agent,node:Option<usize>)->Result<bool> where H:Backend {attributes::goto(a,node,self)}
    pub fn wander(&mut self,a:&mut ai_path_agent::Agent,node:Option<usize>)->Result<bool>{attributes::wander(a,node,self)}
}
impl<H:Backend> attributes::GotoHost for Binding<'_,H> {
    fn name(&mut self,_:&mut Agent,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
    fn value(&mut self,_:&mut Agent,n:usize)->Result<Option<usize>>{self.strings.capture(self.tree,n,Field::Value)}
    fn number(&mut self,a:&mut Agent,v:usize)->Result<f64>{let text=self.strings.text(self.tree,v)?;self.backend.number(a,self.tree,&text)}
    fn unit_scale(&mut self,a:&mut Agent)->Result<Option<f32>>{self.backend.unit_scale(a,self.tree)}
    fn maximum_speed(&mut self,a:&mut Agent,o:usize)->Result<Option<f32>>{self.backend.maximum_speed(a,self.tree,o)}
}
impl<H> attributes::WanderHost for Binding<'_,H> {
    fn name(&mut self,_:&mut ai_path_agent::Agent,n:usize)->Result<Option<Vec<u8>>>{Ok(self.tree.node(n)?.name.clone())}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use anyhow::Context;
    fn agent()->Agent {
        Agent{path:ai_path_agent::Agent{base:super::super::ai_agent_base::Agent::new(Some(7)),point:ai_path_agent::Point{position:[0.;3],attributes:[None;16]},request_state:42,pathfinder:None},vector_90:[0.;3],field_9c:8,reference:Some(4),value_a4:1.,flag_a8:Some(true)}
    }
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,names:usize,values:usize,text:Option<String>}
    impl Fixture<'_> {
        fn name(&mut self)->Option<Vec<u8>> {
            let n=if let Some(ns)=self.case["names"].as_array(){&ns[self.names.min(ns.len()-1)]}else{&self.case["name"]};self.names+=1;
            n.as_str().map(|v|v.as_bytes().to_vec())
        }
    }
    impl attributes::GotoHost for Fixture<'_> {
        fn name(&mut self,_:&mut Agent,_:usize)->Result<Option<Vec<u8>>>{Ok(self.name())}
        fn value(&mut self,_:&mut Agent,_:usize)->Result<Option<usize>>{self.values+=1;self.text=self.case["value"].as_str().map(str::to_string);Ok(self.text.as_ref().map(|_|1))}
        fn number(&mut self,_:&mut Agent,v:usize)->Result<f64>{assert_eq!(v,1);self.trace.push(json!(["number",self.text]));Ok(f64::from_bits(self.case["number"].as_u64().unwrap_or(2f64.to_bits())))}
        fn unit_scale(&mut self,_:&mut Agent)->Result<Option<f32>>{
            let value=self.case["world"].as_bool().unwrap_or(true).then(||self.case["scale"].as_u64().unwrap_or(2f32.to_bits() as u64) as u32);
            self.trace.push(json!(["scale",value]));if self.case["mutation"]=="scale_value"{self.text=Some("2".into());}Ok(value.map(f32::from_bits))
        }
        fn maximum_speed(&mut self,_:&mut Agent,o:usize)->Result<Option<f32>>{self.trace.push(json!(["maximum",o]));Ok(self.case["record"].as_bool().unwrap_or(true).then(||f32::from_bits(self.case["limit"].as_u64().unwrap_or(3f32.to_bits() as u64) as u32)))}
    }
    impl attributes::WanderHost for Fixture<'_> {
        fn name(&mut self,_:&mut ai_path_agent::Agent,_:usize)->Result<Option<Vec<u8>>>{Ok(self.name())}
    }
    #[test]
    fn goto_and_wander_attributes_match_original_instructions()->Result<()> {
        let rows:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_path_agent_attributes.json"))?;
        for row in rows["cases"].as_array().unwrap() {
            let c=&row["input"];let mut a=agent();a.path.base.speed=9.;
            let node=c["node"].as_bool().unwrap_or(true).then_some(1);let mut h=Fixture{case:c,trace:vec![],names:0,values:0,text:None};
            let goto=c["class"]=="Goto";
            let result=if goto{attributes::goto(&mut a,node,&mut h)?}else{attributes::wander(&mut a.path,node,&mut h)?};
            assert_eq!(json!({"result":result,"trace":h.trace,"names":h.names,"values":h.values,"speed":a.path.base.speed.to_bits()}),json!({"result":row["result"],"trace":row["trace"],"names":row["names"],"values":row["values"],"speed":row["speed"]}),"{c}");
            if goto{assert_eq!(a.flag_a8,Some(false));assert_eq!(row["flag"],0);}else{assert_eq!(a.flag_a8,Some(true));assert_eq!(row["flag"],7);}
        }Ok(())
    }
    struct Services {node:usize,replace:bool}
    impl Backend for Services {
        fn number(&mut self,_:&mut Agent,_:&mut Tree,t:&[u8])->Result<f64>{assert_eq!(t,b"2");Ok(2.)}
        fn unit_scale(&mut self,_:&mut Agent,t:&mut Tree)->Result<Option<f32>>{
            if self.replace{t.nodes[self.node].value=Some(b"2-replaced-buffer".to_vec());}else{t.nodes[self.node].value.as_mut().unwrap()[0]=b'2';}Ok(Some(3.))
        }
        fn maximum_speed(&mut self,_:&mut Agent,_:&mut Tree,o:usize)->Result<Option<f32>>{assert_eq!(o,7);Ok(Some(5.))}
    }
    #[test]
    fn goto_real_tree_retains_value_identity_across_scale_callback()->Result<()> {
        for replace in [false,true] {
            let mut tree=Tree::default();let node=tree.append(None,super::super::ai_navigation_definitions::Node{name:Some(b"Speed".to_vec()),value:Some(b"1".to_vec()),..Default::default()})?;
            let mut services=Services{node,replace};let mut a=agent();let previous=a.path.base.speed;
            let result=Binding::new(&mut tree,&mut services).goto(&mut a,Some(node));
            if replace{assert!(result.unwrap_err().to_string().contains("replaced"));assert_eq!(a.path.base.speed,previous);}else{assert!(result?);assert_eq!(a.path.base.speed,5.);}
            assert_eq!(a.flag_a8,Some(false));
        }Ok(())
    }
    struct Bind {base:bool,path:Option<usize>,trace:Vec<&'static str>}
    impl ai_path_agent::BindingHost<Agent> for Bind {
        fn bind_base_owned(&mut self,a:&mut Agent,n:usize)->Result<bool>{assert_eq!(n,9);self.trace.push("base");a.flag_a8=Some(true);a.path.base.bound_owner=Some(17);Ok(self.base)}
        fn create_pathfinder_owned(&mut self,a:&mut Agent)->Result<Option<usize>>{self.trace.push("factory");assert_eq!(a.flag_a8,Some(true));assert_eq!(a.path.base.bound_owner,Some(17));a.reference=Some(22);Ok(self.path)}
        fn initialize_owned(&mut self,a:&mut Agent)->Result<()>{self.trace.push("reset");a.path.pathfinder.context("Required Goto path factory returned null")?;assert_eq!(a.reference,Some(22));a.reference=None;a.field_9c=-1;a.flag_a8=Some(false);a.path.request_state=-1;Ok(())}
    }
    #[test]
    fn goto_bind_keeps_derived_fields_and_stores_factory_result_before_reset()->Result<()> {
        for (base,path) in [(false,Some(88)),(true,None),(true,Some(88))] {
            let mut a=agent();let mut h=Bind{base,path,trace:vec![]};let result=a.bind(9,&mut h);
            if !base{assert!(!result?);assert_eq!(h.trace,["base"]);assert_eq!(a.flag_a8,Some(true));}
            else if path.is_none(){assert!(result.is_err());assert_eq!(a.path.pathfinder,None);assert_eq!(h.trace,["base","factory","reset"]);}
            else{assert!(result?);assert_eq!(a.path.pathfinder,path);assert_eq!(a.flag_a8,Some(false));assert_eq!(a.reference,None);assert_eq!(a.path.request_state,-1);assert_eq!(h.trace,["base","factory","reset"]);}
        }Ok(())
    }
}
