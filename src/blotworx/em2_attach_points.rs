//! PC9c92a0 attachment-point config and9c8c9e cached transform composition.
//! Euler order follows Gamebryo NiMatrix3::FromEulerAnglesXYZ (X*(Y*Z)).
//! Native trig/x87 rounding parity is not established by this semantic bridge.
use super::native_transform_record::Transform;
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
#[derive(Debug)]
pub(super) struct Point {
    pub bone: String,
    pub transform: Transform,
}
fn matrix(left: [f32; 9], right: [f32; 9]) -> [f32; 9] {
    std::array::from_fn(|i| {
        let r = i / 3;
        let c = i % 3;
        ((left[r * 3] as f64 * right[c] as f64 + left[r * 3 + 1] as f64 * right[3 + c] as f64)
            + left[r * 3 + 2] as f64 * right[6 + c] as f64) as f32
    })
}
pub(super) fn load(
    model: &str,
    name: &str,
    settings: &BTreeMap<String, String>,
) -> Result<Option<Point>> {
    if !name.to_ascii_lowercase().starts_with("attachpoint_") {
        return Ok(None);
    }
    let model = model.replace('\\', "/");
    let model = model.rsplit('/').next().unwrap_or(&model);
    let model = model.rsplit_once('.').map_or(model, |(stem, _)| stem);
    let prefix = format!("AttachPointLocations_{model}_{name}_");
    let get = |field: &str| {
        let key = format!("{prefix}{field}");
        settings
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&key))
            .map(|(_, v)| v.split('\0').next().unwrap_or_default())
    };
    let bone = get("BoneName").unwrap_or("");
    if bone.is_empty() {
        return Ok(None);
    }
    let number = |field: &str| -> Result<f32> {
        super::config_value::decimal_float(get(field).unwrap_or("0.f")).map_err(anyhow::Error::msg)
    };
    let mut rotations = [[0.; 9]; 3];
    let mut position = [0.; 3];
    for (axis, name) in ["X", "Y", "Z"].iter().enumerate() {
        // PC fdiv180.f64, fmul promoted NiMath PI.f32, then fstp single.
        let angle = ((number(&format!("Rotation_{name}"))? as f64 / 180.)
            * (std::f32::consts::PI as f64)) as f32;
        let (sin, cos) = angle.sin_cos();
        rotations[axis] = match axis {
            0 => [1., 0., 0., 0., cos, sin, 0., -sin, cos],
            1 => [cos, 0., -sin, 0., 1., 0., sin, 0., cos],
            _ => [cos, sin, 0., -sin, cos, 0., 0., 0., 1.],
        };
        position[axis] = number(&format!("Translation_{name}"))?;
    }
    let rotation = matrix(rotations[0], matrix(rotations[1], rotations[2]));
    let transform = std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            position[i - 9]
        } else {
            1.
        }
    });
    ensure!(
        transform.iter().all(|v: &f32| v.is_finite()),
        "Nonfinite attachment-point configuration"
    );
    Ok(Some(Point {
        bone: bone.to_ascii_lowercase(),
        transform,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_broom_point_shape_preserves_gamebryo_z_rotation_and_translation() -> Result<()> {
        let prefix = "AttachPointLocations_fantasiabroom_AttachPoint_RightBucket_Locator_";
        let settings = [
            ("BoneName", "skeleton:RightBucket"),
            ("Rotation_Z", "90"),
            ("Translation_X", "-1"),
        ]
        .into_iter()
        .map(|(k, v)| (format!("{prefix}{k}"), v.into()))
        .collect();
        let point = load(
            "Characters/FantasiaBroom/fantasiabroom.nif",
            "attachpoint_rightbucket_locator",
            &settings,
        )?
        .unwrap();
        assert_eq!(point.bone, "skeleton:rightbucket");
        assert_eq!(&point.transform[9..], &[-1., 0., 0., 1.]);
        assert!((point.transform[1] - 1.).abs() < 1e-7);
        assert!((point.transform[3] + 1.).abs() < 1e-7);
        assert!(point.transform[0].abs() < 1e-6);
        Ok(())
    }
    #[test]
    fn empty_configuration_and_ordinary_bones_do_not_invent_attach_points() -> Result<()> {
        assert!(load("b.nif", "Weapon", &BTreeMap::new())?.is_none());
        assert!(load("b.nif", "attachpoint_missing", &BTreeMap::new())?.is_none());
        Ok(())
    }
}
