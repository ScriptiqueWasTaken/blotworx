//! JPSRefineGoal193be8, its actual IRefineGoal base1ca7a4/reset1ca888,
//! and49b274 definition entry with2b7c10 attribute admission.
//! Point/cache helper193d54 composes with the enclosing entry/projection
//! modules; actual runtime services and normal scheduling are still required.
use anyhow::{Context, Result};
use super::{ai_navigation_definitions::Tree, ai_navigation_modifier_init as definition};
use super::ai_graph_link_passes::Memory;

pub trait Units {
    fn unit_context(&mut self) -> Result<Option<usize>>;
    fn context_scale(&mut self, context: usize) -> Result<f32>;
}
pub trait PointHost: Units {
    /// Actual194458 virtual20 implementation; absence must not be treated
    /// as successful projection. Point addresses refer to canonical data.
    fn refine_20(&mut self, state: &mut State, memory: &mut Memory, source: u32, output: u32) -> Result<()>;
}
pub(super) fn word(memory: &Memory, base: u32, offset: u32) -> Result<u32> {
    memory.word(base.wrapping_add(offset))
}
fn scalar(memory: &Memory, base: u32, offset: u32) -> Result<f32> {
    Ok(f32::from_bits(word(memory,base,offset)?))
}
pub(super) fn position(memory: &Memory, point: u32) -> Result<[f32;3]> {
    Ok([scalar(memory,point,4)?,scalar(memory,point,8)?,scalar(memory,point,12)?])
}
pub(super) fn set_position(memory: &mut Memory, point: u32, values: [f32;3]) -> Result<()> {
    for (index,value) in values.into_iter().enumerate() {
        memory.write(point.wrapping_add(4+index as u32*4),value.to_bits())?;
    }
    Ok(())
}

pub struct State {
    pub identity: usize,
    pub pathfinder: usize,
    pub name: Vec<u8>,
    pub base_vectors: [[f32; 3]; 2],
    pub vectors_24_30: [[f32; 3]; 2],
    pub scalar_3c: f32,
    pub word_40: u32,
    // Neither the derived nor base constructor writes these words.
    pub word_44: Option<u32>,
    pub timer_words_4c_50_54: [Option<u32>; 3],
}
impl State {
    pub fn construct(identity: usize, pathfinder: usize, name: &[u8]) -> Self {
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        Self {
            identity, pathfinder, name: name.to_vec(),
            base_vectors: [[f32::MAX; 3]; 2],
            vectors_24_30: [[f32::MAX; 3]; 2],
            scalar_3c: -f32::MAX, word_40: 0, word_44: None,
            timer_words_4c_50_54: [None, Some(0), Some(0)],
        }
    }
    /// The installed reset is the BASE reset, not a fresh derived construction.
    pub fn reset(&mut self) {
        self.base_vectors = [[f32::MAX; 3]; 2];
    }
    pub fn initialize_definition(&mut self, tree: &mut Tree, root: Option<usize>) -> Result<bool> {
        definition::initialize(self, root, &mut Binding { tree })
    }
    ///193d54: retain point attributes, validate the live sector-cache identity,
    /// reuse the base projection cache, or refine and constrain its height.
    /// This is the shared helper, not the enclosing193fe4 refinement entry.
    pub fn initialize_point(&mut self, memory: &mut Memory, source: u32, output: u32,
        classified: u8, cache: u32, host: &mut impl PointHost) -> Result<()> {
        // Preserve sequential reads/writes, including exact or overlapping
        // point aliases. The native vtable word is not copied.
        for offset in (4..0x50).step_by(4) {
            let value=word(memory,source,offset)?;
            memory.write(output.wrapping_add(offset),value)?;
        }
        let provider=word(memory,cache,0x10)?;
        let mut cell=if provider==0 {0} else {
            let table=word(memory,provider,8)?;
            let slot=table.wrapping_add(word(memory,cache,0x14)?.wrapping_mul(8));
            let valid=if memory.byte(slot)?!=0 {
                word(memory,word(memory,slot,4)?,8)?==word(memory,cache,0)?
            } else {false};
            if !valid {memory.write(cache.wrapping_add(0x18),0)?;}
            word(memory,cache,0x18)?
        };
        if cell==0 {
            if self.base_vectors[1][0]!=f32::MAX {
                let source_position=position(memory,source)?;
                let delta: [f32;3]=std::array::from_fn(|i|source_position[i]-self.base_vectors[0][i]);
                let squared=delta[1].mul_add(delta[1],delta[2].mul_add(delta[2],delta[0]*delta[0]));
                let context=host.unit_context()?;
                let first=if let Some(context)=context {host.context_scale(context)?} else {1.};
                let second=if let Some(context)=context {host.context_scale(context)?} else {1.};
                let limit=(f32::from_bits(0x3c23d70a)*first)*second;
                if squared<=limit {
                    set_position(memory,output,self.base_vectors[1])?;
                    return Ok(());
                }
            }
            if classified==0 {
                host.refine_20(self,memory,source,output)?;
                self.base_vectors[0]=position(memory,source)?;
                self.base_vectors[1]=position(memory,output)?;
            } else {
                // Source re-reads this even when provider was null; it does
                // not invent a clean empty cache in that case.
                cell=word(memory,cache,0x18)?;
            }
        }
        if cell!=0 {
            let y=scalar(memory,output,8)?;
            let lower=scalar(memory,cell,0x1c)?;
            if !(y<lower) && y<=scalar(memory,cell,0x18)? {return Ok(());}
            let midpoint=(scalar(memory,cell,0x1c)?+scalar(memory,cell,0x18)?)*0.5;
            memory.write(output.wrapping_add(8),midpoint.to_bits())?;
        }
        Ok(())
    }
}
struct Binding<'a> { tree: &'a mut Tree }
impl definition::Host<State> for Binding<'_> {
    fn child_count(&mut self, _: &mut State, root: usize) -> Result<u32> {
        Ok(u32::try_from(self.tree.node(root)?.children.len())?)
    }
    fn child(&mut self, _: &mut State, root: usize, index: u32) -> Result<Option<usize>> {
        Ok(self.tree.node(root)?.children.get(index as usize).copied())
    }
    fn name(&mut self, _: &mut State, node: usize) -> Result<Option<Vec<u8>>> {
        Ok(self.tree.node(node)?.name.clone())
    }
    fn initialize_attribute(&mut self, _: &mut State, node: usize) -> Result<bool> {
        //2b7c10 re-reads the actual node's name; it accepts only underscore
        //metadata. Unknown gameplay settings must not silently pass.
        if self.tree.node(node)?.name.is_none() { return Ok(false); }
        Ok(self.tree.node(node)?.name.as_ref().context("Modifier name disappeared")?.first() == Some(&b'_'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ai_navigation_definitions::Node;
    use std::collections::BTreeMap;
    const POINT:u32=0x7a000000;
    const CACHE:u32=0x7b000000;
    const PROVIDER:u32=0x7c000000;
    const TABLE:u32=0x7d000000;
    const SECTOR:u32=0x7e000000;
    const CELL:u32=0x7f100000;
    struct PointBoundary {scales:Vec<Option<u32>>,context_reads:usize,reads:usize,refinements:usize,output:u32,refined:[u32;3]}
    impl Units for PointBoundary {
        fn unit_context(&mut self)->Result<Option<usize>> {
            self.context_reads+=1;Ok(self.scales[0].map(|_|80))
        }
        fn context_scale(&mut self, context:usize)->Result<f32> {
            assert_eq!(context,80);
            let value=self.scales[self.reads.min(self.scales.len()-1)];self.reads+=1;
            Ok(f32::from_bits(value.context("Live unit context has no scale")?))
        }
    }
    impl PointHost for PointBoundary {
        fn refine_20(&mut self,_:&mut State,memory:&mut Memory,source:u32,output:u32)->Result<()> {
            assert_eq!(source,POINT);assert_eq!(output,self.output);self.refinements+=1;
            for (index,value) in self.refined.into_iter().enumerate() {memory.write(output+4+index as u32*4,value)?;}
            Ok(())
        }
    }
    fn point_case(state:&mut State,case:&serde_json::Value)->Result<()> {
        let input=&case["input"];
        let mut memory=Memory{regions:BTreeMap::new(),resources:BTreeMap::new(),stride:0,journal:BTreeMap::new()};
        for (base,size) in [(POINT,0x100),(CACHE,0x1c),(PROVIDER,0x10),(TABLE,8),(SECTOR,12),(CELL,0x20)] {
            memory.regions.insert(base,vec![0xa5;size]);
        }
        memory.write(POINT,0x4ec1b0)?;
        let source:[u32;3]=serde_json::from_value(input["source"].clone())?;
        for (index,value) in source.into_iter().enumerate() {memory.write(POINT+4+index as u32*4,value)?;}
        for index in 0..16 {memory.write(POINT+0x10+index*4,0x10000000+index)?;}
        let kind=input["cache"].as_str().context("Missing cache kind")?;
        let provider=if kind.starts_with("none") {0} else {PROVIDER};
        let cell=if matches!(kind,"none"|"no_cell") {0} else {CELL};
        for (index,value) in [42,7,8,9,provider,0,cell].into_iter().enumerate() {memory.write(CACHE+index as u32*4,value)?;}
        memory.write(PROVIDER+8,TABLE)?;
        memory.write(TABLE,if kind=="inactive" {0} else {(if kind=="active7" {7} else {1})<<24})?;
        memory.write(TABLE+4,SECTOR)?;memory.write(SECTOR+8,if kind=="mismatch" {43} else {42})?;
        memory.write(CELL+0x18,4f32.to_bits())?;memory.write(CELL+0x1c,2f32.to_bits())?;
        let base:[[u32;3];2]=serde_json::from_value(input["base"].clone())?;
        state.base_vectors=base.map(|v|v.map(f32::from_bits));
        let mut host=PointBoundary{scales:serde_json::from_value(input["scales"].clone())?,context_reads:0,reads:0,refinements:0,
            output:POINT+input["output_shift"].as_u64().context("Missing output")? as u32,
            refined:serde_json::from_value(input["refined"].clone())?};
        state.initialize_point(&mut memory,POINT,host.output,input["classified"].as_u64().context("Missing classification")? as u8,CACHE,&mut host)?;
        assert_eq!(memory.regions[&POINT],serde_json::from_value::<Vec<u8>>(case["points"].clone())?,"{input}");
        assert_eq!(memory.regions[&CACHE],serde_json::from_value::<Vec<u8>>(case["cache"].clone())?,"{input}");
        assert_eq!(host.reads as u64,case["scale_reads"].as_u64().unwrap(),"{input}");
        assert_eq!(host.context_reads as u64,case["context_reads"].as_u64().unwrap(),"{input}");
        assert_eq!(host.refinements as u64,case["refinements"].as_u64().unwrap(),"{input}");
        Ok(())
    }
    fn image(state: &State) -> Vec<u8> {
        let mut bytes=vec![0xa5;0x58];
        let mut word=|offset:usize,value:u32|bytes[offset..offset+4].copy_from_slice(&value.to_be_bytes());
        word(0,0x4eba50);word(4,50);word(8,40);
        for (index,value) in state.base_vectors.iter().flatten().enumerate() {word(0xc+index*4,value.to_bits());}
        for (index,value) in state.vectors_24_30.iter().flatten().enumerate() {word(0x24+index*4,value.to_bits());}
        word(0x3c,state.scalar_3c.to_bits());word(0x40,state.word_40);
        if let Some(value)=state.word_44 {word(0x44,value);}
        word(0x48,0x4e4838);
        for (index,value) in state.timer_words_4c_50_54.iter().enumerate() {if let Some(value)=value {word(0x4c+index*4,*value);}}
        bytes
    }
    #[test]
    fn refine_goal_owned_state_and_point_cache_match_original() -> Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_refine_goal.json"))?;
        for case in fixture["cases"].as_array().context("Missing cases")? {
            let input=&case["input"];
            let name:Vec<u8>=serde_json::from_value(input["name"].clone())?;
            let mut state=State::construct(60,50,&name);
            let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
            let result=match input["mode"].as_str().unwrap() {
                "reset"=>{
                    state.base_vectors=[[7.,8.,9.],[10.,11.,12.]];
                    state.vectors_24_30=[[13.,14.,15.],[16.,17.,18.]];
                    state.scalar_3c=19.;state.word_40=20;state.word_44=Some(21);
                    state.timer_words_4c_50_54=[Some(22),Some(23),Some(24)];
                    state.reset();None
                },
                "definition"=>{
                    for name in input["children"].as_array().unwrap() {
                        tree.append(Some(root),Node{name:Some(serde_json::from_value(name.clone())?),..Node::default()})?;
                    }
                    Some(state.initialize_definition(&mut tree,(!input["null_root"].as_bool().unwrap()).then_some(root))?)
                },
                "construct"=>None,
                "point"=>{point_case(&mut state,case)?;None},
                _=>anyhow::bail!("Unknown reference case"),
            };
            let expected:Vec<u8>=serde_json::from_value(case["image"].clone())?;
            assert_eq!(image(&state),expected,"{input}");
            assert_eq!(state.name,serde_json::from_value::<Vec<u8>>(case["name"].clone())?,"{input}");
            assert_eq!(result,case["result"].as_bool(),"{input}");
        }
        Ok(())
    }
}
