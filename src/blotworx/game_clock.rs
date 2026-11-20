//! Recovered clock arithmetic and pause/step selection. Integration with the
//! frame scheduler and original runtime comparison remain pending.
use std::num::NonZeroU32;

/// Physics-manager accumulator, separate from the global game clock above it.
/// REL 0x240928 initializes +0x38 from 0x4bd734 (0x3d088889); +0x3c and
/// +0x40 start at zero. REL 0x242790..0x2427fc uses repeated f32 subtraction,
/// not a division/floor tick count or an integer-rate accumulator.
#[derive(Default, Debug)]
pub struct PhysicsClock {
    pub remainder: f32,
    pub frame_simulated: f32,
}
impl PhysicsClock {
    pub const STEP: f32 = f32::from_bits(0x3d088889);

    /// Caller invokes this only when the original manager has a world. Each
    /// callback must perform integrate, collide, and advance in that order
    /// (DOL 0x802de0a8, 0x802de124, 0x802de190). Entity writeback follows the
    /// entire loop and uses frame_simulated. No excess time is silently dropped.
    /// A callback failure aborts the frame; it must not be retried after partial
    /// simulation side effects. This is not the presentation/frame scheduler.
    pub fn advance(
        &mut self,
        delta: f32,
        mut simulate: impl FnMut(f32) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            delta.is_finite() && delta >= 0. && self.remainder.is_finite() && self.remainder >= 0.,
            "Invalid physics clock delta or remainder"
        );
        let accumulated = self.remainder + delta;
        anyhow::ensure!(
            accumulated.is_finite(),
            "Physics clock accumulation overflow"
        );
        self.frame_simulated = 0.;
        self.remainder = accumulated;
        while self.remainder >= Self::STEP {
            let remaining = self.remainder - Self::STEP;
            anyhow::ensure!(
                remaining < self.remainder,
                "Physics clock cannot represent step subtraction"
            );
            simulate(Self::STEP)?;
            self.frame_simulated += Self::STEP;
            self.remainder = remaining;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rate {
    pub ticks_per_second: NonZeroU32,
    pub division_threshold: i32,
}
impl Rate {
    /// REL 0x14ba94 reads Wii bus clock at 0x800000f8 and divides by four.
    /// Initialization 0x14b3ac..0x14b3c8 stores frequency * 5 as u32.
    pub fn from_bus_clock(bus_clock: u32) -> anyhow::Result<Self> {
        let frequency = NonZeroU32::new(bus_clock >> 2).ok_or_else(|| {
            anyhow::anyhow!("Game clock bus frequency produces zero ticks per second")
        })?;
        Ok(Self {
            ticks_per_second: frequency,
            division_threshold: frequency.get().wrapping_mul(5) as i32,
        })
    }
    pub fn seconds(&self, ticks: i64) -> f32 {
        super::locomotion_actions::ticks_to_seconds(
            ticks,
            self.ticks_per_second,
            self.division_threshold,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Frozen,
    Realtime,
    Capped(NonZeroU32),
}
impl Step {
    /// REL 0x14b4f4..0x14b524 selects the earlier of the hardware sample
    /// and previous + frequency/rate. Preserve integer division and wrapping.
    pub fn elapsed_ticks(self, frequency: NonZeroU32, previous: u64, now: u64) -> u64 {
        match self {
            Self::Frozen => 0,
            Self::Realtime => now.wrapping_sub(previous),
            Self::Capped(rate) => {
                let limit = previous.wrapping_add((frequency.get() / rate.get()) as u64);
                limit.min(now).wrapping_sub(previous)
            }
        }
    }
}

/// Unscaled timestamp half of REL 0x14b440. Even a frozen update refreshes
/// the hardware baseline, so resuming cannot accumulate the paused interval.
/// Scaled gameplay time is separate and still requires its conversion path.
#[derive(Clone, Copy, Debug)]
pub struct UnscaledClock {
    pub ticks: u64,
    previous_sample: u64,
}
impl UnscaledClock {
    pub fn new(hardware_ticks: u64) -> Self {
        Self {
            ticks: hardware_ticks,
            previous_sample: hardware_ticks,
        }
    }
    pub fn sample(&mut self, step: Step, frequency: NonZeroU32, hardware_ticks: u64) -> u64 {
        let elapsed = step.elapsed_ticks(frequency, self.previous_sample, hardware_ticks);
        self.ticks = self.ticks.wrapping_add(elapsed);
        self.previous_sample = hardware_ticks;
        elapsed
    }
}

/// REL 0x14b528..0x14b53c uses u64 -> double -> float, float multiply,
/// then DOL 0x8000e0a8's saturating/truncating unsigned conversion.
fn scale_ticks(ticks: u64, scale: f32) -> u64 {
    let value = (ticks as f64 as f32) * scale;
    if value.is_sign_negative() {
        0
    } else if value.is_nan() {
        u64::MAX
    } else {
        value as u64
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub rate: Rate,
    pub unscaled: UnscaledClock,
    pub scaled_ticks: u64,
    pub unscaled_seconds: f64,
    pub scaled_seconds: f64,
    pub hardware_seconds: f64,
    pub frames: u64,
    pub reciprocal_delta: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub unscaled: f32,
    pub scaled: f32,
    pub hardware: f32,
}
impl Clock {
    /// REL1eff10: entity byte40 selects double seconds18/10, narrowed once.
    pub fn entity_seconds(&self, scaled: bool) -> f32 {
        (if scaled {
            self.scaled_seconds
        } else {
            self.unscaled_seconds
        }) as f32
    }
    pub fn new(rate: Rate, hardware_ticks: u64) -> Self {
        Self {
            rate,
            unscaled: UnscaledClock::new(hardware_ticks),
            scaled_ticks: hardware_ticks,
            unscaled_seconds: 0.,
            scaled_seconds: 0.,
            hardware_seconds: 0.,
            frames: 0,
            reciprocal_delta: 0.,
        }
    }
    /// REL 0x14b440. Frozen updates still accumulate hardware elapsed time
    /// and frame count, but preserve gameplay timestamps and reciprocal delta.
    pub fn sample(&mut self, step: Step, hardware_ticks: u64, scale: f32) -> Frame {
        let hardware = self
            .rate
            .seconds(hardware_ticks.wrapping_sub(self.unscaled.previous_sample) as i64);
        let delta = self
            .unscaled
            .sample(step, self.rate.ticks_per_second, hardware_ticks);
        let mut frame = Frame {
            unscaled: 0.,
            scaled: 0.,
            hardware,
        };
        if step != Step::Frozen {
            let scaled = scale_ticks(delta, scale);
            frame.unscaled = self.rate.seconds(delta as i64);
            frame.scaled = self.rate.seconds(scaled as i64);
            self.scaled_ticks = self.scaled_ticks.wrapping_add(scaled);
            self.unscaled_seconds += frame.unscaled as f64;
            self.scaled_seconds += frame.scaled as f64;
            self.reciprocal_delta = 1. / frame.unscaled;
        }
        self.hardware_seconds += hardware as f64;
        self.frames = self.frames.wrapping_add(1);
        frame
    }
}

#[derive(Default)]
pub struct Control {
    pub paused: bool,
    pub suspended: bool,
    pub fixed_rate: u32,
    step_requested: bool,
}
impl Control {
    /// REL 0x14ba68 sets both pause and one-shot step bytes.
    pub fn request_step(&mut self) {
        self.paused = true;
        self.step_requested = true;
    }
    /// REL 0x14b464..0x14b4f0. Suspension does not consume a pending step.
    pub fn take_step(&mut self) -> Step {
        if self.suspended || (self.paused && !self.step_requested) {
            return Step::Frozen;
        }
        let rate = if self.paused && self.fixed_rate == 0 {
            30
        } else {
            self.fixed_rate
        };
        self.step_requested = false;
        NonZeroU32::new(rate).map_or(Step::Realtime, Step::Capped)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn physics_accumulator_preserves_original_float_boundary_behavior() {
        let mut clock = super::PhysicsClock::default();
        let mut ticks = 0;
        clock
            .advance(1., |dt| {
                assert_eq!(dt.to_bits(), 0x3d088889);
                ticks += 1;
                Ok(())
            })
            .unwrap();
        // Instruction-level fadds/fsubs evaluation: rounding leaves the 30th
        // step just short. A floor(delta*30) implementation changes this result.
        assert_eq!(ticks, 29);
        assert_eq!(clock.remainder.to_bits(), 0x3d088827);
        assert_eq!(clock.frame_simulated.to_bits(), 0x3f77777e);
        clock
            .advance(0., |_| {
                panic!("Zero delta must retain the sub-step remainder")
            })
            .unwrap();
        assert_eq!(clock.frame_simulated, 0.);
        assert_eq!(clock.remainder.to_bits(), 0x3d088827);
    }
    #[test]
    fn physics_half_frames_accumulate_and_invalid_delta_preserves_state() {
        let mut clock = super::PhysicsClock::default();
        let half = super::PhysicsClock::STEP * 0.5;
        clock
            .advance(half, |_| panic!("Only half a step is ready"))
            .unwrap();
        let mut ticks = 0;
        clock
            .advance(half, |_| {
                ticks += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(ticks, 1);
        assert_eq!(clock.remainder, 0.);
        let previous = clock.frame_simulated;
        assert!(clock
            .advance(f32::NAN, |_| panic!("Invalid delta must not simulate"))
            .is_err());
        assert_eq!(clock.frame_simulated, previous);
        assert_eq!(clock.remainder, 0.);
    }
    use super::*;
    #[test]
    fn scaled_clock_truncates_each_update_and_pause_preserves_gameplay_time() {
        let mut clock = Clock::new(Rate::from_bus_clock(4000).unwrap(), 100);
        let frame = clock.sample(Step::Realtime, 103, 0.5);
        assert_eq!(frame.scaled, 0.001);
        assert_eq!(clock.scaled_ticks, 101);
        clock.sample(Step::Realtime, 106, 0.5);
        assert_eq!(
            clock.scaled_ticks, 102,
            "fractional ticks are discarded each update"
        );
        let reciprocal = clock.reciprocal_delta;
        assert_eq!(
            clock.sample(Step::Frozen, 1106, 2.),
            Frame {
                unscaled: 0.,
                scaled: 0.,
                hardware: 1.
            }
        );
        assert_eq!(clock.scaled_ticks, 102);
        assert_eq!(clock.unscaled.ticks, 106);
        assert_eq!(clock.reciprocal_delta, reciprocal);
        clock.sample(Step::Realtime, 1110, 2.);
        assert_eq!(clock.scaled_ticks, 110);
        assert_eq!(clock.frames, 4);
    }
    #[test]
    fn scale_conversion_preserves_native_unsigned_saturation() {
        assert_eq!(scale_ticks(3, 0.5), 1);
        assert_eq!(scale_ticks(3, -1.), 0);
        assert_eq!(scale_ticks(3, f32::INFINITY), u64::MAX);
        assert_eq!(scale_ticks(3, f32::from_bits(0x7fc00000)), u64::MAX);
        assert_eq!(scale_ticks(3, f32::from_bits(0xffc00000)), 0);
        assert_eq!(scale_ticks(16_777_217, 1.), 16_777_216);
    }
    #[test]
    fn configured_rate_caps_elapsed_time_instead_of_forcing_a_fixed_step() {
        let hz = NonZeroU32::new(1000).unwrap();
        let capped = Step::Capped(NonZeroU32::new(30).unwrap());
        assert_eq!(capped.elapsed_ticks(hz, 100, 110), 10);
        assert_eq!(capped.elapsed_ticks(hz, 100, 200), 33);
        assert_eq!(capped.elapsed_ticks(hz, 100, 100), 0);
        assert_eq!(Step::Realtime.elapsed_ticks(hz, 100, 200), 100);
    }
    #[test]
    fn frozen_updates_refresh_hardware_baseline_without_advancing_unscaled_time() {
        let hz = NonZeroU32::new(1000).unwrap();
        let mut clock = UnscaledClock::new(100);
        assert_eq!(clock.sample(Step::Realtime, hz, 110), 10);
        assert_eq!(clock.sample(Step::Frozen, hz, 5000), 0);
        assert_eq!(clock.ticks, 110);
        assert_eq!(clock.sample(Step::Realtime, hz, 5010), 10);
        assert_eq!(clock.ticks, 120);
    }
    #[test]
    fn pause_step_and_suspension_preserve_native_request_lifetime() {
        let mut control = Control::default();
        assert_eq!(control.take_step(), Step::Realtime);
        control.paused = true;
        assert_eq!(control.take_step(), Step::Frozen);
        control.request_step();
        control.suspended = true;
        assert_eq!(control.take_step(), Step::Frozen);
        control.suspended = false;
        assert_eq!(
            control.take_step(),
            Step::Capped(NonZeroU32::new(30).unwrap())
        );
        assert_eq!(control.take_step(), Step::Frozen);
        control.fixed_rate = 60;
        control.request_step();
        assert_eq!(
            control.take_step(),
            Step::Capped(NonZeroU32::new(60).unwrap())
        );
        assert_eq!(control.take_step(), Step::Frozen);
    }
    #[test]
    fn clock_rate_is_derived_from_supplied_bus_frequency() {
        let rate = Rate::from_bus_clock(4000).unwrap();
        assert_eq!(rate.ticks_per_second.get(), 1000);
        assert_eq!(rate.division_threshold, 5000);
        assert_eq!(rate.seconds(1500), 1.5);
        assert!(Rate::from_bus_clock(3).is_err());
    }
}
