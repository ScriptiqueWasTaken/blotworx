//! Reconstructed PC x87 arithmetic with explicit precision and preserved host FP state.
use anyhow::{ensure,Result};
/// Required input, not a guessed global. The original PC render thread's
/// effective x87 precision still needs a live reference capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Precision { Single, Double, Extended }
/// PC NiTextureTransform4b6980, with raw [tx,ty,rotation,sx,sy,cx,cy].
/// Returns the row-major NiMatrix3 before D3D shader packing.
pub(super) fn texture_matrix(values:[f32;7],method:u32,precision:Precision)->Result<[f32;9]> {
    ensure!(values.iter().all(|v|v.is_finite())&&method<=2,"Unsupported PC texture transform source");
    let (sin,cos)=sincos(values[2],precision)?;let trig=[sin,cos,-sin];
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let mut result=[0f32;9];result[8]=1.;
        macro_rules! evaluate {($($instruction:literal),+) => {unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",$($instruction,)+"fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            v=in(reg) values.as_ptr(),t=in(reg) trig.as_ptr(),out=in(reg) result.as_mut_ptr(),options(nostack,preserves_flags)
        );}}}
        match method {
            0=>evaluate!(
                "fld dword ptr [{v}]","fsub dword ptr [{v}+20]","fmul dword ptr [{t}+4]",
                "fld dword ptr [{v}+4]","fsub dword ptr [{v}+24]","fmul dword ptr [{t}+8]","faddp st(1), st(0)","fadd dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
                "fld dword ptr [{v}]","fsub dword ptr [{v}+20]","fmul dword ptr [{t}]",
                "fld dword ptr [{v}+4]","fsub dword ptr [{v}+24]","fmul dword ptr [{t}+4]","faddp st(1), st(0)","fadd dword ptr [{v}+24]","fstp dword ptr [{out}+20]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}+4]","fstp dword ptr [{out}]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+8]","fstp dword ptr [{out}+4]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}]","fstp dword ptr [{out}+12]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+4]","fstp dword ptr [{out}+16]"
            ),
            1=>evaluate!(
                "fld dword ptr [{v}+20]","fchs","fsub dword ptr [{v}]","fmul dword ptr [{t}+4]",
                "fld dword ptr [{v}+4]","fsub dword ptr [{v}+24]","fmul dword ptr [{t}]","faddp st(1), st(0)","fmul dword ptr [{v}+12]","fadd dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
                "fld dword ptr [{v}+4]","fsub dword ptr [{v}+24]","fmul dword ptr [{t}+4]",
                "fld dword ptr [{v}+20]","fchs","fsub dword ptr [{v}]","fmul dword ptr [{t}]","fsubp st(1), st(0)","fmul dword ptr [{v}+16]","fadd dword ptr [{v}+24]","fstp dword ptr [{out}+20]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}+4]","fstp dword ptr [{out}]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}]","fstp dword ptr [{out}+4]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+8]","fstp dword ptr [{out}+12]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+4]","fstp dword ptr [{out}+16]"
            ),
            2=>evaluate!(
                "fld dword ptr [{v}]","fsub dword ptr [{v}+20]","fmul dword ptr [{t}+4]",
                "fld dword ptr [{v}+4]","fadd dword ptr [{v}+24]","fld1","fsubrp st(1), st(0)","fmul dword ptr [{t}]","faddp st(1), st(0)","fadd dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
                "fld dword ptr [{v}+20]","fsub dword ptr [{v}]","fmul dword ptr [{t}]",
                "fld dword ptr [{v}+4]","fadd dword ptr [{v}+24]","fld1","fsubrp st(1), st(0)","fmul dword ptr [{t}+4]","faddp st(1), st(0)","fadd dword ptr [{v}+24]","fstp dword ptr [{out}+20]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}+4]","fstp dword ptr [{out}]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+8]","fstp dword ptr [{out}+4]",
                "fld dword ptr [{v}+12]","fmul dword ptr [{t}+8]","fstp dword ptr [{out}+12]",
                "fld dword ptr [{v}+16]","fmul dword ptr [{t}+4]","fchs","fstp dword ptr [{out}+16]"
            ),
            _=>unreachable!(),
        }
        ensure!(result.iter().all(|v|v.is_finite()),"PC texture matrix overflow");Ok(result)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(trig,precision);anyhow::bail!("PC texture transform needs a verified arithmetic backend on this architecture")}
}
/// StaticLightingSpecularDirection publication11435d0 ->402540. The sum,
/// square root and reciprocal each have an original float store; replacing
/// these with a single SIMD normalize changes the published shader values.
pub(super) fn shader_light_direction(vector:[f32;3],precision:Precision)->Result<[f32;3]> {
    ensure!(vector.iter().all(|v|v.is_finite()),"Nonfinite static light direction source");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let threshold=1e-6f32;let mut squared=0f32;let mut length=0f32;let mut reciprocal=0f32;let mut output=[0f32;3];
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{vector}]","fmul dword ptr [{vector}]",
            "fld dword ptr [{vector}+4]","fmul dword ptr [{vector}+4]","faddp st(1), st(0)",
            "fld dword ptr [{vector}+8]","fmul dword ptr [{vector}+8]","faddp st(1), st(0)",
            "fstp dword ptr [{squared}]","fld dword ptr [{squared}]","fsqrt","fstp dword ptr [{length}]",
            "fld dword ptr [{length}]","fcom dword ptr [{threshold}]","fnstsw ax","fstp st(0)",
            "test ah, 0x41","jnz 2f",
            "fld1","fdiv dword ptr [{length}]","fstp dword ptr [{reciprocal}]",
            "fld dword ptr [{vector}]","fmul dword ptr [{reciprocal}]","fstp dword ptr [{output}]",
            "fld dword ptr [{vector}+4]","fmul dword ptr [{reciprocal}]","fstp dword ptr [{output}+4]",
            "fld dword ptr [{vector}+8]","fmul dword ptr [{reciprocal}]","fstp dword ptr [{output}+8]","jmp 3f",
            "2:","fldz","fst dword ptr [{output}]","fst dword ptr [{output}+4]","fstp dword ptr [{output}+8]",
            "3:","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,vector=in(reg) vector.as_ptr(),
            squared=in(reg) &mut squared,length=in(reg) &mut length,reciprocal=in(reg) &mut reciprocal,
            threshold=in(reg) &threshold,output=in(reg) output.as_mut_ptr(),out("ax") _,options(nostack)
        );}
        ensure!(output.iter().all(|v|v.is_finite()),"Static light direction normalization failed");Ok(output)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC static light direction needs a verified arithmetic backend on this architecture")}
}
impl Precision {
    pub(super) fn control(self)->u16 { match self {Self::Single=>0x007f,Self::Double=>0x027f,Self::Extended=>0x037f} }
}
/// f386ad..f38724: each dot-product accumulation is stored to float.
pub(super) fn dot_stored(a:[f32;3],b:[f32;3],precision:Precision)->Result<f32> {
    ensure!(a.iter().chain(&b).all(|v|v.is_finite()),"Nonfinite PC projection vector");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))] struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let mut result=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]", "fninit", "fldcw [{control}]",
            "fld dword ptr [{a}]", "fmul dword ptr [{b}]", "fldz", "faddp st(1), st(0)", "fstp dword ptr [{result}]",
            "fld dword ptr [{a}+4]", "fmul dword ptr [{b}+4]", "fadd dword ptr [{result}]", "fstp dword ptr [{result}]",
            "fld dword ptr [{a}+8]", "fmul dword ptr [{b}+8]", "fadd dword ptr [{result}]", "fstp dword ptr [{result}]",
            "fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,a=in(reg) a.as_ptr(),b=in(reg) b.as_ptr(),result=in(reg) &mut result,
            options(nostack,preserves_flags)
        );}
        ensure!(result.is_finite(),"PC projection overflows finite storage");Ok(result)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC camera projection needs a verified backend on this architecture")}
}

/// ab3d15..ab3d95: rotate the tracker offset, store each float, then add
/// translation. This does not apply entity/model scale to TargetOffset.
pub(super) fn target_point(position:[f32;3],rotation:[f32;9],offset:[f32;3],precision:Precision)->Result<([f32;3],[f32;3])> {
    ensure!(position.iter().chain(&rotation).chain(&offset).all(|v|v.is_finite()),"Nonfinite PC tracker pose");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))] struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut rotated=[0f32;3];let mut point=[0f32;3];
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]", "fninit", "fldcw [{control}]",
            "fld dword ptr [{offset}]", "fmul dword ptr [{rotation}]",
            "fld dword ptr [{offset}+4]", "fmul dword ptr [{rotation}+4]", "faddp st(1), st(0)",
            "fld dword ptr [{offset}+8]", "fmul dword ptr [{rotation}+8]", "faddp st(1), st(0)", "fstp dword ptr [{rotated}]",
            "fld dword ptr [{offset}]", "fmul dword ptr [{rotation}+12]",
            "fld dword ptr [{offset}+4]", "fmul dword ptr [{rotation}+16]", "faddp st(1), st(0)",
            "fld dword ptr [{offset}+8]", "fmul dword ptr [{rotation}+20]", "faddp st(1), st(0)", "fstp dword ptr [{rotated}+4]",
            "fld dword ptr [{offset}]", "fmul dword ptr [{rotation}+24]",
            "fld dword ptr [{offset}+4]", "fmul dword ptr [{rotation}+28]", "faddp st(1), st(0)",
            "fld dword ptr [{offset}+8]", "fmul dword ptr [{rotation}+32]", "faddp st(1), st(0)", "fstp dword ptr [{rotated}+8]",
            "fld dword ptr [{position}]", "fadd dword ptr [{rotated}]", "fstp dword ptr [{point}]",
            "fld dword ptr [{rotated}+4]", "fadd dword ptr [{position}+4]", "fstp dword ptr [{point}+4]",
            "fld dword ptr [{rotated}+8]", "fadd dword ptr [{position}+8]", "fstp dword ptr [{point}+8]",
            "fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            position=in(reg) position.as_ptr(),rotation=in(reg) rotation.as_ptr(),offset=in(reg) offset.as_ptr(),
            rotated=in(reg) rotated.as_mut_ptr(),point=in(reg) point.as_mut_ptr(),options(nostack,preserves_flags)
        );}
        ensure!(rotated.iter().chain(&point).all(|v|v.is_finite()),"PC tracker pose overflows finite storage");Ok((point,rotated))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC tracker x87 arithmetic needs a verified backend on this architecture")}
}

/// Execute the reconstructed x87 operation sequence at an explicit precision.
/// FXSAVE/FXRSTOR preserve the caller's entire legacy FP state. No original
/// executable instructions or game code are executed by the shipped runtime.
pub(super) fn blend(outgoing:f32,incoming:f32,weight:f32,precision:Precision)->Result<f32> {
    ensure!([outgoing,incoming,weight].iter().all(|v|v.is_finite()),"Nonfinite PC camera blend input");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))] struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut result=0.;
        // All x87 registers are private to this block; restoring the saved
        // context also restores status/control/tag words and XMM registers.
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]", "fninit", "fldcw [{control}]",
            "fld dword ptr [{outgoing}]", "fld dword ptr [{weight}]", "fld1",
            "fsubrp st(1), st(0)", "fmulp st(1), st(0)",
            "fld dword ptr [{incoming}]", "fmul dword ptr [{weight}]",
            "faddp st(1), st(0)", "fstp dword ptr [{result}]", "fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            outgoing=in(reg) &outgoing,incoming=in(reg) &incoming,weight=in(reg) &weight,result=in(reg) &mut result,
            options(nostack,preserves_flags)
        );}
        ensure!(f32::is_finite(result),"PC camera blend overflows finite storage");Ok(result)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC camera x87 arithmetic needs a verified backend on this architecture")}
}

pub(super) fn clock(elapsed:f32,delta:f32,duration:f32,precision:Precision)->Result<(f32,f32)> {
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))] struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut next=0f32;let mut ratio=0f32;let mut weight=0f32;let three=3f64;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]", "fninit", "fldcw [{control}]",
            "fld dword ptr [{delta}]", "fadd dword ptr [{elapsed}]", "fstp dword ptr [{next}]",
            "fld dword ptr [{next}]", "fdiv dword ptr [{duration}]", "fstp dword ptr [{ratio}]",
            "fld1", "fld dword ptr [{ratio}]", "fcomi st(0), st(1)", "jbe 2f",
            "fstp st(0)", "fld1", "2:", "fstp st(1)", "fstp dword ptr [{ratio}]",
            "fld dword ptr [{ratio}]", "fld st(0)", "fadd st(0), st(0)", "fsubr qword ptr [{three}]",
            "fld st(1)", "fmulp st(2), st(0)", "fmulp st(1), st(0)", "fstp dword ptr [{weight}]",
            "fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            elapsed=in(reg) &elapsed,delta=in(reg) &delta,duration=in(reg) &duration,
            next=in(reg) &mut next,ratio=in(reg) &mut ratio,weight=in(reg) &mut weight,three=in(reg) &three,
            options(nostack)
        );}
        ensure!(next.is_finite()&&weight.is_finite(),"PC camera clock overflows finite storage");Ok((next,weight))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(elapsed,delta,duration,precision);anyhow::bail!("PC camera x87 clock needs a verified backend on this architecture")}
}


/// NiMatrix3::FromEulerAnglesXYZ (4a4380) stores both fsincos outputs as
/// float before constructing the axis matrices.
pub(super) fn sincos(angle:f32,precision:Precision)->Result<(f32,f32)> {
    ensure!(angle.is_finite(),"Nonfinite PC Euler angle");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut sine=0f32;let mut cosine=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{angle}]","fsincos","fstp dword ptr [{cosine}]","fstp dword ptr [{sine}]",
            "fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            angle=in(reg) &angle,sine=in(reg) &mut sine,cosine=in(reg) &mut cosine,
            options(nostack,preserves_flags)
        );}
        ensure!(sine.is_finite()&&cosine.is_finite(),"PC Euler sine/cosine failed");Ok((sine,cosine))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC Euler calculation needs a verified backend on this architecture")}
}

/// PC CRT asin interior branch8e59fc and atan2 for finite directions.
/// X87 retains caller precision; SSE2 uses double arithmetic. Callers must
/// supply the original CRT dispatch path, not infer it from x87 precision alone.
#[derive(Clone,Copy,Debug)]
pub(super) enum FacingMath { X87, Sse2 }
pub(super) fn facing_angles(direction:[f32;3],precision:Precision,math:FacingMath)->Result<(f32,f32)> {
    ensure!(direction.iter().all(|v|v.is_finite()),"Nonfinite PC facing direction");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();
        let control=precision.control();let mut yaw=0f32;let mut pitch=0f32;
        if direction[1]<=-1. {pitch=-std::f32::consts::FRAC_PI_2;}
        else if direction[1]>=1. {pitch=std::f32::consts::FRAC_PI_2;}
        else if matches!(math,FacingMath::Sse2) {pitch=(direction[1] as f64).asin() as f32;}
        else {unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{direction}+4]","fld1","fadd st(0), st(1)",
            "fld1","fsub st(0), st(2)","fmulp st(1), st(0)","fsqrt","fpatan",
            "fstp dword ptr [{pitch}]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            direction=in(reg) direction.as_ptr(),pitch=in(reg) &mut pitch,
            options(nostack,preserves_flags)
        );}}
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{direction}]","fld dword ptr [{direction}+8]","fpatan",
            "fstp dword ptr [{yaw}]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            direction=in(reg) direction.as_ptr(),yaw=in(reg) &mut yaw,
            options(nostack,preserves_flags)
        );}
        ensure!(yaw.is_finite()&&pitch.is_finite(),"PC facing angles failed");Ok((-yaw,pitch))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC facing angles need a verified backend on this architecture")}
}

/// PCab4cf0 averages stored-float sums, then a stored-float reciprocal.
pub(super) fn history_average(points:&[[f32;3]],precision:Precision)->Result<[f32;3]> {
    ensure!(!points.is_empty()&&points.len()<=i32::MAX as usize,"Invalid PC tracker history count");
    ensure!(points.iter().flatten().all(|v|v.is_finite()),"Nonfinite PC history");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();
        let count=points.len() as i32;let mut remaining=points.len();let mut cursor=points.as_ptr();let mut out=[0f32;3];let mut factor=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "2:",
            "fld dword ptr [{cursor}]","fadd dword ptr [{out}]","fstp dword ptr [{out}]",
            "fld dword ptr [{cursor}+4]","fadd dword ptr [{out}+4]","fstp dword ptr [{out}+4]",
            "fld dword ptr [{cursor}+8]","fadd dword ptr [{out}+8]","fstp dword ptr [{out}+8]",
            "add {cursor},12","dec {remaining}","jnz 2b",
            "fild dword ptr [{count}]","fld1","fdivrp st(1),st(0)","fstp dword ptr [{factor}]",
            "fld dword ptr [{factor}]","fld st(0)","fmul dword ptr [{out}]","fstp dword ptr [{out}]",
            "fld st(0)","fmul dword ptr [{out}+4]","fstp dword ptr [{out}+4]",
            "fmul dword ptr [{out}+8]","fstp dword ptr [{out}+8]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            cursor=inout(reg) cursor=>_,remaining=inout(reg) remaining=>_,count=in(reg) &count,
            out=in(reg) out.as_mut_ptr(),factor=in(reg) &mut factor,options(nostack)
        );}
        ensure!(out.iter().all(|v|v.is_finite()),"PC history average overflow");Ok(out)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC history averaging needs verified architecture support")}
}
/// PCab4e96: no delta-time multiplier and no FMA.
pub(super) fn ground_blend(height:f32,previous:f32,rate:f32,precision:Precision)->Result<f32> {
    ensure!([height,previous,rate].iter().all(|v|v.is_finite()),"Nonfinite PC ground blend");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let mut out=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{height}]","fsub dword ptr [{previous}]","fmul dword ptr [{rate}]","fadd dword ptr [{previous}]","fstp dword ptr [{out}]","fxrstor64 [{environment}]",
            environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,height=in(reg) &height,
            previous=in(reg) &previous,rate=in(reg) &rate,out=in(reg) &mut out,options(nostack,preserves_flags)
        );}
        ensure!(out.is_finite(),"PC ground blend overflow");Ok(out)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC ground blending needs verified architecture support")}
}

/// Two float inputs with one native x87 float store.
pub(super) fn sum(a:f32,b:f32,precision:Precision)->Result<f32> {
 ensure!(a.is_finite()&&b.is_finite(),"Nonfinite PC stored sum");
 #[cfg(target_arch="x86_64")]
 {
  #[repr(align(16))]struct Environment([u8;512]);let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let mut out=0f32;
  unsafe {std::arch::asm!("fxsave64 [{env}]","fninit","fldcw [{cw}]","fld dword ptr [{a}]","fadd dword ptr [{b}]","fstp dword ptr [{out}]","fxrstor64 [{env}]",env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,a=in(reg) &a,b=in(reg) &b,out=in(reg) &mut out,options(nostack,preserves_flags));}
  ensure!(out.is_finite(),"PC stored sum overflow");Ok(out)
 }
 #[cfg(not(target_arch="x86_64"))]
 {let _=precision;anyhow::bail!("PC stored sum needs verified architecture support")}
}

/// Original PC JPSLightComponent::RecalculatePointLightAttenuation ab5e50.
/// model: Deprecated0, Constant1, Linear2, Quadratic3, Blend4, unknown5.
/// The old fourth field survives the deprecated path. Positive-range math
/// retains its x87 intermediate precision until each native field store.
pub(super) fn point_attenuation(model:u8,range:f32,shape:f32,legacy:[f32;3],previous:[f32;4],precision:Precision)->Result<[f32;4]> {
    ensure!(model<=5&&range.is_finite()&&shape.is_finite()&&legacy.iter().chain(&previous).all(|v|v.is_finite()),"Unsupported PC point attenuation input");
    if model==0 {return Ok([legacy[0],legacy[1],legacy[2],previous[3]]);}
    if model==1||range<=0. {return Ok([1.,0.,0.,range]);}
    let blend=match model {3=>1.,4=>shape,_=>0.};
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let mut out=[1.,0.,0.,range];
        unsafe {std::arch::asm!(
            "fxsave64 [{env}]","fninit","fldcw [{cw}]",
            "fld dword ptr [{range}]","fld dword ptr [{blend}]","fld st(0)","fld1","fsubrp st(1), st(0)",
            "fdiv st(0), st(2)","fstp dword ptr [{out}+4]",
            "fld st(1)","fmul st(0), st(0)","fdivp st(1), st(0)","fstp dword ptr [{out}+8]",
            "fstp dword ptr [{out}+12]","fxrstor64 [{env}]",
            env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,
            range=in(reg) &range,blend=in(reg) &blend,out=in(reg) out.as_mut_ptr(),options(nostack,preserves_flags)
        );}
        ensure!(out.iter().all(|v|v.is_finite()),"PC point attenuation overflow");Ok(out)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(blend,precision);anyhow::bail!("PC point attenuation needs a verified arithmetic backend on this architecture")}
}

/// PC RecreateLight half-angle store followed by NiSpotLight angle setters.
/// The original radians calculation uses a double holding FLOAT pi, then a
/// float radians store before the native cosine call.
pub(super) fn spot_angle(degrees:f32,precision:Precision)->Result<(f32,f32)> {
    ensure!(degrees.is_finite(),"Nonfinite native spot angle");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();
        let half_factor=0.5f64;let pi=3.1415927410125732f64;let divisor=180f64;let mut half=0f32;let mut radians=0f32;
        unsafe {std::arch::asm!(
            "fxsave64 [{env}]","fninit","fldcw [{cw}]",
            "fld dword ptr [{degrees}]","fmul qword ptr [{factor}]","fstp dword ptr [{half}]",
            "fld dword ptr [{half}]","fmul qword ptr [{pi}]","fdiv qword ptr [{divisor}]","fstp dword ptr [{radians}]",
            "fxrstor64 [{env}]",env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,
            degrees=in(reg) &degrees,factor=in(reg) &half_factor,half=in(reg) &mut half,
            pi=in(reg) &pi,divisor=in(reg) &divisor,radians=in(reg) &mut radians,options(nostack,preserves_flags)
        );}
        ensure!(radians.is_finite(),"Native spot radians overflow");
        let (_,cosine)=sincos(radians,precision)?;Ok((half,cosine))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC spot angle needs a verified arithmetic backend on this architecture")}
}

/// PC SH projection9a7d50 / task-kernel b67520. Coordinates are supplied in
/// the native PC kernel's space; the Wii camera rotation/sign flips do not
/// belong in this producer. Preserve its intermediate float stores.
pub(super) fn sh_project(vector:[f32;3],precision:Precision)->Result<[f32;9]> {
    ensure!(vector.iter().all(|v|v.is_finite()),"Nonfinite PC SH direction");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();
        let constant=0.282094806432724f32;
        let k=[0.3257350027561188f64,0.236543670296669,0.07884789258241653,-0.2731371223926544,0.1365685611963272];
        let mut temporary=0f32;let mut out=[0f32;9];
        unsafe {std::arch::asm!(
            "fxsave64 [{env}]","fninit","fldcw [{cw}]",
            "fld dword ptr [{constant}]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+8]","fld st(0)","fld qword ptr [{k}]","fmul st(1), st(0)","fxch st(1)","fstp dword ptr [{out}+8]",
            "fld st(1)","fmul st(0), st(0)","fstp dword ptr [{t}]","fld dword ptr [{t}]","fmul qword ptr [{k}+8]","fsub qword ptr [{k}+16]","fstp dword ptr [{out}+24]",
            "fld dword ptr [{v}]","fld st(0)","fmul st(0), st(2)","fstp dword ptr [{out}+12]",
            "fld dword ptr [{v}+4]","fld st(0)","fmulp st(3), st(0)","fxch st(2)","fstp dword ptr [{out}+4]",
            "fxch st(2)","fmul qword ptr [{k}+24]","fstp dword ptr [{t}]","fld dword ptr [{t}]","fld st(0)","fmul st(0), st(3)","fstp dword ptr [{out}+28]","fmul st(0), st(1)","fstp dword ptr [{out}+20]",
            "fld st(1)","fmul st(0), st(2)","fld st(1)","fmul st(0), st(2)","fsubp st(1), st(0)","fstp dword ptr [{t}]",
            "fld dword ptr [{t}]","fld qword ptr [{k}+32]","fmul st(1), st(0)","fxch st(1)","fstp dword ptr [{out}+32]",
            "fxch st(2)","fmulp st(1), st(0)","fadd st(0), st(0)","fstp dword ptr [{t}]","fmul dword ptr [{t}]","fstp dword ptr [{out}+16]",
            "fxrstor64 [{env}]",env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,
            constant=in(reg) &constant,v=in(reg) vector.as_ptr(),k=in(reg) k.as_ptr(),t=in(reg) &mut temporary,out=in(reg) out.as_mut_ptr(),options(nostack,preserves_flags)
        );}
        ensure!(out.iter().all(|v|v.is_finite()),"PC SH projection overflow");Ok(out)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH projection needs a verified arithmetic backend on this architecture")}
}
/// PC b675c0: ambient and diffuse scales are stored to float before use;
/// each coefficient product/add retains the selected x87 precision until its
/// output store. No FMA, intermediate SIMD rounding or color clamping.
pub(super) fn sh_accumulate(out:&mut [[f32;3];9],ambient:[f32;3],diffuse:Option<([f32;3],[f32;9])>,weight:f32,precision:Precision)->Result<()> {
    ensure!(weight.is_finite()&&ambient.iter().chain(out.iter().flatten()).all(|v|v.is_finite()),"Nonfinite PC SH contribution");
    if let Some((color,basis))=&diffuse {ensure!(color.iter().chain(basis).all(|v|v.is_finite()),"Nonfinite PC SH diffuse contribution");}
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();
        let constants=[3.544907808303833f64,2.9567930698394775];let mut scale=[0f32;2];let mut color=[0f32;3];
        // Only integer indexing and bounded array access occur between saving
        // and restoring the environment. Every arithmetic operation is x87.
        unsafe {
            std::arch::asm!("fxsave64 [{env}]","fninit","fldcw [{cw}]",env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,options(nostack,preserves_flags));
            std::arch::asm!("fld dword ptr [{weight}]","fmul qword ptr [{k}]","fstp dword ptr [{scale}]",weight=in(reg) &weight,k=in(reg) constants.as_ptr(),scale=in(reg) scale.as_mut_ptr(),options(nostack,preserves_flags));
            for channel in 0..3 {
                std::arch::asm!("fld dword ptr [{a}]","fmul dword ptr [{scale}]","fadd dword ptr [{out}]","fstp dword ptr [{out}]",a=in(reg) &ambient[channel],scale=in(reg) scale.as_ptr(),out=in(reg) &mut out[0][channel],options(nostack,preserves_flags));
            }
            if let Some((source,basis))=&diffuse {
                std::arch::asm!("fld dword ptr [{weight}]","fmul qword ptr [{k}+8]","fstp dword ptr [{scale}+4]",weight=in(reg) &weight,k=in(reg) constants.as_ptr(),scale=in(reg) scale.as_mut_ptr(),options(nostack,preserves_flags));
                for channel in 0..3 {
                    std::arch::asm!("fld dword ptr [{source}]","fmul dword ptr [{scale}+4]","fstp dword ptr [{color}]",source=in(reg) &source[channel],scale=in(reg) scale.as_ptr(),color=in(reg) &mut color[channel],options(nostack,preserves_flags));
                }
                for coefficient in 0..9 {for channel in 0..3 {
                    std::arch::asm!("fld dword ptr [{basis}]","fmul dword ptr [{color}]","fadd dword ptr [{out}]","fstp dword ptr [{out}]",basis=in(reg) &basis[coefficient],color=in(reg) &color[channel],out=in(reg) &mut out[coefficient][channel],options(nostack,preserves_flags));
                }}
            }
            std::arch::asm!("fxrstor64 [{env}]",env=in(reg) environment.as_ptr(),
                out("xmm0") _,out("xmm1") _,out("xmm2") _,out("xmm3") _,out("xmm4") _,out("xmm5") _,out("xmm6") _,out("xmm7") _,
                out("xmm8") _,out("xmm9") _,out("xmm10") _,out("xmm11") _,out("xmm12") _,out("xmm13") _,out("xmm14") _,out("xmm15") _,options(nostack,preserves_flags));
        }
        ensure!(out.iter().flatten().all(|v|v.is_finite()),"PC SH accumulation overflow");Ok(())
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH accumulation needs a verified arithmetic backend on this architecture")}
}

// Small PC SH sampling expressions, each with the original float stores.
// Macro inputs/outputs stay private to these typed numeric adapters.
#[cfg(target_arch="x86_64")]
macro_rules! sh_expression {
    ($precision:expr,$values:expr,$output:expr,$($instruction:literal),+ $(,)?)=>{{
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=$precision.control();
        let values=$values;let output=$output;
        unsafe {std::arch::asm!("fxsave64 [{env}]","fninit","fldcw [{cw}]",$($instruction,)+"fxrstor64 [{env}]",
            env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,v=in(reg) values.as_ptr(),out=in(reg) output.as_mut_ptr(),options(nostack,preserves_flags));}
    }}
}
/// PC d2e643..d2e8a6, after the caller's D3DX view/projection transform.
/// Returns scale, U translation and V translation. Native has separate float
/// stores for normalized coordinates and for the intermediate U offset.
pub(super) fn texgen_projection(bound:[f32;3],eye:[f32;3],direction:[f32;3],projected:[f32;4],precision:Precision)->Result<[f32;3]> {
    ensure!(bound.iter().chain(&eye).chain(&direction).chain(&projected).all(|v|v.is_finite())&&projected[3]!=0.,"Unsupported nonfinite/zero-W texgen projection source");
    #[cfg(target_arch="x86_64")]
    {
        let values=[bound[0],bound[1],bound[2],eye[0],eye[1],eye[2],direction[0],direction[1],direction[2]];let mut delta=[0f32;4];
        sh_expression!(precision,&values,&mut delta,
            "fld dword ptr [{v}]","fsub dword ptr [{v}+12]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+4]","fsub dword ptr [{v}+16]","fstp dword ptr [{out}+4]",
            "fld dword ptr [{v}+8]","fsub dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
            "fld dword ptr [{v}+28]","fmul dword ptr [{out}+4]","fld dword ptr [{v}+24]","fmul dword ptr [{out}]","faddp st(1), st(0)",
            "fld dword ptr [{v}+32]","fmul dword ptr [{out}+8]","faddp st(1), st(0)","fstp dword ptr [{out}+12]"
        );
        ensure!(delta.iter().all(|v|v.is_finite()),"Native texgen distance overflow");
        let distance=delta[3].max(f32::from_bits(0x358637bd));
        let values=[distance,f32::from_bits(0x3d4ccccd),0.,f32::from_bits(0x3fe00000),projected[0],projected[1],projected[3]];let mut out=[0f32;6];
        sh_expression!(precision,&values,&mut out,
            "fld dword ptr [{v}]","fmul dword ptr [{v}+4]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+20]","fdiv dword ptr [{v}+24]","fstp dword ptr [{out}+16]",
            "fld dword ptr [{out}+16]","fld dword ptr [{out}]","fchs","fmulp st(1), st(0)","fadd qword ptr [{v}+8]","fstp dword ptr [{out}+8]",
            "fld dword ptr [{v}+16]","fdiv dword ptr [{v}+24]","fstp dword ptr [{out}+12]",
            "fld dword ptr [{out}+12]","fmul dword ptr [{out}]","fadd qword ptr [{v}+8]","fstp dword ptr [{out}+20]",
            "fld1","fsub dword ptr [{out}+20]","fstp dword ptr [{out}+4]"
        );
        ensure!(out[..3].iter().all(|v|v.is_finite()),"Native texgen projection overflow");Ok([out[0],out[1],out[2]])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC texgen projection needs verified architecture support")}
}
pub(super) fn sh_distance(light:[f32;3],probe:[f32;3],precision:Precision)->Result<([f32;3],f32)> {
    ensure!(light.iter().chain(&probe).all(|v|v.is_finite()),"Nonfinite PC SH position");
    #[cfg(target_arch="x86_64")]
    {
        let values=[light[0],light[1],light[2],probe[0],probe[1],probe[2]];let mut out=[0f32;5];
        sh_expression!(precision,&values,&mut out,
            "fld dword ptr [{v}]","fsub dword ptr [{v}+12]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+4]","fsub dword ptr [{v}+16]","fstp dword ptr [{out}+4]",
            "fld dword ptr [{v}+8]","fsub dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
            "fld dword ptr [{out}]","fmul dword ptr [{out}]","fld dword ptr [{out}+4]","fmul dword ptr [{out}+4]","faddp st(1), st(0)",
            "fld dword ptr [{out}+8]","fmul dword ptr [{out}+8]","faddp st(1), st(0)","fstp dword ptr [{out}+12]","fld dword ptr [{out}+12]","fsqrt","fstp dword ptr [{out}+16]"
        );
        ensure!(out.iter().all(|v|v.is_finite()),"PC SH distance overflow");Ok(([out[0],out[1],out[2]],out[4]))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH distance needs verified architecture support")}
}
pub(super) fn sh_normalize(delta:[f32;3],distance:f32,precision:Precision)->Result<[f32;3]> {
    #[cfg(target_arch="x86_64")]
    {
        let values=[delta[0],delta[1],delta[2],distance];let mut out=[0f32;4];
        sh_expression!(precision,&values,&mut out,
            "fld1","fdiv dword ptr [{v}+12]","fstp dword ptr [{out}+12]",
            "fld dword ptr [{out}+12]","fmul dword ptr [{v}]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+4]","fmul dword ptr [{out}+12]","fstp dword ptr [{out}+4]",
            "fld dword ptr [{out}+12]","fmul dword ptr [{v}+8]","fstp dword ptr [{out}+8]"
        );
        // A directional probe exactly at the light origin writes native NaNs;
        // that direction is not consumed by directional accumulation.
        Ok([out[0],out[1],out[2]])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(delta,distance,precision);anyhow::bail!("PC SH normalization needs verified architecture support")}
}
pub(super) fn sh_falloff(mode:u16,parameters:[f32;3],distance:f32,precision:Precision)->Result<Option<f32>> {
    ensure!(parameters.iter().all(|v|v.is_finite()),"Nonfinite PC SH falloff");
    if mode==0x10 {return Ok(Some(parameters[0]));}
    #[cfg(target_arch="x86_64")]
    {
        let values=[parameters[0],parameters[1],parameters[2],distance];let mut out=[0f32;1];
        match mode {
            0x20=>{
                sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}+8]","fmul dword ptr [{v}+12]","fmul dword ptr [{v}+12]","fld dword ptr [{v}+4]","fmul dword ptr [{v}+12]","faddp st(1), st(0)","fadd dword ptr [{v}]","fstp dword ptr [{out}]");
                ensure!(out[0].is_finite(),"PC SH polynomial overflow");
                if out[0]<=f32::EPSILON{return Ok(None);}
                let denominator=out;
                sh_expression!(precision,&denominator,&mut out,"fld1","fdiv dword ptr [{v}]","fstp dword ptr [{out}]");
            },
            0x40=>sh_expression!(precision,&values,&mut out,"fld1","fld dword ptr [{v}+12]","fsub dword ptr [{v}]","fld dword ptr [{v}+4]","fsub dword ptr [{v}]","fdivp st(1), st(0)","fsubp st(1), st(0)","fstp dword ptr [{out}]"),
            _=>anyhow::bail!("Unverified PC SH falloff mode {mode:#x}"),
        }
        ensure!(out[0].is_finite(),"Invalid PC SH falloff interval");
        Ok(Some(if out[0]<=0. {0.}else if out[0]>=1. {1.}else{out[0]}))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(distance,precision);anyhow::bail!("PC SH falloff needs verified architecture support")}
}
pub(super) fn sh_spot_dot(direction:[f32;3],probe:[f32;3],precision:Precision)->Result<f32> {
    #[cfg(target_arch="x86_64")]
    {
        let values=[direction[0],direction[1],direction[2],probe[0],probe[1],probe[2]];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}+16]","fchs","fmul dword ptr [{v}+4]","fld dword ptr [{v}+12]","fchs","fmul dword ptr [{v}]","faddp st(1), st(0)","fld dword ptr [{v}+20]","fchs","fmul dword ptr [{v}+8]","faddp st(1), st(0)","fstp dword ptr [{out}]");
        ensure!(out[0].is_finite(),"Nonfinite PC SH cone dot product");Ok(out[0])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(direction,probe,precision);anyhow::bail!("PC SH cone needs verified architecture support")}
}
pub(super) fn sh_spot_ratio(dot:f32,inner:f32,outer:f32,precision:Precision)->Result<f32> {
    #[cfg(target_arch="x86_64")]
    {
        let values=[dot,inner,outer];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fsub dword ptr [{v}+8]","fld dword ptr [{v}+4]","fsub dword ptr [{v}+8]","fdivp st(1), st(0)","fstp dword ptr [{out}]");
        ensure!(out[0].is_finite(),"Invalid PC SH cone interval");Ok(out[0])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(dot,inner,outer,precision);anyhow::bail!("PC SH cone ratio needs verified architecture support")}
}
/// Native x87 CRT positive cone path: FYL2X then 8eec10 exponentiation.
/// Ratio and exponent have already reached the original float stores. The
/// caller's selected precision is retained; all host FP state is restored.
pub(super) fn sh_spot_power(ratio:f32,exponent:f32,precision:Precision)->Result<f32> {
    ensure!(ratio.is_finite()&&(0. ..=1.).contains(&ratio)&&exponent.is_finite()&&exponent>=0.,"Unverified PC SH power domain");
    if exponent==0.||ratio==1. {return Ok(1.);}
    if ratio==0. {return Ok(0.);}
    #[cfg(target_arch="x86_64")]
    {
        let values=[ratio,exponent];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,
            "fld dword ptr [{v}+4]","fld dword ptr [{v}]","fyl2x",
            "fld st(0)","frndint","fsubr st(1), st(0)","fxch st(1)","fchs",
            "f2xm1","fld1","faddp st(1), st(0)","fscale","fstp st(1)","fstp dword ptr [{out}]"
        );
        ensure!(out[0].is_finite(),"PC SH power overflow");Ok(out[0])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH power needs verified architecture support")}
}
pub(super) fn sh_sample_weight(dimmer:f32,falloff:f32,cone:f32,precision:Precision)->Result<f32> {
    #[cfg(target_arch="x86_64")]
    {
        let values=[dimmer,falloff,cone];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fmul dword ptr [{v}+4]","fmul dword ptr [{v}+8]","fstp dword ptr [{out}]");
        ensure!(out[0].is_finite(),"PC SH weight overflow");
        Ok(if out[0]>=f32::from_bits(0x3b808081){out[0]}else{if out[0].is_sign_negative(){-0.0}else{0.0}})
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(dimmer,falloff,cone,precision);anyhow::bail!("PC SH weight needs verified architecture support")}
}

/// The SH task applies known ray visibility after sampling, with one float
/// multiplication/store and no second contribution cutoff.
pub(super) fn sh_visibility(weight:f32,visibility:f32,precision:Precision)->Result<f32> {
    ensure!(weight.is_finite()&&visibility.is_finite(),"Nonfinite PC SH visibility");
    #[cfg(target_arch="x86_64")]
    {
        let values=[weight,visibility];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fmul dword ptr [{v}+4]","fstp dword ptr [{out}]");Ok(out[0])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH visibility needs verified architecture support")}
}

/// SH cache differs from the ordinary spot setter: radians are stored before
/// halving. Original CacheLightProperties9a8cbc..9a8d48 preserves both stores.
pub(super) fn sh_cached_spot_cos(degrees:f32,precision:Precision)->Result<f32> {
    ensure!(degrees.is_finite(),"Nonfinite PC cached spot angle");
    #[cfg(target_arch="x86_64")]
    {
        let values=[degrees,3.1415927410125732f32,180.,0.5];let mut out=[0f32;2];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fmul dword ptr [{v}+4]","fdiv dword ptr [{v}+8]","fstp dword ptr [{out}]","fld dword ptr [{out}]","fmul dword ptr [{v}+12]","fstp dword ptr [{out}+4]");
        Ok(sincos(out[1],precision)?.1)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC cached spot angle needs verified architecture support")}
}
pub(super) fn sh_cached_color(color:[f32;3],subtractive:bool,precision:Precision)->Result<[f32;3]> {
    ensure!(color.iter().all(|v|v.is_finite()),"Nonfinite PC cached light color");
    #[cfg(target_arch="x86_64")]
    {
        let values=[color[0],color[1],color[2],255.];let mut out=[0f32;3];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fmul dword ptr [{v}+12]","fstp dword ptr [{out}]","fld dword ptr [{v}+4]","fmul dword ptr [{v}+12]","fstp dword ptr [{out}+4]","fld dword ptr [{v}+8]","fmul dword ptr [{v}+12]","fstp dword ptr [{out}+8]");
        ensure!(out.iter().all(|v|v.is_finite()),"PC cached light color overflow");
        Ok(if subtractive {out.map(|v|-v)}else{out})
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=(subtractive,precision);anyhow::bail!("PC cached light color needs verified architecture support")}
}

/// PC9a7e60: subtract/store each lane, then square/add/store before comparing
/// against the configured squared-displacement threshold. No square root.
pub(super) fn sh_probe_displacement(old:[f32;3],new:[f32;3],precision:Precision)->Result<f32> {
    ensure!(old.iter().chain(&new).all(|v|v.is_finite()),"Nonfinite PC SH probe position");
    #[cfg(target_arch="x86_64")]
    {
        let values=[old[0],old[1],old[2],new[0],new[1],new[2]];let mut out=[0f32;4];
        sh_expression!(precision,&values,&mut out,
            "fld dword ptr [{v}]","fsub dword ptr [{v}+12]","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+4]","fsub dword ptr [{v}+16]","fstp dword ptr [{out}+4]",
            "fld dword ptr [{v}+8]","fsub dword ptr [{v}+20]","fstp dword ptr [{out}+8]",
            "fld dword ptr [{out}]","fmul st(0), st(0)",
            "fld dword ptr [{out}+4]","fmul st(0), st(0)","faddp st(1), st(0)",
            "fld dword ptr [{out}+8]","fmul st(0), st(0)","faddp st(1), st(0)","fstp dword ptr [{out}+12]");
        ensure!(out[3].is_finite(),"PC SH probe displacement overflow");Ok(out[3])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH probe needs verified architecture support")}
}

/// PCb3eae0: multiply by double255 using native precision, then convert to
/// signed32 with truncation and retain the low byte (no saturation).
pub(super) fn sh_cached_weight(weight:f32,precision:Precision)->Result<u8> {
    ensure!(weight.is_finite(),"Nonfinite PC SH cached weight");
    #[cfg(target_arch="x86_64")]
    {
        #[repr(align(16))]struct Environment([u8;512]);
        let mut environment=std::mem::MaybeUninit::<Environment>::uninit();let control=precision.control();let truncate=control|0xc00;let scale=255f64;let mut result=0i32;
        unsafe {std::arch::asm!(
            "fxsave64 [{environment}]","fninit","fldcw [{control}]",
            "fld dword ptr [{weight}]","fmul qword ptr [{scale}]","fldcw [{truncate}]","fistp dword ptr [{result}]",
            "fxrstor64 [{environment}]",environment=in(reg) environment.as_mut_ptr(),control=in(reg) &control,
            truncate=in(reg) &truncate,weight=in(reg) &weight,scale=in(reg) &scale,result=in(reg) &mut result,options(nostack,preserves_flags)
        );}
        Ok(result as u8)
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC SH cached weight needs verified architecture support")}
}

/// Native profile config formatting receives a double, optionally after the
/// original RGB*255 operation at the caller's x87 precision.
pub(super) fn profile_config_source(value:f32,rgb:bool,precision:Precision)->Result<f64> {
    ensure!(value.is_finite(),"Nonfinite profile config source");
    if !rgb{return Ok(value as f64);}
    #[cfg(target_arch="x86_64")]
    {
        let values=[value,0.,f32::from_bits(0x406fe000)];let mut out=[0f64;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fmul qword ptr [{v}+4]","fstp qword ptr [{out}]");
        ensure!(out[0].is_finite(),"Profile RGB config publication overflow");Ok(out[0])
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC profile config publication needs verified architecture support")}
}
/// PC render-profile scalar step: factor is stored as float, while the
/// scalar subtraction retains native precision until multiplication/store.
pub(super) fn profile_scalar_step(current:f32,desired:f32,seconds:f32,precision:Precision)->Result<(f32,f32)> {
    ensure!(current.is_finite()&&desired.is_finite()&&seconds.is_finite()&&seconds>0.,"Invalid native scalar profile transition");
    #[cfg(target_arch="x86_64")]
    {
        // The two middle words are the original double constant30.0.
        let values=[seconds,0.,f32::from_bits(0x403e0000),desired,current];let mut out=[0f32;2];
        sh_expression!(precision,&values,&mut out,
            "fld dword ptr [{v}]","fmul qword ptr [{v}+4]","fld1","fdivrp st(1), st(0)","fstp dword ptr [{out}]",
            "fld dword ptr [{v}+12]","fsub dword ptr [{v}+16]","fmul dword ptr [{out}]","fstp dword ptr [{out}+4]"
        );
        ensure!(out.iter().all(|v|v.is_finite()),"Native scalar profile step overflow");Ok((out[0],out[1]))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC profile interpolation needs verified architecture support")}
}
pub(super) fn profile_scalar_advance(current:f32,desired:f32,step:f32,precision:Precision)->Result<(f32,bool)> {
    ensure!(current.is_finite()&&desired.is_finite()&&step.is_finite(),"Nonfinite native scalar profile transition");
    #[cfg(target_arch="x86_64")]
    {
        let values=[current,step];let mut out=[0f32;1];
        sh_expression!(precision,&values,&mut out,"fld dword ptr [{v}]","fadd dword ptr [{v}+4]","fstp dword ptr [{out}]");
        let continuing=(step>0.&&out[0]<desired)||(step<0.&&out[0]>desired);
        Ok((if continuing {out[0]}else{desired},continuing))
    }
    #[cfg(not(target_arch="x86_64"))]
    {let _=precision;anyhow::bail!("PC profile interpolation needs verified architecture support")}
}
