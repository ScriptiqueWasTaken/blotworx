//! LPF module49b31c/49b554, base451ad0/451d54, attributes49b76c/452314,
//! and reset49b810/452610. Context lookup and native numeric conversion must
//! come from the owning runtime; construction is not pathfinder publication.
use anyhow::{Context,Result};
use super::{ai_goto_gap::{Pool,Node},ai_traversal_registry::names_equal};
pub struct Point {pub position:[f32;3]}
impl Point {pub fn node()->Node<Self> {Node{payload:Self{position:[0.;3]},previous:None,next:None,active:0}}}
pub struct Taboo {pub keys:[i32;2],pub word_10:u32,pub scalar_14:f32,pub flag_18:u8}
impl Taboo {pub fn node()->Node<Self> {Node{payload:Self{keys:[-1;2],word_10:0,scalar_14:0.,flag_18:0},previous:None,next:None,active:0}}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PointPool { Current, Scratch }
pub struct State {
    pub identity:usize,
    pub manager:Option<usize>,pub context:Option<usize>,
    pub position:[f32;3],pub scalar_18:Option<f32>,pub flag_1c:Option<u8>,
    pub points:Option<Pool<Point>>,pub word_24:u32,
    pub vectors_28_34:[[f32;3];2],pub scalar_40:Option<f32>,
    pub vectors_44_50:[[f32;3];2],pub scalar_5c:Option<f32>,pub flags_60_62:Option<[u8;3]>,
    pub scratch:Option<Pool<Point>>,
    pub astar_radius:Option<f32>,pub local_path_max_length:Option<u32>,
    pub smoothing_period:Option<f32>,pub smoothing_distance:Option<f32>,pub smoothing_max_node_skip:Option<u32>,
    pub pathfinder:Option<usize>,pub word_80:Option<u32>,
    pub taboo:Option<Pool<Taboo>>,pub words_88_8c:Option<[u32;2]>,pub taboo_edge_max_count:Option<u32>,
}
pub trait Host {
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>>;
    fn manager(&mut self,state:&mut State)->Result<Option<usize>>;
    fn child_count(&mut self,state:&mut State,root:usize)->Result<u32>;
    fn child(&mut self,state:&mut State,root:usize,index:u32)->Result<Option<usize>>;
    fn name(&mut self,state:&mut State,node:usize)->Result<Option<Vec<u8>>>;
    fn value(&mut self,state:&mut State,node:usize)->Result<Option<usize>>;
    fn integer(&mut self,state:&mut State,value:usize)->Result<i32>;
    fn number(&mut self,state:&mut State,value:usize)->Result<f64>;
    fn context(&mut self,state:&mut State,manager:Option<usize>,value:usize)->Result<Option<usize>>;
    /// Capacity is read inside the allocation boundary, after any header hook.
    fn point_pool(&mut self,state:&mut State,_:PointPool)->Result<Option<Pool<Point>>> {
        Ok(Some(Pool::new(state.local_path_max_length.context("LPF path length is unwritten")?,Point::node)?))
    }
    fn release_scratch(&mut self,state:&mut State)->Result<()> {state.scratch=None;Ok(())}
    fn taboo_pool(&mut self,state:&mut State)->Result<Option<Pool<Taboo>>> {
        Ok(Some(Pool::new(state.taboo_edge_max_count.context("LPF taboo count is unwritten")?,Taboo::node)?))
    }
}
impl State {
    pub fn construct(identity:usize,pathfinder:usize,host:&mut impl Host)->Result<Self> {
        let mut state=Self{identity,manager:None,context:None,position:[0.;3],scalar_18:None,flag_1c:None,
            points:None,word_24:0,vectors_28_34:[[0.;3];2],scalar_40:None,vectors_44_50:[[0.;3];2],scalar_5c:None,flags_60_62:None,
            scratch:None,astar_radius:None,local_path_max_length:None,smoothing_period:None,smoothing_distance:None,smoothing_max_node_skip:None,
            pathfinder:None,word_80:None,taboo:None,words_88_8c:None,taboo_edge_max_count:None};
        state.astar_radius=Some(25.*host.unit_scale(&mut state)?.unwrap_or(1.));
        state.local_path_max_length=Some(512);
        state.smoothing_period=Some(f32::from_bits(0x3dcccccd));
        state.smoothing_max_node_skip=Some(10);
        state.smoothing_distance=Some(2.*host.unit_scale(&mut state)?.unwrap_or(1.));
        state.pathfinder=Some(pathfinder);state.words_88_8c=Some([0;2]);state.taboo_edge_max_count=Some(5);
        Ok(state)
    }
    pub fn attribute(&mut self,node:Option<usize>,host:&mut impl Host)->Result<bool> {
        let node=node.context("LPF attribute node is null")?;
        // The derived handler queries its tag before delegating to the base.
        let name=host.name(self,node)?.context("LPF attribute tag is null")?;
        if names_equal(&name,b"TabooEdgeMaxCount") {
            let Some(value)=host.value(self,node)? else{return Ok(false)};
            let value=host.integer(self,value)?;
            if value>0 {self.taboo_edge_max_count=Some(value as u32);}
            return Ok(true);
        }
        for (tag,field) in [(b"LpfContext".as_slice(),0),(b"AStarRadius",1),(b"LocalPathMaxLength",2),
            (b"SmoothingPeriod",3),(b"SmoothingDistance",4),(b"SmoothingMaxNodeSkip",5)] {
            let name=host.name(self,node)?.context("LPF attribute tag is null")?;
            if !names_equal(&name,tag) {continue;}
            let Some(value)=host.value(self,node)? else{return Ok(false)};
            match field {
                0=>{let manager=self.manager;self.context=host.context(self,manager,value)?;return Ok(self.context.is_some());}
                2|5=>{let number=host.integer(self,value)?;
                    if field==2 && number>3 {self.local_path_max_length=Some(number as u32);}
                    if field==5 && number>0 {self.smoothing_max_node_skip=Some(number as u32);}
                }
                _=>{
                    let scale=if field==1 || field==4 {host.unit_scale(self)?.unwrap_or(1.)}else{1.};
                    let number=host.number(self,value)? as f32;
                    let number=if field==3 {number}else{scale*number};
                    match field {
                        1 if number>=0.=>self.astar_radius=Some(number),
                        3 if number>=0.=>self.smoothing_period=Some(number),
                        4 if number>0.=>self.smoothing_distance=Some(number),
                        _=>{},
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub fn initialize(&mut self,root:Option<usize>,host:&mut impl Host)->Result<bool> {
        let Some(root)=root else{return Ok(false)};
        self.manager=host.manager(self)?;
        if self.manager.is_none() {return Ok(false);}
        let mut index=0;
        while index<host.child_count(self,root)? {
            let child=host.child(self,root,index)?.context("LPF definition child is null")?;
            if !self.attribute(Some(child),host)? {return Ok(false);}
            index=index.wrapping_add(1);
        }
        if self.context.is_none() {return Ok(false);}
        self.points=host.point_pool(self,PointPool::Current)?;
        let required=self.local_path_max_length.context("LPF path length is unwritten")? as usize;
        if self.scratch.as_ref().is_none_or(|pool|pool.nodes.len()<required) {
            if self.scratch.is_some() {host.release_scratch(self)?;}
            self.scratch=host.point_pool(self,PointPool::Scratch)?;
        }
        //451d54 invokes the actual derived virtual14 before49b554 allocates
        //its new taboo cache. Scratch64 is deliberately retained by reset.
        self.reset()?;
        self.taboo=host.taboo_pool(self)?;
        Ok(true)
    }
    pub fn reset(&mut self)->Result<()> {
        self.position=[f32::MAX;3];self.scalar_18=Some(0.);self.flag_1c=Some(0);
        if let Some(pool)=&mut self.points {pool.clear()?;}
        self.word_24=0;self.vectors_28_34=[[f32::MAX;3];2];self.scalar_40=Some(0.);
        self.vectors_44_50=[[f32::MAX;3];2];self.scalar_5c=Some(0.);self.flags_60_62=Some([0;3]);self.word_80=Some(0);
        if let Some(pool)=&mut self.taboo {pool.clear()?;}
        self.words_88_8c=Some([0;2]);Ok(())
    }
}
impl Drop for State {
    fn drop(&mut self) {self.taboo=None;self.points=None;self.scratch=None;}
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Binding<'a>{input:&'a Value,trace:Vec<Value>,units:usize}
    impl Binding<'_> {
        fn row(&self,node:usize)->&Value {
            if self.input["op"]=="attribute" {assert_eq!(node,100);self.input}
            else {&self.input["nodes"][node-100]}
        }
        fn flag(&self,name:&str)->bool {self.input[name].as_bool().unwrap_or(true)}
    }
    impl Host for Binding<'_> {
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {
            let value=if self.input["op"]=="construct" {
                let scales=self.input["scales"].as_array().unwrap();&scales[self.units.min(scales.len()-1)]
            } else {&self.input["scale"]};
            self.units+=1;self.trace.push(json!(["world",value]));Ok(value.as_u64().map(|v|f32::from_bits(v as u32)))
        }
        fn manager(&mut self,_:&mut State)->Result<Option<usize>> {
            assert!(self.input["scale"].as_u64().is_some(),"Native manager query requires an actual global context");
            self.trace.push(json!(["world",self.input["scale"]]));self.trace.push(json!(["manager"]));Ok(self.flag("manager").then_some(7))
        }
        fn child_count(&mut self,_:&mut State,_:usize)->Result<u32> {Ok(self.input["nodes"].as_array().unwrap().len() as u32)}
        fn child(&mut self,_:&mut State,_:usize,index:u32)->Result<Option<usize>> {Ok(Some(100+index as usize))}
        fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.row(node)["name"].as_str().map(|s|s.as_bytes().to_vec()))}
        fn value(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {Ok(self.row(node)["value"].as_str().map(|_|node))}
        fn integer(&mut self,_:&mut State,value:usize)->Result<i32> {self.trace.push(json!(["integer",value-100]));Ok(self.row(value)["integer"].as_i64().unwrap_or(0) as i32)}
        fn number(&mut self,_:&mut State,value:usize)->Result<f64> {self.trace.push(json!(["number",value-100]));Ok(f64::from_bits(self.row(value)["number"].as_u64().unwrap_or(0)))}
        fn context(&mut self,_:&mut State,manager:Option<usize>,value:usize)->Result<Option<usize>> {
            assert_eq!(manager,Some(7));self.trace.push(json!(["context",value-100]));Ok(self.flag("found").then_some(17))
        }
        fn point_pool(&mut self,state:&mut State,kind:PointPool)->Result<Option<Pool<Point>>> {
            let name=match kind{PointPool::Current=>"current",PointPool::Scratch=>"scratch"};let capacity=state.local_path_max_length.unwrap();
            self.trace.push(json!(["pool",name,capacity]));
            if self.input["fail"]==name {return Ok(None)};
            Ok(Some(Pool::new(capacity,Point::node)?))
        }
        fn release_scratch(&mut self,state:&mut State)->Result<()> {self.trace.push(json!(["release_scratch"]));state.scratch=None;Ok(())}
        fn taboo_pool(&mut self,state:&mut State)->Result<Option<Pool<Taboo>>> {
            let capacity=state.taboo_edge_max_count.unwrap();self.trace.push(json!(["pool","taboo",capacity]));
            if self.input["fail"]=="taboo" {return Ok(None)};
            Ok(Some(Pool::new(capacity,Taboo::node)?))
        }
    }
    fn pool<T>(pool:&Option<Pool<T>>,payload:impl Fn(&T)->Value)->Value {
        match pool {None=>Value::Null,Some(pool)=>json!({"capacity":pool.nodes.len(),"head":pool.active_head,"free":pool.free_head,"tail":pool.tail,"count":pool.count,
            "nodes":pool.nodes.iter().map(|node|json!([payload(&node.payload),node.previous,node.next,node.active])).collect::<Vec<_>>()})}
    }
    fn snapshot(state:&State)->Value {
        json!({"manager":state.manager,"context":state.context,"position":state.position.map(f32::to_bits),"scalar18":state.scalar_18.map(f32::to_bits),"flag1c":state.flag_1c,
            "word24":state.word_24,"vectors":[state.vectors_28_34[0].map(f32::to_bits),state.vectors_28_34[1].map(f32::to_bits),state.vectors_44_50[0].map(f32::to_bits),state.vectors_44_50[1].map(f32::to_bits)],
            "scalar40":state.scalar_40.map(f32::to_bits),"scalar5c":state.scalar_5c.map(f32::to_bits),"flags60":state.flags_60_62,
            "config":[state.astar_radius.unwrap().to_bits(),state.local_path_max_length.unwrap(),state.smoothing_period.unwrap().to_bits(),state.smoothing_distance.unwrap().to_bits(),state.smoothing_max_node_skip.unwrap(),state.taboo_edge_max_count.unwrap()],
            "pathfinder":state.pathfinder,"word80":state.word_80,"words88":state.words_88_8c,
            "pools":[pool(&state.points,|p|json!(p.position.map(f32::to_bits))),pool(&state.scratch,|p|json!(p.position.map(f32::to_bits))),
                pool(&state.taboo,|p|json!([p.keys[0] as u32,p.keys[1] as u32,p.word_10,p.scalar_14.to_bits(),p.flag_18]))]})
    }
    #[test]
    fn full_lpf_lifecycle_matches_original_owned_pools_options_and_scratch_reuse()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_lpf_module.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];let mut host=Binding{input,trace:vec![],units:0};
            let mut state=State::construct(1,50,&mut host)?;let mut result=None;
            if input["op"]!="construct" {
                host.trace.clear();host.units=0;
                if input["previous_context"]==true {state.context=Some(17);}
                let capacity=input["scratch"].as_u64().unwrap_or(0) as u32;
                if capacity!=0 {
                    let mut scratch=Pool::new(capacity,Point::node)?;
                    scratch.active_head=Some(0);scratch.free_head=Some(2);scratch.tail=Some(1);scratch.count=2;
                    scratch.nodes[0].payload.position[0]=7.;scratch.nodes[1].next=None;
                    scratch.nodes[0].active=1;scratch.nodes[1].active=1;state.scratch=Some(scratch);
                }
                result=Some(if input["op"]=="attribute" {state.manager=Some(7);state.attribute(Some(100),&mut host)?}
                    else {state.initialize(input["nodes"].is_array().then_some(1),&mut host)?});
            }
            assert_eq!(json!({"result":result,"state":snapshot(&state),"trace":host.trace}),case["output"],"{input}");
        }
        Ok(())
    }
}
