//! Static node loading80335ce0..8033616c and voice pitch803812a4..803812dc.
//! Per-voice random modifiers retain the original channel/draw order.
//! RTPC/state contributions and native SRC remain separate.
use anyhow::{ensure,Result};
#[derive(Debug,PartialEq,Eq)]
pub struct Fixed {pub volume:i16,pub lfe:i16,pub pitch:i16,pub filter:u8}
fn integer(value:f32)->Result<i32> {
    ensure!(value.is_finite() && value as f64>=i32::MIN as f64 && (value as f64)<2147483648.,"Unsafe native audio integer conversion");
    Ok(value as i32)
}
fn tenths(value:f32)->Result<i16> {
    let value=value*10.;
    let rounded=if value<0. {(value-0.5).ceil()} else {(value+0.5).floor()};
    Ok(integer(rounded)? as i16)
}
impl Fixed {
    pub fn load(ranges:&[[f32;3];4])->Result<Self> {
        Ok(Self{volume:tenths(ranges[0][0])?,lfe:tenths(ranges[1][0])?,pitch:integer(ranges[2][0])? as i16,filter:integer(ranges[3][0])? as u8})
    }
}
pub fn pitch_ratio(pitch:f32)->Result<f32> {
    ensure!(pitch.is_finite(),"Nonfinite voice pitch");
    // Native fdivs precedes the double pow leaf and frsp follows it.
    let exponent=pitch.clamp(-2400.,2400.)/1200.;
    Ok(libm::pow(2.,exponent as f64) as f32)
}
///80334a34..80334b9c. Per-node order is LFE, pitch, LPF, volume;
/// width subtraction/additions are single, the random fraction/product double.
pub fn random_offsets(chain:&[[[f32;2];4]],random:&mut dyn FnMut()->u16)->Result<[f32;4]> {
    let mut output=[0.;4];
    for ranges in chain {for channel in [1,2,3,0] {
        let [min,max]=ranges[channel];let width=max-min;
        ensure!(min.is_finite() && max.is_finite() && width.is_finite(),"Nonfinite audio random range");
        let value=if width==0. {0.}else {let draw=random();ensure!(draw<=32767,"Audio RNG exceeded native fifteen-bit range");((draw as f64/32767.)*width as f64) as f32};
        let offset=value+min;output[channel]+=offset;
    }}Ok(output)
}
#[cfg(debug_assertions)]
pub fn validate()->Result<()> {
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_parameters.json"))?;
    for case in fixture["parameters"].as_array().unwrap() {
        let bases=case["input_bits"].as_array().unwrap().iter().map(|v|f32::from_bits(v.as_u64().unwrap() as u32)).collect::<Vec<_>>();
        let result=Fixed::load(&std::array::from_fn(|i|[bases[i],0.,0.]))?;
        let actual=[result.volume as i64,result.lfe as i64,result.pitch as i64,result.filter as i64];
        ensure!(actual.iter().zip(case["native"].as_array().unwrap()).all(|(a,b)|Some(*a)==b.as_i64()),"Native static audio parameter mismatch: {case}");
    }
    for case in fixture["pitch"].as_array().unwrap() {
        let input=f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
        ensure!(pitch_ratio(input)?.to_bits()==case["ratio_bits"].as_u64().unwrap() as u32,"Native pitch clamp/conversion mismatch: {case}");
    }
    println!("PASS: original static audio parameter conversions and final voice pitch clamp/ratio");
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn original_parameter_and_pitch_samples()->anyhow::Result<()> {super::validate()}
    #[test]
    fn original_per_voice_modifiers_match_values_rng_and_draw_order()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_random_parameters.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let chain=serde_json::from_value::<Vec<[[u32;2];4]>>(case["range_bits"].clone())?.into_iter().map(|node|node.map(|pair|pair.map(f32::from_bits))).collect::<Vec<_>>();
            let mut rng=super::super::game_random::State{seed:case["seed"].as_u64().unwrap() as u32,draws:0};
            let output=super::random_offsets(&chain,&mut ||rng.draw() as u16)?;
            assert_eq!(output.map(f32::to_bits).to_vec(),serde_json::from_value::<Vec<u32>>(case["output_bits"].clone())?);
            assert_eq!(rng.seed,case["seed_after"].as_u64().unwrap() as u32);assert_eq!(rng.draws,case["draws"].as_u64().unwrap());
        }Ok(())
    }
}
