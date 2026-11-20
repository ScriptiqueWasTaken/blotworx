//! Retail goal detection classes: Distance2D5/Distance3D arrival and Distance3D
//! goal change. Constructor fields stay unwritten until native initialization.
use anyhow::{Context,Result};
use super::{ai_graph_link_passes::Memory,ai_refine_goal::position};
use super::{ai_navigation_definitions::Tree,ai_navigation_modifier_init as definition};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Class { Reached2D5,Reached3D,Changed3D }
impl Class {
    pub fn name(self)->&'static [u8] {match self{Self::Reached2D5=>b"Mpd::CDetectGoalReached_Distance2D5",Self::Reached3D=>b"Mpd::CDetectGoalReached_Distance3D",Self::Changed3D=>b"Mpd::CDetectGoalChanged_Distance3D"}}
    pub fn kind(self)->super::ai_pathfinder_modifiers::Kind {super::ai_pathfinder_modifiers::Kind(if self==Self::Changed3D {2}else{1})}
}
pub enum Fields {
    Reached2D5 {flag:Option<u8>,height:Option<f32>},
    Reached3D {flag:Option<u8>},
    Changed3D {distance:Option<f32>,last_time:Option<f32>},
}
pub struct State {pub identity:usize,pub pathfinder:usize,pub name:Vec<u8>,pub fields:Fields}
pub trait DefinitionBackend {
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>>;
    fn number(&mut self,state:&mut State,text:Option<&[u8]>)->Result<f64>;
    ///45f994 CEntityHeight (metadata36094) lookup followed by its actual scalar at+4.
    fn entity_height(&mut self,state:&mut State)->Result<Option<f32>>;
}
pub trait RuntimeHost {
    fn radius(&mut self,state:&mut State,memory:&mut Memory,parent:usize,retained:bool)->Result<f32>;
    fn query_54(&mut self,state:&mut State,memory:&mut Memory,parent:usize)->Result<u32>;
    fn word_f0(&mut self,state:&mut State,memory:&mut Memory,parent:usize)->Result<u32>;
    fn byte_3d8(&mut self,state:&mut State,memory:&mut Memory,parent:usize)->Result<u8>;
    fn manager_time(&mut self,state:&mut State,memory:&mut Memory)->Result<f32>;
    fn query_58(&mut self,state:&mut State,memory:&mut Memory,parent:usize,source:u32,target:u32)->Result<u32>;
}
impl State {
    pub fn construct(class:Class,identity:usize,pathfinder:usize,name:&[u8])->Self {
        Self {identity,pathfinder,name:name[..name.iter().position(|&b|b==0).unwrap_or(name.len())].to_vec(),fields:match class {
            Class::Reached2D5=>Fields::Reached2D5{flag:None,height:None},Class::Reached3D=>Fields::Reached3D{flag:None},Class::Changed3D=>Fields::Changed3D{distance:None,last_time:None},
        }}
    }
    pub fn class(&self)->Class {match self.fields{Fields::Reached2D5{..}=>Class::Reached2D5,Fields::Reached3D{..}=>Class::Reached3D,Fields::Changed3D{..}=>Class::Changed3D}}
    pub fn reset(&mut self) {match &mut self.fields {Fields::Reached2D5{flag,..}|Fields::Reached3D{flag}=>*flag=Some(0),Fields::Changed3D{last_time,..}=>*last_time=Some(0.)}}
    fn flag(&self)->Result<u8> {match self.fields {Fields::Reached2D5{flag,..}|Fields::Reached3D{flag}=>flag.context("Arrival detector flag is unwritten"),_=>anyhow::bail!("Changed detector has no arrival flag")}}
    fn set_flag(&mut self,value:u8)->Result<()> {match &mut self.fields {Fields::Reached2D5{flag,..}|Fields::Reached3D{flag}=>{*flag=Some(value);Ok(())},_=>anyhow::bail!("Changed detector has no arrival flag")}}
    ///1c7dcc/1c8074/1c7810. Return the native byte for2D5, not a guessed bool.
    pub fn query(&mut self,memory:&mut Memory,source_point:u32,target_point:u32,host:&mut impl RuntimeHost)->Result<u32> {
        if self.class()==Class::Changed3D {
            let source=position(memory,source_point)?;let target=position(memory,target_point)?;
            let dx=target[0]-source[0];let dz=target[2]-source[2];let horizontal=dz.mul_add(dz,dx*dx);
            let dy=target[1]-source[1];let distance=dy.mul_add(dy,horizontal);
            let Fields::Changed3D{distance:threshold,..}=self.fields else {unreachable!()};
            let threshold=threshold.context("Goal-change distance is unwritten")?;
            //Native ble tests !GT, including unordered; only GT reports distance change.
            if distance>threshold*threshold {return Ok(1)}
            let parent=self.pathfinder;if host.query_54(self,memory,parent)?!=2 {return Ok(0)}
            let parent=self.pathfinder;if host.word_f0(self,memory,parent)?!=2 {return Ok(0)}
            let source=position(memory,source_point)?;let target=position(memory,target_point)?;
            if source.iter().zip(target).all(|(a,b)|*a==b) {return Ok(0)}
            let now=host.manager_time(self,memory)?;
            let parent=self.pathfinder;let delay=if host.byte_3d8(self,memory,parent)?!=0 {f32::from_bits(0x3f4ccccd)}else{f32::from_bits(0x3f99999a)};
            let Fields::Changed3D{last_time,..}=&mut self.fields else {unreachable!()};
            if !(now<last_time.context("Goal-change timestamp is unwritten")?+delay) {*last_time=Some(now);Ok(1)}else{Ok(0)}
        } else {
            let old=self.flag()?;let parent=self.pathfinder;
            let radius=host.radius(self,memory,parent,old!=0)?;
            let source=position(memory,source_point)?;let target=position(memory,target_point)?;
            let dx=target[0]-source[0];let dz=target[2]-source[2];let horizontal=dz.mul_add(dz,dx*dx);
            if self.class()==Class::Reached3D {
                let dy=target[1]-source[1];let distance=dy.mul_add(dy,horizontal);
                let result=u8::from(distance<radius*radius);self.set_flag(result)?;return Ok(result as u32)
            }
            if !(horizontal<radius*radius) {self.set_flag(0)?;}else {
                let Fields::Reached2D5{height,..}=self.fields else {unreachable!()};
                let height=height.context("Arrival height limit is unwritten")?;
                if !((source[1]-target[1]).abs()<height) {
                    if old==0 {let value=host.query_58(self,memory,parent,source_point,target_point)? as u8;self.set_flag(value)?;}
                }else{self.set_flag(1)?;}
            }
            Ok(self.flag()? as u32)
        }
    }
    pub fn initialize_definition(&mut self,tree:&mut Tree,root:Option<usize>,backend:&mut impl DefinitionBackend)->Result<bool> {
        match self.class() {
            Class::Reached2D5=>{
                let value=if let Some(entity_height)=backend.entity_height(self)? {0.5f32*entity_height}else{f32::from_bits(0x3f59999a)*backend.unit_scale(self)?.unwrap_or(1.)};
                let Fields::Reached2D5{height,..}=&mut self.fields else {unreachable!()};*height=Some(value);
            },
            Class::Changed3D=>{
                let value=backend.unit_scale(self)?.unwrap_or(1.);
                let Fields::Changed3D{distance,..}=&mut self.fields else {unreachable!()};*distance=Some(value);
            },Class::Reached3D=>{},
        }
        if !definition::initialize(self,root,&mut Binding{tree,backend})? {return Ok(false)}
        self.reset();Ok(true)
    }
}
struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B}
impl<B:DefinitionBackend> definition::Host<State> for Binding<'_,B> {
    fn child_count(&mut self,_:&mut State,root:usize)->Result<u32> {Ok(u32::try_from(self.tree.node(root)?.children.len())?)}
    fn child(&mut self,_:&mut State,root:usize,index:u32)->Result<Option<usize>> {Ok(self.tree.node(root)?.children.get(index as usize).copied())}
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn initialize_attribute(&mut self,state:&mut State,node:usize)->Result<bool> {
        let name=self.tree.node(node)?.name.as_deref();
        let expected=match state.class(){Class::Reached2D5=>Some(b"MaxDeltaHeight".as_slice()),Class::Changed3D=>Some(b"MaxDist".as_slice()),Class::Reached3D=>None};
        if name.is_some() && expected.is_some() && super::ai_traversal_registry::names_equal(name.unwrap(),expected.unwrap()) {
            let scale=self.backend.unit_scale(state)?.unwrap_or(1.);
            let value=self.backend.number(state,self.tree.node(node)?.value.as_deref())? as f32*scale;
            //1c7c6c/1c7718 bge is !LT: reject unordered as well as nonpositive.
            if 0.<value {match &mut state.fields {Fields::Reached2D5{height,..}=>*height=Some(value),Fields::Changed3D{distance,..}=>*distance=Some(value),_=>unreachable!()}}
            return Ok(true)
        }
        Ok(self.tree.node(node)?.name.as_ref().is_some_and(|name|name.first()==Some(&b'_')))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ai_navigation_definitions::Node;
    use serde_json::{Value,json};
    const S:u32=0x74000000;const T:u32=0x74000100;
    struct Host<'a>{case:&'a Value,trace:Vec<Value>,target:u32}
    impl Host<'_> {
        fn word(&self,key:&str,default:u32)->u32 {self.case[key].as_u64().map_or(default,|v|v as u32)}
        fn scalar(&self,key:&str,default:f32)->f32 {f32::from_bits(self.word(key,default.to_bits()))}
        fn mutation(&self,name:&str)->bool {self.case["mutation"].as_str()==Some(name)}
    }
    impl RuntimeHost for Host<'_> {
        fn radius(&mut self,_:&mut State,_:&mut Memory,_:usize,retained:bool)->Result<f32> {self.trace.push(json!(["radius",retained]));Ok(self.scalar(if retained {"exit"}else{"entry"},if retained {2.}else{1.}))}
        fn query_54(&mut self,state:&mut State,memory:&mut Memory,parent:usize)->Result<u32> {
            self.trace.push(json!(["mode",parent]));
            if self.mutation("parent") {state.pathfinder=2;}
            if self.mutation("equal") {for i in 0..3 {memory.write(S+4+i*4,memory.word(self.target+4+i*4)?)?;}}
            Ok(self.word("mode",2))
        }
        fn word_f0(&mut self,_:&mut State,_:&mut Memory,parent:usize)->Result<u32> {self.trace.push(json!(["status",parent]));Ok(if self.mutation("status") && parent==1 {0}else{self.word("status",2)})}
        fn byte_3d8(&mut self,_:&mut State,_:&mut Memory,parent:usize)->Result<u8> {self.trace.push(json!(["slow",parent]));Ok(if self.mutation("slow") && parent==1 {1}else{self.word("slow",0) as u8})}
        fn manager_time(&mut self,state:&mut State,_:&mut Memory)->Result<f32> {
            if self.mutation("time") {let Fields::Changed3D{last_time,..}=&mut state.fields else {unreachable!()};*last_time=Some(5.);}
            self.trace.push(json!(["time"]));Ok(self.scalar("now",2.))
        }
        fn query_58(&mut self,_:&mut State,_:&mut Memory,parent:usize,source:u32,target:u32)->Result<u32> {self.trace.push(json!(["vertical",parent,source,target]));Ok(self.word("vertical",1))}
    }
    impl DefinitionBackend for Host<'_> {
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {self.trace.push(json!(["scale"]));Ok(self.case["scale"].as_u64().map(|v|f32::from_bits(v as u32)))}
        fn number(&mut self,_:&mut State,text:Option<&[u8]>)->Result<f64> {self.trace.push(json!(["number",text.map(String::from_utf8_lossy)]));Ok(f64::from_bits(self.case["number"].as_u64().unwrap_or(2f64.to_bits())))}
        fn entity_height(&mut self,_:&mut State)->Result<Option<f32>> {self.trace.push(json!(["height"]));Ok((self.case["attribute"]==true).then(||self.scalar("entity_height",2.4)))}
    }
    fn image(state:&State)->(u32,u32) {
        match state.fields {
            Fields::Reached2D5{flag,height}=>(flag.map_or(0xcdcdcdcd,|f|(f as u32)<<24|0xcdcdcd),height.map_or(0xcdcdcdcd,f32::to_bits)),
            Fields::Reached3D{flag}=>(flag.map_or(0xcdcdcdcd,|f|(f as u32)<<24|0xcdcdcd),0xcdcdcdcd),
            Fields::Changed3D{distance,last_time}=>(distance.map_or(0xcdcdcdcd,f32::to_bits),last_time.map_or(0xcdcdcdcd,f32::to_bits)),
        }
    }
    #[test]
    fn all_goal_detector_queries_and_definition_control_match_original()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goal_detection.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let class=match c["class"].as_u64().unwrap(){0=>Class::Reached2D5,1=>Class::Reached3D,_=>Class::Changed3D};
            let mut state=State::construct(class,10,1,b"detector\0ignored");assert_eq!(state.name,b"detector");
            let target=if c["alias"]==true {S}else{T};let mut host=Host{case:c,trace:vec![],target};
            let mut memory=Memory{regions:std::collections::BTreeMap::from([(S,vec![0;0x200])]),resources:Default::default(),stride:0,journal:Default::default()};
            for (p,key,defaults) in [(S,"source",[0,0,0]),(target,"target",[0.5f32.to_bits(),0,0])] {
                for i in 0..3 {memory.write(p+4+i*4,c[key].get(i as usize).and_then(Value::as_u64).map_or(defaults[i as usize],|v|v as u32))?;}
            }
            let result=if c["op"]=="query" {
                state.fields=match class {
                    Class::Reached2D5=>Fields::Reached2D5{flag:Some(host.word("flag",0) as u8),height:Some(host.scalar("threshold",0.5))},
                    Class::Reached3D=>Fields::Reached3D{flag:Some(host.word("flag",0) as u8)},
                    Class::Changed3D=>Fields::Changed3D{distance:Some(host.scalar("threshold",1.)),last_time:Some(host.scalar("last_time",0.))},
                };
                state.query(&mut memory,S,target,&mut host)?
            }else {
                let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
                if let Some(attrs)=c["attrs"].as_array() {for attr in attrs {
                    tree.append(Some(root),Node{name:Some(attr[0].as_str().unwrap().as_bytes().to_vec()),value:attr[1].as_str().map(|s|s.as_bytes().to_vec()),..Node::default()})?;
                }}
                u32::from(state.initialize_definition(&mut tree,(c["null_root"]!=true).then_some(root),&mut host)?)
            };
            let (word_c,word_10)=image(&state);
            let source=(0..3).map(|i|memory.word(S+4+i*4)).collect::<Result<Vec<_>>>()?;
            let target=(0..3).map(|i|memory.word(target+4+i*4)).collect::<Result<Vec<_>>>()?;
            let actual=json!({"result":result,"word_c":word_c,"word_10":word_10,"parent":state.pathfinder,"source":source,"target":target,"trace":host.trace});
            for key in actual.as_object().unwrap().keys() {assert_eq!(actual[key],expected[key],"{key}: {c}");}
        }Ok(())
    }
}
