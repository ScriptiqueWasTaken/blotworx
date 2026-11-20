//! PC SH sampling, contribution accumulation and mesh-local attributes.
//! The scene owner supplies native records, query state and visibility.
//! No light selection, visibility or specular defaults are guessed.
use anyhow::{ensure,Result};
#[derive(Debug,Clone)]
pub(super) struct Snapshot {
    pub coefficients:[[f32;3];9],
    /// Values in shader units, after the native source's color conversion.
    pub specular_color:Option<[f32;3]>,
    /// Already published native direction; do not normalize it in this adapter.
    pub specular_direction:Option<[f32;3]>,
}
#[derive(Debug,Clone)]
pub(super) struct Contribution {
    pub kind:super::d3d9_light::Kind,
    pub ambient:[f32;3],pub diffuse:[f32;3],
    pub light_direction:[f32;3],pub probe_direction:[f32;3],
    /// Already sampled native weight, including its source visibility/dimmer
    /// semantics. This adapter does not invent a weight or raycast result.
    pub weight:f32,
}
#[derive(Debug,Clone)]
pub(super) struct LightRecord {
    /// Native SH flags use type bits1/2/4/8, not ordinary NiLight IDs.
    pub flags:u16,pub position:[f32;3],pub direction:[f32;3],pub dimmer:f32,
    pub falloff:[f32;3],pub inner_cos:f32,pub outer_cos:f32,pub exponent:f32,
    pub ambient:[f32;3],pub diffuse:[f32;3],
}
#[derive(Debug,Clone)]
pub(super) struct Sample {pub weight:f32,pub direction:Option<[f32;3]>}
impl LightRecord {
    pub fn sample(&self,probe:[f32;3],precision:super::pc_float::Precision)->Result<Sample> {
        let mut result=Sample {weight:0.,direction:None};
        if self.flags&0x1000==0||self.flags&0x2000!=0 {return Ok(result);}
        let kind=self.flags&0xf;
        let (delta,distance)=super::pc_float::sh_distance(self.position,probe,precision)?;
        if distance<f32::EPSILON&&kind!=2 {return Ok(result);}
        let direction=super::pc_float::sh_normalize(delta,distance,precision)?;result.direction=Some(direction);
        let Some(falloff)=super::pc_float::sh_falloff(self.flags&0xf0,self.falloff,distance,precision)? else{return Ok(result)};
        let mut cone=1.;
        if kind==8 {
            let dot=super::pc_float::sh_spot_dot(self.direction,direction,precision)?;
            if dot<=self.inner_cos {
                if dot<self.outer_cos {return Ok(result);}
                let ratio=super::pc_float::sh_spot_ratio(dot,self.inner_cos,self.outer_cos,precision)?;
                cone=super::pc_float::sh_spot_power(ratio,self.exponent,precision)?;
            }
        }
        result.weight=super::pc_float::sh_sample_weight(self.dimmer,falloff,cone,precision)?;Ok(result)
    }
}
impl Snapshot {
    /// One already assigned native light, in original source order. A flagged
    /// raycast contribution requires its actual result; unflagged lights do
    /// not run a visibility query. Scene list construction remains upstream.
    pub fn accumulate_sample(&mut self,light:&LightRecord,probe:[f32;3],visibility:Option<f32>,precision:super::pc_float::Precision)->Result<bool> {
        let sample=light.sample(probe,precision)?;
        if sample.weight<=0. {return Ok(false);}
        let mut weight=sample.weight;
        if light.flags&0x4000!=0 {
            let visibility=visibility.ok_or_else(||anyhow::anyhow!("Native SH raycast result is absent"))?;
            ensure!(visibility.is_finite()&&(0. ..=1.).contains(&visibility),"Invalid native SH visibility");
            weight=super::pc_float::sh_visibility(weight,visibility,precision)?;
            if weight<=0. {return Ok(false);}
        }
        let kind=match light.flags&0xf {1=>super::d3d9_light::Kind::Ambient,2=>super::d3d9_light::Kind::Directional,4=>super::d3d9_light::Kind::Point,8=>super::d3d9_light::Kind::Spot,value=>anyhow::bail!("Unverified native SH light type {value}")};
        self.accumulate(&Contribution {kind,ambient:light.ambient,diffuse:light.diffuse,light_direction:light.direction,probe_direction:sample.direction.ok_or_else(||anyhow::anyhow!("Positive native SH sample lacks its direction"))?,weight},precision)?;
        Ok(true)
    }

    pub fn accumulate(&mut self,light:&Contribution,precision:super::pc_float::Precision)->Result<()> {
        let diffuse=if matches!(light.kind,super::d3d9_light::Kind::Ambient) {None}else {
            let direction=if matches!(light.kind,super::d3d9_light::Kind::Directional){light.light_direction.map(|v|-v)}else{light.probe_direction};
            Some((light.diffuse,super::pc_float::sh_project(direction,precision)?))
        };
        let mut next=self.coefficients;
        super::pc_float::sh_accumulate(&mut next,light.ambient,diffuse,light.weight,precision)?;
        self.coefficients=next;Ok(())
    }

    pub fn words(&self,name:&str,parameter:&super::d3d9_effect::Value)->Result<Option<Vec<u32>>> {
        let value=if let Some(index)=name.strip_prefix("SHCoeff") {
            if index.len()!=1||!matches!(index.as_bytes()[0],b'0'..=b'8'){return Ok(None);}
            Some(&self.coefficients[(index.as_bytes()[0]-b'0') as usize])
        } else {match name {"SHSpecCol"=>self.specular_color.as_ref(),"SHSpecDir"=>self.specular_direction.as_ref(),_=>None}};
        let Some(value)=value else{return Ok(None)};
        ensure!(parameter.kind==3&&parameter.class==1&&parameter.rows==1&&parameter.columns==3&&parameter.elements<=1,"Native SH attribute requires its float3 adapter");
        ensure!(value.iter().all(|v|v.is_finite()),"Nonfinite PC SH attribute");
        Ok(Some(value.iter().map(|v|v.to_bits()).collect()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parameter()->super::super::d3d9_effect::Value {super::super::d3d9_effect::Value {kind:3,class:1,rows:1,columns:3,elements:0,name:None,semantic:None,data:super::super::d3d9_effect::Data::Words(vec![])}}
    fn precision(word:u64)->super::super::pc_float::Precision {match word {0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()}}
    #[test]
    fn spotlight_power_matches_original_pc_x87_path() {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_cone.json")).unwrap();
        for row in fixture["cases"].as_array().unwrap() {
            let value=|name:&str|f32::from_bits(row[name].as_u64().unwrap() as u32);
            let actual=super::super::pc_float::sh_spot_power(value("ratio_word"),value("exponent_word"),precision(row["precision"].as_u64().unwrap())).unwrap();
            assert_eq!(actual.to_bits(),row["result_word"].as_u64().unwrap() as u32,"{row}");
        }
    }
    #[test]
    fn raycast_lights_require_real_visibility_and_preserve_small_visible_weights() {
        let mut snapshot=Snapshot {coefficients:[[0.;3];9],specular_color:None,specular_direction:None};
        let light=LightRecord {flags:0x5011,position:[1.,0.,0.],direction:[0.;3],dimmer:0.5,falloff:[1.,0.,0.],inner_cos:0.9,outer_cos:0.5,exponent:1.,ambient:[0.1;3],diffuse:[0.;3]};
        let precision=super::super::pc_float::Precision::Extended;
        assert!(snapshot.accumulate_sample(&light,[0.;3],None,precision).is_err());assert_eq!(snapshot.coefficients,[[0.;3];9]);
        assert!(!snapshot.accumulate_sample(&light,[0.;3],Some(0.),precision).unwrap());
        assert!(snapshot.accumulate_sample(&light,[0.;3],Some(0.001),precision).unwrap());assert!(snapshot.coefficients[0][0]>0.);
        let mut inactive=light;inactive.flags=0;assert!(!snapshot.accumulate_sample(&inactive,[0.;3],None,precision).unwrap());
    }
    #[test]
    fn matches_original_pc_light_sampling() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_sampling.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let vector=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let light=LightRecord {flags:0x1000|row["kind"].as_u64().unwrap() as u16|row["mode"].as_u64().unwrap() as u16,position:[0.;3],direction:[1.,0.,0.],dimmer:row["dimmer"].as_f64().unwrap() as f32,falloff:vector("falloff"),inner_cos:0.9,outer_cos:0.5,exponent:1.,ambient:[0.;3],diffuse:[0.;3]};
            let actual=light.sample(vector("probe"),precision(row["precision"].as_u64().unwrap())).unwrap();
            assert_eq!(actual.weight.to_bits(),row["weight_word"].as_u64().unwrap() as u32,"{row}");
            let words=actual.direction.unwrap_or([-123.,-456.,-789.]).map(f32::to_bits);
            assert_eq!(words.as_slice(),row["direction_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }
    }
    #[test]
    fn invalid_contribution_preserves_previous_snapshot_and_fp_environment() {
        let mut snapshot=Snapshot {coefficients:[[0.25;3];9],specular_color:None,specular_direction:None};
        let mut contribution=Contribution {kind:super::super::d3d9_light::Kind::Ambient,ambient:[0.1;3],diffuse:[0.;3],light_direction:[0.;3],probe_direction:[0.;3],weight:0.5};
        #[cfg(target_arch="x86_64")]
        let before=unsafe {let mut word=0u16;std::arch::asm!("fnstcw [{cw}]",cw=in(reg) &mut word,options(nostack,preserves_flags));word};
        snapshot.accumulate(&contribution,super::super::pc_float::Precision::Single).unwrap();
        #[cfg(target_arch="x86_64")]
        unsafe {let mut after=0u16;std::arch::asm!("fnstcw [{cw}]",cw=in(reg) &mut after,options(nostack,preserves_flags));assert_eq!(before,after);}
        let previous=snapshot.coefficients;contribution.weight=f32::NAN;
        assert!(snapshot.accumulate(&contribution,super::super::pc_float::Precision::Extended).is_err());assert_eq!(snapshot.coefficients,previous);
    }
    #[test]
    fn matches_both_original_pc_projection_routines() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_projection.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let vector=std::array::from_fn(|i|f32::from_bits(row["vector_words"][i].as_u64().unwrap() as u32));
            let actual=super::super::pc_float::sh_project(vector,precision(row["precision"].as_u64().unwrap())).unwrap();
            assert_eq!(actual.map(f32::to_bits).as_slice(),row["output"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }
    }
    #[test]
    fn matches_original_pc_task_accumulation() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_accumulation.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let initial=row["initial"].as_array().unwrap().iter().map(|v|f32::from_bits(v.as_u64().unwrap() as u32)).collect::<Vec<_>>();
            let mut snapshot=Snapshot {coefficients:std::array::from_fn(|i|initial[i*3..i*3+3].try_into().unwrap()),specular_color:None,specular_direction:None};
            let vector=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let kind=match row["kind"].as_u64().unwrap(){1=>super::super::d3d9_light::Kind::Ambient,2=>super::super::d3d9_light::Kind::Directional,4=>super::super::d3d9_light::Kind::Point,8=>super::super::d3d9_light::Kind::Spot,_=>unreachable!()};
            snapshot.accumulate(&Contribution {kind,ambient:vector("ambient"),diffuse:vector("diffuse"),light_direction:vector("light_direction"),probe_direction:vector("probe_direction"),weight:row["weight"].as_f64().unwrap() as f32},precision(row["precision"].as_u64().unwrap())).unwrap();
            assert_eq!(snapshot.coefficients.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),row["output"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }
    }
    #[test]
    fn matches_original_pc_coefficient_publication() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_publication.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let input=row["input"].as_array().unwrap().iter().map(|v|f32::from_bits(v.as_u64().unwrap() as u32)).collect::<Vec<_>>();
            let snapshot=Snapshot {coefficients:std::array::from_fn(|i|input[i*3..i*3+3].try_into().unwrap()),specular_color:None,specular_direction:None};
            let output=(0..9).flat_map(|i|snapshot.words(&format!("SHCoeff{i}"),&parameter()).unwrap().unwrap()).collect::<Vec<_>>();
            assert_eq!(output,row["output"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>());
            assert_eq!(row["result"],1);
        }
    }
    #[test]
    fn preserves_published_specular_values_and_explicit_absence()->Result<()> {
        let mut snapshot=Snapshot {coefficients:[[0.;3];9],specular_color:Some([0.25,0.5,0.75]),specular_direction:Some([2.,-3.,4.])};
        assert_eq!(snapshot.words("SHSpecDir",&parameter())?.unwrap(),[2f32.to_bits(),(-3f32).to_bits(),4f32.to_bits()]);
        assert_eq!(snapshot.words("SHSpecCol",&parameter())?.unwrap(),[0.25f32.to_bits(),0.5f32.to_bits(),0.75f32.to_bits()]);
        snapshot.specular_color=None;assert!(snapshot.words("SHSpecCol",&parameter())?.is_none());
        for name in ["shcoeff0","SHCoeff00","SHCoeff9","unrelated"] {assert!(snapshot.words(name,&parameter())?.is_none());}
        let mut array=parameter();array.elements=2;assert!(snapshot.words("SHCoeff0",&array).is_err());
        snapshot.coefficients[0][0]=f32::NAN;assert!(snapshot.words("SHCoeff0",&parameter()).is_err());Ok(())
    }
}
