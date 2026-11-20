//! CanGo499abc -> inherited1c -> concrete spatial24 over canonical Rust owners.
//! Only geometry/derived virtual operations and shape services remain boundaries.
use std::collections::BTreeMap;
use anyhow::{Context as _,Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_pathfinder_constructor::State,
 ai_navigation_construction::Constructed,ai_spatial_walk::Filter,ai_spatial_acceptance};
pub type Brains=BTreeMap<usize,Constructed>;
pub trait Backend {
 fn entity_diameter(&mut self,state:&mut State,m:&mut Memory)->Result<Option<f32>>;
 fn unit_scale(&mut self,state:&mut State,m:&mut Memory)->Result<Option<f32>>;
 fn derived1c(&mut self,state:&mut State,brains:&mut Brains,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32)->Result<u32>;
 fn derived24(&mut self,state:&mut State,brains:&mut Brains,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>)->Result<u32>;
 /// A distinct mapped frame per invocation, including recursive recovery.
 fn enter_frame(&mut self,m:&mut Memory)->Result<u32>;
 fn leave_frame(&mut self,m:&mut Memory,frame:u32)->Result<()>;
 fn sweep20(&mut self,state:&mut State,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32,output:u32,normal:u32,info:u32)->Result<u32>;
 fn accept1c(&mut self,state:&mut State,m:&mut Memory,interface:u32,cell:u32,start:u32,end:u32,radius:u32)->Result<u32>;
 fn recovery(&mut self,state:&mut State,brains:&mut Brains,m:&mut Memory,callback:u32,start:u32,recovered:u32,mode:u32,entity:u32,radius:u32)->Result<u32>;
}
#[derive(Clone,Copy)]
pub struct Context {pub acceptance:ai_spatial_acceptance::Context,pub inherited1c:u32,pub base24:u32}
pub struct Binding<'a,B>{pub context:Context,pub brains:&'a mut Brains,pub backend:&'a mut B}
fn validate(m:&Memory,service:u32,state:&State)->Result<()> {
 ensure!(usize::try_from(m.word(service.wrapping_add(4))?)?==state.identity,"Spatial service has a different canonical pathfinder owner");Ok(())
}
///4962ac over the actual owned LPF module, not a second raw module image.
pub fn owner_filters(state:&State,m:&Memory,owner:u32)->Result<u32>{
 ensure!(usize::try_from(owner)?==state.identity,"Spatial filters have a different canonical owner");
 let Some(identity)=state.reference_f8.context("Pathfinder LPF reference is unwritten")?.filter(|&p|p!=0) else{return Ok(0)};
 let module=state.lpf.as_ref().context("Pathfinder LPF reference has no canonical module")?;
 ensure!(module.identity==identity && module.pathfinder==Some(state.identity),"Spatial LPF identity or owner differs");
 match module.context.filter(|&p|p!=0){None=>Ok(0),Some(context)=>m.word(u32::try_from(context)?.wrapping_add(8))}
}
struct Query<'a,B>{state:&'a mut State,brains:&'a mut Brains,backend:&'a mut B,context:Context}
impl<B:Backend> super::ai_navigation_sweep::InterfaceHost for Query<'_,B>{
 fn sweep20(&mut self,m:&mut Memory,i:u32,c:u32,s:u32,e:u32,r:u32,o:u32,n:u32,info:u32)->Result<u32>{self.backend.sweep20(self.state,m,i,c,s,e,r,o,n,info)}
}
impl<B:Backend> super::ai_cell_trace::InterfaceHost for Query<'_,B>{
 fn accept1c(&mut self,m:&mut Memory,i:u32,c:u32,s:u32,e:u32,r:u32)->Result<u32>{self.backend.accept1c(self.state,m,i,c,s,e,r)}
}
impl<B:Backend> ai_spatial_acceptance::Host for Query<'_,B>{
 fn select_filters(&mut self,m:&mut Memory,service:u32,output:u32,mode:u32)->Result<()>{
  super::ai_spatial_service::select_filters_with(m,service,output,mode,|m,owner|owner_filters(self.state,m,owner))
 }
 fn owner_provider(&mut self,m:&mut Memory,service:u32)->Result<u32>{
  validate(m,service,self.state)?;
  let refs=&self.state.references_3c_4c_50_54_58_60_6c;
  let record=refs[3].filter(|&p|p!=0).or(refs[2]);
  match record.filter(|&p|p!=0){None=>Ok(0),Some(p)=>m.word(u32::try_from(p)?.wrapping_add(0x4c))}
 }
 fn owner_entity(&mut self,m:&mut Memory,service:u32)->Result<u32>{
  validate(m,service,self.state)?;
  let brain=self.brains.get(&self.state.base.owner).context("Spatial recovery has no canonical owning brain")?;
  ensure!(brain.brain.identity==self.state.base.owner,"Spatial brain registry identity differs");
  Ok(u32::try_from(brain.base.owner.unwrap_or(0))?)
 }
 fn recovery(&mut self,m:&mut Memory,c:u32,s:u32,r:u32,mode:u32,e:u32,radius:u32)->Result<u32>{self.backend.recovery(self.state,self.brains,m,c,s,r,mode,e,radius)}
 fn query24(&mut self,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>)->Result<u32>{
  dispatch24(self.state,self.brains,self.backend,self.context,m,service,start,end,radius,filter)
 }
}
pub fn dispatch24(state:&mut State,brains:&mut Brains,backend:&mut impl Backend,context:Context,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,filter:&mut Option<Filter>)->Result<u32>{
 validate(m,service,state)?;
 let vtable=m.word(service)?;
 if m.word(vtable.wrapping_add(0x24))?!=context.base24{return backend.derived24(state,brains,m,service,start,end,radius,filter)}
 let frame=backend.enter_frame(m)?;let mut c=context.acceptance;c.spatial.scratch=frame;
 let result=ai_spatial_acceptance::query24(m,c,service,start,end,radius,filter,&mut Query{state,brains,backend,context});
 let cleanup=backend.leave_frame(m,frame);match result{Err(e)=>Err(e),Ok(value)=>{cleanup?;Ok(value)}}
}
/// Concrete1c5160 wrapper, including canonical LPF filters and virtual24.
/// Caller dispatch must identify this base34 implementation before calling it.
pub fn base34(state:&mut State,brains:&mut Brains,backend:&mut impl Backend,context:Context,m:&mut Memory,service:u32,start:u32,end:u32,radius:u32,mode:u32)->Result<u32>{
 validate(m,service,state)?;let frame=backend.enter_frame(m)?;
 let result=ai_spatial_acceptance::query34(m,frame,service,start,end,radius,mode,&mut Query{state,brains,backend,context});
 let cleanup=backend.leave_frame(m,frame);match result{Err(e)=>Err(e),Ok(value)=>{cleanup?;Ok(value)}}
}
impl<B:Backend> super::ai_pathfinder_can_go::Host for Binding<'_,B>{
 fn entity_diameter(&mut self,state:&mut State,m:&mut Memory)->Result<Option<f32>>{self.backend.entity_diameter(state,m)}
 fn unit_scale(&mut self,state:&mut State,m:&mut Memory)->Result<Option<f32>>{self.backend.unit_scale(state,m)}
 fn query_1c(&mut self,state:&mut State,m:&mut Memory,selected:usize,start:u32,end:u32,radius:f32)->Result<u32>{
  let service=u32::try_from(selected)?;validate(m,service,state)?;let vtable=m.word(service)?;
  if m.word(vtable.wrapping_add(0x1c))?!=self.context.inherited1c{return self.backend.derived1c(state,self.brains,m,service,start,end,radius.to_bits())}
  let frame=self.backend.enter_frame(m)?;
  let result=super::ai_spatial_modifier::forward1c(m,frame,service,start,end,radius.to_bits(),|m,service,start,end,radius,filter|
   dispatch24(state,self.brains,self.backend,self.context,m,service,start,end,radius,filter));
  let cleanup=self.backend.leave_frame(m,frame);match result{Err(e)=>Err(e),Ok(value)=>{cleanup?;Ok(value)}}
 }
}
#[cfg(test)]
pub(in crate::blotworx) mod tests {
 use super::*;use serde_json::{Value,json};
 use super::super::{ai_navigation_action::{Registry,Class},ai_navigation_construction::{Layout,ActionClass,ActionFactory}};
 fn object(m:&Memory,p:u32)->Result<Value>{
  let mut attributes=Vec::new();for i in 0..16{let a=m.word(p+0x10+i*4)?;attributes.push(if a==0{Value::Null}else{json!({"kind":m.word(a)?,"words":(0..7).map(|j|m.word(a+4+j*4)).collect::<Result<Vec<_>>>()?})});}
  Ok(json!({"kind":m.word(p)?,"position":(0..3).map(|i|m.word(p+4+i*4)).collect::<Result<Vec<_>>>()?,"attributes":attributes}))
 }
 struct Services<'a>{input:&'a Value,next:u32,frames:Vec<u32>,trace:Vec<Value>,shape_reads:usize,derived:Vec<u32>}
 impl Backend for Services<'_>{
  fn entity_diameter(&mut self,_:&mut State,_:&mut Memory)->Result<Option<f32>>{self.shape_reads+=1;Ok(Some(0.8))}
  fn unit_scale(&mut self,_:&mut State,_:&mut Memory)->Result<Option<f32>>{anyhow::bail!("Unexpected fallback diameter")}
  fn derived1c(&mut self,state:&mut State,_:&mut Brains,_:&mut Memory,_:u32,_:u32,_:u32,_:u32)->Result<u32>{state.word_30=123;self.derived.push(0x1c);Ok(0x12345678)}
  fn derived24(&mut self,state:&mut State,_:&mut Brains,m:&mut Memory,_:u32,_:u32,_:u32,_:u32,filter:&mut Option<Filter>)->Result<u32>{state.word_30=456;assert_eq!(filter.as_ref().map(|f|f.word(m)).transpose()?,Some(44));self.derived.push(0x24);Ok(0x80000101)}
  fn enter_frame(&mut self,m:&mut Memory)->Result<u32>{let p=self.next;self.next=self.next.checked_add(0x1000).context("Fixture frame domain exhausted")?;ensure!(m.regions.insert(p,vec![0;0x800]).is_none(),"Fixture overlaps");self.frames.push(p);Ok(p)}
  fn leave_frame(&mut self,m:&mut Memory,p:u32)->Result<()>{assert_eq!(self.frames.pop(),Some(p));ensure!(m.regions.remove(&p).is_some(),"Frame missing");Ok(())}
  fn sweep20(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Base query must use real unfiltered sweep")}
  fn accept1c(&mut self,_:&mut State,_:&mut Memory,_:u32,_:u32,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("Base query unexpectedly used trace filters")}
  fn recovery(&mut self,state:&mut State,_:&mut Brains,m:&mut Memory,callback:u32,start:u32,recovered:u32,mode:u32,entity:u32,radius:u32)->Result<u32>{
   self.trace.push(json!({"kind":"recovery","callback":callback,"start":object(m,start)?,"recovered":object(m,recovered)?,"mode":mode,"context":entity,"radius":radius}));
   state.word_30=state.word_30.wrapping_add(1);
   if self.input["mutate"].as_bool().unwrap(){m.write(recovered+4,99f32.to_bits())?;}
   Ok(self.input["callback"].as_u64().unwrap() as u32)
  }
 }
 fn setup(case:&Value)->Result<(State,Brains,Memory,Context)>{
  let regions=[(0x62000000,0x1800),(0x63000000,0x100),(0x61000000,0x200),(0x5a108,4),(0x4a508,20),(0x564d0,0x40),(0x64000000,0x40),(0x65000000,0x800),(0x67000000,16),(0x68000000,64),(0x69000000,64),(0x6a000000,64),(0x6b000000,0x500),(0x6c000000,64),(0x6d000000,0x300),(0x6e000000,0x1100),(0x41d64,16),(0x6f000000,32),(0x4e14c,4),(0x66000000,0xa0),(0x74000000,0x100),(0x73000000,0x300),(0x361b4,16)];
  let mut m=Memory{regions:regions.into_iter().map(|(p,n)|(p,vec![0;n])).collect(),resources:Default::default(),stride:0,journal:Default::default()};
  for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
  let service=0x6e000000;let owner=m.word(service+4)?;let brain=m.word(owner+4)?;let entity=m.word(brain+0x14)?;
  let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;state.identity=owner as usize;state.base.owner=brain as usize;
  for (i,o) in [(2,0x50),(3,0x54)]{let p=m.word(owner+o)?;state.references_3c_4c_50_54_58_60_6c[i]=(p!=0).then_some(p as usize);}
  let registry=Registry{count:2,heading:Class{index:0,constructor:true},speed:Class{index:1,constructor:true}};
  let b=Constructed::new(brain as usize,Some(entity as usize),Some(30),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},&Layout{agent_classes:0,service_classes:1,attributes:registry})?;
  // Poison every old owner link. Success must come from the canonical typed
  // pathfinder/brain, not an accidentally retained arena snapshot.
  for o in [4,0x50,0x54,0xf8]{m.write(owner+o,0xdeadbeef)?;}m.regions.remove(&brain);
  let context=Context{acceptance:ai_spatial_acceptance::Context{spatial:super::super::ai_spatial_service::Context{globals:super::super::ai_edge_crossing::Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0},vectors:super::super::ai_cell_crossing::Vectors{up:0x564f4,zero:0x564d0},point_cache_metadata:0x41d64,scratch:0},point_kind:0x4ec1b0,cache_kind:0x4ec160,recovery_callback_slot:0x4e14c},inherited1c:0x2b7ac0,base24:0x1c48f8};
  Ok((state,BTreeMap::from([(brain as usize,b)]),m,context))
 }
 fn services(input:&Value)->Services<'_>{Services{input,next:0x70000000,frames:vec![],trace:vec![],shape_reads:0,derived:vec![]}}
 #[test]
 fn canonical_pathfinder_reaches_native_sweeps_and_recovery_without_raw_owner_mirrors()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_acceptance.json"))?;let mut count=0;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let input=&case["input"];if input["op"]!="base" || !input["child"].is_null(){continue}
   let (mut state,mut brains,mut m,context)=setup(case)?;let mut backend=services(input);
   let mut filter=(!input["filter"].is_null()).then_some(Filter::Address(0x64000000));
   let result=dispatch24(&mut state,&mut brains,&mut backend,context,&mut m,0x6e000000,0x6d000000,0x6d000200,input["radius"].as_u64().unwrap() as u32,&mut filter)?;
   let expected_trace:Vec<_>=case["trace"].as_array().unwrap().iter().filter(|v|v["kind"]=="recovery").cloned().collect();
   assert_eq!(backend.trace,expected_trace,"case {index}");assert_eq!(state.word_30,64+backend.trace.len() as u32);
   let actual=json!({"result":result,"start_after":object(&m,0x6d000000)?,"end_after":object(&m,0x6d000200)?,"cache":(0..7).map(|i|m.word(0x6d000104+i*4)).collect::<Result<Vec<_>>>()?,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
   assert!(backend.frames.is_empty());assert!(!m.regions.keys().any(|&p|p>=0x70000000 && p<0x73000000));count+=1;
  }assert!(count>=40,"Insufficient original scenarios replayed: {count}");Ok(())
 }
 #[test]
 fn canonical_selector34_reaches_actual_base_acceptance_and_keeps_nested_frames_live()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_acceptance.json"))?;let mut count=0;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){let input=&case["input"];if input["op"]!="wrapper" || !input["child"].is_null(){continue}
   let (mut state,mut brains,mut m,context)=setup(case)?;let mut backend=services(input);
   let result=base34(&mut state,&mut brains,&mut backend,context,&mut m,0x6e000000,0x6d000000,0x6d000200,input["radius"].as_u64().unwrap() as u32,input["mode"].as_u64().unwrap() as u32)?;
   let expected_trace:Vec<_>=case["trace"].as_array().unwrap().iter().filter(|v|v["kind"]=="recovery").cloned().collect();
   assert_eq!(backend.trace,expected_trace,"case {index}");assert_eq!(state.word_30,64+backend.trace.len() as u32);
   let actual=json!({"result":result,"start_after":object(&m,0x6d000000)?,"end_after":object(&m,0x6d000200)?,"cache":(0..7).map(|i|m.word(0x6d000104+i*4)).collect::<Result<Vec<_>>>()?,"counter":m.word(0x4a508)?,"radius_counter":m.word(0x4a50c)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
   assert!(backend.frames.is_empty());assert!(!m.regions.keys().any(|&p|p>=0x70000000 && p<0x73000000));count+=1;
  }assert_eq!(count,14,"Insufficient original scenarios replayed: {count}");Ok(())
 }
 #[test]
 fn can_go_dispatch_preserves_derived_methods_and_canonical_mutations()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_acceptance.json"))?;let case=&fixture["cases"][0];
  for inherited in [false,true]{let (mut state,mut brains,mut m,context)=setup(case)?;let mut backend=services(&case["input"]);let service=0x6e000000;
   let vt=m.word(service)?;m.write(vt+0x1c,if inherited{context.inherited1c}else{0x1111})?;m.write(vt+0x24,0x2222)?;
   state.tables_fc_1fc[super::super::ai_pathfinder_modifiers::Kind(3).table()].selected=Some(Some(service as usize));
   let result=super::super::ai_pathfinder_can_go::query(&mut state,&mut m,0x6d000000,0x6d000200,&mut Binding{context,brains:&mut brains,backend:&mut backend})?;
   assert_eq!((result,state.word_30),(if inherited{0x80000101}else{0x12345678},if inherited{456}else{123}));assert_eq!(backend.shape_reads,1);assert_eq!(backend.derived,[if inherited{0x24}else{0x1c}]);assert!(backend.frames.is_empty());
  }Ok(())
 }
 struct LpfConstructor;
 impl super::super::ai_lpf_module::Host for LpfConstructor {
  fn unit_scale(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<f32>>{Ok(Some(1.))}
  fn manager(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<usize>>{anyhow::bail!("Unexpected LPF manager during construction")}
  fn child_count(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<u32>{unreachable!()}
  fn child(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize,_:u32)->Result<Option<usize>>{unreachable!()}
  fn name(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<Option<Vec<u8>>>{unreachable!()}
  fn value(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<Option<usize>>{unreachable!()}
  fn integer(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<i32>{unreachable!()}
  fn number(&mut self,_:&mut super::super::ai_lpf_module::State,_:usize)->Result<f64>{unreachable!()}
  fn context(&mut self,_:&mut super::super::ai_lpf_module::State,_:Option<usize>,_:usize)->Result<Option<usize>>{unreachable!()}
 }
 pub(in crate::blotworx) fn attach_lpf(state:&mut State,identity:Option<usize>,context:Option<usize>)->Result<()> {
  state.reference_f8=Some(identity);state.lpf=identity.map(|id|super::super::ai_lpf_module::State::construct(id,state.identity,&mut LpfConstructor).map(Box::new)).transpose()?;
  if let Some(module)=&mut state.lpf{module.context=context;}Ok(())
 }
 #[test]
 fn canonical_lpf_filters_match_original_selector_and_native_pool_recycling()->Result<()> {
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_filters.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x60000000,vec![0;0x40]),(0x61000000,vec![0;0x100]),(0x62000000,vec![0;0x20]),(0x62100000,vec![0;0x200]),(0x63000000,vec![0;16]),(0x63100000,vec![0;0x100]),(0x64000000,vec![0;16]),(0x64100000,vec![0;0x100]),(0x65000000,vec![0;64]),(0x66000000,vec![0;4])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   let input=&case["input"];let stage=input["stage"].as_u64().unwrap();let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;state.identity=0x61000000;
   attach_lpf(&mut state,(stage>=1).then_some(0x65000000),(stage>=2).then_some(0x65000020))?;
   m.write(0x610000f8,0xdeadbeef)?;m.write(0x65000008,0xdeadbeef)?;
   super::super::ai_spatial_service::select_filters_with(&mut m,0x60000000,0x66000000,input["mode"].as_u64().unwrap() as u32,|m,owner|owner_filters(&state,m,owner))?;
   let count=input["active"].as_u64().unwrap()+input["free"].as_u64().unwrap();
   let actual=json!({"output":m.word(0x66000000)?,"pool":(0..6).map(|i|m.word(0x62000000+i*4)).collect::<Result<Vec<_>>>()?,"nodes":(0..count as u32).map(|i|(0..5).map(|j|m.word(0x62100000+i*32+j*4)).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }Ok(())
 }
 #[test]
 fn canonical_filter_lookup_reloads_context_and_rejects_missing_owned_module()->Result<()> {
  let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;
  let mut m=Memory{regions:[(1000,vec![0;32]),(2000,vec![0;32])].into(),resources:Default::default(),stride:0,journal:Default::default()};
  m.write(1008,123)?;m.write(2008,456)?;attach_lpf(&mut state,Some(70),Some(1000))?;
  assert_eq!(owner_filters(&state,&m,50)?,123);state.lpf.as_mut().unwrap().context=Some(2000);assert_eq!(owner_filters(&state,&m,50)?,456);
  state.lpf.as_mut().unwrap().context=None;assert_eq!(owner_filters(&state,&m,50)?,0);
  state.lpf=None;assert!(owner_filters(&state,&m,50).is_err());assert!(owner_filters(&state,&m,51).is_err());
  state.reference_f8=Some(None);assert_eq!(owner_filters(&state,&m,50)?,0);state.reference_f8=None;assert!(owner_filters(&state,&m,50).is_err());Ok(())
 }

}
