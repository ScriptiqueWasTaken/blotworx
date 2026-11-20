//! PredictMove services over canonical parent/action/brain/entity data. No raw
//! parent/Action mirrors are required; updates stay in their owning records.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_predict_move::{self,State,Clock,Request},ai_pathfinder_constructor::State as Pathfinder,
 ai_navigation_construction::Constructed,ai_navigation_entity::Owned,ai_navigation_action::Registry};
pub trait Backend{
 fn clock(&mut self,state:&mut State,parents:&mut BTreeMap<usize,Pathfinder>,field:Clock)->Result<f32>;
 fn unit_scale(&mut self,state:&mut State,parents:&mut BTreeMap<usize,Pathfinder>)->Result<Option<f32>>;
 fn zero(&mut self,state:&mut State)->Result<[f32;3]>;
}
pub struct Binding<'a,B>{pub parents:&'a mut BTreeMap<usize,Pathfinder>,pub brains:&'a BTreeMap<usize,Constructed>,pub entities:&'a BTreeMap<usize,Owned>,pub actions:&'a Registry,pub backend:&'a mut B}
impl<B> Binding<'_,B>{
 fn parent(&self,id:usize)->Result<&Pathfinder>{let p=self.parents.get(&id).context("PredictMove parent is absent")?;ensure!(p.identity==id,"PredictMove parent identity differs");Ok(p)}
 fn request(&self,parent:usize,index:i32)->Result<Option<Request>>{
  if index<0{return Ok(None)}
  Ok(self.parent(parent)?.base.action.attributes.get(index as usize).and_then(Option::as_ref).map(|s|Request{changed:s.changed,value:f32::from_bits(s.value_bits)}))
 }
}
impl<B:Backend> ai_predict_move::Host for Binding<'_,B>{
 fn clock(&mut self,s:&mut State,c:Clock)->Result<f32>{self.backend.clock(s,self.parents,c)}
 fn unit_scale(&mut self,s:&mut State)->Result<Option<f32>>{self.backend.unit_scale(s,self.parents)}
 fn zero(&mut self,s:&mut State)->Result<[f32;3]>{self.backend.zero(s)}
 fn enabled(&mut self,_:&mut State,p:usize)->Result<u8>{self.parent(p)?.reset_fields.flags_369_36a_3c8_400_401_368_74[2].context("PredictMove movement flag3c8 is unwritten")}
 fn target(&mut self,_:&mut State,p:usize)->Result<[f32;3]>{Ok(self.parent(p)?.points_200_274_2c4_314_378[4].position)}
 fn entity(&mut self,_:&mut State,p:usize)->Result<usize>{super::ai_navigation_owner_queries::entity(self.parent(p)?,self.brains)}
 fn position(&mut self,_:&mut State,e:usize)->Result<[f32;3]>{let owned=self.entities.get(&e).context("PredictMove entity is absent")?;ensure!(owned.entity.identity==e,"PredictMove entity identity differs");Ok(owned.entity.values_34_58[..3].try_into()?)}
 fn speed(&mut self,_:&mut State,p:usize)->Result<Option<Request>>{self.request(p,self.actions.speed.index)}
 fn heading(&mut self,_:&mut State,p:usize)->Result<Option<Request>>{self.request(p,self.actions.heading.index)}
 fn default_speed(&mut self,_:&mut State,p:usize)->Result<f32>{self.parent(p)?.base.scales_14_18_1c[0].context("PredictMove default speed14 is unwritten")}
}
#[cfg(test)]
mod tests {
 use super::*;use serde_json::{Value,json};use super::super::{ai_navigation_entity as entity,ai_navigation_action::{Scalar,Kind,Class},ai_navigation_construction::{ActionClass,ActionFactory,Layout}};
 struct Shape;
 impl entity::Host for Shape{
  fn radius(&mut self,_:&mut Owned)->Result<f32>{Ok(1.)}
  fn height(&mut self,_:&mut Owned)->Result<f32>{Ok(2.)}
  fn maximum_speed(&mut self,_:&mut Owned)->Result<f32>{Ok(9.)}
  fn unit_scale(&mut self,_:&mut Owned)->Result<Option<f32>>{Ok(Some(1.))}
 }
 struct ClockHost<'a>{input:&'a Value,clock:[f32;3],trace:Vec<Value>}
 impl Backend for ClockHost<'_>{
  fn clock(&mut self,s:&mut State,_:&mut BTreeMap<usize,Pathfinder>,c:Clock)->Result<f32>{let (i,o)=match c{Clock::Stamp58=>(0,0x58),Clock::Delta5c=>(1,0x5c),Clock::Stamp50=>(2,0x50)};self.trace.push(json!(["clock",o]));if self.input["mutation"]=="clock_parent" && i==0{s.pathfinder=2;}Ok(self.clock[i])}
  fn unit_scale(&mut self,_:&mut State,_:&mut BTreeMap<usize,Pathfinder>)->Result<Option<f32>>{self.trace.push(json!(["scale"]));Ok(self.input["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
  fn zero(&mut self,_:&mut State)->Result<[f32;3]>{Ok(std::array::from_fn(|i|f32::from_bits(self.input["zero"][i].as_u64().unwrap() as u32)))}
 }
 fn registry()->Registry{Registry{count:2,heading:Class{index:0,constructor:true},speed:Class{index:1,constructor:true}}}
 #[test]
 fn canonical_prediction_actions_entity_positions_and_frame_handoff_match_original()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_predict_move.json"))?;let mut count=0;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let input=&case["input"];if input["op"]=="init" || input["op"]=="reset" || input["mutation"]=="speed_target" || input["mutation"]=="heading_entity"{continue}
   let mut detector=super::super::ai_predict_move::tests::from_fields(case);let mut parents=BTreeMap::new();
   for id in [1,2]{let mut p=super::super::ai_pathfinder_modifiers::tests::state()?;p.identity=id;p.base.owner=20;
    p.reset_fields.flags_369_36a_3c8_400_401_368_74[2]=Some(input["enabled"].as_u64().unwrap() as u8);
    p.points_200_274_2c4_314_378[4].position=std::array::from_fn(|i|f32::from_bits(input["target"][i].as_u64().unwrap() as u32));
    p.base.scales_14_18_1c[0]=Some(f32::from_bits(input["default_speed"].as_u64().unwrap() as u32));
    for (i,key,kind) in [(0,"heading",Kind::Heading),(1,"speed",Kind::Speed)]{p.base.action.attributes[i]=input[key].as_array().map(|v|Scalar{kind,secondary_bits:None,changed:v[0].as_u64().unwrap() as u8,value_bits:v[1].as_u64().unwrap() as u32});}parents.insert(id,p);
   }
   let brain=Constructed::new(20,Some(42),Some(30),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},&Layout{agent_classes:0,service_classes:1,attributes:registry()})?;
   let brains=BTreeMap::from([(20,brain)]);let mut entity=Owned::new(42,None,[0;2],&entity::Registry{count:3,diameter:entity::Class{index:0,constructor:Some(entity::Kind::Diameter)},height:entity::Class{index:1,constructor:Some(entity::Kind::Height)},maximum_speed:entity::Class{index:2,constructor:Some(entity::Kind::MaximumSpeed)}},&mut Shape)?;
   for i in 0..3{entity.entity.values_34_58[i]=f32::from_bits(input["position"][i].as_u64().unwrap() as u32);}let mut entities=BTreeMap::from([(42,entity)]);let actions=registry();
   let mut clock=ClockHost{input,clock:std::array::from_fn(|i|f32::from_bits(input["clock"][i].as_u64().unwrap() as u32)),trace:vec![]};
   let result={let mut host=Binding{parents:&mut parents,brains:&brains,entities:&entities,actions:&actions,backend:&mut clock};match input["op"].as_str().unwrap(){"detect"=>json!(detector.detect(&mut host)?),"record"|"sequence"=>{detector.record(&mut host)?;Value::Null},_=>unreachable!()}};
   let result=if input["op"]=="sequence"{
    if input["follow"]==true{entities.get_mut(&42).unwrap().entity.values_34_58[..3].copy_from_slice(&detector.predicted);}
    clock.clock[0]=detector.stamp;clock.clock[2]=0.2;
    json!(detector.detect(&mut Binding{parents:&mut parents,brains:&brains,entities:&entities,actions:&actions,backend:&mut clock})?)
   }else{result};
   assert_eq!(json!(super::super::ai_predict_move::tests::fields(&detector)),case["fields"],"case {index}: {input}");assert_eq!(result,case["result"],"case {index}");assert_eq!(json!(detector.pathfinder),case["parent"]);
   let trace:Vec<Value>=case["trace"].as_array().unwrap().iter().filter(|v|v[0]=="clock" || v[0]=="scale").cloned().collect();assert_eq!(clock.trace,trace,"case {index}");count+=1;
  }assert!(count>400);Ok(())
 }
}
