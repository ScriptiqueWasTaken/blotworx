//! Retail base spatial factory1c2e14/1c2fcc and pool initialization1c3a14.
//! Allocation, observers, configuration and world class dispatch are explicit.
use super::{ai_graph_link_passes::Memory,ai_traversal_exclusion::{self,Services},ai_spatial_service};
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
#[derive(Clone,Copy)]
pub struct Context{
 pub root_slot:u32,pub manager_class:u32,pub observer_slot:u32,
 pub service_kind:u32,pub parent_kind:u32,pub interface_kind:u32,pub pool_kind:u32,pub node_kind:u32,
 pub service_metadata:u32,pub registry_flag:u32,pub registry:u32,
}
#[derive(Clone,Copy)]
pub struct Allocation{pub address:u32,pub result:u32}
pub trait Host:Services{
 fn owner_filters(&mut self,m:&mut Memory,owner:u32)->Result<u32>{ai_spatial_service::owner_filters(m,owner)}
 fn allocate(&mut self,m:&mut Memory,bytes:u32)->Result<Allocation>;
 fn observe(&mut self,m:&mut Memory,observer:u32,address:u32,bytes:u32,result:u32)->Result<()>;
 ///49b274, including real child/property callbacks. Rejection is retained
 /// after allocations; this is not a preflight or a successful default.
 fn configure(&mut self,m:&mut Memory,service:u32,root:u32)->Result<u32>;
}
fn allocate(m:&mut Memory,c:Context,bytes:u32,host:&mut impl Host)->Result<u32>{
 let allocation=host.allocate(m,bytes)?;let observer=m.word(c.observer_slot)?;
 if observer!=0{host.observe(m,observer,allocation.address,bytes,allocation.result)?;}
 Ok(allocation.address)
}
/// Factory allocation/name ownership and once-only class registration.
/// Unmapped/null required allocations fail checked access rather than creating
/// a purportedly valid service. Native padding/unwritten bytes stay untouched.
pub fn create(m:&mut Memory,c:Context,owner:u32,name:u32,host:&mut impl Host)->Result<u32>{
 let service=construct_name(m,c,owner,|m,i|m.byte(name.wrapping_add(i)),host)?;
 register(m,c,service)?;Ok(service)
}
/// The source-name slice is the enclosing definition backend's captured string.
/// Allocate only the actual object and owned name; no temporary arena copy.
pub fn create_text(m:&mut Memory,c:Context,owner:u32,name:&[u8],register_class:bool,host:&mut impl Host)->Result<u32>{
 let service=construct_name(m,c,owner,|_,i|Ok(name.get(i as usize).copied().unwrap_or(0)),host)?;
 if register_class{register(m,c,service)?;}Ok(service)
}
fn construct_name(m:&mut Memory,c:Context,owner:u32,mut name_byte:impl FnMut(&mut Memory,u32)->Result<u8>,host:&mut impl Host)->Result<u32>{
 let service=allocate(m,c,0x24,host)?;
 if service!=0{
  m.write(service,c.interface_kind)?;m.write(service.wrapping_add(4),owner)?;
  let mut length=0u32;while name_byte(m,length)?!=0{length=length.wrapping_add(1);}let bytes=length.wrapping_add(1);let stored=allocate(m,c,bytes,host)?;
  m.write(service.wrapping_add(8),stored)?;
  let mut source=0u32;let mut destination=stored;
  loop{let byte=name_byte(m,source)?;m.write_byte(destination,byte)?;if byte==0{break}source=source.wrapping_add(1);destination=destination.wrapping_add(1);}
  m.write(service,c.parent_kind)?;m.write(service,c.service_kind)?;
  for offset in [12,16,20,24,28,32]{m.write(service.wrapping_add(offset),0)?;}
 }
 Ok(service)
}
fn register(m:&mut Memory,c:Context,service:u32)->Result<()>{
 let kind=m.word(service)?;
 if word(m,c.service_metadata,4)?==0{
  m.write(c.service_metadata.wrapping_add(4),kind)?;
  if m.byte(c.registry_flag)?==0{m.write(c.registry.wrapping_add(0x3c00),0)?;m.write_byte(c.registry_flag,1)?;}
  let count=word(m,c.registry,0x3c00)?;
  m.write(c.registry.wrapping_add(count.wrapping_mul(4)),c.service_metadata)?;
  m.write(c.registry.wrapping_add(0x3c00),count.wrapping_add(1))?;
 }
 Ok(())
}
fn make_pool(m:&mut Memory,c:Context,count:u32,host:&mut impl Host)->Result<u32>{
 let pool=allocate(m,c,0x1c,host)?;if pool==0{return Ok(0)}
 m.write(pool,c.pool_kind)?;
 for offset in [8,0x14]{m.write(pool.wrapping_add(offset),0)?;}
 m.write(pool.wrapping_add(0x18),count)?;
 for offset in [0x10,4,12]{m.write(pool.wrapping_add(offset),0)?;}
 if count==0{return Ok(pool)}
 let block=allocate(m,c,count.wrapping_mul(0x14).wrapping_add(16),host)?;
 //8000d334 array prefix and concrete1c44f0 constructors.
 let nodes=if block==0{0}else{
  m.write(block,0x14)?;m.write(block.wrapping_add(4),count)?;
  let nodes=block.wrapping_add(16);
  for i in 0..count{
   let node=nodes.wrapping_add(i.wrapping_mul(0x14));m.write(node,c.node_kind)?;
   m.write(node.wrapping_add(8),0)?;m.write(node.wrapping_add(12),0)?;m.write_byte(node.wrapping_add(0x10),0)?;
  }
  nodes
 };
 m.write(pool.wrapping_add(4),nodes)?;m.write(pool.wrapping_add(0x10),nodes)?;m.write(nodes.wrapping_add(8),0)?;
 let mut index=1u32;
 while index<word(m,pool,0x18)?{
  let base=word(m,pool,4)?;let previous=base.wrapping_add(index.wrapping_sub(1).wrapping_mul(0x14));let node=base.wrapping_add(index.wrapping_mul(0x14));
  m.write(node.wrapping_add(8),previous)?;
  let node=word(m,pool,4)?.wrapping_add(index.wrapping_mul(0x14));let previous=word(m,node,8)?;m.write(previous.wrapping_add(12),node)?;
  index=index.wrapping_add(1);
 }
 let count=word(m,pool,0x18)?;let base=word(m,pool,4)?;
 m.write(base.wrapping_add(count.wrapping_sub(1).wrapping_mul(0x14)).wrapping_add(12),0)?;Ok(pool)
}
pub(super) fn append(m:&mut Memory,pool:u32,value:u32)->Result<()>{
 let node=word(m,pool,0x10)?;if node==0{return Ok(())}
 m.write(pool.wrapping_add(0x14),word(m,pool,0x14)?.wrapping_add(1))?;
 m.write(pool.wrapping_add(0x10),word(m,node,12)?)?;
 m.write(node.wrapping_add(4),value)?;m.write_byte(node.wrapping_add(0x10),1)?;
 let tail=word(m,pool,12)?;
 if tail==0{
  m.write(pool.wrapping_add(8),node)?;m.write(pool.wrapping_add(12),node)?;m.write(node.wrapping_add(12),0)?;
  let head=word(m,pool,8)?;m.write(head.wrapping_add(8),0)?;
  let tail=word(m,pool,12)?;m.write(tail.wrapping_add(8),0)?;
  let tail=word(m,pool,12)?;m.write(tail.wrapping_add(12),0)?;
 }else{
  m.write(node.wrapping_add(8),tail)?;m.write(node.wrapping_add(12),0)?;
  let tail=word(m,pool,12)?;m.write(tail.wrapping_add(12),node)?;m.write(pool.wrapping_add(12),node)?;
 }
 Ok(())
}
///46b600..74: unlink one active0x14-byte node and prepend it to free.
/// Retains value/previous fields and native wrapping count semantics.
pub(super) fn unlink(m:&mut Memory,pool:u32,node:u32)->Result<()> {
 m.write_byte(node.wrapping_add(0x10),0)?;
 if node==word(m,pool,8)? {let next=word(m,node,12)?;m.write(pool.wrapping_add(8),next)?;}
 if node==word(m,pool,12)? {let previous=word(m,node,8)?;m.write(pool.wrapping_add(12),previous)?;}
 let previous=word(m,node,8)?;if previous!=0{let next=word(m,node,12)?;m.write(previous.wrapping_add(12),next)?;}
 let next=word(m,node,12)?;if next!=0{let previous=word(m,node,8)?;m.write(next.wrapping_add(8),previous)?;}
 let free=word(m,pool,0x10)?;m.write(node.wrapping_add(12),free)?;m.write(pool.wrapping_add(0x10),node)?;
 let count=word(m,pool,0x14)?;m.write(pool.wrapping_add(0x14),count.wrapping_sub(1))
}
///1c3a14: actual manager capacity, six pools in native allocation/publication
/// order, configuration, then interleaved actor filters in14 and18.
pub fn initialize(m:&mut Memory,c:Context,service:u32,root:u32,host:&mut impl Host)->Result<u32>{
 let manager=ai_traversal_exclusion::find_service(m,m.word(c.root_slot)?,c.manager_class,host)?;
 if manager!=0{
  let config=word(m,manager,4)?;
  if config!=0{
   let capacity=word(m,config,0x18)?;
   if capacity!=0{
    for offset in [12,16,28,24,20,32]{let pool=make_pool(m,c,capacity,host)?;m.write(service.wrapping_add(offset),pool)?;}
   }
  }
 }
 if host.configure(m,service,root)?&255==0{return Ok(0)}
 if word(m,service,0x14)?==0 || word(m,service,0x18)?==0{return Ok(1)}
 let owner=word(m,service,4)?;
 if host.owner_filters(m,owner)?==0{return Ok(1)}
 let list=host.owner_filters(m,word(m,service,4)?)?;let mut node=word(m,list,8)?;
 while node!=0{
  append(m,word(m,service,0x14)?,word(m,node,4)?)?;
  append(m,word(m,service,0x18)?,word(m,node,4)?)?;
  node=word(m,node,12)?;
 }
 Ok(1)
}

#[cfg(test)]
mod tests{
 use super::*;use serde_json::{Value,json};
 const FIXED:u32=0x60000000;const OWNER:u32=0x62000000;const RECORD:u32=0x62001000;const NAME:u32=0x67000000;
 #[derive(Clone,Copy)]struct Block{address:u32,size:u32}
 struct Runtime<'a>{input:&'a Value,next:u32,subject:u32,initializing:bool,headers:i64,observations:usize,blocks:Vec<Block>,trace:Vec<Value>}
 impl Services for Runtime<'_>{
  fn class20(&mut self,_:&mut Memory,service:u32)->Result<u32>{self.trace.push(json!(["class20",service]));Ok(if self.input["manager"].as_bool().unwrap() && !self.input["derived"].as_bool().unwrap(){0x41d2c}else{0x64000000})}
 }
 impl Host for Runtime<'_>{
  fn allocate(&mut self,m:&mut Memory,bytes:u32)->Result<Allocation>{
   let header=self.initializing && bytes==0x1c;let failed=header && self.headers==self.input["fail_header"].as_i64().unwrap();if header{self.headers+=1;}
   let address=if failed{0}else{self.next};let result=0x40000000+self.blocks.len() as u32;
   self.blocks.push(Block{address,size:bytes});
   if address!=0{anyhow::ensure!(m.regions.insert(address,vec![0xcc;bytes as usize]).is_none(),"Fixture allocation overlaps");self.next=self.next.wrapping_add(bytes.wrapping_add(15)&!15);}
   if !self.initializing && bytes==0x24{self.subject=address;}
   self.trace.push(json!(["allocate",bytes,address,result]));Ok(Allocation{address,result})
  }
  fn observe(&mut self,m:&mut Memory,_:u32,address:u32,bytes:u32,result:u32)->Result<()>{
   let service=(0..9).map(|i|m.word(self.subject+i*4)).collect::<Result<Vec<_>>>()?;
   self.trace.push(json!(["observe",address,bytes,result,service,m.word(0x11868)?,m.byte(0x11e2)?,m.word(0x4de4)?]));
   let mutation=self.input["observer_mutation"].as_str().unwrap();
   if mutation=="capacity" && self.initializing{m.write(0x61003018,self.input["capacity"].as_u64().unwrap() as u32+7)?;}
   if mutation=="name" && !self.initializing && self.observations==1{m.write_byte(NAME,b'z')?;m.write_byte(NAME+1,0)?;}
   if mutation=="registration"{m.write(0x11868,0x77770000)?;}
   self.observations+=1;Ok(())
  }
  fn configure(&mut self,m:&mut Memory,service:u32,root:u32)->Result<u32>{
   self.trace.push(json!(["configure",service,root]));
   match self.input["configure_mutation"].as_str().unwrap(){"clear_owned"=>m.write(OWNER+0xf8,0)?,"publish_owned"=>m.write(OWNER+0xf8,RECORD)?,_=>{}}
   Ok(self.input["configure"].as_u64().unwrap() as u32)
  }
 }
 fn context()->Context{Context{root_slot:0x5a108,manager_class:0x41d2c,observer_slot:0x4e1c0,service_kind:0x4ebbdc,parent_kind:0x4ebc20,interface_kind:0x520118,pool_kind:0x4ec020,node_kind:0x4ec008,service_metadata:0x11864,registry_flag:0x11e2,registry:0x11e4}}
 #[test]
 fn retail_spatial_factory_and_pool_initialization_match_owned_bytes_and_callbacks()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_construction.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(FIXED,vec![0;36]),(0x61000000,vec![0;0x40]),(0x61001000,vec![0;4]),(0x61002000,vec![0;0x30]),(0x61003000,vec![0;32]),(OWNER,vec![0;0x100]),(RECORD,vec![0;0x60]),(0x62001100,vec![0;0x100]),(0x63000000,vec![0;0x40]),(0x63000100,vec![0;0x80]),(0x64000000,vec![0;0x20]),(NAME,vec![0;0x100]),(0x68000000,vec![0;0x30]),(0x11864,vec![0;24]),(0x11e2,vec![0;1]),(0x11e4,vec![0;0x3c04]),(0x41d2c,vec![0;0x18]),(0x5a108,vec![0;4]),(0x4e1bc,vec![0;8])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (p,v) in case["bytes"].as_object().unwrap(){m.write_byte(p.parse()?,v.as_u64().unwrap() as u8)?;}
   let input=&case["input"];let op=input["op"].as_str().unwrap();let c=context();
   let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:op=="init",headers:0,observations:0,blocks:vec![],trace:vec![]};
   let mut service=FIXED;
   if op!="init"{service=create(&mut m,c,OWNER,NAME,&mut host)?;if op=="twice"{service=create(&mut m,c,OWNER,NAME,&mut host)?;}}
   let result=if op=="both" || op=="init"{host.initializing=true;initialize(&mut m,c,service,0x68000000,&mut host)?}else{service};
   let count=m.word(0x4de4)?;let count_max=count.max(input["count"].as_u64().unwrap() as u32).max(1);
   let mut allocations=Vec::new();
   for b in &host.blocks{
    let bytes=if b.address==0{Value::Null}else{Value::String((0..b.size).map(|i|m.byte(b.address+i).map(|v|format!("{v:02x}"))).collect::<Result<Vec<_>>>()?.concat())};
    allocations.push(json!({"address":b.address,"size":b.size,"bytes":bytes}));
   }
   let actual=json!({"result":result,"subject":service,"service":(0..9).map(|i|m.word(service+i*4)).collect::<Result<Vec<_>>>()?,"allocations":allocations,"trace":host.trace,"metadata":m.word(0x11868)?,"flag":m.byte(0x11e2)?,"count":count,"registry":(0..count_max).map(|i|m.word(0x11e4+i*4)).collect::<Result<Vec<_>>>()?,"capacity":m.word(0x61003018)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }
 impl super::super::ai_pathfinder_spatial_construction::Allocator for Runtime<'_>{
  fn class20(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,m:&mut Memory,service:u32)->Result<u32>{Services::class20(self,m,service)}
  fn allocate(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,m:&mut Memory,bytes:u32)->Result<Allocation>{Host::allocate(self,m,bytes)}
  fn observe(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,m:&mut Memory,o:u32,a:u32,b:u32,r:u32)->Result<()>{Host::observe(self,m,o,a,b,r)}
 }
 impl super::super::ai_pathfinder_spatial_construction::Backend for Runtime<'_>{
  fn configure(&mut self,state:&mut super::super::ai_pathfinder_constructor::State,m:&mut Memory,service:u32,root:u32)->Result<u32>{state.word_30=123;Host::configure(self,m,service,root)}
 }
 #[test]
 fn canonical_lpf_initialization_matches_original_allocations_and_filter_propagation()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_construction.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(FIXED,vec![0;36]),(0x61000000,vec![0;0x40]),(0x61001000,vec![0;4]),(0x61002000,vec![0;0x30]),(0x61003000,vec![0;32]),(OWNER,vec![0;0x100]),(RECORD,vec![0;0x60]),(0x62001100,vec![0;0x100]),(0x63000000,vec![0;0x40]),(0x63000100,vec![0;0x80]),(0x64000000,vec![0;0x20]),(NAME,vec![0;0x100]),(0x68000000,vec![0;0x30]),(0x11864,vec![0;24]),(0x11e2,vec![0;1]),(0x11e4,vec![0;0x3c04]),(0x41d2c,vec![0;0x18]),(0x5a108,vec![0;4]),(0x4e1bc,vec![0;8])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (p,v) in case["bytes"].as_object().unwrap(){m.write_byte(p.parse()?,v.as_u64().unwrap() as u8)?;}
   let input=&case["input"];let op=input["op"].as_str().unwrap();if op!="init"{continue;}let c=context();
   let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:op=="init",headers:0,observations:0,blocks:vec![],trace:vec![]};
   let service=FIXED;
   let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;state.identity=OWNER as usize;
   super::super::ai_pathfinder_spatial_binding::tests::attach_lpf(&mut state,input["owner_filter"].as_bool().unwrap().then_some(RECORD as usize),Some((RECORD+32) as usize))?;
   m.write(OWNER+0xf8,0xdeadbeef)?;m.write(RECORD+8,0xdeadbeef)?;
   let result=initialize(&mut m,c,service,0x68000000,&mut super::super::ai_pathfinder_spatial_construction::Binding{state:&mut state,backend:&mut host})?;
   assert_eq!(state.word_30,123);
   let count=m.word(0x4de4)?;let count_max=count.max(input["count"].as_u64().unwrap() as u32).max(1);
   let mut allocations=Vec::new();
   for b in &host.blocks{
    let bytes=if b.address==0{Value::Null}else{Value::String((0..b.size).map(|i|m.byte(b.address+i).map(|v|format!("{v:02x}"))).collect::<Result<Vec<_>>>()?.concat())};
    allocations.push(json!({"address":b.address,"size":b.size,"bytes":bytes}));
   }
   let actual=json!({"result":result,"subject":service,"service":(0..9).map(|i|m.word(service+i*4)).collect::<Result<Vec<_>>>()?,"allocations":allocations,"trace":host.trace,"metadata":m.word(0x11868)?,"flag":m.byte(0x11e2)?,"count":count,"registry":(0..count_max).map(|i|m.word(0x11e4+i*4)).collect::<Result<Vec<_>>>()?,"capacity":m.word(0x61003018)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
  }
  Ok(())
 }
 impl super::super::ai_goto_gap::Host for Runtime<'_>{
  fn unit_scale(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<Option<f32>>{anyhow::bail!("Spatial reset must not invoke steering scale")}
  fn owner_entity(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<usize>{unreachable!()}
  fn profiling_enabled(&mut self,_:&mut super::super::ai_goto_gap::State)->Result<bool>{unreachable!()}
  fn register_budget(&mut self,_:&mut super::super::ai_goto_gap::State,_:&str)->Result<i32>{unreachable!()}
 }
 #[test]
 fn owned_spatial_factory_configuration_and_filters_match_complete_original_pipeline()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_owned.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(FIXED,vec![0;36]),(0x61000000,vec![0;0x40]),(0x61001000,vec![0;4]),(0x61002000,vec![0;0x30]),(0x61003000,vec![0;32]),(OWNER,vec![0;0x100]),(RECORD,vec![0;0x60]),(0x62001100,vec![0;0x100]),(0x63000000,vec![0;0x40]),(0x63000100,vec![0;0x80]),(0x64000000,vec![0;0x20]),(NAME,vec![0;0x100]),(0x68000000,vec![0;0x30]),(0x11864,vec![0;24]),(0x11e2,vec![0;1]),(0x11e4,vec![0;0x3c04]),(0x41d2c,vec![0;0x18]),(0x5a108,vec![0;4]),(0x4e1bc,vec![0;8]),(0x4ebbdc,vec![0;0x40]),(0x6a000000,vec![0;0x400])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (p,v) in case["bytes"].as_object().unwrap(){m.write_byte(p.parse()?,v.as_u64().unwrap() as u8)?;}
   let input=&case["input"];let op=input["op"].as_str().unwrap();let c=context();
   let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:op=="init",headers:0,observations:0,blocks:vec![],trace:vec![]};
   for (p,v) in case["extra"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (i,text) in [b"Other".as_slice(),b"Static",b"Physics"].into_iter().enumerate(){for (j,byte) in text.iter().copied().chain([0]).enumerate(){m.write_byte(0x6a000200+i as u32*0x40+j as u32,byte)?;}}
   let mut parent=super::super::ai_pathfinder_modifiers::tests::state()?;parent.identity=OWNER as usize;
   super::super::ai_pathfinder_spatial_binding::tests::attach_lpf(&mut parent,Some(RECORD as usize),Some((RECORD+32) as usize))?;
   m.write(OWNER+0xf8,0xdeadbeef)?;m.write(RECORD+8,0xdeadbeef)?;
   use super::super::{ai_pathfinder_modifier_factory::Owned,ai_pathfinder_modifiers::Kind,ai_navigation_definitions::{Tree,Node}};
   let mut owned=Owned::construct_spatial(Kind(3),Owned::SPATIAL_CLASS,&mut parent,&mut m,c,NAME,&mut host)?;
   let service=u32::try_from(owned.identity())?;assert_eq!(owned.pathfinder(&m)?,parent.identity);assert_eq!(owned.kind(),Kind(3));
   host.initializing=true;
   let mut tree=Tree::default();let root=if let Some(rows)=input["properties"].as_array(){
    let root=tree.append(None,Node::default())?;for row in rows{tree.append(Some(root),Node{name:Some(row[0].as_str().unwrap().as_bytes().to_vec()),value:Some(row[1].as_str().unwrap().as_bytes().to_vec()),..Node::default()})?;}Some(root)
   }else{None};
   let result=u32::from(owned.initialize_spatial(&mut parent,&mut m,c,&mut tree,root,&mut host)?);

   let count=m.word(0x4de4)?;let count_max=count.max(input["count"].as_u64().unwrap() as u32).max(1);
   let mut allocations=Vec::new();
   for b in &host.blocks{
    let bytes=if b.address==0{Value::Null}else{Value::String((0..b.size).map(|i|m.byte(b.address+i).map(|v|format!("{v:02x}"))).collect::<Result<Vec<_>>>()?.concat())};
    allocations.push(json!({"address":b.address,"size":b.size,"bytes":bytes}));
   }
   let actual=json!({"result":result,"subject":service,"service":(0..9).map(|i|m.word(service+i*4)).collect::<Result<Vec<_>>>()?,"allocations":allocations,"trace":host.trace,"metadata":m.word(0x11868)?,"flag":m.byte(0x11e2)?,"count":count,"registry":(0..count_max).map(|i|m.word(0x11e4+i*4)).collect::<Result<Vec<_>>>()?,"capacity":m.word(0x61003018)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
   assert_eq!(owned.name(&m)?.as_ref(),b"BipedPathFinder");
   let name=m.word(service+8)?;m.write_byte(name,b'Z')?;assert_eq!(owned.name(&m)?.as_ref(),b"ZipedPathFinder");
   m.write(service+4,OWNER+4)?;assert_eq!(owned.pathfinder(&m)?,(OWNER+4) as usize);m.write(service+4,OWNER)?;
   let before=m.regions.clone();owned.reset(&mut host)?;assert_eq!(m.regions,before,"Concrete spatial reset wrote fields");
  }
  Ok(())
 }

 impl super::super::ai_goto_gap_setup::Host for Runtime<'_>{
  fn world_service(&mut self,_:&mut super::super::ai_goto_gap::State,_:super::super::ai_goto_gap_setup::Service)->Result<Option<usize>>{anyhow::bail!("No gap setup in spatial fixture")}
  fn entity_attribute(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize,_:super::super::ai_goto_gap_setup::Attribute)->Result<Option<usize>>{unreachable!()}
 }
 impl super::super::ai_goto_gap_definition_binding::Backend for Runtime<'_>{
  fn number(&mut self,_:&mut super::super::ai_goto_gap::State,_:Option<&[u8]>)->Result<f64>{unreachable!()}
  fn integer(&mut self,_:&mut super::super::ai_goto_gap::State,_:Option<&[u8]>)->Result<i32>{unreachable!()}
  fn diameter_attribute(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize)->Result<Option<usize>>{unreachable!()}
  fn diameter_value(&mut self,_:&mut super::super::ai_goto_gap::State,_:usize)->Result<f32>{unreachable!()}
 }
 impl super::super::ai_goal_detection::DefinitionBackend for Runtime<'_>{
  fn unit_scale(&mut self,_:&mut super::super::ai_goal_detection::State)->Result<Option<f32>>{unreachable!()}
  fn number(&mut self,_:&mut super::super::ai_goal_detection::State,_:Option<&[u8]>)->Result<f64>{unreachable!()}
  fn entity_height(&mut self,_:&mut super::super::ai_goal_detection::State)->Result<Option<f32>>{unreachable!()}
 }
 impl super::super::ai_pathfinder_concrete_modifiers::Backend for Runtime<'_>{
  fn parent_entity(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory)->Result<usize>{anyhow::bail!("No typed owner query expected in spatial fixture")}
  fn parent_height(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:usize)->Result<Option<f32>>{anyhow::bail!("No typed height query expected in spatial fixture")}

  fn pathfinder_integer(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("No scalar parser expected in spatial fixture")}
  fn pathfinder_number(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&[u8])->Result<f64>{unreachable!()}
  fn pathfinder_scale(&mut self,_:&mut super::super::ai_pathfinder_constructor::State)->Result<Option<f32>>{unreachable!()}
  fn find_database(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:usize,_:&[u8])->Result<Option<usize>>{anyhow::bail!("No database query expected in spatial fixture")}
  fn register_database_callback(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:usize)->Result<()>{unreachable!()}
  fn allocate_typed(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:&super::super::ai_pathfinder_concrete_modifiers::Class)->Result<usize>{anyhow::bail!("No typed modifier allocation expected in spatial fixture")}
  fn allocate_table(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,m:&mut Memory,_:super::super::ai_pathfinder_modifiers::Kind,capacity:u32)->Result<super::super::ai_pathfinder_modifiers::Storage>{
   let identity=allocate(m,context(),capacity.wrapping_mul(4),self)?;
   Ok(super::super::ai_pathfinder_modifiers::Storage{identity:identity as usize,slots:vec![None;capacity as usize]})
  }
  fn free_table(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:super::super::ai_pathfinder_modifiers::Kind,_:usize)->Result<()>{anyhow::bail!("Fresh table fixture must not free old storage")}
 }

 fn spatial_metadata()->super::super::ai_pathfinder_concrete_modifiers::Metadata{
  use super::super::{ai_pathfinder_concrete_modifiers::{Metadata,Class},ai_pathfinder_modifiers::Kind,ai_pathfinder_modifier_factory::Owned};
  let mut defaults=[None;13];defaults[3]=Some(0x11864);
  Metadata{classes:vec![Class{identity:0x11864,kind:Kind(3),name:Owned::SPATIAL_CLASS.to_vec(),parent:None,vtable:Some(0x4ebbdc)}],defaults}
 }
 #[test]
 fn concrete_default_backend_matches_original_inline_spatial_construction_without_registration()->Result<()>{
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_default_owned.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(FIXED,vec![0;36]),(0x61000000,vec![0;0x40]),(0x61001000,vec![0;4]),(0x61002000,vec![0;0x30]),(0x61003000,vec![0;32]),(OWNER,vec![0;0x100]),(RECORD,vec![0;0x60]),(0x62001100,vec![0;0x100]),(0x63000000,vec![0;0x40]),(0x63000100,vec![0;0x80]),(0x64000000,vec![0;0x20]),(NAME,vec![0;0x100]),(0x68000000,vec![0;0x30]),(0x11864,vec![0;24]),(0x11e2,vec![0;1]),(0x11e4,vec![0;0x3c04]),(0x41d2c,vec![0;0x18]),(0x5a108,vec![0;4]),(0x4e1bc,vec![0;8]),(0x4ebbdc,vec![0;0x40]),(0x6a000000,vec![0;0x400])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (p,v) in case["bytes"].as_object().unwrap(){m.write_byte(p.parse()?,v.as_u64().unwrap() as u8)?;}
   let input=&case["input"];let op=input["op"].as_str().unwrap();let c=context();
   let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:op=="init",headers:0,observations:0,blocks:vec![],trace:vec![]};
   for (p,v) in case["extra"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (i,text) in [b"Other".as_slice(),b"Static",b"Physics"].into_iter().enumerate(){for (j,byte) in text.iter().copied().chain([0]).enumerate(){m.write_byte(0x6a000200+i as u32*0x40+j as u32,byte)?;}}
   let mut parent=super::super::ai_pathfinder_modifiers::tests::state()?;parent.identity=OWNER as usize;
   super::super::ai_pathfinder_spatial_binding::tests::attach_lpf(&mut parent,Some(RECORD as usize),Some((RECORD+32) as usize))?;
   m.write(OWNER+0xf8,0xdeadbeef)?;m.write(RECORD+8,0xdeadbeef)?;
   use super::super::{ai_pathfinder_modifier_factory::Owned,ai_pathfinder_modifiers::Kind,ai_navigation_definitions::{Tree,Node}};
   use super::super::{ai_pathfinder_concrete_modifiers as concrete,ai_pathfinder_definition_binding::DefaultsBackend};
   let metadata=spatial_metadata();let mut objects=std::collections::BTreeMap::new();let mut tree=Tree::default();
   let (service,result)={let mut binding=concrete::Binding{metadata:&metadata,objects:&mut objects,memory:&mut m,spatial:c,backend:&mut host};
    let service=DefaultsBackend::construct_default(&mut binding,&mut parent,Kind(3),0x11864,b"default")?.unwrap();
    binding.backend.initializing=true;
    let result=DefaultsBackend::initialize_default(&mut binding,&mut parent,Kind(3),service,&mut tree)?;
    assert_eq!(DefaultsBackend::object_class(&mut binding,&mut parent,Kind(3),service)?,0x11864);
    (u32::try_from(service)?,u32::from(result))};
   let mut owned=objects.remove(&(service as usize)).unwrap();assert_eq!(owned.pathfinder(&m)?,parent.identity);assert_eq!(owned.kind(),Kind(3));
   let count=m.word(0x4de4)?;let count_max=count.max(input["count"].as_u64().unwrap() as u32).max(1);
   let mut allocations=Vec::new();
   for b in &host.blocks{
    let bytes=if b.address==0{Value::Null}else{Value::String((0..b.size).map(|i|m.byte(b.address+i).map(|v|format!("{v:02x}"))).collect::<Result<Vec<_>>>()?.concat())};
    allocations.push(json!({"address":b.address,"size":b.size,"bytes":bytes}));
   }
   let actual=json!({"result":result,"subject":service,"service":(0..9).map(|i|m.word(service+i*4)).collect::<Result<Vec<_>>>()?,"allocations":allocations,"trace":host.trace,"metadata":m.word(0x11868)?,"flag":m.byte(0x11e2)?,"count":count,"registry":(0..count_max).map(|i|m.word(0x11e4+i*4)).collect::<Result<Vec<_>>>()?,"capacity":m.word(0x61003018)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {input}");}
   assert_eq!(owned.name(&m)?.as_ref(),b"default");
   let name=m.word(service+8)?;m.write_byte(name,b'Z')?;assert_eq!(owned.name(&m)?.as_ref(),b"Zefault");
   m.write(service+4,OWNER+4)?;assert_eq!(owned.pathfinder(&m)?,(OWNER+4) as usize);m.write(service+4,OWNER)?;
   let before=m.regions.clone();owned.reset(&mut host)?;assert_eq!(m.regions,before,"Concrete spatial reset wrote fields");
  }
  Ok(())
 }

 fn concrete_fixture_memory(case:&Value)->Result<Memory>{
   let mut m=Memory{regions:[(FIXED,vec![0;36]),(0x61000000,vec![0;0x40]),(0x61001000,vec![0;4]),(0x61002000,vec![0;0x30]),(0x61003000,vec![0;32]),(OWNER,vec![0;0x100]),(RECORD,vec![0;0x60]),(0x62001100,vec![0;0x100]),(0x63000000,vec![0;0x40]),(0x63000100,vec![0;0x80]),(0x64000000,vec![0;0x20]),(NAME,vec![0;0x100]),(0x68000000,vec![0;0x30]),(0x11864,vec![0;24]),(0x11e2,vec![0;1]),(0x11e4,vec![0;0x3c04]),(0x41d2c,vec![0;0x18]),(0x5a108,vec![0;4]),(0x4e1bc,vec![0;8]),(0x4ebbdc,vec![0;0x40]),(0x6a000000,vec![0;0x400])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (p,v) in case["bytes"].as_object().unwrap(){m.write_byte(p.parse()?,v.as_u64().unwrap() as u8)?;}
   for (p,v) in case["extra"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?;}
   for (i,text) in [b"Other".as_slice(),b"Static",b"Physics"].into_iter().enumerate(){for (j,byte) in text.iter().copied().chain([0]).enumerate(){m.write_byte(0x6a000200+i as u32*0x40+j as u32,byte)?;}}
   Ok(m)
 }
 #[test]
 fn concrete_modifier_backend_configures_named_group_and_rejects_partial_failures()->Result<()>{
  use super::super::{ai_pathfinder_concrete_modifiers as concrete,ai_pathfinder_definition_binding as definitions,ai_navigation_definitions::{Tree,Node},ai_pathfinder_modifiers::Kind};
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_default_owned.json"))?;let case=&fixture["cases"][4];let input=&case["input"];
  for valid in [true,false]{
   let mut m=concrete_fixture_memory(case)?;let mut parent=super::super::ai_pathfinder_modifiers::tests::state()?;parent.identity=OWNER as usize;m.write(OWNER+0xf8,0xdeadbeef)?;
   let metadata=spatial_metadata();let mut objects=std::collections::BTreeMap::new();let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:true,headers:0,observations:0,blocks:vec![],trace:vec![]};
   let mut tree=Tree::default();let root=tree.append(None,Node::default())?;let group=tree.append(Some(root),Node{name:Some(b"Modifiers_ICanGo".to_vec()),..Node::default()})?;
   tree.append(Some(group),Node{name:Some(b"default".to_vec()),value:Some(b"Beta".to_vec()),..Node::default()})?;
   for (name,class) in [(b"Alpha".as_slice(),super::super::ai_pathfinder_modifier_factory::Owned::SPATIAL_CLASS),(b"Beta".as_slice(),if valid{super::super::ai_pathfinder_modifier_factory::Owned::SPATIAL_CLASS}else{b"UnsupportedClass"})]{
    let n=tree.append(Some(group),Node{name:Some(b"ICanGo".to_vec()),identifier:Some(name.to_vec()),..Node::default()})?;
    tree.append(Some(n),Node{name:Some(b"Class".to_vec()),value:Some(class.to_vec()),..Node::default()})?;
   }
   let mut binding=concrete::Binding{metadata:&metadata,objects:&mut objects,memory:&mut m,spatial:context(),backend:&mut host};
   assert_eq!(binding.configure(&mut parent,&mut tree,root)?,valid);
   let table=&parent.tables_fc_1fc[Kind(3).table()];assert_eq!(table.count,if valid{2}else{1});assert_eq!(binding.objects.len(),if valid{2}else{1});
   if valid{
    let id=table.selected.flatten().unwrap();assert_eq!(definitions::Backend::object_name(&mut binding,&mut parent,Kind(3),id)?,b"Beta");
    assert_eq!(definitions::DefaultsBackend::object_class(&mut binding,&mut parent,Kind(3),id)?,0x11864);
    assert_eq!(binding.memory.word(0x11868)?,context().service_kind,"Named path must register its class");
    let name=binding.memory.word(id as u32+8)?;binding.memory.write_byte(name,b'Z')?;
    assert_eq!(definitions::Backend::object_name(&mut binding,&mut parent,Kind(3),id)?,b"Zeta");
    binding.memory.write(id as u32+4,123)?;assert!(definitions::Backend::object_name(&mut binding,&mut parent,Kind(3),id).is_err());
   }
  }Ok(())
 }

 impl super::super::ai_pathfinder_reset::Host for Runtime<'_>{
  fn detach_goal(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:u32)->Result<()>{anyhow::bail!("Unexpected goal detach in reset fixture")}
  fn reset_modifier(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:super::super::ai_pathfinder_modifiers::Kind,_:usize)->Result<()>{anyhow::bail!("Concrete reset must not use legacy modifier storage")}
  fn release(&mut self,_:&mut super::super::ai_pathfinder_constructor::State,_:&mut Memory,_:usize,_:u32)->Result<()>{anyhow::bail!("Unexpected graph release in reset fixture")}
 }
 #[test]
 fn concrete_final_reset_uses_same_registry_for_typed_and_arena_modifiers()->Result<()>{
  use super::super::{ai_pathfinder_concrete_modifiers as concrete,ai_pathfinder_definition_binding::DefaultsBackend,ai_pathfinder_modifier_factory::Owned,ai_pathfinder_modifiers::{Kind,Storage}};
  let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_spatial_default_owned.json"))?;let case=&fixture["cases"][0];let input=&case["input"];
  let mut m=concrete_fixture_memory(case)?;m.regions.insert(1000,vec![0;0x1c4]);let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;state.identity=OWNER as usize;
  state.word_364=Some(0);state.scalar_40c=Some(0.);state.references_3c_4c_50_54_58_60_6c[5]=Some(1000);
  let mut host=Runtime{input,next:0x65000000,subject:FIXED,initializing:false,headers:0,observations:0,blocks:vec![],trace:vec![]};let metadata=spatial_metadata();let mut objects=std::collections::BTreeMap::new();
  let spatial={let mut binding=concrete::Binding{metadata:&metadata,objects:&mut objects,memory:&mut m,spatial:context(),backend:&mut host};DefaultsBackend::construct_default(&mut binding,&mut state,Kind(3),0x11864,b"default")?.unwrap()};
  let mut refine=Owned::construct(Kind(0),Owned::REFINE_CLASS,100,&state,b"refine",&mut host)?;
  let Owned::RefineGoal(r)=&mut refine else{unreachable!()};r.base_vectors=[[1.,2.,3.];2];objects.insert(100,refine);
  for (kind,id) in [(Kind(0),100),(Kind(3),spatial)]{let t=&mut state.tables_fc_1fc[kind.table()];t.count=1;t.capacity=1;t.selected=Some(Some(id));t.storage=Some(Storage{identity:id,slots:vec![Some(id)]});}
  let before=(0..0x24).map(|i|m.byte(spatial as u32+i)).collect::<Result<Vec<_>>>()?;
  concrete::reset_owned(&mut state,&mut m,&mut objects,&mut host)?;
  assert_eq!(state.reset_fields.word_36c,Some(2));let Owned::RefineGoal(r)=&objects[&100] else{unreachable!()};assert_eq!(r.base_vectors,[[f32::MAX;3];2]);
  assert_eq!((0..0x24).map(|i|m.byte(spatial as u32+i)).collect::<Result<Vec<_>>>()?,before);Ok(())
 }

 impl super::super::ai_predict_move::DefinitionBackend for Runtime<'_>{
  fn number(&mut self,_:&mut super::super::ai_predict_move::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("No accident numeric parser expected in this fixture")}
 }

 impl super::super::ai_node_candidate::DefinitionBackend for Runtime<'_>{
  fn unit_scale(&mut self,_:&mut super::super::ai_node_candidate::State)->Result<Option<f32>>{anyhow::bail!("Candidate scale not expected in this fixture")}
  fn integer(&mut self,_:&mut super::super::ai_node_candidate::State,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("Candidate integer not expected in this fixture")}
  fn number(&mut self,_:&mut super::super::ai_node_candidate::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Candidate number not expected in this fixture")}
 }

 impl super::super::ai_node_reached::DefinitionBackend for Runtime<'_>{
 fn unit_scale(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{anyhow::bail!("Node detector not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_node_reached::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Node detector not expected in this fixture")}
 fn entity_height(&mut self,_:&mut super::super::ai_node_reached::State)->Result<Option<f32>>{anyhow::bail!("Node detector not expected in this fixture")}
 }

 impl super::super::ai_edge_awareness::DefinitionBackend for Runtime<'_>{
 fn unit_scale(&mut self,_:&mut super::super::ai_edge_awareness::State)->Result<Option<f32>>{anyhow::bail!("Edge awareness not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_edge_awareness::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Edge awareness not expected in this fixture")}
 }

 impl super::super::ai_steering::DefinitionBackend for Runtime<'_>{
 fn unit_scale(&mut self,_:&mut super::super::ai_steering::State)->Result<Option<f32>>{anyhow::bail!("Biped steering not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_steering::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Biped steering not expected in this fixture")}
 }

 impl super::super::ai_compute_target::DefinitionBackend for Runtime<'_>{
 fn unit_scale(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<f32>>{anyhow::bail!("Queue target not expected in this fixture")}
 fn profiles(&mut self,_:&mut super::super::ai_compute_target::State)->Result<Option<usize>>{anyhow::bail!("Queue target not expected in this fixture")}
 fn number(&mut self,_:&mut super::super::ai_compute_target::State,_:Option<&[u8]>)->Result<f64>{anyhow::bail!("Queue target not expected in this fixture")}
 }

}
