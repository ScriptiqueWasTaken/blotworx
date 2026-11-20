//! Motion history: REL 0x308254..0x308628. Constructor 0x304340
//! supplies capacity two. Input vectors are the ground state's +0x40/+0x50;
//! mapping those state vectors into the Box3D controller remains separate.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub direction: [f32; 3],
    pub speed: f32,
    pub secondary_direction: [f32; 3],
    pub secondary_speed: f32,
    pub turn: f32,
}
fn horizontal(vector: [f32; 3]) -> Result<([f32; 3], f32)> {
    ensure!(
        vector.iter().all(|v| v.is_finite()),
        "Non-finite ground history input"
    );
    let mut direction = [vector[0], 0., vector[2]];
    let square = vector[2].mul_add(vector[2], vector[0] * vector[0]);
    ensure!(square.is_finite(), "Ground history magnitude overflow");
    if square <= 0.001 {
        return Ok((direction, 0.));
    }
    let estimate = super::pose_blend::estimate(square);
    let correction = (-square).mul_add(estimate * estimate, 1.);
    let reciprocal = correction.mul_add(0.5 * estimate, estimate);
    for lane in &mut direction {
        *lane *= reciprocal;
    }
    Ok((direction, square * reciprocal))
}

#[derive(Default)]
pub struct History {
    samples: [Sample; 2],
    head: usize,
    count: usize,
    pub average_speed: f32,
    pub average_secondary_speed: f32,
}
impl History {
    pub fn clear(&mut self) {
        self.head = 0;
        self.count = 0;
        self.average_speed = 0.;
        self.average_secondary_speed = 0.;
    }
    pub fn latest(&self) -> Option<&Sample> {
        (self.count > 0).then(|| &self.samples[(self.head + self.count - 1) % 2])
    }
    pub fn previous(&self) -> Option<&Sample> {
        (self.count > 1).then(|| &self.samples[self.head])
    }
    pub fn push(&mut self, primary: [f32; 3], secondary: [f32; 3], frame_time: f32) -> Result<()> {
        ensure!(frame_time.is_finite(), "Non-finite history frame time");
        // Retail permits initial insertion even when time is zero.
        if self.count > 0 && frame_time <= 0. {
            return Ok(());
        }
        let (direction, speed) = horizontal(primary)?;
        let (secondary_direction, secondary_speed) = horizontal(secondary)?;
        let sample = Sample {
            direction,
            speed,
            secondary_direction,
            secondary_speed,
            turn: 0.,
        };
        let count = self.count as f32;
        if self.count < 2 {
            self.average_speed = self.average_speed.mul_add(count, speed) / (count + 1.);
            self.average_secondary_speed =
                self.average_secondary_speed.mul_add(count, secondary_speed) / (count + 1.);
            self.samples[(self.head + self.count) % 2] = sample;
            self.count += 1;
        } else {
            let old = self.samples[self.head];
            self.average_speed = (self.average_speed.mul_add(count, speed) - old.speed) / count;
            self.average_secondary_speed =
                (self.average_secondary_speed.mul_add(count, secondary_speed)
                    - old.secondary_speed)
                    / count;
            self.samples[self.head] = sample;
            self.head = (self.head + 1) % 2;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_two_entry_capacity_zero_time_gate_and_reset() -> Result<()> {
        let mut h = History::default();
        h.push([0., 50., 0.], [0.; 3], 0.)?;
        assert_eq!(h.latest().unwrap().speed, 0.);
        h.push([3., 0., 4.], [0.; 3], 0.)?;
        assert!(h.previous().is_none());
        h.push([3., 0., 4.], [0.; 3], 1. / 30.)?;
        assert_eq!(h.previous().unwrap().speed, 0.);
        let first = h.latest().unwrap().speed;
        assert!((first - 5.).abs() < 1e-6);
        h.push([0., 0., 10.], [0.; 3], 1. / 30.)?;
        assert_eq!(h.previous().unwrap().speed, first);
        assert!((h.average_speed - 7.5).abs() < 1e-6);
        h.clear();
        assert!(h.latest().is_none());
        assert!(h.previous().is_none());
        assert_eq!(h.average_speed, 0.);
        Ok(())
    }
    #[test]
    fn tiny_horizontal_vectors_keep_direction_but_report_zero_speed() -> Result<()> {
        let (direction, speed) = horizontal([0.01, 500., 0.01])?;
        assert_eq!(direction, [0.01, 0., 0.01]);
        assert_eq!(speed, 0.);
        let (direction, speed) = horizontal([0.04, -500., 0.])?;
        assert!((direction[0] - 1.).abs() < 1e-6);
        assert!((speed - 0.04).abs() < 1e-7);
        Ok(())
    }
}
