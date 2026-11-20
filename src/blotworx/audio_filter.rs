//! Retail AX LPF controls8038a320..8038a488. The integer recurrence is the
//! Wii AX one-pole filter (Dolphin AXVoice.h LowPassFilter). The surrounding
//! linear source/host resamplers remain a tracked discrepancy from native SRC.
use anyhow::{ensure,Result};
const TABLE:[u16;22]=[0,0,14,799,2787,5977,10367,16783,21778,25991,28416,30355,31726,32244,32570,32668,32734,32754,32760,32764,32765,32765];

pub fn coefficients(value:f32)->Result<Option<[u16;2]>> {
    ensure!(value.is_finite(),"Nonfinite voice low-pass value");
    let value=value.clamp(0.,100.);
    if value==0. {return Ok(None);}
    // fdivs then fmuls, integer truncation and unfused interpolation.
    let position=20.*(value/100.);let index=position as usize;
    let fraction=position-index as f32;
    let delta=TABLE[index+1] as f32-TABLE[index] as f32;
    let feedback=(TABLE[index] as f32+fraction*delta) as u16;
    Ok(Some([32768-feedback,feedback]))
}
#[derive(Clone,Debug)]
pub struct Filter {pub coefficients:[u16;2],pub history:i16}
impl Filter {
    pub fn sample(&mut self,input:i16)->i16 {
        let [a,b]=self.coefficients;
        let value=(a as i64*input as i64+b as i64*self.history as i64)>>15;
        self.history=value.clamp(i16::MIN as i64,i16::MAX as i64) as i16;self.history
    }
}
/// Constant-size per-voice state. Filter at the original32kHz clock before
/// interpolation to the host48kHz clock; never allocate a filtered PCM copy.
pub struct Stream {
    filter:Filter,source_phase:f64,source_step:f64,
    layout:Option<super::audio_loop::Layout>,
    host_phase:u64,native_index:u64,current:i16,next:i16,initialized:bool,
}
impl Stream {
    pub fn dynamic(value:f32,source_rate:f64)->Result<Self> {
        ensure!(source_rate.is_finite() && source_rate>0.,"Invalid filtered source rate");
        Ok(Self{filter:Filter{coefficients:coefficients(value)?.unwrap_or([32768,0]),history:0},layout:None,source_phase:0.,source_step:source_rate/32000.,host_phase:0,native_index:0,current:0,next:0,initialized:false})
    }
    pub fn set_layout(&mut self,layout:Option<super::audio_loop::Layout>) {self.layout=layout;}
    pub fn set_coefficients(&mut self,coefficients:[u16;2]) {self.filter.coefficients=coefficients;}
    pub fn new(value:f32,source_rate:f64)->Result<Option<Self>> {
        ensure!(source_rate.is_finite() && source_rate>0.,"Invalid filtered source rate");
        Ok(coefficients(value)?.map(|coefficients|Self{filter:Filter{coefficients,history:0},layout:None,source_phase:0.,source_step:source_rate/32000.,host_phase:0,native_index:0,current:0,next:0,initialized:false}))
    }
    fn native_with(&mut self,sampler:&mut impl FnMut(u64)->Result<Option<i16>>)->Result<i16> {
        let index=self.source_phase as u64;
        let a=sampler(index)?.unwrap_or(0);let b=sampler(index+1)?.unwrap_or(a);
        let fraction=(self.source_phase-self.source_phase.floor()) as f32;
        let input=(a as f32+(b as f32-a as f32)*fraction) as i16;
        self.source_phase+=self.source_step;Ok(self.filter.sample(input))
    }
    pub fn sample(&mut self,samples:&[i16])->f32 {
        let layout=self.layout;self.sample_with(&mut |index|Ok(super::audio_loop::sample(samples,layout,index))).expect("In-memory PCM access is infallible")
    }
    pub fn sample_with(&mut self,sampler:&mut impl FnMut(u64)->Result<Option<i16>>)->Result<f32> {
        if !self.initialized {self.current=self.native_with(sampler)?;self.next=self.native_with(sampler)?;self.initialized=true;}
        let index=self.host_phase/48000;
        while self.native_index<index {self.current=self.next;self.next=self.native_with(sampler)?;self.native_index+=1;}
        let fraction=(self.host_phase%48000) as f32/48000.;
        let output=self.current as f32+(self.next as f32-self.current as f32)*fraction;
        self.host_phase+=32000;Ok(output)
    }

}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_filter_preserves_source_time_and_history()->Result<()> {
        let samples=(0..1000).map(|i|if i%2==0 {16000}else {-16000}).collect::<Vec<i16>>();
        let mut stream=Stream::dynamic(35.,32000.)?;
        for _ in 0..144 {stream.sample(&samples);}
        let phase=stream.source_phase;let host=stream.host_phase;let history=stream.filter.history;
        stream.set_coefficients(coefficients(80.)?.unwrap());
        assert_eq!(stream.source_phase,phase);assert_eq!(stream.host_phase,host);assert_eq!(stream.filter.history,history);
        for _ in 0..144 {assert!(stream.sample(&samples).is_finite());}
        assert!(stream.source_phase>phase);
        stream.set_coefficients([32768,0]);
        for _ in 0..144 {stream.sample(&samples);}
        assert!(stream.source_phase>phase+96.);Ok(())
    }
    #[test]
    fn original_lpf_coefficients_and_bypass()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_filter.json"))?;
        assert_eq!(TABLE.to_vec(),serde_json::from_value::<Vec<u16>>(fixture["table"].clone())?);
        for case in fixture["cases"].as_array().unwrap() {
            let value=f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
            let expected=case["ax"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u16).collect::<Vec<_>>();
            if let Some(coefficients)=coefficients(value)? {assert_eq!(coefficients.as_slice(),&expected[2..],"{case}");assert_eq!(expected[0],1);}else {assert_eq!(expected[0],0);}
            assert_eq!(value.clamp(0.,100.).to_bits(),case["stored_bits"].as_u64().unwrap() as u32);
        }Ok(())
    }
    #[test]
    fn integer_filter_keeps_history_and_signed_flooring()->Result<()> {
        let mut filter=Filter{coefficients:coefficients(35.)?.unwrap(),history:0};
        assert_eq!(filter.coefficients,[15985,16783]);
        let samples=[32767,-32768,0,0,1000,-1000];
        let mut history=0i64;
        for input in samples {history=(15985*input as i64+16783*history)>>15;assert_eq!(filter.sample(input),history as i16);}
        assert!(coefficients(f32::NAN).is_err());Ok(())
    }
}
