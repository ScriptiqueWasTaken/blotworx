//! Emitter/listener minimum distance80360010..80360140 and separation80378190.
//! Inputs are the native position domain; scene-coordinate conversion, listener
//! updates, path playback and the mixer belong to their own runtime stages.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Listener {
    pub position: [f32; 3],
    pub scale: f32,
}
pub struct Source<'a> {
    pub positions: &'a [[f32; 3]],
    pub listener_mask: u8,
    pub relative_listener: i8,
}

/// Verified subset of registered-object state (8035d520,8035dc70,8035dd40).
/// Orientation is retained even though the distance query only uses position.
/// Command dispatch and listener ownership belong to the runtime host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub position: [f32; 3],
    pub orientation: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Emitter {
    positions: Vec<Position>,
    listener_mask: u8,
    packed_mode_relative: u8,
    radius_scale: f32,
}
impl Default for Emitter {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            listener_mask: 1,
            packed_mode_relative: 0x1f,
            radius_scale: 1.,
        }
    }
}
impl Emitter {
    pub fn positions(&self) -> &[Position] {
        &self.positions
    }
    pub fn mode(&self) -> u8 {
        self.packed_mode_relative >> 5
    }
    pub fn relative_listener(&self) -> i8 {
        (self.packed_mode_relative as i8).wrapping_shl(3) >> 3
    }
    pub fn listener_mask(&self) -> u8 {
        self.listener_mask
    }
    pub fn radius_scale(&self) -> f32 {
        self.radius_scale
    }
    /// Object+58, constructor8035d520 and registry update8035ed30. Native
    /// storage does not clamp this value to a positive range.
    pub fn set_radius_scale(&mut self, scale: f32) {
        self.radius_scale = scale;
    }
    pub fn set_listener_mask(&mut self, mask: u8) {
        self.listener_mask = mask;
    }
    /// Successful native SetMultiplePositions path. Reject oversized input rather
    /// than silently truncating it; producer constraints remain unverified.
    pub fn set_positions(
        &mut self,
        positions: &[Position],
        mode: u32,
        relative: i32,
    ) -> Result<()> {
        self.set_positions_using(positions, mode, relative, |positions| {
            let mut allocated = Vec::new();
            allocated.try_reserve_exact(positions.len())?;
            allocated.extend_from_slice(positions);
            Ok(allocated)
        })
    }
    fn set_positions_using(
        &mut self,
        positions: &[Position],
        mode: u32,
        relative: i32,
        allocate: impl FnOnce(&[Position]) -> Result<Vec<Position>>,
    ) -> Result<()> {
        ensure!(
            positions.len() <= u16::MAX as usize,
            "Audio source positions exceed native halfword storage"
        );
        let result = if positions.len() == self.positions.len() {
            self.positions.copy_from_slice(positions);
            Ok(())
        } else {
            // Native frees the old buffer before allocating a different count.
            // Failure leaves no positions, but still changes the packed fields.
            self.positions = Vec::new();
            if positions.is_empty() {
                Ok(())
            } else {
                allocate(positions).map(|allocated| self.positions = allocated)
            }
        };
        self.packed_mode_relative = ((mode as u8 & 7) << 5) | (relative as u8 & 31);
        result
    }
    pub fn minimum_distance(&self, listeners: &[Listener; 8]) -> Result<f32> {
        minimum_distance(
            self.positions.iter().map(|p| p.position),
            self.listener_mask,
            self.relative_listener(),
            listeners,
        )
    }
}

pub fn separation(a: [f32; 3], b: [f32; 3]) -> Result<f32> {
    let x = a[0] - b[0];
    let y = a[1] - b[1];
    let z = a[2] - b[2];
    let squared = z * z + (x * x + y * y);
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Audio separation requires unverified special-value/FPSCR behavior"
    );
    if squared == 0. {
        return Ok(0.);
    }
    ensure!(
        squared.is_normal(),
        "Subnormal audio separation requires original FPSCR mode validation"
    );
    let half = 0.5 * squared;
    let mut inverse = super::pose_blend::estimate(squared);
    inverse = inverse * (1.5 - half * (inverse * inverse));
    inverse = inverse * (1.5 - half * (inverse * inverse));
    Ok(squared * inverse)
}
impl Source<'_> {
    pub fn minimum_distance(&self, listeners: &[Listener; 8]) -> Result<f32> {
        ensure!(
            self.positions.len() <= u16::MAX as usize,
            "Audio source positions exceed native halfword storage"
        );
        minimum_distance(
            self.positions.iter().copied(),
            self.listener_mask,
            self.relative_listener,
            listeners,
        )
    }
}
fn minimum_distance(
    positions: impl Iterator<Item = [f32; 3]> + Clone,
    listener_mask: u8,
    relative_listener: i8,
    listeners: &[Listener; 8],
) -> Result<f32> {
    // Native byte+7 stores a signed five-bit listener index. Preserve the
    // field truncation rather than treating its full byte as an index.
    let relative = ((relative_listener as u8 & 31) as i8).wrapping_shl(3) >> 3;
    let mut minimum = 10_000_000_000f32;
    let mut mask = listener_mask;
    let mut index = 0;
    while mask != 0 {
        if mask & 1 != 0 {
            if index as i8 == relative {
                return Ok(0.);
            }
            let listener = listeners[index];
            let mut source_positions = positions.clone().peekable();
            let fallback = if source_positions.peek().is_none() {
                Some([0., 0., 1.])
            } else {
                None
            };
            for position in source_positions.chain(fallback) {
                let distance = separation(listener.position, position)? / listener.scale;
                // Native compare/branch semantics preserve equal-value
                // replacement (including signed zero), unlike f32::min.
                if !(minimum < distance) {
                    minimum = distance;
                }
            }
        }
        mask >>= 1;
        index += 1;
    }
    ensure!(
        !minimum.is_nan(),
        "Audio distance needs unverified final NaN payload/FPSCR behavior"
    );
    Ok(minimum)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_emitter_constructor_and_position_updates() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_emitter.json"
        ))?;
        fn compare(emitter: &Emitter, state: &serde_json::Value) {
            assert_eq!(
                emitter.listener_mask() as u64,
                state["mask"].as_u64().unwrap()
            );
            assert_eq!(emitter.mode() as u64, state["mode"].as_u64().unwrap());
            assert_eq!(
                emitter.relative_listener() as i64,
                state["relative"].as_i64().unwrap()
            );
            let positions: Vec<[f32; 6]> =
                serde_json::from_value(state["positions"].clone()).unwrap();
            assert_eq!(emitter.positions.len(), positions.len());
            for (position, expected) in emitter.positions.iter().zip(positions) {
                for (actual, expected) in position
                    .position
                    .iter()
                    .chain(position.orientation.iter())
                    .zip(expected)
                {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                }
            }
        }
        for case in fixture["cases"].as_array().unwrap() {
            let mut emitter = Emitter::default();
            compare(&emitter, &case["initial"]);
            for step in case["steps"].as_array().unwrap() {
                let positions: Vec<[f32; 6]> = serde_json::from_value(step["positions"].clone())?;
                let positions = positions
                    .into_iter()
                    .map(|p| Position {
                        position: [p[0], p[1], p[2]],
                        orientation: [p[3], p[4], p[5]],
                    })
                    .collect::<Vec<_>>();
                let mut attempted = false;
                let failed = step["fail"] == true;
                let result = emitter.set_positions_using(
                    &positions,
                    step["mode"].as_u64().unwrap() as u32,
                    step["relative"].as_i64().unwrap() as i32,
                    |input| {
                        attempted = true;
                        if failed {
                            anyhow::bail!("Injected native allocation failure");
                        }
                        Ok(input.to_vec())
                    },
                );
                assert_eq!(result.is_err(), attempted && failed);
                let native_allocated = step["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call[0] == "allocate");
                assert_eq!(attempted, native_allocated);
                emitter.set_listener_mask(step["mask"].as_u64().unwrap() as u8);
                compare(&emitter, &step["output"]);
            }
        }
        Ok(())
    }
    #[test]
    fn emitter_defaults_and_packed_updates_match_object_layout() -> Result<()> {
        let mut emitter = Emitter::default();
        assert!(emitter.positions().is_empty());
        assert_eq!(emitter.listener_mask(), 1);
        assert_eq!(emitter.mode(), 0);
        assert_eq!(emitter.relative_listener(), -1);
        let listeners = [Listener {
            position: [0.; 3],
            scale: 1.,
        }; 8];
        assert_eq!(
            emitter.minimum_distance(&listeners)?.to_bits(),
            separation([0.; 3], [0., 0., 1.])?.to_bits()
        );
        let position = Position {
            position: [3., 4., 0.],
            orientation: [0., 1., 0.],
        };
        emitter.set_positions(&[position], 10, 31)?;
        assert_eq!(emitter.mode(), 2);
        assert_eq!(emitter.relative_listener(), -1);
        assert_eq!(emitter.positions(), &[position]);
        assert_eq!(
            emitter.minimum_distance(&listeners)?.to_bits(),
            separation([0.; 3], position.position)?.to_bits()
        );
        emitter.set_positions(&[], 0, 32)?;
        assert_eq!(emitter.relative_listener(), 0);
        assert_eq!(emitter.minimum_distance(&listeners)?, 0.);
        emitter.set_listener_mask(0);
        assert_eq!(emitter.minimum_distance(&listeners)?, 1e10);
        Ok(())
    }
    #[test]
    fn original_separation_and_minimum_distance_float_bits() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_distance.json"
        ))?;
        for (index, case) in fixture["separations"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let a = serde_json::from_value(case["a"].clone())?;
            let b = serde_json::from_value(case["b"].clone())?;
            assert_eq!(
                separation(a, b)?.to_bits(),
                case["output_bits"].as_u64().unwrap() as u32,
                "separation {index}"
            );
        }
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let positions: Vec<[f32; 3]> = serde_json::from_value(case["positions"].clone())?;
            let listeners: [Listener; 8] = serde_json::from_value(case["listeners"].clone())?;
            let source = Source {
                positions: &positions,
                listener_mask: case["mask"].as_u64().unwrap() as u8,
                relative_listener: case["relative"].as_i64().unwrap() as i8,
            };
            let result = source.minimum_distance(&listeners);
            if case["unverified_nan"] == true {
                assert!(
                    result.unwrap_err().to_string().contains("NaN"),
                    "case {index}"
                );
            } else {
                assert_eq!(
                    result?.to_bits(),
                    case["output_bits"].as_u64().unwrap() as u32,
                    "distance {index}: {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn unused_invalid_inputs_are_not_eagerly_read() -> Result<()> {
        let listeners = [Listener {
            position: [f32::NAN; 3],
            scale: f32::NAN,
        }; 8];
        let positions = [[f32::NAN; 3]];
        assert_eq!(
            Source {
                positions: &positions,
                listener_mask: 0,
                relative_listener: -1
            }
            .minimum_distance(&listeners)?,
            1e10
        );
        assert_eq!(
            Source {
                positions: &positions,
                listener_mask: 1,
                relative_listener: 0
            }
            .minimum_distance(&listeners)?,
            0.
        );
        assert!(separation([f32::INFINITY, 0., 0.], [0.; 3]).is_err());
        Ok(())
    }
}
