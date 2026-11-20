//!Owned resources constructed by18d69c. Construction/unwritten fields, default
//!observer initialization/reset and deleting cleanup follow native code. Observer
//!runtime update policy is separate; these records alone do not admit an NPC.
use anyhow::{Context,Result,ensure};
use std::collections::BTreeMap;
use super::{ai_navigation_brain::{Brain,Observer as Kind},ai_graph_link_passes::Memory};
pub trait Allocator {
    ///Includes native allocation observer before returning to the caller.
    fn allocate_brain_record(&mut self,brain:&mut Brain,memory:&mut Memory,bytes:u32)->Result<usize>;
    fn free_brain_record(&mut self,brain:&mut Brain,memory:&mut Memory,identity:usize)->Result<()>;
}
#[derive(Debug)]
pub struct Observer {
    pub identity:usize,pub owner:usize,pub kind:Kind,pub service:usize,
    pub name:Option<Vec<u8>>,pub name_allocation:Option<usize>,pub rate:Option<f32>,
    pub vector_10:[Option<f32>;3],pub scalar_1c:Option<f32>,pub values_20_24:[Option<f32>;2],
    pub vectors_28_34:[[Option<f32>;3];2],pub scalar_40:Option<f32>,pub flag_44:Option<u8>,
}
impl Observer {
    ///1c6ce8 /1bddc4 with null attributes:49b274 returns1, then virtual14.
    pub fn initialize_default(&mut self){
        self.rate=Some(0.5);if matches!(self.kind,Kind::Default){self.flag_44=Some(0);}
        self.reset();
    }
    pub fn reset(&mut self){
        self.vector_10=[Some(f32::MAX);3];self.scalar_1c=Some(0.);self.values_20_24=[Some(0.);2];
        self.vectors_28_34=[[Some(f32::MAX);3];2];self.scalar_40=Some(0.);
        if matches!(self.kind,Kind::Default){self.flag_44=Some(0);}
    }
}
#[derive(Debug)]
pub struct Motion {pub identity:usize,pub owner:usize,pub word_04:u32,pub vector:[f32;3],pub scalar:f32}
#[derive(Default,Debug)]
pub struct Store {pub observers:BTreeMap<usize,Observer>,pub motions:BTreeMap<usize,Motion>}
impl Store {
    fn vacant(&self,id:usize)->Result<()>{ensure!(id!=0,"Required brain resource allocation is null");ensure!(!self.observers.contains_key(&id)&&!self.motions.contains_key(&id)&&!self.observers.values().any(|o|o.name_allocation==Some(id)),"Brain resource identity is already owned");Ok(())}
    pub fn construct_observer(&mut self,b:&mut Brain,m:&mut Memory,kind:Kind,name:&[u8],a:&mut impl Allocator)->Result<usize>{
        let bytes=match kind{Kind::FixedTurn=>0x44,Kind::Default=>0x48};let id=a.allocate_brain_record(b,m,bytes)?;self.vacant(id)?;
        //18d7b8/18d850 read service54 AFTER the object allocation hook.
        let service=b.service_54.context("Observer allocation left its brain without a pathfinder")?;ensure!(service!=0,"Observer pathfinder is null");
        self.observers.insert(id,Observer{identity:id,owner:b.identity,kind,service,name:None,name_allocation:None,rate:None,
            vector_10:[None;3],scalar_1c:None,values_20_24:[None;2],vectors_28_34:[[None;3];2],scalar_40:None,flag_44:None});
        let name=&name[..name.iter().position(|&v|v==0).unwrap_or(name.len())];
        let bytes=u32::try_from(name.len())?.checked_add(1).context("Observer name exceeds native allocation domain")?;
        let allocation=a.allocate_brain_record(b,m,bytes)?;ensure!(allocation!=0,"Observer name allocation is null");self.vacant(allocation)?;
        let s=self.observers.get_mut(&id).unwrap();s.name_allocation=Some(allocation);s.name=Some(name.to_vec());
        s.vector_10=[Some(0.);3];s.scalar_1c=Some(0.);s.vectors_28_34=[[Some(0.);3];2];Ok(id)
    }
    pub fn initialize_observer(&mut self,owner:usize,id:usize)->Result<()>{let s=self.observers.get_mut(&id).context("Brain observer is not owned")?;ensure!(s.owner==owner,"Brain observer owner differs");ensure!(s.name.is_some()&&s.name_allocation.is_some(),"Brain observer construction is incomplete");s.initialize_default();Ok(())}
    pub fn construct_motion(&mut self,b:&mut Brain,m:&mut Memory,a:&mut impl Allocator)->Result<Option<usize>>{
        let id=a.allocate_brain_record(b,m,0x18)?;if id==0{return Ok(None)}self.vacant(id)?;
        self.motions.insert(id,Motion{identity:id,owner:b.identity,word_04:0,vector:[0.;3],scalar:0.});Ok(Some(id))
    }
    ///29e854/29e8bc: name is released before the observer record. The owning
    ///brain destructor controls ordering between its different resource slots.
    pub fn destroy_observer(&mut self,b:&mut Brain,m:&mut Memory,id:usize,a:&mut impl Allocator)->Result<()>{
        let s=self.observers.get(&id).context("Destroyed observer is not owned")?;ensure!(s.owner==b.identity,"Destroyed observer owner differs");
        if let Some(name)=s.name_allocation{a.free_brain_record(b,m,name)?;}
        a.free_brain_record(b,m,id)?;self.observers.remove(&id);Ok(())
    }
    pub fn destroy_motion(&mut self,b:&mut Brain,m:&mut Memory,id:usize,a:&mut impl Allocator)->Result<()>{
        let s=self.motions.get(&id).context("Destroyed motion record is not owned")?;ensure!(s.owner==b.identity,"Destroyed motion owner differs");
        a.free_brain_record(b,m,id)?;self.motions.remove(&id);Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Base;
    impl super::super::ai_navigation_brain::Construction for Base{fn construct_base(&mut self,_:usize,_:Option<usize>,_:usize)->Result<()>{Ok(())}}
    struct Heap<'a>{row:&'a Value,allocations:usize,trace:Vec<Value>}
    impl Allocator for Heap<'_>{
        fn allocate_brain_record(&mut self,b:&mut Brain,_:&mut Memory,bytes:u32)->Result<usize>{
            let calls=self.row["trace"].as_array().unwrap().iter().filter(|v|v[0]=="allocate").collect::<Vec<_>>();
            let call=calls[self.allocations];assert_eq!(call[1],json!(bytes));let id=call[2].as_u64().unwrap() as usize;
            self.trace.push(json!(["allocate",bytes,id,b.service_54.unwrap_or(0)]));self.allocations+=1;
            if self.row["input"]["mutation"].as_u64().unwrap()==self.allocations as u64{b.service_54=Some(0x73000000+self.allocations*0x1000);}
            Ok(id)
        }
        fn free_brain_record(&mut self,_:&mut Brain,_:&mut Memory,id:usize)->Result<()>{self.trace.push(json!(["free",id]));Ok(())}
    }
    fn snapshot(store:&Store,id:Option<usize>)->Value{
        let Some(id)=id else{return Value::Null};
        if let Some(m)=store.motions.get(&id){return json!({"word_04":m.word_04,"vector":m.vector.map(f32::to_bits),"scalar":m.scalar.to_bits()})}
        let s=&store.observers[&id];let bits=|x:Option<f32>|x.map(f32::to_bits).unwrap_or(0xa5a5a5a5);
        json!({"service":s.service,"name":s.name,"name_allocation":s.name_allocation,"rate":bits(s.rate),"vector":s.vector_10.map(bits),"scalar":bits(s.scalar_1c),"values":s.values_20_24.map(bits),"vectors":s.vectors_28_34.map(|v|v.map(bits)),"tail":bits(s.scalar_40),"flag":if matches!(s.kind,Kind::Default){Some(s.flag_44.unwrap_or(0xa5))}else{None}})
    }
    #[test]
    fn original_construction_default_initialization_and_deleting_cleanup()->Result<()>{
        let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_brain_resources.json"))?;
        for row in capture["cases"].as_array().unwrap(){
            let c=&row["input"];let mut b=Brain::new(0x72000000,None,0,&mut Base)?;b.service_54=Some(0x73000000);
            let mut memory=Memory::default();let mut store=Store::default();let mut heap=Heap{row,allocations:0,trace:vec![]};
            let motion=c["kind"]=="motion";
            let id=if motion{store.construct_motion(&mut b,&mut memory,&mut heap)?}else{
                let (kind,name)=if c["kind"]=="fixed"{(Kind::FixedTurn,b"FixedTurn".as_slice())}else{(Kind::Default,b"Default".as_slice())};
                Some(store.construct_observer(&mut b,&mut memory,kind,name,&mut heap)?)
            };
            assert_eq!(json!(id.unwrap_or(0)),row["identity"],"{c}");assert_eq!(snapshot(&store,id),row["constructed"],"{c}");
            if let Some(id)=id{if !motion&&c["initialize"].as_bool().unwrap(){store.initialize_observer(b.identity,id)?;}}
            assert_eq!(snapshot(&store,id),row["initialized"],"{c}");
            if let Some(id)=id{if c["destroy"].as_bool().unwrap(){
                if motion{store.destroy_motion(&mut b,&mut memory,id,&mut heap)?;}else{store.destroy_observer(&mut b,&mut memory,id,&mut heap)?;}
                assert!(store.observers.is_empty()&&store.motions.is_empty());
            }}
            assert_eq!(json!(b.service_54.unwrap()),row["service"],"{c}");assert_eq!(json!(heap.trace),row["trace"],"{c}");
        }
        eprintln!("{} original observer/motion lifecycle cases",capture["cases"].as_array().unwrap().len());Ok(())
    }
}
