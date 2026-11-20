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

#[derive(Default)]
pub(super) struct Images {
    // Converted PNG bytes are shared by repeated imports; no extracted copies
    // are written to disk. Failed lookups remain separate diagnostic entries.
    encoded: BTreeMap<String, (Vec<u8>, u32, u32, String)>,
    pub observations: Vec<serde_json::Value>,
}
impl Images {
    pub fn adapt(&mut self, movie: &mut GfxFile, path: &str, store: &mut Store) -> Result<()> {
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
                // DDS names in the Flash movie identify placeholders. The
                // game's image provider replaces them with native NIF textures;
                // the placeholder itself must never become rendered content.
                let mut failures = Vec::new();
                for candidate in requests(path, image.filename)? {
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
                        let (rgba, width, height) =
                            crate::render_support::nif::decode_image_bytes(&file.data).with_context(
                                || format!("Cannot decode native UI texture {}", file.path),
                            )?;
                        observation["resolution"] = "native-texture".into();
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
