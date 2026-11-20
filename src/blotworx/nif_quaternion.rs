//! Retail NiQuaternion interpolation804c613c and normalization804c5fcc.
//! This is not a generic normalized slerp; preserve the original warp,
//! fused arithmetic and normalization gate, including undersized outputs.
use anyhow::{ensure,Result};
const INTERCEPT:f32=f32::from_bits(0x3f82be64);
const SLOPE:f32=f32::from_bits(0xbf0852f2);
pub(super) fn dot(p:[f32;4],q:[f32;4])->f32 {
    let a=p[0].mul_add(q[0],p[1]*q[1]);
    p[3].mul_add(q[3],p[2].mul_add(q[2],a))
}
fn inverse_polynomial(value:f32)->f32 {
    SLOPE.mul_add(value-f32::from_bits(0x3f758559),INTERCEPT)
}
pub(super) fn normalize(q:[f32;4])->Result<[f32;4]> {
    let a=q[1].mul_add(q[1],q[2]*q[2]);
    let norm=q[0].mul_add(q[0],q[3].mul_add(q[3],a));
    ensure!(norm.is_finite(),"Nonfinite native quaternion norm");
    //804c6024..34 subtracts AFTER fabs. Do not replace with abs(norm-1).
    if (norm.abs() as f64)-1. < f64::from_bits(0x3ee4f8b580000000) {return Ok(q);}
    let mut factor=inverse_polynomial(norm);
    if norm<=f32::from_bits(0x3f6a4b55) {
        factor*=inverse_polynomial(norm*(factor*factor));
        if norm<=f32::from_bits(0x3f26f151) {factor*=inverse_polynomial(norm*(factor*factor));}
    }
    Ok(q.map(|v|v*factor))
}
fn warp(time:f32,cosine:f32)->f32 {
    let factor=-f32::from_bits(0x3f52a2d1).mul_add(cosine,-1.);
    let factor=factor*factor;
    let curve=2f32.mul_add(time,-3.);
    let k=f32::from_bits(0x3f15e2d1)*factor;
    let value=curve.mul_add(k*time,1.);
    time*(k+value)
}
pub(super) fn interpolate(p:[f32;4],q:[f32;4],time:f32)->Result<[f32;4]> {
    ensure!(p.iter().chain(q.iter()).chain(std::iter::once(&time)).all(|v|v.is_finite()),"Nonfinite quaternion interpolation input");
    let cosine=dot(p,q);
    let positive=cosine>=0.;
    let cosine=if positive {cosine}else{-cosine};
    let weight=if time<=0.5 {warp(time,cosine)}else{1.-warp(1.-time,cosine)};
    normalize(std::array::from_fn(|i|if positive {weight.mul_add(q[i]-p[i],p[i])}else{-weight.mul_add(p[i]+q[i],-p[i])}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_interpolation_and_normalization_match_original_float_words()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/nif_quaternion.json"))?;
        assert_eq!(fixture["coefficients"],serde_json::json!([INTERCEPT.to_bits(),SLOPE.to_bits()]));
        for (key,normalization) in [("cases",false),("normalization",true)] {
            for c in fixture[key].as_array().unwrap() {
                let vector=|key:&str|std::array::from_fn(|i|f32::from_bits(c[key][i].as_u64().unwrap() as u32));
                let actual=if normalization {normalize(vector("p"))?}else{interpolate(vector("p"),vector("q"),f32::from_bits(c["time"].as_u64().unwrap() as u32))?};
                let expected=std::array::from_fn::<_,4,_>(|i|c["output"][i].as_u64().unwrap() as u32);
                assert_eq!(actual.map(f32::to_bits),expected,"{key}: {c}");
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_dbc_rotation_keys_change_the_live_nif_camera_view()->Result<()> {
        use anyhow::Context;
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let name="dbc_1st_igc.DBC_visit1_lab_robotArm_death_cam 01";
        let entity=world.scripts.resolve_name(name).context("Original death camera missing")?;
        world.run_callback_fixture(&format!("AnimGBSetIdle({name:?},false); AnimGBReset({name:?}); AnimGBSequence({name:?},'death'); GrabCameraNif(GetEntity({name:?}),nil,1,0)"))?;
        world.actors.get_mut(&entity).context("Camera actor missing")?.update(&world.actor_sequences[&entity],&world.actor_bindings[&entity],0.,0.)?;
        let before=world.active_nif_camera_frame()?.context("Camera view not selected")?;
        world.actors.get_mut(&entity).unwrap().update(&world.actor_sequences[&entity],&world.actor_bindings[&entity],1.,1.)?;
        let after=world.active_nif_camera_frame()?.context("Animated camera disappeared")?;
        ensure!(before.direction!=after.direction,"Quaternion keys did not change the camera direction");
        ensure!(after.direction.iter().chain(after.eye.iter()).all(|v|v.is_finite()),"Camera frame became nonfinite");
        ensure!(world.actors[&entity].active_id().is_some(),"Camera actor lost its active clip");
        std::fs::write("target/nif-quaternion-camera-frames.json",serde_json::to_vec_pretty(&serde_json::json!({"scope":"Original camera resource, actor clock and camera-frame publication; not an authored cutscene playthrough","entity":entity,"name":name,"before":before,"after":after}))?)?;
        Ok(())
    }
}
