//! DirectDraw Surface texture support.
//!
//! Retail textures are preserved byte-for-byte until the pixels are changed.
//! Edited images are emitted as a conventional uncompressed RGBA8 DDS so the
//! result remains readable by the game and common texture tools.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Debug)]
pub struct DdsFile {
    pub width: u32,
    pub height: u32,
    pub mipmaps: u32,
    pub format: String,
    pub rgba: Vec<u8>,
    original: Vec<u8>,
    edited: bool,
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    data.get(offset..offset + 4)
        .context("Truncated DDS header")
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn decode_uncompressed(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let flags = u32_at(data, 80)?;
    ensure!(flags & 0x40 != 0, "DDS pixel format is not RGB");
    let bits = u32_at(data, 88)?;
    ensure!(
        bits == 24 || bits == 32,
        "Unsupported uncompressed DDS bit depth"
    );
    let masks = [
        u32_at(data, 92)?,
        u32_at(data, 96)?,
        u32_at(data, 100)?,
        u32_at(data, 104)?,
    ];
    let bytes_per_pixel = (bits / 8) as usize;
    let pitch = u32_at(data, 20)?.max(width * bytes_per_pixel as u32) as usize;
    let required = pitch
        .checked_mul(height as usize)
        .and_then(|n| n.checked_add(128))
        .context("DDS pixel data size overflow")?;
    ensure!(data.len() >= required, "Truncated DDS pixel data");
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let row = &data[128 + y * pitch..];
        for x in 0..width as usize {
            let offset = x * bytes_per_pixel;
            let raw = if bytes_per_pixel == 4 {
                u32::from_le_bytes(row[offset..offset + 4].try_into().unwrap())
            } else {
                u32::from_le_bytes([row[offset], row[offset + 1], row[offset + 2], 0])
            };
            let channel = |mask: u32| -> u8 {
                if mask == 0 {
                    return 255;
                }
                let shift = mask.trailing_zeros();
                let max = mask >> shift;
                (((raw & mask) >> shift) * 255 / max) as u8
            };
            rgba.extend([
                channel(masks[0]),
                channel(masks[1]),
                channel(masks[2]),
                channel(masks[3]),
            ]);
        }
    }
    Ok(rgba)
}

impl DdsFile {
    pub fn load(data: &[u8]) -> Result<Self> {
        ensure!(
            data.len() >= 128 && &data[..4] == b"DDS ",
            "Invalid DDS header"
        );
        ensure!(u32_at(data, 4)? == 124, "Unsupported DDS header size");
        let height = u32_at(data, 12)?;
        let width = u32_at(data, 16)?;
        ensure!(width > 0 && height > 0, "DDS dimensions must be nonzero");
        let mipmaps = u32_at(data, 28)?.max(1);
        let fourcc = &data[84..88];
        let format = if fourcc == b"DXT1" {
            "DXT1"
        } else if fourcc == b"DXT3" {
            "DXT3"
        } else if fourcc == b"DXT5" {
            "DXT5"
        } else if fourcc == [0, 0, 0, 0] {
            "RGBA"
        } else {
            std::str::from_utf8(fourcc).unwrap_or("custom")
        }
        .to_owned();
        let rgba = if fourcc == [0, 0, 0, 0] {
            image::RgbaImage::from_raw(width, height, decode_uncompressed(data, width, height)?)
                .context("Invalid uncompressed DDS pixel data")?
        } else {
            image::load_from_memory_with_format(data, image::ImageFormat::Dds)
                .context("DDS pixel data is not supported by the image decoder")?
                .to_rgba8()
        };
        ensure!(
            rgba.width() == width && rgba.height() == height,
            "DDS decoder dimensions disagree with header"
        );
        Ok(Self {
            width,
            height,
            mipmaps,
            format,
            rgba: rgba.into_raw(),
            original: data.to_vec(),
            edited: false,
        })
    }

    pub fn template() -> Self {
        Self {
            width: 1,
            height: 1,
            mipmaps: 1,
            format: "RGBA".into(),
            rgba: vec![255, 255, 255, 255],
            original: Vec::new(),
            edited: true,
        }
    }

    pub fn replace_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .context("DDS dimensions overflow")?;
        ensure!(
            rgba.len() == expected,
            "RGBA data does not match DDS dimensions"
        );
        ensure!(width > 0 && height > 0, "DDS dimensions must be nonzero");
        self.width = width;
        self.height = height;
        self.mipmaps = 1;
        self.format = "RGBA".into();
        self.rgba = rgba;
        self.edited = true;
        Ok(())
    }

    pub fn save(&self) -> Result<Vec<u8>> {
        if !self.edited {
            return Ok(self.original.clone());
        }
        let mut out = Vec::with_capacity(128 + self.rgba.len());
        out.extend_from_slice(b"DDS ");
        out.extend_from_slice(&124u32.to_le_bytes());
        out.extend_from_slice(&0x0002_100Fu32.to_le_bytes()); // CAPS | HEIGHT | WIDTH | PIXELFORMAT
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&(self.width * 4).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.resize(76, 0);
        out.extend_from_slice(&32u32.to_le_bytes()); // pixel format size
        out.extend_from_slice(&0x41u32.to_le_bytes()); // RGB | alpha pixels
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes());
        out.extend_from_slice(&0x00ff0000u32.to_le_bytes());
        out.extend_from_slice(&0x0000ff00u32.to_le_bytes());
        out.extend_from_slice(&0x000000ffu32.to_le_bytes());
        out.extend_from_slice(&0xff000000u32.to_le_bytes());
        out.extend_from_slice(&0x1000u32.to_le_bytes()); // texture
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        // DDS stores BGRA for the masks above; convert from the editor's RGBA.
        for pixel in self.rgba.chunks_exact(4) {
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        let reparsed = Self::load(&out)?;
        ensure!(
            reparsed.width == self.width
                && reparsed.height == self.height
                && reparsed.rgba == self.rgba,
            "DDS save verification failed"
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_roundtrips_and_decodes() {
        let source = DdsFile::template().save().unwrap();
        let file = DdsFile::load(&source).unwrap();
        assert_eq!((file.width, file.height), (1, 1));
        assert_eq!(file.rgba, [255, 255, 255, 255]);
    }
}
