//! Instance-construction loop479d04..479e5c. Metadata factories and virtual
//! initialization/destruction are mandatory; final manager publication is
//! separate. Slots preserve the original free/active intrusive-list ordering.
use anyhow::{Context as _,Result};

#[derive(Debug,serde::Serialize)]
#[serde(tag="class",content="fields")]
pub enum Concrete {
    #[serde(rename="JPSGotoTraversal")]
    Goto(super::ai_goto_traversal::State),
    #[serde(rename="JPSWanderTraversal")]
    Wander(super::ai_wander_traversal::State),
}

impl Concrete {
    pub fn astar(&self)->&super::ai_goto_traversal::State {match self{Self::Goto(s)=>s,Self::Wander(s)=>&s.base}}
    pub fn astar_mut(&mut self)->&mut super::ai_goto_traversal::State {match self{Self::Goto(s)=>s,Self::Wander(s)=>&mut s.base}}
    pub fn wander_mut(&mut self)->Result<&mut super::ai_wander_traversal::State>{match self{Self::Wander(s)=>Ok(s),Self::Goto(_)=>anyhow::bail!("Expected actual Wander traversal")}}
}
pub(super) fn construct_astar(handle:usize,concrete:Concrete)->Instance {
    Instance{handle,traversal_id:Some(u32::MAX),lease:Some(super::ai_traversal_acquisition::Lease{owner_key:0,reuse_count:0,clock_bits:0,field_20:Some(2)}),concrete:Some(Box::new(concrete))}
}
#[derive(Debug,serde::Serialize)]
pub struct Instance {
    pub handle:usize,
    pub traversal_id:Option<u32>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub lease:Option<super::ai_traversal_acquisition::Lease>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub concrete:Option<Box<Concrete>>,
}
#[derive(Debug,serde::Serialize)]
pub struct Slot {
    pub instance:Option<Instance>,
    pub previous:Option<usize>,
    pub next:Option<usize>,
    pub active:bool,
}
#[derive(Debug,serde::Serialize)]
pub struct Pool {
    pub slots:Vec<Slot>,
    pub free_head:Option<usize>,
    pub active_head:Option<usize>,
    pub active_tail:Option<usize>,
    pub active_count:u32,
}
pub trait Host {
    fn create(&mut self,pool:&mut Pool,metadata:usize)->Result<Instance>;
    fn initialize(&mut self,pool:&mut Pool,instance:&mut Instance,configuration:usize)->Result<bool>;
    fn destroy(&mut self,pool:&mut Pool,instance:Instance)->Result<()>;
    fn traversal_count(&mut self)->Result<u32>;
}
impl Pool {
    ///47c458 initializes a native0x14-byte slot for each requested instance.
    pub fn new(count:u32)->Result<Self> {
        count.checked_mul(0x14).and_then(|n|n.checked_add(0x10))
            .context("Traversal pool size overflows its native allocation domain")?;
        let count=count as usize;
        let mut slots=Vec::new();slots.try_reserve_exact(count)?;
        for i in 0..count {slots.push(Slot{instance:None,previous:i.checked_sub(1),
            next:(i+1<count).then_some(i+1),active:false});}
        Ok(Self {slots,free_head:(count!=0).then_some(0),active_head:None,active_tail:None,active_count:0})
    }
    pub fn insert(&mut self,instance:Instance)->Result<Option<Instance>> {
        let Some(index)=self.free_head else {return Ok(Some(instance))};
        let slot=self.slots.get_mut(index).context("Traversal free-list slot absent")?;
        self.free_head=slot.next;
        slot.instance=Some(instance);slot.active=true;
        self.active_count=self.active_count.wrapping_add(1);
        slot.previous=self.active_tail;slot.next=None;
        if let Some(tail)=self.active_tail {
            self.slots.get_mut(tail).context("Traversal active-list tail absent")?.next=Some(index);
        } else {self.active_head=Some(index);}
        self.active_tail=Some(index);
        Ok(None)
    }
    /// Native failures destroy the failed instance and continue to the next
    /// requested instance. Successful instances read the manager count AFTER
    /// initialization. A full pool does not destroy the initialized object;
    /// return such objects explicitly instead of silently losing ownership.
    pub fn construct(&mut self,requested:u32,metadata:usize,configuration:usize,host:&mut impl Host)->Result<Vec<Instance>> {
        let mut unpooled=Vec::new();
        for _ in 0..requested {
            let mut instance=host.create(self,metadata)?;
            if !host.initialize(self,&mut instance,configuration)? {
                host.destroy(self,instance)?;
                continue;
            }
            instance.traversal_id=Some(host.traversal_count()?);
            if let Some(instance)=self.insert(instance)? {unpooled.push(instance);}
        }
        Ok(unpooled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Fixture<'a> {case:&'a Value,trace:Vec<Value>,created:usize,counter:u32}
    impl Fixture<'_> {fn flag(&self,key:&str)->bool{self.case[key].as_bool().unwrap_or(false)}}
    impl Host for Fixture<'_> {
        fn create(&mut self,_:&mut Pool,metadata:usize)->Result<Instance> {
            self.trace.push(json!(["create",metadata]));let handle=100+self.created;self.created+=1;
            Ok(Instance{handle,traversal_id:Some(0xdecafbad),lease:None,concrete:None})
        }
        fn initialize(&mut self,pool:&mut Pool,instance:&mut Instance,configuration:usize)->Result<bool> {
            let index=instance.handle-100;self.trace.push(json!(["initialize",instance.handle,configuration]));
            if self.flag("increment_count"){self.counter=self.counter.wrapping_add(1);}
            if self.case["exhaust_after_initialize"].as_u64()==Some(index as u64){pool.free_head=None;}
            Ok(self.case["success"][index].as_bool().unwrap())
        }
        fn destroy(&mut self,_:&mut Pool,instance:Instance)->Result<()> {
            self.trace.push(json!(["destroy",instance.handle,instance.traversal_id]));
            if self.flag("destroy_increment"){self.counter=self.counter.wrapping_add(1);}
            Ok(())
        }
        fn traversal_count(&mut self)->Result<u32> {self.trace.push(json!(["traversal_count",self.counter]));Ok(self.counter)}
    }
    #[test]
    fn original_instance_registration_loop_matches_partial_failures_ids_lists_and_unpooled_objects()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_instances.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let count=c["success"].as_array().unwrap().len() as u32;
            let mut pool=Pool::new(count)?;
            let mut host=Fixture{case:c,trace:vec![],created:0,counter:c["counter"].as_u64().unwrap() as u32};
            let unpooled=pool.construct(count,9,8,&mut host)?;
            assert_eq!(json!(host.trace),expected["trace"],"service trace: {c}");
            assert_eq!(json!(pool),expected["pool"],"pool lists: {c}");
            assert_eq!(json!(unpooled),expected["unpooled"],"initialized but unpooled: {c}");
            assert_eq!(json!(host.counter),expected["counter"],"manager state: {c}");
        }
        Ok(())
    }
}
