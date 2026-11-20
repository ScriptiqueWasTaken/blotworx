//! Original NiPS world publication804e5b88 and volume-relative frame
//! 804f3f2c..804f42cc. Simulation submission precedes world publication;
//! callers must not mistake these frame primitives for a full update scheduler.
use super::native_transform_record::{self, Transform};
use anyhow::{ensure, Result};

pub const IDENTITY: Transform = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1.];

/// Native NiTransform multiplication keeps rotation, scale and translation
/// separate. A flattened4x4 product has different single/FMA boundaries.
pub fn compose(parent: &Transform, local: &Transform) -> Transform {
    let rotation = super::native_matrix::multiply(
        &parent[..9].try_into().unwrap(),
        &local[..9].try_into().unwrap(),
    );
    let translation = super::native_matrix::transform(
        &parent[..9].try_into().unwrap(),
        local[9..12].try_into().unwrap(),
    );
    std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            translation[i - 9] * parent[12] + parent[i]
        } else {
            parent[12] * local[12]
        }
    })
}
pub fn relative(system: &Transform, emitter: &Transform) -> Result<Transform> {
    ensure!(
        system.iter().chain(emitter).all(|v| v.is_finite()) && system[12] != 0.,
        "Singular/nonfinite particle relative frame"
    );
    let frame = compose(&native_transform_record::inverse(system), emitter);
    ensure!(
        frame.iter().all(|v| v.is_finite()),
        "Particle relative frame overflow"
    );
    Ok(frame)
}
pub fn point(frame: &Transform, local: [f32; 3]) -> [f32; 3] {
    let rotated = direction(frame, local);
    std::array::from_fn(|i| rotated[i] * frame[12] + frame[9 + i])
}
/// Volume emitter velocity uses rotation, not emitter scale.
pub fn direction(frame: &Transform, local: [f32; 3]) -> [f32; 3] {
    super::native_matrix::transform(&frame[..9].try_into().unwrap(), local)
}
pub struct Publication {
    pub previous_scale: f32,
    pub unmodified: Transform,
    pub world: Transform,
    pub scale_set: bool,
}
pub fn publish(previous: &Transform, computed: Transform, world_space: bool) -> Publication {
    let world = if world_space {
        let mut world = IDENTITY;
        world[12] = computed[12];
        world
    } else {
        computed
    };
    Publication {
        previous_scale: previous[12],
        unmodified: computed,
        world,
        scale_set: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transform(value: &serde_json::Value) -> Transform {
        std::array::from_fn(|i| value[i].as_f64().unwrap() as f32)
    }
    #[test]
    fn original_publication_and_relative_frames_match_every_word() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_space.json"
        ))?;
        for case in fixture["publication"].as_array().unwrap() {
            let mut previous = IDENTITY;
            previous[12] = case["old_scale"].as_f64().unwrap() as f32;
            let result = publish(
                &previous,
                transform(&case["computed"]),
                case["world_space"].as_bool().unwrap(),
            );
            assert_eq!(
                serde_json::json!(result.world.map(f32::to_bits)),
                case["published_bits"]
            );
            assert_eq!(
                serde_json::json!(result.unmodified.map(f32::to_bits)),
                case["unmodified_bits"]
            );
            assert_eq!(
                result.previous_scale.to_bits(),
                case["previous_scale_bits"].as_u64().unwrap() as u32
            );
            assert_eq!(
                u8::from(result.scale_set),
                case["scale_set"].as_u64().unwrap() as u8
            );
        }
        for case in fixture["relative"].as_array().unwrap() {
            assert_eq!(
                serde_json::json!(relative(
                    &transform(&case["system"]),
                    &transform(&case["emitter"])
                )?
                .map(f32::to_bits)),
                case["relative_bits"]
            );
        }
        for case in fixture["applied"].as_array().unwrap() {
            let vector = |value: &serde_json::Value| {
                std::array::from_fn(|i| value[i].as_f64().unwrap() as f32)
            };
            let frame = transform(&case["frame"]);
            assert_eq!(
                serde_json::json!(point(&frame, vector(&case["position"])).map(f32::to_bits)),
                case["position_bits"]
            );
            assert_eq!(
                serde_json::json!(direction(&frame, vector(&case["velocity"])).map(f32::to_bits)),
                case["velocity_bits"]
            );
        }
        Ok(())
    }
    #[test]
    fn emitter_scale_changes_volume_but_not_velocity() {
        let mut frame = IDENTITY;
        frame[12] = -3.;
        frame[9..12].copy_from_slice(&[10., 20., 30.]);
        assert_eq!(point(&frame, [1., 2., 3.]), [7., 14., 21.]);
        assert_eq!(direction(&frame, [1., 2., 3.]), [1., 2., 3.]);
    }
}
