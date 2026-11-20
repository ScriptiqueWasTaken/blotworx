//! Native listener records: initialization80379fb0, pose8037a0f0,
//! scale8037a290 and spatialization settings8037a2d0.
//! Coordinates are already in the audio domain; no scene conversion is guessed.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Pose {
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub position: [f32; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub pose: Pose,
    pub channel_gains: [f32; 4],
    pub spatialization: u8,
    pub basis: [[f32; 3]; 3],
    pub scale: f32,
}
const ROTATION: [[f32; 3]; 3] = [
    [0.70710677, 0., 0.70710677],
    [0., 1., 0.],
    [-0.70710677, 0., 0.70710677],
];
impl Default for Listener {
    fn default() -> Self {
        Self {
            pose: Pose {
                forward: [0., 0., 1.],
                up: [0., 1., 0.],
                position: [0.; 3],
            },
            channel_gains: [0.; 4],
            spatialization: 1,
            basis: ROTATION,
            scale: 1.,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Listeners {
    records: [Listener; 8],
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[2] * b[2] + (a[0] * b[0] + a[1] * b[1])
}
fn normalize(mut vector: [f32; 3]) -> Result<[f32; 3]> {
    let squared = dot(vector, vector);
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Listener normalization requires unverified FPSCR behavior"
    );
    if squared == 0. {
        return Ok(vector);
    }
    ensure!(
        squared.is_normal(),
        "Subnormal listener normalization requires original FPSCR validation"
    );
    let half = 0.5 * squared;
    let mut inverse = super::pose_blend::estimate(squared);
    inverse = inverse * (1.5 - half * (inverse * inverse));
    inverse = inverse * (1.5 - half * (inverse * inverse));
    for value in &mut vector {
        *value *= inverse;
    }
    Ok(vector)
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
impl Listeners {
    pub fn records(&self) -> &[Listener; 8] {
        &self.records
    }
    pub fn distance_records(&self) -> [super::audio_distance::Listener; 8] {
        std::array::from_fn(|i| super::audio_distance::Listener {
            position: self.records[i].pose.position,
            scale: self.records[i].scale,
        })
    }
    pub fn set_pose(&mut self, index: u32, pose: Pose) -> Result<u32> {
        let Some(record) = self.records.get_mut(index as usize) else {
            return Ok(31);
        };
        ensure!(
            pose.forward
                .iter()
                .chain(pose.up.iter())
                .chain(pose.position.iter())
                .all(|x| x.is_finite()),
            "Nonfinite listener pose requires original FPSCR validation"
        );
        // The original subtracts the up projection before normalizing either
        // vector. Do not replace this with a conventional look-at constructor.
        let projection = dot(pose.forward, pose.up);
        let forward = normalize(std::array::from_fn(|i| {
            pose.forward[i] - pose.up[i] * projection
        }))?;
        let up = normalize(pose.up)?;
        let right = cross(up, forward);
        let frame = [right, up, forward];
        let mut basis = [[0.; 3]; 3];
        for row in 0..3 {
            for column in 0..3 {
                basis[row][column] = ROTATION[row][2] * frame[2][column]
                    + (ROTATION[row][0] * frame[0][column] + ROTATION[row][1] * frame[1][column]);
            }
        }
        record.pose = Pose {
            forward,
            up,
            position: pose.position,
        };
        record.basis = basis;
        Ok(1)
    }
    /// Native scale helper has no bounds check. Rust diagnoses invalid indices
    /// instead of reproducing an out-of-bounds write to unrelated native state.
    pub fn set_scale(&mut self, index: u32, scale: f32) -> Result<()> {
        let record = self
            .records
            .get_mut(index as usize)
            .ok_or_else(|| anyhow::anyhow!("Audio listener scale index out of range: {index}"))?;
        record.scale = scale;
        Ok(())
    }
    pub fn set_spatialization(&mut self, index: u32, enabled: u32, gains: Option<[f32; 4]>) -> u32 {
        let Some(record) = self.records.get_mut(index as usize) else {
            return 31;
        };
        record.spatialization = enabled as u8;
        if let Some(gains) = gains {
            record.channel_gains = gains;
        }
        1
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn compare(actual: &Listener, expected: &serde_json::Value) {
        let pose = actual
            .pose
            .forward
            .into_iter()
            .chain(actual.pose.up)
            .chain(actual.pose.position)
            .map(f32::to_bits)
            .collect::<Vec<_>>();
        assert_eq!(
            pose,
            serde_json::from_value::<Vec<u32>>(expected["pose_bits"].clone()).unwrap()
        );
        assert_eq!(
            actual.channel_gains.map(f32::to_bits),
            serde_json::from_value::<[u32; 4]>(expected["gains_bits"].clone()).unwrap()
        );
        assert_eq!(
            actual.spatialization as u64,
            expected["enabled"].as_u64().unwrap()
        );
        assert_eq!(
            actual
                .basis
                .into_iter()
                .flatten()
                .map(f32::to_bits)
                .collect::<Vec<_>>(),
            serde_json::from_value::<Vec<u32>>(expected["basis_bits"].clone()).unwrap()
        );
        assert_eq!(
            actual.scale.to_bits() as u64,
            expected["scale_bits"].as_u64().unwrap()
        );
    }
    #[test]
    fn original_listener_initialization_pose_basis_and_settings() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_listener.json"
        ))?;
        for (case_index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut listeners = Listeners::default();
            for record in listeners.records() {
                compare(record, &case["initial"]);
            }
            let index = case["index"].as_u64().unwrap() as u32;
            let pose = serde_json::from_value(case["pose"].clone())?;
            assert_eq!(
                listeners.set_pose(index, pose)? as u64,
                case["pose_status"].as_u64().unwrap(),
                "case {case_index}"
            );
            let scaled = listeners.set_scale(index, case["scale"].as_f64().unwrap() as f32);
            assert_eq!(scaled.is_ok(), index < 8);
            assert_eq!(
                listeners.set_spatialization(
                    index,
                    case["enabled"].as_u64().unwrap() as u32,
                    serde_json::from_value(case["gains"].clone())?
                ) as u64,
                case["settings_status"].as_u64().unwrap()
            );
            if index < 8 {
                compare(&listeners.records()[index as usize], &case["output"]);
            }
            for i in 0..8 {
                if i != index {
                    compare(&listeners.records()[i as usize], &case["initial"]);
                }
            }
        }
        Ok(())
    }
    #[test]
    fn absent_gain_array_preserves_previous_values_and_distance_view() -> Result<()> {
        let mut listeners = Listeners::default();
        assert_eq!(
            listeners.set_spatialization(0, 0, Some([-3., -6., 1., 2.])),
            1
        );
        assert_eq!(listeners.set_spatialization(0, 255, None), 1);
        assert_eq!(listeners.records()[0].channel_gains, [-3., -6., 1., 2.]);
        listeners.set_pose(
            0,
            Pose {
                position: [3., 4., 0.],
                ..listeners.records()[0].pose
            },
        )?;
        listeners.set_scale(0, 2.)?;
        let view = listeners.distance_records();
        assert_eq!(view[0].position, [3., 4., 0.]);
        assert_eq!(view[0].scale, 2.);
        assert!(listeners
            .set_pose(
                0,
                Pose {
                    forward: [f32::NAN; 3],
                    ..listeners.records()[0].pose
                }
            )
            .is_err());
        Ok(())
    }
}
