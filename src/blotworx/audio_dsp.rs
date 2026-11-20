//! Nintendo DSP frame arithmetic. Wwise scheduling/mixing is separate.
//! Reference: vgmstream src/coding/ngc_dsp_decoder.c (valid predictor path).
use anyhow::{ensure, Context, Result};
#[derive(Clone, Debug)]
pub struct Decoder {
    pub coefficients: [i16; 16],
    pub history: [i16; 2],
}
impl Decoder {
    pub fn frame(&mut self, bytes: &[u8; 8]) -> Result<[i16; 14]> {
        self.range(bytes,0,14,None)
    }
    pub fn range(&mut self,bytes:&[u8;8],start:usize,end:usize,context:Option<u8>)->Result<[i16;14]> {
        ensure!(start<=end && end<=14,"Invalid DSP nibble range");
        let control=context.unwrap_or(bytes[0]);
        let predictor = (control >> 4) as usize;
        ensure!(predictor < 8, "Invalid DSP predictor {predictor}");
        let scale = 1i64 << (control & 15);
        let a = self.coefficients[predictor * 2] as i64;
        let b = self.coefficients[predictor * 2 + 1] as i64;
        let mut output = [0; 14];
        for (i, sample) in output.iter_mut().enumerate().take(end).skip(start) {
            let packed = bytes[1 + i / 2];
            let nibble = if i % 2 == 0 { packed >> 4 } else { packed & 15 };
            let signed = ((nibble as i8) << 4) >> 4;
            let value = (((signed as i64 * scale) << 11)
                + 1024
                + a * self.history[0] as i64
                + b * self.history[1] as i64)
                >> 11;
            *sample = value.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
            self.history = [*sample, self.history[0]];
        }
        Ok(output)
    }
}
#[derive(Clone,Debug)]
pub struct Definition {pub sample_rate:u32,pub sample_count:u32,pub loop_points:Option<[u32;2]>,pub decoder:Decoder,pub loop_context:[u16;3]}
impl Definition {
    pub fn parse(fmt:&[u8],header:&[u8],sampler:Option<&[u8]>)->Result<Self> {
        let u32_at=|b:&[u8],p:usize|->Result<u32>{Ok(u32::from_be_bytes(b.get(p..p+4).context("Truncated DSP metadata")?.try_into()?))};
        ensure!(fmt.len() == 40, "Unsupported Wii fmt layout");
        ensure!(
            u16::from_be_bytes(fmt[..2].try_into()?) == 0xfff0,
            "Unsupported audio codec"
        );
        ensure!(
            u16::from_be_bytes(fmt[2..4].try_into()?) == 1,
            "Multichannel DSP layout is not yet supported"
        );
        ensure!(
            u16::from_be_bytes(fmt[12..14].try_into()?) == 8
                && u16::from_be_bytes(fmt[14..16].try_into()?) == 4,
            "Unsupported DSP frame format"
        );
        let sample_rate = u32_at(fmt, 4)?;
        let sample_count=u32_at(fmt,8)?;
        ensure!(sample_rate > 0, "Zero audio sample rate");
        ensure!(header.len() == 46, "Unsupported WiiH layout");
        let decoder = Decoder {
            coefficients: std::array::from_fn(|i| {
                i16::from_be_bytes([header[2 * i], header[2 * i + 1]])
            }),
            history: [
                i16::from_be_bytes(header[36..38].try_into()?),
                i16::from_be_bytes(header[38..40].try_into()?),
            ],
        };
        let loop_points=sampler.map(super::audio_loop::sampler_points).transpose()?.flatten();
        Ok(Self{sample_rate,sample_count,loop_points,decoder,loop_context:std::array::from_fn(|i|u16::from_be_bytes([header[40+2*i],header[41+2*i]]))})
    }
}
/// Old Wwise mono DSP container. Borrowed metadata remains available for the
/// eventual event player; decoding alone does not apply or discard its loops.
pub struct Media<'a> {
    pub sample_rate: u32,
    pub sample_count:u32,
    pub loop_points:Option<[u32;2]>,
    pub declared_size: usize,
    pub actual_size: usize,
    pub sampler: Option<&'a [u8]>,
    decoder: Decoder,
    data: &'a [u8],
    loop_context:[u16;3],
}
impl<'a> Media<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        fn u32_at(b: &[u8], p: usize) -> Result<u32> {
            Ok(u32::from_be_bytes(
                b.get(p..p.checked_add(4).context("Audio offset overflow")?)
                    .context("Truncated audio integer")?
                    .try_into()?,
            ))
        }
        ensure!(
            bytes.get(..4) == Some(b"RIFX") && bytes.get(8..12) == Some(b"WAVE"),
            "Expected Wii RIFX/WAVE media"
        );
        let declared_size = (u32_at(bytes, 4)? as usize)
            .checked_add(8)
            .context("Audio size overflow")?;
        ensure!(
            declared_size <= bytes.len(),
            "Truncated or prefetched media is not yet supported"
        );
        // Retail bank sizes can exceed their RIFF declaration. Walk the bounded
        // DIDX media slice rather than truncating a complete last data chunk.
        let mut chunks = std::collections::BTreeMap::new();
        let mut pos = 12usize;
        while pos < bytes.len() {
            let header = bytes
                .get(pos..pos.checked_add(8).context("Audio offset overflow")?)
                .context("Truncated audio chunk header")?;
            let size = u32_at(header, 4)? as usize;
            let end = pos
                .checked_add(8)
                .and_then(|p| p.checked_add(size))
                .context("Audio chunk overflow")?;
            let body = bytes.get(pos + 8..end).context("Truncated audio chunk")?;
            ensure!(
                chunks.insert(&header[..4], body).is_none(),
                "Duplicate audio chunk"
            );
            // Retail Vorbis media 06cd1a4d ends on an odd data byte with no
            // trailing RIFF pad. Inter-chunk padding is still required.
            pos = if end == bytes.len() {
                end
            } else {
                end.checked_add(size & 1)
                    .context("Audio padding overflow")?
            };
            ensure!(pos <= bytes.len(), "Missing audio padding byte");
        }
        let fmt = *chunks.get(&b"fmt "[..]).context("Missing audio fmt")?;
        let header=chunks.get(&b"WiiH"[..]).copied().unwrap_or(&[]);
        let sampler=chunks.get(&b"smpl"[..]).copied();
        let definition=Definition::parse(fmt,header,sampler)?;
        let data = *chunks.get(&b"data"[..]).context("Missing audio samples")?;
        ensure!(data.len() % 8 == 0, "Partial DSP frame");
        Ok(Self {
            sample_rate:definition.sample_rate,
            sample_count:definition.sample_count,loop_points:definition.loop_points,
            declared_size,
            actual_size: bytes.len(),
            sampler: chunks.get(&b"smpl"[..]).copied(),
            decoder:definition.decoder,
            data,
            loop_context:definition.loop_context,
        })
    }
    pub fn validate_loop_context(&self,samples:&[i16],start:usize)->Result<()> {
        let history=[if start==0 {self.decoder.history[0]}else {*samples.get(start-1).context("DSP loop start outside PCM")?},if start==0 {self.decoder.history[1]}else if start==1 {self.decoder.history[0]}else {*samples.get(start-2).context("DSP loop start outside PCM")?}];
        let predictor=*self.data.get(start/14*8).context("DSP loop frame outside source")? as u16;
        ensure!(self.loop_context==[predictor,history[0] as u16,history[1] as u16],"DSP loop context differs from first pass; alternate loop-context decoding is not connected");Ok(())
    }
    pub fn decode(&self) -> Result<Vec<i16>> {
        let count = (self.data.len() / 8)
            .checked_mul(14)
            .context("Decoded sample count overflow")?;
        let mut output = Vec::new();
        output.try_reserve_exact(count)?;
        let mut decoder = self.decoder.clone();
        for frame in self.data.chunks_exact(8) {
            output.extend_from_slice(&decoder.frame(frame.try_into()?)?);
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires BLOTWORX_TEST_DATA; optional BLOTWORX_AUDIO_CAPTURE WAV output"]
    fn decode_original_candidate() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").expect("Set BLOTWORX_TEST_DATA");
        let bank_bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = super::super::audio_bank::Bank::parse(&bank_bytes)?;
        let media = bank.media[&0x17d3ec7f];
        let source = Media::parse(media)?;
        assert_eq!(source.declared_size, 6094);
        assert_eq!(source.actual_size, 6176);
        assert!(source.sampler.is_some());
        let rate = source.sample_rate;
        assert_eq!(rate, 16000);
        let pcm = source.decode()?;
        for end in 0..media.len() {
            assert!(
                Media::parse(&media[..end]).is_err(),
                "Accepted truncated media at {end}"
            );
        }
        assert_eq!(pcm.len(), 10416);
        assert!(pcm.iter().any(|&s| s != 0));
        println!(
            "Decoded {} mono samples at {} Hz; peak {}",
            pcm.len(),
            rate,
            pcm.iter().map(|&v| (v as i32).abs()).max().unwrap()
        );
        if let Some(path) = std::env::var_os("BLOTWORX_AUDIO_CAPTURE") {
            let size = (pcm.len() * 2) as u32;
            let mut wav = b"RIFF".to_vec();
            wav.extend_from_slice(&(36 + size).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&rate.to_le_bytes());
            wav.extend_from_slice(&(rate * 2).to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&size.to_le_bytes());
            for sample in pcm {
                wav.extend_from_slice(&sample.to_le_bytes());
            }
            std::fs::write(path, wav)?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires BLOTWORX_TEST_DATA imported EM1 files"]
    fn global_bank_dsp_coverage() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").expect("Set BLOTWORX_TEST_DATA");
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = super::super::audio_bank::Bank::parse(&bytes)?;
        let (mut decoded, mut unsupported, mut samples) = (0, 0, 0usize);
        for (id, bytes) in &bank.media {
            match Media::parse(bytes) {
                Ok(media) => {
                    let pcm = media.decode().with_context(|| format!("Media {id:08x}"))?;
                    samples += pcm.len();
                    decoded += 1;
                }
                Err(error) if error.to_string() == "Unsupported audio codec" => unsupported += 1,
                Err(error) => {
                    return Err(error.context(format!("Unexpected Global media {id:08x} failure")))
                }
            }
        }
        assert_eq!((decoded, unsupported), (182, 4));
        println!("Global bank: {decoded} DSP clips decoded, {samples} samples; {unsupported} unsupported codec clips");
        Ok(())
    }
    #[test]
    fn signed_nibbles_scale_and_predictor_history() {
        let mut d = Decoder {
            coefficients: [0; 16],
            history: [0; 2],
        };
        assert_eq!(
            d.frame(&[1, 0x17, 0x8f, 0, 0, 0, 0, 0]).unwrap()[..4],
            [2, 14, -16, -2]
        );
        d.coefficients[2] = 2048;
        d.history = [123, -456];
        assert_eq!(d.frame(&[0x10, 0, 0, 0, 0, 0, 0, 0]).unwrap(), [123; 14]);
        assert_eq!(d.history, [123, 123]);
    }
    #[test]
    fn saturation_and_invalid_predictor() {
        let mut d = Decoder {
            coefficients: [0; 16],
            history: [0; 2],
        };
        let out = d.frame(&[15, 0x78, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(out[..2], [32767, -32768]);
        let before = d.history;
        assert!(d.frame(&[0x80; 8]).is_err());
        assert_eq!(d.history, before);
    }
}
