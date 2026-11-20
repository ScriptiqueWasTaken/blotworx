#[cfg(feature="blotworx-ui-render")]
mod gpu {include!("ui_gpu.rs");}
#[cfg(feature="blotworx-ui-render")]
mod render_cache {include!("ui_render_cache.rs");}
#[cfg(feature="blotworx-ui-render")]
type HudRenderer=render_cache::Cached<ruffle_render_wgpu::backend::WgpuRenderBackend<ruffle_render_wgpu::target::TextureTarget>>;
#[cfg(feature="blotworx-ui-render")]
include!("ui_pause.rs");
// Included inside ui_movie so the gameplay owner uses the same movie adapter,
// asset loader and synchronous FSCommand services as the original-data probe.
#[cfg(feature="blotworx-ui-render")]
pub struct Gameplay {
    handle:Arc<Mutex<ruffle_core::Player>>,executor:NullExecutor,host:Host,
    root:PathBuf, resources:Rc<RefCell<super::resource_store::Store>>,
    images: Rc<RefCell<super::ui_images::Images>>,
    initialized:bool,last_tick:u64,aspect:f32,visible:Option<bool>,
    raster_height: u32,
    movie_time:Option<f64>,skip_duration:Option<f32>,
    pause_time:Option<f64>,
    pause_page: PausePage,
    pause_map_shown: bool,
    pause_options: PauseOptions,
    cursor: Option<([f32; 2], u8)>,
    ticket_hud_gain:Option<(u64,f32,f32)>,ticket_dropdown_updates:u64,ticket_hide_tick:Option<u64>,
    dialogue:Option<DialogueHud>,dialogue_updates:u64,dialogue_reveal_tick:Option<u64>,
    portrait_anchor: Option<[f64; 2]>,
    popups: std::collections::VecDeque<super::collectibles::Notification>,
    popup_active: bool,
    popup_icon: Option<url::Url>,
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
struct HudFrameInput {data:Arc<PlayerData>,visible:bool,tick:u64,rect:egui::Rect,pixels_per_point:f32,messages:Vec<HudMessage>,popups:Vec<super::collectibles::Notification>,popup_duration:f64,movie:Option<MovieHud>,pause_time:Option<f64>,pause_input:Vec<PauseInput>,pause_map:Option<super::map_info::State>,pause_lists:Option<Arc<PauseLists>>,pause_settings:[i32;3],pause_defaults:[i32;3],pause_collectibles:[f64;4],cursor:Option<[f32;2]>,ticket_timing:[f64;3],ticket_hud_gain:Option<(u64,f32,f32)>,dialogue:Option<DialogueHud>,portrait_packs:Vec<String>}
#[cfg(feature="blotworx-ui-render")]
#[derive(Clone, Copy)]
pub struct MovieHud {
    pub seconds: f64,
    pub skip_duration: Option<f32>,
}
#[cfg(feature="blotworx-ui-render")]
#[derive(Clone, PartialEq)]
struct DialogueHud {
    speaker: String,
    text: String,
    choices: Vec<String>,
    interactive: bool,
    portrait: Option<String>,
}
#[cfg(feature="blotworx-ui-render")]
fn publish_dialogue_text(view: &mut NativeView<'_>, dialogue: &DialogueHud) -> Result<()> {
    use super::ui_calls::Value as V;
    for (field, value) in [
        ("GENERIC_DIALOG_TITLE", dialogue.speaker.as_str()),
        ("GENERIC_DIALOG_TEXT", dialogue.text.as_str()),
        ("GENERIC_DIALOG_SHOWA", if dialogue.interactive { "true" } else { "false" }),
    ] {
        super::ui_calls::call("UpdateVariable", field, V::String(value), view)?;
    }
    // UpdateAButtonText and UpdateBButtonText turn on their choice flag even
    // for empty strings. Clear reused clips when a line has no such choice.
    for (index, (field, flag, clip)) in [
        ("DIALOG_ABUTTON_TEXT", "_root.GENERIC_DIALOG.showAButton", "_root.GENERIC_DIALOG.A._visible"),
        ("DIALOG_BBUTTON_TEXT", "_root.GENERIC_DIALOG.showBButton", "_root.GENERIC_DIALOG.B._visible"),
    ].into_iter().enumerate() {
        if let Some(label) = dialogue.choices.get(index)
            .filter(|label| dialogue.interactive && !label.trim().is_empty())
        {
            super::ui_calls::call("UpdateVariable", field, V::String(label), view)?;
        } else {
            write(view.player, flag, Value::Bool(false));
            write(view.player, clip, Value::Bool(false));
        }
    }
    if !dialogue.interactive {
        write(view.player, "_root.GENERIC_DIALOG.AButton._visible", Value::Bool(false));
    }
    Ok(())
}
#[cfg(feature="blotworx-ui-render")]
impl HudFrameInput {
    fn from_world(rect:egui::Rect,world:&super::assets::World,tick:u64)->Result<Self> {
        let dialogue = world.demo_story.presented_dialogue().filter(|(dialogue, _)| dialogue.choices.len() <= 2)
            .map(|(dialogue, interactive)| DialogueHud {
                speaker: dialogue.speaker.clone(),
                text: dialogue.text.clone(),
                choices: dialogue.choices.iter().map(|(label, _)| label.clone()).collect(),
                interactive,
                portrait: dialogue.portrait.clone(),
            });
        let portrait_packs = if dialogue.as_ref().is_some_and(|dialogue| dialogue.portrait.is_some())
            || !world.ui_notifications.is_empty() || !world.collectibles.notifications.is_empty()
        {
            world.report.packs.clone()
        } else { Vec::new() };
        let ticket_timing = [("TicketInitialDelay", 1.5), ("TicketUpdateDelay", 0.03), ("TicketHideDelay", 2.0)]
            .map(|(name, fallback)| world.runtime_settings.get(name)
                .and_then(|value| crate::blotworx::config_value::decimal_float(value).ok())
                .filter(|value| value.is_finite() && *value >= 0.)
                .map_or(fallback, f64::from));
        let visible = (!world.demo_story.enabled || world.demo_story.hud)
            && !world.screen_fade.hide_gameplay_hud;
        let popup_duration = world.runtime_settings.get("UI_Popup_DisplayTime")
            .and_then(|value| crate::blotworx::config_value::decimal_float(value).ok())
            .filter(|value| value.is_finite() && *value > 0.).map_or(4., f64::from);
        Ok(Self{data:Arc::new(PlayerData::gameplay(world)?),visible,
            tick,rect,pixels_per_point:1.,messages:world.ability_hud_requests.clone(),popups:Vec::new(),popup_duration,movie:None,pause_time:None,pause_input:Vec::new(),pause_map:None,pause_lists:None,pause_settings:[0,100,100],pause_defaults:[0,100,100],pause_collectibles:[0.;4],cursor:None,ticket_timing,ticket_hud_gain:world.ticket_hud_gain,dialogue,portrait_packs})
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

#[cfg(all(debug_assertions, feature="blotworx-ui-render"))]
pub(super) fn validate_pause_menu() -> Result<()> {
    use std::time::Duration;
    ensure!(matches!(PauseCommand::from_movie("OptionsMenuSFX", "*72"), Some(PauseCommand::SfxVolume(72))));
    ensure!(matches!(PauseCommand::from_movie("OptionsMenuMusic", "*38"), Some(PauseCommand::MusicVolume(38))));
    let root = PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?);
    let world = super::assets::load(&root, "Levels/EB_VillageTest.level", &egui::Context::default())?;
    let mut hud = Gameplay::load(&root)?;
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(853.,480.));
    for tick in 1..=20 {
        let input = HudFrameInput::from_world(rect, &world, tick)?;
        hud.render_frame(&input, &mut Vec::new())?;
    }
    let mut input = HudFrameInput::from_world(rect, &world, 20)?;
    {
        let mut player = lock(&hud.handle)?;
        write(&mut player, "_global.MAX_HEALTH", Value::Number(1.));
        write(&mut player, "_global.HEALTH", Value::Number(1.));
        pause_movie(&mut player, &hud.host, "Open", "Menu", "GenericDialog")?;
    }
    hud.render_frame(&input, &mut Vec::new())?;
    {
        let mut player = lock(&hud.handle)?;
        for (path, field) in [("_global.MAX_HEALTH", "MaxHealth"), ("_global.HEALTH", "Health")] {
            let value = f64::from(input.data.health.as_ref().context("Missing HUD health")?[field]);
            ensure!(matches!(read(&mut player, path), Value::Number(current) if current == value),
                "The movie's health did not recover from stale state");
        }
        ensure!(matches!(read(&mut player, "_root.GENERIC_DIALOG._visible"), Value::Bool(false)),
            "An empty native dialogue remained visible");
    }
    input.dialogue = Some(DialogueHud {
        speaker: "Gus".into(), text: "Check the next machine.".into(), choices: Vec::new(),
        interactive: true, portrait: None,
    });
    for tick in 21..=26 {
        input.tick = tick;
        hud.render_frame(&input, &mut Vec::new())?;
    }
    {
        let mut player = lock(&hud.handle)?;
        ensure!(matches!(read(&mut player, "_root.GENERIC_DIALOG._visible"), Value::Bool(true))
            && matches!(read(&mut player, "_root.GENERIC_DIALOG.dialogBox.text"),
                Value::String(text) if text.contains("Check the next machine.")),
            "The authored dialog movie did not reveal the current line");
        for path in ["_root.GENERIC_DIALOG.A._visible", "_root.GENERIC_DIALOG.B._visible"] {
            ensure!(matches!(read(&mut player, path), Value::Bool(false)),
                "A linear dialogue showed a choice button at {path}");
        }
    }
    input.dialogue = None;
    input.visible = false;
    let mut last_frame = None;
    {
        let mut player = lock(&hud.handle)?;
        pause_movie(&mut player, &hud.host, "Open", "Menu", "Black")?;
        ensure!(matches!(read(&mut player, "_root.BLACK._visible"), Value::Bool(true)),
            "The authored fullscreen black clip did not open");
    }
    for (movie, expected) in [
        (Some(MovieHud { seconds: 0., skip_duration: Some(3.) }), true),
        (Some(MovieHud { seconds: 0.1, skip_duration: None }), false),
        (Some(MovieHud { seconds: 0.2, skip_duration: Some(3.) }), true),
        (None, false),
    ] {
        input.movie = movie;
        if let Some(frame) = hud.render_frame(&input, &mut Vec::new())? {
            last_frame = Some(frame);
        }
        let mut player = lock(&hud.handle)?;
        let visible = read(&mut player, "_root.SKIP_MOVIE._visible");
        ensure!(matches!(visible, Value::Bool(value) if value == expected),
            "Movie skip clip visibility {visible:?}, expected {expected}");
        ensure!(matches!(read(&mut player, "_root.BLACK._visible"), Value::Bool(false)),
            "The original HUD composited a second fullscreen black layer");
        if movie.is_none() {
            let frame = last_frame.as_ref().context("HUD texture did not render")?;
            let [width, height] = frame.size;
            ensure!(frame.pixels[(height / 2) * width + width / 2].a() == 0,
                "The former movie HUD still covers the center of the gameplay viewport");
        }
    }
    input.visible = true;
    input.cursor = Some([0.5, 0.63]);
    input.pause_map = Some(super::map_info::State { map: "MAP_DBC_ZONEB".into(), map_type: "MAP".into() });
    input.pause_lists = Some(Arc::new(PauseLists::from_world(&world)));
    let mut resume = false;
    for tick in 0..=90 {
        input.pause_time = Some(f64::from(tick) / 30.);
        input.pause_input = match tick {
            18 => vec![PauseInput::Key(PauseKey::PreviousPage)],
            36 => vec![PauseInput::Key(PauseKey::NextPage)],
            54 | 55 => vec![PauseInput::Pointer { position: egui::pos2(426.5, 302.4), pressed: Some(tick == 54) }],
            72 | 90 => vec![PauseInput::Key(PauseKey::Back)],
            _ => Vec::new(),
        };
        std::thread::sleep(Duration::from_millis(12));
        if let Some(frame) = hud.render_frame_acknowledged(&input, &mut Vec::new(), |_, _, outcome| resume |= outcome.resume)? {
            last_frame = Some(frame);
        }
        if tick == 17 || tick == 35 || tick == 53 {
            let (page, path, file) = if tick == 35 {
                (PausePage::Map, "_root.PAUSE_MENU_MAP._url", "PauseMenu_Map.gfx")
            } else {
                (PausePage::Main, "_root.PAUSE_MENU_MAIN._url", "PauseMenu_MainPage.gfx")
            };
            let mut player = lock(&hud.handle)?;
            let value = read(&mut player, path);
            ensure!(hud.pause_page == page && matches!(value, Value::String(ref url) if url.ends_with(file)),
                "Original pause page failed to load: {value:?}");
            if page == PausePage::Map {
                let title = read(&mut player, "_root.PAUSE_MENU_MAP.PauseMC.Map.MapName.text");
                ensure!(matches!(title, Value::String(text) if !text.is_empty() && !text.contains("MAP_DBC_ZONEB")),
                    "The map title did not localize");
                ensure!(hud.images.borrow().observations.iter().any(|record| record["map"] == "MAP_DBC_ZONEB" && record["texture"].is_string()),
                    "The map did not bind the authored DBC artwork");
                if let Some(path) = std::env::var_os("BLOTWORX_MAP_CAPTURE") {
                    let frame = last_frame.as_ref().context("Map screenshot did not render")?;
                    let pixels = frame.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect::<Vec<_>>();
                    image::save_buffer(path, &pixels, frame.size[0] as u32, frame.size[1] as u32, image::ColorType::Rgba8)?;
                }
            }
            let cursor = read(&mut player, "_root.RETICLES.Reticle_Menu._visible");
            ensure!(matches!(cursor, Value::Bool(true) | Value::Number(1.)), "Original menu cursor is hidden: {cursor:?}");
        }
        if tick == 71 {
            let mut player = lock(&hud.handle)?;
            let url = read(&mut player, "_root.OPTIONS._url");
            ensure!(hud.pause_options == PauseOptions::Open && matches!(url, Value::String(ref path) if path.ends_with("Options.gfx")),
                "Original Options screen failed to open: {url:?}");
            ensure!(matches!(read(&mut player, "_root.PAUSE_MENU_MAIN._visible"), Value::Bool(false)),
                "The main pause page draws underneath the Options submenu");
            if let Some(path) = std::env::var_os("BLOTWORX_OPTIONS_CAPTURE") {
                let frame = last_frame.as_ref().context("Options screenshot did not render")?;
                let pixels = frame.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect::<Vec<_>>();
                image::save_buffer(path, &pixels, frame.size[0] as u32, frame.size[1] as u32, image::ColorType::Rgba8)?;
            }
        }
        if tick == 89 {
            ensure!(!hud.pause_options.active() && !resume, "Cancel should return from Options to the pause main page");
            let mut player = lock(&hud.handle)?;
            ensure!(matches!(read(&mut player, "_root.PAUSE_MENU_MAIN._visible"), Value::Bool(true)),
                "Returning from Options left the main pause page hidden");
        }
    }
    ensure!(resume, "The original pause menu did not emit Resume");
    input.pause_time = None;
    input.pause_input.clear();
    hud.render_frame(&input, &mut Vec::new())?;
    let mut player = lock(&hud.handle)?;
    for path in ["_root.OPTIONS._visible", "_root.RETICLES.Reticle_Menu._visible"] {
        ensure!(!matches!(read(&mut player, path), Value::Bool(true)), "{path} remained visible after Resume");
    }
    println!("Original skip prompt hid on release and movie exit. Original pause menu opened Options with the menu cursor, returned to Main, and emitted Resume");
    Ok(())
}

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
        let host=Host(Arc::new(Mutex::new(Observation { ticket_dings: Some(Vec::new()), pause_commands: Some(Vec::new()), ..Default::default() })));
        host.0.lock().unwrap().effects=Some(Arc::new(Mutex::new(super::ui_effects::Runtime::load(&root)?)));
        let executor=NullExecutor::new();
        let parent=root.join(&file.path).parent().context("HUD parent missing")?.to_owned();
        let resources=Rc::new(RefCell::new(resources));
        let images = Rc::new(RefCell::new(images));
        let navigator=Files{inner:NullNavigatorBackend::with_base_path(&parent,&executor)?,root:root.clone(),host:host.clone(),
            images:images.clone(),resources:resources.clone()};
        let (renderer,graphics)=gpu::create(diagnostic)?;
        let mut renderer=render_cache::Cached::new(renderer);renderer.enabled=reuse;
        let handle=PlayerBuilder::new().with_movie(movie_data).with_autoplay(true).with_renderer(renderer)
            .with_log(host.clone()).with_external_interface(Box::new(host.clone())).with_fs_commands(Box::new(host.clone())).with_navigator(navigator).build();
        {
            let mut player=lock(&handle)?;
            player.set_background_color(Some(swf::Color{r:0,g:0,b:0,a:0}));
            // Ruffle recreates its target, surface and texture pool on every
            // set_viewport_dimensions call, even for identical dimensions.
            // Start at the movie's reference raster until the first frame arrives.
            player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions{width:853,height:480,scale_factor:1.});
        }
        Ok(Self{handle,executor,host,root,resources,images,initialized:false,last_tick:0,aspect:0.,visible:None,raster_height:480,movie_time:None,skip_duration:None,pause_time:None,pause_page:PausePage::Main,pause_map_shown:false,pause_options:PauseOptions::Closed,cursor:None,ticket_hud_gain:None,ticket_dropdown_updates:0,ticket_hide_tick:None,dialogue:None,dialogue_updates:0,dialogue_reveal_tick:None,portrait_anchor:None,popups:Default::default(),popup_active:false,popup_icon:None,values:Default::default(),texture:None,errors:Default::default(),frames:0,delivered_ability_messages:0,timings:Default::default(),graphics})
    }
    fn scroll_dialogue(&mut self) -> Result<bool> {
        let mut player = lock(&self.handle)?;
        let can_scroll = player.call_internal_interface(&format!("__bwx_{CAN_SCROLL_DIALOGUE}"), []);
        if matches!(can_scroll, Value::Bool(true) | Value::Number(1.)) {
            player.call_internal_interface(&format!("__bwx_{SCROLL_DIALOGUE}"), []);
            return Ok(true);
        }
        Ok(false)
    }
    fn render_frame(&mut self,input:&HudFrameInput,messages:&mut Vec<HudMessage>)->Result<Option<egui::ColorImage>> {
        self.render_frame_acknowledged(input,messages,|_,_,_|{})
    }
    fn take_ticket_dings(&self) -> Vec<(i32, i32)> {
        self.host.0.lock().unwrap().ticket_dings.as_mut().map(std::mem::take).unwrap_or_default()
    }
    fn render_frame_acknowledged(&mut self,input:&HudFrameInput,messages:&mut Vec<HudMessage>,acknowledge:impl FnOnce(usize,Vec<(i32,i32)>,PauseOutcome))->Result<Option<egui::ColorImage>> {
        let update_started=std::time::Instant::now();
        let data=input.data.clone();let rect=input.rect;let tick=input.tick;
        let mut dirty=false;
        if !input.popups.is_empty() {
            self.resources.borrow_mut().admit_packs(&input.portrait_packs)?;
        }
        self.popups.extend(input.popups.iter().cloned());
        self.host.0.lock().unwrap().player=Some(data.clone());
        self.executor.run();
        let mut player=lock(&self.handle)?;
        let height = (rect.height() * input.pixels_per_point).round().clamp(480., 1080.) as u32;
        let width = (height * 853 + 240) / 480;
        let resized = self.raster_height != height;
        if resized {
            player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions {
                width, height, scale_factor: f64::from(height) / 480.,
            });
            self.raster_height = height;
            dirty = true;
        }
        let ready=self.host.0.lock().unwrap().callbacks.contains("__bwx_Init");
        let aspect=rect.width()/rect.height().max(1.);
        if ready && (!self.initialized || (aspect-self.aspect).abs()>0.001) {
            super::ui_calls::invoke("Init",&super::ui_calls::initialization_arguments(aspect)?,&mut NativeView{player:&mut player,host:&self.host})?;
            self.initialized=true;self.aspect=aspect;dirty=true;
            self.values.clear();
            if self.dialogue.is_some() { self.dialogue_reveal_tick = Some(tick + 3); }
        }
        let initialized = self.host.0.lock().unwrap().native_initialization.initialized() != 0;
        let pause_time = input.pause_time.filter(|_| initialized);
        let pause_changed = pause_time.is_some() != self.pause_time.is_some();
        if initialized {
            let visible=input.visible && input.movie.is_none() && pause_time.is_none();
            if std::mem::take(&mut self.host.0.lock().unwrap().popup_done) {
                self.popup_active = false;
                self.popup_icon = None;
                dirty = true;
            }
            let popup_visible = visible && input.dialogue.is_none();
            if popup_visible && self.popup_icon.is_some()
                && matches!(read(&mut player, "_root.POPUP.Popup.icon._x"), Value::Number(_))
            {
                let url = self.popup_icon.take().unwrap();
                player.call_internal_interface(&format!("__bwx_{LOAD_POPUP_ICON}"), [Value::String(url.to_string())]);
                dirty = true;
            }
            if self.popup_active
                && matches!(read(&mut player, "_root.POPUP._visible"), Value::Bool(value) if value != popup_visible)
            {
                write(&mut player, "_root.POPUP._visible", Value::Bool(popup_visible));
                dirty = true;
            }
            if popup_visible && !self.popup_active && !self.popups.is_empty()
                && matches!(read(&mut player, "_root.POPUP.Popup._totalframes"), Value::Number(frames) if frames > 1.)
            {
                let popup = self.popups.front().unwrap();
                let images = self.images.borrow();
                let name = images.pause_label(&popup.title_key).unwrap_or(&popup.title_key);
                let event_label = images.pause_label(popup.event);
                let (title, description) = event_label.map_or_else(|| {
                    let description = popup.description_key.as_deref()
                        .and_then(|key| images.pause_label(key)).unwrap_or("");
                    (name, description)
                }, |label| (label, name));
                let icon = if popup.icon.is_empty() { "quest_generic" } else { &popup.icon };
                // The PopUp_In keyframe instantiates the authored icon slot.
                // Apply the image on the following movie update.
                self.popup_icon = hud_icon_url(&self.root, icon, POPUP_ICON_SUFFIX);
                use super::ui_calls::Value as V;
                let mut view = NativeView { player: &mut player, host: &self.host };
                super::ui_calls::call("SetPopupText", title, V::String(description), &mut view)?;
                super::ui_calls::call("DisplayPopup", "Show", V::Number(input.popup_duration), &mut view)?;
                self.popups.pop_front();
                self.popup_active = true;
                dirty = true;
            }
            let cursor = input.cursor.filter(|_| pause_time.is_some() || (visible && input.dialogue.is_none()))
                .map(|pointer| (pointer, if pause_time.is_some() { 7 } else { 4 }));
            if resized || cursor != self.cursor || self.visible.is_none() {
                use super::ui_calls::Value as V;
                let mut view = NativeView{player:&mut player,host:&self.host};
                if let Some(([x, y], kind)) = cursor {
                    if kind == 7 && pause_changed {
                        super::ui_calls::invoke("ShowReticles", &[], &mut view)?;
                    }
                    view.player.handle_event(ruffle_core::events::PlayerEvent::MouseMove {
                        x: f64::from(x) * f64::from(width), y: f64::from(y) * f64::from(height),
                    });
                    write(view.player, "_global.P1_CURSOR_DATA", Value::List(vec![Value::Number(f64::from(kind))]));
                }
                super::ui_calls::call("Cursor", "FastUpdate", V::BooleanByte(u8::from(cursor.is_some())),
                    &mut view)?;
                self.cursor = cursor;
                dirty = true;
            }
            if pause_changed {
                if pause_time.is_some() {
                    self.pause_page = PausePage::Main;
                    self.images.borrow_mut().pause_map = input.pause_map.clone();
                    begin_pause(&mut player, &self.host, input)?;
                } else {
                    if self.pause_options.active() { close_pause_options(&mut player, &self.host, &mut self.pause_options)?; }
                    pause_movie(&mut player, &self.host, "Close", "Pause", self.pause_page.name())?;
                    write(&mut player, "_global.IsGamePaused", Value::Bool(false));
                    pause_movie(&mut player, &self.host, "PauseDropdownCounter", "Pause", "false")?;
                }
                dirty = true;
            }
            if pause_time.is_some() {
                for event in &input.pause_input {
                    if self.pause_options.active() && matches!(event, PauseInput::Key(PauseKey::Back))
                        && !matches!(read(&mut player, "_root.OPTIONS.m_TVMenuUp"), Value::Bool(true) | Value::Number(1.))
                    {
                        close_pause_options(&mut player, &self.host, &mut self.pause_options)?;
                        dirty = true;
                    } else {
                        event.forward(&mut player, rect);
                    }
                }
            }
            let duration=input.movie.and_then(|movie|movie.skip_duration);
            if duration != self.skip_duration
                && (duration.is_none() || matches!(
                    read(&mut player, "_root.SKIP_MOVIE.skipMovieMC._totalframes"),
                    Value::Number(count) if count >= 2.
                ))
            {
                use super::ui_calls::Value as V;
                let (command, value) = duration.map_or(("Hide", 0.), |seconds| ("showButton", f64::from(seconds)));
                super::ui_calls::call("SkipMovieUpdate", command, V::Number(value),
                    &mut NativeView{player:&mut player,host:&self.host})?;
                self.skip_duration=duration;
                dirty=true;
            }
            if self.visible!=Some(visible){
                super::ui_calls::invoke(if visible{"ShowHud"}else{"HideHud"},&[],&mut NativeView{player:&mut player,host:&self.host})?;self.visible=Some(visible);dirty=true;
            }
            // Blast ownership and camera disabling already produce the native
            // Show/Hide(HudElement,AmmoBars) messages. Drain only after the
            // original HUD has initialized; these are events, not per-frame
            // guesses derived from brush input or current ammo values.
            let delivered=super::ui_calls::drain_string_messages(messages,&mut NativeView{player:&mut player,host:&self.host})?;
            self.delivered_ability_messages+=delivered;dirty|=delivered>0;
            let values=data.health.iter().flat_map(|health| ["MaxHealth", "Health", "Lives"].into_iter()
                .filter_map(move |name| health.get(name).map(|value| (name, f64::from(*value)))))
                .chain(data.inventory.iter().flat_map(|inventory|inventory.iter().map(|(name,value)|(name.as_str(),f64::from(*value)))));
            for (name,value) in values {
                let native = match name {
                    "MaxHealth" => Some("_global.MAX_HEALTH"),
                    "Health" => Some("_global.HEALTH"),
                    _ => None,
                };
                let stale = native.is_some_and(|path| match read(&mut player, path) {
                    Value::Number(current) => current != value,
                    Value::String(current) => current.parse::<f64>().ok() != Some(value),
                    _ => true,
                });
                if self.values.get(name)!=Some(&value) || stale {
                    use super::ui_calls::Value as V;
                    super::ui_calls::invoke("Update",&[V::String("UPDATE"),V::String("Player01"),V::String(name),V::Number(value)],&mut NativeView{player:&mut player,host:&self.host})?;
                    self.values.insert(name.to_owned(),value);dirty=true;
                }
            }
            let dialogue = input.dialogue.as_ref().filter(|_| input.movie.is_none());
            if dialogue != self.dialogue.as_ref() {
                use super::ui_calls::Value as V;
                let mut view = NativeView{player:&mut player,host:&self.host};
                if self.dialogue.is_some() && dialogue.is_none() {
                    super::ui_calls::call("Close", "Menu", V::String("GenericDialog"), &mut view)?;
                }
                if let Some(dialogue) = dialogue {
                    // Keep the panel open during a page replacement so its
                    // cleanup cannot erase the newly submitted text.
                    if self.dialogue.is_none() {
                        super::ui_calls::call("Open", "Menu", V::String("GenericDialog"), &mut view)?;
                    }
                    let mut portrait_visible = dialogue.portrait.is_some();
                    if let Some(portrait) = &dialogue.portrait {
                        if self.dialogue.as_ref().and_then(|previous| previous.portrait.as_ref()) != Some(portrait) {
                            let loaded = (|| -> Result<()> {
                                let path = super::resource_path::canonical(portrait)?;
                                self.resources.borrow_mut().admit_packs(&input.portrait_packs)?;
                                let asset = self.root.join(format!("{path}{PORTRAIT_SUFFIX}"));
                                let url = url::Url::from_file_path(asset)
                                    .map_err(|_| anyhow::anyhow!("Invalid dialogue portrait URL"))?;
                                if self.portrait_anchor.is_none() {
                                    let x=read(view.player,"_root.GENERIC_DIALOG.Portrait._x");
                                    let y=read(view.player,"_root.GENERIC_DIALOG.Portrait._y");
                                    if let (Value::Number(x), Value::Number(y)) = (x,y) { self.portrait_anchor=Some([x,y]); }
                                }
                                if let Some([x,y])=self.portrait_anchor {
                                    // The cooked 64px slot has centered registration,
                                    // whereas a loaded bitmap starts at its top left.
                                    write(view.player,"_root.GENERIC_DIALOG.Portrait._x",Value::Number(x-32.));
                                    write(view.player,"_root.GENERIC_DIALOG.Portrait._y",Value::Number(y-32.));
                                }
                                view.player.call_internal_interface(&format!("__bwx_{LOAD_DIALOGUE_PORTRAIT}"), [Value::String(url.to_string())]);
                                Ok(())
                            })();
                            if let Err(error) = loaded {
                                self.errors.insert(format!("Dialogue portrait {portrait}: {error:#}"));
                                portrait_visible = false;
                            }
                        }
                    }
                    super::ui_calls::call("UpdateVariable", "GENERIC_DIALOG_ICON",
                        V::String(if portrait_visible { "true" } else { "false" }), &mut view)?;
                    publish_dialogue_text(&mut view, dialogue)?;
                    self.dialogue_updates += 1;
                }
                self.dialogue = dialogue.cloned();
                // REL1e71f8 and 1e677c: the host sets three updates, then
                // calls the movie's reveal command when that count hits zero.
                self.dialogue_reveal_tick = dialogue.map(|_| tick + 3);
                dirty = true;
            }
            if let Some(dialogue) = &self.dialogue {
                let text = read(&mut player, "_root.GENERIC_DIALOG.dialogBox.text");
                let reveal = self.dialogue_reveal_tick.is_some_and(|reveal_tick| tick >= reveal_tick);
                let missing_text = self.dialogue_reveal_tick.is_none() && !dialogue.text.trim().is_empty()
                    && matches!(&text, Value::String(value) if value.trim().is_empty());
                if (reveal || missing_text) && matches!(text, Value::String(_)) {
                    let mut view = NativeView{player:&mut player,host:&self.host};
                    publish_dialogue_text(&mut view, dialogue)?;
                    super::ui_calls::call("GenericDialogCommand", "UPDATE", super::ui_calls::Value::String(""), &mut view)?;
                    self.dialogue_reveal_tick = None;
                    dirty = true;
                }
            }
            let ticket_closed = std::mem::take(&mut self.host.0.lock().unwrap().ticket_closed);
            if ticket_closed {
                self.ticket_hide_tick = None;
                super::ui_calls::call("DropdownCounterCommand", "Close", super::ui_calls::Value::String(""),
                    &mut NativeView{player:&mut player,host:&self.host})?;
                dirty = true;
            }
            if input.movie.is_none() && input.ticket_hud_gain != self.ticket_hud_gain {
                if let Some((serial, previous, next)) = input.ticket_hud_gain {
                    use super::ui_calls::Value as V;
                    let previous = self.ticket_hud_gain
                        .filter(|&(last, _, total)| last.wrapping_add(1) != serial && total <= previous)
                        .map_or(previous, |(_, _, total)| total);
                    for (path, seconds) in [
                        "_root.TICKETS.g_TMDelayBeforeUpdate",
                        "_root.TICKETS.g_TMTicketChangeSpeed",
                        "_root.TICKETS.g_TMDelayAfterUpdate",
                    ].into_iter().zip(input.ticket_timing) {
                        // The movie multiplies elapsed seconds by tickets per
                        // second, while the game setting is seconds per ticket.
                        let value = if path.ends_with("g_TMTicketChangeSpeed") {
                            if seconds > 0. { seconds.recip() } else { f64::from(next - previous).abs().max(1.) * 30. }
                        } else {
                            seconds
                        };
                        write(&mut player, path, Value::Number(value));
                    }
                    let mut view = NativeView{player:&mut player,host:&self.host};
                    for (command, value) in [
                        ("Open", V::String("")),
                        ("ShowTicketIcon", V::String("")),
                        ("SetInitialValue", V::Number(f64::from(previous))),
                        ("SetFinalTicketValue", V::Number(f64::from(next))),
                        ("Show", V::BooleanByte(0)),
                    ] {
                        super::ui_calls::call("DropdownCounterCommand", command, value, &mut view)?;
                    }
                    self.ticket_dropdown_updates+=1;
                    let [before, per_ticket, after] = input.ticket_timing;
                    let seconds = before + f64::from((next - previous).abs()) * per_ticket + after;
                    self.ticket_hide_tick = Some(tick.saturating_add((seconds * 30.).ceil() as u64));
                    dirty=true;
                }
                self.ticket_hud_gain=input.ticket_hud_gain;
            }
            if input.movie.is_none() && pause_time.is_none() && self.ticket_hide_tick.is_some_and(|deadline| tick >= deadline) {
                super::ui_calls::call("DropdownCounterCommand", "Hide", super::ui_calls::Value::String(""),
                    &mut NativeView{player:&mut player,host:&self.host})?;
                self.ticket_hide_tick = None;
                dirty = true;
            }
        }
        // Movie time follows gameplay ticks, never the OS/render frame rate.
        let elapsed=input.movie.map(|movie|movie.seconds);
        let delta=if let Some(seconds)=pause_time {
            (seconds-self.pause_time.unwrap_or(seconds)).max(0.)
        } else if let Some(seconds)=elapsed {
            (seconds-self.movie_time.unwrap_or(seconds)).max(0.)
        } else {
            tick.saturating_sub(self.last_tick) as f64/30.
        };
        self.movie_time=elapsed;
        self.pause_time=pause_time;
        self.last_tick=tick;
        if delta>0. || self.frames==0 {player.tick(FloatDuration::from_secs(delta));}
        if initialized {
            if let Some(health) = &data.health {
                for (name, path) in [("MaxHealth", "_global.MAX_HEALTH"), ("Health", "_global.HEALTH")] {
                    if let Some(&value) = health.get(name) {
                        let value = f64::from(value);
                        let stale = match read(&mut player, path) {
                            Value::Number(current) => current != value,
                            Value::String(current) => current.parse::<f64>().ok() != Some(value),
                            _ => true,
                        };
                        if stale {
                            use super::ui_calls::Value as V;
                            super::ui_calls::invoke("Update", &[V::String("UPDATE"), V::String("Player01"), V::String(name), V::Number(value)],
                                &mut NativeView { player: &mut player, host: &self.host })?;
                            dirty = true;
                        }
                    }
                }
            }
        }
        if self.ticket_hud_gain.is_some() {
            // The cooked movie prefixes its numeric font control marker.
            // AVM1 renders the marker literally when using the host text engine.
            let path = "_root.TICKETS.g_TMDropdownMC.NumberMC.Numbers.text";
            if let Value::String(value) = read(&mut player, path) {
                if let Some(number) = value.strip_prefix('*') {
                    write(&mut player, path, Value::String(number.to_owned()));
                    dirty = true;
                }
            }
        }
        let commands = self.host.0.lock().unwrap().pause_commands.as_mut().map(std::mem::take).unwrap_or_default();
        let mut pause_outcome = apply_pause_commands(&mut player, &self.host, &mut self.pause_page,
            &mut self.pause_options, input, pause_time.is_some(), commands, &self.images.borrow(), &self.root)?;
        if pause_changed {
            pause_outcome.sounds.push(if pause_time.is_some() {
                "Play_SFX_UI_PauseMenu_Pause"
            } else { "Play_SFX_UI_PauseMenu_Resume" }.into());
        }
        if pause_time.is_some() && self.pause_page == PausePage::Map {
            if !self.pause_map_shown && matches!(read(&mut player, "_root.PAUSE_MENU_MAP.PauseMC.Map.MapMC._totalframes"), Value::Number(frames) if frames > 0.) {
                pause_movie(&mut player, &self.host, "PauseMenuCommand", "MAP", "SHOW")?;
                pause_movie(&mut player, &self.host, "PauseMenuCommand", "MAP", "UPDATE")?;
                self.pause_map_shown = true;
                dirty = true;
            }
        } else {
            self.pause_map_shown = false;
        }
        if pause_time.is_some() {
            dirty |= format_pause_fields(&mut player, self.pause_page, self.pause_options.active(), &self.images.borrow());
        }
        drop(player);self.executor.run();
        if initialized {
            let mut player = lock(&self.handle)?;
            // The authored Hide callback invokes the child animation but sets
            // its parent visible first. The engine owns the fullscreen fade.
            for (path, visible) in [
                ("_root.SKIP_MOVIE._visible", self.skip_duration.is_some()),
                ("_root.SKIP_MOVIE.skipMovieMC._visible", self.skip_duration.is_some()),
                ("_root.SKIP_MOVIE.skipMovieMC.timerMC._visible", false),
                ("_root.BLACK._visible", false),
                ("_root.GENERIC_DIALOG._visible", self.dialogue.is_some() && self.dialogue_reveal_tick.is_none()),
            ] {
                if matches!(read(&mut player, path), Value::Bool(current) if current != visible) {
                    write(&mut player, path, Value::Bool(visible));
                    dirty = true;
                }
            }
            // A load can finish after the native Close command was sent.
            for (inactive, path) in [
                (!self.pause_options.active(), "_root.OPTIONS._visible"),
                (!self.pause_options.active(), "_root.TV_SETTINGS._visible"),
                (pause_time.is_none(), "_root.RETICLES.Reticle_Menu._visible"),
            ] {
                if inactive && matches!(read(&mut player, path), Value::Bool(true)) {
                    write(&mut player, path, Value::Bool(false));
                    dirty = true;
                }
            }
        }
        self.timings.entry("movie_update").or_default().record(update_started.elapsed());
        // The VM has consumed its exact message prefix. Acknowledge before
        // GPU work so the scene can submit without waiting for pixel readback.
        acknowledge(input.messages.len()-messages.len(), self.take_ticket_dings(), pause_outcome);
        let mut rendered_image=None;
        if delta>0. || self.frames==0 || dirty || lock(&self.handle)?.needs_render() {
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
            "frames":self.frames,"last_tick":self.last_tick,"values":self.values,"skip_duration":self.skip_duration,"pause_time":self.pause_time,"ticket_dropdown_updates":self.ticket_dropdown_updates,"dialogue_updates":self.dialogue_updates,"delivered_ability_messages":self.delivered_ability_messages,
            "popup_active":self.popup_active,"pending_popups":self.popups.len(),"display":display,"graphics":self.graphics,"raster_height":self.raster_height,"render_reuse":render_reuse,"timings":self.timings,"errors":self.errors,"gameplay_connected":true,"native_render_parity_validated":false})
    }
}
