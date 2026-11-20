//! PC RenderSceneEffectsComponent registered settings and verified defaults.
//! Factory words retain original float bits. This does not implement profile
//! interpolation or publish desired values prematurely as current renderer data.
use anyhow::{ensure,Context,Result};
use std::sync::Arc;
#[derive(Debug,Clone)]
pub(super) enum Value {Float(f32),Int(i32),Color([f32;3]),Point2([f32;2]),Point3([f32;3]),Text(Option<Arc<str>>)}
#[derive(Clone,Copy)]
enum DefaultValue {Float(u32),Int(i32),Color([u32;3]),Point2([u32;2]),Point3([u32;3]),Text}
struct Spec {name:&'static str,value:DefaultValue}
const SPECS:&[Spec]=&[
    Spec {name:"FogStart",value:DefaultValue::Float(0x00000000)},
    Spec {name:"FogEnd",value:DefaultValue::Float(0x00000000)},
    Spec {name:"FogColor",value:DefaultValue::Color([0x00000000,0x00000000,0x00000000])},
    Spec {name:"FogType",value:DefaultValue::Int(0)},
    Spec {name:"Gen3FogDistanceStart",value:DefaultValue::Float(0x00000000)},
    Spec {name:"Gen3FogDistanceEnd",value:DefaultValue::Float(0x42c80000)},
    Spec {name:"Gen3FogDistanceFalloffPower",value:DefaultValue::Float(0x40200000)},
    Spec {name:"Gen3FogHeightStart",value:DefaultValue::Float(0x00000000)},
    Spec {name:"Gen3FogHeightEnd",value:DefaultValue::Float(0x41200000)},
    Spec {name:"Gen3FogHeightFalloffPower",value:DefaultValue::Float(0x40000000)},
    Spec {name:"Gen3FogColorLower",value:DefaultValue::Color([0x3f800000,0x3f800000,0x3f800000])},
    Spec {name:"Gen3FogColorUpper",value:DefaultValue::Color([0x3f800000,0x3f800000,0x3f800000])},
    Spec {name:"Gen3FogDensityLower",value:DefaultValue::Float(0x3f4ccccd)},
    Spec {name:"Gen3FogDensityUpper",value:DefaultValue::Float(0x3ecccccd)},
    Spec {name:"Gen3ToneBrightness",value:DefaultValue::Float(0x41a00000)},
    Spec {name:"Gen3ToneContrast",value:DefaultValue::Float(0x41700000)},
    Spec {name:"Gen3ToneSaturation",value:DefaultValue::Float(0x40a00000)},
    Spec {name:"BloomThreshold",value:DefaultValue::Color([0x00000000,0x00000000,0x00000000])},
    Spec {name:"BloomScale",value:DefaultValue::Color([0x00000000,0x00000000,0x00000000])},
    Spec {name:"BloomOffsetScale",value:DefaultValue::Float(0x00000000)},
    Spec {name:"Range",value:DefaultValue::Float(0x42c80000)},
    Spec {name:"Priority",value:DefaultValue::Int(0)},
    Spec {name:"InterpolateTime",value:DefaultValue::Float(0x40000000)},
    Spec {name:"Gen3BloomThreshold",value:DefaultValue::Float(0x3f000000)},
    Spec {name:"Gen3BloomIntensity",value:DefaultValue::Float(0x3f000000)},
    Spec {name:"Gen3BloomBlur",value:DefaultValue::Float(0x40000000)},
    Spec {name:"NeutralLUTNifFilePath",value:DefaultValue::Text},
    Spec {name:"ColorSwapWeight",value:DefaultValue::Float(0x3f800000)},
    Spec {name:"InertGroundingShadowColour",value:DefaultValue::Color([0x3f000000,0x3f000000,0x3f000000])},
    Spec {name:"ToonGroundingShadowColour",value:DefaultValue::Color([0x3f2e978d,0x3f41cac1,0x3f5df3b6])},
    Spec {name:"ToonMinimumDiffuseLighting",value:DefaultValue::Color([0x3e418937,0x3e78d4fe,0x3eb0a3d7])},
    Spec {name:"ToonBrightnessBoost",value:DefaultValue::Float(0x3f400000)},
    Spec {name:"StaticLightingSpecularDirection",value:DefaultValue::Point3([0x3f000000,0x3f000000,0x3f000000])},
    Spec {name:"WindMaxFrequency",value:DefaultValue::Float(0x41200000)},
    Spec {name:"WindMaxInEffectTime",value:DefaultValue::Float(0x41a00000)},
    Spec {name:"WindMaxAmplitude",value:DefaultValue::Float(0x3dcccccd)},
    Spec {name:"WindDirection",value:DefaultValue::Point2([0x3f800000,0x3f800000])},
    Spec {name:"WindMaxTransitionTime",value:DefaultValue::Float(0x40a00000)},
    Spec {name:"FilmGrainVignetteNifFilePath",value:DefaultValue::Text},
    Spec {name:"FilmGrainNoiseNifFilePath",value:DefaultValue::Text},
    Spec {name:"FilmGrainBlackNoiseTolerance",value:DefaultValue::Float(0x3e4ccccd)},
    Spec {name:"FilmGrainBlackNoiseAmount",value:DefaultValue::Float(0x3ecccccd)},
    Spec {name:"FilmGrainBlackNoiseRNGTime",value:DefaultValue::Int(1)},
    Spec {name:"FilmGrainWhiteNoiseTolerance",value:DefaultValue::Float(0x3e99999a)},
    Spec {name:"FilmGrainWhiteNoiseAmount",value:DefaultValue::Float(0x3ecccccd)},
    Spec {name:"FilmGrainWhiteNoiseRNGTime",value:DefaultValue::Int(1)},
    Spec {name:"FilmGrainFadeSpeed",value:DefaultValue::Float(0x42480000)},
    Spec {name:"FilmGrainFadeStartValue",value:DefaultValue::Float(0x3f666666)},
    Spec {name:"FilmGrainFadeValueRange",value:DefaultValue::Float(0x3d4ccccd)},
    Spec {name:"TouchSelectionPaintGlowColor",value:DefaultValue::Color([0x3e428f5c,0x3ed70a3d,0x3f451eb8])},
    Spec {name:"TouchSelectionThinnerGlowColor",value:DefaultValue::Color([0x00000000,0x3f800000,0x00000000])},
    Spec {name:"TouchSelectionWidthMain",value:DefaultValue::Float(0x3f000000)},
    Spec {name:"TouchSelectionWidthHighlight",value:DefaultValue::Float(0x40400000)},
    Spec {name:"TouchSelectionEffectAlpha",value:DefaultValue::Float(0x3f19999a)},
    Spec {name:"TouchSelectionMainEffectStrength",value:DefaultValue::Float(0x3f800000)},
    Spec {name:"TouchSelectionHighlightStrength",value:DefaultValue::Float(0x3f800000)},
];
impl DefaultValue {
    fn value(self)->Value {match self {
        Self::Float(v)=>Value::Float(f32::from_bits(v)),Self::Int(v)=>Value::Int(v),
        Self::Color(v)=>Value::Color(v.map(f32::from_bits)),Self::Point2(v)=>Value::Point2(v.map(f32::from_bits)),Self::Point3(v)=>Value::Point3(v.map(f32::from_bits)),Self::Text=>Value::Text(None),
    }}
    fn decode(self,value:&serde_json::Value)->Result<Value> {
        let number=|value:&serde_json::Value|->Result<f32>{let value=value.as_f64().context("Expected numeric render property")? as f32;ensure!(value.is_finite(),"Nonfinite render property");Ok(value)};
        let vector=|names:&[&str]|->Result<Vec<f32>>{names.iter().map(|name|number(value.get(*name).with_context(||format!("Missing vector field {name}"))?)).collect()};
        Ok(match self {
            Self::Float(_)=>Value::Float(number(value)?),Self::Int(_)=>Value::Int(i32::try_from(value.as_i64().context("Expected integer render property")?)?),
            Self::Color(_)=>Value::Color(vector(&["r","g","b"])?.try_into().unwrap()),
            Self::Point2(_)=>Value::Point2(vector(&["x","y"])?.try_into().unwrap()),
            Self::Point3(_)=>Value::Point3(vector(&["x","y","z"])?.try_into().unwrap()),
            Self::Text=>Value::Text(Some(Arc::from(value.as_str().context("Expected render texture path")?))),
        })
    }
}
impl Value {
    fn same_bits(&self,other:&Self)->bool {
        match (self,other) {
            (Self::Float(a),Self::Float(b))=>a.to_bits()==b.to_bits(),(Self::Int(a),Self::Int(b))=>a==b,
            (Self::Color(a),Self::Color(b))|(Self::Point3(a),Self::Point3(b))=>a.map(f32::to_bits)==b.map(f32::to_bits),
            (Self::Point2(a),Self::Point2(b))=>a.map(f32::to_bits)==b.map(f32::to_bits),(Self::Text(a),Self::Text(b))=>a==b,_=>false,
        }
    }
    pub fn json(&self)->serde_json::Value {match self {
        Self::Float(v)=>serde_json::json!(v),Self::Int(v)=>serde_json::json!(v),Self::Color(v)|Self::Point3(v)=>serde_json::json!(v),Self::Point2(v)=>serde_json::json!(v),Self::Text(v)=>serde_json::json!(v.as_deref()),
    }}
}
#[derive(Debug,Clone)]
pub(super) struct Settings {pub(super) values:Vec<Value>}
impl Settings {
    pub fn read(mut get:impl FnMut(&str)->Option<serde_json::Value>,factory_defaults:bool)->Result<Self> {
        let values=SPECS.iter().map(|spec|match get(spec.name) {
            Some(value)=>spec.value.decode(&value).with_context(||format!("Render property {}",spec.name)),
            None if factory_defaults=>Ok(spec.value.value()),None=>anyhow::bail!("Unresolved render property {} without factory ownership",spec.name),
        }).collect::<Result<_>>()?;Ok(Self {values})
    }
    pub fn get(&self,name:&str)->Option<&Value> {SPECS.iter().position(|spec|spec.name==name).map(|index|&self.values[index])}
    pub fn entries(&self)->impl Iterator<Item=(&'static str,&Value)> {SPECS.iter().zip(&self.values).map(|(spec,value)|(spec.name,value))}
    pub fn same_bits(&self,other:&Self)->bool {self.values.len()==other.values.len()&&self.values.iter().zip(&other.values).all(|(a,b)|a.same_bits(b))}
    pub fn summary(&self)->serde_json::Value {serde_json::Value::Object(SPECS.iter().zip(&self.values).map(|(spec,value)|(spec.name.to_owned(),value.json())).collect())}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pc_registered_factory_defaults()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_defaults.json"))?;
        assert_eq!(SPECS.len(),fixture["properties"].as_array().unwrap().len());
        for (spec,property) in SPECS.iter().zip(fixture["properties"].as_array().unwrap()) {
            assert_eq!(spec.name,property["name"].as_str().unwrap());
            let bytes=property["factory_bytes"].as_str().unwrap();let bytes=(8..bytes.len()).step_by(2).map(|i|u8::from_str_radix(&bytes[i..i+2],16).unwrap()).collect::<Vec<_>>();
            let words:Vec<_>=bytes.chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())).collect();
            let actual=match spec.value.value(){Value::Float(v)=>vec![v.to_bits()],Value::Int(v)=>vec![v as u32],Value::Color(v)|Value::Point3(v)=>v.map(f32::to_bits).to_vec(),Value::Point2(v)=>v.map(f32::to_bits).to_vec(),Value::Text(v)=>{assert!(v.is_none());vec![0]}};
            assert_eq!(actual,words,"{}",spec.name);
        }Ok(())
    }
    #[test]
    fn native_activation_reads_cover_the_registered_schema()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_layout.json"))?;
        let mut seen=std::collections::BTreeSet::new();
        for getter in fixture["getters"].as_array().unwrap() {
            let index=getter["index"].as_u64().unwrap() as usize;let spec=&SPECS[index];assert!(seen.insert(index));
            assert_eq!(spec.name,getter["name"].as_str().unwrap());
            let slot=match spec.value {DefaultValue::Float(_)=>53,DefaultValue::Int(_)=>51,DefaultValue::Color(_)=>42,DefaultValue::Point3(_)=>45,DefaultValue::Point2(_)=>46,DefaultValue::Text=>47};
            assert_eq!(slot,getter["slot"].as_u64().unwrap(),"{}",spec.name);
        }
        assert_eq!(seen,(0..SPECS.len()).filter(|index|!matches!(SPECS[*index].name,"Range"|"Priority")).collect());Ok(())
    }
    #[test]
    fn null_paths_and_signed_zero_keep_their_source_semantics()->Result<()> {
        let original=Settings::read(|_|None,true)?;
        let authored=Settings::read(|name|match name {"FogStart"=>Some(serde_json::json!(-0.0)),"NeutralLUTNifFilePath"=>Some(serde_json::json!("")),_=>None},true)?;
        assert!(!original.same_bits(&authored));
        assert!(matches!(original.get("NeutralLUTNifFilePath"),Some(Value::Text(None))));
        assert!(matches!(authored.get("NeutralLUTNifFilePath"),Some(Value::Text(Some(v))) if v.is_empty()));
        assert!(Settings::read(|_|None,false).is_err());
        assert!(Settings::read(|name|(name=="FogColor").then(||serde_json::json!({"r":1.,"g":0.})),true).is_err());Ok(())
    }
}
