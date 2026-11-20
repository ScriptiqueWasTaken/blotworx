//! PC4ce330 texture-effect branches. Assignment order is supplied by the
//! owning NiDynamicEffectState; this does not infer effects from shader names.
use std::collections::BTreeMap;
#[derive(Debug,Clone,Copy)]
pub(super) struct Registration {
    pub identity:u64,pub index:i32,pub effect_type:u8,pub texture_type:u32,
}
/// Native state membership, separate from the objects themselves. Snapshots
/// resolve identities through the scene owner so inherited states do not keep
/// stale copies of animated textures, switches or projection properties.
#[derive(Debug,Clone,Default)]
pub(super) struct Assignments {
    lights:BTreeMap<i32,u64>,projected_lights:BTreeMap<i32,u64>,
    projected_shadows:BTreeMap<i32,u64>,environment:Option<u64>,fog:Option<u64>,
    multitexture:bool,
}
impl Assignments {
    pub fn add(&mut self,source:Option<Registration>) {
        let Some(source)=source else{return};
        if source.effect_type<=7 {self.lights.entry(source.index).or_insert(source.identity);return;}
        match source.texture_type {
            0=>{self.projected_lights.entry(source.index).or_insert(source.identity);},
            1=>{self.projected_shadows.entry(source.index).or_insert(source.identity);},
            2=>self.environment=Some(source.identity),3=>self.fog=Some(source.identity),_=>{},
        }
        // PC sets this even for an unrecognized texture type. Removal later
        // recomputes it from membership; do not silently normalize that quirk.
        self.multitexture=true;
    }
    pub fn remove(&mut self,source:Option<Registration>) {
        let Some(source)=source else{return};
        if source.effect_type<=7 {self.lights.remove(&source.index);return;}
        match source.texture_type {
            0=>{self.projected_lights.remove(&source.index);},
            1=>{self.projected_shadows.remove(&source.index);},
            2 if self.environment==Some(source.identity)=>self.environment=None,
            3 if self.fog==Some(source.identity)=>self.fog=None,_=>{},
        }
        self.multitexture=!self.projected_lights.is_empty()||!self.projected_shadows.is_empty()||self.environment.is_some()||self.fog.is_some();
    }
    pub fn multitexture(&self)->bool {self.multitexture}
    pub fn light_ids(&self)->impl Iterator<Item=u64>+'_ {self.lights.values().copied()}
    pub fn snapshot<T>(&self,mut resolve:impl FnMut(u64)->anyhow::Result<Effect<T>>)->anyhow::Result<State<T>> {
        let environment=self.environment.map(&mut resolve).transpose()?;
        let fog=self.fog.map(&mut resolve).transpose()?;
        let projected_shadows=self.projected_shadows.values().map(|&id|resolve(id).map(Some)).collect::<anyhow::Result<_>>()?;
        let projected_lights=self.projected_lights.values().map(|&id|resolve(id).map(Some)).collect::<anyhow::Result<_>>()?;
        Ok(State {environment,fog,projected_shadows,projected_lights})
    }
}
#[derive(Debug,Clone,Copy)]
pub(super) enum Kind {Environment,ProjectedShadow,ProjectedLight,Fog}
impl Kind {
    pub fn named(name:&str)->Option<Self> {
        [("EnvironmentMap",Self::Environment),("ProjectedShadowMap",Self::ProjectedShadow),("ProjectedLightMap",Self::ProjectedLight),("FogMap",Self::Fog)].into_iter().find(|(n,_)|name.eq_ignore_ascii_case(n)).map(|(_,kind)|kind)
    }
}
#[derive(Debug,Clone)]
pub(super) struct Effect<T> {pub switch:u8,pub value:T}
#[derive(Debug,Clone)]
pub(super) struct State<T> {
    pub environment:Option<Effect<T>>,pub fog:Option<Effect<T>>,
    pub projected_shadows:Vec<Option<Effect<T>>>,pub projected_lights:Vec<Option<Effect<T>>>,
}
impl<T> State<T> {
    pub fn select(&self,kind:Kind,index:u32)->Option<&Effect<T>> {
        let slots=match kind {
            // Singleton object kinds ignore ObjectIndex in the PC function.
            Kind::Environment=>return self.environment.as_ref().filter(|e|e.switch!=0),
            Kind::Fog=>return self.fog.as_ref().filter(|e|e.switch!=0),
            Kind::ProjectedShadow=>&self.projected_shadows,
            Kind::ProjectedLight=>&self.projected_lights,
        };
        slots.iter().flatten().filter(|e|e.switch!=0).nth(index as usize)
    }
}
/// PC NiTextureEffect fields at128/14c/17c/18e/168. This retains source
/// projection data; object-space conversion and camera generation are separate.
#[derive(Debug,Clone)]
pub(super) struct Projection {
    pub rotation:[f32;9],pub translation:[f32;3],pub plane:[f32;4],
    pub clipping:bool,pub coordinate_mode:u32,
}
impl Projection {
    pub fn handles(property:&str)->bool {
        ["WorldProjectionMatrix","WorldProjectionTranslation","WorldProjectionTransform","WorldClippingPlane","TexCoordGen"].iter().any(|name|property.eq_ignore_ascii_case(name))
    }
    pub fn resolve(source:Option<&Self>,property:&str)->Option<super::d3d9_predefined::Value> {
        use super::d3d9_predefined::Value;
        if property.eq_ignore_ascii_case("WorldClippingPlane") {
            return Some(Value::Vector(match source {Some(s) if s.clipping=>s.plane,Some(_)=>[0.;4],None=>[1.,0.,0.,0.]}));
        }
        if property.eq_ignore_ascii_case("TexCoordGen") {
            let mode=source.map(|s|match s.coordinate_mode {0|1=>2.,2|3=>3.,4=>1.,_=>0.}).unwrap_or(0.);
            return Some(Value::Vector([mode,0.,0.,0.]));
        }
        if property.eq_ignore_ascii_case("WorldProjectionTranslation") {
            let v=source.map(|s|s.translation).unwrap_or([0.;3]);
            return Some(Value::Vector([v[0],v[1],v[2],1.]));
        }
        if property.eq_ignore_ascii_case("WorldProjectionMatrix")||property.eq_ignore_ascii_case("WorldProjectionTransform") {
            let mut matrix=[0.;16];matrix[15]=1.;
            if let Some(source)=source {
                for r in 0..3 {for c in 0..3 {matrix[r*4+c]=source.rotation[c*3+r];}}
                if property.eq_ignore_ascii_case("WorldProjectionTransform") {matrix[12..15].copy_from_slice(&source.translation);}
            } else {matrix[0]=1.;matrix[5]=1.;matrix[10]=1.;}
            return Some(Value::Matrix(matrix));
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pc_effect_assignment_lifecycle()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_effect_assignment.json"))?;
        let objects:BTreeMap<_,_>=fixture["objects"].as_array().unwrap().iter().map(|v|{
            let r=Registration {identity:v["id"].as_u64().unwrap(),index:v["index"].as_i64().unwrap() as i32,effect_type:v["effect_type"].as_u64().unwrap() as u8,texture_type:v["texture_type"].as_u64().unwrap() as u32};(r.identity,r)
        }).collect();
        let mut assignments=Assignments::default();
        for step in fixture["steps"].as_array().unwrap() {
            let source=objects.get(&step["id"].as_u64().unwrap()).copied();
            if step["op"]=="add" {assignments.add(source);}else{assignments.remove(source);}
            let state=assignments.snapshot(|id|Ok(Effect {switch:1,value:id}))?;
            let ids=|slots:Vec<Option<Effect<u64>>>|slots.into_iter().map(|e|e.unwrap().value).collect::<Vec<_>>();
            let actual=serde_json::json!({"lights":assignments.light_ids().collect::<Vec<_>>(),"projected_lights":ids(state.projected_lights),"projected_shadows":ids(state.projected_shadows),"environment":state.environment.map(|e|e.value),"fog":state.fog.map(|e|e.value),"multitexture":assignments.multitexture()});
            assert_eq!(actual,step["after"],"{step}");
        }Ok(())
    }
    #[test]
    fn original_pc_projection_property_publication()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_texture_projection.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let floats=|name:&str|row[name].as_array().unwrap().iter().map(|v|v.as_f64().unwrap() as f32).collect::<Vec<_>>();
            let source=Projection {rotation:floats("rotation").try_into().unwrap(),translation:floats("translation").try_into().unwrap(),plane:floats("plane").try_into().unwrap(),clipping:row["clipping"].as_bool().unwrap(),coordinate_mode:row["mode"].as_u64().unwrap() as u32};
            let name=match row["mapping"].as_u64().unwrap() {16=>"WorldProjectionMatrix",18=>"WorldProjectionTranslation",20=>"WorldClippingPlane",22=>"TexCoordGen",23=>"WorldProjectionTransform",_=>unreachable!()};
            let value=Projection::resolve(row["present"].as_bool().unwrap().then_some(&source),name).unwrap();
            let words:Vec<_>=match value {super::super::d3d9_predefined::Value::Matrix(m)=>m.into_iter().map(f32::to_bits).collect(),super::super::d3d9_predefined::Value::Vector(v)=>v.into_iter().map(f32::to_bits).collect()};
            assert_eq!(words,row["words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }Ok(())
    }
    #[test]
    fn original_pc_texture_effect_selection()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_texture_effect_selection.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let config=&row["state"];
            let slot=|value:&serde_json::Value,name:&str,index:usize|value.as_u64().map(|switch|Effect {switch:switch as u8,value:serde_json::json!([name,index])});
            let list=|name:&str|config[name].as_array().unwrap().iter().enumerate().map(|(i,v)|slot(v,name,i)).collect();
            let state=State {environment:slot(&config["environment"],"environment",0),fog:slot(&config["fog"],"fog",0),projected_shadows:list("shadow"),projected_lights:list("light")};
            let kind=match row["kind"].as_u64().unwrap() {8=>Kind::Environment,9=>Kind::ProjectedShadow,10=>Kind::ProjectedLight,11=>Kind::Fog,_=>unreachable!()};
            let selected=state.select(kind,row["index"].as_u64().unwrap() as u32).map(|e|e.value.clone()).unwrap_or(serde_json::Value::Null);
            assert_eq!(selected,row["selected"],"{row}");
        }Ok(())
    }
}
