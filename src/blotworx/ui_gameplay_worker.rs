// The VM and GPU backend are constructed and owned on this thread. Only
// immutable game snapshots and completed premultiplied pixels cross threads.
use super::*;
use crate::blotworx::assets::World;
use std::sync::mpsc::{self,SyncSender,Receiver};
struct Completed {tick:u64,consumed:usize,image:std::result::Result<Option<egui::ColorImage>,String>}
struct Acknowledged {tick:u64,consumed:usize}
struct PixelJob {reply:Option<Receiver<Completed>>,completed:Option<Completed>}
impl PixelJob {
    fn resolve(&mut self)->Result<&mut Completed> {
        if self.completed.is_none(){self.completed=Some(self.reply.take().context("HUD reply already consumed")?.recv().context("HUD worker stopped before completing this frame")?);}
        Ok(self.completed.as_mut().unwrap())
    }
    fn take_image(&mut self)->Result<Option<egui::ColorImage>> {self.resolve()?.image.as_mut().map_err(|e|anyhow::anyhow!(e.clone())).map(Option::take)}
}
mod overlay {include!("ui_gameplay_overlay.rs");}
enum Command {Frame(HudFrameInput,SyncSender<Completed>,Option<SyncSender<Acknowledged>>),Inspect(SyncSender<serde_json::Value>),Shutdown}
struct Pending {job:Arc<Mutex<PixelJob>>,ack:Option<Receiver<Acknowledged>>,sent:Vec<HudMessage>,rect:egui::Rect,tick:u64,presented:bool}
pub struct GameplayWorker {
    overlay:Arc<Mutex<overlay::State>>,pub eager_presentation:bool,
    sender:SyncSender<Command>,thread:Option<std::thread::JoinHandle<()>>,pending:Option<Pending>,
    image:Option<egui::ColorImage>,texture:Option<egui::TextureHandle>,
    submitted:u64,completed:u64,deferred:u64,last_tick:Option<u64>,
    timings:std::collections::BTreeMap<&'static str,HudTiming>,
}
fn panic_text(error:Box<dyn std::any::Any+Send>)->String {
    let text=error.downcast_ref::<String>().map(String::as_str)
        .or_else(||error.downcast_ref::<&str>().copied()).unwrap_or("unknown panic");
    format!("HUD worker panicked: {text}")
}
impl GameplayWorker {
    pub fn load(root:&Path)->Result<Self> {Self::load_with_reuse(root,true)}
    pub fn load_with_reuse(root:&Path,reuse:bool)->Result<Self> {Self::load_with_graphics(root,reuse,false)}
    pub fn load_with_graphics(root:&Path,reuse:bool,diagnostic:bool)->Result<Self> {
        let root=root.to_owned();let(sender,commands)=mpsc::sync_channel(1);let(ready,loaded)=mpsc::sync_channel(1);
        let thread=std::thread::Builder::new().name("Blotworx HUD".into()).spawn(move|| {
            let load=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||Gameplay::load_with_graphics(&root,reuse,diagnostic)));
            let mut core=match load {
                Ok(Ok(core))=>core,
                Ok(Err(error))=>{let _=ready.send(Err(format!("{error:#}")));return;},
                Err(error)=>{let _=ready.send(Err(panic_text(error)));return;},
            };
            if ready.send(Ok(())).is_err(){return;}
            while let Ok(command)=commands.recv() {
                match command {
                    Command::Frame(input,reply,mut ack)=> {
                        let mut messages=input.messages.clone();
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||core.render_frame_acknowledged(&input,&mut messages,|consumed| {
                            if let Some(ack)=ack.take(){let _=ack.send(Acknowledged{tick:input.tick,consumed});}
                        })));
                        // Error/panic before VM acknowledgement still commits
                        // exactly the successful prefix, matching eager delivery.
                        if let Some(ack)=ack {let _=ack.send(Acknowledged{tick:input.tick,consumed:input.messages.len()-messages.len()});}
                        let panicked=result.is_err();
                        let image=match result {Ok(result)=>result.map_err(|error|format!("{error:#}")),Err(error)=>Err(panic_text(error))};
                        let completed=Completed{tick:input.tick,consumed:input.messages.len()-messages.len(),image};
                        if reply.send(completed).is_err() || panicked {break;}
                    }
                    Command::Inspect(reply)=>{let _=reply.send(core.summary());}
                    Command::Shutdown=>break,
                }
            }
        }).context("Start HUD worker")?;
        match loaded.recv() {
            Ok(Ok(()))=>Ok(Self{overlay:Default::default(),eager_presentation:false,sender,thread:Some(thread),pending:None,image:None,texture:None,
                submitted:0,completed:0,deferred:0,last_tick:None,timings:Default::default()}),
            outcome=> {
                let _=thread.join();
                match outcome {Ok(Err(error))=>anyhow::bail!("{error}"),_=>anyhow::bail!("HUD worker stopped during initialization")}
            }
        }
    }
    pub fn prepare(&mut self,rect:egui::Rect,world:&mut World,tick:u64)->Result<()> {
        // A scene-presentation error can abort a previous UI frame. Commit its
        // successful message prefix before submitting a new snapshot; never
        // replay already delivered calls or leave an unbounded frame queue.
        if self.pending.as_ref().is_some_and(|pending|!pending.presented){self.deferred+=1;}
        self.finish_previous(world)?;
        let started=std::time::Instant::now();
        let input=HudFrameInput::from_world(rect,world,tick)?;let sent=input.messages.clone();
        let(reply,receive)=mpsc::sync_channel(1);
        let(ack,acknowledged)=mpsc::sync_channel(1);
        self.sender.send(Command::Frame(input,reply,Some(ack))).map_err(|_|anyhow::anyhow!("HUD worker disconnected"))?;
        self.pending=Some(Pending{job:Arc::new(Mutex::new(PixelJob{reply:Some(receive),completed:None})),ack:Some(acknowledged),sent,rect,tick,presented:false});self.submitted+=1;
        self.timings.entry("snapshot_and_submit").or_default().record(started.elapsed());Ok(())
    }
    fn acknowledge(&mut self,world:&mut World)->Result<()> {
        let pending=self.pending.as_mut().context("No current HUD frame was submitted")?;
        let Some(reply)=pending.ack.take() else{return Ok(())};
        let started=std::time::Instant::now();
        let ack=reply.recv().context("HUD worker stopped before acknowledging this frame")?;
        self.timings.entry("wait_for_vm_acknowledgement").or_default().record(started.elapsed());
        ensure!(ack.tick==pending.tick,"HUD acknowledged a different simulation tick");
        commit_hud_messages(world,&pending.sent,ack.consumed)
    }
    pub fn finish_previous(&mut self,world:&mut World)->Result<()> {
        if let Some(error)=self.overlay.lock().unwrap().error.clone(){anyhow::bail!("{error}");}
        if self.pending.is_none(){return Ok(())}
        self.acknowledge(world)?;
        let pending=self.pending.take().unwrap();let started=std::time::Instant::now();
        let mut job=pending.job.lock().unwrap();
        ensure!(job.resolve()?.tick==pending.tick,"HUD returned a different simulation tick");
        let image=job.take_image()?;
        self.timings.entry("wait_for_current_frame").or_default().record(started.elapsed());
        if let Some(image)=image {if self.eager_presentation{self.image=Some(image)}else{self.overlay.lock().unwrap().image=Some(image);}}
        self.completed+=1;self.last_tick=Some(pending.tick);Ok(())
    }
    pub fn present(&mut self,ui:&egui::Ui,rect:egui::Rect,world:&mut World)->Result<()> {
        let expected=self.pending.as_ref().context("HUD frame was not prepared")?.rect;
        self.acknowledge(world)?;
        // Logical-point rounding only; do not stretch a frame initialized for
        // another viewport. The worker receives the current frame's exact size.
        ensure!((expected.width()-rect.width()).abs()<0.001 && (expected.height()-rect.height()).abs()<0.001,
            "HUD prepared for {:?}, actual viewport {:?}",expected.size(),rect.size());
        self.pending.as_mut().unwrap().presented=true;
        if !self.eager_presentation {
            overlay::schedule(ui,rect,self.pending.as_ref().unwrap().job.clone(),self.overlay.clone());return Ok(());
        }
        self.finish_previous(world)?;
        if let Some(image)=self.image.take() {
            let started=std::time::Instant::now();
            if let Some(texture)=&mut self.texture{texture.set(image,egui::TextureOptions::LINEAR)}else{
                self.texture=Some(ui.ctx().load_texture("original-gameplay-hud",image,egui::TextureOptions::LINEAR));}
            self.timings.entry("texture_upload_queue").or_default().record(started.elapsed());
        }
        if let Some(texture)=&self.texture {ui.painter().with_clip_rect(rect).image(texture.id(),rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}
        Ok(())
    }
    pub fn destroy_graphics(&mut self){overlay::destroy(&self.overlay);overlay::collect(true);}
    pub fn destroy_retired_graphics(){overlay::collect(true);}
    // Read only local counters. Never queue an Inspect command behind a frame
    // or wait for the VM/GPU merely to display/export timing information.
    pub fn performance_snapshot(&self)->serde_json::Value {
        serde_json::json!({"main_thread":self.timings,"compositor":overlay::summary(&self.overlay),
            "submitted":self.submitted,"completed":self.completed,
            "pending_tick":self.pending.as_ref().map(|pending|pending.tick)})
    }
    pub fn performance_lines(&self)->Vec<String> {
        let mut lines=Vec::new();
        for (key,label) in [("snapshot_and_submit","HUD snapshot"),("wait_for_vm_acknowledgement","HUD script wait")] {
            if let Some(timing)=self.timings.get(key) {crate::blotworx::performance::append_line(&mut lines,label,timing);}
        }
        overlay::timing_lines(&self.overlay,&mut lines);lines
    }
    pub fn summary(&self)->serde_json::Value {
        let(reply,receive)=mpsc::sync_channel(1);
        let mut value=if self.sender.send(Command::Inspect(reply)).is_ok() {
            receive.recv().unwrap_or_else(|_|serde_json::json!({"error":"HUD worker stopped during inspection"}))
        }else{serde_json::json!({"error":"HUD worker disconnected"})};
        value["worker"]=serde_json::json!({"same_frame":true,"in_flight_limit":1,"submitted":self.submitted,
            "completed":self.completed,"deferred_presentations":self.deferred,"last_completed_tick":self.last_tick,
            "pending_tick":self.pending.as_ref().map(|pending|pending.tick),"main_thread_timings":self.timings,
            "presentation":if self.eager_presentation{"eager_texture_delta"}else{"same_frame_after_scene_submission"}});
        value["compositor"]=overlay::summary(&self.overlay);value
    }
}
impl Drop for GameplayWorker {
    fn drop(&mut self) {
        let _=self.sender.send(Command::Shutdown);
        if let Some(thread)=self.thread.take(){let _=thread.join();}
        overlay::retire(self.overlay.clone());
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]#[ignore="requires original EM1 data and GPU in BLOTWORX_TEST_DATA"]
    fn worker_matches_direct_pixels_messages_and_same_tick_resize()->Result<()> {
        let root=PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?);
        let mut world=crate::blotworx::assets::load(&root,"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let mut direct=Gameplay::load(&root)?;let worker=GameplayWorker::load(&root)?;
        let compare=|input:HudFrameInput,direct:&mut Gameplay,world:&mut World|->Result<()> {
            let(reply,receive)=mpsc::sync_channel(1);
            let(ack,acknowledged)=mpsc::sync_channel(1);
            worker.sender.send(Command::Frame(input.clone(),reply,Some(ack))).map_err(|_|anyhow::anyhow!("Worker disconnected"))?;
            let ack=acknowledged.recv()?;
            let mut messages=input.messages.clone();let image=direct.render_frame(&input,&mut messages)?;
            let completed=receive.recv()?;let remote=completed.image.map_err(anyhow::Error::msg)?;
            ensure!(completed.tick==input.tick,"Worker tick differs");
            ensure!(completed.consumed==input.messages.len()-messages.len(),"Worker message prefix differs");
            ensure!(ack.tick==input.tick && ack.consumed==completed.consumed,"VM acknowledgement differs from completed frame");
            match (image,remote) {
                (Some(a),Some(b))=>ensure!(a.size==b.size && a.pixels==b.pixels,"HUD pixels differ at tick {}",input.tick),
                (None,None)=>{},_=>anyhow::bail!("HUD render decision differs at tick {}",input.tick),
            }
            commit_hud_messages(world,&input.messages,completed.consumed)?;Ok(())
        };
        let rect=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(853.,480.));
        for tick in 1..=90 {
            if tick==61{world.enable_demo_abilities()?;}
            compare(HudFrameInput::from_world(rect,&world,tick)?,&mut direct,&mut world)?;
        }
        let resized=egui::Rect::from_min_size(egui::pos2(11.,7.),egui::vec2(640.,480.));
        compare(HudFrameInput::from_world(resized,&world,90)?,&mut direct,&mut world)?;
        let native=direct.summary();let remote=worker.summary();
        for key in ["display","values","frames","native_initialized","delivered_ability_messages","errors"] {
            ensure!(native[key]==remote[key],"Worker summary differs: {key}");
        }
        ensure!(direct.last_tick==90,"Resize advanced movie simulation time");
        ensure!(world.ability_hud_requests.is_empty(),"Worker did not acknowledge messages");
        std::fs::write("target/hud-worker-pixel-validation.json",serde_json::to_vec_pretty(&serde_json::json!({
            "compared_ticks":90,"same_tick_resize":true,"exact_premultiplied_pixels":true,"messages_exact":true,
            "direct":native,"worker":remote,"original_game_parity_validated":false}))?)?;
        Ok(())
    }
}
