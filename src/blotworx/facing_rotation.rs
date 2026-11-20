//! REL2996b0: normalized direction -> yaw/pitch matrix, roll=0.
use anyhow::{Result,ensure};
pub fn from_direction(direction:[f32;4])->Result<[f32;9]> {
    ensure!(direction.iter().all(|v|v.is_finite()),"Nonfinite transformation direction");
    let yaw=-(super::native_angle::atan2(direction[0] as f64,direction[2] as f64) as f32);
    let pitch=if direction[1]<=-1. {-std::f32::consts::FRAC_PI_2}
        else if direction[1]>=1. {std::f32::consts::FRAC_PI_2}
        else {super::native_angle::asin(direction[1] as f64) as f32};
    let (sy,cy)=super::native_angle::sin_cos_medium(yaw as f64)?;
    let (sp,cp)=super::native_angle::sin_cos_medium(pitch as f64)?;
    let (sy,cy,sp,cp)=(sy as f32,cy as f32,sp as f32,cp as f32);
    // DOL804c5750 constructs Y*(X*Z); its Z rotation is zero here.
    Ok(super::native_matrix::multiply(&[cy,0.,-sy,0.,1.,0.,sy,0.,cy],
        &[1.,0.,0.,0.,cp,sp,0.,-sp,cp]))
}
