use super::*;
use std::collections::BTreeMap;
use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};

#[path = "ui_extras_images.rs"]
mod extras_images;

type Renderer = WgpuRenderBackend<TextureTarget>;
type Labels = BTreeMap<String, String>;

const EXTRAS_CARDS: [&str; 4] = ["Concept", "Cartoons", "Movies", "Credits"];
const EXTRAS_LABEL_FIELDS: [&str; 3] = ["textField", "textField3", "textField3_static"];

fn extras_label_paths() -> impl Iterator<Item = String> {
    EXTRAS_CARDS.into_iter().flat_map(|card| {
        EXTRAS_LABEL_FIELDS.map(|field| {
            format!("_root.ExtrasMC.ExtrasMenuMC.{card}.{field}.text")
        })
    })
}

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
            | "Error"
            | "ExtrasMenuLoad"
            | "Extras"
            | "ExtrasSub"
            | "ExtrasRollover" => {
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
    extra_keys: impl Iterator<Item = String>,
) -> Result<Labels> {
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
    Ok([
        "UI_StartGame", "UI_Extras", "UI_NewGame", "UI_ExtrasConcept", "UI_ExtrasCartoons",
        "UI_ExtrasMovies", "UI_ExtrasCredits", "UI_Unlocked", "UI_Unlocked_Singular",
    ].into_iter().map(str::to_owned).chain(extra_keys).filter_map(|key| {
        let hash = epic_mickey_hash(key.as_bytes(), locale.hash_seed);
        locale.dialog_entries.iter()
            .find(|entry| entry.hashed_key == hash)
            .map(|entry| (key, entry.text.clone()))
    }).collect())
}

struct Stage {
    handle: Arc<Mutex<ruffle_core::Player>>,
    host: Host,
    events: Events,
    executor: NullExecutor,
    texture: Option<egui::TextureHandle>,
    labels: Labels,
    viewport: [u32; 2],
}

impl Stage {
    fn rect(ui: &egui::Ui) -> egui::Rect {
        let rect = ui.max_rect();
        let size = egui::vec2(640., 480.);
        egui::Rect::from_center_size(rect.center(), size * (rect.width() / size.x).min(rect.height() / size.y))
    }

    fn resize(&mut self, ui: &egui::Ui, target: egui::Rect) -> Result<()> {
        let height = (target.height() * ui.ctx().pixels_per_point()).round().clamp(1., 1440.) as u32;
        let viewport = [(height * 4).div_ceil(3), height];
        if viewport != self.viewport {
            lock(&self.handle)?.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions {
                width: viewport[0],
                height: viewport[1],
                scale_factor: f64::from(height) / 480.,
            });
            self.viewport = viewport;
        }
        Ok(())
    }

    fn load(root: &Path, path_name: &str, methods: &[&str]) -> Result<Self> {
        Self::load_with_labels(root, path_name, methods, std::iter::empty())
    }

    fn load_with_labels(
        root: &Path,
        path_name: &str,
        methods: &[&str],
        extra_keys: impl Iterator<Item = String>,
    ) -> Result<Self> {
        let mut resources = super::super::resource_store::Store::new(
            root,
            &["MainMenuTextures.pak".into(), "HUDTextures.pak".into()],
        )?;
        let file = resources.read(path_name)?;
        let mut movie = crate::gfx::GfxFile::load(&file.data)?;
        let labels = localize_frontend_labels(&mut movie, &mut resources, extra_keys)?;
        let mut images = super::super::ui_images::Images::default();
        images.adapt(&mut movie, &file.path, &mut resources)?;
        if path_name.ends_with("/extrasmenu.gfx") {
            extras_images::prepare(&mut movie, &mut resources)?;
        }
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
        let (renderer, _) = super::gpu::create_sized(false, (640, 480))
            .context("Create title renderer")?;
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
            viewport: [640, 480],
        })
    }

    fn call(&self, method: &str) -> Result<bool> {
        self.call_with(method, &[])
    }

    fn call_with(&self, method: &str, arguments: &[super::super::ui_calls::Value<'_>]) -> Result<bool> {
        let mut player = lock(&self.handle)?;
        super::super::ui_calls::Movie::invoke(
            &mut NativeView {
                player: &mut player,
                host: &self.host,
            },
            super::super::ui_calls::Invocation {
                method,
                arguments,
            },
        )
    }

    fn refresh_labels(&self) -> Result<()> {
        let mut player = lock(&self.handle)?;
        let base = "_root.MMC.MainMenuMC";
        for (button, key) in [
            ("StartGameButton", "UI_StartGame"),
            ("ExtrasButton", "UI_Extras"),
        ] {
            if let Some(label) = self.labels.get(key) {
                let path = format!("{base}.MainPage.{button}.label");
                player.call_internal_interface(
                    "__bwx_frontend_set",
                    [Value::String(path), Value::String(label.clone())],
                );
            }
        }
        if let Some(label) = self.labels.get("UI_NewGame") {
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

    fn bind_menu_controls(&self) -> Result<()> {
        let mut player = lock(&self.handle)?;
        for (key, code) in [
            ("aKey", 13.), ("bKey", 27.), ("plusKey", 34.), ("minusKey", 33.),
            ("upKey", 38.), ("downKey", 40.), ("leftKey", 37.), ("rightKey", 39.),
        ] {
            player.call_internal_interface(
                "__bwx_frontend_set",
                [Value::String(format!("_global.{key}")), Value::Number(code)],
            );
        }
        Ok(())
    }

    fn hide_extras_counter_sources(&self) -> Result<()> {
        let mut player = lock(&self.handle)?;
        // The original EditText definitions for these backing fields have
        // zero alpha. Scaleform keeps them hidden when text is assigned.
        for card in EXTRAS_CARDS {
            for field in ["textField2_static", "textField3_static"] {
                let path = format!("_root.ExtrasMC.ExtrasMenuMC.{card}.{field}._alpha");
                player.call_internal_interface(
                    "__bwx_frontend_set",
                    [Value::String(path), Value::Number(0.)],
                );
            }
        }
        Ok(())
    }

    fn refresh_extras_labels(&self) -> Result<()> {
        let mut player = lock(&self.handle)?;
        // Rollover and fade frames can replace these text fields. Translate
        // the key on the live field without changing the authored label ID.
        for path in extras_label_paths() {
            if let Value::String(key) = read(&mut player, &path) {
                if let Some(label) = self.labels.get(key.as_str()) {
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
        let target = Self::rect(ui);
        self.resize(ui, target)?;
        let mut player = lock(&self.handle)?;
        if player.needs_render() || self.texture.is_none() {
            player.render();
            let renderer = (player.renderer() as &dyn std::any::Any)
                .downcast_ref::<Renderer>()
                .context("Title renderer unavailable")?;
            if let Some(frame) = renderer.capture_frame() {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width() as usize, frame.height() as usize],
                    frame.as_raw(),
                );
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

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
enum ExtrasCategory {
    Concept,
    Movies,
    Cartoons,
}

impl ExtrasCategory {
    fn from_command(command: &str) -> Option<Self> {
        match command {
            "Concept" => Some(Self::Concept),
            "Movies" => Some(Self::Movies),
            "Cartoons" => Some(Self::Cartoons),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Concept => 0,
            Self::Movies => 1,
            Self::Cartoons => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
enum ExtrasPage {
    Landing,
    Entering(ExtrasCategory),
    Browsing(ExtrasCategory),
    Leaving,
    Watching(ExtrasCategory),
    ViewingArt(usize),
    Credits,
}

struct ExtrasMenu {
    stage: Stage,
    initialized: bool,
    fade_started: bool,
    slot: usize,
    counts: [String; 4],
    items: [Vec<super::super::extras::Entry>; 3],
    page: ExtrasPage,
    offset: usize,
    assets: super::super::resource_store::Store,
    locked_thumbnail: Vec<u8>,
    concept_art: Option<String>,
    last_credits: Option<serde_json::Value>,
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
        // RELbc740 maps the three native states to Concept_Art, Film,
        // and Cartoon. REL113794 inserts unlocked entries ahead of locked
        // entries and orders each group by the generated _Name key.
        let mut items = [Vec::new(), Vec::new(), Vec::new()];
        for entry in database.entries {
            let index = match entry.kind.as_str() {
                "Concept_Art" => 0,
                "Film" => 1,
                "Cartoon" => 2,
                _ => continue,
            };
            items[index].push(entry);
        }
        for group in &mut items {
            group.sort_by_cached_key(|entry| (!entry.unlocked, format!("{}_Name", entry.name)));
        }
        let [art, films, cartoons] = items.each_ref().map(|entries| {
            let unlocked = entries.iter().take_while(|entry| entry.unlocked).count();
            format!("{unlocked}/{}", entries.len())
        });
        let extra_keys = items.iter().flatten().filter(|item| item.unlocked).flat_map(|item| {
            [format!("{}_Name", item.name), format!("{}_Desc", item.name)]
        });
        let stage = Stage::load_with_labels(
            root,
            "HUDElements/Flash/cooked/extrasmenu.gfx",
            &["Initialize", "FadeIn", "UpdateSubMenu", "EnableScroll", "RestoreMenu",
                "RestoreMovieMenu", "RestoreCartoonMenu", "SetExtrasInformation"],
            extra_keys,
        )?;
        let mut assets = super::super::resource_store::Store::new(root, &[])?;
        let locked_thumbnail = extras_images::read_texture(&mut assets, extras_images::LOCKED, extras_images::THUMBNAIL_SIZE)?;
        Ok(Self {
            stage,
            initialized: false,
            fade_started: false,
            slot,
            counts: [art, cartoons, films, "1/1".into()],
            items,
            page: ExtrasPage::Landing,
            offset: 0,
            assets,
            locked_thumbnail,
            concept_art: None,
            last_credits: None,
        })
    }

    fn initialize(&mut self) -> Result<()> {
        if !self.initialized {
            self.stage.bind_menu_controls()?;
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
                self.stage.hide_extras_counter_sources()?;
                self.fade_started = self.stage.call("FadeIn")?;
            }
        }
        Ok(())
    }

    const PAGE_SIZE: usize = 6;

    fn transition(&self, active: bool) -> Result<()> {
        let mut player = lock(&self.stage.handle)?;
        player.call_internal_interface("__bwx_frontend_set", [
            Value::String("_global.IsInTransitionMovie".into()),
            Value::Bool(active),
        ]);
        Ok(())
    }

    fn publish_category(&mut self, category: ExtrasCategory) -> Result<()> {
        use super::super::ui_calls::Value as Argument;
        let items = &self.items[category.index()];
        {
            let mut player = lock(&self.stage.handle)?;
            for (path, array) in [
                ("_global.EXTRALOCKED", items.iter().map(|item| Value::Number(f64::from(!item.unlocked))).collect()),
                ("_global.EXTRA_NEWLY_UNLOCKED", vec![Value::Number(0.); items.len()]),
            ] {
                player.call_internal_interface("__bwx_frontend_set", [
                    Value::String(path.into()), Value::List(array),
                ]);
            }
        }
        ensure!(
            self.stage.call_with("UpdateSubMenu", &[Argument::Number(items.len() as f64)])?,
            "Authored Extras submenu callback is unavailable"
        );
        self.stage.call("EnableScroll")?;
        self.update_thumbnails(category, 0)?;
        self.page = ExtrasPage::Browsing(category);
        Ok(())
    }

    fn update_thumbnails(&mut self, category: ExtrasCategory, offset: usize) -> Result<()> {
        let mut player = lock(&self.stage.handle)?;
        let items = &self.items[category.index()];
        for (index, texture) in extras_images::PLACES.into_iter().enumerate() {
            let image = items.get(offset + index)
                .filter(|item| item.unlocked && !item.thumbnail.is_empty())
                .map(|item| extras_images::read_texture(&mut self.assets, &item.thumbnail, extras_images::THUMBNAIL_SIZE))
                .transpose()?;
            extras_images::publish(
                &mut player, texture, image.as_deref().unwrap_or(&self.locked_thumbnail),
            )?;
        }
        self.offset = offset;
        Ok(())
    }

    fn item_index(&self, command: &str) -> Option<usize> {
        let slot = command.strip_prefix("thumbitem").unwrap_or(command).parse::<usize>().ok()?;
        (1..=Self::PAGE_SIZE).contains(&slot).then(|| self.offset + slot - 1)
    }

    fn show_details(&self, category: ExtrasCategory, index: Option<usize>) -> Result<()> {
        use super::super::ui_calls::Value as Argument;
        let item = index.and_then(|index| self.items[category.index()].get(index))
            .filter(|item| item.unlocked);
        let lookup = |suffix| item.and_then(|item| {
            self.stage.labels.get(&format!("{}{suffix}", item.name)).map(String::as_str)
        }).unwrap_or("");
        self.stage.call_with("SetExtrasInformation", &[
            Argument::String(lookup("_Name")),
            Argument::String(lookup("_Desc")),
        ])?;
        Ok(())
    }

    fn show_concept_art(&mut self, index: usize) -> Result<()> {
        let item = &self.items[ExtrasCategory::Concept.index()][index];
        let pixels = extras_images::read_texture(&mut self.assets, &item.asset, extras_images::CONCEPT_SIZE)?;
        let mut player = lock(&self.stage.handle)?;
        extras_images::publish(&mut player, extras_images::CONCEPT_ART, &pixels)?;
        self.concept_art = Some(item.asset.clone());
        self.page = ExtrasPage::ViewingArt(index);
        Ok(())
    }

    fn sync_concept_viewer(&mut self) -> Result<()> {
        if let ExtrasPage::ViewingArt(_) = self.page {
            let mut player = lock(&self.stage.handle)?;
            // The authored Back handler closes the viewer locally with no FSCommand.
            if read(&mut player, "_root.currentSubMenuIndex") == Value::Number(0.) {
                self.page = ExtrasPage::Browsing(ExtrasCategory::Concept);
                self.concept_art = None;
            }
        }
        Ok(())
    }

    fn summary(&self) -> serde_json::Value {
        let (labels, details) = lock(&self.stage.handle).map(|mut player| {
            let labels = extras_label_paths().filter_map(|path| match read(&mut player, &path) {
                Value::String(value) => Some((path, value)),
                _ => None,
            }).collect::<BTreeMap<_, _>>();
            let details = ["TitleInfo", "DescText"].map(|field| {
                match read(&mut player, &format!("_root.ExtrasMC.Thumbs.{field}.text")) {
                    Value::String(value) => Some(value),
                    _ => None,
                }
            });
            (labels, details)
        }).unwrap_or_default();
        serde_json::json!({
            "initialized": self.initialized, "fade_started": self.fade_started,
            "slot": self.slot, "counts": self.counts, "labels": labels,
            "page": self.page, "offset": self.offset, "details": details,
            "concept_art": self.concept_art, "last_credits": self.last_credits,
            "visible_entries": match self.page {
                ExtrasPage::Browsing(category) | ExtrasPage::Watching(category) => self.items[category.index()]
                    .iter().skip(self.offset).take(Self::PAGE_SIZE).collect::<Vec<_>>(),
                _ => Vec::new(),
            },
        })
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
    replay_back_down: bool,
    pub(in crate::blotworx) elapsed: f64,
    pub(in crate::blotworx) start_tick: u64,
    pub(in crate::blotworx) title_report: Option<serde_json::Value>,
    commands: Vec<(String, String)>,
    delete: Option<DeleteConfirmation>,
    last_deleted_slot: Option<usize>,
    pad_cursor: Option<[f32; 2]>,
}

struct DeleteConfirmation {
    stage: Stage,
    slot: usize,
    expected: Arc<super::super::checkpoint::Snapshot>,
    initialized: bool,
    accepts_input: bool,
    answer: Option<bool>,
}

impl DeleteConfirmation {
    fn tick(&mut self, delta: f64, answer: Option<bool>) -> Result<bool> {
        use super::super::ui_calls::Value as Argument;
        lock(&self.stage.handle)?.tick(FloatDuration::from_secs(delta));
        self.stage.executor.run();
        if !self.initialized && self.stage.call("FadeIn")? {
            for (method, key) in [
                ("SetMessageText", "Lotcheck_Confirm_Delete"),
                ("SetAButtonText", "Lotcheck_Delete_File"),
                ("SetBButtonText", "Lotcheck_No"),
            ] {
                let label = self.stage.labels.get(key).with_context(|| format!("Missing save prompt text {key}"))?;
                ensure!(self.stage.call_with(method, &[Argument::String(label)])?, "Missing save prompt method {method}");
            }
            self.initialized = true;
        }
        let events = std::mem::take(&mut *self.stage.events.0.lock().unwrap());
        for (command, args) in events {
            if command == "Error" {
                match args.as_str() {
                    "FadeInComplete" => self.accepts_input = true,
                    "FadeOutComplete" => return Ok(true),
                    _ => {}
                }
            }
        }
        if self.accepts_input && self.answer.is_none() {
            if let Some(answer) = answer {
                self.answer = Some(answer);
                ensure!(self.stage.call("FadeOut")?, "Save prompt did not close");
            }
        }
        Ok(false)
    }
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
            replay_back_down: false,
            elapsed: 0.,
            start_tick,
            title_report,
            commands: Vec::new(),
            delete: None,
            last_deleted_slot: None,
            pad_cursor: None,
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
        pad_released: u16,
        pad_stick: [f32; 2],
        replay: Option<super::super::input_playback::Frame>,
    ) -> Result<Option<usize>> {
        use ruffle_core::events::{
            KeyDescriptor, KeyLocation, LogicalKey, MouseButton, NamedKey, PhysicalKey, PlayerEvent,
        };
        let now = std::time::Instant::now();
        let delta = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.elapsed += delta;
        let replay_back = replay.is_some_and(|frame| frame.spin_pressed);
        let replay_back_pressed = replay_back && !self.replay_back_down;
        self.replay_back_down = replay_back;
        if let Some(prompt) = &mut self.delete {
            let replay_accept = replay.is_some_and(|frame| frame.interact_pressed);
            let (accept, back) = if replay.is_some() {
                (replay_accept && !self.replay_down, replay_back_pressed)
            } else {
                (pad_released & 0x1000 != 0 || ui.input(|i| i.key_released(egui::Key::Enter)),
                 pad_released & 0x2000 != 0 || ui.input(|i| i.key_released(egui::Key::Escape)))
            };
            self.replay_down = replay_accept;
            let answer = if back { Some(false) } else { accept.then_some(true) };
            if let Some(background) = &mut self.background {
                background.show_title_background(ui);
            }
            self.stage.paint(ui)?;
            if prompt.tick(delta, answer)? {
                let prompt = self.delete.take().unwrap();
                if prompt.answer == Some(true) {
                    super::super::save_service::Service::delete_confirmed(&self.directory, prompt.slot, &prompt.expected)?;
                    self.last_deleted_slot = Some(prompt.slot);
                    self.save_info()?;
                    self.stage.call("RefreshLoadGameButtons")?;
                    self.stage.refresh_labels()?;
                }
            } else {
                prompt.stage.paint(ui)?;
            }
            return Ok(None);
        }
        if self.extras.as_ref().is_some_and(|extras| extras.page == ExtrasPage::Credits) {
            let quick_exit = if replay.is_some() {
                replay_back_pressed
            } else {
                pad_released & 0x2000 != 0 || ui.input(|input| {
                    input.key_released(egui::Key::Escape) || input.key_released(egui::Key::Backspace)
                })
            };
            self.present_extras_credits(ui, quick_exit)?;
            return Ok(None);
        }

        let target = Stage::rect(ui);
        let stage = self.extras.as_mut().map_or(&mut self.stage, |extras| &mut extras.stage);
        stage.resize(ui, target)?;
        {
            let viewport = stage.viewport;
            let point = |[x, y]: [f32; 2]| target.left_top() + egui::vec2(x * target.width(), y * target.height());
            let mut player = lock(&stage.handle)?;
            player.tick(FloatDuration::from_secs(delta));
            let mut pointer = |pos: egui::Pos2, press| {
                let x = f64::from((pos.x - target.left()) / target.width().max(1.)) * f64::from(viewport[0]);
                let y = f64::from((pos.y - target.top()) / target.height().max(1.)) * f64::from(viewport[1]);
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
                    let pos = point([x, y]);
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
                        egui::Event::PointerMoved(pos) => {
                            self.pad_cursor = None;
                            pointer(pos, None);
                        }
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            ..
                        } if target.contains(pos) || !pressed => {
                            self.pad_cursor = None;
                            pointer(pos, Some(pressed));
                        }
                        _ => {}
                    }
                }
                if pad_stick != [0.; 2] {
                    let cursor = self.pad_cursor.get_or_insert([0.5; 2]);
                    let step = delta.min(0.05) as f32;
                    cursor[0] = (cursor[0] + pad_stick[0] * step * 0.75).clamp(0., 0.999);
                    cursor[1] = (cursor[1] - pad_stick[1] * step).clamp(0., 0.999);
                }
                if let Some(cursor) = self.pad_cursor {
                    let pos = point(cursor);
                    pointer(pos, None);
                    if pad_edges & 0x1000 != 0 { pointer(pos, Some(true)); }
                    if pad_released & 0x1000 != 0 { pointer(pos, Some(false)); }
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
                (egui::Key::PageUp, PhysicalKey::PageUp, NamedKey::PageUp, 0x0020),
            ] {
                let pressed = if replay.is_some() {
                    (egui_key == egui::Key::Escape && replay_back_pressed)
                        || (egui_key == egui::Key::PageUp && replay.is_some_and(|frame| frame.menu_delete_pressed))
                } else {
                    (pad_edges & bit != 0 && (bit != 0x1000 || self.pad_cursor.is_none()))
                        || ui.input(|i| i.key_pressed(egui_key))
                };
                if pressed {
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
            extras.sync_concept_viewer()?;
            if extras.fade_started {
                extras.stage.refresh_extras_labels()?;
            }
        } else if !self.initialized {
            if self.stage.call("Initialize")? {
                self.initialized = true;
                self.stage.bind_menu_controls()?;
                self.save_info()?;
                self.stage.call("FadeIn")?;
                self.stage.refresh_labels()?;
            }
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
                    self.in_slots = false;
                }
                "Extras" if args == "MainMenu" && self.extras.is_some() => {
                    self.extras = None;
                    self.background = None;
                    self.next = None;
                    self.stage.call("ExtraToMain")?;
                }
                "Extras" if self.extras.is_some() => self.handle_extras_command(&args)?,
                "ExtrasSub" if self.extras.is_some() => self.open_extra(&args)?,
                "ExtrasRollover" => {
                    if let Some(extras) = &self.extras {
                        if let ExtrasPage::Browsing(category) = extras.page {
                            extras.show_details(category, extras.item_index(&args))?;
                        }
                    }
                }
                "MainMenuDelete" if self.in_slots && self.extras.is_none() && self.delete.is_none() => {
                    let slot = menu_slot(&args)?;
                    use super::super::save_service::Service;
                    match Service::read_checkpoint(&self.directory, slot) {
                        Ok(expected) => {
                            let keys = ["Lotcheck_Confirm_Delete", "Lotcheck_Delete_File", "Lotcheck_No"];
                            let stage = Stage::load_with_labels(&self.root, "HUDElements/Flash/cooked/error.gfx",
                                &["FadeIn", "FadeOut", "SetMessageText", "SetAButtonText", "SetBButtonText"],
                                keys.into_iter().map(str::to_owned))?;
                            self.delete = Some(DeleteConfirmation { stage, slot, expected,
                                initialized: false, accepts_input: false, answer: None });
                        }
                        Err(error) if error.downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
                        Err(error) => return Err(error),
                    }
                }
                _ => {}
            }
            self.commands.push((command, args));
        }
        if self.extras.as_ref().is_some_and(|extras| extras.page == ExtrasPage::Credits) {
            self.present_extras_credits(ui, false)?;
            return Ok(None);
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
        if self.background.as_ref().is_some_and(|background| background.completed) {
            self.finish_extras_movie()?;
        }
        let stage = self.extras.as_mut().map_or(&mut self.stage, |extras| &mut extras.stage);
        stage.paint(ui)?;
        if let Some([x, y]) = self.pad_cursor {
            let tip = target.left_top() + egui::vec2(x * target.width(), y * target.height());
            let outline = [[0., 0.], [2., 19.], [15., 12.]];
            ui.painter().add(egui::Shape::convex_polygon(
                outline.into_iter().map(|[x, y]| tip + egui::vec2(x, y)).collect(),
                egui::Color32::WHITE,
                egui::Stroke::new(1.5, egui::Color32::BLACK),
            ));
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
        Ok(selection)
    }

    fn extras_movie(&mut self, filename: String, looping: bool) {
        self.background = Some(super::super::movie::Player::title_background(
            self.root.clone(), filename, looping,
        ));
    }

    fn handle_extras_command(&mut self, command: &str) -> Result<()> {
        let extras = self.extras.as_mut().context("Extras menu is closed")?;
        if let Some(category) = ExtrasCategory::from_command(command) {
            extras.transition(true)?;
            extras.page = ExtrasPage::Entering(category);
            let number = category.index() + 1;
            self.extras_movie(format!("FE_Extras_to_Sub{number}.bik"), false);
        } else if let ExtrasPage::Watching(_) = extras.page {
            if command == "StopMovie" {
                self.finish_extras_movie()?;
            }
        } else if let ExtrasPage::Browsing(category) = extras.page {
            if command == "Main" {
                extras.transition(true)?;
                extras.page = ExtrasPage::Leaving;
                let number = category.index() + 1;
                self.extras_movie(format!("FE_Extras_Sub{number}_to_Extras.bik"), false);
            } else if let Ok(page) = command.parse::<usize>() {
                if let Some(offset) = page.checked_mul(ExtrasMenu::PAGE_SIZE)
                    .filter(|offset| *offset < extras.items[category.index()].len())
                {
                    extras.update_thumbnails(category, offset)?;
                }
                extras.stage.call("EnableScroll")?;
            }
        } else if command == "Credits" && extras.page == ExtrasPage::Landing {
            extras.page = ExtrasPage::Credits;
            self.background = Some(super::super::movie::Player::extras_credits(self.root.clone()));
        }
        Ok(())
    }

    fn open_extra(&mut self, command: &str) -> Result<()> {
        let extras = self.extras.as_mut().context("Extras menu is closed")?;
        let ExtrasPage::Browsing(category) = extras.page else { return Ok(()); };
        let Some(index) = extras.item_index(command) else { return Ok(()); };
        let Some(item) = extras.items[category.index()].get(index)
            .filter(|item| item.unlocked) else { return Ok(()); };
        if category == ExtrasCategory::Concept {
            return extras.show_concept_art(index);
        }
        let file = item.asset.replace('\\', "/");
        ensure!(!file.is_empty(), "Unlocked Extras movie has no asset");
        extras.page = ExtrasPage::Watching(category);
        self.extras_movie(file, false);
        Ok(())
    }

    fn present_extras_credits(&mut self, ui: &mut egui::Ui, quick_exit: bool) -> Result<()> {
        let movie = self.background.as_mut().context("Missing Extras credits reel")?;
        if quick_exit {
            movie.quick_exit();
        }
        movie.show_title_background(ui);
        ensure!(movie.error.is_none(), "Extras credits: {}", movie.error.as_deref().unwrap_or(""));
        if movie.completed {
            let extras = self.extras.as_mut().context("Missing Extras return state")?;
            extras.last_credits = Some(movie.summary());
            extras.stage.call("RestoreMenu")?;
            extras.stage.call("FadeIn")?;
            extras.page = ExtrasPage::Landing;
            self.extras_movie("FE_Extras_Loop.bik".into(), true);
        }
        Ok(())
    }

    fn finish_extras_movie(&mut self) -> Result<()> {
        let Some(extras) = &mut self.extras else { return Ok(()); };
        match extras.page {
            ExtrasPage::Entering(category) => {
                extras.transition(false)?;
                extras.publish_category(category)?;
                extras.stage.call("FadeIn")?;
                self.extras_movie(format!("FE_Extras_Sub{}_Loop.bik", category.index() + 1), true);
            }
            ExtrasPage::Leaving => {
                extras.transition(false)?;
                extras.stage.call("RestoreMenu")?;
                extras.stage.call("FadeIn")?;
                extras.page = ExtrasPage::Landing;
                self.extras_movie("FE_Extras_Loop.bik".into(), true);
            }
            ExtrasPage::Watching(category) => {
                extras.stage.call(match category {
                    ExtrasCategory::Movies => "RestoreMovieMenu",
                    ExtrasCategory::Cartoons => "RestoreCartoonMenu",
                    ExtrasCategory::Concept => unreachable!(),
                })?;
                extras.page = ExtrasPage::Browsing(category);
                self.extras_movie(format!("FE_Extras_Sub{}_Loop.bik", category.index() + 1), true);
            }
            ExtrasPage::Landing | ExtrasPage::Browsing(_) | ExtrasPage::ViewingArt(_) | ExtrasPage::Credits => {}
        }
        Ok(())
    }

    pub(in crate::blotworx) fn summary(&self) -> serde_json::Value {
        serde_json::json!({"initialized":self.initialized,"in_slots":self.in_slots,"commands":self.commands,
            "extras":self.extras.as_ref().map(ExtrasMenu::summary), "delete_prompt":self.delete.as_ref().map(|prompt|prompt.slot),
            "last_deleted_slot":self.last_deleted_slot,
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
