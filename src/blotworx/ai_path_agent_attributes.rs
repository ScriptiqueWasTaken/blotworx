//! Goto1c098c and Wander2b7ca8 virtual10. Values remain raw handles across
//! scale callbacks. Both final private-name tests perform fresh name reads.
use anyhow::{Context,Result};
use super::{ai_goto_agent::Agent,ai_path_agent,ai_traversal_registry::names_equal};
pub trait GotoHost {
    fn name(&mut self,a:&mut Agent,node:usize)->Result<Option<Vec<u8>>>;
    fn value(&mut self,a:&mut Agent,node:usize)->Result<Option<usize>>;
    fn number(&mut self,a:&mut Agent,value:usize)->Result<f64>;
    fn unit_scale(&mut self,a:&mut Agent)->Result<Option<f32>>;
    fn maximum_speed(&mut self,a:&mut Agent,owner:usize)->Result<Option<f32>>;
}
pub fn goto(a:&mut Agent,node:Option<usize>,h:&mut impl GotoHost)->Result<bool> {
    a.flag_a8=Some(false);
    let Some(node)=node else{return Ok(false)};
    let name=h.name(a,node)?.context("Goto attribute has no name for comparison")?;
    if names_equal(&name,b"Speed") {
        if let Some(value)=h.value(a,node)? {
            let scale=h.unit_scale(a)?.unwrap_or(1.);
            let value=(h.number(a,value)? as f32)*scale;
            if !(value<=0.) {
                let owner=a.path.base.owner.context("Goto speed attribute has no owner")?;
                a.path.base.speed=match h.maximum_speed(a,owner)? {Some(limit) if value>limit=>limit,_=>value};
            }
            return Ok(true)
        }
    }
    if h.name(a,node)?.is_some() {
        return Ok(h.name(a,node)?.context("Goto private-name buffer became null")?.first()==Some(&b'_'))
    }
    Ok(false)
}
pub trait WanderHost {
    fn name(&mut self,a:&mut ai_path_agent::Agent,node:usize)->Result<Option<Vec<u8>>>;
}
pub fn wander(a:&mut ai_path_agent::Agent,node:Option<usize>,h:&mut impl WanderHost)->Result<bool> {
    let Some(node)=node else{return Ok(false)};
    if h.name(a,node)?.is_some() {
        return Ok(h.name(a,node)?.context("Wander private-name buffer became null")?.first()==Some(&b'_'))
    }
    Ok(false)
}
