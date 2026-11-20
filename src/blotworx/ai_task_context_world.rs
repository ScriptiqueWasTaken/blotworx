//!World+4c ownership/publication46d26c..46d2a4 and concrete clock dispatch.
//!The World actor counts and definition root are required construction data;
//!neither is inferred from a gameplay actor list or an arbitrary profile.
use anyhow::{Context as _,Result,ensure};
use std::collections::BTreeMap;
use super::{ai_task_startup::{Context,Numbers},ai_task_clock::{self,Getter},ai_task_frame,ai_task_manager};
#[derive(Debug)]
pub struct World {
    pub identity:usize,pub definition_root:usize,pub actor_count:u32,pub other_count:u32,
    pub task_manager:Option<usize>,
}
#[derive(Debug,Default)]
pub struct Store {
    pub definitions:super::ai_navigation_definitions::Tree,
    pub worlds:BTreeMap<usize,World>,pub managers:BTreeMap<usize,Context>,
    pub current_world:Option<usize>,pub globals:ai_task_clock::Globals,
}
impl Store {
    pub fn current(&self)->Result<&World>{let id=self.current_world.context("Navigation World is not installed")?;self.worlds.get(&id).context("Current navigation World is not owned")}
    pub fn manager_identity(&self)->Result<usize>{self.current()?.task_manager.context("Current World task manager is null")}
    pub fn manager(&self,id:usize)->Result<&Context>{ensure!(id!=0,"Task manager identity is null");self.managers.get(&id).context("Task manager is not owned")}
    pub fn manager_mut(&mut self,id:usize)->Result<&mut Context>{ensure!(id!=0,"Task manager identity is null");self.managers.get_mut(&id).context("Task manager is not owned")}
    ///Publication is observable even when initialization subsequently returnsfalse.
    ///Constructor467580 writes the GLOBAL World; caller46d27c writes its target.
    pub fn install(&mut self,target:usize,id:usize,n:&mut impl Numbers)->Result<bool>{
        ensure!(self.worlds.contains_key(&target),"Task installation target World is absent");
        let global=self.current()?.identity;
        ensure!(!self.managers.contains_key(&id),"Task manager identity is already owned");
        let manager=Context::construct(id)?;self.managers.insert(id,manager);
        self.worlds.get_mut(&global).context("Global World identity differs from its key")?.task_manager=Some(id);
        let world=self.worlds.get_mut(&target).unwrap();world.task_manager=Some(id);
        let root=self.definitions.find_tag(world.definition_root,b"TimeMgt")?;
        let (actors,others)=(world.actor_count,world.other_count);
        self.managers.get_mut(&id).unwrap().initialize(&self.definitions,root,actors,others,n,&mut self.globals)
    }
    pub fn tasks(&mut self,id:Option<usize>)->Result<&mut ai_task_manager::State>{let id=match id{Some(id)=>id,None=>self.manager_identity()?};Ok(&mut self.manager_mut(id)?.tasks)}
    ///Deterministic getter4682e0 resolves the current GLOBAL World every call.
    pub fn sample(&mut self,backend:&mut impl ai_task_clock::Backend)->Result<Option<f32>>{
        Ok(match self.globals.getter{
            Getter::Absent=>None,Getter::External(id)=>Some(backend.sample(id)?),
            Getter::Deterministic=>Some(self.manager(self.manager_identity()?)?.frame.unaccounted),
        })
    }
    pub fn reset_clock(&mut self,backend:&mut impl ai_task_clock::Backend)->Result<()>{if let Some(id)=self.globals.reset{backend.reset(id)?;}Ok(())}
    pub fn begin_frame(&mut self,delta:f32,backend:&mut impl ai_task_clock::Backend)->Result<()>{
        let id=self.manager_identity()?;self.manager_mut(id)?.frame_ready=false;
        ai_task_frame::begin(&mut Runtime{store:self,backend},id,delta)?;
        self.manager_mut(id)?.frame_ready=true;Ok(())
    }
}
struct Runtime<'a,B>{store:&'a mut Store,backend:&'a mut B}
impl<B:ai_task_clock::Backend> ai_task_manager::Host for Runtime<'_,B>{
    fn tasks(&mut self,id:Option<usize>)->Result<&mut ai_task_manager::State>{self.store.tasks(id)}
    fn task_clock(&mut self)->Result<Option<f32>>{self.store.sample(self.backend)}
}
impl<B:ai_task_clock::Backend> ai_task_frame::Host for Runtime<'_,B>{
    fn frame(&mut self,id:usize)->Result<&mut ai_task_frame::Frame>{Ok(&mut self.store.manager_mut(id)?.frame)}
    fn task_globals(&mut self)->Result<&mut ai_task_frame::Globals>{Ok(&mut self.store.globals.frame)}
    fn world_actor_count(&mut self)->Result<u32>{Ok(self.store.current()?.actor_count)}
    fn reset_task_clock(&mut self)->Result<()>{self.store.reset_clock(self.backend)}
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    #[derive(Default)]
    struct Services{errno:i32,calls:Vec<usize>}
    impl Numbers for Services{
        fn number(&mut self,t:&[u8])->Result<f64>{Ok(std::str::from_utf8(t)?.parse()?)}
        fn errno(&mut self)->&mut i32{&mut self.errno}
    }
    impl ai_task_clock::Backend for Services{
        fn sample(&mut self,id:usize)->Result<f32>{assert_eq!(id,90);self.calls.push(id);Ok(42.)}
        fn reset(&mut self,id:usize)->Result<()>{assert_eq!(id,91);self.calls.push(id);Ok(())}
    }
    #[test]
    fn original_world_publication_and_initialization()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_task_context_world.json"))?;
        for row in fixture["cases"].as_array().unwrap(){
            let c=&row["input"];let global=0x73000000;let target=if c["different"].as_bool().unwrap_or(false){global+0x1000}else{global};
            let mut store=Store::default();store.definitions.nodes=serde_json::from_value(row["nodes"].clone())?;
            let parent=store.definitions.append(None,super::super::ai_navigation_definitions::Node::default())?;
            if c["root"].as_bool().unwrap_or(true){store.definitions.nodes[parent].children.push(0);}
            let actors=c["actors"].as_u64().unwrap_or(0) as u32;let others=c["other"].as_u64().unwrap_or(0) as u32;
            store.worlds.insert(global,World{identity:global,definition_root:parent,actor_count:if target==global{actors}else{0},other_count:if target==global{others}else{0},task_manager:None});
            store.worlds.insert(target,World{identity:target,definition_root:parent,actor_count:actors,other_count:others,task_manager:None});store.current_world=Some(global);
            let initial=c["initial_clock"].as_u64().unwrap_or(0) as usize;
            store.globals.getter=if initial==0{Getter::Absent}else{Getter::External(initial)};store.globals.estimates=[55.,66.];
            let result=store.install(target,0x72000000,&mut Services::default())?;
            assert_eq!(json!(u32::from(result)),row["result"],"{c}");
            assert_eq!(json!(store.current()?.task_manager),row["world"],"{c}");
            assert_eq!(json!(store.worlds[&target].task_manager),row["world_target"],"{c}");
            let manager=store.manager(0x72000000)?;
            assert_eq!(json!(manager.capacity),row["capacity"],"{c}");
            assert_eq!(json!([manager.tasks.entries.len(),manager.periodic.len(),manager.estimated.len()]),row["counts"],"{c}");
            assert_eq!(json!(manager.tasks.entries.iter().map(|e|&e.name).collect::<Vec<_>>()),row["aperiodic"]["names"],"{c}");
            assert_eq!(json!(manager.tasks.limit.to_bits()),row["limit"],"{c}");
            assert_eq!(json!(u8::from(manager.deterministic)),row["deterministic"],"{c}");
            let clock=match store.globals.getter{Getter::Absent=>0,Getter::Deterministic=>0x4682e0,Getter::External(id)=>id};
            assert_eq!(json!(clock),row["clock"],"{c}");
            assert_eq!(json!(store.globals.estimates.map(f32::to_bits)),row["estimation_globals"],"{c}");
            assert_eq!(manager.actors.len(),if result{actors as usize}else{0});assert_eq!(manager.others.len(),if result{others as usize}else{0});
            assert!(!manager.frame_ready);
        }
        eprintln!("{} original World install cases",fixture["cases"].as_array().unwrap().len());Ok(())
    }
    #[test]
    fn global_clock_captured_manager_and_frame_share_owned_storage()->Result<()>{
        let mut store=Store::default();let mut services=Services::default();
        assert!(store.manager_identity().is_err());assert!(store.tasks(None).is_err());
        for (world,id,time) in [(1,11,13.),(2,12,23.)]{
            store.worlds.insert(world,World{identity:world,definition_root:0,actor_count:0,other_count:0,task_manager:Some(id)});
            let mut c=Context::construct(id)?;c.frame.unaccounted=time;store.managers.insert(id,c);
        }
        store.current_world=Some(1);assert_eq!(store.tasks(Some(12))?.identity,12);assert_eq!(store.tasks(None)?.identity,11);
        assert_eq!(store.sample(&mut services)?,None);store.reset_clock(&mut services)?;assert!(services.calls.is_empty());
        store.globals.getter=Getter::Deterministic;assert_eq!(store.sample(&mut services)?,Some(13.));
        store.current_world=Some(2);assert_eq!(store.sample(&mut services)?,Some(23.));assert_eq!(store.tasks(Some(11))?.identity,11);
        store.globals.getter=Getter::External(90);assert_eq!(store.sample(&mut services)?,Some(42.));assert_eq!(services.calls,[90]);
        store.globals.reset=Some(91);store.begin_frame(0.1,&mut services)?;assert_eq!(services.calls,[90,91]);
        assert!(store.manager(12)?.frame_ready);assert_eq!(store.manager(12)?.frame.unaccounted,0.);assert_eq!(store.manager(11)?.frame.unaccounted,13.);
        store.globals.getter=Getter::Deterministic;assert_eq!(store.sample(&mut services)?,Some(0.));
        Ok(())
    }
}
