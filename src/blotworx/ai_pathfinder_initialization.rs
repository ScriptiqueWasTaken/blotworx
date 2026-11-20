//! Derived48fec4 initialization around the existing LPF, attribute and modifier
//! implementations. Successful initialization is separate from World admission.
use super::{ai_graph_link_passes::Memory, ai_navigation_definitions::Tree,
    ai_pathfinder_constructor::{State,Reference}, ai_pathfinder_modifiers::{self as modifiers,Kind},
    ai_pathfinder_definition_binding as definitions, ai_lpf_definition_binding as lpf};
use anyhow::{ensure,Context,Result};

/// Actual shared6af60 storage and mutable51f2b8 configuration. The caller owns
/// one instance per native manager lifetime, not one per actor/pathfinder.
pub struct Shared {
    pub arrays_2c_30_34:[u32;3],
    pub search_count:u32,
    pub budget_keys_38_5c:[u32;2],
}
pub trait WorkspaceHost {
    /// Includes the original allocation observer; it may change live fields.
    /// Returned nonnull data must be mapped in the existing navigation arena.
    fn allocate(&mut self,state:&mut State,shared:&mut Shared,memory:&mut Memory,bytes:u32)->Result<u32>;
    ///4969ac construction. Overrides are explicit instrumentation boundaries;
    ///the normal implementation constructs all three actual backing buffers.
    fn strategy(&mut self,state:&mut State,shared:&mut Shared,memory:&mut Memory,allocation:u32,args:[u32;3])->Result<u32> where Self:Sized {
        construct_strategy(state,shared,memory,allocation,args,self)
    }
    fn manager(&mut self,state:&mut State,shared:&mut Shared)->Result<usize>;
    fn budget(&mut self,state:&mut State,shared:&mut Shared,manager:usize,key:u32)->Result<i32>;
}
pub trait Backend:WorkspaceHost+definitions::DefaultsBackend+lpf::Backend+super::ai_pathfinder_reset::Host {
    /// Actual virtual14, including all concrete reset side effects.
    fn reset(&mut self,state:&mut State,memory:&mut Memory)->Result<()> where Self:Sized {super::ai_pathfinder_reset::reset(state,memory,self)}
    fn profiling_enabled(&mut self,state:&mut State,manager:usize)->Result<bool>;
    fn register_budget(&mut self,state:&mut State,manager:usize,name:&[u8])->Result<i32>;
}

///4969ac..496b2c and496c10 reset. Lengths passed to the allocator are captured
///arguments; zero-fill lengths are fresh object fields after the observer.
pub fn construct_strategy(state:&mut State,shared:&mut Shared,memory:&mut Memory,p:u32,args:[u32;3],host:&mut impl WorkspaceHost)->Result<u32> {
    memory.write(p.wrapping_add(0x1b0),0)?;
    for (i,(length_offset,pointer_offset)) in [(12,0),(0x1a0,0x19c),(0x1c0,0x1b4)].into_iter().enumerate() {
        memory.write(p.wrapping_add(length_offset),args[i])?;
        let data=host.allocate(state,shared,memory,args[i])?;
        memory.write(p.wrapping_add(pointer_offset),data)?;
        let length=memory.word(p.wrapping_add(length_offset))?;
        memory.fill(data,length,0)?;
    }
    super::ai_pathfinder_reset::reset_strategy(memory,p)?;
    Ok(p)
}

///48ff08..48ffd4. The actual190d38(1) is zero, so each setter loop runs once.
pub fn prepare(state:&mut State,mut unit_scale:impl FnMut(&mut State)->Result<Option<f32>>)->Result<()> {
    state.mode_28=Some(1);
    for (index,factor) in [10f32,50.].into_iter().enumerate() {
        let value=factor*unit_scale(state)?.unwrap_or(1.);
        if value>=0. {state.distances_2c_34[index]=Some(value);}
    }
    state.scalars_3e8_3ec_408_414=[Some(0.1),Some(0.2),Some(1.),Some(1.)];
    state.flags_f4_f5=[1,0];state.scalar_40c=Some(0.);state.precision_cc=Some(0);
    Ok(())
}
fn array(memory:&mut Memory,allocation:u32,count:u32,stride:u32,constructor:u32)->Result<u32> {
    if allocation==0{return Ok(0)}
    memory.write(allocation,stride)?;memory.write(allocation.wrapping_add(4),count)?;
    let base=allocation.wrapping_add(16);
    for i in 0..count {
        let p=base.wrapping_add(i.wrapping_mul(stride));
        match constructor {
            0x4923c8=>{
                memory.write(p,0x51fc28)?;memory.write(p.wrapping_add(0x20),0x51dd48)?;
                memory.write(p.wrapping_add(0x24),0)?;memory.write(p.wrapping_add(0x28),0)?;
                for offset in [0x2c,0x30,0x34]{memory.write(p.wrapping_add(offset),f32::MAX.to_bits())?;}
                memory.write(p.wrapping_add(0x38),0x4ebe74)?;memory.write(p.wrapping_add(0x3c),0)?;
                memory.write(p.wrapping_add(0x40),0x4ebe68)?;memory.write(p.wrapping_add(0x44),0)?;
            }
            0x4923b0=>for offset in [0x110,0x114,0x118]{memory.write(p.wrapping_add(offset),0)?;},
            0x492388=>{
                memory.write(p,0x51f770)?;
                for offset in [4,8,12,16]{memory.write(p.wrapping_add(offset),0)?;}
                memory.write_byte(p.wrapping_add(20),0)?;
            }
            _=>anyhow::bail!("Unknown pathfinder array constructor"),
        }
    }
    Ok(base)
}
///4900a0..490568. All buffers use the same mapped arena as graph/LPF queries.
pub fn workspace(state:&mut State,shared:&mut Shared,memory:&mut Memory,host:&mut impl WorkspaceHost)->Result<()> {
    let bytes=state.word_38.wrapping_add(1).wrapping_mul(4);
    let p=host.allocate(state,shared,memory,bytes)?;
    state.references_3c_4c_50_54_58_60_6c[6]=(p!=0).then_some(p as usize);
    let mut index=0u32;
    while index<state.word_38.wrapping_add(1) {
        memory.write(p.wrapping_add(index.wrapping_mul(4)),0)?;
        index=index.wrapping_add(1);
    }
    let bytes=state.words_90_94_98[1];let p=host.allocate(state,shared,memory,bytes)?;
    state.words_90_94_98[0]=p;memory.write(p,state.words_90_94_98[1])?;
    if shared.arrays_2c_30_34[0]==0 {
        let count=shared.search_count;
        let p=host.allocate(state,shared,memory,count.wrapping_mul(0x48).wrapping_add(16))?;
        shared.arrays_2c_30_34[0]=array(memory,p,count,0x48,0x4923c8)?;
        for i in 1..3 {
            let bytes=shared.search_count.wrapping_mul(4);
            shared.arrays_2c_30_34[i]=host.allocate(state,shared,memory,bytes)?;
        }
    }
    if state.mode_28==Some(2) {
        let p=host.allocate(state,shared,memory,0x1c4)?;
        let p=if p==0{0}else{host.strategy(state,shared,memory,p,[0x4000,0x3000,0x50000])?};
        state.references_3c_4c_50_54_58_60_6c[5]=(p!=0).then_some(p as usize);
        for i in 1..3 {
            let count=state.words_a4_a8_ac[0];
            let p=host.allocate(state,shared,memory,count.wrapping_mul(0x120).wrapping_add(16))?;
            state.words_a4_a8_ac[i]=array(memory,p,count,0x120,0x4923b0)?;
        }
        let mut index=0u32;
        while index<state.words_a4_a8_ac[0] {
            for i in 1..3 {
                let p=state.words_a4_a8_ac[i].wrapping_add(index.wrapping_mul(0x120));
                memory.write(p.wrapping_add(4),0)?;memory.write_byte(p.wrapping_add(8),0)?;
                memory.write(p,0)?;memory.write(p.wrapping_add(0x10c),0)?;memory.write(p.wrapping_add(0x11c),0)?;
            }
            index=index.wrapping_add(1);
        }
    }else{
        // Source captures both products before the allocator/observer.
        let args=[1,state.word_30.wrapping_mul(0x1c),state.word_30.wrapping_mul(0x14)];
        let p=host.allocate(state,shared,memory,0x1c4)?;
        let p=if p==0{0}else{host.strategy(state,shared,memory,p,args)?};
        state.references_3c_4c_50_54_58_60_6c[5]=(p!=0).then_some(p as usize);
    }
    let manager=WorkspaceHost::manager(host,state,shared)?;
    let key=shared.budget_keys_38_5c[0];state.word_ec=host.budget(state,shared,manager,key)?;
    let manager=host.manager(state,shared)?;
    let key=shared.budget_keys_38_5c[1];state.word_3dc=Some(host.budget(state,shared,manager,key)?);
    Ok(())
}
fn belongs(state:&mut State,kind:Kind,object:usize,host:&mut impl definitions::DefaultsBackend)->Result<bool> {
    let wanted=host.default_class(kind)?;
    let mut class=host.object_class(state,kind,object)?;
    let mut visited=std::collections::BTreeSet::new();
    loop {
        if class==wanted{return Ok(true)}
        ensure!(visited.insert(class),"Cyclic pathfinder class metadata");
        let Some(parent)=host.parent_class(state,class)? else {return Ok(false)};
        class=parent;
    }
}
///4920d4..49235c, after default-modifier selection. Only the actual LPF
///CanGo table requires a matching class; a selected different class is legal.
pub fn finish(state:&mut State,shared:&mut Shared,memory:&mut Memory,host:&mut impl Backend)->Result<bool> {
 finish_with_services(state,shared,memory,host,&mut LegacyFinish)
}
trait FinishServices<H:Backend>{
 fn belongs(&mut self,state:&mut State,memory:&mut Memory,kind:Kind,object:usize,host:&mut H)->Result<bool>;
 fn reset(&mut self,state:&mut State,memory:&mut Memory,host:&mut H)->Result<()>;
}
struct LegacyFinish;
impl<H:Backend> FinishServices<H> for LegacyFinish{
 fn belongs(&mut self,s:&mut State,_:&mut Memory,k:Kind,o:usize,h:&mut H)->Result<bool>{belongs(s,k,o,h)}
 fn reset(&mut self,s:&mut State,m:&mut Memory,h:&mut H)->Result<()>{h.reset(s,m)}
}
struct ConcreteFinish<'a>{metadata:&'a super::ai_pathfinder_concrete_modifiers::Metadata,objects:&'a mut std::collections::BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,spatial:super::ai_spatial_construction::Context}
impl<H:Backend+super::ai_pathfinder_concrete_modifiers::Backend> FinishServices<H> for ConcreteFinish<'_>{
 fn belongs(&mut self,s:&mut State,m:&mut Memory,k:Kind,o:usize,h:&mut H)->Result<bool>{
  belongs(s,k,o,&mut super::ai_pathfinder_concrete_modifiers::Binding{metadata:self.metadata,objects:self.objects,memory:m,spatial:self.spatial,backend:h})
 }
 fn reset(&mut self,s:&mut State,m:&mut Memory,h:&mut H)->Result<()>{super::ai_pathfinder_concrete_modifiers::reset_owned(s,m,self.objects,h)}
}
fn finish_with_services<H:Backend>(state:&mut State,shared:&mut Shared,memory:&mut Memory,host:&mut H,services:&mut impl FinishServices<H>)->Result<bool> {
    // Actual vtable51fb58+40 is18dd48: SET base+c to the inline+3e0
    //object, returning1 (discarded). It does not fill/replace that object.
    state.base.references_c_10[0]=Some(Some(Reference::InlineAcceptance));
    if state.reference_f8.flatten().is_some() {
        let kind=Kind(3);let mut index=0;let mut found=false;
        while index<state.tables_fc_1fc[kind.table()].count {
            let object=state.tables_fc_1fc[kind.table()].storage.as_ref().context("CanGo table allocation absent")?
                .slots.get(index as usize).copied().flatten().context("CanGo table slot unwritten")?;
            if services.belongs(state,memory,kind,object,host)?{found=true;break}
            index=index.wrapping_add(1);
        }
        if !found{return Ok(false)}
        let object=state.tables_fc_1fc[kind.table()].selected.flatten().context("LPF selected CanGo is null")?;
        let _=services.belongs(state,memory,kind,object,host)?;
    }
    let pool=host.allocate(state,shared,memory,0x1c)?;
    if pool!=0 {
        let count=state.word_42c.context("Pathfinder pool count unwritten")?;
        memory.write(pool,0x51fef0)?;
        for offset in [8,0x14]{memory.write(pool.wrapping_add(offset),0)?;}
        memory.write(pool.wrapping_add(0x18),count)?;
        for offset in [0x10,4,12]{memory.write(pool.wrapping_add(offset),0)?;}
        if count!=0 {
            let p=host.allocate(state,shared,memory,count.wrapping_mul(0x18).wrapping_add(16))?;
            let nodes=array(memory,p,count,0x18,0x492388)?;
            memory.write(pool.wrapping_add(4),nodes)?;memory.write(pool.wrapping_add(0x10),nodes)?;
            memory.write(nodes.wrapping_add(12),0)?;
            let mut i=1u32;
            while i<memory.word(pool.wrapping_add(0x18))? {
                let base=memory.word(pool.wrapping_add(4))?;
                let node=base.wrapping_add(i.wrapping_mul(0x18));
                memory.write(node.wrapping_add(12),base.wrapping_add(i.wrapping_sub(1).wrapping_mul(0x18)))?;
                let node=memory.word(pool.wrapping_add(4))?.wrapping_add(i.wrapping_mul(0x18));
                let previous=memory.word(node.wrapping_add(12))?;memory.write(previous.wrapping_add(16),node)?;
                i=i.wrapping_add(1);
            }
            let count=memory.word(pool.wrapping_add(0x18))?;
            let base=memory.word(pool.wrapping_add(4))?;
            memory.write(base.wrapping_add(count.wrapping_sub(1).wrapping_mul(0x18)).wrapping_add(16),0)?;
        }
    }
    state.word_430=Some(pool);state.word_364=Some(0);services.reset(state,memory,host)?;
    let manager=WorkspaceHost::manager(host,state,shared)?;
    if host.profiling_enabled(state,manager)? {
        let manager=WorkspaceHost::manager(host,state,shared)?;
        state.word_44=Backend::register_budget(host,state,manager,b"MpdAstarLoop")?;
    }
    Ok(true)
}
/// Complete enclosing48fec4 control. All concrete strategy/defaults, graph
///services, reset and profiling operations are mandatory backend methods.
///This does not register a brain or admit an agent to World scheduling.
pub fn initialize(state:&mut State,tree:&mut Tree,root:usize,shared:&mut Shared,memory:&mut Memory,host:&mut impl Backend)->Result<bool> {
    prepare(state,|s|definitions::Backend::unit_scale(host,s))?;
    if !state.initialize_lpf_definition(tree,root,host)?{return Ok(false)}
    if !definitions::Binding::new(tree,host).initialize(state,root)?{return Ok(false)}
    workspace(state,shared,memory,host)?;
    modifiers::initialize_defaults(state,&mut definitions::Binding::new(tree,host))?;
    finish(state,shared,memory,host)
}

/// Full48fec4 sequence using canonical owned concrete modifiers and one shared
/// arena. Every required factory/service must exist; unsupported classes error
/// rather than publishing a partially initialized/fictional navigation agent.
pub fn initialize_concrete<H:Backend+super::ai_pathfinder_concrete_modifiers::Backend>(
 state:&mut State,tree:&mut Tree,root:usize,shared:&mut Shared,memory:&mut Memory,
 metadata:&super::ai_pathfinder_concrete_modifiers::Metadata,
 objects:&mut std::collections::BTreeMap<usize,super::ai_pathfinder_modifier_factory::Owned>,
 spatial:super::ai_spatial_construction::Context,host:&mut H)->Result<bool>{
 prepare(state,|state|super::ai_pathfinder_concrete_modifiers::Backend::pathfinder_scale(host,state))?;
 if !state.initialize_lpf_definition(tree,root,host)?{return Ok(false)}
 {
  let mut binding=super::ai_pathfinder_concrete_modifiers::Binding{metadata,objects,memory,spatial,backend:host};
  if !binding.configure(state,tree,root)?{return Ok(false)}
 }
 workspace(state,shared,memory,host)?;
 {
  let mut binding=super::ai_pathfinder_concrete_modifiers::Binding{metadata,objects,memory,spatial,backend:host};
  binding.initialize_defaults(state,tree)?;
 }
 finish_with_services(state,shared,memory,host,&mut ConcreteFinish{metadata,objects,spatial})
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_pathfinder_modifiers::Storage,ai_navigation_definitions::Node};
    use serde_json::{Value,json};
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>,allocations:Vec<u32>,next_allocation:u32,scale_reads:usize,managers:usize,errno:i32}
    impl Fixture<'_> {
        fn scale(&mut self)->Option<f32> {
            let scales=self.case["scales"].as_array().unwrap();
            let value=&scales[self.scale_reads.min(scales.len()-1)];self.scale_reads+=1;
            value.as_u64().map(|v|f32::from_bits(v as u32))
        }
    }
    impl WorkspaceHost for Fixture<'_> {
        fn allocate(&mut self,s:&mut State,shared:&mut Shared,m:&mut Memory,bytes:u32)->Result<u32> {
            let index=self.allocations.len();
            let allocation=self.next_allocation;
            self.next_allocation=self.next_allocation.checked_add((bytes.checked_add(0xffff).context("Fixture allocation overflow")? & !0xffffu32).max(0x10000)).context("Fixture address overflow")?;
            let p=if self.case["fail_allocation"].as_u64()==Some(index as u64){0}else{allocation};
            self.trace.push(json!(["allocate",bytes,p]));self.allocations.push(p);
            if p!=0 {ensure!(m.regions.insert(p,vec![0xcd;bytes as usize]).is_none(),"Live allocation reused");}
            match self.case["mutation"].as_str() {
                Some("considered") if index==0=>s.word_38=1,
                Some("raw_size") if index==1=>s.words_90_94_98[1]=12,
                Some("shared_count") if index==2=>shared.search_count=2,
                Some("strategy")=>s.word_30=9,
                Some("hier_count") if bytes==0x370=>s.words_a4_a8_ac[0]=2,
                Some("pool_count") if bytes==0x1c=>s.word_42c=Some(2),
                Some(name) if name==format!("strategy_shrink{index}")=>m.write(0x72000000+[12,0x1a0,0x1c0][index],bytes/2)?,
                _=>{},
            }
            Ok(p)
        }
        fn strategy(&mut self,s:&mut State,shared:&mut Shared,m:&mut Memory,p:u32,args:[u32;3])->Result<u32> {
            self.trace.push(json!(["strategy",p,args]));
            if self.case["native_strategy"]==true {construct_strategy(s,shared,m,p,args,self)}else{Ok(p)}
        }
        fn manager(&mut self,_:&mut State,_:&mut Shared)->Result<usize> {
            self.managers+=1;let id=100+self.managers;self.trace.push(json!(["manager",id]));Ok(id)
        }
        fn budget(&mut self,_:&mut State,_:&mut Shared,manager:usize,key:u32)->Result<i32> {
            self.trace.push(json!(["budget",manager,key]));Ok(manager as i32+key as i32)
        }
    }
    impl super::super::ai_pathfinder_reset::Host for Fixture<'_> {
        fn detach_goal(&mut self,_:&mut State,_:&mut Memory,_:u32)->Result<()> {anyhow::bail!("Reset boundary is supplied in enclosing-control reference")}
        fn reset_modifier(&mut self,_:&mut State,_:&mut Memory,_:Kind,_:usize)->Result<()> {anyhow::bail!("Reset boundary is supplied in enclosing-control reference")}
        fn release(&mut self,_:&mut State,_:&mut Memory,_:usize,_:u32)->Result<()> {anyhow::bail!("Reset boundary is supplied in enclosing-control reference")}
    }
    impl Backend for Fixture<'_> {
        fn reset(&mut self,s:&mut State,memory:&mut Memory)->Result<()> {
            if self.case["native_reset"]==true {return super::super::ai_pathfinder_reset::reset(s,memory,self);}
            self.trace.push(json!(["reset",s.word_364,s.word_430]));
            if self.case["mutation"]=="reset_budget" {s.word_44=27;}
            Ok(())
        }
        fn profiling_enabled(&mut self,_:&mut State,_:usize)->Result<bool> {Ok(self.case["profiling"]==true)}
        fn register_budget(&mut self,_:&mut State,manager:usize,name:&[u8])->Result<i32> {
            self.trace.push(json!(["register",manager,std::str::from_utf8(name)?]));Ok(17)
        }
    }
    impl definitions::Backend for Fixture<'_> {
        fn integer(&mut self,_:&mut State,_:Option<&[u8]>)->Result<i32>{anyhow::bail!("Unexpected numeric definition")}
        fn number(&mut self,_:&mut State,_:&[u8])->Result<f64>{anyhow::bail!("Unexpected numeric definition")}
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {Ok(self.scale())}
        fn find_database(&mut self,_:&mut State,_:usize,_:&[u8])->Result<Option<usize>>{anyhow::bail!("No supplied database")}
        fn register_database_callback(&mut self,_:&mut State,_:usize)->Result<()>{anyhow::bail!("No supplied database")}
        fn class(&mut self,_:&mut State,_:Kind,_:&[u8])->Result<Option<usize>>{anyhow::bail!("No class factory supplied")}
        fn construct(&mut self,_:&mut State,_:Kind,_:usize,_:&[u8])->Result<usize>{anyhow::bail!("No constructor supplied")}
        fn initialize_modifier(&mut self,_:&mut State,_:Kind,_:usize,_:&mut Tree,_:usize)->Result<bool>{anyhow::bail!("No modifier constructed")}
        fn object_name(&mut self,_:&mut State,_:Kind,_:usize)->Result<Vec<u8>>{anyhow::bail!("No object-name provider")}
        fn allocate_table(&mut self,_:&mut State,_:Kind,_:u32)->Result<Storage>{anyhow::bail!("Existing defaults should be retained")}
        fn free_table(&mut self,_:&mut State,_:Kind,_:usize)->Result<()>{anyhow::bail!("Existing defaults should be retained")}
    }
    impl definitions::DefaultsBackend for Fixture<'_> {
        fn default_class(&self,kind:Kind)->Result<usize> {ensure!(kind==Kind(3),"Unexpected class query");Ok(83)}
        fn object_class(&mut self,_:&mut State,kind:Kind,object:usize)->Result<usize> {
            self.trace.push(json!(["object_class",kind.0,object]));
            Ok(if self.case["lpf"]=="present" || object==100 && self.case["lpf"]=="selected_other" {83}else{999})
        }
        fn parent_class(&mut self,_:&mut State,_:usize)->Result<Option<usize>> {Ok(None)}
        fn construct_default(&mut self,_:&mut State,_:Kind,_:usize,_:&[u8])->Result<Option<usize>> {anyhow::bail!("No default constructor supplied")}
        fn initialize_default(&mut self,_:&mut State,_:Kind,_:usize,_:&mut Tree)->Result<bool>{anyhow::bail!("No default constructed")}
    }
    impl lpf::Backend for Fixture<'_> {
        fn unit_scale(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<f32>>{anyhow::bail!("LPF control is independently tested")}
        fn manager(&mut self,_:&mut super::super::ai_lpf_module::State)->Result<Option<usize>>{anyhow::bail!("LPF manager not supplied")}
        fn context(&mut self,_:&mut super::super::ai_lpf_module::State,_:Option<usize>,_:&[u8])->Result<Option<usize>>{anyhow::bail!("LPF context not supplied")}
        fn errno(&mut self)->&mut i32{&mut self.errno}
        fn number(&mut self,_:&mut super::super::ai_lpf_module::State,_:&[u8])->Result<f64>{anyhow::bail!("No LPF numeric provider")}
        fn allocate_module(&mut self,_:&mut State)->Result<Option<usize>>{anyhow::bail!("No LPF allocator supplied")}
    }
    fn setup(c:&Value)->Result<(State,Shared,Memory)> {
        let mut s=super::super::ai_pathfinder_modifiers::tests::state()?;
        s.distances_2c_34=[Some(-2.),Some(-3.)];s.word_38=2;s.word_30=3;s.words_90_94_98[1]=16;s.words_a4_a8_ac[0]=3;
        s.word_42c=Some(c["pool_count"].as_u64().unwrap_or(3) as u32);s.word_364=Some(123);s.word_3e0=Some(456);
        for t in &mut s.tables_fc_1fc {t.selected=Some(Some(999));}
        let shared=Shared{arrays_2c_30_34:[if c["shared_cached"]==true {900}else{0},0,0],search_count:3,budget_keys_38_5c:[11,22]};
        let memory=Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        Ok((s,shared,memory))
    }
    fn image(s:&State)->Vec<u32> {
        let mut words=vec![s.mode_28.unwrap(),s.distances_2c_34[0].unwrap().to_bits(),s.distances_2c_34[1].unwrap().to_bits()];
        words.extend(s.scalars_3e8_3ec_408_414.iter().map(|v|v.unwrap().to_bits()));words.push(s.scalar_40c.unwrap().to_bits());
        words.extend([s.references_3c_4c_50_54_58_60_6c[6].unwrap_or(0) as u32,s.words_90_94_98[0],
            s.references_3c_4c_50_54_58_60_6c[5].unwrap_or(0) as u32,s.words_a4_a8_ac[1],s.words_a4_a8_ac[2],s.word_ec as u32,
            s.word_3dc.unwrap() as u32,s.word_3e0.unwrap(),s.word_364.unwrap(),s.word_430.unwrap(),s.word_44 as u32,
            match s.base.references_c_10[0].flatten(){Some(Reference::InlineAcceptance)=>0x720003e0,Some(Reference::External(id))=>id as u32,None=>0}]);words
    }
    fn runs(bytes:&[u8])->Vec<[usize;2]> {
        let mut rows:Vec<[usize;2]>=Vec::new();
        for &byte in bytes {
            if let Some(row)=rows.last_mut().filter(|row|row[0]==byte as usize){row[1]+=1;}
            else{rows.push([byte as usize,1]);}
        }
        rows
    }
    #[test]
    fn pathfinder_strategy_constructor_matches_original_fresh_lengths_and_reset()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_strategy.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let (mut s,mut shared,mut memory)=setup(c)?;
            memory.regions.insert(0x72000000,vec![0xcd;0x1c4]);
            let args=std::array::from_fn(|i|c["args"][i].as_u64().unwrap() as u32);
            let mut host=Fixture{case:c,trace:vec![],allocations:vec![],next_allocation:0x74000000,scale_reads:0,managers:0,errno:0};
            let result=construct_strategy(&mut s,&mut shared,&mut memory,0x72000000,args,&mut host)?;
            let allocations=host.allocations.iter().filter(|&&p|p!=0).map(|&p|json!([p,runs(&memory.regions[&p])])).collect::<Vec<_>>();
            assert_eq!(json!({"result":result,"trace":host.trace,"object":runs(&memory.regions[&0x72000000]),"allocations":allocations}),
                json!({"result":expected["result"],"trace":expected["trace"],"object":expected["object"],"allocations":expected["allocations"]}),"{c}");
        }
        Ok(())
    }
    #[test]
    fn pathfinder_strategy_missing_backing_storage_retains_partial_publication()->Result<()> {
        for index in 0..3 {
            let c=json!({"fail_allocation":index});let (mut s,mut shared,mut memory)=setup(&c)?;
            memory.regions.insert(0x72000000,vec![0xcd;0x1c4]);
            let mut host=Fixture{case:&c,trace:vec![],allocations:vec![],next_allocation:0x74000000,scale_reads:0,managers:0,errno:0};
            assert!(construct_strategy(&mut s,&mut shared,&mut memory,0x72000000,[3,4,5],&mut host).is_err());
            assert_eq!(memory.word(0x72000000+[0,0x19c,0x1b4][index])?,0);
            assert_eq!(memory.word(0x72000004)?,0xcdcdcdcd,"Reset must not run after invalid memset storage");
        }
        Ok(())
    }
    #[test]
    fn pathfinder_enclosing_initialization_matches_original_storage_and_order()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_initialization.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let (mut s,mut shared,mut memory)=setup(c)?;
            let mut host=Fixture{case:c,trace:vec![],allocations:vec![],next_allocation:0x74000000,scale_reads:0,managers:0,errno:0};
            let result=if c["lpf"].is_null() {
                // Actual enclosing entry, actual tree and existing attribute
                // handler. Only the documented backend boundaries are supplied.
                let mut tree=Tree::default();let root=tree.append(None,Node{name:Some(b"Service".to_vec()),..Node::default()})?;
                tree.append(Some(root),Node{name:Some(b"DataMode".to_vec()),value:Some(if c["mode"]==2 {b"Hierarchical".to_vec()}else{b"Flat".to_vec()}),..Node::default()})?;
                initialize(&mut s,&mut tree,root,&mut shared,&mut memory,&mut host)?
            }else{
                // LPF initializer has its own66 source cases. Here supply only
                // the existing handle/table to exercise this method's gates.
                prepare(&mut s,|_|Ok(host.scale()))?;s.mode_28=Some(c["mode"].as_u64().unwrap_or(1) as u32);
                workspace(&mut s,&mut shared,&mut memory,&mut host)?;
                s.reference_f8=Some(Some(33));let table=&mut s.tables_fc_1fc[Kind(3).table()];
                table.count=1;table.capacity=1;table.storage=Some(Storage{identity:123,slots:vec![Some(100)]});
                table.selected=Some(Some(if c["lpf"]=="selected_other" {101}else{100}));
                finish(&mut s,&mut shared,&mut memory,&mut host)?
            };
            if c["native_reset"]==true {
                assert_eq!(s.reset_fields.word_374,Some(-1));
                assert_eq!(s.reset_fields.word_36c,Some(2));
                assert_eq!(s.reset_fields.word_68,Some(0));
                assert_eq!(s.points_200_274_2c4_314_378[1].position,[f32::MAX;3]);
                assert_eq!(s.reset_fields.scalars_410_3f0_404_418[1],Some(-f32::MAX));
            }
            let allocations=host.allocations.iter().filter(|&&p|p!=0).map(|&p|json!([p,memory.regions[&p].iter().map(|v|format!("{v:02x}")).collect::<String>()])).collect::<Vec<_>>();
            assert_eq!(json!({"result":result,"trace":host.trace,"words":image(&s),"bytes":[s.precision_cc.unwrap(),s.flags_f4_f5[0],s.flags_f4_f5[1]],
                "scale_reads":host.scale_reads,"shared":[shared.arrays_2c_30_34[0],shared.arrays_2c_30_34[1],shared.arrays_2c_30_34[2],shared.search_count],"allocations":allocations}),
                json!({"result":expected["result"],"trace":expected["trace"],"words":expected["words"],"bytes":expected["bytes"],"scale_reads":expected["scale_reads"],"shared":expected["shared"],"allocations":expected["allocations"]}),"{c}");
        }
        Ok(())
    }
    #[test]
    fn pathfinder_required_storage_failure_does_not_publish_or_reset()->Result<()> {
        for index in [0,1,7] {
            let c=json!({"fail_allocation":index,"scales":[null]});let (mut s,mut shared,mut memory)=setup(&c)?;
            let mut host=Fixture{case:&c,trace:vec![],allocations:vec![],next_allocation:0x74000000,scale_reads:0,managers:0,errno:0};
            let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
            assert!(initialize(&mut s,&mut tree,root,&mut shared,&mut memory,&mut host).is_err());
            assert!(host.trace.iter().all(|e|e[0]!="reset" && e[0]!="register"));
            assert_eq!(s.word_430,Some(0));assert_eq!(s.word_364,Some(123));
            assert_eq!(s.base.references_c_10[0],Some(if index==7{Some(Reference::InlineAcceptance)}else{None}));
            if index==0 {assert_eq!(s.references_3c_4c_50_54_58_60_6c[6],None);}
            if index==1 {assert_eq!(s.words_90_94_98[0],0);}
        }
        Ok(())
    }
}
