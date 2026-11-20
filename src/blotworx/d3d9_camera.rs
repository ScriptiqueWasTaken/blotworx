//! PC renderer camera caches, reconstructed from d3a880..d3ac62. These
//! matrices use D3D row vectors and authored frusta, never a GL round-trip.
use anyhow::{ensure, Result};
use super::{d3d9_predefined::{Cache,Matrix},pc_float::Precision};

#[derive(Debug,Clone,Copy)]
pub(super) struct Camera {
    pub eye:[f32;3],pub right:[f32;3],pub up:[f32;3],pub direction:[f32;3],
    pub frustum:[f32;6],pub orthographic:bool,pub left_handed:bool,
    pub viewport:[f32;4],pub precision:Precision,
}
#[derive(Debug)]
pub(super) struct Matrices {
    pub view:[f32;16],pub inverse_view:[f32;16],pub projection:[f32;16],
}

// Retain native intermediate precision until the original float store. In
// particular near*far*inverse_depth must not round after its first product.
fn arithmetic(a:f32,b:f32,c:f32,operation:u8,precision:Precision)->Result<f32> {
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut result=0f32;
        macro_rules! evaluate {($($instruction:literal),+) => {unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            $($instruction,)+
            "fstp dword ptr [{result}]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            a=in(reg) &a,b=in(reg) &b,c=in(reg) &c,result=in(reg) &mut result,
            options(nostack,preserves_flags)
        );}}}
        match operation {
            0=>evaluate!("fld dword ptr [{a}]","fdiv dword ptr [{b}]","/* {c} */"),
            1=>evaluate!("fld dword ptr [{a}]","fmul dword ptr [{b}]","/* {c} */"),
            2=>evaluate!("fld dword ptr [{a}]","fmul dword ptr [{b}]","fmul dword ptr [{c}]"),
            _=>unreachable!(),
        }
        ensure!(result.is_finite(),"PC camera cache arithmetic overflow");Ok(result)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(a,b,c,operation,precision);anyhow::bail!("PC shader camera needs a verified arithmetic backend on this architecture")}
}
fn dot(a:[f32;3],b:[f32;3],precision:Precision)->Result<f32> {
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut result=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{a}]","fmul dword ptr [{b}]",
            "fld dword ptr [{a}+4]","fmul dword ptr [{b}+4]","faddp st(1), st(0)",
            "fld dword ptr [{a}+8]","fmul dword ptr [{b}+8]","faddp st(1), st(0)",
            "fstp dword ptr [{result}]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            a=in(reg) a.as_ptr(),b=in(reg) b.as_ptr(),result=in(reg) &mut result,
            options(nostack,preserves_flags)
        );}
        ensure!(result.is_finite(),"PC camera translation overflow");Ok(result)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(a,b,precision);anyhow::bail!("PC shader camera needs a verified arithmetic backend on this architecture")}
}
impl Camera {
    pub fn matrices(self)->Result<Matrices> {
        ensure!(self.eye.iter().chain(&self.right).chain(&self.up).chain(&self.direction).chain(&self.frustum).chain(&self.viewport).all(|v|v.is_finite()),"Nonfinite PC camera source");
        let [left,right,top,bottom,near,far]=self.frustum;
        let width=right-left;let height=top-bottom;let depth=far-near;
        ensure!(width!=0.&&height!=0.&&depth!=0.&&[width,height,depth,right+left,top+bottom].iter().all(|v|v.is_finite()),"Degenerate PC camera frustum");
        let divide=|a,b|arithmetic(a,b,0.,0,self.precision);
        let product=|a,b|arithmetic(a,b,0.,1,self.precision);
        let inverse_depth=divide(1.,depth)?;
        let mut projection=[0.;16];
        projection[0]=divide(if self.left_handed {-2.}else{2.},width)?;
        projection[5]=divide(2.,height)?;
        let x=divide(if self.left_handed {right+left}else{-(right+left)},width)?;
        let y=divide(-(top+bottom),height)?;
        if self.orthographic {
            projection[12]=x;projection[13]=y;projection[10]=inverse_depth;
            projection[14]=-product(near,inverse_depth)?;projection[15]=1.;
        } else {
            projection[8]=x;projection[9]=y;
            projection[10]=product(far,inverse_depth)?;
            projection[14]=-arithmetic(far,near,inverse_depth,2,self.precision)?;
            projection[11]=1.;
        }
        let r=self.right;let u=self.up;let d=self.direction;let e=self.eye;
        let view=[r[0],u[0],d[0],0.,r[1],u[1],d[1],0.,r[2],u[2],d[2],0.,-dot(e,r,self.precision)?,-dot(e,u,self.precision)?,-dot(e,d,self.precision)?,1.];
        let inverse_view=[r[0],r[1],r[2],0.,u[0],u[1],u[2],0.,d[0],d[1],d[2],0.,e[0],e[1],e[2],1.];
        Ok(Matrices {view,inverse_view,projection})
    }
    pub fn cache(self,world:[f32;16])->Result<Cache> {
        let matrices=self.matrices()?;
        let mut cache=Cache::compose(world,matrices.view,matrices.projection,
            [self.eye[0],self.eye[1],self.eye[2],1.],
            [self.direction[0],self.direction[1],self.direction[2],0.],self.viewport)?;
        cache.set_inverse(Matrix::View,matrices.inverse_view)?;
        Ok(cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_caches_match_original_pc_instructions()->Result<()> {
        let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_renderer_camera.json"))?;
        fn floats<const N:usize>(case:&serde_json::Value,name:&str)->[f32;N] {
            std::array::from_fn(|i|case[name][i].as_f64().unwrap() as f32)
        }
        for (index,case) in reference["cases"].as_array().unwrap().iter().enumerate() {
            let camera=Camera {
                eye:floats(case,"eye"),right:floats(case,"right"),up:floats(case,"up"),direction:floats(case,"direction"),frustum:floats(case,"frustum"),
                orthographic:case["orthographic"].as_bool().unwrap(),left_handed:case["left_handed"].as_bool().unwrap(),
                viewport:[0.125,0.875,0.75,0.25],
                precision:match case["precision"].as_u64().unwrap() {0x7f=>Precision::Single,0x27f=>Precision::Double,0x37f=>Precision::Extended,_=>panic!("Unknown reference precision")},
            };
            let matrices=camera.matrices()?;
            for (name,value) in [("view",matrices.view),("inverse_view",matrices.inverse_view),("projection",matrices.projection)] {
                for (lane,value) in value.iter().enumerate() {
                    assert_eq!(value.to_bits(),case[name][lane].as_u64().unwrap() as u32,"case {index}, {name}[{lane}]");
                }
            }
            let cache=camera.cache(std::array::from_fn(|i|if i%5==0 {1.}else{0.}))?;
            assert!(cache.resolve("InvView")?.is_some());
            assert!(cache.resolve("InvWorld").is_err());
        }
        Ok(())
    }
}
