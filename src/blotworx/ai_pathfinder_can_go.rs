//! CPathFinder499abc: retain selected ICanGo before shape lookup, derive the
//! actual radius, and call its virtual1c with the original point references.
use anyhow::{Context,Result};
use super::{ai_pathfinder_constructor::State,ai_pathfinder_modifiers::Kind,ai_graph_link_passes::Memory};
pub trait Host {
    ///45f994(361b4,1) -> CEntityDiameter+4, not CEntityHeight.
    fn entity_diameter(&mut self,state:&mut State,memory:&mut Memory)->Result<Option<f32>>;
    fn unit_scale(&mut self,state:&mut State,memory:&mut Memory)->Result<Option<f32>>;
    fn query_1c(&mut self,state:&mut State,memory:&mut Memory,selected:usize,start:u32,end:u32,radius:f32)->Result<u32>;
}
pub fn query(state:&mut State,memory:&mut Memory,start:u32,end:u32,host:&mut impl Host)->Result<u32> {
    let selected=state.tables_fc_1fc[Kind(3).table()].selected.context("Pathfinder CanGo selection is unwritten")?;
    let radius=if let Some(diameter)=host.entity_diameter(state,memory)? {0.5f32*diameter}
        else {f32::from_bits(0x3e99999a)*host.unit_scale(state,memory)?.unwrap_or(1.)};
    let selected=selected.context("Pathfinder CanGo requires a selected concrete modifier")?;
    host.query_1c(state,memory,selected,start,end,radius)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    const S:u32=0x79000000;const T:u32=0x79000100;
    struct Backend<'a>{case:&'a Value,trace:Vec<Value>}
    impl Host for Backend<'_> {
        fn entity_diameter(&mut self,state:&mut State,_:&mut Memory)->Result<Option<f32>> {
            self.trace.push(json!(["diameter"]));
            if self.case["mutation"]=="diameter" {state.tables_fc_1fc[Kind(3).table()].selected=Some(Some(2));}
            Ok((self.case["attribute"]==true).then(||f32::from_bits(self.case["diameter"].as_u64().unwrap() as u32)))
        }
        fn unit_scale(&mut self,state:&mut State,_:&mut Memory)->Result<Option<f32>> {
            self.trace.push(json!(["scale"]));
            if self.case["mutation"]=="scale" {state.tables_fc_1fc[Kind(3).table()].selected=Some(Some(2));}
            Ok(self.case["scale"].as_u64().map(|v|f32::from_bits(v as u32)))
        }
        fn query_1c(&mut self,state:&mut State,memory:&mut Memory,selected:usize,start:u32,end:u32,radius:f32)->Result<u32> {
            self.trace.push(json!(["query",selected,start,end,radius.to_bits()]));
            if self.case["mutation"]=="query" {state.tables_fc_1fc[Kind(3).table()].selected=Some(Some(3));memory.write(start+4,10f32.to_bits())?;}
            Ok(self.case["result"].as_u64().unwrap() as u32)
        }
    }
    #[test]
    fn can_go_wrapper_matches_original_retained_selection_points_radius_and_result()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_can_go.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;
            state.tables_fc_1fc[Kind(3).table()].selected=Some(Some(1));
            let mut memory=Memory{regions:std::collections::BTreeMap::from([(S,vec![0;0x200])]),resources:Default::default(),stride:0,journal:Default::default()};
            let mut backend=Backend{case:c,trace:vec![]};let result=query(&mut state,&mut memory,S,T,&mut backend)?;
            let actual=json!({"trace":backend.trace,"result":result,"selected":state.tables_fc_1fc[Kind(3).table()].selected.flatten(),"source_x":memory.word(S+4)?});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],expected[key],"{key}: {c}");}
        }Ok(())
    }
    #[test]
    fn missing_selected_modifier_keeps_preceding_shape_lookup_effects()->Result<()> {
        let case=json!({"attribute":true,"diameter":1.6f32.to_bits(),"mutation":"diameter","result":1});
        let mut state=super::super::ai_pathfinder_modifiers::tests::state()?;
        state.tables_fc_1fc[Kind(3).table()].selected=Some(None);
        let mut memory=Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
        let mut backend=Backend{case:&case,trace:vec![]};
        assert!(query(&mut state,&mut memory,S,T,&mut backend).is_err());
        assert_eq!(backend.trace,vec![json!(["diameter"])]);
        assert_eq!(state.tables_fc_1fc[Kind(3).table()].selected,Some(Some(2)));Ok(())
    }
}
