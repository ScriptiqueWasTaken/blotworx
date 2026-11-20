//! Global values whose producers have explicit scene/render ownership.
use anyhow::{ensure,Result};
#[derive(Debug,Clone)]
pub(super) struct Lighting {
    minimum_color:[f32;3],specular_direction:[f32;3],grounding_shadow:Option<[f32;3]>,
}
#[derive(Debug,Clone,Copy)]
pub(super) enum ScenePass {Main,ToonOffscreen}
/// MSVC's original formatter rounds exact decimal halfway cases away from
/// zero. Rust's formatter uses ties-to-even. Test the binary rational exactly
/// rather than rounding value*1e6 before deciding whether it is a tie.
fn config_text(value:f64)->String {
    let bits=value.abs().to_bits();let exponent=((bits>>52)&0x7ff) as i32;
    let mantissa=(bits&((1u64<<52)-1))|if exponent!=0 {1u64<<52}else{0};
    let power=if exponent==0 {-1074}else{exponent-1023-52}+6;
    if power<0&&power> -128 {
        let numerator=mantissa as u128*15625;let shift=(-power) as u32;
        let mask=(1u128<<shift)-1;
        if numerator&mask==1u128<<(shift-1) {
            let rounded=(numerator>>shift)+1;
            return format!("{}{part}.{fraction:06}",if value.is_sign_negative(){"-"}else{""},part=rounded/1_000_000,fraction=rounded%1_000_000);
        }
    }
    format!("{value:.6}")
}
fn config_value(value:f32,rgb:bool,precision:super::pc_float::Precision)->Result<f32> {
    let value=super::pc_float::profile_config_source(value,rgb,precision)?;
    Ok(config_text(value).parse::<f32>()?.clamp(-f32::MAX,f32::MAX))
}
impl Lighting {
    /// Caller supplies the selected native pass's config RGB and direction.
    /// Mode selection, config resolution and scene overrides remain owners'
    /// responsibilities; no shader-name or default-light guesses occur here.
    pub fn new(minimum_rgb:[f32;3],direction:[f32;3],precision:super::pc_float::Precision)->Result<Self> {
        ensure!(minimum_rgb.iter().all(|v|v.is_finite()),"Nonfinite minimum color source");
        Ok(Self {minimum_color:minimum_rgb.map(|v|v/255.),specular_direction:super::pc_float::shader_light_direction(direction,precision)?,grounding_shadow:None})
    }
    /// Native b60dd7 config publication -> d7c140 parsing ->11434b0/11435d0.
    /// Mode is a scene-pass input, never inferred from a shader filename.
    pub fn from_profile(settings:&super::render_profile_settings::Settings,pass:ScenePass,precision:super::pc_float::Precision)->Result<Self> {
        use super::render_profile_settings::Value;
        let color=|name:&str|->Result<[f32;3]>{
            let Some(Value::Color(value))=settings.get(name) else {anyhow::bail!("Missing profile color {name}")};
            Ok([config_value(value[0],true,precision)?/255.,config_value(value[1],true,precision)?/255.,config_value(value[2],true,precision)?/255.])
        };
        let (minimum_color,grounding_shadow)=match pass {
            ScenePass::Main=>([0.;3],color("InertGroundingShadowColour")?),
            ScenePass::ToonOffscreen=>(color("ToonMinimumDiffuseLighting")?,color("ToonGroundingShadowColour")?),
        };
        let Some(Value::Point3(direction))=settings.get("StaticLightingSpecularDirection") else {anyhow::bail!("Missing profile specular direction")};
        let direction=[config_value(direction[0],false,precision)?,config_value(direction[1],false,precision)?,config_value(direction[2],false,precision)?];
        Ok(Self {minimum_color,grounding_shadow:Some(grounding_shadow),specular_direction:super::pc_float::shader_light_direction(direction,precision)?})
    }
    pub fn resolve(&self,name:&str)->Option<super::d3d9_predefined::Value> {
        let vector=match name {"MinimumColourLevel"=>self.minimum_color,"StaticLightingSpecularDirection"=>self.specular_direction,"GroundingShadowMult"=>self.grounding_shadow?,_=>return None};
        Some(super::d3d9_predefined::Value::Vector([vector[0],vector[1],vector[2],0.]))
    }
}
/// PC d74b80 publishes the selected target's width, height and reciprocals.
/// Selection of application/default/offscreen dimensions belongs to the caller.
/// A viewport's subrectangle is not a substitute for the active target size.
pub(super) fn target_dimensions(extent:[u32;2])->Result<[f32;4]> {
    ensure!(extent.iter().all(|&v|v>0),"Active original render target has a zero extent");
    let [width,height]=extent.map(|v|v as f32);
    Ok([width,height,1./width,1./height])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_config_and_shader_globals_match_native_pipeline()->Result<()> {
        use super::super::render_profile_settings::{Settings,Value};
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_publication.json"))?;
        for row in fixture["formats"].as_array().unwrap(){assert_eq!(config_text(row["input"].as_f64().unwrap()),row["text"].as_str().unwrap(),"{row}");}
        let defaults=Settings::read(|_|None,true)?;
        for row in fixture["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()};
            let settings=Settings::read(|name|{
                let values=row["inputs"].get(name)?;let f=|i:usize|f32::from_bits(values[i].as_u64().unwrap() as u32);
                Some(match defaults.get(name)? {
                    Value::Float(_)=>serde_json::json!(f(0)),Value::Color(_)=>serde_json::json!({"r":f(0),"g":f(1),"b":f(2)}),
                    Value::Point3(_)=>serde_json::json!({"x":f(0),"y":f(1),"z":f(2)}),Value::Point2(_)=>serde_json::json!({"x":f(0),"y":f(1)}),_=>return None,
                })
            },true)?;
            for (pass,base) in [(ScenePass::Main,1),(ScenePass::ToonOffscreen,3)] {
                let lighting=Lighting::from_profile(&settings,pass,precision)?;
                for (name,index) in [("StaticLightingSpecularDirection",0),("MinimumColourLevel",base),("GroundingShadowMult",base+1)] {
                    let Some(super::super::d3d9_predefined::Value::Vector(values))=lighting.resolve(name) else{panic!("Missing {name}")};
                    for lane in 0..3 {assert_eq!(values[lane].to_bits(),row["publications"][index]["words"][lane].as_u64().unwrap() as u32,"{name}, {pass:?}, {precision:?}");}
                }
            }
        }Ok(())
    }
    #[test]
    fn lighting_globals_match_original_pc_publication()->Result<()> {
        for (reference,name) in [
            (include_str!("../../tools/blotworx/fixtures/pc_static_light_direction.json"),"StaticLightingSpecularDirection"),
            (include_str!("../../tools/blotworx/fixtures/pc_minimum_color.json"),"MinimumColourLevel"),
        ] {
            let reference:serde_json::Value=serde_json::from_str(reference)?;
            for case in reference["cases"].as_array().unwrap() {
                let input=std::array::from_fn(|i|case["input"][i].as_f64().unwrap() as f32);
                let precision=match case["precision"].as_u64().unwrap() {0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,_=>super::super::pc_float::Precision::Extended};
                let lighting=if name=="MinimumColourLevel" {Lighting::new(input,[0.,0.,1.],precision)?}else{Lighting::new([0.;3],input,precision)?};
                let Some(super::super::d3d9_predefined::Value::Vector(values))=lighting.resolve(name) else {panic!("Missing lighting binding")};
                for lane in 0..3 {assert_eq!(values[lane].to_bits(),case["words"][lane].as_u64().unwrap() as u32,"{name}: {case}, lane {lane}");}
            }
        }Ok(())
    }
    #[test]
    fn dimensions_match_original_pc_target_publication()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_target_dimensions.json"))?;
        for case in reference["cases"].as_array().unwrap() {
            let extent=std::array::from_fn(|i|case["extent"][i].as_u64().unwrap() as u32);
            for (lane,value) in target_dimensions(extent)?.iter().enumerate() {
                assert_eq!(value.to_bits(),case["words"][lane].as_u64().unwrap() as u32,"{case}, lane {lane}");
            }
        }
        assert!(target_dimensions([0,1]).is_err());Ok(())
    }
}
