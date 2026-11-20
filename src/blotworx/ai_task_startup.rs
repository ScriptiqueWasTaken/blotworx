//!467580/467638 task-manager construction and authored TimeMgt initialization.
//! Numeric conversion remains an explicit native-parser dependency. Registered
//! tasks share the same State consumed by path gates and frame accounting.
use anyhow::{Context as _,Result,ensure};
use super::{ai_navigation_definitions::Tree,ai_task_manager::{State,Task},ai_task_frame::{Frame,Actor}};
#[derive(Debug)]
pub struct Periodic {pub name:Vec<u8>,pub period:f32}
#[derive(Debug)]
pub struct Estimated {pub name:Vec<u8>,pub time:f32}
#[derive(Debug)]
pub struct ActorHistory {pub work:u32,pub aperiodic:[Vec<f32>;3],pub periodic:Vec<f32>}
#[derive(Debug)]
pub struct OtherHistory {pub aperiodic:Vec<f32>,pub periodic:Vec<f32>}
#[derive(Debug)]
pub struct Context {
    pub tasks:State,pub frame:Frame,pub capacity:[u32;3],pub priorities:Vec<f32>,
    pub periodic:Vec<Periodic>,pub estimated:Vec<Estimated>,
    pub actors:Vec<ActorHistory>,pub others:Vec<OtherHistory>,pub deterministic:bool,
    ///Native startup leaves timing scratch uninitialized. Do not expose it
    ///as initialized timing until begin_frame establishes the first frame.
    pub frame_ready:bool,
}
///Original188100: append the default TimeMgt subtree to the existing world
///definitions. Do not deduplicate or sort: registration order defines IDs.
pub fn append_default_definitions(tree:&mut Tree,parent:usize)->Result<usize>{
    use super::ai_navigation_definitions::Node;
    let root=tree.append(Some(parent),Node{name:Some(b"TimeMgt".to_vec()),..Default::default()})?;
    for (name,tpf) in [
        ("LpfManager::PreAggregateComputation",None),("LpfManager::AreaComputation",None),
        ("Mpd::CPathfinder::Astar",None),("Mpd::CPathfinder::FollowPath",None),
        ("Mpd::CGraphManager::SearchNodeCanGo",None),("Mpd::CGraphManager::SearchNodeCanSee",None),
        ("Mpd::CGraphManager::MainUpdate",None),("JPSAStarAgent::ComputeAStarTraversal",None),
        ("JPSAStarAgent::ComputeAStarReachTraversal",Some("1.000000")),
        ("JPSAStarAgent::WanderAStarTraversal",Some("0.500000")),
        ("Mpd::JPSFleeAgent::CheckSafety",None),
    ]{
        let mut node=Node{name:Some(b"Aperiodic".to_vec()),..Default::default()};node.set_identifier(Some(name.as_bytes()));
        let child=tree.append(Some(root),node)?;
        if let Some(value)=tpf{tree.append(Some(child),Node{name:Some(b"Tpf".to_vec()),value:Some(value.as_bytes().to_vec()),..Default::default()})?;}
    }
    Ok(root)
}
pub trait Numbers {fn number(&mut self,text:&[u8])->Result<f64>;fn errno(&mut self)->&mut i32;}
fn equal(a:&[u8],b:&[u8])->bool{super::ai_traversal_registry::names_equal(a,b)}
fn string(a:&[u8])->Vec<u8>{a[..a.iter().position(|&b|b==0).unwrap_or(a.len())].to_vec()}
fn filled<T:Clone>(count:u32,value:T)->Result<Vec<T>>{let mut v=Vec::new();v.try_reserve_exact(count as usize).context("Task manager allocation failed")?;v.resize(count as usize,value);Ok(v)}
impl Context {
    ///Caller publishes this exact identity at World+4c, as constructor467580.
    pub fn construct(identity:usize)->Result<Self>{
        ensure!(identity!=0,"Task manager identity is null");
        Ok(Self{tasks:State{identity,limit:f32::from_bits(0x3c23d70a),exhausted:0,entries:vec![]},
            frame:Frame{previous:0.,current:0.,delta:0.,unaccounted:0.,work:0,selected:-1,actors:vec![],order:vec![]},
            capacity:[0;3],priorities:vec![],periodic:vec![],estimated:vec![],actors:vec![],others:vec![],deterministic:false,frame_ready:false})
    }
    ///Initialization467638. A false return preserves the constructed manager.
    ///True deterministic mode installs its getter; false does not uninstall an
    ///already supplied global getter. The host owns that global installation.
    pub fn initialize(&mut self,tree:&Tree,root:Option<usize>,actors:u32,others:u32,n:&mut impl Numbers,globals:&mut super::ai_task_clock::Globals)->Result<bool>{
        if let Some(root)=root {
            self.deterministic=match tree.first_value(root,b"DeterministicMode")?{
                None=>false,Some(v) if [b"yes".as_slice(),b"true",b"1"].iter().any(|s|equal(v,s))=>true,
                Some(v) if [b"no".as_slice(),b"false",b"0"].iter().any(|s|equal(v,s))=>false,
                Some(_)=>return Ok(false),
            };
            if self.deterministic{globals.getter=super::ai_task_clock::Getter::Deterministic;}
            for (i,key,tag) in [(0,b"MaxAperiodicTask".as_slice(),b"Aperiodic".as_slice()),(1,b"MaxPeriodicTask",b"Periodic"),(2,b"MaxEstimatedTask",b"Estimations")] {
                self.capacity[i]=if let Some(v)=tree.first_value(root,key)?{super::native_text_number::atoi(v,n.errno()) as u32}
                    else if i==2{match tree.find_tag(root,tag)?{Some(child)=>u32::try_from(tree.node(child)?.children.len())?,None=>0}}
                    else{let mut count=0u32;for &child in &tree.node(root)?.children{if tree.node(child)?.name.as_deref().is_some_and(|name|equal(name,tag)){count=count.checked_add(1).context("Task capacity exceeds native domain")?;}}count};
            }
        }
        //Represent native 32-bit allocation domains explicitly; no arbitrary cap.
        for &capacity in &self.capacity{ensure!(capacity<=u32::MAX/4,"Task table exceeds native byte-address domain");}
        ensure!(actors<=(u32::MAX-16)/32&&others<=(u32::MAX-16)/16,"Task actor storage exceeds native allocation domain");
        self.tasks.entries.clear();self.priorities.clear();self.periodic.clear();self.estimated.clear();self.actors.clear();self.others.clear();
        self.frame.actors.clear();self.frame.order.clear();self.frame_ready=false;
        let periodic_history=(0..self.capacity[1]).map(|i|-(i as f32)/100.).collect::<Vec<_>>();
        for i in 0..actors {
            self.frame.actors.push(Actor{elapsed:0.,weight:1.});self.frame.order.push(i);
            self.actors.push(ActorHistory{work:0,aperiodic:[filled(self.capacity[0],0.)?,filled(self.capacity[0],0.)?,filled(self.capacity[0],0.)?],periodic:periodic_history.clone()});
        }
        for _ in 0..others{self.others.push(OtherHistory{aperiodic:filled(self.capacity[0],0.)?,periodic:periodic_history.clone()});}
        if let Some(root)=root {
            for &child in &tree.node(root)?.children {
                let node=tree.node(child)?;let Some(tag)=node.name.as_deref() else{continue};
                let Some(name)=node.identifier.as_deref() else{continue};
                if equal(tag,b"Aperiodic") {
                    if self.tasks.entries.len()>=self.capacity[0] as usize{continue}
                    let mut entry=Task{name:string(name),maximum_time:f32::MAX,maximum_calls:i32::MAX,calls:0,started:0.,elapsed:0.};
                    let mut priority=1.;
                    if let Some(v)=tree.first_value(child,b"Priority")?{priority=n.number(v)? as f32;}
                    if let Some(v)=tree.first_value(child,b"Tpf")?{entry.maximum_time=(n.number(v)? as f32)/1000.;}
                    if let Some(v)=tree.first_value(child,b"MaxCall")?{entry.maximum_calls=super::native_text_number::atoi(v,n.errno());}
                    self.tasks.entries.push(entry);self.priorities.push(priority);
                } else if equal(tag,b"Periodic") {
                    if self.periodic.len()>=self.capacity[1] as usize{continue}
                    let period=if let Some(v)=tree.first_value(child,b"Period")?{(n.number(v)? as f32)/1000.}
                        else if self.tasks.limit==0.{f32::from_bits(0x3c88ce70)}else{self.tasks.limit};
                    self.periodic.push(Periodic{name:string(name),period});
                }
            }
            if let Some(estimates)=tree.find_tag(root,b"Estimations")?{
                for &child in &tree.node(estimates)?.children {
                    let node=tree.node(child)?;let name=node.name.as_deref().context("Estimated task tag is null")?;
                    //Registration can fail at capacity; source still parses value.
                    let time=(n.number(node.value.as_deref().context("Estimated task value is null")?)? as f32)/1000.;
                    if self.estimated.len()<self.capacity[2] as usize{self.estimated.push(Estimated{name:string(name),time});}
                }
            }
        }
        for (i,name) in [b"Intersect2d_SegSeg".as_slice(),b"Intersect2d_CapSeg"].into_iter().enumerate(){
            if let Some(entry)=self.estimated.iter().find(|e|equal(&e.name,name)){globals.estimates[i]=entry.time;}
        }
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json,Value};
    #[derive(Default)]
    struct NumbersFixture {errno:i32}
    impl Numbers for NumbersFixture {
        fn number(&mut self,text:&[u8])->Result<f64>{Ok(std::str::from_utf8(text)?.parse()?)}
        fn errno(&mut self)->&mut i32{&mut self.errno}
    }
    #[test]
    fn original_complete_startup_cases()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_task_startup.json"))?;
        for row in fixture["cases"].as_array().unwrap(){
            let c=&row["input"];let mut manager=Context::construct(0x72000000)?;
            if let Some(limit)=c["limit"].as_u64(){manager.tasks.limit=f32::from_bits(limit as u32);}
            let (tree,root)=if c["builtin"].as_bool().unwrap_or(false){
                let mut built=Tree::default();let parent=built.append(None,Default::default())?;
                let root=append_default_definitions(&mut built,parent)?;
                assert_eq!(serde_json::to_value(&built.nodes[1..])?,Value::Array(row["nodes"].as_array().unwrap().iter().map(|node|{let mut node=node.clone();for c in node["children"].as_array_mut().unwrap(){*c=json!(c.as_u64().unwrap()+1);}node}).collect::<Vec<_>>()),"Default builder differs");
                (built,Some(root))
            }else{(Tree{nodes:serde_json::from_value(row["nodes"].clone())?},c["root"].as_bool().unwrap_or(true).then_some(0))};
            let mut globals=super::super::ai_task_clock::Globals{estimates:[55.,66.],..Default::default()};let mut numbers=NumbersFixture::default();
            let result=manager.initialize(&tree,root,c["actors"].as_u64().unwrap_or(0) as u32,c["other"].as_u64().unwrap_or(0) as u32,&mut numbers,&mut globals)?;
            assert_eq!(json!(u32::from(result)),row["result"],"{c}");
            assert_eq!(json!(manager.capacity),row["capacity"],"{c}");
            assert_eq!(json!([manager.tasks.entries.len(),manager.periodic.len(),manager.estimated.len()]),row["counts"],"{c}");
            assert_eq!(json!(u8::from(manager.deterministic)),row["deterministic"],"{c}");
            assert_eq!(json!(manager.tasks.limit.to_bits()),row["limit"],"{c}");
            assert_eq!(json!(if globals.getter==super::super::ai_task_clock::Getter::Deterministic{0x4682e0u32}else{0}),row["clock"],"{c}");
            assert_eq!(json!(globals.estimates.map(f32::to_bits)),row["estimation_globals"],"{c}");
            let slots=if result{manager.capacity}else{[0;3]};
            let time=(0..slots[0] as usize).map(|i|manager.tasks.entries.get(i).map_or(f32::MAX,|e|e.maximum_time).to_bits()).collect::<Vec<_>>();
            let priority=(0..slots[0] as usize).map(|i|manager.priorities.get(i).copied().unwrap_or(1.).to_bits()).collect::<Vec<_>>();
            let calls=(0..slots[0] as usize).map(|i|manager.tasks.entries.get(i).map_or(i32::MAX,|e|e.maximum_calls) as u32).collect::<Vec<_>>();
            assert_eq!(json!({"names":manager.tasks.entries.iter().map(|e|&e.name).collect::<Vec<_>>(),"time":time,"priority":priority,"calls":calls}),row["aperiodic"],"{c}");
            let period=(0..slots[1] as usize).map(|i|manager.periodic.get(i).map_or(0.1,|e|e.period).to_bits()).collect::<Vec<_>>();
            assert_eq!(json!({"names":manager.periodic.iter().map(|e|&e.name).collect::<Vec<_>>(),"period":period}),row["periodic"],"{c}");
            let estimated=(0..slots[2] as usize).map(|i|manager.estimated.get(i).map_or(0.,|e|e.time).to_bits()).collect::<Vec<_>>();
            assert_eq!(json!({"names":manager.estimated.iter().map(|e|&e.name).collect::<Vec<_>>(),"time":estimated}),row["estimated"],"{c}");
            let actors=manager.actors.iter().zip(&manager.frame.actors).map(|(a,b)|json!({"budget":[b.elapsed.to_bits(),b.weight.to_bits(),a.work],"aperiodic":a.aperiodic.iter().map(|v|v.iter().copied().map(f32::to_bits).collect::<Vec<_>>()).collect::<Vec<_>>(),"periodic":a.periodic.iter().copied().map(f32::to_bits).collect::<Vec<_>>()})).collect::<Vec<_>>();
            assert_eq!(json!(actors),row["actors"],"{c}");
            let others=manager.others.iter().map(|a|json!({"aperiodic":a.aperiodic.iter().copied().map(f32::to_bits).collect::<Vec<_>>(),"periodic":a.periodic.iter().copied().map(f32::to_bits).collect::<Vec<_>>()})).collect::<Vec<_>>();
            assert_eq!(json!(others),row["others"],"{c}");assert_eq!(json!(manager.frame.order),row["order"],"{c}");
            assert!(!manager.frame_ready);
        }
        eprintln!("{} original complete startup cases",fixture["cases"].as_array().unwrap().len());Ok(())
    }
}
