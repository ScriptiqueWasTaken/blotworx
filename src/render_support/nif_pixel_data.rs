//! Standard NiPixelData, distinct from Wii persistent tiled texture data.
//! Layout follows NiPixelFormat/NiPixelData LoadBinary and retail EM1 assets.
use anyhow::{ensure, Context, Result};
struct Reader<'a> {
    remaining: &'a [u8],
    little: bool,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        ensure!(count <= self.remaining.len(), "Truncated NiPixelData");
        let (a, b) = self.remaining.split_at(count);
        self.remaining = b;
        Ok(a)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<u32> {
        let b = self.take(4)?.try_into()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
}
pub(super) fn decode(bytes: &[u8], little: bool, version: u32) -> Result<(Vec<u8>, u32, u32)> {
    decode_data(bytes, little, version, false)
}

/// Linear PC persistent data retains its serialized channel semantics.
pub(super) fn decode_persistent(bytes: &[u8], little: bool, version: u32) -> Result<(Vec<u8>, u32, u32)> {
    decode_data(bytes, little, version, true)
}

#[derive(Debug)]
pub(crate) enum Encoding {Bc([u8;4]),Channels {order:Vec<usize>,stride:usize},Bgra4444}
#[derive(Debug)]
pub(crate) struct Mip<'a> {pub width:u32,pub height:u32,pub faces:Vec<&'a [u8]>}
#[derive(Debug)]
pub(crate) struct Storage<'a> {pub encoding:Encoding,pub mips:Vec<Mip<'a>>}
pub(crate) fn expand_bgra4444(payload:&[u8],rgba:&mut Vec<u8>) {
    rgba.clear();
    for pixel in payload.chunks_exact(2){let v=u16::from_le_bytes([pixel[0],pixel[1]]);rgba.extend([((v>>8)&15) as u8*17,((v>>4)&15) as u8*17,(v&15) as u8*17,(v>>12) as u8*17]);}
}
/// Borrow original mip/face payloads; no decompression or mip regeneration.
pub(crate) fn storage(bytes:&[u8],little:bool,version:u32,persistent:bool)->Result<Storage<'_>> {
    ensure!(
        version >= 0x14030004,
        "Legacy NiPixelData format layout requires separate decoding"
    );
    let mut r = Reader {
        remaining: bytes,
        little,
    };
    let format = r.word()?;
    let bits = r.byte()?;
    r.word()?;
    r.word()?;
    let flags = r.byte()?;
    let tiling = r.word()?;
    r.byte()?;
    ensure!(
        tiling == 0,
        "Nonlinear standard NiPixelData tiling is unsupported"
    );
    let mut components = Vec::new();
    for _ in 0..4 {
        components.push((r.word()?, r.word()?, r.byte()?, r.byte()?));
    }
    let palette = r.word()?;
    ensure!(
        palette == u32::MAX,
        "Palette-backed NiPixelData requires its linked palette"
    );
    let count = r.word()? as usize;
    let stride = r.word()? as usize;
    // Original loader uses fixed 16-entry width/height stack arrays.
    ensure!(
        (1..=16).contains(&count),
        "Invalid native NiPixelData mip count"
    );
    let mut mips = Vec::with_capacity(count);
    for _ in 0..count {
        mips.push((r.word()?, r.word()?, r.word()? as usize));
    }
    let face_bytes = r.word()? as usize;
    if persistent && version >= 0x14020006 { r.word()?; } // persistent pad offset
    let faces = r.word()? as usize;
    if persistent {
        let platform = r.word()?;
        ensure!(little && matches!(platform, 4 | 8), "Unsupported linear PC texture platform");
    }
    ensure!(matches!(faces,1|6),"Unsupported NiPixelData face count");
    ensure!(
        face_bytes.checked_mul(faces)==Some(r.remaining.len()),
        "NiPixelData face size/payload mismatch"
    );
    let (w, h, _) = mips[0];
    ensure!(w > 0 && h > 0, "Empty NiPixelData image");
    let compressed = match format {
        4 => Some((8usize, *b"DXT1")),
        5 => Some((16, *b"DXT3")),
        6 => Some((16, *b"DXT5")),
        _ => None,
    };
    for (i, &(width, height, offset)) in mips.iter().enumerate() {
        ensure!(
            width > 0 && height > 0 && width <= w && height <= h,
            "Invalid NiPixelData mip dimensions"
        );
        let length = if let Some((block, _)) = compressed {
            (width.div_ceil(4) as usize)
                .checked_mul(height.div_ceil(4) as usize)
                .and_then(|n| n.checked_mul(block))
        } else {
            (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(stride))
        }
        .context("NiPixelData mip size overflow")?;
        let end = mips.get(i + 1).map(|m| m.2).unwrap_or(face_bytes);
        ensure!(
            offset
                .checked_add(length)
                .is_some_and(|required| required <= end)
                && end <= face_bytes,
            "Truncated/overlapping NiPixelData mip"
        );
    }

    let encoding=if let Some((_,fourcc))=compressed {
        ensure!(flags&1!=0,"Noncanonical big-endian BC texture requires verified conversion");
        Encoding::Bc(fourcc)
    }else if format==1&&bits==16&&stride==2&&components==[(2,0,4,0),(1,0,4,0),(0,0,4,0),(3,0,4,0)] {
        ensure!(flags&1!=0,"Big-endian packed RGBA4444 requires its byte-order adapter");
        Encoding::Bgra4444
    }else {
    ensure!(
        matches!(format, 0 | 1) && bits as usize == stride * 8 && matches!(stride, 3 | 4),
        "Unsupported standard NiPixelData channel layout"
    );
    let active = components
        .iter()
        .filter(|component| component.2 != 0)
        .collect::<Vec<_>>();
    ensure!(
        active.len() == stride
            && active
                .iter()
                .all(|&&(semantic, representation, bits, signed)| semantic <= 3
                    && representation == 0
                    && bits == 8
                    && signed == 0),
        "Unsupported standard RGB channel representation"
    );
    let mut seen = std::collections::BTreeSet::new();
    ensure!(
        active.iter().all(|component| seen.insert(component.0))
            && [0, 1, 2].iter().all(|s| seen.contains(s)),
        "Invalid RGB channel semantics"
    );

        Encoding::Channels {order:active.iter().map(|c|c.0 as usize).collect(),stride}
    };
    let mut levels=Vec::with_capacity(mips.len());
    for &(width,height,offset) in &mips {
        let length=if let Some((block,_))=compressed {(width.div_ceil(4) as usize).checked_mul(height.div_ceil(4) as usize).and_then(|n|n.checked_mul(block))}else{(width as usize).checked_mul(height as usize).and_then(|n|n.checked_mul(stride))}.context("Mip length overflow")?;
        let mut slices=Vec::with_capacity(faces);
        for face in 0..faces {let start=face.checked_mul(face_bytes).and_then(|n|n.checked_add(offset)).context("Texture face offset overflow")?;let end=start.checked_add(length).context("Texture face end overflow")?;slices.push(r.remaining.get(start..end).context("Missing texture face pixels")?);}
        levels.push(Mip {width,height,faces:slices});
    }
    Ok(Storage {encoding,mips:levels})
}
fn decode_data(bytes:&[u8],little:bool,version:u32,persistent:bool)->Result<(Vec<u8>,u32,u32)> {
    let storage=storage(bytes,little,version,persistent)?;
    let level=&storage.mips[0];let (w,h)=(level.width,level.height);
    ensure!(level.faces.len()==1,"Cubemap NiPixelData needs a cubemap sampler; refusing to substitute one face");
    let payload=level.faces[0];
    match &storage.encoding {
        Encoding::Bc(fourcc)=>decode_bc(payload,w,h,*fourcc),
        Encoding::Channels {order,stride}=>{
            let length=(w as usize).checked_mul(h as usize).and_then(|n|n.checked_mul(4)).context("RGBA texture size overflow")?;
            let mut rgba=Vec::with_capacity(length);
            for pixel in payload.chunks_exact(*stride) {let mut color=[0,0,0,255];for (&component,&value) in order.iter().zip(pixel){color[component]=value;}rgba.extend(color);}
            Ok((rgba,w,h))
        },
        Encoding::Bgra4444=>{
            let mut rgba=Vec::with_capacity(payload.len()*2);
            expand_bgra4444(payload,&mut rgba);
            Ok((rgba,w,h))
        },
    }
}

fn decode_bc(payload: &[u8], w: u32, h: u32, fourcc: [u8;4]) -> Result<(Vec<u8>,u32,u32)> {
    let mut dds=vec![0u8;128];dds[..4].copy_from_slice(b"DDS ");
    for (offset,value) in [(4,124),(8,0x81007),(12,h),(16,w),(20,u32::try_from(payload.len())?),(76,32),(80,4),(108,0x1000)] {
        dds[offset..offset+4].copy_from_slice(&value.to_le_bytes());
    }
    dds[84..88].copy_from_slice(&fourcc);dds.extend_from_slice(payload);
    let image=image::load_from_memory_with_format(&dds,image::ImageFormat::Dds)?.to_rgba8();
    Ok((image.into_raw(),w,h))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = Vec::new();
        let word = |b: &mut Vec<u8>, v: u32| b.extend(v.to_be_bytes());
        word(&mut b, 4);
        b.push(0);
        word(&mut b, u32::MAX);
        word(&mut b, 0);
        b.push(1);
        word(&mut b, 0);
        b.push(0);
        for _ in 0..4 {
            word(&mut b, 19);
            word(&mut b, 5);
            b.extend([0, 0]);
        }
        word(&mut b, u32::MAX);
        word(&mut b, 1);
        word(&mut b, 0);
        for v in [4, 4, 0, 8, 1] {
            word(&mut b, v);
        }
        b.extend([0x00, 0xf8, 0, 0, 0, 0, 0, 0]);
        b
    }
    #[test]
    fn storage_preserves_original_small_mip_blocks()->Result<()> {
        let mut b=fixture();b[63..67].copy_from_slice(&2u32.to_be_bytes());
        let mip=[2u32,2,8].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>();
        b.splice(83..83,mip);b[95..99].copy_from_slice(&16u32.to_be_bytes());
        let second=[0xe0,0x07,0,0,0,0,0,0];b.extend(second);
        let storage=storage(&b,false,0x14030109,false)?;
        assert_eq!(storage.mips.len(),2);assert_eq!(storage.mips[1].width,2);
        assert_eq!(storage.mips[1].faces[0],second);assert_eq!(storage.mips[1].faces[0].as_ptr(),b[111..].as_ptr());Ok(())
    }
    #[test]
    fn cubemap_storage_retains_every_face_while_preview_rejects_flattening()->Result<()> {
        let mut b=fixture();b[87..91].copy_from_slice(&6u32.to_be_bytes());
        for face in 1..6u8 {b.extend([face;8]);}
        let storage=storage(&b,false,0x14030109,false)?;
        assert_eq!(storage.mips[0].faces.len(),6);assert_eq!(storage.mips[0].faces[5],[5;8]);
        assert!(decode(&b,false,0x14030109).is_err());Ok(())
    }
    #[test]
    fn packed_bgra4444_preserves_alpha_and_channel_order()->Result<()> {
        let mut b=fixture();b[..4].copy_from_slice(&1u32.to_be_bytes());b[4]=16;
        b[67..71].copy_from_slice(&2u32.to_be_bytes());
        for (at,semantic) in [(19,2u32),(29,1),(39,0),(49,3)] {
            b[at..at+4].copy_from_slice(&semantic.to_be_bytes());b[at+4..at+8].copy_from_slice(&0u32.to_be_bytes());b[at+8]=4;
        }
        b[83..87].copy_from_slice(&32u32.to_be_bytes());b.truncate(91);
        for _ in 0..16 {b.extend([0x21,0x84]);}
        let (rgba,w,h)=decode(&b,false,0x14030109)?;assert_eq!((w,h),(4,4));
        assert!(rgba.chunks_exact(4).all(|color|color==[68,34,17,136]));Ok(())
    }
    #[test]
    fn persistent_channels_and_platform_are_not_console_tiles() -> Result<()> {
        // Shared BC payload with the persistent pad/platform words inserted.
        let mut b = fixture();
        b.splice(87..87, 0u32.to_be_bytes());
        b.splice(95..95, 8u32.to_be_bytes());
        assert!(decode_persistent(&b, false, 0x14060500).is_err());
        // Convert header words to LE; the BC block bytes remain canonical LE.
        for offset in [0,5,9,14,19,23,29,33,39,43,49,53,59,63,67,71,75,79,83,87,91,95] {
            b[offset..offset+4].reverse();
        }
        let (rgba,w,h)=decode_persistent(&b,true,0x14060500)?;
        assert_eq!((w,h),(4,4));
        assert!(rgba.chunks_exact(4).all(|p|p==[255,0,0,255]));
        assert!(decode_persistent(&b[..b.len()-1],true,0x14060500).is_err());
        Ok(())
    }

    #[test]
    fn dx9_rgb_preserves_serialized_channel_order() -> Result<()> {
        let mut b=Vec::new();
        let word=|b:&mut Vec<u8>,v:u32| b.extend(v.to_le_bytes());
        word(&mut b,1);b.push(32);word(&mut b,u32::MAX);word(&mut b,0);
        b.push(1);word(&mut b,0);b.push(0);
        for semantic in [2,1,0,3] {word(&mut b,semantic);word(&mut b,0);b.extend([8,0]);}
        for v in [u32::MAX,1,4,2,1,0,8,0,1,4] {word(&mut b,v);}
        b.extend([7,11,23,255,31,47,59,127]);
        let (rgba,w,h)=decode_persistent(&b,true,0x14060500)?;
        assert_eq!((w,h),(2,1));
        assert_eq!(rgba,[23,11,7,255,59,47,31,127]);
        b[95..99].copy_from_slice(&5u32.to_le_bytes());
        assert!(decode_persistent(&b,true,0x14060500).is_err());
        Ok(())
    }

    #[test]
    fn standard_bc1_uses_linear_little_endian_blocks_and_rejects_truncation() -> Result<()> {
        let b = fixture();
        let (rgba, w, h) = decode(&b, false, 0x14060500)?;
        assert_eq!((w, h), (4, 4));
        assert!(rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        assert!(decode(&b[..b.len() - 1], false, 0x14060500).is_err());
        let mut cube = b.clone();
        cube[87..91].copy_from_slice(&6u32.to_be_bytes());
        assert!(decode(&cube, false, 0x14060500).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn retail_dbc_standard_textures_decode_without_wii_tile_conversion() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let pak = crate::pack_io::read_filtered(
            std::fs::read(std::path::Path::new(&root).join("packfiles/dbc_1st_zonea.pak"))?,
            |_, path| {
                [
                    "demo_prop_enva1_tex",
                    "demo_computers01_toon_tex",
                    "dun_roof_a1_tex",
                ]
                .iter()
                .any(|name| path.to_ascii_lowercase().contains(name))
            },
        )?;
        let textures = pak
            .pak
            .files
            .iter()
            .filter(|file| !file.data.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(textures.len(), 3);
        for file in textures {
            let (rgba, w, h) = super::super::decode_image_bytes(&file.data)
                .with_context(|| format!("Decode {}", file.path))?;
            assert_eq!(rgba.len(), w as usize * h as usize * 4);
            assert!(w > 0 && h > 0);
            assert!(rgba.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
        }
        Ok(())
    }
}
