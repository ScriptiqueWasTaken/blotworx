//!187048/1870d0: controller shape -> descriptor -> temporary definition ->
//!owned entity factory. Return the actual factory entity or null; a matching
//!profile alone never establishes navigation readiness.
use anyhow::Result;
use std::collections::BTreeMap;
use super::{assets::World,ai_navigation_owned_factory::{self as owned,Storage,Metadata,Backend},ai_navigation_resolver as resolver,ai_navigation_factory,ai_navigation_definitions::Tree};
pub fn flags(enabled:u32,registered_flags:u32)->u32{if enabled!=0{registered_flags}else{0}}
pub struct Factory<'a,B> {
    pub world:&'a World,pub source:&'a Tree,pub manager:Option<&'a resolver::Manager>,
    pub resolution:&'a mut resolver::Runtime,pub runtime:&'a mut ai_navigation_factory::Runtime,
    pub metadata:&'a Metadata,pub storage:&'a mut Storage,pub backend:&'a mut B,
    ///The owning registry's actual51b9e8 value; never invent a default bitmask.
    pub registered_flags:u32,
}
impl<B: Backend> super::ai_controller_navigation_world::GameplayFactory for Factory<'_, B> {
    fn create_obstacle(&mut self, name: &[u8], user: [usize;2]) -> Result<Option<usize>> {
        self.create_profile(b"Obstacle", name, user, 0)
    }
    fn navigation_brain(&self, wrapper: usize) -> Result<Option<usize>> {
        self.storage.gameplay_navigation_brain(wrapper)
    }
    fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()> {
        self.storage.bind_gameplay_brain(brain, gameplay)
    }
}
impl<B:Backend> super::ai_controller_navigation_world::Factory for Factory<'_,B>{
    fn create(&mut self,radius:i32,height:i32,movement:i32,name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>>{
        let descriptor=super::ai_agent_profiles::descriptor(radius,height,movement);
        self.create_profile(descriptor.as_bytes(),name,user,enabled)
    }
}
impl<B:Backend> Factory<'_,B> {
    fn create_profile(&mut self,profile:&[u8],name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>> {
        let Some(mut definition)=self.resolution.resolve(self.manager,self.source,Some(profile))? else{return Ok(None)};
        //The original inline user pair lives through this synchronous call;
        //the concrete entity constructor copies its two words into owned data.
        let users=BTreeMap::from([(1,user)]);
        self.runtime.create(Some(name),Some(definition.root),flags(enabled,self.registered_flags),Some(1),
            &mut owned::Binding{world:self.world,tree:&mut definition.tree,metadata:self.metadata,storage:self.storage,users:&users,backend:self.backend})
        //Temporary definition drops on success, factory failure and error.
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn native_descriptor_flag_mask_and_temporary_definition_lifecycle()->anyhow::Result<()>{
        let capture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_controller_navigation_factory.json"))?;
        for row in capture["cases"].as_array().unwrap(){
            let c=&row["input"];let profile=super::super::ai_agent_profiles::descriptor(c["radius"].as_i64().unwrap() as i32,c["height"].as_i64().unwrap() as i32,c["movement"].as_i64().unwrap() as i32);
            assert_eq!(profile,row["profile"].as_str().unwrap());
            let expected=super::flags(c["enabled"].as_u64().unwrap() as u32,c["flags"].as_u64().unwrap() as u32);
            let calls=row["trace"].as_array().unwrap();let factory=calls.iter().find(|v|v[0]=="factory");
            if let Some(call)=factory{assert_eq!(call[3].as_u64().unwrap() as u32,expected,"{c}");}
            assert_eq!(factory.is_some(),c["manager"].as_bool().unwrap()&&c["definition"].as_bool().unwrap());
            let freed=calls.iter().filter(|v|v[0]=="free").count();assert_eq!(freed,usize::from(factory.is_some()));
        }
        Ok(())
    }
}
