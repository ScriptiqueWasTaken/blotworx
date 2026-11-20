use anyhow::{Context, Result};
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};
use std::path::Path;

pub(super) fn load(ctx: &egui::Context, root: &Path) -> Result<TextureHandle> {
    // UsableFX's original billboard texture. Only the indexed member is read.
    let mut resources = super::resource_store::Store::new(
        root, &["2d_castaway01_shared.pak".to_owned()],
    )?;
    let file = resources.read("Effects/_Shared/textures/overhead_button_a_tex.nif")?;
    let (rgba, width, height) = crate::render_support::nif::decode_image_bytes(&file.data)
        .context("Decode the authored overhead A-button texture")?;
    let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
    Ok(ctx.load_texture("usable-a-button", image, TextureOptions::LINEAR))
}

pub(super) fn draw(
    ui: &Ui, viewport: Rect, view_projection: &[f32; 16], button: &TextureHandle,
    owner: &super::assets::RuntimeEntity,
) -> bool {
    let offset = owner.components.iter().find(|c| c.class_name == "UsableComponent")
        .and_then(|c| c.properties.iter().find(|p| p.name == "UsableEffectOffset"))
        .and_then(|p| crate::render_support::scene::parse_point3(&p.value))
        .unwrap_or([0., 2., 0.25]);
    let point: [f32; 3] = std::array::from_fn(|axis| owner.transform[12 + axis] + offset[axis]);
    let clip: [f32; 4] = std::array::from_fn(|row| {
        view_projection[row] * point[0] + view_projection[4 + row] * point[1]
            + view_projection[8 + row] * point[2] + view_projection[12 + row]
    });
    if !clip.iter().all(|v| v.is_finite()) || clip[3] <= 0.
        || clip[..3].iter().any(|v| v.abs() > clip[3])
    {
        return false;
    }
    let center = viewport.center() + egui::vec2(
        0.5 * viewport.width() * clip[0] / clip[3],
        -0.5 * viewport.height() * clip[1] / clip[3],
    );
    let size = button.size_vec2();
    let size = size * (viewport.height() * 0.075 / size.y);
    ui.painter().with_clip_rect(viewport).image(
        button.id(), Rect::from_center_size(center, size),
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)), Color32::WHITE,
    );
    true
}
