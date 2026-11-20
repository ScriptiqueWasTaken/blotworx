//!46a778/46a79c: direction to pitch/heading in degrees, using DOL math.
pub fn angles(axis:[f32;3],zero:impl FnOnce()->anyhow::Result<[f32;3]>)->anyhow::Result<[f32;3]>{
 anyhow::ensure!(axis.iter().all(|v|v.is_finite()),"Nonfinite native direction source");
 let [x,y,z]=axis;let length=super::cursor::native_sqrt(z*z+(x*x+y*y));
 let [x,y,z]=if length==0.{zero()?}else{[x/length,y/length,z/length]};
 let heading=if z.abs()>f32::from_bits(0x358637bd){let a=super::native_angle::atan((x/z) as f64) as f32;if z<0.{a+std::f32::consts::PI*if x<0.{-1.}else{1.}}else{a}}
 else if x>0.{std::f32::consts::FRAC_PI_2}else if x<0.{-std::f32::consts::FRAC_PI_2}else{0.};
 let pitch=super::native_angle::asin(y as f64) as f32;let radians=f32::from_bits(0x3c8efa35);
 Ok([if pitch.is_nan(){f32::from_bits(0x7fffffff)}else{pitch/radians},heading/radians,0.])
}
