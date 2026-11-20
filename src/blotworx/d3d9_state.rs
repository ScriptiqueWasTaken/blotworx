//! Ordered effect render-state writes. IDs are the public D3D9 device enums;
//! effect-state ordinals are a different encoding and must not be forwarded.
use anyhow::{ensure,Context,Result};
use super::d3d9_effect::{Data,Effect};
#[derive(Debug,serde::Serialize)]
pub(super) struct State {pub state:u32,pub value:u32}
/// Alpha testing is a raster operation in D3D9, absent from core OpenGL.
/// Keep it outside the translated authored shader and preserve inherited state.
#[derive(Debug,Clone,Copy)]
pub(super) struct AlphaTest {pub enabled:bool,pub comparison:u32,pub reference:u8}
impl Default for AlphaTest {fn default()->Self{Self {enabled:false,comparison:8,reference:0}}}
impl AlphaTest {
    pub fn apply(&mut self,writes:&[State])->Result<()> {
        let mut next=*self;
        for write in writes {match write.state {
            15=>next.enabled=write.value!=0,
            24=>{ensure!(write.value<=255,"Alpha reference outside D3D9 byte range");next.reference=write.value as u8;},
            25=>{ensure!((1..=8).contains(&write.value),"Invalid alpha comparison");next.comparison=write.value;},
            _=>{},
        }}
        *self=next;Ok(())
    }
}
pub(super) fn resolve(effect:&Effect,technique:usize,pass:usize)->Result<Vec<State>> {
    let pass=effect.techniques.get(technique).context("Render-state technique is absent")?.passes.get(pass).context("Render-state pass is absent")?;
    let mut result=Vec::new();
    for assignment in &pass.states {
        if matches!(assignment.kind,146|147){continue;}
        ensure!(assignment.index==0,"Indexed render-state assignment requires its decoder");
        let state=match assignment.kind {
            0=>7,3=>14,4=>15,6=>19,7=>20,8=>22,9=>23,10=>24,11=>25,13=>27,
            22=>52,23=>53,24=>54,25=>55,26=>56,27=>57,
            73=>168,79=>175,98=>195,99=>206,100=>207,101=>208,102=>209,
            kind=>anyhow::bail!("Effect render state {kind} requires its device-state mapping"),
        };
        let Data::Words(words)=&assignment.value.data else{anyhow::bail!("Render-state assignment requires expression evaluation")};
        ensure!(words.len()==1,"Render-state assignment is not scalar");
        // Float states (depth bias) are already IEEE bits. Converting their
        // numeric value to an integer would corrupt the D3D state payload.
        result.push(State {state,value:words[0]});
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::d3d9_effect::{State as Assignment,Value,Technique,Pass};
    #[test]
    fn alpha_state_inherits_and_rejects_bad_updates_atomically()->Result<()> {
        let mut state=AlphaTest::default();
        state.apply(&[State {state:15,value:1},State {state:24,value:128},State {state:25,value:5}])?;
        state.apply(&[State {state:14,value:0}])?;
        assert!(state.enabled);assert_eq!(state.reference,128);assert_eq!(state.comparison,5);
        assert!(state.apply(&[State {state:15,value:0},State {state:25,value:9}]).is_err());
        assert!(state.enabled);assert_eq!(state.comparison,5);
        assert!(state.apply(&[State {state:24,value:256}]).is_err());
        state.apply(&[State {state:15,value:0}])?;assert!(!state.enabled);Ok(())
    }
    fn effect(assignments:&[(u32,u32)])->Effect {Effect {parameters:vec![],techniques:vec![Technique {name:None,annotations:vec![],passes:vec![Pass {name:None,annotations:vec![],states:assignments.iter().map(|&(kind,value)|Assignment {kind,index:0,value:Value {kind:2,class:2,name:None,semantic:None,elements:0,columns:1,rows:1,data:Data::Words(vec![value])}}).collect()}]}],objects:vec![],object_slots:0,trailing_bytes:0}}
    #[test]
    fn device_enums_order_and_float_payloads_are_preserved()->Result<()> {
        let e=effect(&[(0,1),(3,0),(79,0.25f32.to_bits()),(98,(-0.5f32).to_bits()),(3,1)]);let states=resolve(&e,0,0)?;
        assert_eq!(states.iter().map(|s|(s.state,s.value)).collect::<Vec<_>>(),[(7,1),(14,0),(175,0.25f32.to_bits()),(195,(-0.5f32).to_bits()),(14,1)]);Ok(())
    }
    #[test]
    fn color_write_mask_matches_d3dx_pass_capture()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/d3dx_sampler_inheritance.json"))?;
        for mask in reference["pass_color_masks"].as_array().unwrap() {
            let states=resolve(&effect(&[(73,mask.as_u64().unwrap() as u32)]),0,0)?;
            assert_eq!(states[0].state,168);assert_eq!(states[0].value as u64,mask.as_u64().unwrap());
        }Ok(())
    }
    #[test]
    fn unknown_states_and_invalid_passes_are_errors() {
        assert!(resolve(&effect(&[(145,1)]),0,0).is_err());assert!(resolve(&effect(&[]),1,0).is_err());
    }
}
