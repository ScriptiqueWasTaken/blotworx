//! Immutable authored audio nodes, resolved at command consumption. Selection
//! uses the recovered non-continuous kernels; decoding is lazy and shared.
use super::{audio_bank::{Bank,container::{Settings,PlaylistItem}},audio_play_action::Request,audio_playback::Clip};
use anyhow::{ensure,Context,Result};
use std::{collections::{BTreeMap,BTreeSet},sync::{Arc,OnceLock,Weak},ops::Range};

pub struct Sound {
    pub metadata:Clip,
    bytes:Arc<[u8]>,range:Range<usize>,
    decoded:OnceLock<Result<Arc<Clip>,String>>,
}
impl Sound {
    pub fn decode(&self)->Result<Arc<Clip>> {
        self.decoded.get_or_init(|| (||->Result<Arc<Clip>> {
            let mut clip=self.metadata.clone();
            match &clip.signal {
                super::audio_signal::Signal::Pcm=>{let media=super::audio_dsp::Media::parse(&self.bytes[self.range.clone()])?;let samples=media.decode()?;if let Some(layout)=clip.layout {ensure!(layout.samples<=samples.len(),"Authored sample count exceeds DSP data");if layout.passes!=1 {media.validate_loop_context(&samples,layout.start)?;}}clip.samples=samples.into();},
                super::audio_signal::Signal::FilePending(path)=>{let spec=Arc::new(super::audio_file::Spec::read(path)?);ensure!(spec.definition.sample_rate==clip.rate,"External audio rate disagrees with source");clip.layout=Some(super::audio_loop::Layout::new(spec.definition.sample_count as usize,spec.definition.loop_points,clip.source_passes)?);clip.signal=super::audio_signal::Signal::File(spec);},
                _=>{},
            }
            Ok(Arc::new(clip))
        })().map_err(|e|format!("{e:#}"))).as_ref().cloned().map_err(|e|anyhow::anyhow!(e.clone()))
    }
}
pub enum Node {
    Sound(Sound),
    Selector{id:u32,settings:Settings,playlist:Vec<PlaylistItem>,children:BTreeMap<u32,Arc<Node>>},
    Switch{id:u32,kind:u32,group:u32,definition:super::audio_switch::Definition,children:BTreeMap<u32,Arc<Node>>},
    Unsupported(String),
}
pub type Cache=BTreeMap<(u32,u32),Arc<Node>>;
impl Node {
    pub fn lineages(&self,output:&mut Vec<(u32,u16,Arc<[u32]>)>) {
        match self {
            Self::Sound(s)=>output.push((s.metadata.sound,s.metadata.maximum_instances,s.metadata.lineage.clone())),
            Self::Selector{children,..}|Self::Switch{children,..}=>for node in children.values(){node.lineages(output)},
            Self::Unsupported(_)=>{},
        }
    }
}
/// Cache identities include the event's originating bank because duplicate
/// HIRC IDs can have different cooked definitions in different bank views.
pub fn prepare(bank:&Bank<'_>,storage:&BTreeMap<u32,Arc<[u8]>>,origin:u32,id:u32,cache:&mut Cache)->Result<Arc<Node>> {prepare_with_root(bank,storage,origin,id,cache,None)}
pub fn prepare_with_root(bank:&Bank<'_>,storage:&BTreeMap<u32,Arc<[u8]>>,origin:u32,id:u32,cache:&mut Cache,root:Option<&std::path::Path>)->Result<Arc<Node>> {
    fn visit(bank:&Bank<'_>,storage:&BTreeMap<u32,Arc<[u8]>>,origin:u32,id:u32,cache:&mut Cache,path:&mut BTreeSet<u32>,root:Option<&std::path::Path>)->Result<Arc<Node>> {
        ensure!(path.insert(id),"Audio container cycle at {id:08x}");
        if let Some(node)=cache.get(&(origin,id)){path.remove(&id);return Ok(node.clone());}
        let object=bank.objects.get(&id).context("Missing audio node")?;
        let built=(||->Result<Node> {Ok(match object.kind {
            2=>{
                let source=object.source()?;
                if source.plugin!=0x50001 || source.stream_type!=0 {
                    return Ok(Node::Sound(Sound{metadata:super::audio_playback::prepare_unbacked(bank,id,root)?,bytes:Arc::from([]),range:0..0,decoded:OnceLock::new()}));
                }
                let (metadata,media)=super::audio_playback::prepare_direct(bank,id)?;
                let pointer=media.as_ptr() as usize;
                let (bytes,offset)=storage.values().find_map(|bytes| {
                    let base=bytes.as_ptr() as usize;
                    pointer.checked_sub(base).filter(|offset|*offset<=bytes.len() && media.len()<=bytes.len()-*offset).map(|offset|(bytes.clone(),offset))
                }).context("Prepared audio media lost its owning bank")?;
                Node::Sound(Sound{metadata,bytes,range:offset..offset+media.len(),decoded:OnceLock::new()})
            },
            5=>{
                let c=object.random_sequence(bank.feedback)?;
                ensure!(c.settings.continuous==0,"Continuous playlist scheduling is not connected");
                ensure!(c.settings.mode<=1 && c.settings.random_mode<=1,"Unimplemented audio selector mode");
                let mut children=BTreeMap::new();
                for item in &c.playlist {if bank.objects.contains_key(&item.sound) {children.insert(item.sound,visit(bank,storage,origin,item.sound,cache,path,root)?);}}
                Node::Selector{id,settings:c.settings,playlist:c.playlist,children}
            },
            6=>{
                let c=object.switch_container(bank.feedback)?;
                ensure!(!c.continuous,"Continuous switch voice updates are not connected");
                let definition=super::audio_switch::Definition::from_container(&c);let mut children=BTreeMap::new();
                for package in &definition.groups {for &child in &package.nodes {if bank.objects.contains_key(&child) {children.insert(child,visit(bank,storage,origin,child,cache,path,root)?);}}}
                Node::Switch{id,kind:c.group_kind,group:c.group,definition,children}
            },
            kind=>anyhow::bail!("Audio Play node {id:08x} class{kind} is not implemented"),
        })})();
        path.remove(&id);
        let node=Arc::new(built.unwrap_or_else(|error|Node::Unsupported(format!("Audio node {id:08x}: {error:#}"))));cache.insert((origin,id),node.clone());Ok(node)
    }
    visit(bank,storage,origin,id,cache,&mut BTreeSet::new(),root)
}
#[derive(Default)]
pub struct Runtime {
    selectors:super::audio_bank::selection::Runtime,
    switches:super::audio_switch::Runtime,
    pub selections:u64,
    owners:BTreeMap<usize,Weak<super::audio_events::RegisteredObject>>,
}
pub trait Sink {
    fn sound(&mut self,clip:Arc<Clip>,request:&Request,random:&mut dyn FnMut()->u16)->Result<u32>;
    fn switch_value(&mut self,group:u32)->Result<Option<u32>>;
    fn reject(&mut self,error:&str);
}
impl Runtime {
    pub fn retain_owner(&mut self,owner:&Arc<super::audio_events::RegisteredObject>) {
        self.owners.insert(owner.handle as usize,Arc::downgrade(owner));
    }
    pub fn collect_retired(&mut self) {
        let dead=self.owners.iter().filter_map(|(&handle,owner)|(owner.strong_count()==0).then_some(handle)).collect::<Vec<_>>();
        for handle in dead {self.selectors.retire_owner(handle);self.switches.retire_object(handle as u32);self.owners.remove(&handle);}
    }
    pub fn play(&mut self,node:&Node,request:&Request,scope:Option<usize>,random:&mut dyn FnMut()->u16,sink:&mut impl Sink)->Result<u32> {
        match node {
            Node::Sound(source)=>match source.decode(){Ok(clip)=>sink.sound(clip,request,random),Err(error)=>{sink.reject(&format!("{error:#}"));Ok(2)}},
            Node::Unsupported(error)=>{sink.reject(error);Ok(2)},
            Node::Selector{id,settings,playlist,children}=>{
                let selected=self.selectors.select(*id,settings,playlist,scope,|id|children.contains_key(&id),||random() as u32)?;
                self.selections+=1;
                if let Some(id)=selected.selected {self.play(&children[&id],request,scope,random,sink)}else {Ok(2)}
            },
            Node::Switch{id,kind,group,definition,children}=>{
                //8032e590: kind0 queries this object's switch; kind1 queries
                // the global state manager, which is not implemented here.
                if *kind!=0 {sink.reject("Global state-group switch playback is not connected");return Ok(2);}
                let value=sink.switch_value(*group)?.unwrap_or(0);
                let Some(nodes)=self.switches.begin(*id,definition,value,request)? else {return Ok(2)};
                let mut successes=0;let mut status=2;
                for id in nodes {
                    if request.native_zero_words[4]!=0 && successes!=0 {continue;}
                    if let Some(node)=children.get(&id){status=self.play(node,request,scope,random,sink)?;if status==1{successes+=1;}}
                }Ok(status)
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(id:u32)->Arc<Node> {
        let clip=Arc::new(Clip{samples:Arc::from([0]),rate:32000,sound:id,gain_db:0.,channel_gains:[1.,1.,0.,0.],maximum_instances:0,lineage:Arc::from([id]),pitch_ratio:1.,low_pass:0.,pitch_cents:0.,random_ranges:Arc::from([]),spatial:None,signal:super::super::audio_signal::Signal::Pcm,source_passes:1,layout:None,admission:None});
        let decoded=OnceLock::new();assert!(decoded.set(Ok(clip.clone())).is_ok());
        Arc::new(Node::Sound(Sound{metadata:(*clip).clone(),bytes:Arc::from([]),range:0..0,decoded}))
    }
    #[derive(Default)]
    struct Capture {sounds:Vec<u32>,errors:Vec<String>}
    impl Sink for Capture {
        fn sound(&mut self,clip:Arc<Clip>,request:&Request,_:&mut dyn FnMut()->u16)->Result<u32>{assert_eq!(request.target,777);self.sounds.push(clip.sound);Ok(1)}
        fn switch_value(&mut self,_:u32)->Result<Option<u32>>{Ok(None)}
        fn reject(&mut self,error:&str){self.errors.push(error.into());}
    }
    #[test]
    fn original_dispatch_vectors_reach_the_selected_leaf_and_consume_shared_rng()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_dispatch.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let playlist=case["weights"].as_array().unwrap().iter().enumerate().map(|(i,w)|PlaylistItem{sound:100+i as u32,weight:w.as_u64().unwrap() as u8}).collect::<Vec<_>>();
            let settings=Settings{loops:1,transition:[0.;3],avoid_repeat:case["avoid"].as_u64().unwrap() as u16,transition_mode:0,random_mode:case["random_mode"].as_u64().unwrap() as u8,mode:case["mode"].as_u64().unwrap() as u8,unused_weight:0,reset_playlist:0,restart_backward:u8::from(case["reverse"].as_bool().unwrap()),continuous:0,global:u8::from(case["global_scope"].as_bool().unwrap())};
            let mut runtime=Runtime::default();let mut rng=super::super::game_random::State::default();
            for step in case["steps"].as_array().unwrap() {
                let owner=step["owner"].as_u64().unwrap() as usize;
                let children=step["loaded"].as_array().unwrap().iter().map(|id|{let id=id.as_u64().unwrap() as u32;(id,source(id))}).collect();
                let node=Node::Selector{id:777,settings:settings.clone(),playlist:playlist.clone(),children};
                let request=Request{target:777,context:super::super::audio_play_action::Context{object:owner as u32,..Default::default()},transition:0,curve:4,kind:0,native_zero_words:[0;5],native_flags:[0,1]};
                let mut sink=Capture::default();let status=runtime.play(&node,&request,Some(owner),&mut ||rng.draw() as u16,&mut sink)?;
                let expected=step["selected"].as_u64().map(|id|id as u32);
                assert_eq!(sink.sounds,expected.into_iter().collect::<Vec<_>>());assert_eq!(status,if expected.is_some(){1}else{2});
                assert!(sink.errors.is_empty());
                assert_eq!(rng.seed,step["seed"].as_u64().unwrap() as u32);assert_eq!(rng.draws,step["draws"].as_u64().unwrap());
            }
        }Ok(())
    }
    fn sequence()->Node {
        Node::Selector{id:777,settings:Settings{loops:1,transition:[0.;3],avoid_repeat:0,transition_mode:0,random_mode:0,mode:1,unused_weight:0,reset_playlist:0,restart_backward:0,continuous:0,global:0},playlist:vec![PlaylistItem{sound:100,weight:50},PlaylistItem{sound:101,weight:50}],children:BTreeMap::from([(100,source(100)),(101,source(101))])}
    }
    fn request(owner:u32)->Request {Request{target:777,context:super::super::audio_play_action::Context{object:owner,..Default::default()},transition:0,curve:4,kind:0,native_zero_words:[0;5],native_flags:[0,1]}}
    #[test]
    fn selected_unsupported_leaf_is_reported_without_playing_a_substitute()->Result<()> {
        let mut node=sequence();let Node::Selector{children,..}=&mut node else{unreachable!()};children.insert(100,Arc::new(Node::Unsupported("selected DSP is unsupported".into())));
        let mut runtime=Runtime::default();let mut sink=Capture::default();
        assert_eq!(runtime.play(&node,&request(7),Some(7),&mut ||panic!("Sequence drew random"),&mut sink)?,2);
        assert!(sink.sounds.is_empty());assert_eq!(sink.errors,["selected DSP is unsupported"]);
        assert_eq!(runtime.play(&node,&request(7),Some(7),&mut ||panic!("Sequence drew random"),&mut sink)?,1);assert_eq!(sink.sounds,[101]);Ok(())
    }
    #[test]
    fn history_outlives_unregistration_until_the_last_retained_owner_drops()->Result<()> {
        let mut events=super::super::audio_events::Events::default();let owner=events.register_object(7)?;let handle=owner.handle;
        let mut runtime=Runtime::default();runtime.retain_owner(&owner);let mut sink=Capture::default();
        runtime.play(&sequence(),&request(handle),Some(handle as usize),&mut ||panic!(),&mut sink)?;
        events.unregister_object(7);runtime.collect_retired();assert!(runtime.selectors.state(777,false,Some(handle as usize)).is_some());
        drop(owner);runtime.collect_retired();assert!(runtime.selectors.state(777,false,Some(handle as usize)).is_none());Ok(())
    }
    #[test]
    #[ignore="requires owned EM1 bank files"]
    fn original_graph_preparation_is_lazy_and_shares_selected_pcm()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes:Arc<[u8]>=std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?.into();
        let bank=Bank::parse(&bytes)?;let sound=bank.objects[&bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0]].action()?.target;
        let storage=BTreeMap::from([(bank.id,bytes.clone())]);let mut cache=Cache::new();let a=prepare(&bank,&storage,bank.id,sound,&mut cache)?;let b=prepare(&bank,&storage,bank.id,sound,&mut cache)?;assert!(Arc::ptr_eq(&a,&b));
        let Node::Sound(source)=&*a else {anyhow::bail!("Original dry event is no longer direct sound")};
        assert!(source.decoded.get().is_none());assert!(source.metadata.samples.is_empty());
        let first=source.decode()?;let second=source.decode()?;assert!(Arc::ptr_eq(&first,&second));assert!(!first.samples.is_empty());Ok(())
    }
}
