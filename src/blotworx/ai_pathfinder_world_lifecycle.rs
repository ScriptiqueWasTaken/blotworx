//! CPathFinder World removal4895b0, actual495d40 reset, and graph-listener
//! removal475788. The State is the owned service, not a raw record copy.
use super::{ai_graph_link_passes::Memory,ai_pathfinder_constructor::State,
    ai_pathfinder_reset as reset};
use anyhow::Result;
use std::collections::BTreeMap;
/// Opaque identity of the source graph-change callback; never executed as code.
pub const GRAPH_CHANGED_CALLBACK:u32=0x489520;
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
pub fn remove_listener(m:&mut Memory,graph:u32,function:u32,data:u32)->Result<()>{
    let count=w(m,graph,0x18)? as i32;
    if count<=0{return Ok(())}
    for i in 0..count as u32 {
        if w(m,w(m,graph,0x20)?,i.wrapping_mul(4))?!=function{continue}
        if w(m,w(m,graph,0x24)?,i.wrapping_mul(4))?!=data{continue}
        m.write(graph.wrapping_add(0x18),w(m,graph,0x18)?.wrapping_sub(1))?;
        let mut at=i;
        while (at as i32)<(w(m,graph,0x18)? as i32) {
            let slot=w(m,graph,0x20)?.wrapping_add(at.wrapping_mul(4));
            m.write(slot,w(m,slot,4)?)?;
            let slot=w(m,graph,0x24)?.wrapping_add(at.wrapping_mul(4));
            m.write(slot,w(m,slot,4)?)?;
            at=at.wrapping_add(1);
        }
        //Native removes the first matching pair and retains the stale tail.
        return Ok(())
    }Ok(())
}
fn after_reset(state:&mut State,m:&mut Memory)->Result<()>{
    if let Some(graph)=state.references_3c_4c_50_54_58_60_6c[2] {
        if state.flags_9c_9d[0]!=0 {
            remove_listener(m,u32::try_from(graph)?,GRAPH_CHANGED_CALLBACK,u32::try_from(state.identity)?)?;
        }
        state.flags_9c_9d[0]=0;
    }
    //A null50 leaves9c untouched in the original. Both references are cleared
    //regardless; read them only after reset's callbacks have finished.
    state.references_3c_4c_50_54_58_60_6c[2]=None;
    state.references_3c_4c_50_54_58_60_6c[3]=None;
    Ok(())
}
pub fn removed20(state:&mut State,m:&mut Memory,h:&mut impl reset::Host)->Result<()>{
    reset::reset(state,m,h)?;
    after_reset(state,m)
}
/// Normal concrete modifier dispatch; every reset operates on its real owned
/// object. Goal/reference ownership services remain required from the host.
pub fn removed20_concrete<H:reset::Host+super::ai_goto_gap::Host>(state:&mut State,m:&mut Memory,
    modifiers:&mut BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,h:&mut H)->Result<()> {
    super::ai_pathfinder_concrete_modifiers::reset_owned(state,m,modifiers,h)?;
    after_reset(state,m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn graph_listener_first_pair_removal_matches_original_counts_shifts_and_stale_tail()->Result<()>{
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_world_lifecycle.json"))?;
        for case in f["listeners"].as_array().unwrap(){
            let mut m:Memory=serde_json::from_value(case["memory"].clone())?;
            for _ in 0..case["input"]["repeat"].as_u64().unwrap_or(1){
                remove_listener(&mut m,100,GRAPH_CHANGED_CALLBACK,0x72000000)?;
            }
            assert_eq!(serde_json::to_value(&m)?,case["expected"],"{}",case["input"]);
        }Ok(())
    }
}
