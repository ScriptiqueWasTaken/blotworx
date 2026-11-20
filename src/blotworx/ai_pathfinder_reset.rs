//! Complete495d40 reset,4989a0 goal detach,48a58c search-reference cleanup
//! and496c10 strategy reset. Real modifier and graph ownership are mandatory.
use super::{ai_pathfinder_constructor::State,ai_pathfinder_modifiers::Kind,ai_graph_link_passes::Memory};
use anyhow::{Context,Result};
/// Written by reset, not initialized by48f0cc. Preserve unwritten constructor
///state; these fields are canonical, not a second raw pathfinder image.
#[derive(Default)]
pub struct Fields {
    pub word_68:Option<u32>,pub word_374:Option<i32>,pub word_370:Option<u32>,pub word_36c:Option<u32>,
    pub flags_369_36a_3c8_400_401_368_74:[Option<u8>;7],
    pub scalars_410_3f0_404_418:[Option<f32>;4],
}
pub trait Host {
    ///Read the owner's actual brain+14 and invoke goal virtual40 with it.
    fn detach_goal(&mut self,state:&mut State,memory:&mut Memory,goal:u32)->Result<()>;
    fn reset_modifier(&mut self,state:&mut State,memory:&mut Memory,kind:Kind,object:usize)->Result<()>;
    ///481ad0 on the real owning manager; callbacks may replace live references.
    fn release(&mut self,state:&mut State,memory:&mut Memory,manager:usize,reference:u32)->Result<()>;
}
pub fn set_goal(state:&mut State,memory:&mut Memory,goal:u32,host:&mut impl Host)->Result<()> {
    let previous=state.word_364.context("Pathfinder goal storage is uninitialized")?;
    if previous!=0 && previous!=goal {host.detach_goal(state,memory,previous)?;}
    state.word_364=Some(goal);Ok(())
}
pub fn reset_strategy(memory:&mut Memory,p:u32)->Result<()> {
    let start=memory.word(p)?;
    memory.write(p.wrapping_add(4),start)?;memory.write(p.wrapping_add(8),start)?;
    memory.write(p.wrapping_add(0x1a4),0)?;
    let start=memory.word(p.wrapping_add(0x19c))?;memory.write(p.wrapping_add(0x198),start)?;
    memory.write(p.wrapping_add(0x1a8),0)?;
    let start=memory.word(p.wrapping_add(0x1b4))?;
    for offset in [0x1b8,0x1bc,0x190,0x194] {memory.write(p.wrapping_add(offset),if offset<0x1b8 {0}else{start})?;}
    Ok(())
}
fn release_graph(state:&mut State,memory:&mut Memory,reference:u32,host:&mut impl Host)->Result<()> {
    let manager=state.references_3c_4c_50_54_58_60_6c[2].context("Search reference release has no graph manager")?;
    host.release(state,memory,manager,reference)
}
pub fn clear_search(state:&mut State,memory:&mut Memory,host:&mut impl Host)->Result<()> {
    if let Some(reference)=state.references_b0_b4_c8_d0_d4_f0[2] {
        release_graph(state,memory,u32::try_from(reference)?,host)?;
        state.references_b0_b4_c8_d0_d4_f0[2]=None;
    }
    let mut index=0u32;let mut offset=0u32;
    while index<state.words_a4_a8_ac[0] && state.words_a4_a8_ac[1]!=0 && state.words_a4_a8_ac[2]!=0 {
        for slot in [1,2] {
            let p=state.words_a4_a8_ac[slot].wrapping_add(offset);
            let reference=memory.word(p.wrapping_add(4))?;
            if reference!=0 {
                release_graph(state,memory,reference,host)?;
                //48a5f4/48a63c reload the possibly replaced array pointer.
                memory.write(state.words_a4_a8_ac[slot].wrapping_add(offset).wrapping_add(4),0)?;
            }
            let p=state.words_a4_a8_ac[slot].wrapping_add(offset);memory.write_byte(p.wrapping_add(8),0)?;
            let p=state.words_a4_a8_ac[slot].wrapping_add(offset);memory.write(p.wrapping_add(0x10c),0)?;
            memory.write(state.words_a4_a8_ac[slot].wrapping_add(offset),0)?;
        }
        offset=offset.wrapping_add(0x120);index=index.wrapping_add(1);
    }
    if let Some(p)=state.references_3c_4c_50_54_58_60_6c[5] {
        let reference=memory.word(u32::try_from(p)?.wrapping_add(0x1b0))?;
        if reference!=0 {
            release_graph(state,memory,reference,host)?;
            let p=state.references_3c_4c_50_54_58_60_6c[5].context("Release removed the search strategy")?;
            memory.write(u32::try_from(p)?.wrapping_add(0x1b0),0)?;
        }
    }
    //Native calls496c10 even if60 is null; diagnose instead of inventing a
    //successful empty search. Prior release side effects remain committed.
    let p=state.references_3c_4c_50_54_58_60_6c[5].context("Pathfinder reset requires constructed strategy storage")?;
    reset_strategy(memory,u32::try_from(p)?)
}
pub fn reset(state:&mut State,memory:&mut Memory,host:&mut impl Host)->Result<()> {
    state.flags_9c_9d[1]=0;
    set_goal(state,memory,0,host)?;
    state.reset_fields.word_374=Some(-1);
    state.reset_fields.flags_369_36a_3c8_400_401_368_74=[Some(0);7];
    state.reset_fields.word_370=Some(0);
    state.reset_fields.scalars_410_3f0_404_418=[Some(state.scalar_40c.context("Pathfinder reset scalar40c is unwritten")?),Some(-f32::MAX),Some(-f32::MAX),Some(-f32::MAX)];
    for index in [3,1,4] {state.points_200_274_2c4_314_378[index].position=[f32::MAX;3];}
    state.flags_f4_f5[1]=0;state.reset_fields.word_36c=Some(2);
    for kind in [0,1,2,4,5,6,3,9,8,7,10,11,12].map(Kind) {
        let mut index=0u32;
        while index<state.tables_fc_1fc[kind.table()].count {
            let object=state.tables_fc_1fc[kind.table()].storage.as_ref().context("Modifier reset table unmapped")?
                .slots.get(index as usize).copied().flatten().context("Modifier reset table slot unwritten")?;
            host.reset_modifier(state,memory,kind,object)?;
            index=index.wrapping_add(1);
        }
    }
    clear_search(state,memory,host)?;
    state.word_64=0;state.reset_fields.word_68=Some(0);
    for index in [3,4] {
        if let (Some(reference),Some(manager))=(state.references_b0_b4_c8_d0_d4_f0[index],state.references_3c_4c_50_54_58_60_6c[3]) {
            host.release(state,memory,manager,u32::try_from(reference)?)?;
            state.references_b0_b4_c8_d0_d4_f0[index]=None;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ai_pathfinder_modifiers::Storage;
    use serde_json::{Value,json};
    const OBJ:usize=0x74000000;const ARRAY:u32=0x76000000;const ARENA:u32=0x79000000;const REPLACEMENT:u32=0x7a000000;
    struct Fixture<'a> {case:&'a Value,trace:Vec<Value>,mutated:bool}
    impl Host for Fixture<'_> {
        fn detach_goal(&mut self,s:&mut State,_:&mut Memory,_:u32)->Result<()> {
            self.trace.push(json!(["detach",444,s.flags_9c_9d[1]]));
            if self.case["mutation"]=="goal" {s.word_364=Some(999);}Ok(())
        }
        fn reset_modifier(&mut self,s:&mut State,_:&mut Memory,kind:Kind,object:usize)->Result<()> {
            let index=(object-OBJ-kind.0*0x100)/0x10;
            self.trace.push(json!(["reset_modifier",kind.0,index,s.word_364]));
            if !self.mutated && kind.0==self.case["kind"].as_u64().unwrap_or(0) as usize {
                let table=&mut s.tables_fc_1fc[kind.table()];
                match self.case["mutation"].as_str() {
                    Some("append")=>{table.count=2;self.mutated=true;},
                    Some("shrink")=>{table.count=0;self.mutated=true;},
                    Some("replace")=>{table.storage.as_mut().unwrap().slots[1]=Some(OBJ+kind.0*0x100+2*0x10);self.mutated=true;},_=>{},
                }
            }Ok(())
        }
        fn release(&mut self,s:&mut State,_:&mut Memory,manager:usize,reference:u32)->Result<()> {
            self.trace.push(json!(["release",manager,reference]));
            match (self.case["mutation"].as_str(),reference) {
                (Some("array_replace"),100)=>s.words_a4_a8_ac[1]=REPLACEMENT,
                (Some("array_count"),100)=>s.words_a4_a8_ac[0]=0,
                (Some("arena_replace"),800)=>s.references_3c_4c_50_54_58_60_6c[5]=Some((ARENA+0x10000) as usize),
                (Some("manager"),902)=>s.references_3c_4c_50_54_58_60_6c[3]=None,
                (Some("d0"),902)=>s.references_b0_b4_c8_d0_d4_f0[3]=Some(999),_=>{},
            }
            if reference==903 {
                match self.case["detach_mutation"].as_str(){
                    Some("clear_graph")=>s.references_3c_4c_50_54_58_60_6c[2]=None,
                    Some("swap_graph")=>s.references_3c_4c_50_54_58_60_6c[2]=Some(200),
                    Some("set_flag")=>s.flags_9c_9d[0]=1,
                    Some("clear_flag")=>s.flags_9c_9d[0]=0,_=>{}
                }
            }Ok(())
        }
    }
    fn setup(c:&Value)->Result<(State,Memory)> {
        let mut s=super::super::ai_pathfinder_modifiers::tests::state()?;
        s.word_364=Some(if c["goal"]==true {0x7b000000}else{0});
        s.scalar_40c=Some(f32::from_bits(c["scalar"].as_u64().unwrap_or(0x80000000) as u32));
        let mut m=Memory {regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        for kind in (0..13).map(Kind) {
            let t=&mut s.tables_fc_1fc[kind.table()];t.count=c["count"].as_u64().unwrap_or(1) as u32;
            t.storage=Some(Storage{identity:0x75000000+0x100*kind.0,slots:(0..3).map(|i|Some(OBJ+kind.0*0x100+i*0x10)).collect()});
        }
        for (j,p) in [ARRAY,ARRAY+0x10000,REPLACEMENT].into_iter().enumerate() {
            m.regions.insert(p,vec![0xcd;2*0x120]);
            for i in 0..2 {m.write(p+i*0x120+4,if c["no_references"]==true {0}else{100+j as u32*10+i})?;}
        }
        for p in [ARENA,ARENA+0x10000] {
            m.regions.insert(p,vec![0xcd;0x1c4]);
            for (offset,value) in [(0,1111),(0x19c,2222),(0x1b4,3333),(0x1b0,800)] {m.write(p+offset,value)?;}
        }
        if c["no_references"]==true {m.write(ARENA+0x1b0,0)?;}
        s.words_a4_a8_ac=[2,if c["null_array"]==1 {0}else{ARRAY},if c["null_array"]==2 {0}else{ARRAY+0x10000}];
        s.references_3c_4c_50_54_58_60_6c[2]=Some(100);
        s.references_3c_4c_50_54_58_60_6c[3]=(c["null_manager"]!=true).then_some(200);
        s.references_3c_4c_50_54_58_60_6c[5]=Some(ARENA as usize);
        for (i,value) in [(2,901),(3,902),(4,903)] {s.references_b0_b4_c8_d0_d4_f0[i]=(c["no_references"]!=true).then_some(value);}
        Ok((s,m))
    }
    fn runs(bytes:&[u8])->Vec<[usize;2]> {
        let mut out:Vec<[usize;2]>=vec![];
        for &b in bytes {if let Some(row)=out.last_mut().filter(|r|r[0]==b as usize) {row[1]+=1;}else{out.push([b as usize,1]);}}out
    }
    #[test]
    fn full_pathfinder_reset_matches_original_lifetimes_fresh_tables_and_search_storage()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_reset.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let (mut s,mut m)=setup(c)?;
            let mut host=Fixture{case:c,trace:vec![],mutated:false};reset(&mut s,&mut m,&mut host)?;
            let f=&s.reset_fields;
            let mut words=vec![s.word_364.unwrap(),f.word_374.unwrap() as u32,f.word_370.unwrap(),f.word_36c.unwrap(),s.word_64,f.word_68.unwrap()];
            words.extend(f.scalars_410_3f0_404_418.map(|v|v.unwrap().to_bits()));
            for index in [1,3,4] {words.extend(s.points_200_274_2c4_314_378[index].position.map(f32::to_bits));}
            let flags=f.flags_369_36a_3c8_400_401_368_74;
            let bytes=[s.flags_9c_9d[1],flags[0].unwrap(),flags[1].unwrap(),flags[2].unwrap(),flags[3].unwrap(),flags[4].unwrap(),flags[5].unwrap(),s.flags_f4_f5[1],flags[6].unwrap()];
            let references=[s.references_3c_4c_50_54_58_60_6c[2].unwrap_or(0) as u32,s.references_3c_4c_50_54_58_60_6c[3].unwrap_or(0) as u32,s.references_3c_4c_50_54_58_60_6c[5].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[2].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[3].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[4].unwrap_or(0) as u32,s.words_a4_a8_ac[0],s.words_a4_a8_ac[1],s.words_a4_a8_ac[2]];
            let tables=(0..13).map(|k|{let t=&s.tables_fc_1fc[Kind(k).table()];json!({"count":t.count,"entries":t.storage.as_ref().unwrap().slots.iter().map(|id|(id.unwrap()-OBJ-k*0x100)/0x10).collect::<Vec<_>>()})}).collect::<Vec<_>>();
            let regions=[ARRAY,ARRAY+0x10000,REPLACEMENT,ARENA,ARENA+0x10000].map(|p|json!([p,runs(&m.regions[&p])]));
            let actual=json!({"trace":host.trace,"words":words,"bytes":bytes,"references":references,"tables":tables,"regions":regions});
            for key in actual.as_object().unwrap().keys() {assert_eq!(actual[key],case[key],"{key}: {c}");}
        }Ok(())
    }
    #[test]
    fn world_removal_executes_real_reset_before_reloading_graph_and_subscription()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_world_lifecycle.json"))?;
        for case in fixture["enclosing"].as_array().unwrap(){
            let c=&case["input"];let (mut s,mut m)=setup(c)?;
            s.identity=0x72000000;
            s.references_3c_4c_50_54_58_60_6c[2]=c["graph"].as_u64().filter(|&p|p!=0).map(|p|p as usize);
            s.flags_9c_9d[0]=c["subscribed"].as_u64().unwrap_or(1) as u8;
            let listeners:Memory=serde_json::from_value(case["listener_memory"].clone())?;
            m.regions.extend(listeners.regions);
            assert!(!m.regions.contains_key(&0x72000000),"Service State must have one typed owner");
            let mut host=Fixture{case:c,trace:vec![],mutated:false};
            super::super::ai_pathfinder_world_lifecycle::removed20(&mut s,&mut m,&mut host)?;
            assert_eq!(json!(host.trace),case["trace"],"reset callbacks {c}");
            assert_eq!(json!(s.flags_9c_9d[0]),case["subscribed"],"subscription {c}");
            let references=[s.references_3c_4c_50_54_58_60_6c[2].unwrap_or(0) as u32,s.references_3c_4c_50_54_58_60_6c[3].unwrap_or(0) as u32,s.references_3c_4c_50_54_58_60_6c[5].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[2].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[3].unwrap_or(0) as u32,s.references_b0_b4_c8_d0_d4_f0[4].unwrap_or(0) as u32,s.words_a4_a8_ac[0],s.words_a4_a8_ac[1],s.words_a4_a8_ac[2]];
            assert_eq!(json!(references),case["references"],"references {c}");
            let f=&s.reset_fields;
            let mut words=vec![s.word_364.unwrap(),f.word_374.unwrap() as u32,f.word_370.unwrap(),f.word_36c.unwrap(),s.word_64,f.word_68.unwrap()];
            words.extend(f.scalars_410_3f0_404_418.map(|v|v.unwrap().to_bits()));
            for index in [1,3,4]{words.extend(s.points_200_274_2c4_314_378[index].position.map(f32::to_bits));}
            assert_eq!(json!(words),case["words"],"reset words {c}");
            let flags=f.flags_369_36a_3c8_400_401_368_74;
            let bytes=[s.flags_9c_9d[1],flags[0].unwrap(),flags[1].unwrap(),flags[2].unwrap(),flags[3].unwrap(),flags[4].unwrap(),flags[5].unwrap(),s.flags_f4_f5[1],flags[6].unwrap()];
            assert_eq!(json!(bytes),case["bytes"],"reset bytes {c}");
            let tables=(0..13).map(|k|{let t=&s.tables_fc_1fc[Kind(k).table()];json!({"count":t.count,"entries":t.storage.as_ref().unwrap().slots.iter().map(|id|(id.unwrap()-OBJ-k*0x100)/0x10).collect::<Vec<_>>()})}).collect::<Vec<_>>();
            assert_eq!(json!(tables),case["tables"],"reset tables {c}");
            for region in case["regions"].as_array().unwrap(){
                let p=region[0].as_u64().unwrap() as u32;
                assert_eq!(json!(runs(&m.regions[&p])),region[1],"region {p:#x}: {c}");
            }
        }Ok(())
    }
    #[test]
    fn reset_diagnoses_absent_strategy_after_releasing_existing_search_refs()->Result<()> {
        let c=json!({"count":0,"null_array":1});let (mut s,mut m)=setup(&c)?;
        s.references_3c_4c_50_54_58_60_6c[5]=None;
        let mut host=Fixture{case:&c,trace:vec![],mutated:false};
        assert!(reset(&mut s,&mut m,&mut host).is_err());
        assert!(s.references_b0_b4_c8_d0_d4_f0[2].is_none());
        assert_eq!(s.word_364,Some(0));assert_eq!(s.reset_fields.word_68,None);
        assert_eq!(host.trace,vec![json!(["release",100,901])]);Ok(())
    }
}
