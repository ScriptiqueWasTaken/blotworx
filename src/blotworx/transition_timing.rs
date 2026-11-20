//! Recovered transition clocks and blending arithmetic. These do not perform
//! generator activation, synchronization or event routing. Transform-track
//! composition is shared with the transition lifecycle runtime.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Stage {
    BlendIn = 0,
    Transition = 1,
    BlendOut = 2,
}

#[derive(Debug, PartialEq)]
pub enum BlendPlan {
    FromToTransition(f32),
    TransitionOnly,
    TargetToTransition(f32),
}

fn nonnegative(value: f32) -> bool {
    value.is_finite() && value >= 0.
}

/// Duration is supplied by the transition generator's synchronization info,
/// not inferred from wall-clock time or the destination animation's length.
pub struct GeneratorClock {
    pub time: f32,
    pub duration: f32,
    pub blend_in: f32,
    pub blend_out: f32,
}
impl GeneratorClock {
    pub fn output_at_stage(
        &self,
        stage: Stage,
        from: &super::generator_output::Output,
        to: &super::generator_output::Output,
        transition: &super::generator_output::Output,
    ) -> Result<super::generator_output::Output> {
        match self.blend_plan_at_stage(stage) {
            BlendPlan::FromToTransition(weight) => from.blend(transition, weight),
            BlendPlan::TransitionOnly => Ok(transition.clone()),
            BlendPlan::TargetToTransition(weight) => to.blend(transition, weight),
        }
    }
    /// Compose only the transform tracks; event ownership, float tracks and
    /// motion tracks have separate generator-output rules. Child ordering
    /// follows 0x8021eddc and the weight direction in 0x8021e374.
    pub fn pose(
        &self,
        from: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        to: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        transition: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
    ) -> Result<Vec<Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>>> {
        self.pose_at_stage(self.stage(), from, to, transition)
    }
    /// Runtime callers supply the retained stage used for child enumeration.
    /// Synchronization may change duration before update commits a new stage.
    pub fn pose_at_stage(
        &self,
        stage: Stage,
        from: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        to: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        transition: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
    ) -> Result<Vec<Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>>> {
        match self.blend_plan_at_stage(stage) {
            BlendPlan::FromToTransition(weight) => {
                super::pose_blend::tracks(from, transition, weight)
            }
            BlendPlan::TransitionOnly => Ok(transition.to_vec()),
            BlendPlan::TargetToTransition(weight) => {
                super::pose_blend::tracks(to, transition, weight)
            }
        }
    }
    pub fn new(duration: f32, blend_in: f32, blend_out: f32) -> Result<Self> {
        ensure!(
            [duration, blend_in, blend_out].into_iter().all(nonnegative),
            "Invalid generator transition duration"
        );
        ensure!(
            (blend_in + blend_out).is_finite(),
            "Transition blend sum overflow"
        );
        Ok(Self {
            time: 0.,
            duration,
            blend_in,
            blend_out,
        })
    }
    /// DOL 0x8021ed0c. Preserve single-precision operation order.
    pub fn effective_blends(&self) -> (f32, f32) {
        let sum = self.blend_in + self.blend_out;
        if sum > self.duration {
            let scale = self.duration / sum;
            let blend_in = scale * self.blend_in;
            (blend_in, self.duration - blend_in)
        } else {
            (self.blend_in, self.blend_out)
        }
    }
    pub fn stage(&self) -> Stage {
        if self.duration == 0. {
            return if self.blend_in == 0. {
                Stage::Transition
            } else {
                Stage::BlendIn
            };
        }
        let (blend_in, blend_out) = self.effective_blends();
        if self.time < blend_in {
            Stage::BlendIn
        } else if self.duration - self.time < blend_out {
            Stage::BlendOut
        } else {
            Stage::Transition
        }
    }
    /// Supply the node timestep from graph synchronization (DOL 0x801ef214),
    /// which may differ from the fixed physics timestep even at stock 30 Hz.
    pub fn advance(&mut self, dt: f32) -> Result<bool> {
        ensure!(
            nonnegative(dt) && (self.time + dt).is_finite(),
            "Invalid transition timestep"
        );
        let before = self.stage();
        self.time += dt;
        Ok(before != self.stage())
    }
    /// DOL 0x8021ec40: a strict comparison requests starting a dormant target.
    /// The graph must still execute its target-generator state protocol.
    pub fn requests_target_start(&self) -> bool {
        self.duration - self.time < self.effective_blends().1
    }
    /// DOL 0x8021f144: zero-duration transitions still need positive elapsed time.
    pub fn finished(&self) -> bool {
        self.time > 0. && self.duration - self.time <= 0.
    }
    /// DOL 0x8021e740 and child ordering in 0x8021eddc. A hold stage copies
    /// the transition output; it is not a blend with a sentinel negative weight.
    pub fn blend_plan(&self) -> BlendPlan {
        self.blend_plan_at_stage(self.stage())
    }
    pub fn blend_plan_at_stage(&self, stage: Stage) -> BlendPlan {
        let (blend_in, blend_out) = self.effective_blends();
        match stage {
            Stage::BlendIn => BlendPlan::FromToTransition(if blend_in == 0. {
                1.
            } else {
                (self.time / blend_in).min(1.)
            }),
            Stage::Transition => BlendPlan::TransitionOnly,
            Stage::BlendOut => BlendPlan::TargetToTransition(
                ((self.duration - self.time).max(0.) / blend_out).max(0.),
            ),
        }
    }
}

/// DOL 0x8021dd28 updates the two clock fields separately. Do not replace
/// remaining with duration - elapsed: accumulated float rounding differs.
pub struct BlendingClock {
    pub time: f32,
    pub remaining: f32,
}
impl BlendingClock {
    pub fn output(
        &self,
        from: &super::generator_output::Output,
        to: &super::generator_output::Output,
        same_generator: bool,
        offset: f32,
        curve: u8,
    ) -> Result<super::generator_output::Output> {
        let weight = self.weight(offset, curve)?;
        if same_generator {
            Ok(to.clone())
        } else {
            from.blend(to, weight)
        }
    }
    /// Same-generator effects copy their sole child instead of renormalizing
    /// its rotations. Other effects blend source to target with the authored
    /// curve, preserving absent transform tracks until final reference fill.
    pub fn pose(
        &self,
        from: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        to: &[Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>],
        same_generator: bool,
        offset: f32,
        curve: u8,
    ) -> Result<Vec<Option<epic_mickey_lib_rs::ffi_anim::FfiTransform>>> {
        let weight = self.weight(offset, curve)?;
        if same_generator {
            Ok(to.to_vec())
        } else {
            super::pose_blend::tracks(from, to, weight)
        }
    }
    pub fn new(duration: f32) -> Result<Self> {
        ensure!(
            nonnegative(duration),
            "Invalid blending transition duration"
        );
        Ok(Self {
            time: 0.,
            remaining: duration,
        })
    }
    pub fn advance(&mut self, dt: f32) -> Result<()> {
        ensure!(
            nonnegative(dt) && (self.time + dt).is_finite() && (self.remaining - dt).is_finite(),
            "Invalid transition timestep"
        );
        self.time += dt;
        self.remaining -= dt;
        Ok(())
    }
    /// DOL 0x8021e084 also completes a same-generator transition after a tick.
    pub fn finished(&self, same_generator: bool) -> bool {
        self.time > 0. && (same_generator || self.remaining <= 0.)
    }
    /// DOL 0x8021d45c uses the current sum, including synchronization changes
    /// to remaining, and a caller-supplied sampling offset.
    pub fn weight(&self, offset: f32, curve: u8) -> Result<f32> {
        ensure!(offset.is_finite(), "Invalid transition sampling offset");
        let total = self.time + self.remaining;
        let sample = (self.time + offset).min(total);
        let t = if total == 0. {
            0.
        } else {
            (sample / total).min(1.)
        };
        blend_curve(t, curve)
    }
}

/// DOL 0x8021f5ec, constants at 0x8066faf8. Explicit mul_add preserves the
/// single fused operation used by curves 0 and 3 rather than expanding it.
pub fn blend_curve(t: f32, curve: u8) -> Result<f32> {
    ensure!(
        t.is_finite() && curve <= 3,
        "Unsupported transition blend curve/input"
    );
    let square = t * t;
    let cube = square * t;
    let value = match curve {
        0 => (-2f32).mul_add(cube, 3. * square),
        1 => t,
        2 => t + (-cube + square),
        3 => 2f32.mul_add(square, -cube),
        _ => unreachable!(),
    };
    Ok(value.clamp(0., 1.))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_pose_uses_correct_child_at_each_stage() -> Result<()> {
        use epic_mickey_lib_rs::ffi_anim::FfiTransform;
        let pose = |x| {
            vec![Some(FfiTransform {
                translation: [x, 0., 0.],
                rotation: [0., 0., 0., 1.],
                scale: [1.; 3],
            })]
        };
        let (from, to, transition) = (pose(0.), pose(20.), pose(10.));
        let mut clock = GeneratorClock::new(1., 0.25, 0.25)?;
        assert_eq!(
            clock.pose(&from, &to, &transition)?[0].unwrap().translation[0],
            0.
        );
        clock.advance(0.125)?;
        assert_eq!(
            clock.pose(&from, &to, &transition)?[0].unwrap().translation[0],
            5.
        );
        clock.advance(0.375)?;
        assert_eq!(
            clock.pose(&from, &to, &transition)?[0].unwrap().translation[0],
            10.
        );
        clock.advance(0.375)?;
        assert_eq!(
            clock.pose(&from, &to, &transition)?[0].unwrap().translation[0],
            15.
        );
        clock.advance(0.125)?;
        assert_eq!(
            clock.pose(&from, &to, &transition)?[0].unwrap().translation[0],
            20.
        );
        let mut unnormalized = pose(7.);
        unnormalized[0].as_mut().unwrap().rotation = [0., 0., 0., 0.99];
        let copied = BlendingClock::new(0.1)?.pose(&from, &unnormalized, true, 0., 0)?;
        assert_eq!(copied[0].unwrap().rotation, [0., 0., 0., 0.99]);
        Ok(())
    }
    #[test]
    fn clock_updates_match_register_observed_inputs_without_tolerance() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/transition_clock_inputs.json"
        ))?;
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 51);
        // This capture did not record blend durations. Check time arithmetic,
        // not stage classification, against these direct call inputs.
        let mut clock =
            GeneratorClock::new(samples[0]["duration"].as_f64().unwrap() as f32, 0., 0.)?;
        clock.time = samples[0]["time"].as_f64().unwrap() as f32;
        let mut differs_from_physics_step = false;
        for sample in samples {
            let before = sample["time"].as_f64().unwrap() as f32;
            assert_eq!(clock.time.to_bits(), before.to_bits());
            let bits = u64::from_str_radix(sample["dt_register_bits"].as_str().unwrap(), 16)?;
            let register = f64::from_bits(bits);
            let dt = register as f32;
            assert_eq!(dt as f64, register); // no loss converting the captured f1
            assert_eq!(register, sample["dt"].as_f64().unwrap());
            assert_eq!(
                u32::from_str_radix(sample["fpscr"].as_str().unwrap(), 16)? & 3,
                0
            );
            differs_from_physics_step |= dt != 1. / 30.;
            clock.advance(dt)?;
        }
        assert!(differs_from_physics_step);
        Ok(())
    }
    #[test]
    fn sampled_generator_stages_and_completion_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/root_transitions.json"
        ))?;
        let number = |s: &serde_json::Value, field: &str| s[field].as_f64().unwrap() as f32;
        for observation in fixture["observations"].as_array().unwrap() {
            if observation["effect_class"] != "hkbGeneratorTransitionEffect" {
                continue;
            }
            for sample in observation["samples"].as_array().unwrap() {
                let mut clock = GeneratorClock::new(
                    number(observation, "duration"),
                    number(sample, "blend_in"),
                    number(sample, "blend_out"),
                )?;
                clock.time = number(sample, "time");
                assert_eq!(clock.stage() as u64, sample["stage"].as_u64().unwrap());
                assert!(!clock.finished());
                assert!(!clock.requests_target_start());
            }
        }
        Ok(())
    }
    #[test]
    fn resolve_boundary_is_not_rounded_up() -> Result<()> {
        let mut clock = GeneratorClock::new(0.4, 0., 0.)?;
        // Observed last active resolve sample; do not manufacture its clock
        // from a constant physics timestep (graph synchronization changes dt).
        clock.time = 0.3999999761581421;
        assert!(!clock.finished());
        assert!(!clock.requests_target_start());
        clock.advance(1. / 30.)?;
        assert!(clock.finished());
        assert!(clock.requests_target_start());
        assert!(!GeneratorClock::new(0., 0., 0.)?.finished());
        Ok(())
    }
    #[test]
    fn overlapping_blend_windows_scale_and_use_strict_exit_boundary() -> Result<()> {
        let mut clock = GeneratorClock::new(1., 2., 2.)?;
        assert_eq!(clock.effective_blends(), (0.5, 0.5));
        assert_eq!(clock.blend_plan(), BlendPlan::FromToTransition(0.));
        clock.advance(0.25)?;
        assert_eq!(clock.blend_plan(), BlendPlan::FromToTransition(0.5));
        clock.advance(0.25)?;
        assert_eq!(clock.stage(), Stage::Transition);
        assert!(!clock.requests_target_start());
        clock.advance(0.25)?;
        assert_eq!(clock.blend_plan(), BlendPlan::TargetToTransition(0.5));
        assert!(clock.requests_target_start());
        Ok(())
    }
    #[test]
    fn blend_curves_and_separate_clock_accumulation() -> Result<()> {
        for (curve, expected) in [(0, 0.15625), (1, 0.25), (2, 0.296875), (3, 0.109375)] {
            assert_eq!(blend_curve(0.25, curve)?, expected);
        }
        let mut clock = BlendingClock::new(0.066)?;
        clock.advance(1. / 30.)?;
        assert_eq!(clock.remaining.to_bits(), 0.03266666457056999f32.to_bits());
        assert!(!clock.finished(false));
        assert!(clock.finished(true));
        assert!(clock.weight(0., 0)? > 0.5);
        clock.advance(1. / 30.)?;
        assert!(clock.finished(false));
        assert_eq!(clock.weight(0., 0)?, 1.);
        Ok(())
    }
}
