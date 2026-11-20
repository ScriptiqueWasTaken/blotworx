//! Structured Scaleform GFx/CFX container support.
//!
//! The SWF tag bodies are retained verbatim, including tags this editor does
//! not understand. Known import and external-image tags expose their paths for
//! authoring without sacrificing byte-preserving round trips.
use anyhow::{ensure, Context, Result};
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GfxCompression {
    None,
    Zlib,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GfxTag {
    pub code: u16,
    pub body: Vec<u8>,
    pub long_length: bool,
}

#[derive(Clone, Debug)]
pub struct GfxFile {
    pub signature: [u8; 3],
    pub version: u8,
    pub compression: GfxCompression,
    pub prefix: Vec<u8>,
    pub tags: Vec<GfxTag>,
    pub suffix: Vec<u8>,
    original: Vec<u8>,
    original_body: Vec<u8>,
}

fn inflate(data: &[u8], compression: GfxCompression, expected: usize) -> Result<Vec<u8>> {
    if compression == GfxCompression::None {
        return Ok(data.to_vec());
    }
    let mut output = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(expected as u64 + 1)
        .read_to_end(&mut output)?;
    ensure!(
        output.len() == expected,
        "GFx decompressed body length mismatch"
    );
    Ok(output)
}

impl GfxFile {
    /// Construct a small uncompressed GFx document suitable as a new asset.
    pub fn template() -> Self {
        let mut file = Self {
            signature: *b"GFX",
            version: 9,
            compression: GfxCompression::None,
            // RECT (1-bit coordinates), 12 fps, one frame.
            prefix: vec![0x08, 0x00, 0x00, 0x0c, 0x01, 0x00],
            tags: vec![
                GfxTag {
                    code: 1,
                    body: Vec::new(),
                    long_length: false,
                },
                GfxTag {
                    code: 0,
                    body: Vec::new(),
                    long_length: false,
                },
            ],
            suffix: Vec::new(),
            original: Vec::new(),
            original_body: Vec::new(),
        };
        // Seed the original fields so the first save follows the normal round-trip path.
        let body = file.body().expect("GFx template is internally valid");
        file.original_body = body;
        file.original = {
            let mut bytes = Vec::with_capacity(file.original_body.len() + 8);
            bytes.extend(file.signature);
            bytes.push(file.version);
            bytes.extend((file.original_body.len() as u32 + 8).to_le_bytes());
            bytes.extend(&file.original_body);
            bytes
        };
        file
    }

    pub fn load(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 8, "Truncated GFx header");
        let mut signature = [0u8; 3];
        signature.copy_from_slice(&data[..3]);
        let compression = match &signature {
            b"GFX" | b"FWS" => GfxCompression::None,
            b"CFX" | b"CWS" => GfxCompression::Zlib,
            _ => anyhow::bail!("Unsupported GFx signature"),
        };
        let expected_total = u32::from_le_bytes(data[4..8].try_into()?) as usize;
        ensure!(
            (8..=64 * 1024 * 1024).contains(&expected_total),
            "Invalid GFx length"
        );
        let body = inflate(&data[8..], compression, expected_total - 8)?;
        ensure!(!body.is_empty(), "Missing GFx frame header");
        let bits = (body[0] >> 3) as usize;
        let tag_start = (5 + 4 * bits).div_ceil(8) + 4;
        ensure!(tag_start <= body.len(), "Truncated GFx frame header");
        let prefix = body[..tag_start].to_vec();
        let mut pos = tag_start;
        let mut tags = Vec::new();
        let mut suffix = Vec::new();
        while pos < body.len() {
            ensure!(pos + 2 <= body.len(), "Truncated GFx tag header");
            let raw = u16::from_le_bytes(body[pos..pos + 2].try_into()?);
            pos += 2;
            let code = raw >> 6;
            let short = (raw & 63) as usize;
            let long_length = short == 63;
            let len = if long_length {
                ensure!(pos + 4 <= body.len(), "Truncated GFx long tag length");
                let n = u32::from_le_bytes(body[pos..pos + 4].try_into()?) as usize;
                pos += 4;
                n
            } else {
                short
            };
            let end = pos.checked_add(len).context("GFx tag length overflow")?;
            let tag_body = body
                .get(pos..end)
                .context("Truncated GFx tag body")?
                .to_vec();
            pos = end;
            tags.push(GfxTag {
                code,
                body: tag_body,
                long_length,
            });
            if code == 0 {
                suffix = body[pos..].to_vec();
                break;
            }
        }
        ensure!(
            tags.iter().any(|tag| tag.code == 0),
            "GFx stream has no End tag"
        );
        Ok(Self {
            signature,
            version: data[3],
            compression,
            prefix,
            tags,
            suffix,
            original: data.to_vec(),
            original_body: body,
        })
    }

    pub fn references(&self) -> Vec<String> {
        let mut out = Vec::new();
        for tag in &self.tags {
            if tag.code == 57 || tag.code == 71 {
                if let Some(end) = tag.body.iter().position(|b| *b == 0) {
                    if let Ok(value) = std::str::from_utf8(&tag.body[..end]) {
                        if !value.is_empty() {
                            out.push(value.to_owned());
                        }
                    }
                }
            } else if tag.code == 1009 {
                let mut offset = 10;
                for index in 0..2 {
                    let Some(&len) = tag.body.get(offset) else {
                        break;
                    };
                    offset += 1;
                    let Some(bytes) = tag.body.get(offset..offset + len as usize) else {
                        break;
                    };
                    offset += len as usize;
                    if index == 1 {
                        if let Ok(value) = std::str::from_utf8(bytes) {
                            if !value.is_empty() {
                                out.push(value.to_owned());
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Number of authored timeline frames (ShowFrame tags).
    pub fn frame_count(&self) -> usize {
        self.tags.iter().filter(|tag| tag.code == 1).count()
    }

    /// Return authored external references grouped by timeline frame.
    ///
    /// GFx tags before each ShowFrame belong to that frame. Unknown tags are
    /// retained by the parser and simply contribute references when they use
    /// one of the reference-bearing encodings understood by `tag_references`.
    pub fn frame_references(&self) -> Vec<Vec<String>> {
        let mut frames = Vec::new();
        let mut current = Vec::new();
        for tag in &self.tags {
            if tag.code == 1 {
                frames.push(std::mem::take(&mut current));
            } else {
                current.extend(self.tag_references(tag));
            }
        }
        if frames.is_empty() {
            frames.push(current);
        } else if !current.is_empty() {
            frames
                .last_mut()
                .expect("frames is non-empty")
                .extend(current);
        }
        frames
    }

    pub fn frame_rate(&self) -> Option<f32> {
        let at = self.prefix.len().checked_sub(4)?;
        let raw = u16::from_le_bytes(self.prefix.get(at..at + 2)?.try_into().ok()?);
        Some(raw as f32 / 256.0)
    }

    pub fn set_frame_rate(&mut self, frames_per_second: f32) -> Result<()> {
        ensure!(
            frames_per_second.is_finite(),
            "GFx frame rate must be finite"
        );
        ensure!(
            (0.0..=255.996).contains(&frames_per_second),
            "GFx frame rate must be between 0 and 255.996 FPS"
        );
        let at = self
            .prefix
            .len()
            .checked_sub(4)
            .context("GFx frame header has no frame-rate field")?;
        let raw = (frames_per_second * 256.0).round() as u16;
        self.prefix[at..at + 2].copy_from_slice(&raw.to_le_bytes());
        Ok(())
    }

    /// Append an empty timeline frame before the End tag.
    pub fn add_frame(&mut self) {
        let end = self
            .tags
            .iter()
            .position(|tag| tag.code == 0)
            .unwrap_or(self.tags.len());
        self.tags.insert(
            end,
            GfxTag {
                code: 1,
                body: Vec::new(),
                long_length: false,
            },
        );
    }

    /// Remove the last timeline frame, retaining one frame for a valid movie.
    pub fn remove_last_frame(&mut self) -> bool {
        if self.frame_count() <= 1 {
            return false;
        }
        if let Some(index) = self.tags.iter().rposition(|tag| tag.code == 1) {
            self.tags.remove(index);
            return true;
        }
        false
    }

    pub fn tag_references(&self, tag: &GfxTag) -> Vec<String> {
        let mut out = Vec::new();
        if tag.code == 57 || tag.code == 71 {
            if let Some(end) = tag.body.iter().position(|b| *b == 0) {
                if let Ok(value) = std::str::from_utf8(&tag.body[..end]) {
                    if !value.is_empty() {
                        out.push(value.to_owned());
                    }
                }
            }
        } else if tag.code == 1009 {
            let mut offset = 10;
            for index in 0..2 {
                let Some(&len) = tag.body.get(offset) else {
                    break;
                };
                offset += 1;
                let Some(bytes) = tag.body.get(offset..offset + len as usize) else {
                    break;
                };
                offset += len as usize;
                if index == 1 {
                    if let Ok(value) = std::str::from_utf8(bytes) {
                        if !value.is_empty() {
                            out.push(value.to_owned());
                        }
                    }
                }
            }
        }
        out
    }

    pub fn tag_body_hex(&self, index: usize) -> Result<String> {
        let tag = self
            .tags
            .get(index)
            .context("GFx tag index is out of range")?;
        Ok(tag
            .body
            .chunks(16)
            .map(|row| {
                row.iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn replace_tag_body_hex(&mut self, index: usize, text: &str) -> Result<()> {
        let clean: String = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        ensure!(
            clean.len() % 2 == 0,
            "GFx tag data has an odd number of hexadecimal digits"
        );
        ensure!(
            clean.chars().all(|character| character.is_ascii_hexdigit()),
            "GFx tag data contains a non-hex character"
        );
        let bytes = (0..clean.len())
            .step_by(2)
            .map(|offset| Ok(u8::from_str_radix(&clean[offset..offset + 2], 16)?))
            .collect::<Result<Vec<_>>>()?;
        let tag = self
            .tags
            .get_mut(index)
            .context("GFx tag index is out of range")?;
        tag.body = bytes;
        Ok(())
    }

    pub fn replace_reference(&mut self, old: &str, new: &str) -> usize {
        let mut changed = 0;
        for tag in &mut self.tags {
            if (tag.code == 57 || tag.code == 71)
                && tag
                    .body
                    .split(|b| *b == 0)
                    .next()
                    .and_then(|v| std::str::from_utf8(v).ok())
                    == Some(old)
            {
                let suffix = tag
                    .body
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(tag.body.len());
                let mut body = new.as_bytes().to_vec();
                body.extend_from_slice(&tag.body[suffix..]);
                tag.body = body;
                changed += 1;
            } else if tag.code == 1009 {
                let mut offset = 10;
                for index in 0..2 {
                    let Some(&len) = tag.body.get(offset) else {
                        break;
                    };
                    let start = offset + 1;
                    let end = start + len as usize;
                    if end > tag.body.len() {
                        break;
                    }
                    offset = end;
                    if index == 1
                        && tag.body[start..end] == *old.as_bytes()
                        && new.len() <= 255
                        && !new.as_bytes().contains(&0)
                    {
                        tag.body.splice(
                            start - 1..end,
                            std::iter::once(new.len() as u8).chain(new.bytes()),
                        );
                        changed += 1;
                        break;
                    }
                }
            }
        }
        changed
    }

    fn body(&self) -> Result<Vec<u8>> {
        let mut body = self.prefix.clone();
        for tag in &self.tags {
            ensure!(tag.code <= 1023, "GFx tag code out of range");
            ensure!(tag.body.len() <= u32::MAX as usize, "GFx tag too large");
            let short = tag.body.len() < 63 && !tag.long_length;
            let header = (tag.code << 6) | if short { tag.body.len() as u16 } else { 63 };
            body.extend(header.to_le_bytes());
            if !short {
                body.extend((tag.body.len() as u32).to_le_bytes());
            }
            body.extend(&tag.body);
        }
        body.extend(&self.suffix);
        Ok(body)
    }

    pub fn save(&self) -> Result<Vec<u8>> {
        let body = self.body()?;
        if body == self.original_body {
            return Ok(self.original.clone());
        }
        let mut out = Vec::with_capacity(body.len() + 8);
        out.extend(self.signature);
        out.push(self.version);
        let payload = if self.compression == GfxCompression::Zlib {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            encoder.write_all(&body)?;
            encoder.finish()?
        } else {
            body.clone()
        };
        out.extend((u32::try_from(body.len() + 8)?).to_le_bytes());
        out.extend(payload);
        let reparsed = Self::load(&out)?;
        ensure!(
            reparsed.prefix == self.prefix
                && reparsed.tags == self.tags
                && reparsed.suffix == self.suffix,
            "GFx save verification failed"
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(compression: GfxCompression) -> Vec<u8> {
        let mut body = vec![0x08, 0x00, 0x00, 0x0c, 0x01, 0x00];
        let import = b"ui/icon.dds\0";
        body.extend(((57u16 << 6) | import.len() as u16).to_le_bytes());
        body.extend(import);
        body.extend((0u16).to_le_bytes());
        let mut out = b"GFX\x09".to_vec();
        out.extend((body.len() as u32 + 8).to_le_bytes());
        if compression == GfxCompression::Zlib {
            out[0..3].copy_from_slice(b"CFX");
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            z.write_all(&body).unwrap();
            out.extend(z.finish().unwrap());
        } else {
            out.extend(body);
        }
        out
    }

    #[test]
    fn exact_roundtrip_and_reference_edit() {
        for compression in [GfxCompression::None, GfxCompression::Zlib] {
            let source = sample(compression);
            let mut file = GfxFile::load(&source).unwrap();
            assert_eq!(file.references(), ["ui/icon.dds"]);
            assert_eq!(file.save().unwrap(), source);
            assert_eq!(file.replace_reference("ui/icon.dds", "ui/new.dds"), 1);
            let saved = file.save().unwrap();
            assert_eq!(GfxFile::load(&saved).unwrap().references(), ["ui/new.dds"]);
        }
    }

    #[test]
    fn template_is_parseable_and_has_frame_end() {
        let source = GfxFile::template().save().unwrap();
        let parsed = GfxFile::load(&source).unwrap();
        assert_eq!(parsed.signature, *b"GFX");
        assert_eq!(
            parsed.tags.iter().map(|tag| tag.code).collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(parsed.save().unwrap(), source);
    }

    #[test]
    fn timeline_frames_are_authored_before_end_and_roundtrip() {
        let mut file = GfxFile::template();
        assert_eq!(file.frame_rate(), Some(12.0));
        file.set_frame_rate(24.0).unwrap();
        assert_eq!(file.frame_rate(), Some(24.0));
        file.add_frame();
        file.add_frame();
        assert_eq!(file.frame_count(), 3);
        assert_eq!(file.tags.last().unwrap().code, 0);
        assert!(file.remove_last_frame());
        assert_eq!(file.frame_count(), 2);
        let saved = file.save().unwrap();
        let parsed = GfxFile::load(&saved).unwrap();
        assert_eq!(parsed.frame_count(), 2);
        assert_eq!(parsed.frame_rate(), Some(24.0));
    }

    #[test]
    fn frame_references_group_external_assets_by_show_frame() {
        let mut file = GfxFile::template();
        file.tags.insert(
            0,
            GfxTag {
                code: 57,
                body: b"ui/icon.dds\0".to_vec(),
                long_length: false,
            },
        );
        file.tags.insert(
            2,
            GfxTag {
                code: 57,
                body: b"Audio/Wii/click.wem\0".to_vec(),
                long_length: false,
            },
        );
        file.tags.insert(
            3,
            GfxTag {
                code: 1,
                body: Vec::new(),
                long_length: false,
            },
        );
        assert_eq!(
            file.frame_references(),
            vec![
                vec!["ui/icon.dds".to_string()],
                vec!["Audio/Wii/click.wem".to_string()]
            ]
        );
    }

    #[test]
    fn unknown_tag_payload_can_be_replaced_transactionally() {
        let mut file = GfxFile::template();
        file.tags.insert(
            1,
            GfxTag {
                code: 82,
                body: vec![0xde, 0xad, 0xbe, 0xef],
                long_length: false,
            },
        );
        let before = file.tag_body_hex(1).unwrap();
        assert_eq!(before, "de ad be ef");
        assert!(file.replace_tag_body_hex(1, "0g").is_err());
        assert_eq!(file.tag_body_hex(1).unwrap(), before);
        file.replace_tag_body_hex(1, "01 02 03").unwrap();
        let saved = file.save().unwrap();
        let reparsed = GfxFile::load(&saved).unwrap();
        assert_eq!(reparsed.tags[1].body, vec![1, 2, 3]);
    }
}
