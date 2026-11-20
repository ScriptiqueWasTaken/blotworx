//! PC b0e6c0 constructor fast fields. Blocking-conversation default is true;
//! these defaults must not be taken from examples in the component catalog.
use anyhow::{ensure,Context,Result};
use epic_mickey_lib_rs::scene_file::Component;
pub struct Properties {pub radius:f32,pub during_conversation:bool,pub by_player:i32}
impl Properties {
    pub fn read(component:&Component)->Result<Self> {
        ensure!(component.class_name=="UsableComponent","Expected usable component");
        let property=|name:&str|component.properties.iter().find(|p|p.name==name).map(|p|&p.value);
        let radius=property("UsableRadius").map(|v|v.as_f64().context("Invalid usable radius")).transpose()?.unwrap_or(0.) as f32;
        ensure!(radius.is_finite(),"Nonfinite usable radius");
        let during_conversation=property("UsableWhileInBlockingConversation").map(|v|v.as_bool().context("Invalid usable conversation flag")).transpose()?.unwrap_or(true);
        let by_player=property("UsableByPlayer").map(|v|v.as_i64().context("Invalid usable player selector")).transpose()?.unwrap_or(0).try_into()?;
        Ok(Self{radius,during_conversation,by_player})
    }
    pub fn allows_player(&self,index:Option<usize>)->bool {
        self.by_player==0 || self.by_player==1 && index==Some(0) || self.by_player==2 && index==Some(1)
    }
}
