// The VM and GPU backend are constructed and owned on this thread. Only
// immutable game snapshots and completed premultiplied pixels cross threads.
use super::*;
use crate::blotworx::assets::World;
use std::sync::mpsc::{self,SyncSender,Receiver};
struct Completed {tick:u64,consumed:usize,image:std::result::Result<Option<egui::ColorImage>,String>}
struct Acknowledged {tick:u64,consumed:usize,ticket_dings:Vec<(i32,i32)>,pause:PauseOutcome}
struct PixelJob {reply:Option<Receiver<Completed>>,completed:Option<Completed>}
impl PixelJob {
    fn resolve(&mut self)->Result<&mut Completed> {
        if self.completed.is_none(){self.completed=Some(self.reply.take().context("HUD reply already consumed")?.recv().context("HUD worker stopped before completing this frame")?);}
        Ok(self.completed.as_mut().unwrap())
    }
    fn take_image(&mut self)->Result<Option<egui::ColorImage>> {self.resolve()?.image.as_mut().map_err(|e|anyhow::anyhow!(e.clone())).map(Option::take)}
}
mod overlay {include!("ui_gameplay_overlay.rs");}
enum Command {Frame(HudFrameInput,SyncSender<Completed>,Option<SyncSender<Acknowledged>>),Inspect(SyncSender<serde_json::Value>),ScrollDialogue(SyncSender<Result<bool>>),Shutdown}
struct Pending {job:Arc<Mutex<PixelJob>>,ack:Option<Receiver<Acknowledged>>,sent:Vec<HudMessage>,rect:egui::Rect,tick:u64,presented:bool}
pub struct GameplayWorker {
    overlay:Arc<Mutex<overlay::State>>,pub eager_presentation:bool,
    sender:SyncSender<Command>,thread:Option<std::thread::JoinHandle<()>>,pending:Option<Pending>,
    image:Option<egui::ColorImage>,texture:Option<egui::TextureHandle>,
    submitted:u64,completed:u64,deferred:u64,last_tick:Option<u64>,
    ticket_audio_posts:u64,
    ticket_pitch: f32,
    last_ticket_ding: f32,
    pause_lists: Option<Arc<PauseLists>>,
    pause_resume: bool,
    pause_quit: bool,
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
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||core.render_frame_acknowledged(&input,&mut messages,|consumed,ticket_dings,pause| {
                            if let Some(ack)=ack.take(){let _=ack.send(Acknowledged{tick:input.tick,consumed,ticket_dings,pause});}
                        })));
                        // Error/panic before VM acknowledgement still commits
                        // exactly the successful prefix, matching eager delivery.
                        if let Some(ack)=ack {let _=ack.send(Acknowledged{tick:input.tick,consumed:input.messages.len()-messages.len(),ticket_dings:core.take_ticket_dings(),pause:PauseOutcome::default()});}
                        let panicked=result.is_err();
                        let image=match result {Ok(result)=>result.map_err(|error|format!("{error:#}")),Err(error)=>Err(panic_text(error))};
                        let completed=Completed{tick:input.tick,consumed:input.messages.len()-messages.len(),image};
                        if reply.send(completed).is_err() || panicked {break;}
                    }
                    Command::Inspect(reply)=>{let _=reply.send(core.summary());}
                    Command::ScrollDialogue(reply)=>{let _=reply.send(core.scroll_dialogue());}
                    Command::Shutdown=>break,
                }
            }
        }).context("Start HUD worker")?;
        match loaded.recv() {
            Ok(Ok(()))=>Ok(Self{overlay:Default::default(),eager_presentation:false,sender,thread:Some(thread),pending:None,image:None,texture:None,
                submitted:0,completed:0,deferred:0,last_tick:None,ticket_audio_posts:0,ticket_pitch:0.,last_ticket_ding:0.,pause_lists:None,pause_resume:false,pause_quit:false,timings:Default::default()}),
            outcome=> {
                let _=thread.join();
                match outcome {Ok(Err(error))=>anyhow::bail!("{error}"),_=>anyhow::bail!("HUD worker stopped during initialization")}
            }
        }
    }
    pub(crate) fn prepare(&mut self,rect:egui::Rect,world:&mut World,tick:u64,pause_time:Option<f64>,pause_input:Vec<PauseInput>,cursor:Option<[f32;2]>)->Result<()> {
        self.submit(rect,world,tick,None,pause_time,pause_input,cursor)
    }
    pub(crate) fn take_pause_exit(&mut self) -> (bool, bool) {
        (std::mem::take(&mut self.pause_resume), std::mem::take(&mut self.pause_quit))
    }
    pub fn scroll_dialogue(&self) -> Result<bool> {
        let (reply, response) = mpsc::sync_channel(1);
        self.sender.send(Command::ScrollDialogue(reply)).map_err(|_| anyhow::anyhow!("HUD worker disconnected"))?;
        response.recv().context("HUD worker stopped before scrolling dialogue")?
    }
    pub fn present_movie(&mut self,ui:&egui::Ui,rect:egui::Rect,world:&mut World,tick:u64,movie:MovieHud)->Result<()> {
        self.submit(rect,world,tick,Some(movie),None,Vec::new(),None)?;
        self.present(ui,rect,world)
    }
    fn submit(&mut self,rect:egui::Rect,world:&mut World,tick:u64,movie:Option<MovieHud>,pause_time:Option<f64>,pause_input:Vec<PauseInput>,cursor:Option<[f32;2]>)->Result<()> {
        // A scene-presentation error can abort a previous UI frame. Commit its
        // successful message prefix before submitting a new snapshot; never
        // replay already delivered calls or leave an unbounded frame queue.
        if self.pending.as_ref().is_some_and(|pending|!pending.presented){self.deferred+=1;}
        self.finish_previous(world)?;
        let started=std::time::Instant::now();
        let mut input=HudFrameInput::from_world(rect,world,tick)?;input.movie=movie;input.pause_time=pause_time;input.cursor=cursor;
        if pause_time.is_some() {
            input.pause_input = pause_input;
            input.pause_map = Some(world.map_info.clone());
            input.pause_lists = Some(self.pause_lists.get_or_insert_with(|| Arc::new(PauseLists::from_world(world))).clone());
            let options = &world.game_options;
            input.pause_settings = [i32::from(options.left_handed), options.sfx_volume, options.music_volume];
            let defaults = super::super::save_options::State::from_settings(&world.runtime_settings)?;
            input.pause_defaults = [i32::from(defaults.left_handed), defaults.sfx_volume, defaults.music_volume];
            let (pins, pin_total) = world.collectibles.totals("Pin");
            let (repeated, _) = world.collectibles.totals("Pin_Inc");
            let (films, film_total) = world.collectibles.totals("Film_Reel");
            let pin_total = world.runtime_settings.get("TotalPinCount").and_then(|text| text.parse().ok()).unwrap_or(pin_total);
            input.pause_collectibles = [f64::from(pins.wrapping_add(repeated)), f64::from(pin_total),
                f64::from(films), f64::from(film_total)];
        }
        if pause_time.is_none() { self.pause_lists = None; }
        let sent=input.messages.clone();
        input.popups.extend(world.ui_notifications.iter().cloned());
        input.popups.extend(world.collectibles.notifications.iter().cloned());
        let(reply,receive)=mpsc::sync_channel(1);
        let(ack,acknowledged)=mpsc::sync_channel(1);
        self.sender.send(Command::Frame(input,reply,Some(ack))).map_err(|_|anyhow::anyhow!("HUD worker disconnected"))?;
        world.ui_notifications.clear();
        world.collectibles.notifications.clear();
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
        commit_hud_messages(world,&pending.sent,ack.consumed)?;
        self.pause_resume |= ack.pause.resume;
        self.pause_quit |= ack.pause.quit;
        if ack.pause.reset_options {
            world.game_options = super::super::save_options::State::from_settings(&world.runtime_settings)?;
        }
        if let Some(value) = ack.pause.music_volume { world.game_options.music_volume = value; }
        if let Some(value) = ack.pause.sfx_volume { world.game_options.sfx_volume = value; }
        if ack.pause.toggle_left_handed { world.game_options.left_handed ^= 1; }
        for event in ack.pause.sounds {
            world.post_audio_from_game(world.player_entity, &event);
        }
        for (previous, next) in ack.ticket_dings {
            let delta = next.wrapping_sub(previous);
            if delta <= 0 { continue; }
            let setting = |key, default| world.runtime_settings.get(key)
                .map(|value| crate::blotworx::config_value::decimal_float(value).map_err(anyhow::Error::msg))
                .transpose().map(|value| value.unwrap_or(default));
            let step = setting("TicketRTPCIncrementValue", 5.)?;
            let reset_time = setting("TicketRTPCResetTime", 10.)?;
            let reset_count = world.runtime_settings.get("TicketRTPCResetCount")
                .map_or(50, |value| crate::blotworx::native_text_number::atoi(value.as_bytes(), &mut 0))
                .max(1);
            let now = world.frame_clock.clock.scaled_seconds as f32;
            // RELed650..ed770: advance by the displayed delta, or seed from
            // the displayed total modulo the configured count after inactivity.
            self.ticket_pitch = if now - self.last_ticket_ding > reset_time {
                step * (next % reset_count) as f32
            } else {
                step.mul_add(delta as f32, self.ticket_pitch)
            };
            self.last_ticket_ding = now;
            let id = crate::blotworx::audio_bank::event_id("TicketCountUP")?;
            let playing = world.audio_playback.post_from_game_with_control(
                "Play_sfx_Ticket_CountUp", world.player_entity, Some((id, self.ticket_pitch))
            );
            self.ticket_audio_posts += u64::from(playing != 0);
        }
        Ok(())
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
            "completed":self.completed,"ticket_audio_posts":self.ticket_audio_posts,"deferred_presentations":self.deferred,"last_completed_tick":self.last_tick,
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
