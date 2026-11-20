//! PC b5dd20 render-profile selection and activation gate. Property reads,
//! interpolation and renderer publication are distinct subsequent phases.
use anyhow::{ensure,Context,Result};
use std::collections::BTreeMap;
#[derive(Debug,Clone,Copy)]
pub(super) struct Candidate<K> {pub id:K,pub center:[f32;3],pub range:f32,pub priority:i32}
#[derive(Debug,Clone,Copy)]
pub(super) struct Selection<K> {
    pub selected:Option<K>,pub active:Option<K>,pub changed:bool,
}
pub(super) fn select<K:Copy+Eq>(candidates:&[Candidate<K>],point:[f32;3],previous:Option<K>,precision:super::pc_float::Precision)->Result<Selection<K>> {
    ensure!(point.iter().all(|v|v.is_finite()),"Nonfinite render-profile query point");
    let mut selected=None;let mut priority=i32::MIN;let mut distance=f32::MAX;
    for candidate in candidates {
        ensure!(candidate.center.iter().all(|v|v.is_finite())&&candidate.range.is_finite(),"Nonfinite render-profile source");
        let (_,length)=super::pc_float::sh_distance(candidate.center,point,precision)?;
        // Range, priority and distance comparisons are all strict. Equal
        // priority/distance retains the earliest registered profile.
        if length<candidate.range&&(candidate.priority>priority||(candidate.priority==priority&&length<distance)) {
            selected=Some(candidate.id);priority=candidate.priority;distance=length;
        }
    }
    // Native does not reset active settings when outside every profile.
    Ok(Selection {selected,active:selected.or(previous),changed:selected.is_some()&&selected!=previous})
}
pub(super) type Key=(usize,usize);
impl<K:Copy> Candidate<K> {
    /// Properties must already include inheritance. Defaults are allowed only
    /// for a known PC RenderSceneEffectsComponent factory instance.
    pub fn from_properties(id:K,center:[f32;3],mut get:impl FnMut(&str)->Option<serde_json::Value>,factory_defaults:bool)->Result<Self> {
        let range=match get("Range") {
            Some(value)=>value.as_f64().context("Invalid render-profile Range")? as f32,
            None if factory_defaults=>100.,None=>anyhow::bail!("Render-profile Range factory ownership is unknown"),
        };
        let priority=match get("Priority") {
            Some(value)=>i32::try_from(value.as_i64().context("Invalid render-profile Priority")?)?,
            None if factory_defaults=>0,None=>anyhow::bail!("Render-profile Priority factory ownership is unknown"),
        };
        ensure!(center.iter().all(|v|v.is_finite())&&range.is_finite(),"Nonfinite render-profile source");
        Ok(Self {id,center,range,priority})
    }
}
#[derive(Debug,Default)]
pub(super) struct Registry {
    candidates:Vec<Candidate<Key>>,errors:BTreeMap<Key,String>,active:Option<Key>,
    last:Option<([u32;3],super::pc_float::Precision)>,selected:Option<Key>,changes:u64,
    settings:BTreeMap<Key,std::sync::Arc<super::render_profile_settings::Settings>>,desired:Option<std::sync::Arc<super::render_profile_settings::Settings>>,
    current:Option<super::render_profile_settings::Settings>,numeric:Option<super::render_profile_interpolation::Numeric>,numeric_ticks:u64,
    lighting:Option<(super::d3d9_globals::Lighting,super::d3d9_globals::Lighting)>,
}
impl Registry {
    pub fn refresh(&mut self,rows:impl IntoIterator<Item=(Key,Result<Candidate<Key>>)>) {
        self.candidates.clear();self.errors.clear();self.settings.clear();self.last=None;
        let mut seen=std::collections::BTreeSet::new();
        for (key,result) in rows {
            if !seen.insert(key) {self.errors.insert(key,"Duplicate render-profile identity".into());continue;}
            match result {Ok(candidate)=>self.candidates.push(candidate),Err(error)=>{self.errors.insert(key,format!("{error:#}"));}}
        }
    }
    pub fn refresh_profiles(&mut self,rows:impl IntoIterator<Item=(Key,Result<(Candidate<Key>,std::sync::Arc<super::render_profile_settings::Settings>)>)>) {
        let old=std::mem::take(&mut self.settings);let mut settings=BTreeMap::new();
        let rows=rows.into_iter().map(|(id,result)|{
            let result=result.map(|(candidate,value)|{
                let value=old.get(&id).filter(|old|old.same_bits(&value)).cloned().unwrap_or(value);
                settings.insert(id,value);candidate
            });(id,result)
        }).collect::<Vec<_>>();
        self.refresh(rows);self.settings=settings;
    }
    pub fn desired(&self)->Option<&std::sync::Arc<super::render_profile_settings::Settings>> {self.desired.as_ref()}
    pub fn current(&self)->Option<&super::render_profile_settings::Settings> {self.numeric.as_ref().map(|v|&v.current).or(self.current.as_ref())}
    pub fn lighting(&self,pass:super::d3d9_globals::ScenePass)->Option<&super::d3d9_globals::Lighting> {
        self.lighting.as_ref().map(|(main,toon)|match pass {super::d3d9_globals::ScenePass::Main=>main,super::d3d9_globals::ScenePass::ToonOffscreen=>toon})
    }
    fn refresh_lighting(&mut self,precision:super::pc_float::Precision)->Result<()> {
        use super::d3d9_globals::{Lighting,ScenePass};
        self.lighting=self.current().map(|settings|Ok::<_,anyhow::Error>((Lighting::from_profile(settings,ScenePass::Main,precision)?,Lighting::from_profile(settings,ScenePass::ToonOffscreen,precision)?))).transpose()?;Ok(())
    }
    /// Explicit scene tick, independent of the number of material draws. The
    /// temporary shared viewport's phase is not claimed as native frame parity.
    pub fn advance_numeric(&mut self)->Result<()> {
        if let Some(transition)=&mut self.numeric {
            if transition.active {
                transition.advance()?;
                let precision=transition.precision;
                self.refresh_lighting(precision)?;
            }
        }
        self.numeric_ticks+=1;Ok(())
    }
    /// Selection is observed once for an unchanged camera/sample generation.
    /// First activation snaps; later activations use the native time gate.
    /// Texture resource transitions remain explicit errors, not fake success.
    pub fn observe(&mut self,point:[f32;3],precision:super::pc_float::Precision)->Result<Selection<Key>> {
        ensure!(self.errors.is_empty(),"Invalid render profiles: {:?}",self.errors);
        let stamp=(point.map(f32::to_bits),precision);
        if self.last==Some(stamp) {return Ok(Selection {selected:self.selected,active:self.active,changed:false});}
        let result=select(&self.candidates,point,self.active,precision)?;
        let desired=if result.changed {result.selected.and_then(|id|self.settings.get(&id).cloned())}else{None};
        if let Some(desired)=&desired {
            use super::render_profile_settings::Value;
            let Some(Value::Float(seconds))=desired.get("InterpolateTime") else {anyhow::bail!("Render profile lost its transition time")};
            if super::render_profile_interpolation::blend_gate(self.active.is_some(),*seconds)? {
                let current=self.current().context("Render-profile activation has no previous current settings")?;
                let numeric=super::render_profile_interpolation::Numeric::begin(current,desired,*seconds,precision)?;
                self.numeric=Some(numeric);
            }else{self.current=Some((**desired).clone());self.numeric=None;}
            self.refresh_lighting(precision)?;
        }
        self.active=result.active;self.selected=result.selected;self.last=Some(stamp);
        if result.changed {self.changes+=1;self.desired=desired;}
        Ok(result)
    }
    pub fn summary(&self)->serde_json::Value {
        serde_json::json!({"candidates":self.candidates.len(),"active":self.active,"selected":self.selected,"changes":self.changes,"desired_settings_ready":self.desired.is_some(),"numeric_ticks":self.numeric_ticks,"numeric_interpolating":self.numeric.as_ref().is_some_and(|v|v.active),"current_lighting":self.current().map(|settings|serde_json::json!({"toon_minimum_diffuse":settings.get("ToonMinimumDiffuseLighting").map(|v|v.json()),"toon_brightness_boost":settings.get("ToonBrightnessBoost").map(|v|v.json()),"specular_direction":settings.get("StaticLightingSpecularDirection").map(|v|v.json())})),"desired_lighting":self.desired.as_ref().map(|settings|serde_json::json!({"toon_minimum_diffuse":settings.get("ToonMinimumDiffuseLighting").map(|v|v.json()),"toon_brightness_boost":settings.get("ToonBrightnessBoost").map(|v|v.json()),"specular_direction":settings.get("StaticLightingSpecularDirection").map(|v|v.json()),"interpolate_time":settings.get("InterpolateTime").map(|v|v.json())})),"errors":self.errors.iter().map(|(id,error)|serde_json::json!({"id":id,"error":error})).collect::<Vec<_>>(),"scope":"Live authored selection and current numeric transitions; viewport query and shared simulation phase provisional; retained texture transitions and renderer publication pending"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_state_blends_interrupts_and_preserves_failed_resource_activation()->Result<()> {
        use super::super::{pc_float::Precision,render_profile_settings::{Settings,Value}};use std::sync::Arc;
        let source=|boost:f32,path:Option<&str>|->Result<_>{Ok(Arc::new(Settings::read(|name|match name {
            "ToonBrightnessBoost"=>Some(serde_json::json!(boost)),"InterpolateTime"=>Some(serde_json::json!(0.1)),
            "NeutralLUTNifFilePath"=>path.map(|path|serde_json::json!(path)),_=>None,
        },true)?))};
        let mut registry=Registry::default();
        let mut activate=|registry:&mut Registry,id,settings|->Result<_>{
            registry.refresh_profiles([(id,Ok((Candidate {id,center:[0.;3],range:10.,priority:0},settings)))]);
            registry.observe([0.;3],Precision::Extended)
        };
        activate(&mut registry,(0,0),source(0.,None)?)?;
        assert!(matches!(registry.current().unwrap().get("ToonBrightnessBoost"),Some(Value::Float(v)) if *v==0.));
        activate(&mut registry,(1,0),source(1.,None)?)?;
        registry.refresh_profiles([((1,0),Ok((Candidate {id:(1,0),center:[0.;3],range:10.,priority:0},source(1.,None)?)))]);
        registry.advance_numeric()?;
        let Some(Value::Float(first))=registry.current().unwrap().get("ToonBrightnessBoost") else{unreachable!()};let first=*first;
        assert!(first>0.&&first<1.);
        activate(&mut registry,(2,0),source(0.,None)?)?;
        assert!(matches!(registry.current().unwrap().get("ToonBrightnessBoost"),Some(Value::Float(v)) if v.to_bits()==first.to_bits()));
        registry.advance_numeric()?;
        assert!(matches!(registry.current().unwrap().get("ToonBrightnessBoost"),Some(Value::Float(v)) if *v<first));
        let before=registry.current().unwrap().clone();
        assert!(activate(&mut registry,(3,0),source(1.,Some("Textures/new.nif"))?).is_err());
        assert_eq!(registry.active,Some((2,0)));assert!(registry.current().unwrap().same_bits(&before));
        registry.refresh_profiles([]);registry.observe([0.;3],Precision::Extended)?;
        for _ in 0..10 {registry.advance_numeric()?;}
        assert!(matches!(registry.current().unwrap().get("ToonBrightnessBoost"),Some(Value::Float(v)) if *v==0.));Ok(())
    }
    #[test]
    fn verified_factory_defaults_and_live_registration()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_selection.json"))?;
        let candidate=Candidate::from_properties((0,0),[0.;3],|_|None,true)?;
        assert_eq!(candidate.range,fixture["defaults"]["Range"].as_f64().unwrap() as f32);
        assert_eq!(candidate.priority,fixture["defaults"]["Priority"].as_i64().unwrap() as i32);
        assert!(Candidate::from_properties((0,0),[0.;3],|_|None,false).is_err());
        let mut registry=Registry::default();registry.refresh([((0,0),Ok(candidate))]);
        assert!(registry.observe([0.;3],super::super::pc_float::Precision::Extended)?.changed);
        assert!(!registry.observe([0.;3],super::super::pc_float::Precision::Extended)?.changed);
        registry.refresh([]);
        let outside=registry.observe([1000.;3],super::super::pc_float::Precision::Extended)?;
        assert_eq!(outside.active,Some((0,0)));assert!(!outside.changed);Ok(())
    }
    #[test]
    fn desired_settings_follow_activation_gate_and_reuse_sources()->Result<()> {
        use super::super::render_profile_settings::{Settings,Value};use std::sync::Arc;
        let initial=Arc::new(Settings::read(|_|None,true)?);
        let changed=Arc::new(Settings::read(|name|(name=="ToonBrightnessBoost").then(||serde_json::json!(0.2)),true)?);
        let candidate=Candidate {id:(0,0),center:[0.;3],range:10.,priority:0};
        let mut registry=Registry::default();registry.refresh_profiles([((0,0),Ok((candidate,initial.clone())))]);
        registry.observe([0.;3],super::super::pc_float::Precision::Extended)?;
        assert!(Arc::ptr_eq(registry.desired().unwrap(),&initial));
        registry.refresh_profiles([((0,0),Ok((candidate,changed.clone())))]);
        assert!(!registry.observe([0.;3],super::super::pc_float::Precision::Extended)?.changed);
        assert!(Arc::ptr_eq(registry.desired().unwrap(),&initial));
        let next=Candidate {id:(1,0),..candidate};registry.refresh_profiles([((1,0),Ok((next,changed.clone())))]);
        assert!(registry.observe([0.;3],super::super::pc_float::Precision::Extended)?.changed);
        assert!(matches!(registry.desired().unwrap().get("ToonBrightnessBoost"),Some(Value::Float(v)) if *v==0.2));
        registry.refresh_profiles([((1,0),Ok((next,Arc::new((*changed).clone()))))]);
        assert!(Arc::ptr_eq(&registry.settings[&(1,0)],&changed));Ok(())
    }
    #[test]
    fn original_pc_profile_selection_and_activation_gate()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_render_profile_selection.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let vector=|v:&serde_json::Value|std::array::from_fn(|i|v[i].as_f64().unwrap() as f32);
            let candidates=row["candidates"].as_array().unwrap().iter().map(|v|Candidate {id:v["id"].as_u64().unwrap(),center:vector(&v["center"]),range:v["range"].as_f64().unwrap() as f32,priority:v["priority"].as_i64().unwrap() as i32}).collect::<Vec<_>>();
            let previous=row["previous"].as_u64();
            let precision=match row["precision"].as_u64().unwrap() {0x7f=>super::super::pc_float::Precision::Single,0x27f=>super::super::pc_float::Precision::Double,0x37f=>super::super::pc_float::Precision::Extended,_=>unreachable!()};
            let result=select(&candidates,vector(&row["point"]),previous,precision)?;
            assert_eq!(result.selected,row["selected"].as_u64(),"{row}");
            assert_eq!(result.changed,row["changed"].as_bool().unwrap(),"{row}");
            assert_eq!(result.active,result.selected.or(previous),"{row}");
        }Ok(())
    }
}
