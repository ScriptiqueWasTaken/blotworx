//! Traversal table publication479e5c..479fa8 and owned portable storage.
//! Concrete traversal factories/initializers and World registration remain
//! required. Array growth copies handles, never copies traversal instances.
use anyhow::{ensure,Context as _,Result};
use std::collections::BTreeMap;
use super::ai_traversal_instances::{Instance,Pool};

#[derive(Default,Debug,serde::Serialize)]
pub struct State {
    pub array:Option<usize>, //10
    pub capacity:u32, //14
    pub count:u32, //18
}
pub trait PublicationHost {
    fn allocate(&mut self,state:&mut State,bytes:u32)->Result<Option<usize>>;
    fn construct_array(&mut self,state:&mut State,allocation:Option<usize>,count:u32)->Result<Option<usize>>;
    fn read_entry(&mut self,state:&mut State,array:usize,index:u32)->Result<Option<usize>>;
    fn write_entry(&mut self,state:&mut State,array:usize,index:u32,pool:Option<usize>)->Result<()>;
    fn release_array(&mut self,state:&mut State,array:usize)->Result<()>;
}
impl State {
    pub fn publish(&mut self,pool:usize,host:&mut impl PublicationHost)->Result<()> {
        if self.count==self.capacity {
            self.capacity=if self.capacity==0 {4}else{self.capacity.wrapping_mul(2)};
            let requested=self.capacity;
            let bytes=requested.wrapping_shl(3).wrapping_add(0x10);
            let allocation=host.allocate(self,bytes)?;
            let array=host.construct_array(self,allocation,requested)?
                .context("Traversal table construction unavailable")?;
            // Native rereads both words after allocation/construction callbacks.
            if self.count>self.capacity {self.count=self.capacity;}
            let mut index=0;
            while index<self.count {
                let old=self.array.context("Traversal table copy has no previous array")?;
                let entry=host.read_entry(self,old,index)?;
                host.write_entry(self,array,index,entry)?;
                index+=1;
            }
            if let Some(old)=self.array {host.release_array(self,old)?;}
            self.array=Some(array);
        }
        let array=self.array.context("Traversal publication has no array")?;
        let index=self.count;
        host.write_entry(self,array,index,Some(pool))?;
        self.count=self.count.wrapping_add(1);
        Ok(())
    }
}

#[derive(Debug)]
pub struct NamedPool {pub name:Vec<u8>,pub instances:Pool}
#[derive(Debug)]
struct Array {entries:Vec<Option<Option<usize>>>,constructed:bool}
#[derive(Default,Debug)]
pub struct Storage {
    next_array:usize,
    next_pool:usize,
    arrays:BTreeMap<usize,Array>,
    pools:BTreeMap<usize,NamedPool>,
}
impl Storage {
    fn add_pool(&mut self,name:&[u8],instances:Pool)->Result<usize> {
        self.next_pool=self.next_pool.checked_add(1).context("Traversal pool handle exhaustion")?;
        let name=&name[..name.iter().position(|&b|b==0).unwrap_or(name.len())];
        self.pools.insert(self.next_pool,NamedPool{name:name.to_vec(),instances});
        Ok(self.next_pool)
    }
    pub fn pool(&self,handle:usize)->Result<&NamedPool> {self.pools.get(&handle).context("Traversal pool absent")}
    pub(super) fn pool_mut(&mut self,handle:usize)->Result<&mut NamedPool> {self.pools.get_mut(&handle).context("Traversal pool absent")}
    fn entry(&self,array:usize,index:u32)->Result<Option<usize>> {
        let array=self.arrays.get(&array).context("Traversal array absent")?;
        ensure!(array.constructed,"Traversal array not constructed");
        array.entries.get(index as usize).context("Traversal entry outside allocated array")?
            .context("Traversal entry has not been initialized")
    }
}
impl PublicationHost for Storage {
    fn allocate(&mut self,_:&mut State,bytes:u32)->Result<Option<usize>> {
        ensure!(bytes>=0x10&&(bytes-0x10)%8==0,"Invalid traversal array allocation size");
        let count=((bytes-0x10)/8) as usize;
        let mut entries=Vec::new();entries.try_reserve_exact(count)?;entries.resize(count,None);
        self.next_array=self.next_array.checked_add(1).context("Traversal array handle exhaustion")?;
        self.arrays.insert(self.next_array,Array{entries,constructed:false});
        Ok(Some(self.next_array))
    }
    fn construct_array(&mut self,_:&mut State,allocation:Option<usize>,count:u32)->Result<Option<usize>> {
        let Some(handle)=allocation else {return Ok(None)};
        let array=self.arrays.get_mut(&handle).context("Traversal array allocation absent")?;
        ensure!(array.entries.len()==count as usize,"Traversal constructor exceeds its allocation");
        array.constructed=true;Ok(Some(handle))
    }
    fn read_entry(&mut self,_:&mut State,array:usize,index:u32)->Result<Option<usize>> {self.entry(array,index)}
    fn write_entry(&mut self,_:&mut State,array:usize,index:u32,pool:Option<usize>)->Result<()> {
        let array=self.arrays.get_mut(&array).context("Traversal array absent")?;
        ensure!(array.constructed,"Traversal array not constructed");
        *array.entries.get_mut(index as usize).context("Traversal write outside allocated array")?=Some(pool);
        Ok(())
    }
    fn release_array(&mut self,_:&mut State,array:usize)->Result<()> {
        self.arrays.remove(&array).context("Traversal array release owner absent")?;
        //4792b0 does not destroy the pool referenced by its entry.
        Ok(())
    }
}

#[derive(Default,Debug)]
pub struct Manager {
    pub state:State,
    pub storage:Storage,
    pub unpooled:Vec<Instance>,
}
pub trait ConstructionHost {
    fn create(&mut self,manager:&mut Manager,pool:&mut Pool,metadata:usize)->Result<Instance>;
    fn initialize(&mut self,manager:&mut Manager,pool:&mut Pool,instance:&mut Instance,configuration:usize)->Result<bool>;
    fn destroy(&mut self,manager:&mut Manager,pool:&mut Pool,instance:Instance)->Result<()>;
}
struct InstanceHost<'a,H> {manager:&'a mut Manager,host:&'a mut H}
impl<H:ConstructionHost> super::ai_traversal_instances::Host for InstanceHost<'_,H> {
    fn create(&mut self,pool:&mut Pool,metadata:usize)->Result<Instance>{self.host.create(self.manager,pool,metadata)}
    fn initialize(&mut self,pool:&mut Pool,instance:&mut Instance,configuration:usize)->Result<bool>{self.host.initialize(self.manager,pool,instance,configuration)}
    fn destroy(&mut self,pool:&mut Pool,instance:Instance)->Result<()>{self.host.destroy(self.manager,pool,instance)}
    fn traversal_count(&mut self)->Result<u32>{Ok(self.manager.state.count)}
}
impl Manager {
    pub(super) fn pool_handle_by_index(&self,index:u32)->Result<usize> {
        let array=self.state.array.context("Traversal acquisition has no table")?;
        self.storage.entry(array,index)?.context("Traversal acquisition has a null pool")
    }
    pub fn construct_and_publish(&mut self,name:&[u8],count:u32,metadata:usize,configuration:usize,host:&mut impl ConstructionHost)->Result<usize> {
        let mut pool=Pool::new(count)?;
        let unpooled=pool.construct(count,metadata,configuration,&mut InstanceHost{manager:self,host})?;
        self.unpooled.extend(unpooled);
        let handle=self.storage.add_pool(name,pool)?;
        self.state.publish(handle,&mut self.storage)?;
        Ok(handle)
    }
    pub fn find(&self,name:Option<&[u8]>)->Result<u32> {
        let Some(name)=name else {return Ok(u32::MAX)};
        for index in 0..self.state.count {
            let array=self.state.array.context("Traversal lookup has no table")?;
            let handle=self.storage.entry(array,index)?.context("Traversal lookup has a null pool")?;
            if super::ai_traversal_registry::names_equal(&self.storage.pool(handle)?.name,name) {return Ok(index)};
        }
        Ok(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a> {case:&'a Value,trace:Vec<Value>,arrays:BTreeMap<usize,Vec<Option<usize>>>}
    impl Fixture<'_> {
        fn mutate(&mut self,state:&mut State,phase:&str) {
            if let Some(values)=self.case[phase].as_object() {
                for (key,value) in values {
                    match key.as_str(){
                        "array"=>state.array=value.as_u64().map(|v|v as usize),
                        "capacity"=>state.capacity=value.as_u64().unwrap() as u32,
                        "count"=>state.count=value.as_u64().unwrap() as u32,
                        _=>panic!("Unknown fixture mutation {key}"),
                    }
                }
            }
        }
    }
    impl PublicationHost for Fixture<'_> {
        fn allocate(&mut self,state:&mut State,bytes:u32)->Result<Option<usize>> {
            self.trace.push(json!(["allocate",bytes,state.capacity,state.count,state.array]));
            if self.case["observer"].as_bool().unwrap_or(false) {
                self.trace.push(json!(["observer",2,bytes,123]));self.mutate(state,"observer_mutation");
            }
            Ok(Some(2))
        }
        fn construct_array(&mut self,_:&mut State,allocation:Option<usize>,count:u32)->Result<Option<usize>> {
            self.trace.push(json!(["construct",allocation,count]));self.arrays.insert(2,vec![None;count as usize]);Ok(Some(2))
        }
        fn read_entry(&mut self,_:&mut State,array:usize,index:u32)->Result<Option<usize>> {
            let value=self.arrays[&array][index as usize];self.trace.push(json!(["read",array,index,value]));Ok(value)
        }
        fn write_entry(&mut self,_:&mut State,array:usize,index:u32,pool:Option<usize>)->Result<()> {
            self.trace.push(json!(["write",array,index,pool]));self.arrays.get_mut(&array).unwrap()[index as usize]=pool;Ok(())
        }
        fn release_array(&mut self,state:&mut State,array:usize)->Result<()> {
            self.trace.push(json!(["release",array]));self.arrays.remove(&array);self.mutate(state,"release_mutation");Ok(())
        }
    }
    #[test]
    fn original_traversal_publication_growth_copy_and_callback_order_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_manager.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];
            let mut state=State{array:c["old"].as_bool().unwrap_or(true).then_some(1),capacity:c["capacity"].as_u64().unwrap() as u32,count:c["count"].as_u64().unwrap() as u32};
            let mut host=Fixture{case:c,trace:vec![],arrays:[(1,100),(3,200)].into_iter().map(|(h,start)|
                (h,(0..16).map(|i|if c["null_entry"].as_u64()==Some(i){None}else{Some(start+i as usize)}).collect())).collect()};
            state.publish(99,&mut host)?;
            assert_eq!(json!(host.trace),expected["trace"],"publication trace: {c}");
            assert_eq!(json!(state),expected["state"],"publication state: {c}");
            assert_eq!(json!(&host.arrays[&state.array.unwrap()][..state.count as usize]),expected["entries"],"copied entries: {c}");
        }
        Ok(())
    }

    #[derive(Default)]
    struct Factory {created:usize,destroyed:Vec<usize>,nested:bool,reentered:bool}
    impl ConstructionHost for Factory {
        fn create(&mut self,_:&mut Manager,_:&mut Pool,_:usize)->Result<Instance> {
            let handle=100+self.created;self.created+=1;Ok(Instance{handle,traversal_id:None,lease:None,concrete:None})
        }
        fn initialize(&mut self,manager:&mut Manager,_:&mut Pool,_:&mut Instance,_:usize)->Result<bool> {
            if self.nested&&!self.reentered {
                self.reentered=true;
                manager.construct_and_publish(b"Nested",1,9,8,self)?;
            }
            Ok(true)
        }
        fn destroy(&mut self,_:&mut Manager,_:&mut Pool,instance:Instance)->Result<()> {self.destroyed.push(instance.handle);Ok(())}
    }
    #[test]
    fn constructed_pools_publish_find_and_survive_multiple_table_replacements()->Result<()> {
        let mut manager=Manager::default();let mut factory=Factory::default();let mut handles=vec![];
        for i in 0..9 {
            let name=format!("Traversal{i}");
            let handle=manager.construct_and_publish(name.as_bytes(),if i==4 {0}else{2},9,8,&mut factory)?;
            handles.push(handle);
            assert_eq!(manager.find(Some(name.to_ascii_uppercase().as_bytes()))?,i);
        }
        assert_eq!((manager.state.capacity,manager.state.count),(16,9));
        assert_eq!(manager.storage.arrays.len(),1,"Retired tables were retained");
        assert_eq!(manager.storage.pools.len(),9,"Pool ownership changed during table growth");
        assert_eq!(manager.find(None)?,u32::MAX);assert_eq!(manager.find(Some(b"missing"))?,u32::MAX);
        for (i,handle) in handles.into_iter().enumerate() {
            let pool=&manager.storage.pool(handle)?.instances;
            assert_eq!(pool.active_count,if i==4 {0}else{2});
            for slot in &pool.slots {assert_eq!(slot.instance.as_ref().unwrap().traversal_id,Some(i as u32));}
        }
        assert!(factory.destroyed.is_empty());assert!(manager.unpooled.is_empty());
        Ok(())
    }
    #[test]
    fn reentrant_construction_uses_the_actual_manager_count_after_initialization()->Result<()> {
        let mut manager=Manager::default();let mut factory=Factory{nested:true,..Factory::default()};
        let outer=manager.construct_and_publish(b"Outer",2,9,8,&mut factory)?;
        assert_eq!(manager.find(Some(b"nested"))?,0);assert_eq!(manager.find(Some(b"outer"))?,1);
        for slot in &manager.storage.pool(outer)?.instances.slots {
            assert_eq!(slot.instance.as_ref().unwrap().traversal_id,Some(1));
        }
        assert_eq!(manager.state.count,2);
        Ok(())
    }
    #[test]
    fn publication_without_backing_storage_is_an_error_not_a_ready_manager()->Result<()> {
        let mut state=State{array:None,capacity:4,count:0};let mut storage=Storage::default();
        assert!(state.publish(1,&mut storage).is_err());assert_eq!(state.count,0);
        Ok(())
    }
}
