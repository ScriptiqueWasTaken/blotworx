//! Serialized IGC packages. Pointer/vtable words are discarded; strings are
//! restored in the order verified against the original PC BuildIGCPackage.
//! Decoding does not imply that every event has a gameplay receiver.
use anyhow::{ensure,Context,Result};
use serde::Serialize;
use std::collections::BTreeMap;

const LAYOUT:[u32;15]=[68,0,160,136,108,116,188,76,188,60,148,128,96,100,204];
#[derive(Debug,Serialize)]
pub(super) struct Meta {
    pub offset:usize,pub name:String,pub kind:u32,pub child_count:u32,
    pub field_20:u32,pub field_24:u32,pub id:u32,pub flags:u32,
}
#[derive(Debug,Serialize)]
pub(super) struct Event {
    pub meta:Meta,
    /// Field offsets are part of the format, not user-facing properties.
    pub strings:BTreeMap<usize,String>,
    pub gamebryo_animation:Option<GamebryoAnimationEvent>,pub system_control:Option<SystemControl>,pub camera:Option<CameraEvent>,pub sound:Option<SoundEvent>,pub lua:Option<LuaEvent>,pub animation:Option<AnimationEvent>,pub dialogue:Option<DialogueEvent>,
}
#[derive(Debug,Serialize)]
pub(super) struct GamebryoAnimationEvent {pub mode:u32,pub scalar60:f32,pub scalar64:f32,pub options:[u16;3],pub entity_binding:[u32;2],pub name_binding:[u32;2]}
#[derive(Debug,Serialize)]
pub(super) struct DialogueEvent {pub selector:u32,pub entity_binding:[u32;2],pub name_binding:[u32;2]}
#[derive(Debug,Serialize)]
pub(super) struct AnimationEvent {pub flags:u32,pub integer:i32,pub scalar_bits:u32,pub bindings:[[u32;2];6]}
#[derive(Debug,Serialize)]
pub(super) struct SystemControl {pub flags:u32,pub duration:f32}
#[derive(Debug,Serialize)]
pub(super) struct CameraEvent {
    pub flags:u32,pub duration:f32,pub transition:u32,pub parameter_78:f32,pub parameter_7c:f32,
    pub option_8c:u16,pub option_8e:u16,pub option_90:u16,
}
#[derive(Debug,Serialize)]
pub(super) struct SoundEvent {
    pub mode:u32,pub parameter_bits:u32,
    /// Overloaded parameter index/type, separate from restored string data.
    pub entity_binding:[u32;2],pub name_binding:[u32;2],pub state_binding:[u32;2],
}
#[derive(Debug,Serialize)]
pub(super) struct LuaEvent {pub binding:[u32;2]}
#[derive(Debug,Serialize)]
pub(super) struct EntityControl {
    pub offset:usize,pub strings:BTreeMap<usize,String>,pub position:[f32;3],
    pub rotation:[f32;3],pub flags:u32,pub values:u32,pub bindings:[[u32;2];3],
}
#[derive(Debug,Serialize)]
pub(super) struct Hook {
    pub meta:Meta,pub frame:u32,pub time:f32,pub controls:Vec<EntityControl>,pub events:Vec<Event>,
}
#[derive(Debug,Serialize)]
pub(super) struct Scene {pub meta:Meta,pub frame_count:u32,pub duration:f32,pub start_updates:u32,pub hooks:Vec<Hook>}
#[derive(Debug,Serialize)]
pub(super) struct Package {pub meta:Meta,pub flags:u32,pub scenes:Vec<Scene>,pub cleanup:Scene}

struct Reader<'a> {data:&'a [u8],big:bool,pool:&'a [u8],string_pos:usize,pool_start:usize}
impl Reader<'_> {
    fn u32(&self,p:usize)->Result<u32> {
        let end=p.checked_add(4).context("IGC word overflow")?;
        let bytes=self.data.get(p..end).context("Truncated IGC word")?.try_into()?;
        Ok(if self.big{u32::from_be_bytes(bytes)}else{u32::from_le_bytes(bytes)})
    }
    fn u16(&self,p:usize)->Result<u16> {
        let bytes=self.data.get(p..p+2).context("Truncated IGC short")?.try_into()?;
        Ok(if self.big {u16::from_be_bytes(bytes)}else{u16::from_le_bytes(bytes)})
    }
    fn f32(&self,p:usize)->Result<f32> {let value=f32::from_bits(self.u32(p)?);ensure!(value.is_finite(),"Nonfinite IGC scalar at {p}");Ok(value)}
    fn vector(&self,p:usize)->Result<[f32;3]> {Ok([self.f32(p)?,self.f32(p+4)?,self.f32(p+8)?])}
    fn string(&mut self)->Result<String> {
        let tail=self.pool.get(self.string_pos..).context("IGC string cursor overflow")?;
        let len=tail.iter().position(|&b|b==0).context("Missing IGC string terminator")?;
        let value=std::str::from_utf8(&tail[..len]).context("IGC string encoding is not supported UTF-8/ASCII")?.to_owned();
        self.string_pos+=len+1;Ok(value)
    }
    fn strings(&mut self,offsets:&[usize])->Result<BTreeMap<usize,String>> {
        offsets.iter().map(|&offset|Ok((offset,self.string()?))).collect()
    }
    fn meta(&mut self,p:usize,kind:u32)->Result<Meta> {
        ensure!(self.u32(p+24)?==kind,"IGC object discriminator mismatch at {p}");
        Ok(Meta {offset:p,name:self.string()?,kind,child_count:self.u32(p+28)?,field_20:self.u32(p+32)?,field_24:self.u32(p+36)?,id:self.u32(p+40)?,flags:self.u32(p+44)?})
    }
    /// Stored advances are relative to the previous object. The first is0;
    /// subsequent entries advance across its complete variable-length data.
    fn spans(&self,table:usize,count:usize,end:usize,min:usize)->Result<Vec<(usize,usize)>> {
        ensure!(end<=self.pool_start,"IGC objects overlap the string pool");
        let size=count.checked_mul(4).context("IGC offset table overflow")?;
        let mut cursor=table.checked_add(size).context("IGC table overflow")?;
        ensure!(cursor<=end,"IGC offset table exceeds parent");
        let mut starts=Vec::with_capacity(count);
        for i in 0..count {
            cursor=cursor.checked_add(self.u32(table+i*4)? as usize).context("IGC advance overflow")?;
            ensure!(cursor.checked_add(min).is_some_and(|p|p<=end),"IGC child exceeds parent");
            if let Some(&previous)=starts.last(){ensure!(cursor>=previous+min,"Overlapping IGC children");}
            starts.push(cursor);
        }
        Ok(starts.iter().enumerate().map(|(i,&p)|(p,starts.get(i+1).copied().unwrap_or(end))).collect())
    }
    fn event(&mut self,p:usize,end:usize)->Result<Event> {
        let kind=self.u32(p)?;let p=p+4;
        let (size,fields): (usize,&[usize])=match kind {
            3=>(116,&[0x38,0x50]),
            4=>(188,&[0x38,0x50,0x64,0x78,0x8c,0xa0]),
            6=>(76,&[0x38]),7=>(60,&[]),8=>(148,&[0x38,0x50,0x68]),
            9=>(128,&[0x38,0x50,0x64]),11=>(100,&[0x38,0x50]),
            _=>anyhow::bail!("IGC event type {kind} string/payload decoder is not connected"),
        };
        ensure!(p.checked_add(size).is_some_and(|p|p<=end),"Truncated IGC event {kind}");
        // Derived fields precede the base object's name in the string stream.
        let strings=self.strings(fields)?;
        let system_control=if kind==7 {Some(SystemControl{flags:self.u32(p+0x34)?,duration:self.f32(p+0x38)?})}else{None};
        let camera=if kind==8 {Some(CameraEvent {flags:self.u32(p+0x80)?,duration:self.f32(p+0x84)?,transition:self.u32(p+0x88)?,parameter_78:self.f32(p+0x78)?,parameter_7c:self.f32(p+0x7c)?,option_8c:self.u16(p+0x8c)?,option_8e:self.u16(p+0x8e)?,option_90:self.u16(p+0x90)?})}else{None};
        let sound=if kind==9 {Some(SoundEvent {
            mode:self.u32(p+0x78)?,parameter_bits:self.u32(p+0x74)?,
            entity_binding:[self.u32(p+0x3c)?,self.u32(p+0x40)?],
            name_binding:[self.u32(p+0x54)?,self.u32(p+0x58)?],
            state_binding:[self.u32(p+0x68)?,self.u32(p+0x6c)?],
        })}else{None};
        let lua=if kind==6 {Some(LuaEvent {binding:[self.u32(p+0x3c)?,self.u32(p+0x40)?]})}else{None};
        let animation=if kind==4 {let mut bindings=[[0;2];6];
            for (i,offset) in [0x3c,0x54,0x68,0x7c,0x90,0xa4].into_iter().enumerate(){bindings[i]=[self.u32(p+offset)?,self.u32(p+offset+4)?];}
            Some(AnimationEvent {flags:self.u32(p+0xb8)?,integer:self.u32(p+0xb0)? as i32,scalar_bits:self.u32(p+0xb4)?,bindings})
        }else{None};
        let dialogue=if kind==11 {Some(DialogueEvent {selector:self.u32(p+0x60)?,entity_binding:[self.u32(p+0x3c)?,self.u32(p+0x40)?],name_binding:[self.u32(p+0x54)?,self.u32(p+0x58)?]})}else{None};
        let gamebryo_animation=if kind==3 {Some(GamebryoAnimationEvent {mode:self.u32(p+0x68)?,scalar60:self.f32(p+0x60)?,scalar64:self.f32(p+0x64)?,options:[self.u16(p+0x6c)?,self.u16(p+0x6e)?,self.u16(p+0x70)?],entity_binding:[self.u32(p+0x3c)?,self.u32(p+0x40)?],name_binding:[self.u32(p+0x54)?,self.u32(p+0x58)?]})}else{None};
        Ok(Event {meta:self.meta(p,kind)?,strings,gamebryo_animation,system_control,camera,sound,lua,animation,dialogue})
    }
    fn hook(&mut self,p:usize,end:usize)->Result<Hook> {
        ensure!(p+108<=end,"Truncated IGC hook");let meta=self.meta(p,2)?;
        ensure!(meta.child_count<=10,"IGC hook exceeds native ten-event layout");
        let count=self.u32(p+92)? as usize;
        let table=(p+108).checked_add(count.checked_mul(104).context("IGC entity controls overflow")?).context("IGC controls overflow")?;
        ensure!(table<=end,"IGC controls exceed hook");let mut controls=Vec::with_capacity(count);
        for i in 0..count {
            let c=p+108+i*104;
            controls.push(EntityControl {offset:c,strings:self.strings(&[4,28,52])?,position:self.vector(c+72)?,rotation:self.vector(c+84)?,flags:self.u32(c+96)?,values:self.u32(c+100)?,
                bindings:[[self.u32(c+8)?,self.u32(c+12)?],[self.u32(c+32)?,self.u32(c+36)?],[self.u32(c+56)?,self.u32(c+60)?]]});
        }
        let spans=self.spans(table,meta.child_count as usize,end,4+52)?;
        let events=spans.into_iter().map(|(p,end)|self.event(p,end)).collect::<Result<_>>()?;
        Ok(Hook {meta,frame:self.u32(p+96)?,time:self.f32(p+100)?,controls,events})
    }
    fn scene(&mut self,p:usize,end:usize)->Result<Scene> {
        ensure!(p+136<=end,"Truncated IGC scene");let meta=self.meta(p,1)?;
        ensure!(meta.child_count<=12,"IGC scene exceeds native twelve-hook layout");
        let spans=self.spans(p+136,meta.child_count as usize,end,108)?;
        let hooks=spans.into_iter().map(|(p,end)|self.hook(p,end)).collect::<Result<_>>()?;
        Ok(Scene {meta,frame_count:self.u32(p+48)?,duration:self.f32(p+56)?,start_updates:self.u32(p+52)?,hooks})
    }
}
impl Package {
    pub fn parse(data:&[u8])->Result<Self> {
        ensure!(data.len()>=68,"Truncated IGC header");
        let big=match &data[..4] {[68,0,0,0]=>false,[0,0,0,68]=>true,_=>anyhow::bail!("Unsupported IGC header size/endianness")};
        let mut r=Reader {data,big,pool:&[],string_pos:0,pool_start:0};
        for (i,&size) in LAYOUT.iter().enumerate(){ensure!(r.u32(i*4)?==size,"Unsupported IGC version/layout field {i}");}
        let start=r.u32(60)? as usize;let size=r.u32(64)? as usize;
        let end=start.checked_add(size).context("IGC string pool overflow")?;
        ensure!(start>=68+160&&end==data.len(),"Invalid IGC string pool range");
        r.pool=&data[start..end];r.pool_start=start;
        let meta=r.meta(68,0)?;ensure!(meta.child_count<=20,"IGC package exceeds native twenty-scene layout");
        let spans=r.spans(68+160,meta.child_count as usize+1,start,136)?;
        let mut scenes=spans.into_iter().map(|(p,end)|r.scene(p,end)).collect::<Result<Vec<_>>>()?;
        let cleanup=scenes.pop().context("Missing IGC cleanup scene")?;
        ensure!(r.string_pos==r.pool.len(),"IGC string stream has {} unclaimed bytes",r.pool.len()-r.string_pos);
        Ok(Self {meta,flags:r.u32(68+56)?,scenes,cleanup})
    }
}

struct Resource {path:String,key:u32,package:Package}
#[derive(Serialize)]
struct Prepared {
    resource:usize,requested_name:String,clock:super::igc_timeline::Clock,
    package:super::igc_package::State,active_scene:Option<usize>,start_counter:u32,
    hook:usize,control:usize,event:usize,first_update:bool,started:bool,
    dispatched_events:u64,blocked:Option<String>,
}
#[derive(Default)]
pub(super) struct Catalog {
    resources:Vec<Resource>,errors:Vec<serde_json::Value>,pending:Option<Prepared>,playtest:bool,
}
impl Catalog {
    pub fn load(files:&[epic_mickey_lib_rs::packfile::VirtualFile])->Self {
        let mut catalog=Self::default();let mut paths=std::collections::HashSet::new();
        for file in files.iter().filter(|file|file.path.to_ascii_lowercase().ends_with(".igcx")) {
            if !paths.insert(file.path.replace('\\',"/").to_ascii_lowercase()){continue;}
            let result=(||->Result<Resource> {
                let filename=file.path.rsplit(['/', '\\']).next().context("Missing IGC filename")?;
                let stem=filename.rsplit_once('.').context("Missing IGC extension")?.0;
                Ok(Resource {path:file.path.clone(),key:super::named_effects::lookup_hash(stem)?,package:Package::parse(&file.data)?})
            })();
            match result {
                Ok(resource)=>catalog.resources.push(resource),
                Err(error)=>catalog.errors.push(serde_json::json!({"path":file.path,"error":format!("{error:#}")})),
            }
        }
        catalog
    }
    pub fn camera_targets(&self)->impl Iterator<Item=String>+'_ {
        self.resources.iter().flat_map(|r|r.package.scenes.iter().chain(std::iter::once(&r.package.cleanup)))
            .flat_map(|s|&s.hooks).flat_map(|h|&h.events).filter(|e|matches!(e.meta.kind,3|8))
            .filter_map(|e|e.strings.get(&0x38).cloned())
    }
    pub fn prepare(&mut self,name:&str)->Result<()> {
        let key=super::named_effects::lookup_hash(name)?;
        // PC a203a0 compares the stripped-filename hash, in registration order.
        // It does not reinterpret arbitrary missing names as another cutscene.
        let resource=self.resources.iter().position(|r|r.key==key)
            .with_context(||format!("IGC {name} has no successfully decoded loaded package"))?;
        ensure!(self.pending.is_none(),"An unresolved IGC preparation is already retained");
        self.pending=Some(Prepared {resource,requested_name:name.to_owned(),clock:Default::default(),package:Default::default(),active_scene:Some(0),start_counter:0,hook:0,control:0,event:0,first_update:true,started:false,dispatched_events:0,blocked:None});
        Ok(())
    }
    pub fn enable_playtest(&mut self){self.playtest=true;}
    pub fn prepared_scenes(&self)->Result<impl Iterator<Item=&Scene>> {
        let pending=self.pending.as_ref().context("IGC binding has no prepared package")?;
        let package=&self.resources[pending.resource].package;
        Ok(package.scenes.iter().chain(std::iter::once(&package.cleanup)))
    }
    pub fn start_playtest(&mut self)->Result<()> {
        ensure!(self.playtest,"Native IGC admission/timeline is not connected outside explicit shared playtests");
        self.pending.as_mut().context("IGC start has no prepared package")?.started=true;Ok(())
    }
    pub fn advance_playtest(&mut self,delta:f32,mut dispatch:impl FnMut(&Hook,Option<&EntityControl>,Option<&Event>)->Result<()>)->Result<()> {
        let Some(pending)=self.pending.as_mut().filter(|p|p.started) else{return Ok(())};
        if let Some(error)=&pending.blocked{anyhow::bail!("{error}");}
        let package=&self.resources[pending.resource].package;
        let result=(||->Result<()> {
            use super::igc_package::Step;
            let delta=if pending.first_update{pending.first_update=false;0.}else{delta};
            let status=if pending.clock.complete {2}else{1};
            // The cleanup scene owns a separate, initially unstarted clock.
            // A completed regular scene must not make cleanup look complete.
            let cleanup_status=if pending.active_scene==Some(package.scenes.len()){status}else{0};
            let step=pending.package.advance(delta,package.scenes.len(),package.flags,false,status,Some(cleanup_status));
            let index=match step {
                Step::Scene(index)=>index,
                Step::Cleanup=>package.scenes.len(),
                Step::Wait=>return Ok(()),
                Step::End=>anyhow::bail!("IGC End player/camera/global notifications are not connected"),
            };
            if pending.active_scene!=Some(index) {
                pending.active_scene=Some(index);pending.clock=Default::default();
                pending.hook=0;pending.control=0;pending.event=0;pending.start_counter=0;
            }
            let scene=package.scenes.get(index).unwrap_or(&package.cleanup);
            // PC af5120 checks warmup before incrementing; a positive update
            // reaching the threshold starts only on the following call.
            if scene.meta.flags&1==0 && pending.start_counter<scene.start_updates {
                if delta>0. {pending.start_counter=pending.start_counter.wrapping_add(1);}
                return Ok(());
            }
            let advanced=pending.clock.update(delta,scene.meta.flags&1!=0,false)?;
            if !advanced{return Ok(());}
            while let Some(hook)=scene.hooks.get(pending.hook) {
                if !pending.clock.due(true,hook.frame,hook.time){break;}
                if hook.meta.flags&1==0 {
                    while let Some(control)=hook.controls.get(pending.control) {dispatch(hook,Some(control),None)?;pending.control+=1;}
                    while let Some(event)=hook.events.get(pending.event) {
                        if event.meta.flags&1==0 {dispatch(hook,None,Some(event))?;pending.dispatched_events+=1;}
                        pending.event+=1;
                    }
                }
                pending.hook+=1;pending.control=0;pending.event=0;
            }
            pending.clock.finish(true,scene.frame_count,scene.duration);
            Ok(())
        })();
        if let Err(error)=result {let message=format!("IGC {} scene {} hook {} control {} event {}: {error:#}",pending.requested_name,package.scenes.get(pending.active_scene.unwrap_or(0)).unwrap_or(&package.cleanup).meta.name,pending.hook,pending.control,pending.event);pending.blocked=Some(message.clone());anyhow::bail!("{message}");}
        Ok(())
    }
    pub fn summary(&self)->serde_json::Value {
        let resources=self.resources.iter().map(|r|serde_json::json!({"path":r.path,"name":r.package.meta.name,
            "scenes":r.package.scenes.len(),"hooks":r.package.scenes.iter().chain(std::iter::once(&r.package.cleanup)).map(|s|s.hooks.len()).sum::<usize>(),
            "events":r.package.scenes.iter().chain(std::iter::once(&r.package.cleanup)).flat_map(|s|&s.hooks).map(|h|h.events.len()).sum::<usize>()})).collect::<Vec<_>>();
        serde_json::json!({"resources":resources,"errors":self.errors,"prepared":self.pending,
            "playback_started":self.pending.as_ref().is_some_and(|p|p.started),"native_admission_verified":false,
            "scope":"Explicit shared-playtest package scene/cleanup sequencing with source time-based hooks and retained cursors; initial admission, blocking/hold release, End notifications and complete receivers remain pending"})
    }
}
