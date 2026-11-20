//! Original source sample count and authored loop range, not padded DSP frames.
//! Host interpolation/AX address rounding remain separate reference work.
use anyhow::{ensure,Context,Result};
///8037d4f4 reads the first loop through the authored sampler-data offset.
pub fn sampler_points(bytes:&[u8])->Result<Option<[u32;2]>> {
    let word=|at:usize|->Result<u32>{Ok(u32::from_be_bytes(bytes.get(at..at.checked_add(4).context("Sampler offset overflow")?).context("Truncated audio sampler")?.try_into()?))};
    if word(28)?==0 {return Ok(None);}
    let offset=word(32)? as usize;
    Ok(Some([word(offset.checked_add(44).context("Sampler offset overflow")?)?,word(offset.checked_add(48).context("Sampler offset overflow")?)?]))
}
#[derive(Clone,Copy,Debug)]
pub struct Layout {pub samples:usize,pub start:usize,pub end:usize,pub passes:u16}
impl Layout {
    pub fn new(samples:usize,points:Option<[u32;2]>,passes:u16)->Result<Self> {
        ensure!(samples>0,"Empty authored audio source");
        let [start,end]=points.unwrap_or([0,0]).map(|n|n as usize);
        if passes!=1 {ensure!(start<end && end<=samples,"Invalid or missing authored audio loop range");}
        Ok(Self{samples,start,end,passes})
    }
    pub fn duration(&self)->Option<u64> {
        if self.passes==0 {None}else if self.passes==1 {Some(self.samples as u64)}else {Some(self.samples as u64+(self.end-self.start) as u64*(self.passes as u64-1))}
    }
    pub fn index(&self,time:u64)->Option<usize> {
        if self.duration().is_some_and(|end|time>=end) {return None;}
        if self.passes==1 || time<self.end as u64 {return Some(time as usize);}
        let span=(self.end-self.start) as u64;
        if self.passes!=0 {
            let final_start=self.end as u64+span*(self.passes as u64-2);
            if time>=final_start {return Some(self.start+(time-final_start) as usize);}
        }
        Some(self.start+((time-self.end as u64)%span) as usize)
    }
}
pub fn sample(samples:&[i16],layout:Option<Layout>,time:u64)->Option<i16> {
    samples.get(if let Some(layout)=layout {layout.index(time)?}else {usize::try_from(time).ok()?}).copied()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_sampler_local_fade_callback_and_gain_bits()->Result<()> {
        let data:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_loop.json"))?;
        for case in data["sampler"].as_array().unwrap() {
            let encoded=case["encoded"].as_str().unwrap();let bytes=(0..encoded.len()).step_by(2).map(|i|u8::from_str_radix(&encoded[i..i+2],16).unwrap()).collect::<Vec<_>>();
            assert_eq!(serde_json::to_value(sampler_points(&bytes)?)?,case["points"]);
        }
        for case in data["callbacks"].as_array().unwrap() {
            let mut fade=super::super::audio_transition::Mute::default();fade.apply_db(f32::from_bits(case["db_bits"].as_u64().unwrap() as u32))?;
            assert_eq!(fade.byte as u64,case["byte"].as_u64().unwrap());
        }
        for case in data["gain"].as_array().unwrap() {
            let values=case["values_bits"].as_array().unwrap().iter().enumerate().map(|(i,v)|(i as u32,f32::from_bits(v.as_u64().unwrap() as u32))).collect::<Vec<_>>();
            let db=super::super::audio_mute_mix::gain_db_local(f32::from_bits(case["base_bits"].as_u64().unwrap() as u32),&values,case["local"].as_u64().unwrap() as u8)?;
            assert_eq!(db.to_bits(),case["gain_bits"].as_u64().unwrap() as u32,"{case}");
        }Ok(())
    }
    #[test]
    fn finite_loops_preserve_intro_tail_and_total_pass_count()->Result<()> {
        let clip=Layout::new(8,Some([2,5]),3)?;
        assert_eq!(clip.duration(),Some(14));
        assert_eq!((0..16).filter_map(|i|clip.index(i)).collect::<Vec<_>>(),[0,1,2,3,4,2,3,4,2,3,4,5,6,7]);
        let once=Layout::new(8,Some([2,5]),1)?;
        assert_eq!((0..10).filter_map(|i|once.index(i)).collect::<Vec<_>>(),(0..8).collect::<Vec<_>>());Ok(())
    }
    #[test]
    fn infinite_loops_keep_bounded_offsets_without_repeated_buffers()->Result<()> {
        let clip=Layout::new(8,Some([2,5]),0)?;assert_eq!(clip.duration(),None);
        for time in [5,999,1_000_000_000_000,u64::MAX] {assert!((2..5).contains(&clip.index(time).unwrap()));}
        assert!(Layout::new(8,None,0).is_err());assert!(Layout::new(8,Some([4,4]),2).is_err());Ok(())
    }
}
