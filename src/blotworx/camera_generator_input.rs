//! Retail gameplay generator input stages d9f90..da1ac and da1fc..da238.
//! The intervening screen-border steering callback is mandatory at the caller; these
//! stages must not be combined in a way that silently bypasses it.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub previous: [f32; 2], //48c/490: last NONZERO input, not this tick's input
    pub ramp: [f32; 2],     //494/498
    pub impulse: [f32; 2],  //1a4/1a8: consumed and cleared before screen steering
    pub filtered: [f32; 2], //170/174
}
// All eight fields have explicit zero stores in the retail constructor:
// d1968/196c, d1a70/1a74 and d2308..d2314. No allocation-content assumption.
#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub ramp_up: [f32; 2],
    pub ramp_down: [f32; 2],
    pub scale: [f32; 2],
    pub smoothing: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Flags {
    pub suppress_yaw: bool,      // BOTH native dd and de
    pub horizontal_invert: bool, // current profile+2c property's value18
}
fn sign(value: f32) -> f32 {
    if value > 0. {
        1.
    } else if value < 0. {
        -1.
    } else {
        0.
    }
}
impl State {
    /// Original d9f90..da1ac. Input is the base controller's already sampled
    /// rotation (yaw/pitch), not raw pad axes. No extra delta scaling/clamping.
    /// Returns mutable commands for the intervening native screen steering.
    pub fn commands(
        &mut self,
        mut input: [f32; 2],
        delta: f32,
        flags: Flags,
        p: Parameters,
    ) -> Result<[f32; 2]> {
        ensure!(
            input
                .into_iter()
                .chain(self.previous)
                .chain(self.ramp)
                .chain(self.impulse)
                .chain(p.ramp_up)
                .chain(p.ramp_down)
                .chain(p.scale)
                .chain([delta])
                .all(f32::is_finite),
            "Nonfinite native camera input stage is not reconstructed"
        );
        if flags.suppress_yaw {
            input[0] = 0.;
        }
        for axis in 0..2 {
            let continuing = input[axis] != 0. && sign(input[axis]) == sign(self.previous[axis]);
            let target = if continuing { 1. } else { 0. };
            let coefficient = if continuing {
                p.ramp_up[axis]
            } else {
                p.ramp_down[axis]
            };
            self.ramp[axis] = coefficient.mul_add(target - self.ramp[axis], self.ramp[axis]);
            if input[axis] != 0. {
                self.previous[axis] = input[axis];
            }
        }
        let yaw = if flags.horizontal_invert {
            self.previous[0]
        } else {
            -self.previous[0]
        };
        // Native rounds impulse*delta and retained_input*ramp independently
        // before fusing scale*product + rounded impulse. Do not fold delta
        // into the input again or reset retained direction on release.
        let yaw = (yaw * self.ramp[0]).mul_add(p.scale[0], delta * self.impulse[0]);
        let pitch = (self.previous[1] * self.ramp[1]).mul_add(p.scale[1], delta * self.impulse[1]);
        self.impulse = [0.; 2];
        Ok([yaw, pitch])
    }
    /// da1fc..da238, AFTER the original screen steering has adjusted commands.
    /// The returned yaw is added to generator+354. The second filtered lane
    /// remains controller state and is consumed by the pitch solver later.
    pub fn filter(&mut self, commands: [f32; 2], coefficient: f32) -> Result<f32> {
        ensure!(
            commands
                .into_iter()
                .chain(self.filtered)
                .chain([coefficient])
                .all(f32::is_finite),
            "Nonfinite native camera input filter is not reconstructed"
        );
        for axis in 0..2 {
            self.filtered[axis] =
                coefficient.mul_add(commands[axis] - self.filtered[axis], self.filtered[axis]);
        }
        Ok(self.filtered[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_command_and_filter_instruction_cases() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_generator_input.json"
        ))?;
        fn floats<const N: usize>(v: &serde_json::Value) -> [f32; N] {
            std::array::from_fn(|i| f32::from_bits(v[i].as_u64().unwrap() as u32))
        }
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 100);
        for case in cases {
            let mut state = State {
                previous: floats(&case["previous"]),
                ramp: floats(&case["ramp"]),
                impulse: floats(&case["impulse"]),
                filtered: floats(&case["filtered"]),
            };
            let p = Parameters {
                ramp_up: floats(&case["ramp_up"]),
                ramp_down: floats(&case["ramp_down"]),
                scale: floats(&case["scale"]),
                smoothing: f32::from_bits(case["smoothing"].as_u64().unwrap() as u32),
            };
            let commands = state.commands(
                floats(&case["input"]),
                f32::from_bits(case["delta"].as_u64().unwrap() as u32),
                Flags {
                    suppress_yaw: case["suppress_yaw"].as_bool().unwrap(),
                    horizontal_invert: case["invert"].as_bool().unwrap(),
                },
                p,
            )?;
            assert_eq!(
                commands.map(f32::to_bits),
                floats::<2>(&case["commands"]).map(f32::to_bits),
                "commands {case}"
            );
            assert_eq!(
                state.previous.map(f32::to_bits),
                floats::<2>(&case["out_previous"]).map(f32::to_bits),
                "previous {case}"
            );
            assert_eq!(
                state.ramp.map(f32::to_bits),
                floats::<2>(&case["out_ramp"]).map(f32::to_bits),
                "ramp {case}"
            );
            assert_eq!(state.impulse.map(f32::to_bits), [0; 2]);
            // The fixture supplies the result of the intervening callback.
            state.filter(floats(&case["post_callback_commands"]), p.smoothing)?;
            assert_eq!(
                state.filtered.map(f32::to_bits),
                floats::<2>(&case["out_filtered"]).map(f32::to_bits),
                "filter {case}"
            );
        }
        Ok(())
    }
    #[test]
    fn nonfinite_samples_reject_before_mutation() {
        let mut state = State::default();
        let before = state;
        let p = Parameters {
            ramp_up: [0.15; 2],
            ramp_down: [0.75; 2],
            scale: [2.5, 1.],
            smoothing: 0.3,
        };
        assert!(state
            .commands(
                [f32::NAN, 1.],
                1. / 30.,
                Flags {
                    suppress_yaw: false,
                    horizontal_invert: false
                },
                p
            )
            .is_err());
        assert_eq!(state, before);
        assert!(state.filter([1., f32::INFINITY], 0.3).is_err());
        assert_eq!(state, before);
    }
}
