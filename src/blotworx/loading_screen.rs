use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use std::{path::Path, time::{Duration, Instant}};

pub(super) struct Screen {
    background: TextureHandle,
    dots: Vec<TextureHandle>,
    started: Instant,
}

impl Screen {
    pub(super) fn load(ctx: &egui::Context, root: &Path, initial: bool) -> anyhow::Result<Self> {
        let directory = root.join("LoadingScreens");
        let filename = if initial {
            directory.join("MickeyInitialScreen.dds")
        } else {
            directory.join("Localize/English_NTSC/MickeySplashScreen.dds")
        };
        let background = texture(ctx, &filename)?;
        let mut dots = Vec::with_capacity(8);
        for index in 1..=8 {
            dots.push(texture(ctx, &directory.join(format!("LoadingDots_{index:02}.tga")))?);
        }
        Ok(Self { background, dots, started: Instant::now() })
    }

    pub(super) fn paint(&self, ui: &mut Ui) {
        let viewport = ui.max_rect();
        let size = self.background.size_vec2();
        let scale = (viewport.width() / size.x).min(viewport.height() / size.y);
        let image = Rect::from_center_size(viewport.center(), size * scale);
        let uv = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.));
        let painter = ui.painter();
        painter.rect_filled(viewport, 0., Color32::BLACK);
        painter.image(self.background.id(), image, uv, Color32::WHITE);
        let frame_time = Duration::from_millis(110);
        let frame = (self.started.elapsed().as_millis() / frame_time.as_millis()) as usize;
        let dot = &self.dots[frame % self.dots.len()];
        let corner = image.right_bottom() - Vec2::splat(18. * scale);
        let marker = Rect::from_min_max(corner - dot.size_vec2() * scale, corner);
        painter.image(dot.id(), marker, uv, Color32::WHITE);
        ui.ctx().request_repaint_after(frame_time);
    }
}

fn texture(ctx: &egui::Context, path: &Path) -> anyhow::Result<TextureHandle> {
    use anyhow::Context as _;
    let image = image::ImageReader::open(path)
        .with_context(|| format!("Open {}", path.display()))?
        .decode()
        .with_context(|| format!("Decode {}", path.display()))?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(ctx.load_texture(path.to_string_lossy(), ColorImage::from_rgba_unmultiplied(size, &image),
        TextureOptions::LINEAR))
}
