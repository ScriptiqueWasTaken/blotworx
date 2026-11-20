use super::*;
use crate::gfx::{GfxFile, GfxTag};
use ruffle_render::bitmap::{Bitmap, BitmapFormat, PixelRegion};

pub(super) const PLACES: [u16; 6] = [111, 113, 115, 117, 119, 121];
pub(super) const THUMBNAIL_SIZE: u32 = 128;
pub(super) const CONCEPT_ART: u16 = 139;
pub(super) const CONCEPT_SIZE: u32 = 1024;
pub(super) const LOCKED: &str = "HUDElements/Flash/cooked/map_placeholder.tga.dds";

const CONCEPT_VIEWER: u16 = 144;
const CONCEPT_DRAW: u16 = 0x7ffe;
const CANCEL_ICON: u16 = 140;
const CANCEL_TEXTURE: &str = "HUDElements/Flash/cooked/general_icon_button_b.tga.dds";

fn encode_tag(version: u8, tag: swf::Tag<'_>) -> Result<GfxTag> {
    let header = swf::Header {
        compression: swf::Compression::None,
        version,
        stage_size: Default::default(),
        frame_rate: swf::Fixed8::ONE,
        num_frames: 1,
    };
    let mut encoded = Vec::new();
    swf::write_swf(&header, &[tag], &mut encoded)?;
    GfxFile::load(&encoded)?
        .tags
        .into_iter()
        .next()
        .context("Missing encoded Extras tag")
}

fn scale_bitmap(matrix: &mut swf::Matrix, size: u32) {
    let scale = |axis: swf::Fixed16| swf::Fixed16::from_f64(axis.to_f64() * 4. / f64::from(size));
    matrix.a = scale(matrix.a);
    matrix.b = scale(matrix.b);
    matrix.c = scale(matrix.c);
    matrix.d = scale(matrix.d);
}

fn png(image: image::RgbaImage) -> Result<Vec<u8>> {
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn define_bitmap(id: u16, png: &[u8]) -> GfxTag {
    let mut body = id.to_le_bytes().to_vec();
    body.extend_from_slice(png);
    GfxTag {
        code: 21,
        body,
        long_length: true,
    }
}

pub(super) fn prepare(
    movie: &mut GfxFile,
    store: &mut super::super::super::resource_store::Store,
) -> Result<()> {
    let mut found = Vec::new();
    let mut art_shape = None;
    let mut viewer_found = false;
    for tag in &mut movie.tags {
        let Some(id) = tag.body.get(..2) else {
            continue;
        };
        let id = u16::from_le_bytes(id.try_into().unwrap());
        if tag.code == 2 && PLACES.contains(&id.saturating_sub(1)) {
            let mut shape =
                swf::read::Reader::new(&tag.body, movie.version).read_define_shape(1)?;
            if id == PLACES[0] + 1 {
                let mut rectangle = shape.clone();
                rectangle.id = CONCEPT_DRAW;
                for fill in &mut rectangle.styles.fill_styles {
                    if let swf::FillStyle::Bitmap { id, matrix, .. } = fill {
                        if *id == PLACES[0] {
                            *id = CONCEPT_ART;
                            scale_bitmap(matrix, CONCEPT_SIZE);
                        }
                    }
                }
                art_shape = Some(encode_tag(
                    movie.version,
                    swf::Tag::DefineShape(Box::new(rectangle)),
                )?);
            }
            for fill in &mut shape.styles.fill_styles {
                if let swf::FillStyle::Bitmap {
                    id: texture,
                    matrix,
                    ..
                } = fill
                {
                    if *texture == id - 1 {
                        scale_bitmap(matrix, THUMBNAIL_SIZE);
                        found.push(*texture);
                    }
                }
            }
            *tag = encode_tag(movie.version, swf::Tag::DefineShape(Box::new(shape)))?;
        } else if tag.code == 39 && id == CONCEPT_VIEWER {
            let mut sprite =
                swf::read::Reader::new(&tag.body, movie.version).read_define_sprite()?;
            for child in &mut sprite.tags {
                if let swf::Tag::PlaceObject(placement) = child {
                    if placement.action == swf::PlaceObjectAction::Place(CONCEPT_ART) {
                        // Scaleform resolves the direct 4x4 proxy as a host
                        // image. Use a shared bitmap fill so Ruffle observes
                        // subsequent texture updates. Move the centered quad
                        // to the direct bitmap's top-left origin.
                        let center = swf::Twips::from_pixels(2.);
                        *placement
                            .matrix
                            .as_mut()
                            .context("Missing concept viewer transform")? *=
                            swf::Matrix::translate(center, center);
                        placement.action = swf::PlaceObjectAction::Place(CONCEPT_DRAW);
                        viewer_found = true;
                    }
                }
            }
            *tag = encode_tag(movie.version, swf::Tag::DefineSprite(sprite))?;
        }
    }
    ensure!(viewer_found, "Missing authored Concept Art viewer");
    ensure!(
        PLACES.iter().all(|id| found.contains(id)),
        "Missing authored Extras thumbnails"
    );
    let thumb = png(image::RgbaImage::new(THUMBNAIL_SIZE, THUMBNAIL_SIZE))?;
    let art = png(image::RgbaImage::new(CONCEPT_SIZE, CONCEPT_SIZE))?;
    let cancel = crate::dds::DdsFile::load(&store.read(CANCEL_TEXTURE)?.data)?;
    let cancel = image::RgbaImage::from_raw(cancel.width, cancel.height, cancel.rgba)
        .context("Invalid authored Concept Art Back icon")?;
    ensure!(
        cancel.dimensions() == (32, 32),
        "Unexpected Concept Art Back icon size"
    );
    let cancel = png(cancel)?;
    movie.tags.splice(
        0..0,
        PLACES
            .into_iter()
            .map(|id| define_bitmap(id, &thumb))
            .chain([
                define_bitmap(CONCEPT_ART, &art),
                define_bitmap(CANCEL_ICON, &cancel),
                art_shape.context("Missing authored Concept Art rectangle")?,
            ]),
    );
    Ok(())
}

pub(super) fn read_texture(
    store: &mut super::super::super::resource_store::Store,
    path: &str,
    size: u32,
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
    let mut rgba = if image.dimensions() == (size, size) {
        image.into_raw()
    } else {
        image::imageops::resize(&image, size, size, image::imageops::FilterType::Triangle)
            .into_raw()
    };
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        for color in &mut pixel[..3] {
            *color = ((u16::from(*color) * alpha + 127) / 255) as u8;
        }
    }
    Ok(rgba)
}

pub(super) fn publish(player: &mut ruffle_core::Player, texture: u16, rgba: &[u8]) -> Result<()> {
    let size = if texture == CONCEPT_ART {
        CONCEPT_SIZE
    } else {
        THUMBNAIL_SIZE
    };
    let bitmap = Bitmap::new(size, size, BitmapFormat::Rgba, rgba);
    let updated = player
        .update_root_bitmap(texture, bitmap, PixelRegion::for_whole_size(size, size))
        .map_err(|error| anyhow::anyhow!("Update Extras bitmap {texture}: {error}"))?;
    ensure!(updated, "Extras bitmap slot {texture} is unavailable");
    Ok(())
}
