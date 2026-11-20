use anyhow::{ensure,Result};
use std::{path::PathBuf,sync::Arc};
#[derive(Clone,Debug)]
pub enum Signal {
    Pcm, FilePending(PathBuf), File(Arc<super::audio_file::Spec>), Silence([f32;3]),
    MusicFile { spec: Arc<super::audio_file::Spec>, offset: u64, lead: u64, length: u64 },
}
fn silence_seconds([base,min,max]:[f32;3],draw:u16)->Result<f32> {
    ensure!(draw<=32767,"Invalid silence RNG sample");let fraction=draw as f32/32767.;let offset=min+fraction*(max-min);
    let mut seconds=base+offset;if seconds<0.001 {seconds=0.001;}Ok(seconds)
}
impl Signal {
    pub fn silence_layout(&self,rate:u32,passes:u16,random:&mut dyn FnMut()->u16)->Result<Option<super::audio_loop::Layout>> {
        let Self::Silence([base,min,max])=*self else {return Ok(None)};
        //8031de00 always calls rand, even for equal modifiers. Arithmetic is
        // single precision throughout, unlike ordinary voice random ranges.
        let seconds=silence_seconds([base,min,max],random())?;
        let count=seconds*rate as f32;ensure!(count.is_finite() && count>=0. && (count as f64)<4294967296.,"Silence duration exceeds native sample count");
        let count=count as u32;Ok(Some(super::audio_loop::Layout::new(count as usize,Some([0,count]),passes)?))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_silence_duration_bits_sample_counts_and_unconditional_rng()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_silence.json"))?;
        for c in fixture["cases"].as_array().unwrap() {
            let p=std::array::from_fn(|i|f32::from_bits(c["parameters_bits"][i].as_u64().unwrap() as u32));let draw=c["draw"].as_u64().unwrap() as u16;
            assert_eq!(silence_seconds(p,draw)?.to_bits(),c["seconds_bits"].as_u64().unwrap() as u32);
            let mut calls=0;let layout=Signal::Silence(p).silence_layout(c["rate"].as_u64().unwrap() as u32,1,&mut ||{calls+=1;draw})?.unwrap();
            assert_eq!(calls,1);assert_eq!(layout.samples as u64,c["samples"].as_u64().unwrap());
        }Ok(())
    }
}
