use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const DEFAULT_ART: &str = "LoadingScreens/MickeyInitialScreen.dds";
const MINIMUM_PRESENTATION: Duration = Duration::from_millis(330);
const DOT_FRAME: Duration = Duration::from_millis(110);

pub(super) struct Screen {
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
        let mut screen = Self {
            background: None,
            dots: Vec::new(),
            first_paint: None,
        };
        let mut store = match super::resource_store::Store::new(root, &[]) {
            Ok(store) => store,
            Err(error) => {
                log::warn!("Loading screens: {error:#}");
                return screen;
            }
        };
        let requested = destination
            .and_then(|level| super::assets::loading_artwork(root, level)
                .inspect_err(|error| log::warn!("Level loading artwork: {error:#}"))
                .ok()
                .flatten());
        let background = requested.and_then(|art| {
            let path = localized_path(&art);
            texture(ctx, &mut store, &path)
                .inspect_err(|error| log::warn!("Loading artwork {path}: {error:#}"))
                .ok()
        });
        screen.background = background.or_else(|| {
            texture(ctx, &mut store, DEFAULT_ART)
                .inspect_err(|error| log::warn!("Loading artwork {DEFAULT_ART}: {error:#}"))
                .ok()
        });
        for index in 1..=8 {
            match texture(
                ctx,
                &mut store,
                &format!("LoadingScreens/LoadingDots_{index:02}.tga"),
            ) {
                Ok(frame) => screen.dots.push(frame),
                Err(error) => log::warn!("Loading indicator frame {index}: {error:#}"),
            }
        }
        screen
    }

    pub(super) fn ready_to_dismiss(&self) -> bool {
        self.first_paint
            .is_some_and(|start| start.elapsed() >= MINIMUM_PRESENTATION)
    }

    pub(super) fn paint(&mut self, ui: &mut Ui) {
        let elapsed = self.first_paint.get_or_insert_with(Instant::now).elapsed();
        let ctx = ui.ctx();
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
    ctx: &egui::Context,
    store: &mut super::resource_store::Store,
    path: &str,
) -> anyhow::Result<TextureHandle> {
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
    Ok(ctx.load_texture(path, ColorImage::from_rgba_unmultiplied(size, &image),
        TextureOptions::LINEAR))
}
