//! Reset dispatch over actual owned modifiers and canonical graph managers.
//! Map identities are references, never inferred readiness or copied state.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_pathfinder_constructor::State,ai_pathfinder_modifier_factory::Owned,
    ai_pathfinder_modifiers::Kind,ai_graph_link_passes::Memory,ai_graph_handles};
pub trait Backend:super::ai_goto_gap::Host {
    /// Real path-object virtual40; its owning brain/entity lookup is required.
    fn detach_goal(&mut self,state:&mut State,memory:&mut Memory,goal:u32)->Result<()>;
}
pub struct Binding<'a,B> {
    pub modifiers:&'a mut BTreeMap<usize,Owned>,
    pub graphs:&'a mut BTreeMap<usize,ai_graph_handles::State>,
    pub backend:&'a mut B,
}
impl<B:Backend> super::ai_pathfinder_reset::Host for Binding<'_,B> {
    fn detach_goal(&mut self,state:&mut State,memory:&mut Memory,goal:u32)->Result<()> {self.backend.detach_goal(state,memory,goal)}
    fn reset_modifier(&mut self,state:&mut State,memory:&mut Memory,kind:Kind,identity:usize)->Result<()> {
        let modifier=self.modifiers.get_mut(&identity).context("Pathfinder reset has no owned concrete modifier")?;
        ensure!(modifier.identity()==identity && modifier.pathfinder(memory)?==state.identity && modifier.kind()==kind,"Pathfinder reset modifier identity, owner or table class differs");
        modifier.reset(self.backend)
    }
    fn release(&mut self,_:&mut State,_:&mut Memory,manager:usize,reference:u32)->Result<()> {
        let graph=self.graphs.get_mut(&manager).context("Pathfinder reference release has no canonical graph manager")?;
        //481ad0's boolean return is discarded by495d40/48a58c, including a
        //declined/deferred release. Parent references are cleared by the caller.
        let _=graph.release(Some(usize::try_from(reference)?))?;Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_pathfinder_modifiers::Storage,ai_goto_gap};
    use serde_json::Value;
    struct Host {scale_reads:usize,detached:Vec<u32>}
    impl ai_goto_gap::Host for Host {
        fn unit_scale(&mut self,_:&mut ai_goto_gap::State)->Result<Option<f32>> {self.scale_reads+=1;Ok(Some(self.scale_reads as f32))}
        fn owner_entity(&mut self,_:&mut ai_goto_gap::State)->Result<usize> {Ok(10)}
        fn profiling_enabled(&mut self,_:&mut ai_goto_gap::State)->Result<bool> {Ok(false)}
        fn register_budget(&mut self,_:&mut ai_goto_gap::State,_:&str)->Result<i32> {anyhow::bail!("No test profiling service")}
    }
    impl Backend for Host {
        fn detach_goal(&mut self,_:&mut State,_:&mut Memory,goal:u32)->Result<()> {self.detached.push(goal);Ok(())}
    }
    fn memory()->Memory {
        let mut memory=Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        memory.regions.insert(1000,vec![0;0x1c4]);memory
    }
    fn setup(host:&mut Host)->Result<(State,BTreeMap<usize,Owned>)> {
        let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;
        state.word_364=Some(7);state.scalar_40c=Some(-0.);
        state.words_a4_a8_ac[0]=0;state.references_3c_4c_50_54_58_60_6c[5]=Some(1000);
        state.references_3c_4c_50_54_58_60_6c[2]=Some(50);
        let mut modifiers=BTreeMap::new();
        for (kind,identity,class) in [(Kind(0),101,Owned::REFINE_CLASS),(Kind(8),100,Owned::GAP_CLASS)] {
            let modifier=Owned::construct(kind,class,identity,&state,b"actual owned modifier",host)?;
            modifiers.insert(identity,modifier);
            let table=&mut state.tables_fc_1fc[kind.table()];table.count=1;
            table.storage=Some(Storage{identity,slots:vec![Some(identity)]});
        }
        let Owned::RefineGoal(refine)=modifiers.get_mut(&101).unwrap() else {unreachable!()};
        refine.base_vectors=[[1.,2.,3.];2];refine.word_40=123;refine.word_44=Some(456);
        let Owned::GapAvoidance(gap)=modifiers.get_mut(&100).unwrap() else {unreachable!()};
        gap.word_114=Some(789);gap.scalars_cc_d0=[Some(8.),Some(9.)];
        Ok((state,modifiers))
    }
    #[test]
    fn pathfinder_reset_dispatches_owned_classes_and_original_graph_release_corpus()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_handle_release.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut host=Host{scale_reads:0,detached:vec![]};let (mut state,mut modifiers)=setup(&mut host)?;
            let graph=serde_json::from_value(case["input"]["state"].clone())?;
            let mut graphs=BTreeMap::from([(50,graph)]);let mut memory=memory();
            state.references_b0_b4_c8_d0_d4_f0[2]=case["input"]["source"].as_u64().map(|v|v as usize);
            let scale_reads_before=host.scale_reads;
            super::super::ai_pathfinder_reset::reset(&mut state,&mut memory,&mut Binding{modifiers:&mut modifiers,graphs:&mut graphs,backend:&mut host})?;
            assert_eq!(serde_json::to_value(&graphs[&50])?,case["state"],"{}",case["input"]);
            assert_eq!(host.detached,[7]);assert_eq!(host.scale_reads,scale_reads_before+2);
            assert!(state.references_b0_b4_c8_d0_d4_f0[2].is_none());
            let Owned::RefineGoal(refine)=&modifiers[&101] else {unreachable!()};
            assert_eq!(refine.base_vectors,[[f32::MAX;3];2]);assert_eq!((refine.word_40,refine.word_44),(123,Some(456)));
            let Owned::GapAvoidance(gap)=&modifiers[&100] else {unreachable!()};
            assert_eq!(gap.scales_74_78,[Some((scale_reads_before+1) as f32),Some((scale_reads_before+2) as f32)]);assert_eq!(gap.scalars_cc_d0,[Some(0.);2]);assert_eq!(gap.word_114,Some(789));
        }Ok(())
    }
    #[test]
    fn reset_binding_rejects_wrong_owner_and_missing_graph_after_prior_side_effects()->Result<()> {
        let mut host=Host{scale_reads:0,detached:vec![]};let (mut state,mut modifiers)=setup(&mut host)?;
        let Owned::RefineGoal(refine)=modifiers.get_mut(&101).unwrap() else {unreachable!()};refine.pathfinder=usize::MAX;
        let mut graphs=BTreeMap::new();let mut memory=memory();
        assert!(super::super::ai_pathfinder_reset::reset(&mut state,&mut memory,&mut Binding{modifiers:&mut modifiers,graphs:&mut graphs,backend:&mut host}).is_err());
        assert_eq!(state.word_364,Some(0));
        let Owned::RefineGoal(refine)=&modifiers[&101] else {unreachable!()};assert_eq!(refine.base_vectors,[[1.,2.,3.];2]);
        let mut host=Host{scale_reads:0,detached:vec![]};let (mut state,mut modifiers)=setup(&mut host)?;
        state.references_b0_b4_c8_d0_d4_f0[2]=Some(100);
        let scale_reads_before=host.scale_reads;
        assert!(super::super::ai_pathfinder_reset::reset(&mut state,&mut memory,&mut Binding{modifiers:&mut modifiers,graphs:&mut graphs,backend:&mut host}).is_err());
        assert_eq!(state.references_b0_b4_c8_d0_d4_f0[2],Some(100));
        let Owned::RefineGoal(refine)=&modifiers[&101] else {unreachable!()};assert_eq!(refine.base_vectors,[[f32::MAX;3];2]);
        assert_eq!(host.scale_reads,scale_reads_before+2);Ok(())
    }
    #[test]
    fn spatial_reset_reads_canonical_owner_and_preserves_all_arena_bytes()->Result<()> {
        let mut host=Host{scale_reads:0,detached:vec![]};let (mut state,mut modifiers)=setup(&mut host)?;
        let mut m=memory();let address=2000u32;m.regions.insert(address,vec![0xab;0x24]);m.write(address+4,u32::try_from(state.identity)?)?;
        modifiers.insert(address as usize,Owned::Spatial{address});
        let mut graphs=BTreeMap::new();
        let before=m.regions.clone();
        let mut binding=Binding{modifiers:&mut modifiers,graphs:&mut graphs,backend:&mut host};
        super::super::ai_pathfinder_reset::Host::reset_modifier(&mut binding,&mut state,&mut m,Kind(3),address as usize)?;
        assert_eq!(m.regions,before);
        m.write(address+4,0xdeadbeef)?;
        assert!(super::super::ai_pathfinder_reset::Host::reset_modifier(&mut binding,&mut state,&mut m,Kind(3),address as usize).is_err());
        assert_eq!(m.word(address+4)?,0xdeadbeef);Ok(())
    }

}
