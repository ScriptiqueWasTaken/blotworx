//! Retail listener-space projection and zero-spread mono four-channel panner.
//! Single game-position voices apply authored distance attenuation and follow
//! retained emitter/listener state. Cones/spread/multiple sources remain gated.
use anyhow::{ensure,Result};
use std::sync::Arc;
#[derive(Clone,Debug)]
pub struct Definition {
    attenuation:Option<Arc<super::audio_bank::attenuation::Attenuation>>,
    pub dynamic:bool,
    spatialized:bool,
}
impl Definition {
    pub fn from_mode(bank:&super::audio_bank::Bank<'_>,mode:&super::audio_bank::positioning::Mode)->Result<Self> {
        use super::audio_bank::positioning::{Mode,SpatialSource};
        let Mode::ThreeD{attenuation,spatialized,source:SpatialSource::Game{dynamic},..}=mode else {anyhow::bail!("Authored spatial paths/source mode are not connected")};
        let attenuation=bank.objects.get(attenuation).map(|node|node.attenuation().map(Arc::new)).transpose()?;
        if let Some(a)=&attenuation {
            a.validate_native_definition()?;
            ensure!(a.cone.is_none() && a.rtpc.is_empty(),"Spatial cone/RTPC playback is not connected");
            ensure!(a.mapped_curve(4)?.is_none(),"Spatial spread playback is not connected");
        }
        Ok(Self{attenuation,dynamic:*dynamic,spatialized:*spatialized})
    }
    pub fn low_pass(&self,emitter:&super::audio_distance::Emitter,listeners:&super::audio_listener::Listeners,tables:&super::audio_transition::Tables)->Result<f32> {
        ensure!(emitter.listener_mask()==1 && emitter.relative_listener()==-1 && emitter.mode()==0 && emitter.positions().len()==1,"Spatial filter requires one game position and listener");
        let listener=&listeners.records()[0];
        let scale=listener.scale*emitter.radius_scale();ensure!(scale.is_finite() && scale>0.,"Invalid spatial radius scale");
        let distance=super::audio_distance::separation(emitter.positions()[0].position,listener.pose.position)?/scale;
        Ok(self.attenuation.as_ref().map(|a|a.evaluate(3,distance,tables)).transpose()?.flatten().unwrap_or(0.))
    }
    pub fn channels(&self,emitter:&super::audio_distance::Emitter,listeners:&super::audio_listener::Listeners,tables:&super::audio_transition::Tables)->Result<[f32;4]> {
        ensure!(emitter.listener_mask()==1 && emitter.relative_listener()==-1,"Multiple/relative audio listeners are not connected");
        ensure!(emitter.mode()==0 && emitter.positions().len()==1,"Spatial sound requires one registered game position; multi-position mixing is not connected");
        let listener=&listeners.records()[0];let source=emitter.positions()[0].position;
        let scale=listener.scale*emitter.radius_scale();ensure!(scale.is_finite() && scale>0.,"Invalid spatial radius scale");
        let distance=super::audio_distance::separation(source,listener.pose.position)?/scale;
        let attenuation=self.attenuation.as_ref().map(|a|a.evaluate(0,distance,tables)).transpose()?.flatten().unwrap_or(0.);
        let mut channels=if listener.spatialization==0 {listener.channel_gains}else if self.spatialized {
            let [x,z]=project(listener.pose.position,source,listener.basis);pan(x,z)?
        }else {
            // Native non-spatialized3D uses the centered2D matrix, then
            //8037b300 converts each linear channel to dB.
            let value=20.*logarithm(0.5);[value;4]
        };
        for value in &mut channels {*value+=attenuation;}Ok(channels)
    }
}
pub fn stereo(base:f32,channels:[f32;4])->Result<[f32;2]> {
    let mut quad=[0.;4];for i in 0..4 {quad[i]=super::audio_panner::output_gain(channels[i]+base)?;}
    super::audio_panner::stereo_fold(quad)
}
/// REL258: Game microphone position blends player -> camera; tilted views
/// flatten forward and restore world-up before conversion to Wwise coordinates.
pub fn game_pose(player:[f32;3],eye:[f32;3],mut forward:[f32;3],mut up:[f32;3],ratio:f32)->Result<super::audio_listener::Pose> {
    ensure!(player.iter().chain(eye.iter()).chain(forward.iter()).chain(up.iter()).all(|v|v.is_finite()) && ratio.is_finite(),"Nonfinite microphone pose");
    let position=std::array::from_fn::<_,3,_>(|i|player[i]+(eye[i]-player[i])*ratio);
    if (up[1]-1.).abs()>1e-4 {
        forward[1]=0.;let squared=forward[2].mul_add(forward[2],forward[0]*forward[0])+0.;
        if squared>0. {
            let estimate=super::pose_blend::estimate_double(squared);let bits=estimate.to_bits();let operand=f64::from_bits(((bits>>28)+((bits>>27)&1))<<28);
            let square=(estimate*operand) as f32;let half=(estimate*0.5) as f32;let factor=(-square.mul_add(squared,-3.))*half;
            forward=if squared*factor>1e-4 {forward.map(|v|v*factor)}else {[0.;3]};
        }else {forward=[0.;3];}
        up=[0.,1.,0.];
    }
    let flip=|v:[f32;3]|[-v[0],v[1],v[2]];
    Ok(super::audio_listener::Pose{position:flip(position),forward:flip(forward),up:flip(up)})
}
pub fn project(listener:[f32;3],source:[f32;3],basis:[[f32;3];3])->[f32;2] {
    let d=std::array::from_fn::<_,3,_>(|i|source[i]-listener[i]);
    [(basis[0][0]*d[0]+basis[0][1]*d[1])+basis[0][2]*d[2],(basis[2][0]*d[0]+basis[2][1]*d[1])+basis[2][2]*d[2]]
}
fn logarithm(value:f32)->f32 {
    //80378500 approximates log10 with original bit decomposition, not libm.
    let bits=value.to_bits();let mantissa=f32::from_bits(bits|0x3f800000);
    let p=(mantissa-1.)/(1.+mantissa);let exponent=((bits>>23)&255) as f32-127.;
    let mut a=f32::from_bits(0x3eaaaaab)*p;a*=p;a=p+p*a;
    let b=f32::from_bits(0x3f317218)*exponent;
    f32::from_bits(0x3ede5bd9)*(2.*a+b)
}
pub fn pan(x:f32,z:f32)->Result<[f32;4]> {
    let squared=x*x+z*z;
    ensure!(x.is_finite() && z.is_finite() && squared.is_finite(),"Nonfinite spatial panner input");
    if squared==0. {return Ok([-6.;4]);}
    ensure!(squared.is_normal(),"Subnormal spatial panner needs native FPSCR validation");
    let half=0.5*squared;let mut inverse=super::pose_blend::estimate(squared);
    inverse=inverse*(1.5-half*(inverse*inverse));inverse=inverse*(1.5-half*(inverse*inverse));
    let nx=x*inverse;let nz=z*inverse;
    Ok(if x>0. {if z>0. {[20.*logarithm(nz),20.*logarithm(nx),-97.,-97.]}else {[-97.,20.*logarithm(nx),-97.,20.*logarithm(-nz)]}}
    else if z>0. {[20.*logarithm(nz),-97.,20.*logarithm(-nx),-97.]}else {[-97.,-97.,20.*logarithm(-nx),20.*logarithm(-nz)]})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_zero_spread_panner_and_projection_float_bits()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/audio_spatial.json"))?;
        let bits=|value:&serde_json::Value|value.as_array().unwrap().iter().map(|x|x.as_u64().unwrap() as u32).collect::<Vec<_>>();
        for case in fixture["panning"].as_array().unwrap() {let input=bits(&case["input_bits"]);assert_eq!(pan(f32::from_bits(input[0]),f32::from_bits(input[1]))?.map(f32::to_bits).to_vec(),bits(&case["output_bits"]),"{case}");}
        for case in fixture["projections"].as_array().unwrap() {let vector=|field:&str|bits(&case[field]).into_iter().map(f32::from_bits).collect::<Vec<_>>();let listener=vector("listener_bits").try_into().unwrap();let source=vector("source_bits").try_into().unwrap();let b=vector("basis_bits");let basis=std::array::from_fn(|row|std::array::from_fn(|col|b[row*3+col]));assert_eq!(project(listener,source,basis).map(f32::to_bits).to_vec(),bits(&case["output_bits"]));}Ok(())
    }
}
