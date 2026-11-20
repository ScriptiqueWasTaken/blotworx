use super::*;
use crate::gfx::{GfxFile, GfxTag};
use ruffle_render::bitmap::{Bitmap, BitmapFormat, PixelRegion};

pub(super) const PLACES: [u16; 6] = [111, 113, 115, 117, 119, 121];
const SIZE: u32 = 128;
pub(super) const LOCKED: &str = "HUDElements/Flash/cooked/map_placeholder.tga.dds";

pub(super) fn prepare(movie: &mut GfxFile) -> Result<()> {
    let image = image::RgbaImage::new(SIZE, SIZE);
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    let mut found = Vec::new();
    for tag in &mut movie.tags {
        if tag.code != 2 || tag.body.len() < 2 {
            continue;
        }
        let id = u16::from_le_bytes([tag.body[0], tag.body[1]]);
        if !PLACES.contains(&(id.saturating_sub(1))) {
            continue;
        }
        let mut shape = swf::read::Reader::new(&tag.body, movie.version).read_define_shape(1)?;
        let mut changed = false;
        for fill in &mut shape.styles.fill_styles {
            if let swf::FillStyle::Bitmap {
                id: texture,
                matrix,
                ..
            } = fill
            {
                if *texture != id - 1 {
                    continue;
                }
                // The authored proxy maps a 4Ã—4 image into each slot. Keep
                // its position and fit a full-resolution replacement inside it.
                let scale = |axis: swf::Fixed16| {
                    swf::Fixed16::from_f64(axis.to_f64() * 4. / f64::from(SIZE))
                };
                matrix.a = scale(matrix.a);
                matrix.b = scale(matrix.b);
                matrix.c = scale(matrix.c);
                matrix.d = scale(matrix.d);
                changed = true;
            }
        }
        ensure!(changed, "Missing Extras proxy bitmap for shape {id}");
        let header = swf::Header {
            compression: swf::Compression::None,
            version: movie.version,
            stage_size: Default::default(),
            frame_rate: swf::Fixed8::ONE,
            num_frames: 1,
        };
        let mut encoded = Vec::new();
        swf::write_swf(
            &header,
            &[swf::Tag::DefineShape(Box::new(shape))],
            &mut encoded,
        )?;
        let mut shape = GfxFile::load(&encoded)?;
        *tag = shape.tags.remove(0);
        found.push(id - 1);
    }
    ensure!(
        PLACES.iter().all(|id| found.contains(id)),
        "Missing authored Extras thumbnail slots"
    );
    movie.tags.splice(
        0..0,
        PLACES.into_iter().map(|id| {
            let mut body = id.to_le_bytes().to_vec();
            body.extend_from_slice(png.get_ref());
            GfxTag {
                code: 21,
                body,
                long_length: true,
            }
        }),
    );
    Ok(())
}

pub(super) fn read_texture(
    store: &mut super::super::super::resource_store::Store,
    path: &str,
) -> Result<Vec<u8>> {
    let file = store.read(path)?;
    let (pixels, width, height) = if path.ends_with(".dds") {
        let dds = crate::dds::DdsFile::load(&file.data)?;
        (dds.rgba, dds.width, dds.height)
    } else {
        crate::render_support::nif::decode_image_bytes(&file.data)
            .with_context(|| format!("Decode Extras image {path}"))?
    };
    let image = image::RgbaImage::from_raw(width, height, pixels)
        .with_context(|| format!("Invalid Extras image dimensions for {path}"))?;
    let mut rgba =
        image::imageops::resize(&image, SIZE, SIZE, image::imageops::FilterType::Triangle)
            .into_raw();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        for color in &mut pixel[..3] {
            *color = ((u16::from(*color) * alpha + 127) / 255) as u8;
        }
    }
    Ok(rgba)
}

pub(super) fn publish(player: &mut ruffle_core::Player, texture: u16, rgba: &[u8]) -> Result<()> {
    let bitmap = Bitmap::new(SIZE, SIZE, BitmapFormat::Rgba, rgba);
    let updated = player
        .update_root_bitmap(texture, bitmap, PixelRegion::for_whole_size(SIZE, SIZE))
        .map_err(|error| anyhow::anyhow!("Update Extras thumbnail {texture}: {error}"))?;
    ensure!(updated, "Extras thumbnail slot {texture} is unavailable");
    Ok(())
}
