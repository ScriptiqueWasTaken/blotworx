//! PC SHKernel + SHTask completion numeric path for non-raycast lights.
//! Inputs are physical repository order, including inactive light slots.
//! Batch selection, asynchronous timing and raycast processing are separate.
use anyhow::{ensure,Result};
use std::sync::Arc;
use super::{d3d9_light::Kind,d3d9_sh::{LightRecord,Snapshot,Contribution},d3d9_sh_pool::{TaskOutput,CachedLight},pc_float::{self,Precision}};
/// Native SHTask's physical-slot round-robin cursor. Workflow waits/reset
/// remain the owner's responsibility; advance only after actual completion.
#[derive(Debug,Clone)]
pub(super) struct Window {total:u32,begin:u32,end:u32}
impl Window {
    pub fn new(total:u32)->Self {Self {total,begin:0,end:total.min(64)}}
    pub fn reset(&mut self,total:u32){*self=Self::new(total);}
    pub fn submit(&mut self)->std::ops::Range<usize> {
        if self.total>64 {self.end=self.begin+(self.total-self.begin).min(64);}
        self.begin as usize..self.end as usize
    }
    pub fn completed(&mut self)->std::ops::Range<usize> {
        let completed=self.begin as usize..self.end as usize;
        if self.total>64 {
            let count=self.end.wrapping_sub(self.begin);let end=self.end;
            self.begin=if end>=self.total{0}else{end};
            self.end=end.wrapping_add(count).min(self.total);
        }
        completed
    }
}
/// Join the native pool's physical data indices to the verified worker and
/// completion path. Inactive columns have no completion consumer and can be
/// omitted without replacing their unknown/stale position bytes with zeros.
pub(super) fn execute_pool_range(pool:&mut super::d3d9_sh_pool::Pool,gate_open:bool,frame:u32,range:std::ops::Range<usize>,lights:&[Arc<LightRecord>],precision:Precision)->Result<()> {
    ensure!(range.start<=range.end&&range.len()<=64,"SH worker range exceeds native task width");
    let inputs=pool.task_inputs(range.clone())?;
    let positions=inputs.iter().map(|input|input.position).collect::<Vec<_>>();
    let cached=inputs.iter().map(|input|input.cached_lights).collect::<Vec<_>>();
    let results=execute(frame,&positions,lights,&cached,precision)?;
    let outputs=inputs.into_iter().zip(results).map(|(input,result)|(input.index,result)).collect();
    pool.complete_sparse_batch(gate_open,frame,range,&outputs)
}
/// The scene owner supplies the real repository gate, task range and raycast
/// configuration. This does not infer original async completion timing.
pub(super) fn execute_scene_range(pool:&mut super::d3d9_sh_pool::Pool,scene:&mut super::d3d9_scene_lights::Registry,gate_open:bool,frame:u32,range:std::ops::Range<usize>,precision:Precision,raycasts:bool)->Result<()> {
    let lights=scene.records(precision,raycasts)?.into_iter().map(|(_,light)|light).collect::<Vec<_>>();
    execute_pool_range(pool,gate_open,frame,range,&lights,precision)
}
/// Execute one native batch. The engine's fixed task buffer has64 probe slots;
/// the caller must schedule subsequent ranges rather than discard overflow.
/// Probe positions include any inactive slots in the submitted physical range.
pub(super) fn execute(frame:u32,probes:&[[f32;3]],lights:&[Arc<LightRecord>],initial_cached:&[[CachedLight;9]],precision:Precision)->Result<Vec<Arc<TaskOutput>>> {
    ensure!(probes.len()<=64&&probes.len()==initial_cached.len(),"Invalid PC SH worker batch size");
    let mut snapshots=probes.iter().map(|_|Snapshot {coefficients:[[0.;3];9],specular_color:None,specular_direction:None}).collect::<Vec<_>>();
    let mut cached=initial_cached.to_vec();for cells in &mut cached {for cell in cells {cell.reset();}}
    // This loop order matches completion's repository-light outer loop. Do
    // not reorder by entity name, distance, weight or parallel reduction.
    for (index,light) in lights.iter().enumerate() {
        for (probe,position) in probes.iter().copied().enumerate() {
            let sample=light.sample(position,precision)?;
            if sample.weight<=0. {continue;}
            ensure!(light.flags&0x4000==0,"PC SH raycast task processing is not connected");
            let kind=match light.flags&0xf {1=>Kind::Ambient,2=>Kind::Directional,4=>Kind::Point,8=>Kind::Spot,value=>anyhow::bail!("Unknown PC SH worker light type {value}")};
            snapshots[probe].accumulate(&Contribution {kind,ambient:light.ambient,diffuse:light.diffuse,light_direction:light.direction,
                probe_direction:sample.direction.ok_or_else(||anyhow::anyhow!("Positive PC SH sample has no direction"))?,weight:sample.weight},precision)?;
            CachedLight::insert(&mut cached[probe],index as u16,pc_float::sh_cached_weight(sample.weight,precision)?);
        }
    }
    Ok(snapshots.into_iter().zip(cached).map(|(snapshot,cached_lights)|Arc::new(TaskOutput {computed_frame:frame,coefficients:Arc::new(snapshot.coefficients),cached_lights})).collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pool_worker_and_shader_publication_preserve_native_coefficients_with_holes()->Result<()> {
        use super::super::{d3d9_sh_pool::{Pool,Service},d3d9_sh_query::{Client,Publication}};
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_worker.json"))?;
        let vector=|value:&serde_json::Value|std::array::from_fn(|i|value[i].as_f64().unwrap() as f32);
        let lights=fixture["lights"].as_array().unwrap().iter().map(|light|Arc::new(LightRecord {flags:light["flags"].as_u64().unwrap() as u16,position:vector(&light["position"]),direction:vector(&light["direction"]),ambient:vector(&light["ambient"]),diffuse:vector(&light["diffuse"]),dimmer:light["dimmer"].as_f64().unwrap() as f32,falloff:vector(&light["falloff"]),inner_cos:light["inner_cos"].as_f64().unwrap() as f32,outer_cos:light["outer_cos"].as_f64().unwrap() as f32,exponent:light["exponent"].as_f64().unwrap() as f32})).collect::<Vec<_>>();
        let unexpected=|_:&mut Pool,_|->Result<()>{anyhow::bail!("Unexpected SH task wait")};
        for row in fixture["cases"].as_array().unwrap().iter().filter(|row|!row["probes"].as_array().unwrap().is_empty()) {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            let mut pool=Pool::new(64)?;
            let mut clients=row["probes"].as_array().unwrap().iter().map(|probe|Client {position:vector(&probe["position"]),query:None,token:0}).collect::<Vec<_>>();
            for (id,client) in clients.iter_mut().enumerate(){client.request(&mut Service {pool:&mut pool,client:id as u64,frame:1,precision,complete:unexpected},false)?;}
            let removed=(clients.len()>2).then_some(2);
            let shared=removed.is_some_and(|id|clients.iter().enumerate().any(|(other,client)|other!=id&&client.query.as_ref().unwrap().same(clients[id].query.as_ref().unwrap())));
            if let Some(id)=removed {pool.release(id as u64);pool.age(2)?;}
            for (id,client) in clients.iter_mut().enumerate().filter(|(id,_)|Some(*id)!=removed){client.request(&mut Service {pool:&mut pool,client:id as u64,frame:2,precision,complete:unexpected},false)?;}
            execute_pool_range(&mut pool,true,2,0..64,&lights,precision)?;
            for (id,client) in clients.iter_mut().enumerate().filter(|(id,_)|Some(*id)!=removed) {
                let mut snapshot=Snapshot {coefficients:[[17.;3];9],specular_color:None,specular_direction:None};
                let mut service=Service {pool:&mut pool,client:id as u64,frame:2,precision,complete:unexpected};
                assert!(client.publish(Publication {resource_kind:0x8000,capability:None},2,Some(&mut service),Ok,&mut snapshot)?);
                assert_eq!(snapshot.coefficients.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),row["outputs"][id]["coefficients"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{precision:?}, client {id}");
            }
            if let Some(id)=removed {
                assert!(pool.query_for_client(id as u64).is_none());
                assert_eq!(clients[id].query.as_ref().unwrap().read()?.status,if shared{2}else{0});
            }
        }Ok(())
    }
    #[test]
    fn scene_task_ranges_cover_overflow_without_fabricated_outputs()->Result<()> {
        use super::super::{d3d9_sh_pool::Pool,d3d9_sh_query::Request,d3d9_scene_lights::Registry};
        let mut pool=Pool::new(128)?;let mut scene=Registry::default();
        for id in 0..70 {pool.request(id,Request {position:[id as f32*2.,0.,0.],mode:2},1,Precision::Extended)?;}
        execute_scene_range(&mut pool,&mut scene,true,1,0..64,Precision::Extended,false)?;
        assert_eq!(pool.query_for_client(0).unwrap().read()?.status,2);assert_eq!(pool.query_for_client(69).unwrap().read()?.status,1);
        execute_scene_range(&mut pool,&mut scene,true,1,64..128,Precision::Extended,false)?;
        assert_eq!(pool.query_for_client(0).unwrap().read()?.status,1);assert_eq!(pool.query_for_client(69).unwrap().read()?.status,2);
        assert!(pool.complete_sparse_batch(true,2,64..128,&Default::default()).is_err());
        assert_eq!(pool.completed_frame,1);assert_eq!(pool.query_for_client(69).unwrap().read()?.status,2);
        assert!(execute_scene_range(&mut pool,&mut scene,true,2,0..128,Precision::Extended,false).is_err());Ok(())
    }
    #[test]
    fn native_cached_light_quantization_and_full_capacity()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_cached_light.json"))?;
        for row in reference["cases"].as_array().unwrap() {
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            let mut cells=std::array::from_fn(|i|CachedLight::from_word(row["initial"][i].as_u64().unwrap() as u32));
            for step in row["steps"].as_array().unwrap() {
                let weight=f32::from_bits(step["weight_word"].as_u64().unwrap() as u32);
                CachedLight::insert(&mut cells,step["id"].as_u64().unwrap() as u16,pc_float::sh_cached_weight(weight,precision)?);
                assert_eq!(cells.map(|cell|cell.word().unwrap()).to_vec(),step["words"].as_array().unwrap().iter().map(|word|word.as_u64().unwrap() as u32).collect::<Vec<_>>(),"{step}");
            }
        }Ok(())
    }
    #[test]
    fn native_round_robin_windows_do_not_drop_slots()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_window.json"))?;
        for row in reference["cases"].as_array().unwrap() {
            let count=row["count"].as_u64().unwrap() as u32;let mut window=Window::new(0);window.reset(count);
            assert_eq!(serde_json::json!([window.total,window.begin,window.end]),row["reset"]);
            let mut seen=std::collections::BTreeSet::new();
            for step in row["steps"].as_array().unwrap() {
                let submitted=window.submit();assert!(submitted.len()<=64);seen.extend(submitted.clone());
                assert_eq!(serde_json::json!([submitted.start,submitted.len()]),step["submitted"]);
                let completed=window.completed();assert_eq!(serde_json::json!([completed.start,completed.end]),step["completed"]);
                assert_eq!(serde_json::json!([window.begin,window.end]),step["cursor"]);
            }
            assert_eq!(seen,(0..count as usize).collect());
        }Ok(())
    }
    #[test]
    fn unhandled_raycast_work_is_reported_instead_of_guessed()->Result<()> {
        let mut light=LightRecord {flags:0x1000|0x4000|0x10|1,position:[1.,0.,0.],direction:[0.,0.,1.],ambient:[1.;3],diffuse:[0.;3],dimmer:1.,falloff:[1.,0.,0.],inner_cos:1.,outer_cos:1.,exponent:1.};
        let cached=[[CachedLight::default();9]];
        assert!(execute(1,&[[0.;3]],&[Arc::new(light.clone())],&cached,Precision::Extended).is_err());
        light.dimmer=0.;let result=execute(1,&[[0.;3]],&[Arc::new(light)],&cached,Precision::Extended)?;
        assert_eq!(*result[0].coefficients,[[0.;3];9]);Ok(())
    }
    #[test]
    fn native_kernel_and_completion_outputs_match()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_worker.json"))?;
        let vector=|value:&serde_json::Value|std::array::from_fn(|i|value[i].as_f64().unwrap() as f32);
        let lights=fixture["lights"].as_array().unwrap().iter().map(|light|Arc::new(LightRecord {flags:light["flags"].as_u64().unwrap() as u16,position:vector(&light["position"]),direction:vector(&light["direction"]),ambient:vector(&light["ambient"]),diffuse:vector(&light["diffuse"]),dimmer:light["dimmer"].as_f64().unwrap() as f32,falloff:vector(&light["falloff"]),inner_cos:light["inner_cos"].as_f64().unwrap() as f32,outer_cos:light["outer_cos"].as_f64().unwrap() as f32,exponent:light["exponent"].as_f64().unwrap() as f32})).collect::<Vec<_>>();
        for row in fixture["cases"].as_array().unwrap() {
            let probes=row["probes"].as_array().unwrap().iter().map(|probe|vector(&probe["position"])).collect::<Vec<_>>();
            let cached=row["initial_cached"].as_array().unwrap().iter().map(|cells|std::array::from_fn(|i|CachedLight::from_word(cells[i].as_u64().unwrap() as u32))).collect::<Vec<_>>();
            let precision=match row["precision"].as_u64().unwrap(){0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>unreachable!()};
            let results=execute(row["frame"].as_u64().unwrap() as u32,&probes,&lights,&cached,precision)?;
            for (result,expected) in results.iter().zip(row["outputs"].as_array().unwrap()) {
                assert_eq!(result.coefficients.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>(),expected["coefficients"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>(),"precision {:?}, count {}",precision,probes.len());
                assert_eq!(result.cached_lights.map(|cell|cell.word().unwrap()).to_vec(),expected["cached"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u32).collect::<Vec<_>>());
                assert_eq!(result.computed_frame,expected["frame"].as_u64().unwrap() as u32);
            }
        }Ok(())
    }
}
