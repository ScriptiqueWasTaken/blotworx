#[cfg(feature="blotworx-ui-render")]
mod gpu {include!("ui_gpu.rs");}
#[cfg(feature="blotworx-ui-render")]
mod render_cache {include!("ui_render_cache.rs");}
#[cfg(feature="blotworx-ui-render")]
type HudRenderer=render_cache::Cached<ruffle_render_wgpu::backend::WgpuRenderBackend<ruffle_render_wgpu::target::TextureTarget>>;
// Included inside ui_movie so the gameplay owner uses the same movie adapter,
// asset loader and synchronous FSCommand services as the original-data probe.
#[cfg(feature="blotworx-ui-render")]
pub struct Gameplay {
    handle:Arc<Mutex<ruffle_core::Player>>,executor:NullExecutor,host:Host,
    initialized:bool,last_tick:u64,aspect:f32,visible:Option<bool>,
    values:std::collections::BTreeMap<String,f64>,texture:Option<egui::TextureHandle>,
    pub errors:BTreeSet<String>,pub frames:u64,pub delivered_ability_messages:u64,
    timings:std::collections::BTreeMap<&'static str,HudTiming>,
    graphics:gpu::Info,
}

/// Recent windows stay bounded independently of session length.
#[cfg(feature="blotworx-ui-render")]
type HudTiming = super::performance::Timing;

#[cfg(feature="blotworx-ui-render")]
type HudMessage=(&'static str,&'static str,&'static str);
#[cfg(feature="blotworx-ui-render")]
#[derive(Clone)]
struct HudFrameInput {data:Arc<PlayerData>,visible:bool,tick:u64,rect:egui::Rect,messages:Vec<HudMessage>}
#[cfg(feature="blotworx-ui-render")]
impl HudFrameInput {
    fn from_world(rect:egui::Rect,world:&super::assets::World,tick:u64)->Result<Self> {
        let visible = (!world.demo_story.enabled || world.demo_story.hud)
            && !world.screen_fade.hide_gameplay_hud;
        Ok(Self{data:Arc::new(PlayerData::gameplay(world)?),visible,
            tick,rect,messages:world.ability_hud_requests.clone()})
    }
}
#[cfg(feature="blotworx-ui-render")]
fn commit_hud_messages(world:&mut super::assets::World,sent:&[HudMessage],consumed:usize)->Result<()> {
    ensure!(consumed<=sent.len() && world.ability_hud_requests.get(..consumed)==sent.get(..consumed),"HUD message prefix changed during scene presentation");
    world.ability_hud_requests.drain(..consumed);Ok(())
}
#[cfg(feature="blotworx-ui-render")]
mod gameplay_worker {include!("ui_gameplay_worker.rs");}
#[cfg(feature="blotworx-ui-render")]
pub use gameplay_worker::GameplayWorker;

#[cfg(all(any(test,debug_assertions),feature="blotworx-ui-render"))]
/// Original-asset diagnostic for exact GPU-target reuse. Uses real movie frames,
/// visibility, ammo and aspect changes; it does not establish original-render parity.
pub fn validate_exact_hud_reuse(root:&Path,report:&Path)->Result<()> {
    let data_root=root.canonicalize()?;
    let parent=report.parent().context("HUD validation report has no parent")?.canonicalize()?;
    ensure!(!parent.starts_with(&data_root),"HUD validation output must be outside game data");

        let mut world=super::assets::load(&root,"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let mut baseline=Gameplay::load_with_graphics(&root,false,false)?;let mut cached=Gameplay::load(&root)?;
        let mut baseline_image:Option<egui::ColorImage>=None;let mut cached_image:Option<egui::ColorImage>=None;
        for tick in 1..=300 {
            if tick==31{world.enable_demo_abilities()?;}
            let size=if tick<121{egui::vec2(853.,480.)}else{egui::vec2(640.,480.)};
            let mut input=HudFrameInput::from_world(egui::Rect::from_min_size(egui::Pos2::ZERO,size),&world,tick)?;
            input.visible=!(91..111).contains(&tick);
            if tick>=61 {
                let mut inventory=input.data.inventory.clone();
                if let Some(values)=&mut inventory{values.insert("Paint".into(),if tick<81{(81-tick) as f32}else{0.});}
                input.data=Arc::new(PlayerData{health:input.data.health.clone(),inventory,level:input.data.level.clone(),entity:input.data.entity.clone()});
            }
            let mut a=input.messages.clone();let mut b=a.clone();
            if let Some(image)=baseline.render_frame(&input,&mut a)?{baseline_image=Some(image);}
            if let Some(image)=cached.render_frame(&input,&mut b)?{cached_image=Some(image);}
            ensure!(a==b,"Message acknowledgement differs at tick {tick}");
            let consumed=input.messages.len()-a.len();
            let a=baseline_image.as_ref().context("No baseline image")?;let b=cached_image.as_ref().context("No cached image")?;
            ensure!(a.size==b.size&&a.pixels==b.pixels,"Reused HUD pixels differ at tick {tick}");
            commit_hud_messages(&mut world,&input.messages,consumed)?;
        }
        let a=baseline.summary();let b=cached.summary();
        for key in ["frames","last_tick","display","values","native_initialized","delivered_ability_messages","errors"]{ensure!(a[key]==b[key],"Native state differs: {key}");}
        std::fs::write(report,serde_json::to_vec_pretty(&serde_json::json!({
            "ticks":300,"exact_pixels_every_tick":true,"visibility_ammo_and_aspect_changes":true,"baseline":a,"cached":b,"original_game_parity_validated":false}))?)?;
        ensure!(b["render_reuse"]["identical_frames"].as_u64().unwrap_or(0)>0,"Original HUD produced no reusable targets; report saved");
        Ok(())
    
}

#[cfg(all(test,feature="blotworx-ui-render"))]
mod gameplay_tests {
    use super::*;
    #[test]#[ignore="requires original EM1 files and GPU in BLOTWORX_TEST_DATA"]
    fn exact_vector_reuse_matches_forced_registration_through_updates_and_resize()->Result<()> {
        let root=PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?);
        validate_exact_hud_reuse(&root,Path::new("target/hud-exact-reuse-pixel-validation.json"))
    }
    #[test]#[ignore="requires original EM1 files and GPU in BLOTWORX_TEST_DATA"]
    fn original_movie_receives_ability_ammo_visibility_and_live_values()->Result<()> {
        let root=std::path::PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?);
        let ctx=egui::Context::default();
        let mut world=super::super::assets::load(&root,"Levels/GV_ZoneA_Start.level",&ctx)?;
        let mut hud=Gameplay::load(&root)?;
        fn frame(ctx:&egui::Context,hud:&mut Gameplay,world:&mut super::super::assets::World,tick:u64,size:[f32;2])->Result<()> {
            let mut outcome=Ok(());
            let mut output=ctx.run_ui(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(size[0],size[1]))),..Default::default()},|ui| {
                outcome=hud.draw(ui,ui.available_rect_before_wrap(),world,tick);
            });
            // This fixture captures the movie renderer directly, so no egui
            // renderer consumes its texture uploads. Explicitly discard them.
            output.textures_delta.clear();outcome
        }
        for tick in 1..=60 {frame(&ctx,&mut hud,&mut world,tick,[853.,480.])?;}
        assert_eq!({let mut player=lock(&hud.handle)?;read(&mut player,"_global.SHOW_AMMO")},Value::Bool(false));
        world.enable_demo_abilities()?;
        assert_eq!(world.ability_hud_requests,[("Show","HudElement","AmmoBars")]);
        let inventory=world.ammo.inventory.as_mut().context("Original ammo inventory absent")?;
        inventory.amounts=[15.,7.];
        for tick in 61..=90 {frame(&ctx,&mut hud,&mut world,tick,[853.,480.])?;}
        assert!(world.ability_hud_requests.is_empty());assert_eq!(hud.delivered_ability_messages,1);
        assert_eq!({let mut player=lock(&hud.handle)?;read(&mut player,"_global.SHOW_AMMO")},Value::Bool(true));
        assert_eq!(hud.values["Paint"],15.);assert_eq!(hud.values["Thinner"],7.);
        {
            let mut player=lock(&hud.handle)?;player.render();
            let backend=(player.renderer() as &dyn std::any::Any)
                .downcast_ref::<HudRenderer>().context("HUD GPU backend missing")?;
            backend.inner.capture_frame().context("HUD image missing")?.save("target/demo-ammo-hud-component.png")?;
        }
        let shown=hud.summary();
        for tick in 91..=120 {frame(&ctx,&mut hud,&mut world,tick,[640.,480.])?;}
        assert_eq!({let mut player=lock(&hud.handle)?;read(&mut player,"_global.SHOW_AMMO")},Value::Bool(true));
        assert_eq!(hud.delivered_ability_messages,1,"Resize duplicated an ability event");
        world.ability_hud_requests.push(("Hide","HudElement","AmmoBars"));
        frame(&ctx,&mut hud,&mut world,121,[640.,480.])?;
        assert_eq!({let mut player=lock(&hud.handle)?;read(&mut player,"_global.SHOW_AMMO")},Value::Bool(false));
        assert!(world.ability_hud_requests.is_empty());assert_eq!(hud.delivered_ability_messages,2);
        let mut benchmark=Vec::new();
        // Alternate short blocks to reduce startup/thermal ordering bias. Legacy
        // reproduces the removed repeated-resize call; both advance every tick.
        let mut tick=122;
        for repeat in 0..4 {
            for legacy in if repeat%2==0 {[true,false]}else{[false,true]} {
                let started=std::time::Instant::now();
                for _ in 0..15 {
                    if legacy {
                        let mut player=lock(&hud.handle)?;
                        player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions{width:853,height:480,scale_factor:1.});
                    }
                    frame(&ctx,&mut hud,&mut world,tick,[640.,480.])?;tick+=1;
                }
                benchmark.push(serde_json::json!({"legacy_repeated_resize":legacy,"frames":15,"elapsed_ms":started.elapsed().as_secs_f64()*1000.}));
            }
        }
        std::fs::write("target/demo-hud-resize-benchmark.json",serde_json::to_vec_pretty(&serde_json::json!({"blocks":benchmark,"timings":hud.timings,"scope":"Alternating original-movie HUD render/readback blocks; excludes world simulation and main GL rendering, no full-game FPS claim."}))?)?;
        std::fs::write("target/demo-ammo-hud-component.json",serde_json::to_vec_pretty(&serde_json::json!({"shown":shown,"hidden":hud.summary(),"scope":"Original movie and live World boundary, not a full game or native-render comparison"}))?)?;
        Ok(())
    }
}
#[cfg(feature="blotworx-ui-render")]
impl PlayerData {
    fn gameplay(world:&super::assets::World)->Result<Self> {
        let entity=&world.entities[world.player_entity];
        let properties=|class:&str|entity.components.iter().find(|c|c.class_name==class);
        let mut health=None;let mut inventory=None;
        if let Some(component)=properties("HealthComponent") {
            let mut values=std::collections::BTreeMap::new();
            for name in ["Lives","MaxHealth","Health"] {
                let value=component.properties.iter().find(|p|p.name==name).and_then(|p|p.value.as_i64()).with_context(||format!("HUD missing {name}"))?;
                values.insert(name.into(),i32::try_from(value)?);
            }
            if let Some(live)=world.runtime_health.get(&world.player_entity){values.insert("Health".into(),live.health);}
            health=Some(values);
        }
        if let Some(component)=properties("InventoryComponent") {
            let mut values=std::collections::BTreeMap::new();
            for name in ["PaintMax","Paint","ThinnerMax","Thinner","ETicket"] {
                let value=component.properties.iter().find(|p|p.name==name).and_then(|p|p.value.as_f64()).with_context(||format!("HUD missing {name}"))? as f32;
                values.insert(name.into(),value);
            }
            if let Some(live)=&world.ammo.inventory {
                for (i,name) in ["Paint","Thinner"].iter().enumerate(){values.insert((*name).into(),live.amounts[i]);}
                for (i,name) in ["PaintMax","ThinnerMax"].iter().enumerate(){if let Some(max)=live.maxima[i]{values.insert((*name).into(),max);}}
            }
            inventory=Some(values);
        }
        Ok(Self{health,inventory,level:world.report.level.clone(),entity:entity.name.clone()})
    }
}
#[cfg(feature="blotworx-ui-render")]
impl Gameplay {
    pub fn load(root:&Path)->Result<Self> {Self::load_with_reuse(root,true)}
    fn load_with_reuse(root:&Path,reuse:bool)->Result<Self> {Self::load_with_graphics(root,reuse,false)}
    fn load_with_graphics(root:&Path,reuse:bool,diagnostic:bool)->Result<Self> {
        let root=root.canonicalize()?;
        let mut resources=super::resource_store::Store::new(&root,&["HUDTextures.pak".into()])?;
        let file=resources.read("HUDElements/Flash/Cooked/mickeycontainer.gfx")?;
        let mut movie=crate::gfx::GfxFile::load(&file.data)?;
        let mut images=super::ui_images::Images::default();images.adapt(&mut movie,&file.path,&mut resources)?;
        let first=movie.tags.iter().position(|t|t.code==1).context("HUD movie has no frame")?;
        movie.tags.insert(first,crate::gfx::GfxTag{code:12,body:bridge(movie.version)?,long_length:true});
        let url=url::Url::from_file_path(root.join(&file.path)).map_err(|_|anyhow::anyhow!("Invalid HUD URL"))?;
        let movie_data=SwfMovieData::from_data(&movie.save()?,url.to_string(),None,None)?;
        let host=Host(Arc::new(Mutex::new(Observation::default())));
        host.0.lock().unwrap().effects=Some(Arc::new(Mutex::new(super::ui_effects::Runtime::load(&root)?)));
        let executor=NullExecutor::new();
        let parent=root.join(&file.path).parent().context("HUD parent missing")?.to_owned();
        let navigator=Files{inner:NullNavigatorBackend::with_base_path(&parent,&executor)?,root,host:host.clone(),
            images:Rc::new(RefCell::new(images)),resources:Rc::new(RefCell::new(resources))};
        let (renderer,graphics)=gpu::create(diagnostic)?;
        let mut renderer=render_cache::Cached::new(renderer);renderer.enabled=reuse;
        let handle=PlayerBuilder::new().with_movie(movie_data).with_autoplay(true).with_renderer(renderer)
            .with_log(host.clone()).with_external_interface(Box::new(host.clone())).with_fs_commands(Box::new(host.clone())).with_navigator(navigator).build();
        {
            let mut player=lock(&handle)?;
            player.set_background_color(Some(swf::Color{r:0,g:0,b:0,a:0}));
            // Ruffle recreates its target, surface and texture pool on every
            // set_viewport_dimensions call, even for identical dimensions.
            // This offscreen movie stays853x480; aspect changes go through Init.
            player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions{width:853,height:480,scale_factor:1.});
        }
        Ok(Self{handle,executor,host,initialized:false,last_tick:0,aspect:0.,visible:None,values:Default::default(),texture:None,errors:Default::default(),frames:0,delivered_ability_messages:0,timings:Default::default(),graphics})
    }
    fn render_frame(&mut self,input:&HudFrameInput,messages:&mut Vec<HudMessage>)->Result<Option<egui::ColorImage>> {
        self.render_frame_acknowledged(input,messages,|_|{})
    }
    fn render_frame_acknowledged(&mut self,input:&HudFrameInput,messages:&mut Vec<HudMessage>,acknowledge:impl FnOnce(usize))->Result<Option<egui::ColorImage>> {
        let update_started=std::time::Instant::now();
        let data=input.data.clone();let rect=input.rect;let tick=input.tick;
        let mut dirty=false;
        self.host.0.lock().unwrap().player=Some(data.clone());
        self.executor.run();
        let mut player=lock(&self.handle)?;
        let ready=self.host.0.lock().unwrap().callbacks.contains("__bwx_Init");
        let aspect=rect.width()/rect.height().max(1.);
        if ready && (!self.initialized || (aspect-self.aspect).abs()>0.001) {
            super::ui_calls::invoke("Init",&super::ui_calls::initialization_arguments(aspect)?,&mut NativeView{player:&mut player,host:&self.host})?;
            self.initialized=true;self.aspect=aspect;dirty=true;
        }
        if self.host.0.lock().unwrap().native_initialization.initialized()!=0 {
            let visible=input.visible;
            if self.visible!=Some(visible){
                super::ui_calls::invoke(if visible{"ShowHud"}else{"HideHud"},&[],&mut NativeView{player:&mut player,host:&self.host})?;self.visible=Some(visible);dirty=true;
            }
            // Blast ownership and camera disabling already produce the native
            // Show/Hide(HudElement,AmmoBars) messages. Drain only after the
            // original HUD has initialized; these are events, not per-frame
            // guesses derived from brush input or current ammo values.
            let delivered=super::ui_calls::drain_string_messages(messages,&mut NativeView{player:&mut player,host:&self.host})?;
            self.delivered_ability_messages+=delivered;dirty|=delivered>0;
            let values=data.health.iter().flat_map(|m|m.iter().map(|(k,v)|(k.clone(),*v as f64)))
                .chain(data.inventory.iter().flat_map(|m|m.iter().map(|(k,v)|(k.clone(),*v as f64))));
            for (name,value) in values {
                if self.values.get(&name)!=Some(&value) {
                    use super::ui_calls::Value as V;
                    super::ui_calls::invoke("Update",&[V::String("UPDATE"),V::String("Player01"),V::String(&name),V::Number(value)],&mut NativeView{player:&mut player,host:&self.host})?;
                    self.values.insert(name,value);dirty=true;
                }
            }
        }
        // Movie time follows gameplay ticks, never the OS/render frame rate.
        let delta=tick.saturating_sub(self.last_tick);self.last_tick=tick;
        if delta>0 || self.frames==0 {player.tick(FloatDuration::from_secs(delta as f64/30.));}
        drop(player);self.executor.run();
        self.timings.entry("movie_update").or_default().record(update_started.elapsed());
        // The VM has consumed its exact message prefix. Acknowledge before
        // GPU work so the scene can submit without waiting for pixel readback.
        acknowledge(input.messages.len()-messages.len());
        let mut rendered_image=None;
        if delta>0 || self.frames==0 || dirty || lock(&self.handle)?.needs_render() {
            let mut player=lock(&self.handle)?;
            let started=std::time::Instant::now();
            let before=(player.renderer() as &dyn std::any::Any).downcast_ref::<HudRenderer>().context("HUD renderer missing")?.submitted;
            player.render();
            self.timings.entry("render_submit").or_default().record(started.elapsed());
            let started=std::time::Instant::now();
            let backend=(player.renderer() as &dyn std::any::Any).downcast_ref::<HudRenderer>().context("HUD renderer missing")?;
            // The GPU target and egui both use premultiplied RGBA. Ruffle's
            // screenshot helper unpremultiplies every pixel, only for the old
            // egui upload path to multiply it again. Preserve the native bytes.
            // If all render inputs and resources are exactly unchanged, the
            // retained target is already the current image. Do not copy it from
            // GPU to CPU and upload it again. Movie execution still ran above.
            if backend.submitted!=before {
                let target=backend.inner.target();
                let info=target.buffer.as_ref().context("HUD frame unavailable")?;
                let (buffer,dimensions)=info.buffer.inner();
                let image=ruffle_render_wgpu::utils::capture_image(backend.inner.device(),buffer,dimensions,None,|rgba,stride| {
                    let pixels=rgba.chunks(stride as usize).take(target.size.height as usize)
                        .flat_map(|row|row[..target.size.width as usize*4].chunks_exact(4))
                        .map(|p|egui::Color32::from_rgba_premultiplied(p[0],p[1],p[2],p[3])).collect();
                    egui::ColorImage::new([target.size.width as usize,target.size.height as usize],pixels)
                });
                self.timings.entry("gpu_readback_and_premultiplied_copy").or_default().record(started.elapsed());
                rendered_image=Some(image);
            }
            self.frames+=1;
        }
        let mut seen=self.host.0.lock().unwrap();
        for error in seen.native_errors.drain(..){self.errors.insert(error);}
        for call in seen.unhandled.drain(..){
            // Coordinates/instance IDs vary per invocation; retain one useful
            // diagnostic per service instead of growing a log for every flash.
            self.errors.insert(format!("Unsupported HUD service: {} {}",call["kind"].as_str().unwrap_or("unknown"),call["name"].as_str().unwrap_or("unknown")));
        }
        seen.traces.clear();seen.warnings.clear();seen.native_updates.clear();seen.effect_events.clear();seen.synchronous_fs_commands.clear();
        Ok(rendered_image)
    }
    #[cfg(test)]
    pub fn draw(&mut self,ui:&egui::Ui,rect:egui::Rect,world:&mut super::assets::World,tick:u64)->Result<()> {
        let input=HudFrameInput::from_world(rect,world,tick)?;let mut messages=input.messages.clone();
        let result=self.render_frame(&input,&mut messages);
        commit_hud_messages(world,&input.messages,input.messages.len()-messages.len())?;
        if let Some(image)=result? {
            if let Some(texture)=&mut self.texture{texture.set(image,egui::TextureOptions::LINEAR)}else{
                self.texture=Some(ui.ctx().load_texture("original-gameplay-hud",image,egui::TextureOptions::LINEAR));}
        }
        if let Some(texture)=&self.texture{ui.painter().with_clip_rect(rect).image(texture.id(),rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}
        Ok(())
    }
    pub fn summary(&self)->serde_json::Value {
        let render_reuse=lock(&self.handle).ok().and_then(|player|(player.renderer() as &dyn std::any::Any).downcast_ref::<HudRenderer>().map(|r|serde_json::json!({"enabled":r.enabled,"submitted":r.submitted,"shape_requests":r.shape_requests,"shape_reused":r.shape_reused,"identical_frames":r.identical_frames})));
        let initialized=self.host.0.lock().unwrap().native_initialization.initialized();
        let display=match lock(&self.handle) {Ok(mut player)=>snapshot(&mut player),Err(error)=>serde_json::json!({"error":format!("{error:#}")})};
        serde_json::json!({"movie":"mickeycontainer.gfx","initialized":self.initialized,"native_initialized":initialized,
            "frames":self.frames,"last_tick":self.last_tick,"values":self.values,"delivered_ability_messages":self.delivered_ability_messages,
            "display":display,"graphics":self.graphics,"render_reuse":render_reuse,"timings":self.timings,"errors":self.errors,"gameplay_connected":true,"native_render_parity_validated":false})
    }
}
