//! Global volume/LPF SIS offsets and worker-clock transitions.
use super::audio_transition::{Tables,Transition};
use anyhow::{ensure,Result};
use std::{collections::{BTreeMap,VecDeque}, sync::{Arc, Weak}};
use super::audio_events::RegisteredObject;
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum Property {Volume,LowPass}
impl Property {pub fn from_action(kind:u32)->Result<Self>{match kind {0xa010|0xb010|0xa011|0xb011=>Ok(Self::Volume),0xe010|0xf010=>Ok(Self::LowPass),_=>anyhow::bail!("Unsupported SIS parameter {kind:08x}")}}fn index(self)->usize {match self{Self::Volume=>0,Self::LowPass=>1}}}
#[derive(Clone,Copy,Debug)]
pub struct Definition {pub id:u32,pub base:[f32;2]}
pub struct Command {pub node:u32,pub owner:Option<Arc<RegisteredObject>>,pub property:Property,pub mode:u32,pub value:f32,pub curve:u8,pub duration:i32,pub due:u32}
#[derive(Clone,Copy,Debug)]
pub struct Change {pub node:u32,pub owner:Option<u32>,pub property:Property,pub delta:f32}
#[derive(Default)]
struct State {offset:f32,transition:Option<Transition>}
struct Node {base:[f32;2],states:[State;2]}
struct ObjectNode {owner:Weak<RegisteredObject>,states:[State;2]}
fn target(base:f32,current:f32,mode:u32,value:f32)->f32 {match mode {0=>0.,1=>value-base,_=>current+value}}
#[derive(Default)]
pub struct Runtime {
    nodes:BTreeMap<u32,Node>,
    objects:BTreeMap<(u32,u32),ObjectNode>,
    pending:VecDeque<Command>,
    active:Vec<(u32,Option<u32>,Property)>,
}
impl Runtime {
    pub fn define(&mut self,definitions:impl IntoIterator<Item=Definition>){for d in definitions {self.nodes.entry(d.id).or_insert_with(||Node{base:d.base,states:std::array::from_fn(|_|State::default())});}}
    pub fn enqueue(&mut self,c:Command){let at=self.pending.partition_point(|v|v.due<=c.due);self.pending.insert(at,c);}
    pub fn active(&self)->bool {!self.pending.is_empty() || !self.active.is_empty()}
    pub fn initial(&self,lineage:&[u32],owner:Option<u32>)->[f32;2] {
        let mut result=[0.;2];
        for id in lineage {
            if let Some(node)=self.nodes.get(id) {
                for (i, value) in result.iter_mut().enumerate() { *value += node.states[i].offset; }
            }
            if let Some(node)=owner.and_then(|owner|self.objects.get(&(*id,owner))) {
                for (i, value) in result.iter_mut().enumerate() { *value += node.states[i].offset; }
            }
        }
        result
    }
    pub fn dispatch(&mut self,tables:&Tables,tick:u32)->Result<Vec<Change>> {
        let mut changes=Vec::new();
        while self.pending.front().is_some_and(|c|c.due<=tick) {
            let c=self.pending.pop_front().unwrap();ensure!(c.mode<=2 && c.curve<=8 && c.value.is_finite(),"Invalid SIS parameter command");
            let Some(n)=self.nodes.get_mut(&c.node)else {continue};let i=c.property.index();
            let owner = c.owner.as_ref().map(|owner|owner.handle);
            let state = if let Some(owner) = &c.owner {
                let states = self.objects.entry((c.node,owner.handle)).or_insert_with(|| ObjectNode {
                    owner:Arc::downgrade(owner),states:std::array::from_fn(|_|State::default()),
                });
                &mut states.states[i]
            } else {
                &mut n.states[i]
            };
            let value=match c.mode {0=>0.,1=>c.value-n.base[i],_=>c.value};
            if let Some(t)=&mut state.transition {t.retarget_relative(tables,value,c.curve,c.duration,tick,c.mode==2)?;}
            else {
                let target=target(n.base[i],state.offset,c.mode,c.value);
                if target==state.offset || c.duration==0 {let delta=target-state.offset;state.offset=target;changes.push(Change{node:c.node,owner,property:c.property,delta});}
                else {state.transition=Some(if c.property==Property::Volume {Transition::new(tables,state.offset,target,c.curve,c.duration,tick)?}else {Transition::new_linear(state.offset,target,c.curve,c.duration,tick)});self.active.push((c.node,owner,c.property));}
            }
        }
        Ok(changes)
    }
    pub fn advance(&mut self,tables:&Tables,tick:u32)->Result<Vec<Change>> {
        let mut changes=self.dispatch(tables,tick)?;
        // Native due actions precede transitions at the incremented tick.
        let mut retired=Vec::new();
        self.objects.retain(|_, node| node.owner.strong_count() != 0);
        for &(node,owner,property) in &self.active {
            let state=match owner {
                Some(owner) => {
                    let Some(object)=self.objects.get_mut(&(node,owner)) else {
                        retired.push((node,Some(owner),property));
                        continue;
                    };
                    &mut object.states[property.index()]
                }
                None => &mut self.nodes.get_mut(&node).unwrap().states[property.index()],
            };
            let (value,complete)=state.transition.as_mut().unwrap().sample(tables,tick.wrapping_add(1))?;
            let delta=value-state.offset;state.offset=value;changes.push(Change{node,owner,property,delta});
            if complete {state.transition=None;retired.push((node,owner,property));}
        }
        self.active.retain(|key|!retired.contains(key));Ok(changes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_volume_preserves_other_emitters_and_reused_object_ids() -> Result<()> {
        let tables = Tables::retail()?;
        let mut runtime = Runtime::default();
        let mut events = super::super::audio_events::Events::default();
        let source = events.register_object(1)?;
        let other = events.register_object(2)?;
        runtime.define([Definition { id: 7, base: [-5., 0.] }]);
        runtime.enqueue(Command {
            node: 7, owner: Some(source.clone()), property: Property::Volume,
            mode: 1, value: -15., curve: 4, duration: 0, due: 4,
        });
        assert!(runtime.advance(&tables, 3)?.is_empty());
        let changes = runtime.advance(&tables, 4)?;
        assert_eq!(changes[0].owner, Some(source.handle));
        assert_eq!(runtime.initial(&[7], Some(source.handle)), [-10., 0.]);
        assert_eq!(runtime.initial(&[7], Some(other.handle)), [0.; 2]);
        events.unregister_object(1);
        let replacement = events.register_object(1)?;
        assert_eq!(runtime.initial(&[7], Some(replacement.handle)), [0.; 2]);
        drop(source);
        runtime.advance(&tables, 5)?;
        assert!(runtime.objects.is_empty());
        Ok(())
    }
    #[test]
    fn original_sis_targets_deltas_transition_requests_and_shared_vtables()->Result<()> {
        let data:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_sis.json"))?;
        for c in data["cases"].as_array().unwrap() {
            let base=c["base"].as_f64().unwrap() as f32;let current=c["current"].as_f64().unwrap() as f32;
            let result=target(base,current,c["mode"].as_u64().unwrap() as u32,c["value"].as_f64().unwrap() as f32);
            let transitions=c["transitions"].as_array().unwrap();
            if let Some(t)=transitions.first() {assert_eq!(result.to_bits(),t["end_bits"].as_u64().unwrap() as u32);assert_eq!(t["start_bits"].as_u64().unwrap() as u32,current.to_bits());assert_eq!(t["db"].as_u64().unwrap(),u64::from(c["property"]=="volume"));}
            else {assert_eq!(result.to_bits(),c["result_bits"].as_u64().unwrap() as u32);assert_eq!((result-current).to_bits(),c["notifications"][0].as_u64().unwrap() as u32);}
        }assert_eq!(data["vtables"].as_array().unwrap().len(),5);Ok(())
    }
    #[test]
    fn delayed_state_inheritance_relative_reset_and_linear_filter_transition()->Result<()> {
        let tables=Tables::retail()?;let mut r=Runtime::default();r.define([Definition{id:7,base:[-5.,10.]}]);
        r.enqueue(Command{node:7,owner:None,property:Property::Volume,mode:1,value:-15.,curve:4,duration:0,due:3});
        for tick in 0..3 {assert!(r.advance(&tables,tick)?.is_empty());}assert_eq!(r.initial(&[7],None),[0.;2]);
        let c=r.advance(&tables,3)?;assert_eq!(c[0].delta,-10.);assert_eq!(r.initial(&[1,7],None),[-10.,0.]);
        r.enqueue(Command{node:7,owner:None,property:Property::LowPass,mode:1,value:40.,curve:4,duration:30,due:4});
        r.advance(&tables,4)?;assert!((r.initial(&[7],None)[1]-3.).abs()<0.0001);
        for tick in 5..14 {r.advance(&tables,tick)?;}assert_eq!(r.initial(&[7],None),[-10.,30.]);
        r.enqueue(Command{node:7,owner:None,property:Property::Volume,mode:2,value:-2.,curve:4,duration:0,due:14});r.dispatch(&tables,14)?;assert_eq!(r.initial(&[7],None)[0],-12.);
        r.enqueue(Command{node:7,owner:None,property:Property::Volume,mode:0,value:99.,curve:4,duration:0,due:14});r.dispatch(&tables,14)?;assert_eq!(r.initial(&[7],None)[0],0.);assert!(!r.active());Ok(())
    }
}
