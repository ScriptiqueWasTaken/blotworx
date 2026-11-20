use super::*;
use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};

type Renderer = WgpuRenderBackend<TextureTarget>;

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<String>>>);

impl FsCommandProvider for Events {
    fn on_fs_command(&self, command: &str, _args: &str) -> bool {
        match command {
            "PlayStartScreenMovie" | "StartScreenEnd" => {
                self.0.lock().unwrap().push(command.to_owned());
                true
            }
            _ => false,
        }
    }
}

fn localize_start_label(
    movie: &mut crate::gfx::GfxFile,
    resources: &mut super::super::resource_store::Store,
) -> Result<()> {
    use epic_mickey_lib_rs::dct::{epic_mickey_hash, DCT};
    resources.admit_packs(&["English_NTSC.pak".into()])?;
    let file = resources.read("Localize/ENGLISH_NTSC_RVL.dct")?;
    let locale = DCT::from_binary(file.data);
    let key = "UI_Start";
    let hash = epic_mickey_hash(key.as_bytes(), locale.hash_seed);
    let text = &locale
        .dialog_entries
        .iter()
        .find(|entry| entry.hashed_key == hash)
        .context("Missing localized title prompt")?
        .text;
    let label = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    for tag in movie.tags.iter_mut().filter(|tag| tag.code == 37) {
        let data = swf::read::Reader::new(&tag.body, movie.version).read_define_edit_text()?;
        let Some(initial) = data.initial_text() else {
            continue;
        };
        let initial = std::str::from_utf8(initial.as_bytes())?;
        if let Some((before, after)) = initial.split_once(key) {
            let replacement = format!("{before}{label}{after}");
            let start = tag.body.len() - initial.len() - 1;
            tag.body.truncate(start);
            tag.body
                .extend(replacement.bytes().filter(|&byte| byte != 0));
            tag.body.push(0);
            return Ok(());
        }
    }
    anyhow::bail!("Original title movie has no UI_Start field")
}

pub(in crate::blotworx) struct Title {
    handle: Arc<Mutex<ruffle_core::Player>>,
    host: Host,
    events: Events,
    executor: NullExecutor,
    root: PathBuf,
    intro: String,
    loop_clip: String,
    last: std::time::Instant,
    pub(in crate::blotworx) tick: u64,
    elapsed: f64,
    texture: Option<egui::TextureHandle>,
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
        let mut resources = super::super::resource_store::Store::new(
            &root,
            &["MainMenuTextures.pak".into(), "HUDTextures.pak".into()],
        )?;
        let file = resources.read("HUDElements/Flash/cooked/startscreen.gfx")?;
        let mut movie = crate::gfx::GfxFile::load(&file.data)?;
        localize_start_label(&mut movie, &mut resources)?;
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
                body: bridge_with_methods(movie.version, &["FadeIn", "FadeOut"])?,
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
            root: root.clone(),
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
            root,
            intro,
            loop_clip,
            tick: 0,
            elapsed: 0.,
            last: std::time::Instant::now(),
            texture: None,
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
            let mut player = lock(&self.handle)?;
            player.tick(FloatDuration::from_secs(delta));
            if accept && self.shown && !self.accepted {
                self.accepted = super::super::ui_calls::Movie::invoke(
                    &mut NativeView {
                        player: &mut player,
                        host: &self.host,
                    },
                    super::super::ui_calls::Invocation {
                        method: "FadeOut",
                        arguments: &[],
                    },
                )?;
            }
        }
        self.executor.run();
        let events = std::mem::take(&mut *self.events.0.lock().unwrap());
        for event in events {
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
                let mut player = lock(&self.handle)?;
                self.shown = super::super::ui_calls::Movie::invoke(
                    &mut NativeView {
                        player: &mut player,
                        host: &self.host,
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
        Ok(self.ended && !was_ended)
    }

    pub(in crate::blotworx) fn summary(&self) -> serde_json::Value {
        let observation = self.host.0.lock().unwrap();
        serde_json::json!({"tick":self.tick,"commands":self.callbacks,"looping":self.looping,
            "shown":self.shown,"accepted":self.accepted,"ended":self.ended,"background":self.background.as_ref().map(super::super::movie::Player::summary),
            "child_movies":observation.fetches,"warnings":observation.warnings})
    }
}
