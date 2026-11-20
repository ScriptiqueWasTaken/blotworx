//!1957bc and1c1ca4 initialization over canonical traversal/strategy records.
//! Projection, task admission and goal search remain required lower services.
use anyhow::{Context,Result,ensure};
use super::{ai_graph_link_passes::Memory,ai_agent_path,ai_agent_path_request as request,
    ai_traversal_instances::Instance,ai_flat_handle_strategy::State as Strategy};
pub struct Projection {pub status:u32,pub node:u32}
pub trait Host:request::Host {
    fn random_state(&mut self)->Result<&mut super::game_random::State>;
    fn traversal_mut(&mut self,reservation:u32)->Result<&mut Instance>;
    fn strategy_mut(&mut self,identity:usize)->Result<&mut Strategy>;
    fn initial_position(&mut self,m:&mut Memory,owner:u32)->Result<[f32;3]>;
    fn service_graph(&mut self,m:&mut Memory,service:u32)->Result<Option<usize>>;
    fn service_acceptance(&mut self,m:&mut Memory,service:u32)->Result<u32>;
    fn task_enter(&mut self,m:&mut Memory,manager:u32,task:u32)->Result<()>;
    fn task_leave(&mut self,m:&mut Memory,manager:u32,task:u32)->Result<()>;
    fn project_actor(&mut self,m:&mut Memory,provider:u32,position:[f32;3],service:u32,flags:[u32;4])->Result<Projection>;
    fn select_provider_node(&mut self,m:&mut Memory,provider:u32,node:u32)->Result<()>;
    fn configuration_node(&mut self,m:&mut Memory,configuration:u32)->Result<u32>;
    fn project_goal(&mut self,m:&mut Memory,path:u32,provider:u32,workspace:u32,current_node:u32,goal:[f32;3])->Result<Projection>;
}
fn w(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn vector(m:&Memory,p:u32,o:u32)->Result<[f32;3]>{Ok([f32::from_bits(w(m,p,o)?),f32::from_bits(w(m,p,o+4)?),f32::from_bits(w(m,p,o+8)?)])}
fn state(h:&mut impl Host,r:u32)->Result<&mut super::ai_goto_traversal::State>{
    let instance=h.traversal_mut(r)?;ensure!(instance.handle==r as usize,"Initialized traversal identity differs");
    super::ai_goto_initialization::state_mut(instance)
}
///1957bc. Each policy call observes the original prior writes; queries after
///callbacks read current canonical fields, never a detached object snapshot.
pub fn base(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    let manager=h.request_manager(m)?;let task=h.request_task(m,r)?;
    if h.request_enabled(m,manager,task)? as u8==0{return Ok(false)}
    let service=w(m,p,12)?;let provider=h.service_provider(m,service)?;
    if provider==0 {ai_agent_path::release(m,p,r,h)?;return Ok(false)}
    if h.provider_ready(m,provider)? as u8==0 {ai_agent_path::release(m,p,r,h)?;return Ok(false)}
    let task=h.request_task(m,r)?;h.task_enter(m,manager,task)?;
    let owner=w(m,p,0x7c)?;let position=h.initial_position(m,owner)?;
    let service=w(m,p,12)?;let result=h.project_actor(m,provider,position,service,[1,1,0,0])?;
    let task=h.request_task(m,r)?;h.task_leave(m,manager,task)?;
    if result.status==1 {ai_agent_path::release(m,p,r,h)?;return Ok(false)}
    if result.node==u32::MAX {request::fail(m,p,r,h)?;return Ok(false)}
    let motion=vector(m,p,0x6c)?;let scalar=f32::from_bits(w(m,p,0x78)?);
    state(h,r)?.values_80_8c=[scalar*scalar,motion[0],motion[1],motion[2]];
    let service=w(m,p,12)?;
    let strategy=state(h,r)?.strategy.context("Traversal has no initialized strategy")?;
    h.strategy_mut(strategy)?.owner=Some(service as usize);
    let service=w(m,p,12)?;let graph=h.service_graph(m,service)?;
    let strategy=state(h,r)?.strategy.context("Traversal strategy disappeared")?;
    h.strategy_mut(strategy)?.manager=graph;
    state(h,r)?.reference_6c=Some(w(m,p,0x7c)? as usize);
    let profile=match w(m,p,0x868)? {
        0x4eae38=>w(m,p,0x8ac)?,
        0x4eb980|0x4eb818=>{let service=w(m,p,12)?;h.service_acceptance(m,service)?},
        _=>anyhow::bail!("Initial path profile class is unsupported"),
    };
    state(h,r)?.references_38_3c[0]=(profile!=0).then_some(profile as usize);
    let action=match w(m,p,0x868)? {
        0x4eae38=>p.wrapping_add(0x884),0x4eb818=>p.wrapping_add(0x86c),
        _=>anyhow::bail!("Initial path has no concrete action virtual10"),
    };
    state(h,r)?.references_38_3c[1]=Some(action as usize);m.write(action.wrapping_add(4),r)?;
    h.select_provider_node(m,provider,result.node)?;
    let owner=w(m,p,0x7c)?;let position=h.initial_position(m,owner)?;
    let s=state(h,r)?;s.word_40=result.node as i32;s.vector_44=position;
    Ok(true)
}
///1c1ca4. A failed goal query retains the successful base phase for retry.
pub fn goto(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    if m.byte(p.wrapping_add(0x86c))?==0 {
        let ready=base(m,p,r,h)?;m.write_byte(p.wrapping_add(0x86c),u8::from(ready))?;
    }
    if m.byte(p.wrapping_add(0x86c))?==0{return Ok(false)}
    let manager=h.request_manager(m)?;let task=h.request_task(m,r)?;
    if h.request_enabled(m,manager,task)? as u8==0{return Ok(false)}
    let service=w(m,p,12)?;let provider=h.service_provider(m,service)?;
    if provider==0{return Ok(false)}
    if h.provider_ready(m,provider)? as u8==0{return Ok(false)}
    if m.byte(p.wrapping_add(0x8a4))?==0 {
        //47e594 ignores provider and clears exactly these workspace fields.
        let workspace=w(m,p,0x880)?;
        for offset in [4u32,12,16]{m.write(workspace.wrapping_add(offset),0)?;}
        m.write_byte(p.wrapping_add(0x8a4),1)?;
    }
    let goal=vector(m,p,0x898)?;
    for (i,value) in goal.into_iter().enumerate(){m.write(p.wrapping_add(0x870+i as u32*4),value.to_bits())?;}
    let configuration=h.provider_configuration(m,provider)?;
    let current=if configuration==0{u32::MAX}else{h.configuration_node(m,configuration)?&0x7fffffff};
    let goal_input=vector(m,p,0x870)?;
    let task=h.request_task(m,r)?;h.task_enter(m,manager,task)?;
    let workspace=w(m,p,0x880)?;
    let result=h.project_goal(m,p,provider,workspace,current,goal_input)?;
    let task=h.request_task(m,r)?;h.task_leave(m,manager,task)?;
    if result.status as u8==0{return Ok(false)}
    if result.node==u32::MAX{request::fail(m,p,r,h)?;return Ok(false)}
    m.write(p.wrapping_add(0x87c),result.node)?;
    let goal=vector(m,p,0x870)?;
    for (i,value) in goal.into_iter().enumerate(){m.write(p.wrapping_add(0x88c+i as u32*4),value.to_bits())?;}
    m.write_byte(p.wrapping_add(0x86c),0)?;Ok(true)
}

///1971ac, using the same shared DOL C-rand stream as the rest of gameplay.
pub fn wander(m:&mut Memory,p:u32,r:u32,h:&mut impl Host)->Result<bool>{
    h.traversal_mut(r)?.concrete.as_deref_mut().context("Wander traversal is absent")?.wander_mut()?;
    if !base(m,p,r,h)?{let _=m.byte(p.wrapping_add(8))?;return Ok(false)}
    let heading=(h.random_state()?.draw() as i32%360) as f32;
    let radians=f32::from_bits(0x3c8efa35)*heading;
    let (sy,cy)=super::native_angle::sin_cos_medium(radians as f64)?;
    let (sp,cp)=super::native_angle::sin_cos_medium(0.)?;
    let direction=[(sy as f32)*(cp as f32),sp as f32,(cy as f32)*(cp as f32)];
    h.traversal_mut(r)?.concrete.as_deref_mut().context("Wander traversal disappeared")?.wander_mut()?.direction_94=direction;
    let scalar=f32::from_bits(w(m,p,0x874)?);
    h.traversal_mut(r)?.concrete.as_deref_mut().context("Wander traversal disappeared")?.wander_mut()?.scalar_a4=scalar;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_agent_path_request,ai_goto_traversal};
    use serde_json::{Value,json};
    use std::io::Read;
    const P:u32=0x72000000;const R:u32=0x73000000;const S:u32=0x74000000;const O:u32=0x75000000;const PROV:u32=0x76000000;const CFG:u32=0x77000000;const STR:u32=0x78000000;const BUF:u32=0x79000000;
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>,instance:Instance,strategy:Strategy,random:super::super::game_random::State}
    impl Fixture<'_>{fn n(&self,k:&str,d:u32)->u32{self.c[k].as_u64().unwrap_or(d as u64) as u32}fn flag(&self,k:&str)->bool{self.c[k].as_bool().unwrap_or(true)}}
    impl ai_agent_path::ResetHost for Fixture<'_>{
        fn reservation(&mut self,_:&mut Memory,_:u32,_:u32,_:u32)->Result<u32>{anyhow::bail!("No reacquisition in initialization")}
        fn release(&mut self,_:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["release",r]));Ok(())}
    }
    impl ai_agent_path_request::Host for Fixture<'_>{
        fn request_manager(&mut self,_:&mut Memory)->Result<u32>{self.trace.push(json!(["manager"]));Ok(80)}
        fn request_task(&mut self,_:&mut Memory,r:u32)->Result<u32>{Ok(state(self,r)?.word_c as u32)}
        fn request_enabled(&mut self,_:&mut Memory,a:u32,t:u32)->Result<u32>{self.trace.push(json!(["enabled",a,t]));Ok(self.n("enabled",1))}
        fn service_provider(&mut self,_:&mut Memory,s:u32)->Result<u32>{self.trace.push(json!(["provider",s]));Ok(if self.flag("provider"){PROV}else{0})}
        fn provider_ready(&mut self,_:&mut Memory,p:u32)->Result<u32>{self.trace.push(json!(["ready",p]));Ok(self.n("ready",1))}
        fn provider_configuration(&mut self,m:&mut Memory,p:u32)->Result<u32>{w(m,p,4)}
        fn configuration_position(&mut self,m:&mut Memory,p:u32)->Result<[f32;3]>{vector(m,p,4)}
        fn reservation_extent(&mut self,_:&mut Memory,_:u32)->Result<f32>{anyhow::bail!("No bounds retry during initialization")}
        fn request_bounds(&mut self,_:&mut Memory,_:u32,_:[f32;3],_:[f32;3],_:[u32;2])->Result<u32>{anyhow::bail!("No bounds retry during initialization")}
        fn set_target(&mut self,_:&mut Memory,_:u32,_:u32,_:[f32;3])->Result<()>{anyhow::bail!("No retry target publication during initialization")}
    }
    impl Host for Fixture<'_>{
        fn random_state(&mut self)->Result<&mut super::super::game_random::State>{ensure!(self.c["op"]=="wander","Goto initialization must not draw random values");Ok(&mut self.random)}
        fn traversal_mut(&mut self,r:u32)->Result<&mut Instance>{ensure!(r==R,"Wrong traversal");Ok(&mut self.instance)}
        fn strategy_mut(&mut self,s:usize)->Result<&mut Strategy>{ensure!(s==STR as usize,"Wrong strategy");Ok(&mut self.strategy)}
        fn initial_position(&mut self,m:&mut Memory,o:u32)->Result<[f32;3]>{vector(m,o,0x34)}
        fn service_graph(&mut self,m:&mut Memory,s:u32)->Result<Option<usize>>{let v=w(m,s,0x50)?;Ok((v!=0).then_some(v as usize))}
        fn service_acceptance(&mut self,m:&mut Memory,s:u32)->Result<u32>{ensure!(self.c["op"]=="wander","Goto uses its own action profile");w(m,s,12)}
        fn task_enter(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{self.trace.push(json!(["enter",a,t]));Ok(())}
        fn task_leave(&mut self,_:&mut Memory,a:u32,t:u32)->Result<()>{self.trace.push(json!(["leave",a,t]));Ok(())}
        fn project_actor(&mut self,_:&mut Memory,p:u32,pos:[f32;3],s:u32,f:[u32;4])->Result<Projection>{self.trace.push(json!(["actor",p,pos.map(f32::to_bits),s,f]));if self.c["mutation"]=="task"{state(self,R)?.word_c=34;}Ok(Projection{status:self.n("project_status",0),node:self.n("node",55)})}
        fn select_provider_node(&mut self,m:&mut Memory,p:u32,n:u32)->Result<()>{self.trace.push(json!(["select",p,n]));if self.c["mutation"]=="position"{m.write(O+0x34,100f32.to_bits())?;}Ok(())}
        fn configuration_node(&mut self,m:&mut Memory,c:u32)->Result<u32>{m.word(c)}
        fn project_goal(&mut self,m:&mut Memory,_:u32,p:u32,b:u32,n:u32,g:[f32;3])->Result<Projection>{self.trace.push(json!(["goal",p,b,n,g.map(f32::to_bits)]));if self.c["mutation"]=="goal"{m.write(P+0x870,100f32.to_bits())?;}Ok(Projection{status:self.n("goal_status",1),node:self.n("goal_node",66)})}
    }
    fn instance(wander:bool)->Instance{let mut i=if wander{super::super::ai_wander_traversal::construct(R as usize)}else{ai_goto_traversal::construct(R as usize)};let g=(i.concrete.as_deref_mut().unwrap()).astar_mut();g.strategy=Some(STR as usize);g.word_c=33;i}
    fn unpack(hex:&str)->Result<Vec<u8>>{let compressed=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect::<Vec<_>>();let mut out=vec![];flate2::read::ZlibDecoder::new(&compressed[..]).read_to_end(&mut out)?;Ok(out)}
    fn compare(text:&str)->Result<()>{
        let fixture:Value=serde_json::from_str(text)?;
        for row in fixture["cases"].as_array().unwrap(){let c=&row["input"];let is_wander=c["op"]=="wander";let mut h=Fixture{c,trace:vec![],instance:instance(is_wander),strategy:Strategy{manager:None,owner:None,handle:Some(92)},random:super::super::game_random::State{seed:c["seed"].as_u64().unwrap_or(1) as u32,draws:0}};let mut m=Memory::default();
            for(p,n,fill)in[(P,0x8b4,0xcd),(S,0x100,0),(O,0x100,0),(PROV,0x50,0),(CFG,16,0),(BUF,24,0xa5)]{m.regions.insert(p,vec![fill;n]);}
            for(o,v)in[(12,S),(0x7c,O),(0x868,0x4eae38),(0x8ac,0x7b000000),(0x880,BUF),(0x87c,44)]{m.write(P+o,v)?;}
            for(o,v)in[(8,6),(0x10,5),(0x80,7),(0x86c,h.n("phase",0) as u8),(0x8a4,h.n("workspace",0) as u8)]{m.write_byte(P+o,v)?;}
            for(o,values)in[(0x6c,[1f32,-2.,3.]),(0x870,[1.,2.,3.]),(0x898,[9.,8.,7.])]{for(i,v)in values.into_iter().enumerate(){m.write(P+o+i as u32*4,v.to_bits())?;}}
            m.write(P+0x78,(c["scalar"].as_f64().unwrap_or(0.5) as f32).to_bits())?;
            for(i,v)in[2f32,3.,4.].into_iter().enumerate(){m.write(O+0x34+i as u32*4,v.to_bits())?;}
            m.write(S,0x7a000000)?;m.write(S+0x50,91)?;m.write(PROV+4,CFG)?;m.write(CFG,0x80000063)?;
            if is_wander{m.write(P+0x868,0x4eb818)?;m.write(P+0x86c,0x4eb8a0)?;m.write(P+0x870,0)?;m.write(P+0x874,(c["length"].as_f64().unwrap_or(2.5) as f32).to_bits())?;m.write(S+12,0x7b000000)?;}
            let result=if c["op"]=="base"{base(&mut m,P,R,&mut h)?}else if is_wander{wander(&mut m,P,R,&mut h)?}else{goto(&mut m,P,R,&mut h)?};
            assert_eq!(u32::from(result),row["result"].as_u64().unwrap() as u32,"result {c}");assert_eq!(json!(h.trace),row["trace"],"trace {c}");
            assert_eq!(m.regions[&P],unpack(row["image"].as_str().unwrap())?,"path {c}");assert_eq!(m.regions[&BUF],unpack(row["workspace"].as_str().unwrap())?,"workspace {c}");
            let field=|o:u32|row["fields"][format!("0x{o:x}")].as_u64().unwrap() as u32;let link=|v:u32|(v!=0).then_some(v as usize);
            let mut expected=instance(is_wander);let g=(expected.concrete.as_deref_mut().unwrap()).astar_mut();g.word_c=field(12) as i32;g.references_38_3c=[link(field(0x38)),link(field(0x3c))];g.word_40=field(0x40) as i32;g.vector_44=std::array::from_fn(|i|f32::from_bits(field(0x44+i as u32*4)));g.reference_6c=link(field(0x6c));g.values_80_8c=std::array::from_fn(|i|f32::from_bits(field(0x80+i as u32*4)));
            if is_wander {
                let w=expected.concrete.as_deref_mut().unwrap().wander_mut()?;w.scalar_90=f32::from_bits(field(0x90));w.direction_94=std::array::from_fn(|i|f32::from_bits(field(0x94+i as u32*4)));w.scalar_a0=f32::from_bits(field(0xa0));w.scalar_a4=f32::from_bits(field(0xa4));
                let actual=h.instance.concrete.as_deref_mut().unwrap().wander_mut()?;assert_eq!(actual.direction_94.map(f32::to_bits),w.direction_94.map(f32::to_bits),"direction bits {c}");
                assert_eq!(actual.scalar_a4.to_bits(),field(0xa4));assert_eq!(h.random.seed,row["random_seed"].as_u64().unwrap() as u32,"shared seed {c}");assert_eq!(h.random.draws,u64::from(result),"RNG draws {c}");
            }
            assert_eq!(serde_json::to_value(&h.instance)?,serde_json::to_value(expected)?,"canonical traversal {c}");
            let expected_strategy=row["strategy"].as_array().unwrap().iter().map(|v|link(v.as_u64().unwrap() as u32)).collect::<Vec<_>>();assert_eq!([h.strategy.manager,h.strategy.owner,h.strategy.handle].as_slice(),expected_strategy,"strategy {c}");
            assert!(!m.regions.contains_key(&R),"Canonical traversal was duplicated into mapped storage");
        }Ok(())
    }
    #[test]
    fn original_base_and_goto_initialization_match_canonical_records_and_phase_retention()->Result<()>{compare(include_str!("../../tools/blotworx/fixtures/ai_agent_path_initialize.json"))}
    #[test]
    fn original_wander_constructor_direction_and_shared_random_sequence_match()->Result<()>{compare(include_str!("../../tools/blotworx/fixtures/ai_wander_traversal.json"))}

}
