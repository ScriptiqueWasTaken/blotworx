//! Bitmap screen text following Gamebryo NiScreenText's Courier16 layout.
//! This draws the imported texture; it does not substitute an installed font.
use anyhow::{ensure, Context, Result};
use std::path::Path;

pub struct Font {
    texture: egui::TextureHandle,
}
const WIDTH: f32 = 11.;
const HEIGHT: f32 = 21.;
const COLUMNS: u32 = 23;
// NiScreenText treats space/unprintable bytes as blank cells.
fn glyph(c: char) -> Option<egui::Rect> {
    let code = c as u32;
    if !(33..=122).contains(&code) {
        return None;
    }
    let index = code - 33;
    let x = (index % COLUMNS) as f32 * WIDTH;
    let y = (index / COLUMNS) as f32 * HEIGHT;
    Some(egui::Rect::from_min_max(
        egui::pos2(x / 256., y / 128.),
        egui::pos2((x + WIDTH) / 256., (y + HEIGHT) / 128.),
    ))
}
impl Font {
    pub fn load(root: &Path, ctx: &egui::Context) -> Result<Self> {
        let path = std::fs::read_dir(root)?
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("COURIER16.TGA")
            })
            .context("Missing game debug font COURIER16.TGA")?
            .path();
        let image =
            image::load_from_memory_with_format(&std::fs::read(path)?, image::ImageFormat::Tga)?
                .to_rgba8();
        ensure!(
            image.dimensions() == (256, 128),
            "Unexpected COURIER16 atlas dimensions"
        );
        let pixels = egui::ColorImage::from_rgba_unmultiplied([256, 128], image.as_raw());
        Ok(Self {
            texture: ctx.load_texture("Game COURIER16", pixels, egui::TextureOptions::NEAREST),
        })
    }
    pub fn text(
        &self,
        ui: &mut egui::Ui,
        text: impl AsRef<str>,
        color: egui::Color32,
    ) -> egui::Response {
        // Pixel dimensions stay physical, independent of the editor's UI scaling.
        let pixel = 1. / ui.ctx().pixels_per_point();
        let cell = egui::vec2(WIDTH * pixel, HEIGHT * pixel);
        let columns = (ui.available_width() / cell.x).floor().max(1.) as usize;
        let mut row = 0;
        let mut column = 0;
        let mut quads = Vec::new();
        for c in text.as_ref().chars() {
            if column >= columns || c == '\n' {
                row += 1;
                column = 0;
            }
            if c == '\n' {
                continue;
            }
            if let Some(uv) = glyph(c) {
                quads.push((column, row, uv));
            }
            column += 1;
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), (row + 1) as f32 * cell.y),
            egui::Sense::click(),
        );
        let mut mesh = egui::Mesh::with_texture(self.texture.id());
        // Snap origin to physical pixels to avoid blurred or uneven glyph edges.
        let origin = (rect.min.to_vec2() / pixel).round() * pixel;
        for (x, y, uv) in quads {
            let pos = egui::pos2(origin.x + x as f32 * cell.x, origin.y + y as f32 * cell.y);
            mesh.add_rect_with_uv(egui::Rect::from_min_size(pos, cell), uv, color);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        response
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn courier_cells_match_screen_text_ascii_mapping() {
        assert_eq!(glyph('!').unwrap().min, egui::pos2(0., 0.));
        assert_eq!(glyph('8').unwrap().min, egui::pos2(0., 21. / 128.));
        assert_eq!(glyph('z').unwrap().max, egui::pos2(231. / 256., 84. / 128.));
        assert!(glyph(' ').is_none());
        assert!(glyph('\n').is_none());
        assert!(glyph('é').is_none());
    }
}
