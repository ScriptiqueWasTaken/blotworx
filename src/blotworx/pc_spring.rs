//! PCd82190/d82230 spring used by the EntityTracker ground plane.
//! No default constructor values: native owning-component initialization
//! must provide all four fields before using this persistent state.
use super::pc_float::Precision;
use anyhow::{ensure, Result};
#[repr(C)]
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct State {
    pub stiffness: f32,
    pub damping: f32,
    pub velocity: f32,
    pub accumulated: f32,
}
impl State {
    pub fn tune(&mut self, stiffness: f32, precision: Precision) -> Result<()> {
        ensure!(
            stiffness.is_finite() && stiffness >= 0.,
            "Unsupported PC spring stiffness"
        );
        if self.stiffness == stiffness {
            return Ok(());
        }
        #[cfg(target_arch = "x86_64")]
        {
            #[repr(align(16))]
            struct Environment([u8; 512]);
            let mut environment = std::mem::MaybeUninit::<Environment>::uninit();
            let control = precision.control();
            let mut root = 0f32;
            unsafe {
                std::arch::asm!(
                 "fxsave64 [{env}]","fninit","fldcw [{cw}]","fld dword ptr [{k}]","fst dword ptr [{state}]","fsqrt","fstp dword ptr [{root}]","fld dword ptr [{root}]","fadd st(0),st(0)","fstp dword ptr [{state}+4]","fxrstor64 [{env}]",
                 env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,k=in(reg) &stiffness,state=in(reg) self,root=in(reg) &mut root,options(nostack,preserves_flags)
                );
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = precision;
            anyhow::bail!("PC spring needs verified architecture support")
        }
    }
    pub fn step(
        &mut self,
        error: f32,
        delta: f32,
        epsilon: f32,
        precision: Precision,
    ) -> Result<f32> {
        ensure!(
            [
                self.stiffness,
                self.damping,
                self.velocity,
                self.accumulated,
                error,
                delta,
                epsilon
            ]
            .iter()
            .all(|v| v.is_finite()),
            "Nonfinite PC spring input"
        );
        ensure!(
            delta >= 0. && self.accumulated >= 0.,
            "Negative PC spring time"
        );
        #[cfg(target_arch = "x86_64")]
        {
            #[repr(align(16))]
            struct Environment([u8; 512]);
            let mut environment = std::mem::MaybeUninit::<Environment>::uninit();
            let control = precision.control();
            let input = [error, delta, epsilon, 0.033333335f32];
            let mut temp = [error, 0., 0., f32::NAN];
            // Constant13de1d4 and native loop order are reference checked. The loop
            // consumes every native iteration; no arbitrary catch-up cap is applied.
            // At2^20 or above, subtracting the1/30 float quantum cannot change
            // a stored-float accumulator. Reject that provably stagnant loop.
            unsafe {
                std::arch::asm!(
                 "fxsave64 [{env}]","fninit","fldcw [{cw}]",
                 "fld dword ptr [{input}+4]","fadd dword ptr [{state}+12]","fstp dword ptr [{state}+12]",
                 "mov eax,dword ptr [{state}+12]","and eax,0x7fffffff","cmp eax,0x49800000","jae 5f",
                 "2:","fld dword ptr [{temp}]","fld st(0)","fabs","fstp dword ptr [{temp}+4]",
                 "fld dword ptr [{temp}+4]","fld dword ptr [{input}+8]","fcompp","fnstsw ax","test ah,0x41","jnz 3f","fldz","jmp 4f",
                 "3:","fld dword ptr [{state}]","fmul st(0),st(1)","fld dword ptr [{state}+4]","fmul dword ptr [{state}+8]","fsubp st(1),st(0)","fadd dword ptr [{state}+8]",
                 "4:","fstp dword ptr [{state}+8]","fsub dword ptr [{state}+8]","fstp dword ptr [{temp}]",
                 "fld dword ptr [{state}+12]","fsub dword ptr [{input}+12]","fstp dword ptr [{temp}+8]",
                 "fldz","fld dword ptr [{temp}+8]","fucomi st(0),st(1)","fcmovb st(0),st(1)","fstp dword ptr [{state}+12]","fstp st(0)",
                 "fld dword ptr [{state}+12]","fld dword ptr [{input}+12]","fcompp","fnstsw ax","test ah,0x41","jnp 2b",
                 "fld dword ptr [{input}]","fsub dword ptr [{temp}]","fstp dword ptr [{temp}+12]","5:","fxrstor64 [{env}]",
                 env=in(reg) environment.as_mut_ptr(),cw=in(reg) &control,state=in(reg) &mut *self,input=in(reg) input.as_ptr(),temp=in(reg) temp.as_mut_ptr(),out("eax") _,options(nostack)
                );
            }
            ensure!(
                [self.velocity, self.accumulated, temp[3]]
                    .iter()
                    .all(|v| v.is_finite()),
                "PC spring overflow or accumulated time cannot advance"
            );
            Ok(temp[3])
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = precision;
            anyhow::bail!("PC spring needs verified architecture support")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn spring_preserves_host_control_and_status() -> Result<()> {
        let mut before = 0u32;
        let mut after = 0u32;
        let mut cw_before = 0u16;
        let mut cw_after = 0u16;
        let mut s = State {
            stiffness: 0.137,
            damping: 0.7137,
            velocity: -0.0317,
            accumulated: 0.0071,
        };
        unsafe {
            std::arch::asm!("stmxcsr [{m}]","fnstcw [{c}]",m=in(reg) &mut before,c=in(reg) &mut cw_before,options(nostack,preserves_flags));
        }
        s.step(0.7137, 0.001, 0.01, Precision::Single)?;
        unsafe {
            std::arch::asm!("stmxcsr [{m}]","fnstcw [{c}]",m=in(reg) &mut after,c=in(reg) &mut cw_after,options(nostack,preserves_flags));
        }
        assert_eq!(before, after);
        assert_eq!(cw_before, cw_after);
        Ok(())
    }
    #[test]
    fn native_zero_delta_still_updates_velocity() -> Result<()> {
        let mut s = State {
            stiffness: 0.25,
            damping: 1.,
            velocity: 0.,
            accumulated: 0.,
        };
        assert_eq!(s.step(1., 0., 0., Precision::Double)?, 0.25);
        assert_eq!(s.accumulated, 0.);
        Ok(())
    }
    #[test]
    fn unrepresentable_time_fails_instead_of_looping() {
        let mut s = State {
            stiffness: 0.25,
            damping: 1.,
            velocity: 0.,
            accumulated: f32::MAX,
        };
        assert!(s.step(1., 0., 0., Precision::Double).is_err());
    }
}
