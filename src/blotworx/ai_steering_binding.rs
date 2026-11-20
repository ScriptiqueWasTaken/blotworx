//! Scoped services over the actual pathfinder action and entity records.
//! Native metadata supplies slots; no fixed global registration order is used.
use anyhow::{Context,Result,ensure};use std::collections::BTreeMap;
use super::{ai_steering::{self,State,Request},ai_navigation_action::Action,ai_navigation_entity::Owned,ai_graph_link_passes::Memory};
pub trait Backend{
 /// Actual optional CEntityTorsoOrientation36164 payload. Missing is distinct from
 /// a present attribute whose provider is not implemented (which must error).
 fn torso_orientation(&mut self,s:&mut State,entities:&BTreeMap<usize,Owned>,entity:usize)->Result<Option<[f32;3]>>;
 fn zero(&mut self,s:&mut State)->Result<[f32;3]>;
}
pub struct Binding<'a,B>{pub parent_identity:usize,pub parent_target:&'a [f32;3],pub action_identity:usize,pub action:&'a mut Action,pub slots:[i32;5],pub entities:&'a BTreeMap<usize,Owned>,pub memory:&'a Memory,pub backend:&'a mut B}
impl<B> Binding<'_,B>{fn entity(&self,id:usize)->Result<&Owned>{let e=self.entities.get(&id).context("Steering entity is absent")?;ensure!(e.entity.identity==id,"Steering entity identity differs");Ok(e)}}
impl<B:Backend> ai_steering::Host for Binding<'_,B>{
 fn position(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{Ok(self.entity(e)?.entity.values_34_58[..3].try_into()?)}
 fn point(&mut self,_:&mut State,p:usize)->Result<[f32;3]>{super::ai_refine_goal::position(self.memory,u32::try_from(p)?)}
 fn heading(&mut self,_:&mut State,e:usize)->Result<f32>{Ok(self.entity(e)?.entity.values_34_58[4])}
 fn request(&mut self,_:&mut State,action:usize,kind:Request)->Result<Option<usize>>{ensure!(action==self.action_identity,"Steering action identity differs");let slot=self.slots[kind.index()];if slot<0{return Ok(None)}Ok(self.action.attributes.get(slot as usize).and_then(Option::as_ref).map(|_|slot as usize))}
 fn torso_orientation(&mut self,s:&mut State,e:usize)->Result<Option<[f32;3]>>{self.backend.torso_orientation(s,self.entities,e)}
 fn forward(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{Ok(self.entity(e)?.entity.values_34_58[6..9].try_into()?)}
 fn target(&mut self,_:&mut State,p:usize)->Result<[f32;3]>{ensure!(p==self.parent_identity,"Steering requires its current borrowed parent");Ok(*self.parent_target)}
 fn current_speed(&mut self,_:&mut State,e:usize)->Result<f32>{Ok(self.entity(e)?.entity.values_34_58[9])}
 fn zero(&mut self,s:&mut State)->Result<[f32;3]>{self.backend.zero(s)}
 fn write(&mut self,_:&mut State,r:usize,offset:u32,bits:u32)->Result<()>{let a=self.action.attributes.get_mut(r).and_then(Option::as_mut).context("Retained steering action slot disappeared")?;match offset{8=>a.value_bits=bits,12=>{*a.secondary_bits.as_mut().context("Retained steering request has no second value")?=bits},_=>anyhow::bail!("Unsupported native steering request offset")};Ok(())}
 fn changed(&mut self,_:&mut State,r:usize,value:u8)->Result<()>{self.action.attributes.get_mut(r).and_then(Option::as_mut).context("Retained steering change slot disappeared")?.changed=value;Ok(())}
}
#[cfg(test)]mod tests{
 use super::*;use super::super::{ai_navigation_action::{Kind,Scalar,Registry,Class},ai_navigation_entity as entity};use serde_json::{Value,json};
 struct Shape;impl entity::Host for Shape{fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(1.)}fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(2.)}fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(1.)}fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(None)}}
 struct Services<'a>{case:&'a Value}impl Backend for Services<'_>{fn torso_orientation(&mut self,_:&mut State,_:&BTreeMap<usize,Owned>,_:usize)->Result<Option<[f32;3]>>{Ok(self.case["direction"].as_array().map(|v|std::array::from_fn(|i|f32::from_bits(v[i].as_u64().unwrap() as u32))))}fn zero(&mut self,_:&mut State)->Result<[f32;3]>{Ok([0.;3])}}
 #[test]fn simple_biped_writes_same_owned_action_slots_from_canonical_entities()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_steering.json"))?;
  for case in fixture["cases"].as_array().unwrap(){let c=&case["input"];if c["op"]!="query"{continue}let mut s=super::super::ai_steering::tests::state(c);let mut backend=Services{case:c};
   let registry=entity::Registry{count:3,diameter:entity::Class{index:0,constructor:Some(entity::Kind::Diameter)},height:entity::Class{index:1,constructor:Some(entity::Kind::Height)},maximum_speed:entity::Class{index:2,constructor:Some(entity::Kind::MaximumSpeed)}};
   let mut e=Owned::new(10,None,[0;2],&registry,&mut Shape)?;for i in 0..3{e.entity.values_34_58[i]=f32::from_bits(c["position"][i].as_u64().unwrap() as u32);e.entity.values_34_58[6+i]=f32::from_bits(c["forward"][i].as_u64().unwrap() as u32);}e.entity.values_34_58[4]=f32::from_bits(c["heading"].as_u64().unwrap() as u32);e.entity.values_34_58[9]=f32::from_bits(c["current_speed"].as_u64().unwrap() as u32);let entities=BTreeMap::from([(10,e)]);
   // Vary physical slot order: neither metadata indices nor retained handles
   // are interchangeable with the request enumeration's portable test labels.
   let slots=[4,1,3,0,2];let mut action=Action::new(&Registry{count:5,heading:Class{index:4,constructor:true},speed:Class{index:1,constructor:true}})?;
   for (i,kind) in [Kind::Heading,Kind::Speed,Kind::Steering,Kind::Acceleration,Kind::TorsoRotate].into_iter().enumerate(){action.attributes[slots[i] as usize]=c["present"][i].as_bool().unwrap().then_some(Scalar{kind,changed:2,value_bits:0x40400000,secondary_bits:(i==4).then_some(0x40800000)});}
   let mut memory=Memory::default();memory.regions.insert(0x74000000,vec![0;0x50]);for i in 0..3{memory.write(0x74000004+i*4,c["target"][i as usize].as_u64().unwrap() as u32)?;}
   let parent=std::array::from_fn(|i|f32::from_bits(c["parent_target"][i].as_u64().unwrap() as u32));let mut binding=Binding{parent_identity:2,parent_target:&parent,action_identity:30,action:&mut action,slots,entities:&entities,memory:&memory,backend:&mut backend};
   let result=s.query(10,0x74000000,f32::from_bits(c["speed"].as_u64().unwrap() as u32),30,&mut binding)?;assert_eq!(result,case["result"].as_u64().unwrap() as u32,"{c}");assert_eq!(s.strafe,case["flag"].as_u64().unwrap() as u8,"{c}");
   let values=(0..5).map(|i|{let a=action.attributes[slots[i] as usize].as_ref();a.map_or([2,0x40400000,0x40800000],|a|[a.changed as u32,a.value_bits,a.secondary_bits.unwrap_or(0x40800000)])}).collect::<Vec<_>>();assert_eq!(json!(values),case["requests"],"{c}");
  }Ok(())
 }
}
