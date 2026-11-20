//!Gameplay side of18f6d4,1868c8/186950 and186c80. Read the owned entity
//!user pair and actual character properties; no copied controller/shape cache.
use anyhow::{Context,Result,ensure};
use super::{assets::World,ai_navigation_entity::Owned,ai_navigation_brain_base::State,
 ai_navigation_region_cache::{Cache,Queries}};
use epic_mickey_lib_rs::scene_file::Property;
pub trait Runtime:super::ai_lpf_context::Host+super::ai_navigation_refresh::Sources {
 fn refresh_metadata(&self)->super::ai_navigation_refresh::Metadata;
}
pub fn controller(identity:usize,brain:usize,base:&State,entity:&super::ai_navigation_entity_base::Entity)->Result<usize>{
 ensure!(identity==brain,"Region query references a different brain");
 ensure!(base.owner==Some(entity.identity),"Region query entity ownership differs");
 Ok(entity.user_data[0])
}
fn selector(properties:&[Property],name:&str)->Result<u32>{
 let mut values=properties.iter().filter(|p|p.name==name);let value=values.next();ensure!(values.next().is_none(),"Ambiguous region {name}");
 //Both native getters initialize result to1 and ignore the virtual return.
 let value=value.map(|p|p.value.as_i64().with_context(||format!("Invalid region {name}"))).transpose()?.unwrap_or(1);
 Ok(i32::try_from(value).with_context(||format!("Region {name} exceeds native signed word"))? as u32)
}
fn key(world:&World,owner:usize,name:&str)->Result<u32>{
 let actor=world.entities.get(owner).context("Region controller owner absent")?;
 let mut components=actor.components.iter().filter(|c|c.class_name=="PhysicsRigidBodyCharacterComponent");
 let c=components.next();ensure!(components.next().is_none(),"Region query requires character slot arbitration");
 //Entity-level native property lookup keeps its initialized result on absence.
 match c{Some(c)=>selector(&c.properties,name),None=>Ok(1)}
}
fn extent(profile:u32,settings:&std::collections::BTreeMap<String,String>)->Result<f32>{
 let radius=super::motion_ground::radius(profile as i32 as i64,settings)?;
 Ok(radius+0.01f32)
}
pub struct Binding<'a,H>{pub world:&'a World,pub entities:Option<&'a std::collections::BTreeMap<usize,Owned>>,pub brain:usize,pub base:&'a State,pub entity:&'a Owned,pub backend:&'a mut H}
impl<H:Runtime> Queries for Binding<'_,H>{
 fn controller(&mut self,_:&mut Cache,b:usize)->Result<usize>{controller(b,self.brain,self.base,&self.entity.entity)}
 fn primary_key(&mut self,_:&mut Cache,c:usize)->Result<u32>{key(self.world,c,"Entity Radius")}
 fn secondary_key(&mut self,_:&mut Cache,c:usize)->Result<u32>{key(self.world,c,"Entity Height")}
 fn extent(&mut self,_:&mut Cache,_:usize,p:u32)->Result<f32>{extent(p,&self.world.runtime_settings)}
}
impl<H:Runtime> super::ai_lpf_context::Refresh for Binding<'_,H>{
 fn refresh_context(&mut self,m:&mut super::ai_graph_link_passes::Memory,_:&mut Cache,b:usize)->Result<()>{ensure!(b==self.brain,"Refresh brain identity differs");let entities=self.entities.context("World refresh requires canonical navigation entities")?;super::ai_navigation_refresh::refresh(m,entities,self.backend.refresh_metadata(),self.backend)}
}
impl<H:Runtime> super::ai_lpf_context::Host for Binding<'_,H>{
 fn child_reset(&mut self,m:&mut super::ai_graph_link_passes::Memory,p:u32,c:u32)->Result<()>{self.backend.child_reset(m,p,c)}
 fn group_clear(&mut self,m:&mut super::ai_graph_link_passes::Memory,g:u32)->Result<()>{self.backend.group_clear(m,g)}
}
#[cfg(test)]
mod tests {
 use super::*;use serde_json::{Value,json};
 #[test]
 fn original_controller_selector_and_extent_queries_match()->Result<()> {
  let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_region_world.json"))?;
  for row in capture["cases"].as_array().unwrap(){let c=&row["input"];let result=match c["op"].as_str().unwrap(){
   "controller"=>{let user=c["controller"].as_u64().unwrap() as usize;let e=super::super::ai_navigation_entity_base::Entity::new(73,None,[user,17],0)?;let b=State{owner:Some(73),action:None,services:vec![],agents:vec![]};let actual=controller(99,99,&b,&e)?;assert!(controller(98,99,&b,&e).is_err());json!(actual)},
   "radius"|"height"=>{let name=if c["op"]=="radius"{"Entity Radius"}else{"Entity Height"};let p=if c["value"].is_null(){vec![]}else{vec![Property::new("int".into(),name.into(),false,false,false,c["value"].clone())]};json!(selector(&p,name)?)},
   "extent"=>{let mut settings=std::collections::BTreeMap::new();for (i,name) in ["ThinEntityRadius","MediumEntityRadius","WideEntityRadius","PlayerEntityRadius"].iter().enumerate(){settings.insert(name.to_string(),c["radii"][i].as_f64().unwrap().to_string());}json!(extent(c["profile"].as_u64().unwrap() as u32,&settings)?.to_bits())},_=>unreachable!()};
   assert_eq!(result,row["result"],"{c}");
  }eprintln!("{} original controller/key/extent cases",capture["cases"].as_array().unwrap().len());Ok(())
 }
 struct Refresh{count:u32}
 impl super::super::ai_lpf_context::Host for Refresh{
  fn child_reset(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:u32,_:u32)->Result<()>{anyhow::bail!("No child in empty source context fixture")}
  fn group_clear(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:u32)->Result<()>{anyhow::bail!("No group in empty source context fixture")}
 }
 impl Runtime for Refresh{fn refresh_metadata(&self)->super::super::ai_navigation_refresh::Metadata{super::super::ai_navigation_refresh::Metadata{outline:super::super::ai_navigation_outline::Class{identity:0x3613c,index:3,has_factory:true},final_class:0x11368}}}
 impl super::super::ai_navigation_refresh::Sources for Refresh {
  fn collect(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,p:super::super::ai_navigation_refresh::Pass)->Result<Vec<u32>>{if p==super::super::ai_navigation_refresh::Pass::Actors{self.count+=1;}Ok(vec![])}
  fn resolve(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:u32)->Result<Option<u32>>{anyhow::bail!("No actor in empty refresh source fixture")}
  fn component(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:u32,_:super::super::ai_navigation_refresh::Pass)->Result<Option<u32>>{anyhow::bail!("No component in empty refresh source fixture")}
  fn graph_service(&mut self,_:&mut super::super::ai_graph_link_passes::Memory)->Result<Option<u32>>{Ok(None)}
  fn object_class(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:u32)->Result<usize>{anyhow::bail!("No object in empty refresh source fixture")}
  fn parent_class(&mut self,_:&mut super::super::ai_graph_link_passes::Memory,_:usize)->Result<Option<usize>>{anyhow::bail!("No class in empty refresh source fixture")}
 }
 #[test]
 #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
 fn original_gus_region_queries_acquire_and_release_canonical_context()->Result<()> {
  let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
  let world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
  let owner=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
  use super::super::ai_navigation_entity::{Registry,Class,Kind};
  let registry=Registry{count:3,diameter:Class{index:0,constructor:Some(Kind::Diameter)},height:Class{index:1,constructor:Some(Kind::Height)},maximum_speed:Class{index:2,constructor:Some(Kind::MaximumSpeed)}};
  let entity=world.construct_navigation_entity(73,None,[owner,0],&registry,None)?;
  let entities=std::collections::BTreeMap::from([(73,entity)]);let entity=&entities[&73];
  let base=State{owner:Some(73),action:None,services:vec![],agents:vec![]};
  let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_lpf_context.json"))?;
  let row=capture["cases"].as_array().unwrap().iter().find(|r|r["input"]["op"]=="reset"&&r["input"]["count"]==0&&r["input"]["groups"]==0&&r["input"]["child"]==0).unwrap();
  let mut memory=super::super::ai_graph_link_passes::Memory::default();
  for a in row["initial"].as_array().unwrap(){let hex=a[1].as_str().unwrap();let bytes=hex.as_bytes().chunks_exact(2).map(|b|u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap()).collect();memory.regions.insert(a[0].as_u64().unwrap() as u32,bytes);}
  memory.regions.insert(0x71000000,vec![0;0x14]);memory.write(0x71000004,0x72000000)?;memory.write(0x71000010,1)?;
  memory.regions.insert(0x70000000,b"LpfContext1\0".to_vec());memory.write(0x72000090,0x70000000)?;
  let mut cache=Cache{entries:[super::super::ai_navigation_region_cache::Entry{key:[99,98],references:0},super::super::ai_navigation_region_cache::Entry{key:[97,96],references:0}]};
  let mut refresh=Refresh{count:0};
  {let mut queries=Binding{world:&world,entities:Some(&entities),brain:99,base:&base,entity,backend:&mut refresh};
   assert_eq!(queries.controller(&mut cache,99)?,owner);assert_eq!(queries.primary_key(&mut cache,owner)?,0);assert_eq!(queries.secondary_key(&mut cache,owner)?,0);
   {let mut native=super::super::ai_lpf_context::Native{memory:&mut memory,backend:&mut queries};assert_eq!(cache.register(99,0x71000000,&mut native)?,Some(0x72000000));assert_eq!(cache.register(99,0x71000000,&mut native)?,Some(0x72000000));}
   assert_eq!(cache.entries[0].references,2);cache.release(99,&mut queries)?;cache.release(99,&mut queries)?;assert_eq!(cache.entries[0].references,0);
  }
  assert_eq!(refresh.count,1);assert_eq!(memory.word(0x7200005c)?,(entity.entity.radius+0.01f32).to_bits());assert_eq!(memory.word(0x72000060)?,1.8f32.to_bits());Ok(())
 }
}
