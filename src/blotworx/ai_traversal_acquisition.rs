//! Traversal acquisition47c968 and wrapper47c8f0. Budget services and the
//! context clock remain mandatory, not default elapsed-time approximations.
use anyhow::{ensure,Context as _,Result};
use super::{ai_traversal_instances::Pool,ai_traversal_manager::Manager};

#[derive(Debug,serde::Serialize)]
pub struct Lease {
    pub owner_key:u32, //24
    pub reuse_count:i32, //28
    pub clock_bits:u32, //2c
    pub field_20:Option<u32>,
}
pub trait Host {
    fn prepare_mode0(&mut self,pool:&mut Pool,first:usize,owner_key:u32)->Result<u32>;
    fn prepare_mode1(&mut self,pool:&mut Pool,first:usize,owner_key:u32)->Result<u32>;
    fn clock_50(&mut self,pool:&mut Pool)->Result<f32>;
    fn clock_58(&mut self,pool:&mut Pool)->Result<f32>;
    fn post_acquire(&mut self,pool:&mut Pool,instance:usize,owner_key:u32,amount:f32)->Result<u32>;
}
pub(super) struct Selection {pub slot:usize,pub handle:usize}
impl Pool {
    fn lease(&self,index:usize)->Result<&Lease> {
        self.slots.get(index).context("Traversal acquisition slot absent")?.instance.as_ref()
            .context("Traversal acquisition slot has no instance")?.lease.as_ref()
            .context("Traversal acquisition requires initialized lease fields")
    }
    fn handle(&self,index:usize)->Result<usize> {
        Ok(self.slots.get(index).context("Traversal acquisition slot absent")?.instance.as_ref()
            .context("Traversal acquisition slot has no instance")?.handle)
    }
    fn selected_lease(&mut self,selected:&Selection)->Result<&mut Lease> {
        let instance=self.slots.get_mut(selected.slot).context("Selected traversal slot absent")?
            .instance.as_mut().context("Selected traversal instance absent")?;
        ensure!(instance.handle==selected.handle,"Selected traversal was replaced during service callback");
        instance.lease.as_mut().context("Selected traversal lease fields absent")
    }
    pub(super) fn acquire_kernel(&mut self,key:u32,mode:u32,host:&mut impl Host)->Result<Option<Selection>> {
        let initial=self.active_head;
        if let Some(first)=initial {
            let handle=self.handle(first)?;
            match mode {0=>{let _=host.prepare_mode0(self,handle,key)?;},1=>{let _=host.prepare_mode1(self,handle,key)?;},_=>{}}
        }
        let mut cursor=initial;let mut steps=0;
        while let Some(index)=cursor {
            ensure!(steps<self.slots.len(),"Cyclic traversal acquisition list");steps+=1;
            if self.lease(index)?.owner_key==key {
                let selected=Selection{slot:index,handle:self.handle(index)?};
                let now=host.clock_50(self)?;
                let lease=self.selected_lease(&selected)?;
                if now!=f32::from_bits(lease.clock_bits) {
                    if lease.reuse_count!=-1 {lease.reuse_count=lease.reuse_count.wrapping_add(1);}
                    lease.clock_bits=now.to_bits();
                }
                return Ok(Some(selected));
            }
            cursor=self.slots[index].next;
        }
        let cutoff=host.clock_58(self)?;
        let mut chosen=None;cursor=self.active_head;steps=0;
        while let Some(index)=cursor {
            ensure!(steps<self.slots.len(),"Cyclic traversal candidate list");steps+=1;
            let lease=self.lease(index)?;
            if lease.owner_key==0 || f32::from_bits(lease.clock_bits)<cutoff {chosen=Some(index);break;}
            if lease.reuse_count>4 {
                let replace=match chosen {None=>true,Some(old)=>self.lease(old)?.reuse_count<lease.reuse_count};
                if replace {chosen=Some(index);}
            }
            cursor=self.slots[index].next;
        }
        let Some(index)=chosen else {return Ok(None)};
        let selected=Selection{slot:index,handle:self.handle(index)?};
        let lease=self.selected_lease(&selected)?;lease.owner_key=key;lease.reuse_count=-1;
        let now=host.clock_50(self)?;
        self.selected_lease(&selected)?.clock_bits=now.to_bits();
        Ok(Some(selected))
    }
}
impl Manager {
    pub fn acquire(&mut self,traversal:u32,key:u32,host:&mut impl Host)->Result<Option<usize>> {
        //47c910 rejects strictly greater IDs. Equal count is not rewritten as
        // an early >= rejection; unavailable backing entries remain errors.
        if traversal>self.state.count {return Ok(None)};
        let handle=self.pool_handle_by_index(traversal)?;
        let pool=&mut self.storage.pool_mut(handle)?.instances;
        let Some(selected)=pool.acquire_kernel(key,0,host)? else {return Ok(None)};
        let _=host.post_acquire(pool,selected.handle,key,1.)?;
        pool.selected_lease(&selected)?.field_20=Some(0);
        Ok(Some(selected.handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_traversal_instances::Instance,ai_traversal_manager::{ConstructionHost,PublicationHost}};
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Fixture<'_> {
        fn key(&self)->u32{self.case["key"].as_u64().unwrap_or(77) as u32}
        fn prepare(&mut self,pool:&mut Pool,first:usize,key:u32,kind:&str)->Result<u32> {
            self.trace.push(json!(["prepare_context"]));self.trace.push(json!([kind,first,key]));
            if let Some(head)=self.case["prepare_head"].as_u64(){pool.active_head=Some(head as usize);}
            Ok(self.case["budget_result"].as_u64().unwrap_or(1) as u32)
        }
    }
    impl Host for Fixture<'_> {
        fn prepare_mode0(&mut self,pool:&mut Pool,first:usize,key:u32)->Result<u32>{self.prepare(pool,first,key,"prepare0")}
        fn prepare_mode1(&mut self,pool:&mut Pool,first:usize,key:u32)->Result<u32>{self.prepare(pool,first,key,"prepare1")}
        fn clock_50(&mut self,_:&mut Pool)->Result<f32>{self.trace.push(json!(["clock50"]));Ok(f32::from_bits(self.case["now"].as_u64().unwrap_or(2f32.to_bits() as u64) as u32))}
        fn clock_58(&mut self,_:&mut Pool)->Result<f32>{self.trace.push(json!(["clock58"]));Ok(f32::from_bits(self.case["cutoff"].as_u64().unwrap_or(1.5f32.to_bits() as u64) as u32))}
        fn post_acquire(&mut self,_:&mut Pool,_:usize,key:u32,amount:f32)->Result<u32>{
            self.trace.push(json!(["post_context"]));self.trace.push(json!(["post",key,amount.to_bits()]));
            Ok(self.case["budget_result"].as_u64().unwrap_or(1) as u32)
        }
    }
    fn instance(case:&Value,index:usize)->Instance {
        let r=&case["records"][index];
        Instance{handle:100+index,traversal_id:Some(0),lease:Some(Lease{
            owner_key:r["owner"].as_u64().unwrap() as u32,reuse_count:r["count"].as_i64().unwrap() as i32,
            clock_bits:r["clock"].as_u64().unwrap() as u32,field_20:Some(9)}),concrete:None}
    }
    fn pool(case:&Value)->Result<Pool> {
        let count=case["records"].as_array().unwrap().len();let mut pool=Pool::new(count as u32)?;
        for i in 0..count {assert!(pool.insert(instance(case,i))?.is_none());}
        Ok(pool)
    }
    fn leases(pool:&Pool)->Value {json!(pool.slots.iter().map(|s|s.instance.as_ref().unwrap().lease.as_ref().unwrap()).collect::<Vec<_>>())}
    struct Builder<'a>{case:&'a Value,next:usize}
    impl ConstructionHost for Builder<'_> {
        fn create(&mut self,_:&mut Manager,_:&mut Pool,_:usize)->Result<Instance>{let result=instance(self.case,self.next);self.next+=1;Ok(result)}
        fn initialize(&mut self,_:&mut Manager,_:&mut Pool,_:&mut Instance,_:usize)->Result<bool>{Ok(true)}
        fn destroy(&mut self,_:&mut Manager,_:&mut Pool,_:Instance)->Result<()>{unreachable!("Fixture initialization never fails")}
    }
    #[test]
    fn original_acquisition_scans_clock_words_counters_modes_and_wrapper_writes_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_acquisition.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let mut host=Fixture{case:c,trace:vec![]};let key=host.key();
            let (selected,head,records)=if c["op"]=="kernel" {
                let mut pool=pool(c)?;let selected=pool.acquire_kernel(key,c["mode"].as_u64().unwrap_or(0) as u32,&mut host)?.map(|s|s.handle);
                (selected,pool.active_head,leases(&pool))
            } else {
                let mut manager=Manager::default();let count=c["records"].as_array().unwrap().len() as u32;
                let handle=manager.construct_and_publish(b"Fixture",count,9,8,&mut Builder{case:c,next:0})?;
                // A deliberately initialized spare entry exercises native's
                // strict > count wrapper guard, without uninitialized memory.
                let array=manager.state.array.unwrap();
                manager.storage.write_entry(&mut manager.state,array,1,Some(handle))?;
                let selected=manager.acquire(c["traversal"].as_u64().unwrap_or(0) as u32,key,&mut host)?;
                let pool=&manager.storage.pool(handle)?.instances;
                (selected,pool.active_head,leases(pool))
            };
            assert_eq!(json!(selected),expected["selected"],"selected instance: {c}");
            assert_eq!(json!(head),expected["active_head"],"head publication: {c}");
            assert_eq!(records,expected["leases"],"lease words: {c}");
            assert_eq!(json!(host.trace),expected["trace"],"service calls: {c}");
        }
        Ok(())
    }
    #[test]
    fn uninitialized_lease_and_cyclic_list_are_errors_not_available_instances()->Result<()> {
        let c=json!({"key":77});let mut host=Fixture{case:&c,trace:vec![]};
        let mut p=Pool::new(1)?;assert!(p.insert(Instance{handle:100,traversal_id:Some(0),lease:None,concrete:None})?.is_none());
        assert!(p.acquire_kernel(77,7,&mut host).is_err());
        let c=json!({"records":[{"owner":88,"count":0,"clock":0},{"owner":99,"count":0,"clock":0}]});
        let mut host=Fixture{case:&c,trace:vec![]};let mut p=pool(&c)?;p.slots[0].next=Some(0);
        assert!(p.acquire_kernel(77,7,&mut host).is_err());assert!(host.trace.is_empty());
        Ok(())
    }
}
