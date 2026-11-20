//! Native PC object probe position and movement gate. This does not invent
//! a world-bound center, bone position or manager skin-matrix result.
use anyhow::{ensure,Result};
use super::pc_float::{self,Precision};
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) enum PositionSource {WorldBound,ManagedSkin(i32),Bone,Unchanged}
pub(super) const BONE_NAMES:[&str;4]=["SH_probe1","SH_probe2","SH_probe3","SH_probe4"];
/// One wrapper shared by a model's render-resource attachments. Its bound
/// owner is the model root supplied to native9aa660, not each visited mesh.
#[derive(Debug,Clone,serde::Serialize)]
pub(super) struct Binding {pub index:i32,pub bone:Option<u32>,pub managed:bool}
impl Binding {
    fn bound()->Self {Self {index:-1,bone:None,managed:false}}
    pub fn source(&self,force_bound:bool)->PositionSource {position_source(self.index,force_bound,self.managed,self.bone.is_some())}
}
/// PC9acddd..9acf9f. A present manager with no probes overrides the authored
/// group and gets one bound wrapper. An empty authored group without a manager
/// gets zero wrappers. Missing named bones retain the constructor's bound mode.
pub(super) fn bindings(managed:Option<&[i32]>,group_count:Option<usize>,mut bone:impl FnMut(&str)->Result<Option<u32>>)->Result<Vec<Binding>> {
    if let Some(indices)=managed {
        return Ok(if indices.is_empty(){vec![Binding::bound()]}else{indices.iter().map(|index|Binding {index:*index,bone:None,managed:true}).collect()});
    }
    let Some(count)=group_count else{return Ok(vec![Binding::bound()]);};
    ensure!(count<=BONE_NAMES.len(),"Authored SH_probes count {count} exceeds the original PC's four named probe slots");
    (0..count).map(|index|{
        Ok(match bone(BONE_NAMES[index])? {Some(bone)=>Binding {index:index as i32,bone:Some(bone),managed:false},None=>Binding::bound()})
    }).collect()
}
/// PC9a7df0. A missing bone/manager leaves the caller's output untouched.
/// The global force-bound flag takes priority over either animated source.
pub(super) fn position_source(index:i32,force_bound:bool,manager:bool,bone:bool)->PositionSource {
    if index == -1 || force_bound {PositionSource::WorldBound}
    else if manager {PositionSource::ManagedSkin(index)}
    else if bone {PositionSource::Bone}else{PositionSource::Unchanged}
}
#[derive(Debug,Clone)]
pub(super) struct Probe {
    /// Native wrapper +28, used here only for its -1 initialization sentinel.
    /// Ownership/assignment of other marker values is intentionally upstream.
    pub update_marker:i32,pub position:[f32;3]
}
impl Probe {
    /// Original9aa660 explicitly initializes these fields; no sampled pose is
    /// invented until the selected position service supplies one.
    pub fn new()->Self {Self {update_marker:-1,position:[0.;3]}}
    /// PC9a7e60. The native -1 marker always writes the initial position.
    /// Equality with the threshold does NOT request an update. Failed numeric
    /// input leaves the previous position available and returns an error.
    pub fn update_position(&mut self,new:[f32;3],threshold:f32,precision:Precision)->Result<bool> {
        ensure!(new.iter().all(|v|v.is_finite()),"Nonfinite PC SH probe position");
        let changed=if self.update_marker == -1 {true}else {
            ensure!(threshold.is_finite(),"Nonfinite PC SH probe displacement threshold");
            pc_float::sh_probe_displacement(self.position,new,precision)?>threshold
        };
        if changed {self.position=new;}Ok(changed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_model_probe_allocation_and_constructor_defaults()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_binding.json"))?;
        assert_eq!(serde_json::json!(BONE_NAMES),fixture["names"]);
        for row in fixture["cases"].as_array().unwrap() {
            let managed=row["managed"].as_array().map(|values|values.iter().map(|v|v.as_i64().unwrap() as i32).collect::<Vec<_>>());
            let mask=row["bone_mask"].as_u64().unwrap();let mut calls=Vec::new();
            let result=bindings(managed.as_deref(),row["group_count"].as_u64().map(|v|v as usize),|name|{
                calls.push(name.to_owned());let index=BONE_NAMES.iter().position(|n|*n==name).unwrap();Ok((mask&(1<<index)!=0).then_some(0x21018000+index as u32*256))
            })?;
            assert_eq!(result.len(),row["context_count"].as_u64().unwrap() as usize);
            let expected=row["wrappers"].as_array().unwrap();assert_eq!(result.len(),expected.len());
            for (actual,expected) in result.iter().zip(expected) {
                assert_eq!(actual.index as u32,expected["index"].as_u64().unwrap() as u32);assert_eq!(actual.bone.unwrap_or(0),expected["bone"].as_u64().unwrap() as u32);assert_eq!(actual.managed,expected["manager"].as_bool().unwrap());
                let probe=Probe::new();assert_eq!(probe.update_marker as u32,expected["marker"].as_u64().unwrap() as u32);assert_eq!(probe.position.map(f32::to_bits).to_vec(),expected["position"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>());assert_eq!(expected["query"],0);
            }
            let expected=row["lookups"].as_array().unwrap().iter().filter(|v|v["owner"]=="group").map(|v|v["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>();assert_eq!(calls,expected);
        }
        assert!(bindings(None,Some(5),|_|Ok(None)).is_err());Ok(())
    }
    #[test]
    fn native_position_sources_and_dirty_gate()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_probe_position.json"))?;
        for row in reference["selection"].as_array().unwrap() {
            let source=position_source(row["index"].as_i64().unwrap() as i32,row["force_bound"].as_bool().unwrap(),row["manager"].as_bool().unwrap(),row["bone"].as_bool().unwrap());
            let expected=match source {PositionSource::WorldBound=>[1.,-0.,3.],PositionSource::Bone=>[-4.,5.,-6.],PositionSource::ManagedSkin(_)=>[7.,8.,9.],PositionSource::Unchanged=>[11.,12.,13.]};
            assert_eq!(expected.map(f32::to_bits).to_vec(),row["output_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
            let calls=row["manager_calls"].as_array().unwrap();match source {PositionSource::ManagedSkin(index)=>assert_eq!(calls,&vec![serde_json::json!(index)]),_=>assert!(calls.is_empty())};
        }
        for row in reference["movement"].as_array().unwrap() {
            let vector=|name:&str|std::array::from_fn(|i|row[name][i].as_f64().unwrap() as f32);
            let mut probe=Probe {update_marker:row["marker"].as_i64().unwrap() as i32,position:vector("old")};
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            assert_eq!(probe.update_position(vector("new"),row["threshold"].as_f64().unwrap() as f32,precision)?,row["changed"].as_bool().unwrap(),"{row}");
            assert_eq!(probe.position.map(f32::to_bits).to_vec(),row["position_words"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{row}");
        }Ok(())
    }
    #[test]
    fn rejected_position_does_not_damage_previous_probe_state() {
        let mut probe=Probe {update_marker:2,position:[1.,2.,3.]};
        assert!(probe.update_position([f32::NAN;3],0.,Precision::Extended).is_err());assert_eq!(probe.position,[1.,2.,3.]);
        assert!(probe.update_position([2.;3],f32::NAN,Precision::Extended).is_err());assert_eq!(probe.position,[1.,2.,3.]);
        assert_eq!(probe.update_marker,2);
    }
}
