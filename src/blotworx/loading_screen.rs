use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use std::{
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

const DEFAULT_ART: &str = "LoadingScreens/MickeyInitialScreen.dds";
const MINIMUM_PRESENTATION: Duration = Duration::from_millis(330);
const DOT_FRAME: Duration = Duration::from_millis(110);

pub(super) struct Screen {
    pending: Option<Receiver<Artwork>>,
    opened: Instant,
    background: Option<TextureHandle>,
    dots: Vec<TextureHandle>,
    first_paint: Option<Instant>,
}

impl Screen {
    pub(super) fn load(
        ctx: &egui::Context,
        root: &Path,
        destination: Option<&str>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let root = root.to_owned();
        let destination = destination.map(str::to_owned);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Artwork::load(&root, destination.as_deref()));
            ctx.request_repaint();
        });
        Self {
            pending: Some(receiver),
            opened: Instant::now(),
            background: None,
            dots: Vec::new(),
            first_paint: None,
        }
    }

    pub(super) fn has_presented(&self) -> bool {
        self.first_paint.is_some()
    }

    pub(super) fn ready_to_dismiss(&self) -> bool {
        self.first_paint
            .is_some_and(|start| start.elapsed() >= MINIMUM_PRESENTATION)
    }

    pub(super) fn paint(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(artwork) => {
                    let upload = |(name, image)| ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.background = artwork.background.map(upload);
                    self.dots = artwork.dots.into_iter().map(upload).collect();
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    log::warn!("Loading artwork worker stopped");
                    self.pending = None;
                }
            }
        }
        if self.pending.is_none() {
            self.first_paint.get_or_insert_with(Instant::now);
        }
        let elapsed = self.first_paint.unwrap_or(self.opened).elapsed();

        let viewport = ctx.content_rect();
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("em1_loading_screen"));
        ctx.move_to_top(layer);
        let painter = ctx.layer_painter(layer).with_clip_rect(viewport);
        let size = self.background.as_ref().map_or(Vec2::new(640., 480.), TextureHandle::size_vec2);
        let scale = (viewport.width() / size.x).min(viewport.height() / size.y);
        let image = Rect::from_center_size(viewport.center(), size * scale);
        let uv = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.));
        painter.rect_filled(viewport, 0., Color32::BLACK);
        if let Some(background) = &self.background {
            painter.image(background.id(), image, uv, Color32::WHITE);
        }
        if !self.dots.is_empty() {
            let frame = (elapsed.as_millis() / DOT_FRAME.as_millis()) as usize;
            let dot = &self.dots[frame % self.dots.len()];
            let corner = image.right_bottom() - Vec2::splat(18. * scale);
            let marker = Rect::from_min_max(corner - dot.size_vec2() * scale, corner);
            painter.image(dot.id(), marker, uv, Color32::WHITE);
        } else {
            let radius = 10.;
            let start = elapsed.as_secs_f32() * std::f32::consts::TAU;
            let points = (0..=24).map(|segment| {
                let angle = start + (segment as f32 / 24.) * std::f32::consts::PI * 1.5;
                viewport.center() + Vec2::angled(angle) * radius
            }).collect();
            painter.add(egui::Shape::line(points, egui::Stroke::new(2., Color32::WHITE)));
        }
        let repaint = if self.dots.is_empty() { Duration::from_millis(16) } else { DOT_FRAME };
        ctx.request_repaint_after(repaint);
    }
}

type Texture = (String, ColorImage);

#[derive(Default)]
struct Artwork {
    background: Option<Texture>,
    dots: Vec<Texture>,
}

impl Artwork {
    fn load(root: &Path, destination: Option<&str>) -> Self {
        let mut store = match super::resource_store::Store::new(root, &[]) {
            Ok(store) => store,
            Err(error) => {
                log::warn!("Loading screens: {error:#}");
                return Self::default();
            }
        };
        let requested = destination
            .and_then(|level| super::assets::loading_artwork(root, level)
                .inspect_err(|error| log::warn!("Level loading artwork: {error:#}"))
                .ok()
                .flatten())
            .map(|path| localized_path(&path));
        let mut read = |path: &str| {
            texture(&mut store, path)
                .inspect_err(|error| log::warn!("Loading artwork {path}: {error:#}"))
                .ok()
        };
        let background = requested.as_deref().and_then(&mut read)
            .or_else(|| read(DEFAULT_ART));
        let dots = (1..=8)
            .filter_map(|index| read(&format!("LoadingScreens/LoadingDots_{index:02}.tga")))
            .collect();
        Self { background, dots }
    }
}

fn localized_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some((directory, name)) = normalized.rsplit_once('/') {
        if directory.eq_ignore_ascii_case("LoadingScreens")
            && ["GuardianSplashScreen.dds", "MickeySplashScreen.dds"]
                .iter()
                .any(|splash| splash.eq_ignore_ascii_case(name))
        {
            return format!("{directory}/Localize/English_NTSC/{name}");
        }
    }
    normalized
}

fn texture(
    store: &mut super::resource_store::Store,
    path: &str,
) -> anyhow::Result<Texture> {
    use anyhow::Context as _;
    let bytes = store.read(path)?.data;
    let format = match path.rsplit_once('.') {
        Some((_, extension)) if extension.eq_ignore_ascii_case("tga") => image::ImageFormat::Tga,
        _ => image::ImageFormat::Dds,
    };
    let image = image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
        .decode()
        .with_context(|| format!("Decode {path}"))?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok((path.to_owned(), ColorImage::from_rgba_unmultiplied(size, &image)))
}
