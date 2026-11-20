//! Pathfinder service binding1956e0 and goto binding1c1f50.
//! Actual traversal/service lookup and allocation remain mandatory services.
use anyhow::{Context as _,Result};

#[derive(Debug,serde::Serialize)]
pub struct Action {
    pub allocation:usize,
    pub references:u32,
    pub vector:[f32;3],
    pub scalar:f32,
}
#[derive(Debug,serde::Serialize)]
pub struct Binding {
    pub traversal_id:u32, //0
    pub world_service:Option<usize>, //4
    pub owner_service:Option<usize>, //c
    pub owner_field_40:u32, //7c
    pub buffer:Option<usize>, //880
    pub flag_8a4:bool,
    pub squared_limit:f32, //8a8
    pub action:Option<Action>, //8ac
}
impl Default for Binding {
    fn default()->Self {Self {traversal_id:u32::MAX,world_service:None,owner_service:None,
        owner_field_40:0,buffer:None,flag_8a4:false,squared_limit:0.,action:None}}
}
pub trait Host {
    fn world_service_61d14(&mut self,binding:&mut Binding)->Result<Option<usize>>;
    fn owner_service_6af6c(&mut self,binding:&mut Binding,owner:usize)->Result<Option<usize>>;
    fn owner_field_40(&mut self,binding:&mut Binding,owner:usize)->Result<u32>;
    fn traversal_id(&mut self,binding:&mut Binding,world:usize,name:&str)->Result<u32>;
    fn buffer_size(&mut self,binding:&mut Binding,service:usize)->Result<u32>;
    fn service_radius(&mut self,binding:&mut Binding,service:usize)->Result<f32>;
    fn allocate(&mut self,binding:&mut Binding,size:u32)->Result<Option<usize>>;
    fn store_buffer_size(&mut self,binding:&mut Binding,buffer:usize,size:u32)->Result<()>;
}
impl Binding {
    pub fn bind_base(&mut self,name:&str,owner:Option<usize>,host:&mut impl Host)->Result<bool> {
        self.world_service=host.world_service_61d14(self)?;
        if self.world_service.is_none() {return Ok(false)};
        let Some(owner)=owner else {return Ok(false)};
        self.owner_service=host.owner_service_6af6c(self,owner)?;
        //195758 reads the service after querying the owner, not before it.
        let world=self.world_service.context("Pathfinder owner query removed its world service")?;
        self.owner_field_40=host.owner_field_40(self,owner)?;
        let id=host.traversal_id(self,world,name)?;
        if id==u32::MAX {return Ok(false)};
        self.traversal_id=id;
        Ok(true)
    }

    pub fn bind_goto(&mut self,name:&str,owner:Option<usize>,host:&mut impl Host)->Result<bool> {
        let bound=self.bind_base(name,owner,host)?;
        if bound {
            let service=self.owner_service.context("Goto traversal has no owner service for its buffer")?;
            let size=host.buffer_size(self,service)?;
            self.buffer=host.allocate(self,size)?;
            // Native stores the returned pointer then writes its first word;
            // a missing allocation is an explicit error, never a usable path.
            let buffer=self.buffer.context("Goto traversal buffer allocation unavailable")?;
            host.store_buffer_size(self,buffer,size)?;
            self.flag_8a4=false;
        }
        //1c1fe8 executes even when base binding returned false. Preserve those
        // writes/allocations and the original return result independently.
        let service=self.owner_service.context("Goto traversal requires an owner service after base binding")?;
        let radius=host.service_radius(self,service)?;
        self.squared_limit=radius*radius;
        let allocation=host.allocate(self,0x18)?;
        self.action=allocation.map(|allocation|Action{allocation,references:0,vector:[0.;3],scalar:0.});
        Ok(self.action.is_some()&&bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a> {case:&'a Value,trace:Vec<Value>,allocations:usize}
    impl Fixture<'_> {
        fn flag(&self,key:&str,default:bool)->bool {self.case[key].as_bool().unwrap_or(default)}
        fn base_result(&self)->bool {self.flag("world",true)&&self.flag("owner",true)&&self.case["id"].as_u64().unwrap_or(7)!=u32::MAX as u64}
    }
    impl Host for Fixture<'_> {
        fn world_service_61d14(&mut self,_:&mut Binding)->Result<Option<usize>> {
            self.trace.push(json!(["world"]));Ok(self.flag("world",true).then_some(11))
        }
        fn owner_service_6af6c(&mut self,b:&mut Binding,owner:usize)->Result<Option<usize>> {
            self.trace.push(json!(["owner_service",owner]));
            if self.flag("switch_world",false){b.world_service=Some(10);}
            Ok(self.flag("service",true).then_some(21))
        }
        fn owner_field_40(&mut self,_:&mut Binding,owner:usize)->Result<u32> {self.trace.push(json!(["owner_field",owner]));Ok(0x12345678)}
        fn traversal_id(&mut self,_:&mut Binding,world:usize,name:&str)->Result<u32> {
            self.trace.push(json!(["traversal",world,name]));
            let target=self.case["id"].as_u64().unwrap_or(7) as u32;
            let mut registry=super::super::ai_traversal_registry::Registry::default();
            for index in 0..8 {
                registry.append(if index==target {b"GotoTraversal"}else{b"OtherTraversal"})?;
            }
            Ok(registry.find(Some(name.as_bytes())))
        }
        fn buffer_size(&mut self,_:&mut Binding,service:usize)->Result<u32> {
            self.trace.push(json!(["buffer_size",service]));Ok(self.case["size"].as_u64().unwrap_or(32) as u32)
        }
        fn service_radius(&mut self,_:&mut Binding,service:usize)->Result<f32> {
            self.trace.push(json!(["radius",service]));
            Ok(if service==20 {2.} else {f32::from_bits(self.case["radius"].as_u64().unwrap_or(0.5f32.to_bits() as u64) as u32)})
        }
        fn allocate(&mut self,b:&mut Binding,size:u32)->Result<Option<usize>> {
            self.trace.push(json!(["allocate",size]));
            let buffer=self.allocations==0&&self.base_result();self.allocations+=1;
            let result=if buffer {(!self.flag("buffer_failure",false)).then_some(88)} else {self.flag("action",true).then_some(89)};
            if buffer&&self.flag("switch_service",false){b.owner_service=Some(20);}
            if self.flag("hook",false){self.trace.push(json!(["allocation_hook",result,size,123]));}
            Ok(result)
        }
        fn store_buffer_size(&mut self,_:&mut Binding,buffer:usize,size:u32)->Result<()> {
            self.trace.push(json!(["store_buffer_size",buffer,size]));Ok(())
        }
    }
    #[test]
    fn original_pathfinder_and_goto_binding_service_order_partial_writes_and_float_words_match()->Result<()> {
        let fixtures:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_binding.json"))?;
        for expected in fixtures["cases"].as_array().unwrap() {
            let c=&expected["input"];
            let mut b=Binding {traversal_id:77,world_service:Some(10),owner_service:Some(20),owner_field_40:99,
                buffer:Some(88),flag_8a4:true,squared_limit:9.,action:None};
            let mut host=Fixture{case:c,trace:vec![],allocations:0};
            let owner=host.flag("owner",true).then_some(7);
            let result=if c["op"]=="base" {b.bind_base("GotoTraversal",owner,&mut host)?}else{b.bind_goto("GotoTraversal",owner,&mut host)?};
            let action=b.action.as_ref().map(|a|json!({"allocation":a.allocation,"references":a.references,"vector":a.vector.map(f32::to_bits),"scalar":a.scalar.to_bits()}));
            let actual=json!({"trace":host.trace,"traversal_id":b.traversal_id,"world_service":b.world_service,"owner_service":b.owner_service,
                "owner_field_40":b.owner_field_40,"buffer":b.buffer,"flag_8a4":b.flag_8a4,"squared_limit":b.squared_limit.to_bits(),"action":action,"result":result});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],expected[key],"{key}: {c}");}
        }
        Ok(())
    }
    #[test]
    fn unavailable_required_goto_services_and_buffer_are_errors_not_ready_agents()->Result<()> {
        let c=json!({"service":false});let mut host=Fixture{case:&c,trace:vec![],allocations:0};
        let mut b=Binding::default();
        assert!(b.bind_goto("GotoTraversal",Some(7),&mut host).is_err());
        assert_eq!(b.traversal_id,7);assert_eq!(b.owner_service,None);
        let c=json!({"buffer_failure":true});let mut host=Fixture{case:&c,trace:vec![],allocations:0};
        let mut b=Binding::default();
        assert!(b.bind_goto("GotoTraversal",Some(7),&mut host).is_err());
        assert_eq!(b.buffer,None);assert!(b.action.is_none());assert_eq!(host.allocations,1);
        Ok(())
    }
}
