use super::*;
use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};

type Renderer = WgpuRenderBackend<TextureTarget>;

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<(String, String)>>>);

impl FsCommandProvider for Events {
    fn on_fs_command(&self, command: &str, args: &str) -> bool {
        match command {
            "PlayStartScreenMovie"
            | "StartScreenEnd"
            | "PlayMovie"
            | "NextMovie"
            | "MainMenuPick"
            | "MainMenuLoad"
            | "MainMenuAction"
            | "SaveStartupCheck"
            | "MainMenuDelete"
            | "ExtrasMenuLoad"
            | "Extras"
            | "ExtrasSub" => {
                self.0
                    .lock()
                    .unwrap()
                    .push((command.to_owned(), args.to_owned()));
                true
            }
            _ => false,
        }
    }
}

fn localize_frontend_labels(
    movie: &mut crate::gfx::GfxFile,
    resources: &mut super::super::resource_store::Store,
) -> Result<[Option<String>; 3]> {
    use epic_mickey_lib_rs::dct::{epic_mickey_hash, DCT};
    resources.admit_packs(&["English_NTSC.pak".into()])?;
    let file = resources.read("Localize/ENGLISH_NTSC_RVL.dct")?;
    let locale = DCT::from_binary(file.data);
    for tag in movie.tags.iter_mut().filter(|tag| tag.code == 37) {
        let data = swf::read::Reader::new(&tag.body, movie.version).read_define_edit_text()?;
        let Some(initial) = data.initial_text() else {
            continue;
        };
        let initial = std::str::from_utf8(initial.as_bytes())?;
        let Some(begin) = initial.find("UI_") else {
            continue;
        };
        let end = begin
            + initial[begin..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
        let key = &initial[begin..end];
        let hash = epic_mickey_hash(key.as_bytes(), locale.hash_seed);
        if let Some(entry) = locale
            .dialog_entries
            .iter()
            .find(|entry| entry.hashed_key == hash)
        {
            let label = entry
                .text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let (before, after) = (&initial[..begin], &initial[end..]);
            let replacement = format!("{before}{label}{after}");
            let start = tag.body.len() - initial.len() - 1;
            tag.body.truncate(start);
            tag.body
                .extend(replacement.bytes().filter(|&byte| byte != 0));
            tag.body.push(0);
        }
    }
    Ok(["UI_StartGame", "UI_Extras", "UI_NewGame"].map(|key| {
        let hash = epic_mickey_hash(key.as_bytes(), locale.hash_seed);
        locale
            .dialog_entries
            .iter()
            .find(|entry| entry.hashed_key == hash)
            .map(|entry| entry.text.clone())
    }))
}

struct Stage {
    handle: Arc<Mutex<ruffle_core::Player>>,
    host: Host,
    events: Events,
    executor: NullExecutor,
    texture: Option<egui::TextureHandle>,
    labels: [Option<String>; 3],
}

impl Stage {
    fn load(root: &Path, path_name: &str, methods: &[&str]) -> Result<Self> {
        let mut resources = super::super::resource_store::Store::new(
            root,
            &["MainMenuTextures.pak".into(), "HUDTextures.pak".into()],
        )?;
        let file = resources.read(path_name)?;
        let mut movie = crate::gfx::GfxFile::load(&file.data)?;
        let labels = localize_frontend_labels(&mut movie, &mut resources)?;
        let mut images = super::super::ui_images::Images::default();
        images.adapt(&mut movie, &file.path, &mut resources)?;
        let first = movie
            .tags
            .iter()
            .position(|tag| tag.code == 1)
            .context("Title movie has no display frame")?;
        movie.tags.insert(
            first,
            crate::gfx::GfxTag {
                code: 12,
                body: frontend_bridge(movie.version, methods)?,
                long_length: true,
            },
        );
        let path = root.join(&file.path);
        let url = url::Url::from_file_path(&path)
            .map_err(|_| anyhow::anyhow!("Invalid title movie URL"))?;
        let data = SwfMovieData::from_data(&movie.save()?, url.to_string(), None, None)?;
        let executor = NullExecutor::new();
        let host = Host(Arc::new(Mutex::new(Observation::default())));
        let events = Events::default();
        let files = Files {
            inner: NullNavigatorBackend::with_base_path(
                path.parent().context("Title movie has no parent")?,
                &executor,
            )?,
            root: root.to_owned(),
            host: host.clone(),
            images: Rc::new(RefCell::new(images)),
            resources: Rc::new(RefCell::new(resources)),
        };
        let renderer = Renderer::for_offscreen(
            (640, 480),
            wgpu::Backends::PRIMARY,
            wgpu::PowerPreference::HighPerformance,
        )
        .map_err(|error| anyhow::anyhow!("Create title renderer: {error}"))?;
        let handle = PlayerBuilder::new()
            .with_movie(data)
            .with_autoplay(true)
            .with_renderer(renderer)
            .with_log(host.clone())
            .with_external_interface(Box::new(host.clone()))
            .with_fs_commands(Box::new(events.clone()))
            .with_navigator(files)
            .build();
        {
            let mut player = lock(&handle)?;
            player.set_background_color(Some(swf::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            }));
            player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions {
                width: 640,
                height: 480,
                scale_factor: 1.,
            });
        }
        Ok(Self {
            handle,
            host,
            events,
            executor,
            texture: None,
            labels,
        })
    }

    fn call(&self, method: &str) -> Result<bool> {
        let mut player = lock(&self.handle)?;
        super::super::ui_calls::Movie::invoke(
            &mut NativeView {
                player: &mut player,
                host: &self.host,
            },
            super::super::ui_calls::Invocation {
                method,
                arguments: &[],
            },
        )
    }

    fn refresh_labels(&self) -> Result<()> {
        let mut player = lock(&self.handle)?;
        let base = "_root.MMC.MainMenuMC";
        for (button, label) in ["StartGameButton", "ExtrasButton"]
            .into_iter()
            .zip(&self.labels)
        {
            if let Some(label) = label {
                let path = format!("{base}.MainPage.{button}.label");
                player.call_internal_interface(
                    "__bwx_frontend_set",
                    [Value::String(path), Value::String(label.clone())],
                );
            }
        }
        if let Some(label) = &self.labels[2] {
            for slot in 1..=3 {
                let path = format!("{base}.LoadMenu.Slot{slot}.TitleMC.Title.text");
                if read(&mut player, &path) == Value::String("UI_NewGame".into()) {
                    player.call_internal_interface(
                        "__bwx_frontend_set",
                        [Value::String(path), Value::String(label.clone())],
                    );
                }
            }
        }
        Ok(())
    }

    fn paint(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let mut player = lock(&self.handle)?;
        if player.needs_render() || self.texture.is_none() {
            player.render();
            let renderer = (player.renderer() as &dyn std::any::Any)
                .downcast_ref::<Renderer>()
                .context("Title renderer unavailable")?;
            if let Some(frame) = renderer.capture_frame() {
                let image = egui::ColorImage::from_rgba_premultiplied([640, 480], frame.as_raw());
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ui.ctx().load_texture(
                        "Original title screen",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }
        if let Some(texture) = &self.texture {
            let rect = ui.max_rect();
            let size = egui::vec2(640., 480.);
            let target = egui::Rect::from_center_size(
                rect.center(),
                size * (rect.width() / size.x).min(rect.height() / size.y),
            );
            ui.painter().image(
                texture.id(),
                target,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
        }
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(1. / 30.));
        Ok(())
    }
}

fn frontend_bridge(version: u8, methods: &[&str]) -> Result<Vec<u8>> {
    const SET: &str = "__bwx_frontend_set";
    let mut bytes = bridge_with_methods(version, methods)?;
    ensure!(bytes.pop() == Some(0), "Missing AVM1 adapter terminator");
    let mut body = Vec::new();
    let mut setter = Writer::new(&mut body, version);
    variable(&mut setter, "path")?;
    variable(&mut setter, "value")?;
    setter.write_action(&Action::SetVariable)?;
    let mut w = Writer::new(&mut bytes, version);
    w.write_action(&Action::DefineFunction(DefineFunction {
        name: swf::SwfStr::from_utf8_str(SET),
        params: vec![
            swf::SwfStr::from_utf8_str("path"),
            swf::SwfStr::from_utf8_str("value"),
        ],
        actions: &body,
    }))?;
    variable(&mut w, "this")?;
    string(&mut w, SET)?;
    w.write_action(&Action::GetMember)?;
    variable(&mut w, "this")?;
    string(&mut w, SET)?;
    push(&mut w, StackValue::Int(3))?;
    variable(&mut w, "flash")?;
    string(&mut w, "external")?;
    w.write_action(&Action::GetMember)?;
    string(&mut w, "ExternalInterface")?;
    w.write_action(&Action::GetMember)?;
    string(&mut w, "addCallback")?;
    w.write_action(&Action::CallMethod)?;
    w.write_action(&Action::Pop)?;
    w.write_action(&Action::End)?;
    Ok(bytes)
}

pub(in crate::blotworx) struct Title {
    stage: Stage,
    root: PathBuf,
    intro: String,
    loop_clip: String,
    last: std::time::Instant,
    pub(in crate::blotworx) tick: u64,
    elapsed: f64,
    background: Option<super::super::movie::Player>,
    looping: bool,
    shown: bool,
    accepted: bool,
    ended: bool,
    callbacks: Vec<String>,
}

impl Title {
    pub(in crate::blotworx) fn load(world: &super::super::assets::World) -> Result<Self> {
        let root = world.source_data_root().canonicalize()?;
        let setting = |key| {
            world
                .runtime_settings
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
                .with_context(|| format!("Missing title movie setting {key}"))
        };
        let intro = setting("UI_StartScreen_IntroMovie")?;
        let loop_clip = setting("UI_StartScreen_LoopMovie")?;
        Ok(Self {
            stage: Stage::load(
                &root,
                "HUDElements/Flash/cooked/startscreen.gfx",
                &["FadeIn", "FadeOut"],
            )?,
            root,
            intro,
            loop_clip,
            tick: 0,
            elapsed: 0.,
            last: std::time::Instant::now(),
            background: None,
            looping: false,
            shown: false,
            accepted: false,
            ended: false,
            callbacks: Vec::new(),
        })
    }

    pub(in crate::blotworx) fn show(&mut self, ui: &mut egui::Ui, accept: bool) -> Result<bool> {
        let was_ended = self.ended;
        let now = std::time::Instant::now();
        let delta = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.elapsed += delta;
        self.tick = (self.elapsed * 30.) as u64;
        {
            let mut player = lock(&self.stage.handle)?;
            player.tick(FloatDuration::from_secs(delta));
            if accept && self.shown && !self.accepted {
                self.accepted = super::super::ui_calls::Movie::invoke(
                    &mut NativeView {
                        player: &mut player,
                        host: &self.stage.host,
                    },
                    super::super::ui_calls::Invocation {
                        method: "FadeOut",
                        arguments: &[],
                    },
                )?;
            }
        }
        self.stage.executor.run();
        let events = std::mem::take(&mut *self.stage.events.0.lock().unwrap());
        for (event, _) in events {
            match event.as_str() {
                "PlayStartScreenMovie" => {
                    self.background = Some(super::super::movie::Player::title_background(
                        self.root.clone(),
                        self.intro.clone(),
                        false,
                    ));
                    self.looping = false;
                }
                "StartScreenEnd" => self.ended = true,
                _ => unreachable!(),
            }
            self.callbacks.push(event);
        }
        if let Some(background) = &mut self.background {
            background.show_title_background(ui);
            if !self.shown && background.frame.is_some() {
                let mut player = lock(&self.stage.handle)?;
                self.shown = super::super::ui_calls::Movie::invoke(
                    &mut NativeView {
                        player: &mut player,
                        host: &self.stage.host,
                    },
                    super::super::ui_calls::Invocation {
                        method: "FadeIn",
                        arguments: &[],
                    },
                )?;
            }
            ensure!(
                background.error.is_none(),
                "Title background: {}",
                background.error.as_deref().unwrap_or("")
            );
            if background.completed && !self.looping {
                self.background = Some(super::super::movie::Player::title_background(
                    self.root.clone(),
                    self.loop_clip.clone(),
                    true,
                ));
                self.looping = true;
            }
        } else {
            ui.painter()
                .rect_filled(ui.max_rect(), 0., egui::Color32::BLACK);
        }
        self.stage.paint(ui)?;
        Ok(self.ended && !was_ended)
    }

    pub(in crate::blotworx) fn summary(&self) -> serde_json::Value {
        let observation = self.stage.host.0.lock().unwrap();
        serde_json::json!({"tick":self.tick,"commands":self.callbacks,"looping":self.looping,
            "shown":self.shown,"accepted":self.accepted,"ended":self.ended,"background":self.background.as_ref().map(super::super::movie::Player::summary),
            "child_movies":observation.fetches,"warnings":observation.warnings})
    }
}

struct ExtrasMenu {
    stage: Stage,
    initialized: bool,
    fade_started: bool,
    slot: usize,
    counts: [String; 4],
}

impl ExtrasMenu {
    fn load(root: &Path, directory: &Path, slot: usize) -> Result<Self> {
        use super::super::{assets, save_service::Service};
        let (_, mut database) = assets::collectible_owners_from_game(root)?;
        // REL1fd328 selects the slot carried by ExtrasMenuLoad.
        match Service::read_checkpoint(directory, slot) {
            Ok(snapshot) => database.restore_save_header(&snapshot.bytes)?,
            Err(error) if error.downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error).context("Read Extras unlocks from the selected save"),
        }
        // RELbc610 fills the category tables. RELbc018 publishes Concept Art,
        // Cartoon and Film counts in this order, with a constant Credits row.
        let [art, cartoons, films] = ["Concept_Art", "Cartoon", "Film"].map(|kind| {
            let entries = database.entries.iter().filter(|entry| entry.kind == kind);
            let total = entries.clone().count();
            let unlocked = entries.filter(|entry| entry.unlocked).count();
            format!("{unlocked}/{total}")
        });
        let stage = Stage::load(
            root,
            "HUDElements/Flash/cooked/extrasmenu.gfx",
            &["Initialize", "FadeIn"],
        )?;
        Ok(Self {
            stage,
            initialized: false,
            fade_started: false,
            slot,
            counts: [art, cartoons, films, "1/1".into()],
        })
    }

    fn initialize(&mut self) -> Result<()> {
        if !self.initialized {
            {
                let mut player = lock(&self.stage.handle)?;
                player.call_internal_interface("__bwx_frontend_set", [
                    Value::String("_global.EXTRASMENU".into()),
                    Value::List(self.counts.iter().cloned().map(Value::String).collect()),
                ]);
            }
            self.initialized = self.stage.call("Initialize")?;
        } else if !self.fade_started {
            // DelayedInit clears intervalID and enters HoldFade. Starting
            // FadeIn before this point leaves the menu behind its hold frame.
            let ready = {
                let mut player = lock(&self.stage.handle)?;
                matches!(read(&mut player, "_root.intervalID"), Value::Null | Value::Undefined)
            };
            if ready {
                self.fade_started = self.stage.call("FadeIn")?;
            }
        }
        Ok(())
    }
}

pub(in crate::blotworx) struct Menu {
    stage: Stage,
    extras: Option<ExtrasMenu>,
    root: PathBuf,
    directory: PathBuf,
    background: Option<super::super::movie::Player>,
    next: Option<String>,
    initialized: bool,
    in_slots: bool,
    last: std::time::Instant,
    replay_down: bool,
    pub(in crate::blotworx) elapsed: f64,
    pub(in crate::blotworx) start_tick: u64,
    pub(in crate::blotworx) title_report: Option<serde_json::Value>,
    commands: Vec<(String, String)>,
    notice: Option<String>,
}

impl Menu {
    pub(in crate::blotworx) fn load(
        root: &Path,
        directory: PathBuf,
        start_tick: u64,
        title_report: Option<serde_json::Value>,
    ) -> Result<Self> {
        let root = root.canonicalize()?;
        let stage = Stage::load(
            &root,
            "HUDElements/Flash/cooked/MainMenu.gfx",
            &[
                "Initialize",
                "FadeIn",
                "RefreshLoadGameButtons",
                "MaintoLoadGame",
                "ExtraToMain",
            ],
        )?;
        Ok(Self {
            stage,
            extras: None,
            root,
            directory,
            background: None,
            next: None,
            initialized: false,
            in_slots: false,
            last: std::time::Instant::now(),
            replay_down: false,
            elapsed: 0.,
            start_tick,
            title_report,
            commands: Vec::new(),
            notice: None,
        })
    }

    fn save_info(&self) -> Result<()> {
        use super::super::{
            checkpoint_metadata::Summary, checkpoint_time::Timing, save_service::Service,
            score_state::Playthrough,
        };
        let mut rows = Vec::new();
        let fields = [
            "enabled",
            "percent_complete",
            "tickets",
            "pins",
            "filmReels",
            "plays",
            "playtime_hours",
            "playtime_minutes",
            "lastSave_month",
            "lastSave_day",
            "lastSave_year",
        ];
        for slot in 0..3 {
            let values = match Service::read_checkpoint(&self.directory, slot) {
                Ok(snapshot) => {
                    let summary = Summary::from_header(&snapshot.bytes)?;
                    let play = Playthrough::from_header(&snapshot.bytes)?.value;
                    let timing = Timing::from_header(&snapshot.bytes)?;
                    let seconds = timing.seconds;
                    let [month, day, year] = save_date(timing.hardware_tick_sample);
                    [
                        1,
                        0,
                        summary.tickets,
                        summary.pins,
                        summary.film_reels,
                        play + 1,
                        seconds / 3600,
                        (seconds / 60) % 60,
                        month,
                        day,
                        year,
                    ]
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    [0; 11]
                }
                Err(error) => return Err(error),
            };
            rows.extend(values.map(|value| Value::Number(f64::from(value))));
        }
        let mut player = lock(&self.stage.handle)?;
        let mut set = |path: &str, value: Value| {
            player
                .call_internal_interface("__bwx_frontend_set", [Value::String(path.into()), value]);
        };
        set(
            "_root.m_numSaveInfoEntries",
            Value::Number(fields.len() as f64),
        );
        for (index, key) in fields.into_iter().enumerate() {
            set(
                &format!("_root.m_saveGameIndex_{key}"),
                Value::Number(index as f64),
            );
        }
        set("_global.SAVEGAMEINFO", Value::List(rows));
        set("_global.USE_NTSC_DATEFORMAT", Value::Bool(true));
        Ok(())
    }

    pub(in crate::blotworx) fn show(
        &mut self,
        ui: &mut egui::Ui,
        pad_edges: u16,
        replay: Option<super::super::input_playback::Frame>,
    ) -> Result<Option<usize>> {
        use ruffle_core::events::{
            KeyDescriptor, KeyLocation, LogicalKey, MouseButton, NamedKey, PhysicalKey, PlayerEvent,
        };
        let now = std::time::Instant::now();
        let delta = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.elapsed += delta;
        let stage = self.extras.as_mut().map_or(&mut self.stage, |extras| &mut extras.stage);
        {
            let rect = ui.max_rect();
            let size = egui::vec2(640., 480.);
            let target = egui::Rect::from_center_size(
                rect.center(),
                size * (rect.width() / size.x).min(rect.height() / size.y),
            );
            let mut player = lock(&stage.handle)?;
            player.tick(FloatDuration::from_secs(delta));
            let mut pointer = |pos: egui::Pos2, press| {
                let x = f64::from((pos.x - target.left()) / target.width()) * 640.;
                let y = f64::from((pos.y - target.top()) / target.height()) * 480.;
                player.handle_event(match press {
                    None => PlayerEvent::MouseMove { x, y },
                    Some(true) => PlayerEvent::MouseDown {
                        x,
                        y,
                        button: MouseButton::Left,
                        index: None,
                    },
                    Some(false) => PlayerEvent::MouseUp {
                        x,
                        y,
                        button: MouseButton::Left,
                    },
                });
            };
            if let Some(frame) = replay {
                if let Some([x, y]) = frame.aim {
                    let pos =
                        target.left_top() + egui::vec2(x * target.width(), y * target.height());
                    pointer(pos, None);
                    if frame.interact_pressed && !self.replay_down {
                        pointer(pos, Some(true));
                        pointer(pos, Some(false));
                    }
                }
                self.replay_down = frame.interact_pressed;
            } else {
                for event in ui.input(|i| i.events.clone()) {
                    match event {
                        egui::Event::PointerMoved(pos) if target.contains(pos) => {
                            pointer(pos, None)
                        }
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            ..
                        } if target.contains(pos) => pointer(pos, Some(pressed)),
                        _ => {}
                    }
                }
            }
            for (egui_key, physical_key, logical_key, bit) in [
                (
                    egui::Key::Enter,
                    PhysicalKey::Enter,
                    NamedKey::Enter,
                    0x1000,
                ),
                (
                    egui::Key::Escape,
                    PhysicalKey::Escape,
                    NamedKey::Escape,
                    0x2000,
                ),
                (
                    egui::Key::ArrowUp,
                    PhysicalKey::ArrowUp,
                    NamedKey::ArrowUp,
                    0x0001,
                ),
                (
                    egui::Key::ArrowDown,
                    PhysicalKey::ArrowDown,
                    NamedKey::ArrowDown,
                    0x0002,
                ),
                (
                    egui::Key::ArrowLeft,
                    PhysicalKey::ArrowLeft,
                    NamedKey::ArrowLeft,
                    0x0004,
                ),
                (
                    egui::Key::ArrowRight,
                    PhysicalKey::ArrowRight,
                    NamedKey::ArrowRight,
                    0x0008,
                ),
            ] {
                if replay.is_none()
                    && (pad_edges & bit != 0 || ui.input(|i| i.key_pressed(egui_key)))
                {
                    let key = KeyDescriptor {
                        physical_key,
                        logical_key: LogicalKey::Named(logical_key),
                        key_location: KeyLocation::Standard,
                    };
                    player.handle_event(PlayerEvent::KeyDown { key });
                    player.handle_event(PlayerEvent::KeyUp { key });
                }
            }
        }
        stage.executor.run();
        if let Some(extras) = &mut self.extras {
            extras.initialize()?;
        } else if !self.initialized && self.stage.call("Initialize")? {
            self.initialized = true;
            self.save_info()?;
            self.stage.call("FadeIn")?;
            self.stage.refresh_labels()?;
        }
        let mut selection = None;
        let events = {
            let stage = self.extras.as_ref().map_or(&self.stage, |extras| &extras.stage);
            std::mem::take(&mut *stage.events.0.lock().unwrap())
        };
        for (command, args) in events {
            match command.as_str() {
                "PlayMovie" => {
                    self.background = Some(super::super::movie::Player::title_background(
                        self.root.clone(),
                        args.clone(),
                        false,
                    ))
                }
                "NextMovie" => self.next = Some(args.clone()),
                "SaveStartupCheck" => {
                    self.save_info()?;
                    self.stage.call("RefreshLoadGameButtons")?;
                    self.stage.refresh_labels()?;
                }
                "MainMenuPick" if args == "StartGameButton" => {
                    self.in_slots = true;
                    self.stage.call("RefreshLoadGameButtons")?;
                    self.stage.refresh_labels()?;
                }
                "MainMenuAction" if args == "FadeInComplete" => self.stage.refresh_labels()?,
                "MainMenuLoad" => {
                    selection = Some(menu_slot(&args)?);
                }
                "ExtrasMenuLoad" if self.extras.is_none() => {
                    self.extras = Some(ExtrasMenu::load(&self.root, &self.directory, menu_slot(&args)?)?);
                    self.background = Some(super::super::movie::Player::title_background(
                        self.root.clone(), "FE_Extras_Loop.bik".into(), true,
                    ));
                    self.next = None;
                    self.notice = None;
                    self.in_slots = false;
                }
                "Extras" if args == "MainMenu" && self.extras.is_some() => {
                    self.extras = None;
                    self.background = None;
                    self.next = None;
                    self.notice = None;
                    self.stage.call("ExtraToMain")?;
                }
                "Extras" | "ExtrasSub" if self.extras.is_some() => {
                    self.notice = Some("This Extras category is unavailable.".into());
                }
                "MainMenuDelete" => self.notice = Some("Delete is unavailable.".into()),
                _ => {}
            }
            self.commands.push((command, args));
        }
        if let Some(background) = &mut self.background {
            background.show_title_background(ui);
            ensure!(
                background.error.is_none(),
                "Main menu background: {}",
                background.error.as_deref().unwrap_or("")
            );
            if let Some(next) = self.next.take_if(|_| background.completed) {
                if next.eq_ignore_ascii_case("FE_LoadGame_Loop.bik") {
                    self.in_slots = true;
                } else if next.eq_ignore_ascii_case("FE_MainMenu_Loop.bik") {
                    self.in_slots = false;
                }
                self.stage.call("FadeIn")?;
                self.background = Some(super::super::movie::Player::title_background(
                    self.root.clone(),
                    next,
                    true,
                ));
            }
        } else {
            ui.painter()
                .rect_filled(ui.max_rect(), 0., egui::Color32::BLACK);
        }
        let stage = self.extras.as_mut().map_or(&mut self.stage, |extras| &mut extras.stage);
        stage.paint(ui)?;
        if let Some(notice) = &self.notice {
            ui.label(notice);
        }
        Ok(selection)
    }

    pub(in crate::blotworx) fn summary(&self) -> serde_json::Value {
        serde_json::json!({"initialized":self.initialized,"in_slots":self.in_slots,"commands":self.commands,
            "extras":self.extras.as_ref().map(|extras| serde_json::json!({"initialized":extras.initialized,"fade_started":extras.fade_started,"slot":extras.slot,"counts":extras.counts})),
            "background":self.background.as_ref().map(super::super::movie::Player::summary),
            "child_movies":self.stage.host.0.lock().unwrap().fetches})
    }
}

fn menu_slot(name: &str) -> Result<usize> {
    name.strip_prefix("Slot")
        .and_then(|slot| slot.parse::<usize>().ok())
        .filter(|slot| (1..=3).contains(slot))
        .map(|slot| slot - 1)
        .with_context(|| format!("Invalid authored save slot {name:?}"))
}

// Save headers carry the same Wii timebase sample published at RTC boot.
fn save_date(sample: u64) -> [i32; 3] {
    let rate = super::super::game_clock::Rate::from_bus_clock(243_000_000).unwrap();
    let Ok(seconds) = u32::try_from(sample / u64::from(rate.ticks_per_second.get())) else {
        return [0; 3];
    };
    let mut days = seconds / 86400;
    let mut year = 2000;
    let leap = |y| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    while days >= 365 + u32::from(leap(year)) {
        days -= 365 + u32::from(leap(year));
        year += 1;
    }
    for (month, length) in [
        31,
        28 + u32::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
    .into_iter()
    .enumerate()
    {
        if days < length {
            return [month as i32 + 1, days as i32 + 1, year];
        }
        days -= length;
    }
    unreachable!("calendar overflow")
}
