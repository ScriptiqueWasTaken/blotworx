//! Original PC d45bb0 light values. Assignment is owned by the scene; this
//! adapter never chooses nearest preview lights or fabricates missing ones.
use anyhow::{ensure,Context,Result};
use super::d3d9_predefined::Value;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) enum Kind {Ambient,Point,Directional,Spot}
#[derive(Debug,Clone)]
pub(super) struct Light {
    pub kind:Kind,
    pub dimmer:f32,
    pub ambient:[f32;4],
    pub diffuse:[f32;3],
    pub specular:[f32;3],
    pub position:[f32;3],
    /// Native world direction. Point-light direction is mesh-dependent and
    /// must not be substituted with this field.
    pub direction:[f32;3],
    /// Native point/spot fields at12c..138, including the fourth lane.
    /// These are not reconstructed from the preview attenuation radius.
    pub attenuation:Option<[f32;4]>,
    /// Native spot fields at150,154,158: exponent, outer cosine, inner
    /// cosine. The shader order is inner cosine, outer cosine, exponent.
    pub spot_parameters:Option<[f32;3]>,
}
#[derive(Debug,PartialEq,Eq)]
pub(super) struct Gen3Slot {pub slot:u32,pub switch:u8,pub runtime:bool,pub shadow_map_id:i32}
impl Light {
    /// RecreateLight's explicit slot coercion and diffuse/switch changes.
    /// ShadowMapID is returned to its ExtraData owner, not attached to a
    /// guessed texture or treated as a shadow-generator pointer.
    pub fn apply_gen3_slot(&mut self,authored:u32,switch:u8)->Gen3Slot {
        let slot=if matches!(self.kind,Kind::Point){4}else{authored};
        let runtime=slot>1;
        if !runtime {self.diffuse=[0.;3];}
        Gen3Slot {slot,switch:if runtime{switch}else{0},runtime,shadow_map_id:match slot{0=>0,2=>1,3=>2,_=>-1}}
    }
    pub fn update_spot_angles(&mut self,inner:f32,outer:f32,exponent:f32,precision:super::pc_float::Precision)->Result<()> {
        ensure!(matches!(self.kind,Kind::Spot)&&exponent.is_finite(),"Native spot properties require a spot light and finite exponent");
        let (_,inner)=super::pc_float::spot_angle(inner,precision)?;
        let (_,outer)=super::pc_float::spot_angle(outer,precision)?;
        self.spot_parameters=Some([exponent,outer,inner]);Ok(())
    }
    /// Original PC light constructor fields; world transform remains an
    /// explicit source input, since the scene component updates it separately.
    pub fn new(kind:Kind,position:[f32;3],direction:[f32;3])->Result<Self> {
        ensure!(position.iter().chain(&direction).all(|v|v.is_finite()),"Nonfinite native light transform");
        Ok(Self {kind,dimmer:1.,ambient:[1.;4],diffuse:[1.;3],specular:[1.;3],position,direction,
            attenuation:matches!(kind,Kind::Point|Kind::Spot).then_some([0.,1.,0.,0.]),
            spot_parameters:matches!(kind,Kind::Spot).then_some([1.;3])})
    }
    /// Raw component properties, prior to light assignment. Never derives
    /// original coefficients from the generic preview's radius approximation.
    pub fn update_distance_attenuation(&mut self,model:&str,range:f32,shape:f32,legacy:[f32;3],precision:super::pc_float::Precision)->Result<()> {
        ensure!(matches!(self.kind,Kind::Point|Kind::Spot),"Distance attenuation requires a native point/spot light");
        let previous=self.attenuation.context("Native constructor/previous attenuation fields are absent")?;
        let model=match model {"Deprecated"=>0,"Constant"=>1,"Linear"=>2,"Quadratic"=>3,"Blend"=>4,_=>5};
        self.attenuation=Some(super::pc_float::point_attenuation(model,range,shape,legacy,previous,precision)?);Ok(())
    }
    pub fn words(&self,property:&str,space:Option<&str>,parameter:&super::d3d9_effect::Value)->Result<Option<Vec<u32>>> {
        self.resolve(property,space)?.map(|value|light_words(value,property,parameter)).transpose()
    }
    pub fn resolve(&self,property:&str,space:Option<&str>)->Result<Option<Value>> {
        let rgba=|v:[f32;3]|[v[0],v[1],v[2],1.];
        let color=|v:[f32;4],count:usize| {
            let mut result=v;
            // Each original multiplication has a float store; no accumulation,
            // normalization, clamping or gamma conversion belongs here.
            for lane in &mut result[..count] {*lane *= self.dimmer;}
            result
        };
        let value=match property.to_ascii_lowercase().as_str() {
            "dimmer"=>[self.dimmer,0.,0.,0.],
            "undimmedambient"=>self.ambient,
            "undimmeddiffuse"=>rgba(self.diffuse),
            "undimmedspecular"=>rgba(self.specular),
            "ambient"=>color(self.ambient,4),
            "diffuse"=>color(rgba(self.diffuse),3),
            "specular"=>color(rgba(self.specular),3),
            "attenuation"=>match self.kind {
                Kind::Ambient|Kind::Directional=>[1.,0.,0.,0.],
                Kind::Point|Kind::Spot=>self.attenuation.context("Native light attenuation source is absent")?,
            },
            "spotattenuation"=>match self.kind {
                Kind::Spot=>{
                    let source=self.spot_parameters.context("Native spot attenuation source is absent")?;
                    [source[2],source[1],source[0],0.]
                },
                Kind::Ambient|Kind::Point|Kind::Directional=>[0.987,0.93,0.,0.],
            },
            "position"|"worldposition"=>{
                ensure!(property.eq_ignore_ascii_case("worldposition")||space.is_some_and(|s|s.eq_ignore_ascii_case("world")),"Native light position needs its verified space adapter");
                ensure!(!matches!(self.kind,Kind::Directional),"Directional position requires the native mesh-relative adapter");
                rgba(self.position)
            },
            "direction"|"worlddirection"=>{
                ensure!(property.eq_ignore_ascii_case("worlddirection")||space.is_some_and(|s|s.eq_ignore_ascii_case("world")),"Native light direction needs its verified space adapter");
                ensure!(!matches!(self.kind,Kind::Point|Kind::Ambient),"Point/ambient direction requires the native mesh-relative adapter");
                rgba(self.direction)
            },
            _=>return Ok(None),
        };
        ensure!(value.iter().all(|v|v.is_finite()),"Nonfinite original light value");
        Ok(Some(Value::Vector(value)))
    }
}

/// A mesh's ordered NiDynamicEffectState list. None in Inputs means unknown
/// ownership; an explicitly empty State means the mesh has no assigned lights.
#[derive(Debug,Clone)]
pub(super) struct Slot {
    pub effect_type:u8,
    pub switch:u8,
    pub light:Option<std::sync::Arc<Light>>,
}
#[derive(Debug,Clone)]
pub(super) struct State {pub slots:Vec<Slot>}
/// PC4ce330: normal slots precede shadow slots. GeneralLight has a distinct
/// second scan that includes shadow effects without retesting their switch.
/// Preserve this observed behavior rather than sorting by distance/type.
fn select(slots:&[Slot],kind:u8,index:u32)->Option<usize> {
    if !(1..=7).contains(&kind) {return None;}
    let mut remaining=index as usize;
    for (i,slot) in slots.iter().enumerate() {
        let matches=if kind==1 {slot.effect_type>=1}else{slot.effect_type==kind};
        if slot.switch&1!=0&&matches {
            if remaining==0 {return Some(i);}remaining-=1;
        }
    }
    if kind<=4 {
        for (i,slot) in slots.iter().enumerate() {
            let matches=if kind==1 {(5..=7).contains(&slot.effect_type)}else{slot.effect_type==kind+3&&slot.switch&1!=0};
            if matches {if remaining==0 {return Some(i);}remaining-=1;}
        }
    }
    None
}
/// Fixed explicit/suffixed object requests. Bare automatic indices depend on
/// parser registration order; that owner must supply the resolved index.
fn request(name:&str,index:Option<i32>)->Result<Option<(u8,u32)>> {
    for (kind,prefix) in [(1,"GeneralLight"),(2,"PointLight"),(3,"DirectionalLight"),(4,"SpotLight"),(5,"ShadowPointLight"),(6,"ShadowDirectionalLight"),(7,"ShadowSpotLight")] {
        let Some(start)=name.get(..prefix.len()) else{continue};
        if !start.eq_ignore_ascii_case(prefix) {continue;}
        let suffix=&name[prefix.len()..];
        let parsed=if suffix.is_empty() {None}else {
            let suffix=suffix.trim_start_matches(|c|matches!(c,' '|'\t'|'\r'|'\n'|'\u{000b}'|'\u{000c}'));
            let (negative,digits)=if let Some(d)=suffix.strip_prefix('-'){(true,d)}else{(false,suffix.strip_prefix('+').unwrap_or(suffix))};
            if digits.is_empty()||!digits.bytes().all(|c|c.is_ascii_digit()) {return Ok(None);}
            let Ok(value)=digits.parse::<u32>() else{anyhow::bail!("Native object index overflow needs its CRT conversion adapter")};
            if value>i32::MAX as u32 {anyhow::bail!("Native object index exceeds verified signed range");}
            if negative&&value!=0 {return Ok(None);}
            Some(value)
        };
        let resolved=match (index,parsed) {(Some(value),_)=>value as u32,(None,Some(value))=>value,(None,None)=>anyhow::bail!("Automatic native light index requires registration-order ownership")};
        return Ok(Some((kind,resolved)));
    }
    Ok(None)
}
fn null_value(property:&str,space:Option<&str>)->Result<Option<Value>> {
    let vector=match property.to_ascii_lowercase().as_str() {
        "dimmer"=>[0.;4],
        "undimmedambient"|"undimmeddiffuse"|"undimmedspecular"|"ambient"|"diffuse"|"specular"|"position"|"worldposition"|"modelposition"=>[0.,0.,0.,1.],
        "direction"|"worlddirection"|"modeldirection"=>[1.,0.,0.,1.],
        "spotattenuation"=>[-1.,0.,0.,0.],
        "attenuation"=>[1.,0.,0.,0.],
        "worldtransform"|"modeltransform"=>return Ok(Some(Value::Matrix(std::array::from_fn(|i|if i%5==0 {1.}else{0.})))),
        _=>return Ok(None),
    };
    if property.eq_ignore_ascii_case("position")||property.eq_ignore_ascii_case("direction") {
        ensure!(space.is_some_and(|s|s.eq_ignore_ascii_case("world")||s.eq_ignore_ascii_case("object")),"Null light needs a verified space mapping");
    }
    Ok(Some(Value::Vector(vector)))
}
fn light_words(value:Value,property:&str,parameter:&super::d3d9_effect::Value)->Result<Vec<u32>> {
    if property.eq_ignore_ascii_case("dimmer") {
        ensure!(parameter.kind==3&&parameter.class==0&&parameter.rows==1&&parameter.columns==1&&parameter.elements<=1,"Native light dimmer requires a scalar float parameter");
        let Value::Vector(vector)=value else{unreachable!()};return Ok(vec![vector[0].to_bits()]);
    }
    value.words(parameter)
}
impl State {
    pub fn words(&self,object:&str,index:Option<i32>,property:&str,space:Option<&str>,parameter:&super::d3d9_effect::Value)->Result<Option<Vec<u32>>> {
        let Some((kind,index))=request(object,index)? else{return Ok(None)};
        let value=if let Some(selected)=select(&self.slots,kind,index) {
            let slot=&self.slots[selected];
            let light=slot.light.as_ref().context("Assigned native light snapshot is absent")?;
            ensure!(matches!((slot.effect_type,light.kind),(0,Kind::Ambient)|(2|5,Kind::Point)|(3|6,Kind::Directional)|(4|7,Kind::Spot)),"Assigned native light type differs from its snapshot");
            light.resolve(property,space)?
        } else {
            // Original d2d360 returns0x60 but STILL invokes SetFloat/Vector/
            // Matrix with these d45bb0 values. This is explicit absence,
            // never a fallback for unknown assignment or missing source data.
            null_value(property,space)?
        };
        value.map(|v|light_words(v,property,parameter)).transpose()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gen3_slot_changes() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_slot.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let kind=match row["kind"].as_u64().unwrap(){0=>Kind::Ambient,2=>Kind::Point,3=>Kind::Directional,4=>Kind::Spot,_=>unreachable!()};
            let mut light=Light::new(kind,[0.;3],[1.,0.,0.]).unwrap();light.diffuse=[0.25,0.5,0.75];
            let actual=light.apply_gen3_slot(row["slot"].as_u64().unwrap() as u32,1);
            assert_eq!(actual.slot,row["resolved_slot"].as_u64().unwrap() as u32);
            assert_eq!(actual.switch,row["switch"].as_u64().unwrap() as u8);
            assert_eq!(actual.runtime,row["runtime"].as_u64().unwrap()!=0);
            assert_eq!(actual.shadow_map_id,row["shadow_map_id"].as_u64().unwrap() as u32 as i32);
            assert_eq!(light.diffuse.map(f32::to_bits).as_slice(),row["diffuse"].as_array().unwrap().iter().map(|w|w.as_u64().unwrap() as u32).collect::<Vec<_>>());
        }
    }
    #[test]
    fn unequal_inner_outer_angles_match_native_property_packing() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_spot_angle_order.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()};
            let mut light=Light::new(Kind::Spot,[0.;3],[1.,0.,0.]).unwrap();
            light.update_spot_angles(row["inner"].as_f64().unwrap() as f32,row["outer"].as_f64().unwrap() as f32,row["exponent"].as_f64().unwrap() as f32,precision).unwrap();
            assert_eq!(light.spot_parameters.unwrap().map(f32::to_bits).as_slice(),row["parameters"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>());
            let Some(Value::Vector(shader))=light.resolve("SpotAttenuation",None).unwrap() else{unreachable!()};
            assert_eq!(shader.map(f32::to_bits).as_slice(),row["shader_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>());
        }
    }
    #[test]
    fn native_spot_half_angles_and_cosines() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_spot_angle.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()};
            let angle=row["angle"].as_f64().unwrap() as f32;
            let (half,cosine)=super::super::pc_float::spot_angle(angle,precision).unwrap();
            assert_eq!(half.to_bits(),row["inner_half"].as_u64().unwrap() as u32,"{row}");
            assert_eq!(cosine.to_bits(),row["inner_cos"].as_u64().unwrap() as u32,"{row}");
            let mut light=Light::new(Kind::Spot,[0.;3],[1.,0.,0.]).unwrap();light.update_spot_angles(angle,angle,2.5,precision).unwrap();
            assert_eq!(light.spot_parameters.unwrap().map(f32::to_bits),[2.5f32.to_bits(),cosine.to_bits(),row["outer_cos"].as_u64().unwrap() as u32]);
        }
    }
    #[test]
    fn native_light_constructor_fields() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_constructor.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let kind=match row["name"].as_str().unwrap(){"Ambient"=>Kind::Ambient,"Directional"=>Kind::Directional,"Point"=>Kind::Point,"Spot"=>Kind::Spot,_=>unreachable!()};
            let light=Light::new(kind,[0.;3],[1.,0.,0.]).unwrap();
            let words=row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            assert_eq!(light.dimmer.to_bits(),words[0]);assert_eq!(light.ambient.map(f32::to_bits).as_slice(),&words[1..5]);
            assert_eq!(light.diffuse.map(f32::to_bits).as_slice(),&words[5..8]);assert_eq!(light.specular.map(f32::to_bits).as_slice(),&words[8..11]);
            if let Some(attenuation)=light.attenuation {assert_eq!(attenuation.map(f32::to_bits).as_slice(),&words[13..17]);}
            if let Some(spot)=light.spot_parameters {assert_eq!(spot.map(f32::to_bits).as_slice(),&words[22..25]);}
            assert_eq!(row["switch"],1);
            let state=State {slots:vec![Slot {effect_type:row["type"].as_u64().unwrap() as u8,switch:1,light:Some(std::sync::Arc::new(light))}]};
            if matches!(kind,Kind::Ambient) {assert!(select(&state.slots,1,0).is_none(),"Ambient native type0 is outside GeneralLight selection");}
        }
    }
    #[test]
    fn original_component_distance_attenuation() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_point_attenuation.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let previous=std::array::from_fn(|i|row["previous"][i].as_f64().unwrap() as f32);
            let legacy=std::array::from_fn(|i|row["legacy"][i].as_f64().unwrap() as f32);
            let model=reference["models"][row["model"].as_u64().unwrap() as usize].as_str().unwrap();
            let mut light=Light {kind:Kind::Point,dimmer:1.,ambient:[0.;4],diffuse:[0.;3],specular:[0.;3],position:[0.;3],direction:[0.;3],attenuation:Some(previous),spot_parameters:None};
            light.update_distance_attenuation(model,row["range"].as_f64().unwrap() as f32,row["shape"].as_f64().unwrap() as f32,legacy,super::super::pc_float::Precision::Extended).unwrap();
            let expected=row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            assert_eq!(light.attenuation.unwrap().map(f32::to_bits).as_slice(),expected,"{row}");
        }
    }
    #[test]
    fn original_ordered_light_selection() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_selection.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let slots=reference["lists"][row[0].as_u64().unwrap() as usize].as_array().unwrap().iter().map(|entry|Slot {effect_type:entry[0].as_u64().unwrap() as u8,switch:entry[1].as_u64().unwrap() as u8,light:None}).collect::<Vec<_>>();
            assert_eq!(select(&slots,row[1].as_u64().unwrap() as u8,row[2].as_u64().unwrap() as u32),row[3].as_u64().map(|v|v as usize),"{row}");
        }
    }
    #[test]
    fn fixed_requests_match_original_parser_and_explicit_precedence() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_object_parser.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let name=row["name"].as_str().unwrap();let explicit=row["explicit"].as_i64().map(|v|v as i32);
            // Bare auto-index registration remains with its original parser
            // owner. Refuse to invent that owner's counter state here.
            if name=="GeneralLight"&&explicit.is_none(){assert!(request(name,None).is_err());continue;}
            let expected=if row["kind"]==0 {None}else{Some((row["kind"].as_u64().unwrap() as u8,row["index"].as_u64().unwrap() as u32))};
            assert_eq!(request(name,explicit).unwrap(),expected,"{row}");
        }
    }
    #[test]
    fn known_absent_lights_publish_original_setter_values() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_null_light_setter.json")).unwrap();
        let names=["Dimmer","UndimmedAmbient","UndimmedDiffuse","UndimmedSpecular","Ambient","Diffuse","Specular","WorldPosition","ModelPosition","WorldDirection","ModelDirection","WorldTransform","ModelTransform","SpotAttenuation","Attenuation"];
        let state=State {slots:Vec::new()};
        for row in reference["cases"].as_array().unwrap() {
            assert_eq!(row["status"],96);let mapping=row["mapping"].as_u64().unwrap() as usize;
            let matrix=matches!(mapping,12|13);let scalar=mapping==1;
            let parameter=super::super::d3d9_effect::Value {kind:3,class:if matrix{2}else if scalar{0}else{1},rows:if matrix{4}else{1},columns:if scalar{1}else{4},elements:0,name:None,semantic:None,data:super::super::d3d9_effect::Data::Words(vec![])};
            let actual=state.words("SpotLight0",None,names[mapping-1],None,&parameter).unwrap().unwrap();
            let call=&row["calls"][0];let expected=call["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            assert_eq!(actual,expected,"{row}");
        }
    }
    #[test]
    fn missing_assigned_snapshot_is_not_an_absent_light() {
        let state=State {slots:vec![Slot {effect_type:4,switch:1,light:None}]};
        let parameter=super::super::d3d9_effect::Value {kind:3,class:1,rows:1,columns:4,elements:0,name:None,semantic:None,data:super::super::d3d9_effect::Data::Words(vec![])};
        assert!(state.words("SpotLight0",None,"Ambient",None,&parameter).is_err());
    }
    #[test]
    fn native_pc_light_words() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_values.json")).unwrap();
        let names=["Dimmer","UndimmedAmbient","UndimmedDiffuse","UndimmedSpecular","Ambient","Diffuse","Specular","Position"];
        for row in reference["cases"].as_array().unwrap() {
            let kind=match row["kind"].as_u64().unwrap(){0=>Kind::Ambient,2=>Kind::Point,3=>Kind::Directional,4=>Kind::Spot,_=>unreachable!()};
            let light=Light {kind,dimmer:row["dimmer"].as_f64().unwrap() as f32,ambient:[0.13,0.37,0.71,0.23],diffuse:[0.19,0.43,0.83],specular:[0.29,0.53,0.97],position:[-12.,34.,56.],attenuation:None,spot_parameters:None,direction:if matches!(kind,Kind::Spot){[0.125,-0.375,0.625]}else{[-0.25,0.5,0.75]}};
            let mapping=row["mapping"].as_u64().unwrap() as usize;
            let name=if mapping==10 {"Direction"}else{names[mapping-1]};
            let Some(Value::Vector(actual))=light.resolve(name,Some("world")).unwrap() else{panic!("Missing light value")};
            let expected=row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            assert_eq!(actual.map(f32::to_bits).as_slice(),expected,"{row}");
        }
    }
    #[test]
    fn native_pc_attenuation_words() {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_light_attenuation.json")).unwrap();
        for row in reference["cases"].as_array().unwrap() {
            let kind=match row["kind"].as_u64().unwrap(){2|5=>Kind::Point,3|6=>Kind::Directional,4|7=>Kind::Spot,_=>unreachable!()};
            let source:[f32;4]=std::array::from_fn(|i|row["values"][i].as_f64().unwrap() as f32);
            let light=Light {kind,dimmer:1.,ambient:[0.;4],diffuse:[0.;3],specular:[0.;3],position:[0.;3],direction:[0.;3],attenuation:Some(source),spot_parameters:Some([source[0],source[1],source[2]])};
            let property=if row["mapping"]==14 {"SpotAttenuation"}else{"Attenuation"};
            let Some(Value::Vector(actual))=light.resolve(property,None).unwrap() else{panic!("Missing attenuation")};
            let expected=row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>();
            assert_eq!(actual.map(f32::to_bits).as_slice(),expected,"{row}");
        }
    }
    #[test]
    fn unsupported_spaces_and_mesh_relative_values_reject() {
        let light=Light {kind:Kind::Point,dimmer:1.,ambient:[1.;4],diffuse:[1.;3],specular:[1.;3],position:[0.;3],direction:[0.;3],attenuation:None,spot_parameters:None};
        assert!(light.resolve("Attenuation",None).is_err());
        let spot=Light {kind:Kind::Spot,..light.clone()};
        assert!(spot.resolve("SpotAttenuation",None).is_err());
        assert!(light.resolve("Direction",Some("world")).is_err());
        assert!(light.resolve("Position",Some("object")).is_err());
        assert!(light.resolve("Position",None).is_err());
        assert!(light.resolve("Unknown",None).unwrap().is_none());
    }
}
