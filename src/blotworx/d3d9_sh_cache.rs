//! PC CacheLightProperties9a8940 typed intermediate record. This is NOT the
//! SH task LightRecord: source colors are scaled255 and fields have another
//! layout. Conversion mirrors the native manager; scene registration remains
//! with the scene owner.
use anyhow::{ensure,Context,Result};
use super::{d3d9_light::Kind,pc_float::{self,Precision}};
#[derive(Debug,Clone,PartialEq)]
pub(super) struct Source {
    pub generate_runtime:bool,pub raycast:Option<bool>,pub subtractive:bool,
    /// Exact Translation/Rotation values returned by transformation properties;
    /// ancestor/world resolution is not inferred by this numeric adapter.
    pub translation:[f32;3],pub rotation:[f32;9],pub kind:Kind,
    pub dimmer:f32,pub ambient:[f32;3],pub diffuse:[f32;3],
    pub minimum_range:f32,pub maximum_range:f32,pub attenuation:[f32;3],
    pub inner_angle:f32,pub outer_angle:f32,pub exponent:f32,
}
#[derive(Debug,Clone)]
pub(super) struct Cached {
    pub position:[f32;3],pub direction:[f32;3],pub kind:Kind,
    pub dimmer:f32,pub ambient:[f32;3],pub diffuse:[f32;3],
    pub minimum_range:f32,pub maximum_range:f32,pub attenuation:[f32;3],
    pub inner_cos:f32,pub outer_cos:f32,pub exponent:f32,pub raycast:bool,
}
impl Source {
    /// Cache identity preserves signed zero and every authored float bit.
    pub fn same_bits(&self,other:&Self)->bool {
        fn words(s:&Source)->impl Iterator<Item=u32> {
            s.translation.into_iter().chain(s.rotation).chain(s.ambient).chain(s.diffuse).chain(s.attenuation)
                .chain([s.dimmer,s.minimum_range,s.maximum_range,s.inner_angle,s.outer_angle,s.exponent]).map(f32::to_bits)
        }
        self.generate_runtime==other.generate_runtime&&self.raycast==other.raycast&&self.subtractive==other.subtractive&&self.kind==other.kind&&words(self).eq(words(other))
    }
    /// Build from resolved component properties. The caller must resolve class
    /// inheritance first; omission alone is NOT proof of a native factory default.
    /// `factory_defaults` is explicit so raw cooked-scene callers cannot silently
    /// mistake a missing inherited property for zero.
    pub fn from_properties<'a>(
        light:impl Fn(&str)->Option<&'a serde_json::Value>,
        baking:impl Fn(&str)->Option<&'a serde_json::Value>,
        translation:[f32;3],rotation:[f32;9],factory_defaults:bool,
    )->Result<Option<Self>> {
        let boolean=|name:&str|->Result<bool>{
            match baking(name) {
                Some(v)=>v.as_bool().with_context(||format!("Invalid PC light boolean {name}")),
                None if factory_defaults=>Ok(false),
                None=>anyhow::bail!("Unresolved PC light property {name}"),
            }
        };
        if !boolean("GenerateRuntimeLight")? {return Ok(None);}
        ensure!(translation.iter().chain(&rotation).all(|v|v.is_finite()),"Nonfinite resolved PC light transform");
        let scalar=|property:Option<&serde_json::Value>,name:&str|->Result<f32>{
            match property {
                Some(v)=>{
                    let value=v.as_f64().with_context(||format!("Invalid PC light scalar {name}"))? as f32;
                    ensure!(value.is_finite(),"Nonfinite PC light scalar {name}");Ok(value)
                },
                None if factory_defaults=>Ok(0.),
                None=>anyhow::bail!("Unresolved PC light property {name}"),
            }
        };
        let color=|name:&str|->Result<[f32;3]>{
            match light(name) {
                Some(v)=>{
                    ensure!(["r","g","b"].iter().all(|channel|v.get(*channel).is_some()),"Incomplete PC light color {name}");
                    Ok([scalar(v.get("r"),name)?,scalar(v.get("g"),name)?,scalar(v.get("b"),name)?])
                },
                None if factory_defaults=>Ok([0.;3]),
                None=>anyhow::bail!("Unresolved PC light color {name}"),
            }
        };
        let kind=match light("Light Type").and_then(|v|v.as_str()) {
            Some("Ambient")=>Kind::Ambient,Some("Directional")=>Kind::Directional,
            Some("Point")=>Kind::Point,Some("Spot")=>Kind::Spot,
            other=>anyhow::bail!("Unresolved or unsupported PC Light Type {other:?}"),
        };
        Ok(Some(Self {generate_runtime:true,raycast:Some(boolean("RuntimeLightRayCast")?),
            subtractive:boolean("LightIsSubtractive")?,translation,rotation,kind,
            dimmer:scalar(light("Dimmer"),"Dimmer")?,ambient:color("Color (Ambient)")?,diffuse:color("Color (Diffuse)")?,
            minimum_range:scalar(baking("Min Falloff Range"),"Min Falloff Range")?,
            maximum_range:scalar(baking("Max Falloff Range"),"Max Falloff Range")?,
            attenuation:[scalar(light("Attenuation (Constant)"),"Attenuation (Constant)")?,scalar(light("Attenuation (Linear)"),"Attenuation (Linear)")?,scalar(light("Attenuation (Quadratic)"),"Attenuation (Quadratic)")?],
            inner_angle:scalar(light("Spot Angle (Inner)"),"Spot Angle (Inner)")?,
            outer_angle:scalar(light("Spot Angle (Outer)"),"Spot Angle (Outer)")?,
            exponent:scalar(light("Spot Exponent"),"Spot Exponent")?,
        }))
    }
    pub fn cache(&self,raycasts_enabled:bool,precision:Precision)->Result<Option<Cached>> {
        if !self.generate_runtime {return Ok(None);}
        let raycast=if raycasts_enabled {self.raycast.context("Native runtime-light raycast property is absent")?}else{false};
        ensure!(self.translation.iter().chain(&self.rotation).chain(&self.attenuation).all(|v|v.is_finite())&&[self.dimmer,self.minimum_range,self.maximum_range,self.exponent].iter().all(|v|v.is_finite()),"Nonfinite PC light cache source");
        Ok(Some(Cached {position:self.translation,direction:[self.rotation[2],self.rotation[5],self.rotation[8]],kind:self.kind,
            dimmer:self.dimmer,ambient:pc_float::sh_cached_color(self.ambient,self.subtractive,precision)?,diffuse:pc_float::sh_cached_color(self.diffuse,self.subtractive,precision)?,
            minimum_range:self.minimum_range,maximum_range:self.maximum_range,attenuation:self.attenuation,
            inner_cos:pc_float::sh_cached_spot_cos(self.inner_angle,precision)?,outer_cos:pc_float::sh_cached_spot_cos(self.outer_angle,precision)?,exponent:self.exponent,raycast}))
    }
}
impl Cached {
    /// PCacace0, with the zero-initialized record used by calleracaed0.
    /// A removed/invalid cache is handled separately by the lifecycle owner.
    pub fn to_record(&self)->super::d3d9_sh::LightRecord {
        let kind=match self.kind {Kind::Ambient=>1,Kind::Directional=>2,Kind::Point=>4,Kind::Spot=>8};
        let mut flags=0x1000|kind;
        if self.raycast {flags|=0x4000;}
        if self.dimmer<=f32::EPSILON {flags|=0x2000;}
        let falloff=if self.maximum_range>0. {
            flags|=0x40;[self.minimum_range,self.maximum_range,0.]
        }else if matches!(self.kind,Kind::Point|Kind::Spot) {
            flags|=0x20;self.attenuation
        }else{flags|=0x10;[1.,0.,0.]};
        let spot=matches!(self.kind,Kind::Spot);
        super::d3d9_sh::LightRecord {flags,position:self.position,direction:self.direction,dimmer:self.dimmer,falloff,
            inner_cos:if spot{self.inner_cos}else{0.},outer_cos:if spot{self.outer_cos}else{0.},exponent:if spot{self.exponent}else{0.},
            ambient:self.ambient,diffuse:self.diffuse}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_property_factory_defaults() {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_property_defaults.json")).unwrap();
        let light=serde_json::json!({"Light Type":"Point"});let baking=serde_json::json!({"GenerateRuntimeLight":true});
        let source=Source::from_properties(|name|light.get(name),|name|baking.get(name),[0.;3],[1.,0.,0.,0.,1.,0.,0.,0.,1.],true).unwrap().unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let name=case["name"].as_str().unwrap();
            let values:Vec<f32>=match name {
                "Dimmer"=>vec![source.dimmer],"Min Falloff Range"=>vec![source.minimum_range],"Max Falloff Range"=>vec![source.maximum_range],
                "Attenuation (Constant)"=>vec![source.attenuation[0]],"Attenuation (Linear)"=>vec![source.attenuation[1]],"Attenuation (Quadratic)"=>vec![source.attenuation[2]],
                "Spot Angle (Inner)"=>vec![source.inner_angle],"Spot Angle (Outer)"=>vec![source.outer_angle],"Spot Exponent"=>vec![source.exponent],
                "Color (Ambient)"=>source.ambient.to_vec(),"Color (Diffuse)"=>source.diffuse.to_vec(),
                "Translation"=>source.translation.to_vec(),"Rotation"=>source.rotation.to_vec(),
                "GenerateRuntimeLight"|"RuntimeLightRayCast"|"LightIsSubtractive"=>{assert_eq!(&case["bytes"].as_str().unwrap()[8..10],"00");continue;},
                // Other factories are retained as evidence, outside SH Source.
                _=>continue,
            };
            let bytes=case["bytes"].as_str().unwrap();
            for (i,value) in values.iter().enumerate() {
                let offset=8+i*8;let raw=(0..4).map(|b|u8::from_str_radix(&bytes[offset+b*2..offset+b*2+2],16).unwrap()).collect::<Vec<_>>();
                assert_eq!(value.to_bits(),u32::from_le_bytes(raw.try_into().unwrap()),"{name}");
            }
        }
    }
    #[test]
    fn resolved_properties_require_explicit_default_ownership() {
        let light=serde_json::json!({"Light Type":"Point","Color (Ambient)":{"r":0.2,"g":0.3,"b":0.4}});
        let baking=serde_json::json!({"GenerateRuntimeLight":true});
        let read=|defaults|Source::from_properties(|name|light.get(name),|name|baking.get(name),[0.;3],[1.,0.,0.,0.,1.,0.,0.,0.,1.],defaults);
        assert!(read(false).is_err());
        let source=read(true).unwrap().unwrap();
        assert_eq!(source.attenuation,[0.;3]);assert_eq!(source.dimmer,0.);
        assert_eq!(source.ambient,[0.2,0.3,0.4]);assert_eq!(source.raycast,Some(false));
        assert!(Source::from_properties(|_|None,|_|None,[f32::NAN;3],[f32::NAN;9],true).unwrap().is_none());
        let malformed=serde_json::json!({"Light Type":"Point","Color (Ambient)":{"r":0.2}});
        assert!(Source::from_properties(|name|malformed.get(name),|name|baking.get(name),[0.;3],[0.;9],true).is_err());
        assert!(Source::from_properties(|_|None,|name|baking.get(name),[0.;3],[0.;9],true).is_err());
    }
    #[test]
    fn original_pc_task_record_conversion() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_record_conversion.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let values=row["cache_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            let f=|i:usize|f32::from_bits(values[i]);let vector=|i:usize|[f(i),f(i+1),f(i+2)];
            let kind=match values[21]{0=>Kind::Ambient,1=>Kind::Directional,2=>Kind::Point,3=>Kind::Spot,_=>unreachable!()};
            let cache=Cached {position:vector(0),minimum_range:f(3),direction:vector(4),maximum_range:f(7),ambient:vector(8),diffuse:vector(12),attenuation:[f(11),f(15),f(16)],outer_cos:f(17),inner_cos:f(18),exponent:f(19),dimmer:f(20),kind,raycast:values[22]&0xff00!=0};
            let record=cache.to_record();let mut words=vec![0,0];
            words.extend(record.position.map(f32::to_bits));words.extend(record.direction.map(f32::to_bits));words.extend(record.ambient.map(f32::to_bits));words.extend(record.diffuse.map(f32::to_bits));words.push(record.dimmer.to_bits());words.extend(record.falloff.map(f32::to_bits));words.extend([record.inner_cos,record.outer_cos,record.exponent].map(f32::to_bits));words.push(record.flags as u32);
            assert_eq!(words,row["record_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }
    }
    #[test]
    fn original_pc_cached_property_values() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_cache.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let vector=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let rotation=std::array::from_fn(|i|row["rotation"][i].as_f64().unwrap() as f32);
            let kind=match row["kind"].as_u64().unwrap(){0=>Kind::Ambient,1=>Kind::Directional,2=>Kind::Point,3=>Kind::Spot,_=>unreachable!()};
            let source=Source {generate_runtime:true,raycast:Some(true),subtractive:row["subtractive"].as_bool().unwrap(),translation:vector("translation"),rotation,kind,dimmer:0.75,ambient:vector("ambient"),diffuse:vector("diffuse"),minimum_range:2.,maximum_range:8.,attenuation:[1.,0.1,0.01],inner_angle:20.,outer_angle:40.,exponent:1.};
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            let cached=source.cache(row["ray_config"].as_bool().unwrap(),precision).unwrap().unwrap();
            let mut words=vec![];words.extend(cached.position.map(f32::to_bits));words.push(cached.minimum_range.to_bits());words.extend(cached.direction.map(f32::to_bits));words.push(cached.maximum_range.to_bits());words.extend(cached.ambient.map(f32::to_bits));words.push(cached.attenuation[0].to_bits());words.extend(cached.diffuse.map(f32::to_bits));words.push(cached.attenuation[1].to_bits());words.extend([cached.attenuation[2],cached.outer_cos,cached.inner_cos,cached.exponent,cached.dimmer].map(f32::to_bits));words.push(row["kind"].as_u64().unwrap() as u32);words.push(1|((cached.raycast as u32)<<8));
            assert_eq!(words,row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
            assert_eq!(row["success"],1);
            let mut unrequested=source.clone();unrequested.generate_runtime=false;unrequested.translation=[f32::NAN;3];unrequested.raycast=None;
            assert!(unrequested.cache(true,precision).unwrap().is_none());
            let mut unknown=source;unknown.raycast=None;assert!(unknown.cache(true,precision).is_err());assert!(unknown.cache(false,precision).unwrap().is_some());
        }
    }
}
