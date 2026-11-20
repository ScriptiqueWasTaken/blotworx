//! Scaleform external images bridged to standard SWF bitmaps in memory.
//! Native filename selection: DOL80076950/800765a0/800cf6c8;
//! Wii texture requests: DOL80526f3c, REL20a414. Export labels are NOT paths.
use super::resource_store::Store;
use crate::gfx::{GfxFile, GfxTag};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, io::Cursor};

#[derive(Debug, PartialEq, Eq)]
struct External<'a> {
    id: u32,
    format: u16,
    width: u16,
    height: u16,
    export: &'a str,
    filename: &'a str,
}
fn external(body: &[u8]) -> Result<External<'_>> {
    ensure!(body.len() >= 12, "Truncated DefineExternalImage2");
    let mut at = 10;
    fn string<'a>(body: &'a [u8], at: &mut usize) -> Result<&'a str> {
        let len = *body
            .get(*at)
            .context("Missing external-image string length")? as usize;
        *at += 1;
        let value = body
            .get(*at..*at + len)
            .context("Truncated external-image string")?;
        *at += len;
        Ok(std::str::from_utf8(value).context("External-image name is not UTF-8")?)
    }
    let export = string(body, &mut at)?;
    let filename = string(body, &mut at)?;
    ensure!(
        at == body.len(),
        "Unrecognized DefineExternalImage2 trailing data"
    );
    Ok(External {
        id: u32::from_le_bytes(body[..4].try_into()?),
        format: u16::from_le_bytes(body[4..6].try_into()?),
        width: u16::from_le_bytes(body[6..8].try_into()?),
        height: u16::from_le_bytes(body[8..10].try_into()?),
        export,
        filename,
    })
}
fn source_path(movie: &str, name: &str) -> Result<String> {
    // This provider admits the local-file URL case only. Protocol images and
    // mod localization rewriting require their native services, not guessing.
    ensure!(
        !name.contains(':') && !name.is_empty(),
        "Unsupported external-image URL: {name}"
    );
    let name = name.replace('\\', "/");
    let path = if name.starts_with('/') {
        name.trim_start_matches('/').to_owned()
    } else if let Some((parent, _)) = movie.rsplit_once('/') {
        format!("{parent}/{name}")
    } else {
        name
    };
    let path = super::resource_path::canonical(&path)?;
    // Native8052700c uses strrchr on the entire URL, then writes the Wii
    // suffix in the same1024-byte buffer. Do not search the export label.
    ensure!(
        path.len() < 1024,
        "Native external-image path exceeds its buffer"
    );
    Ok(path)
}
fn requests(movie: &str, name: &str) -> Result<[String; 2]> {
    let path = source_path(movie, name)?;
    let dot = path
        .rfind('.')
        .context("External-image request has no extension")?;
    let stem = &path[..dot];
    ensure!(
        stem.len() + 12 < 1024,
        "Native external-image suffix exceeds its buffer"
    );
    Ok([format!("{stem}_tex.nif_wii"), format!("{stem}_tex.nif")])
}

fn localize_pause_text(movie: &mut GfxFile, locale: &epic_mickey_lib_rs::dct::DCT) {
    for tag in movie.tags.iter_mut().filter(|tag| tag.code == 37) {
        let mut reader = swf::read::Reader::new(&tag.body, movie.version);
        let Ok(text) = reader.read_define_edit_text() else { continue };
        let Some(initial) = text.initial_text() else { continue };
        let Ok(initial) = std::str::from_utf8(initial.as_bytes()) else { continue };
        let Some(begin) = initial.find("UI_") else { continue };
        let end = begin + initial[begin..].bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_').count();
        let Some(translation) = pause_translation(locale, &initial[begin..end]) else { continue };
        let escaped = if text.is_html() {
            translation.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        } else { translation.to_owned() };
        let replacement = format!("{}{escaped}{}", &initial[..begin], &initial[end..]);
        let Some(offset) = tag.body.len().checked_sub(initial.len() + 1) else { continue };
        if !tag.body[offset..].starts_with(initial.as_bytes()) { continue; }
        tag.body.truncate(offset);
        tag.body.extend(replacement.bytes().filter(|&byte| byte != 0));
        tag.body.push(0);
    }
}
fn pause_translation<'a>(locale: &'a epic_mickey_lib_rs::dct::DCT, key: &str) -> Option<&'a str> {
    let hash = epic_mickey_lib_rs::dct::epic_mickey_hash(key.as_bytes(), locale.hash_seed);
    locale.dialog_entries.iter().find(|entry| entry.hashed_key == hash).map(|entry| entry.text.as_str())
}

/// The Wii swaps the texture behind the exported Placeholder_Map symbol.
/// Keep the movie's placement, animation, legend and 4Ã—4 registration area.
fn adapt_pause_map(movie: &mut GfxFile, store: &mut Store, key: &str) -> Result<Option<String>> {
    use swf::{FillStyle, Fixed16, Matrix, Point, PointDelta, Rectangle, Shape, ShapeFlag,
        ShapeRecord, ShapeStyles, StyleChangeData, Tag, Twips};

    if !key.starts_with("MAP_") || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Ok(None);
    }
    let mut tags = movie.tags.iter();
    let import = tags.position(|tag| tag.code == 71 && tag.body.ends_with(b"Placeholder_Map\0"))
        .context("Pause map movie does not import Placeholder_Map")?;
    let bytes = &movie.tags[import].body;
    let start = bytes.iter().position(|&b| b == 0).context("Map import URL is truncated")? + 1;
    ensure!(bytes.get(start..start + 4) == Some(&[1, 0, 1, 0]), "Unexpected map import table");
    let sprite = u16::from_le_bytes(bytes[start + 4..start + 6].try_into()?);
    let file = store.read(&format!("HUDElements/Maps/{key}_tex.nif_wii"))?;
    let (rgba, width, height) = crate::render_support::nif::decode_image_bytes(&file.data)
        .context("Cannot decode pause map texture")?;
    let image = image::RgbaImage::from_raw(width, height, rgba).context("Invalid map RGBA")?;
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    let mut ids = (1..u16::MAX).rev().filter(|id| {
        *id != sprite && !movie.tags.iter().any(|tag| tag.body.starts_with(&id.to_le_bytes()))
    });
    let bitmap = ids.next().context("Missing spare map bitmap character ID")?;
    let shape = ids.next().context("Missing spare map shape character ID")?;
    let extent = Twips::new(80);
    let bounds = Rectangle { x_min: Twips::ZERO, x_max: extent, y_min: Twips::ZERO, y_max: extent };
    let pixels = extent.get() as f32;
    let style = FillStyle::Bitmap {
        id: bitmap,
        matrix: Matrix::scale(Fixed16::from_f32(pixels / width as f32), Fixed16::from_f32(pixels / height as f32)),
        is_smoothed: true,
        is_repeating: false,
    };
    let edges = [(extent, Twips::ZERO), (Twips::ZERO, extent), (-extent, Twips::ZERO), (Twips::ZERO, -extent)];
    let records = std::iter::once(ShapeRecord::StyleChange(Box::new(StyleChangeData {
        move_to: Some(Point::new(Twips::ZERO, Twips::ZERO)), fill_style_0: None,
        fill_style_1: Some(1), line_style: None, new_styles: None,
    }))).chain(edges.map(|(x, y)| ShapeRecord::StraightEdge { delta: PointDelta::new(x, y) })).collect();
    let mut swf = Vec::new();
    swf::write::write_swf(&swf::Header::default_with_swf_version(movie.version), &[
        Tag::DefineBitsJpeg2 { id: bitmap, jpeg_data: png.get_ref() },
        Tag::DefineShape(Box::new(Shape {
            version: 2, id: shape, shape_bounds: bounds, edge_bounds: bounds, flags: ShapeFlag::empty(),
            styles: ShapeStyles { fill_styles: vec![style], line_styles: Vec::new() }, shape: records,
        })),
    ], &mut swf)?;
    let resources = GfxFile::load(&swf)?;
    // DefineSprite with one PlaceObject2 at depth 1 and the identity matrix.
    let mut clip = sprite.to_le_bytes().to_vec();
    clip.extend([1, 0, 0x86, 6, 6, 1, 0]);
    clip.extend(shape.to_le_bytes());
    clip.extend([0, 0x40, 0, 0, 0]);
    movie.tags.splice(import..=import, resources.tags.into_iter().filter(|t| t.code != 0)
        .chain(std::iter::once(GfxTag { code: 39, body: clip, long_length: true })));
    Ok(Some(file.path))
}

#[derive(Default)]
pub(super) struct Images {
    // Converted PNG bytes are shared by repeated imports; no extracted copies
    // are written to disk. Failed lookups remain separate diagnostic entries.
    encoded: BTreeMap<String, (Vec<u8>, u32, u32, String)>,
    pub observations: Vec<serde_json::Value>,
    pub inspect_unverified_blends: bool,
    pause_locale: Option<epic_mickey_lib_rs::dct::DCT>,
    pub(super) pause_map: Option<super::map_info::State>,
}
impl Images {
    pub(super) fn pause_label(&self, key: &str) -> Option<&str> {
        self.pause_locale.as_ref().and_then(|locale| pause_translation(locale, key))
    }
    pub fn adapt(&mut self, movie: &mut GfxFile, path: &str, store: &mut Store) -> Result<()> {
        let movie_name = path.rsplit('/').next().map(str::to_ascii_lowercase);
        if movie_name.as_deref().is_some_and(|name| name.starts_with("pausemenu_") || matches!(name, "options.gfx" | "popup_item_acquire.gfx")) {
            if self.pause_locale.is_none() && store.admit_packs(&["English_NTSC.pak".into()]).is_ok() {
                if let Ok(file) = store.read("Localize/ENGLISH_NTSC_RVL.dct") {
                    self.pause_locale = Some(epic_mickey_lib_rs::dct::DCT::from_binary(file.data));
                }
            }
            if let Some(locale) = &self.pause_locale {
                localize_pause_text(movie, locale);
            }
        }
        if movie_name.as_deref() == Some("pausemenu_map.gfx") {
            if let Some(map) = &self.pause_map {
                match adapt_pause_map(movie, store, &map.map) {
                    Ok(Some(texture)) => self.observations.push(serde_json::json!({"movie":path,"map":map.map,"texture":texture})),
                    Ok(None) => {},
                    Err(error) => self.observations.push(serde_json::json!({"movie":path,"map":map.map,"error":format!("{error:#}")})),
                }
            }
        }
        for tag in &mut movie.tags {
            if tag.code != 1009 {
                continue;
            }
            let image = external(&tag.body)?;
            let mut observation = serde_json::json!({"movie":path,"id":image.id,"format":image.format,
                "authored_size":[image.width,image.height],"export":image.export,"filename":image.filename});
            let converted = (|| -> Result<Vec<u8>> {
                ensure!(
                    image.id <= u16::MAX as u32,
                    "External-image namespace exceeds SWF CharacterId: {:#x}",
                    image.id
                );
                ensure!(
                    image.format == 14,
                    "Unvalidated external-image format {}",
                    image.format
                );
                let mut sources = requests(path, image.filename)?.into_iter()
                    .map(|path| (path, false)).collect::<Vec<_>>();
                if movie_name.as_deref() == Some("skipmovie.gfx")
                    && image.filename.eq_ignore_ascii_case("HUD_Timer_HalfMoon.dds")
                {
                    sources.insert(0, ("HUDElements/icons/hud_timer_halfmoon_walpha_tex.nif_wii".into(), false));
                }
                // These dialogue images contain authored artwork and have no
                // corresponding texture NIF in the Wii asset set. Other DDS
                // files can be export placeholders and still require a NIF.
                let loose_art: &[&str] = match movie_name.as_deref() {
                    Some("general_shared.gfx") => &[
                        "General_Paper_9_Slice.tga.dds",
                        "General_Icon_Button_A.tga.dds",
                        "General_Icon_Button_B.tga.dds",
                        "FullMoon.tga.dds",
                    ],
                    Some("dialogbox_generic.gfx") => {
                        &["DialogBox_Generic_I4.dds"]
                    }
                    Some("reticles.gfx") => &["Reticles_I1.dds", "Reticles_I3.dds", "Reticles_I7.dds"],
                    Some("options.gfx") => &[
                        "Options_Greyscale_Texture.dds",
                        "Options_I4A.dds",
                        "Options_VolumeSlider_Bkgd_Texture.dds",
                    ],
                    _ => &[],
                };
                if loose_art.iter().any(|name| image.filename.eq_ignore_ascii_case(name)) {
                    sources.push((source_path(path, image.filename)?, true));
                }
                let mut failures = Vec::new();
                for (candidate, loose_dds) in sources {
                    let key = candidate.to_ascii_lowercase();
                    if let Some((png, width, height, actual)) = self.encoded.get(&key) {
                        ensure!(
                            (*width, *height) == (image.width as u32, image.height as u32),
                            "Cached native image dimensions differ from authored size"
                        );
                        observation["texture"] = actual.clone().into();
                        observation["cached"] = true.into();
                        observation["decoded_size"] = serde_json::json!([width, height]);
                        let mut bytes = (image.id as u16).to_le_bytes().to_vec();
                        bytes.extend(png);
                        return Ok(bytes);
                    }
                    let loaded = (|| -> Result<Vec<u8>> {
                        let file = store.read(&candidate)?;
                        let (mut rgba, width, height) = if loose_dds {
                            let image = crate::dds::DdsFile::load(&file.data)
                                .with_context(|| format!("Cannot decode loose UI texture {}", file.path))?;
                            (image.rgba, image.width, image.height)
                        } else {
                            crate::render_support::nif::decode_image_bytes(&file.data)
                                .with_context(|| format!("Cannot decode native UI texture {}", file.path))?
                        };
                        if loose_dds && image.filename.eq_ignore_ascii_case("DialogBox_Generic_I4.dds")
                            && !self.inspect_unverified_blends
                        {
                            // The stain is a multiply layer with a white matte.
                            // Recover transparency for the offscreen HUD so white
                            // leaves the scene unchanged where the paper ends.
                            for pixel in rgba.chunks_exact_mut(4) {
                                let matte = *pixel[..3].iter().min().unwrap();
                                let coverage = 255 - matte;
                                if coverage != 0 {
                                    for color in &mut pixel[..3] {
                                        *color = (u16::from(*color - matte) * 255 / u16::from(coverage)) as u8;
                                    }
                                }
                                pixel[3] = coverage;
                            }
                        }
                        observation["resolution"] = if loose_dds { "authored-loose-dds" } else { "native-texture" }.into();
                        ensure!((width,height)==(image.width as u32,image.height as u32),
                            "Native image size {:?} differs from authored {:?}; resource scaling is unimplemented",(width,height),(image.width,image.height));
                        let buffer = image::RgbaImage::from_raw(width, height, rgba)
                            .context("Invalid native texture RGBA length")?;
                        let mut png = Cursor::new(Vec::new());
                        buffer.write_to(&mut png, image::ImageFormat::Png)?;
                        observation["texture"] = file.path.into();
                        observation["decoded_size"] = serde_json::json!([width, height]);
                        Ok(png.into_inner())
                    })();
                    match loaded {
                        Ok(png) => {
                            let mut bytes = (image.id as u16).to_le_bytes().to_vec();
                            bytes.extend(&png);
                            let actual = observation["texture"]
                                .as_str()
                                .context("Decoded image has no identity")?
                                .to_owned();
                            self.encoded.insert(
                                key,
                                (png, image.width as u32, image.height as u32, actual),
                            );
                            return Ok(bytes);
                        }
                        Err(error) => failures.push(format!("{candidate}: {error:#}")),
                    }
                }
                anyhow::bail!("{}", failures.join("; "))
            })();
            match converted {
                Ok(body) => {
                    // DefineBitsJPEG2 accepts PNG and preserves straight RGBA,
                    // avoiding lossy unpremultiply/re-premultiply conversions.
                    // Character identity and all shape/matrix tags stay intact.
                    *tag = GfxTag {
                        code: 21,
                        body,
                        long_length: true,
                    };
                    observation["result"] = "decoded".into();
                }
                Err(error) => {
                    observation["result"] = "unsupported".into();
                    observation["error"] = format!("{error:#}").into();
                }
            }
            self.observations.push(observation);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_image_identity_and_native_filename() -> Result<()> {
        let mut bytes = vec![3, 0, 0, 0, 14, 0, 64, 0, 8, 0];
        for s in ["UI_Mickey_Heads_PageCurl_2", "Popup_Item_Acquire_IF.dds"] {
            bytes.push(s.len() as u8);
            bytes.extend(s.as_bytes());
        }
        let value = external(&bytes)?;
        assert_eq!(value.id, 3);
        assert_eq!(value.export, "UI_Mickey_Heads_PageCurl_2");
        assert_eq!(
            requests("HUDElements/Flash/Cooked/StatusBar.gfx", value.filename)?,
            [
                "HUDElements/Flash/Cooked/Popup_Item_Acquire_IF_tex.nif_wii",
                "HUDElements/Flash/Cooked/Popup_Item_Acquire_IF_tex.nif"
            ]
        );
        for length in 0..bytes.len() {
            assert!(external(&bytes[..length]).is_err());
        }
        bytes.push(0);
        assert!(external(&bytes).is_err());
        assert_eq!(
            requests("a.gfx", "Reticles_I1.dds")?,
            ["Reticles_I1_tex.nif_wii", "Reticles_I1_tex.nif"]
        );
        assert!(requests("a.gfx", "../../escape.dds").is_err());
        assert!(requests("a.gfx", "https://x.dds").is_err());
        Ok(())
    }
}
