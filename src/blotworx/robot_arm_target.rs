//! RobotArmGeneral target geometry5f3a4/5f51c and enabled attack choice5eed0.
use anyhow::{ensure,Result};
fn direction(source:[f32;3],target:[f32;3],horizontal:bool)->Result<(f32,[f32;4])> {
    let mut delta=[target[0]-source[0],if horizontal {0.}else{target[1]-source[1]},target[2]-source[2],0.];
    let length=super::lua_vector::Vector4(delta).length3().map_err(|e|anyhow::anyhow!("{e}"))?;
    if length>0.0001 {let scale=1./length;delta=delta.map(|v|v*scale);Ok((length,delta))} else {Ok((0.,delta))}
}
pub fn can_attack(source:[f32;3],target:[f32;3],facing:[f32;3])->Result<bool> {
    let (length,direction)=direction(source,target,true)?;
    if length<=f32::EPSILON {return Ok(false);}
    let dot=direction[2].mul_add(facing[2],direction[0]*facing[0])+direction[1]*facing[1];
    Ok(dot>0.95)
}
pub fn player_angle(source:[f32;3],target:[f32;3])->Result<f32> {
    let (_,direction)=direction(source,target,false)?;
    let angle=super::native_angle::atan2((-direction[0]) as f64,direction[2] as f64) as f32;
    Ok(angle*f32::from_bits(0x42652ee0))
}
pub fn choose(mask:u32,maximum:&mut i32,mut random:impl FnMut()->Result<i32>)->Result<i32> {
    if mask==0 {return Ok(-1);}
    if *maximum>=32 {*maximum=31;}
    ensure!(*maximum>=0,"Enabled arm mask has no initialized maximum index");
    // Native starts its counter at100 and decrements before every attempt.
    for _ in 0..99 {
        let random=random()?;
        ensure!((0..=32767).contains(&random),"Arm selection requires the original C rand domain");
        let index=random%(*maximum+1);
        if mask&(1u32<<index)!=0 {return Ok(index);}
    }
    Ok(-1)
}
