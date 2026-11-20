//! PC af40f0 control ordering and af68e0 literal-pose conversion.
//! Final physics/controller lifecycle is owned by the caller, not this decoder.
use anyhow::{ensure,Result};
#[derive(Clone,Copy,Debug,serde::Serialize)]
pub(super) enum Operation {
    PauseAi(bool),Hidden {child:bool,hidden:bool},Motion(bool),
    Location([bool;3]),Target([bool;2]),AnimationCulling(bool),
}
pub(super) fn operations(flags:u32,values:u32)->Vec<Operation> {
    let has=|bit|flags&bit!=0;let value=|bit|values&bit!=0;let mut out=Vec::new();
    if has(0x100){out.push(Operation::PauseAi(value(0x100)));}
    if has(1){out.push(Operation::Hidden {child:has(4),hidden:!value(1)});}
    if has(0x40){out.push(Operation::Motion(value(0x40)));}
    if has(0x20){out.push(Operation::Location([value(8),value(16),value(32)]));}
    else if has(2){out.push(Operation::Target([value(2),value(4)]));}
    if has(0x80){out.push(Operation::AnimationCulling(value(0x80)));}out
}
fn multiply(a:[f32;4],b:[f32;4])->[f32;4] {
    let [x,y,z,w]=a;let [u,v,t,s]=b;
    [w*u+x*s+y*t-z*v,w*v-x*t+y*s+z*u,w*t+x*v-y*u+z*s,w*s-x*u-y*v-z*t]
}
pub(super) fn rotation(degrees:[f32;3],current:[f32;9],keep:bool)->Result<[f32;9]> {
    ensure!(degrees.iter().chain(&current).all(|x|x.is_finite()),"Nonfinite IGC control rotation");
    if keep{return Ok(current);}
    // PC12007f8 is float PI promoted to double, not double-precision PI.
    let q=degrees.map(|d|{let angle=(d as f64*std::f32::consts::PI as f64/180.) as f32;(angle*0.5).sin_cos()});
    let [x,y,z,w]=multiply([q[0].0,0.,0.,q[0].1],multiply([0.,q[1].0,0.,q[1].1],[0.,0.,q[2].0,q[2].1]));
    Ok([1.-2.*(y*y+z*z),2.*(x*y-z*w),2.*(x*z+y*w),
        2.*(x*y+z*w),1.-2.*(x*x+z*z),2.*(y*z-x*w),
        2.*(x*z-y*w),2.*(y*z+x*w),1.-2.*(x*x+y*y)])
}
#[derive(Clone,Debug,serde::Serialize)]
pub(super) enum PlayerWrite {
    Motion(bool),Pose {position:[f32;3],rotation:[f32;9],ground_snap:bool},AnimationCulling(bool),AnimationEvent(String),AnimationClip{resource:String,node:String},
}

/// PC 98eef0 starts its sphere 0.01 above the requested location and ends
/// four units below the unmodified location. The mutation survives a miss.
pub(super) fn ground_sweep(position:[f32;3])->Result<([f32;3],[f32;3])> {
    ensure!(position.into_iter().all(f32::is_finite),"Nonfinite IGC teleport location");
    let mut start=position;start[1]=(position[1] as f64+0.009999999776482582)as f32;
    let mut end=position;end[1]=position[1]-4.;
    Ok((start,end))
}
/// PC 98eef0 scans sorted hits, excludes layer3, and requires normal.y>0.83.
/// PC 96ec90 subtracts the selected distance only when strictly less than4.
/// Query admission, source identity and sphere contact points belong to caller.
pub(super) fn grounded_position(mut start:[f32;3],hits:impl IntoIterator<Item=(f32,f32,u32)>)->Result<[f32;3]> {
    for (height,normal_y,filter) in hits {
        ensure!(height.is_finite()&&normal_y.is_finite(),"Nonfinite IGC ground hit");
        if filter&31==3 || normal_y as f64<=0.8299999833106995 {continue;}
        let distance=(start[1]-height).max(0.);
        if distance<4. {start[1]-=distance;}
        break;
    }Ok(start)
}
