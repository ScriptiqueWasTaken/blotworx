//! EM1308d24..308e6c and ground transition222e5c..222e7c.
//! This retains the ground motion context, not physical support/contact.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Settings {
    pub enabled: bool,
    pub height: f32,
    pub gravity: f32,
    pub slope_multiplier: f32,
    pub minimum_slope_gravity: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            height: 0.3,
            gravity: -100.,
            slope_multiplier: 0.05,
            minimum_slope_gravity: -0.5,
        }
    }
}
impl Settings {
    pub fn load(values: &BTreeMap<String, String>) -> Result<Self> {
        // Original registration strings:308d4c/58 and305310/18.
        let enabled = values
            .get("CharacterControllerAnimationOnStairsFix")
            .map(|s| s.trim().parse::<i32>())
            .transpose()
            .context("Invalid CharacterControllerAnimationOnStairsFix")?
            .unwrap_or(0)
            != 0;
        let height = values
            .get("PlayerStairHeight")
            .map(|s| s.trim().parse::<f32>())
            .transpose()
            .context("Invalid PlayerStairHeight")?
            .unwrap_or(0.3);
        ensure!(height.is_finite(), "Nonfinite PlayerStairHeight");
        let gravity = values
            .get("CharacterGravityWhenAboveStairs")
            .map(|s| s.trim().parse::<f32>())
            .transpose()
            .context("Invalid CharacterGravityWhenAboveStairs")?
            .unwrap_or(-100.);
        ensure!(
            gravity.is_finite(),
            "Nonfinite CharacterGravityWhenAboveStairs"
        );
        let read = |key: &str, default: f32| -> Result<f32> {
            let value = values
                .get(key)
                .map(|s| s.trim().parse::<f32>())
                .transpose()
                .with_context(|| format!("Invalid {key}"))?
                .unwrap_or(default);
            ensure!(value.is_finite(), "Nonfinite {key}");
            Ok(value)
        };
        Ok(Self {
            enabled,
            height,
            gravity,
            slope_multiplier: read("CharacterControllerGravityOnSlopesMultiplier", 0.05)?,
            minimum_slope_gravity: read("CharacterControllerMinGravityOnSlopes", -0.5)?,
        })
    }
    pub fn retains(&self, motion: i32, support: u32, distance: f32) -> bool {
        //308e4c uses the source constant2. Preserve f32 multiply and <=.
        self.enabled && matches!(motion, 0 | 3) && support != 2 && distance <= 2. * self.height
    }
}
/// Ground0's sliding-support branch2234a8..2235ac. The normal support
/// classifier publishes flag75 when its result is1. This takes precedence
/// over the above-stairs flag, including when both are set. Positive incoming
/// speed accumulates a retained downward force; negative speed uses stairs
/// gravity. Native positive-speed output changes world Y, not the up vector.
pub fn sliding_velocity(
    mut velocity: [f32; 4],
    up: [f32; 4],
    incoming_y: f32,
    maximum_speed: f32,
    dt: f32,
    settings: &Settings,
    force: &mut f32,
) -> [f32; 4] {
    if incoming_y < 0. {
        return above_stairs_velocity(velocity, up, incoming_y, settings.gravity, dt, true);
    }
    if maximum_speed == 0. {
        return velocity;
    }
    // Keep the original separate multiplies/division, including signed speed;
    // algebraically cancelling maximum_speed changes single rounding.
    let scaled = -settings.slope_multiplier * maximum_speed;
    let ratio = incoming_y.abs() / maximum_speed;
    *force += scaled * ratio;
    if settings.minimum_slope_gravity < *force {
        *force = settings.minimum_slope_gravity;
    }
    velocity[1] += *force;
    velocity
}
///223668..2236b4. Base velocity and incoming world Y are distinct inputs.
/// Preserve the fused multiply/add in the scalar and each of the four lanes.
pub fn above_stairs_velocity(
    velocity: [f32; 4],
    up: [f32; 4],
    incoming_y: f32,
    gravity: f32,
    dt: f32,
    enabled: bool,
) -> [f32; 4] {
    if !enabled {
        return velocity;
    }
    let scalar = gravity.mul_add(dt, incoming_y);
    std::array::from_fn(|i| up[i].mul_add(scalar, velocity[i]))
}
/// Ground0's post-solver operation223320..2234a8. This is distinct from
/// the base ground-frame solver: omitting it can double upward motion when
/// the above-stairs force subsequently restores incoming vertical velocity.
pub fn reproject(
    velocity: [f32; 4],
    up: [f32; 4],
    normal: [f32; 4],
    surface: [f32; 4],
    disabled: bool,
) -> [f32; 4] {
    if disabled {
        return velocity;
    }
    let mut relative = std::array::from_fn::<_, 4, _>(|i| velocity[i] - surface[i]);
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let alignment = dot(normal, up);
    if dot(relative, up) > 0.001 && alignment > f32::EPSILON {
        let squared = dot(relative, relative);
        let estimate = super::pose_blend::estimate(squared);
        let inverse = (-squared)
            .mul_add(estimate * estimate, 1.)
            .mul_add(0.5 * estimate, estimate);
        let normalized = relative.map(|v| v * inverse);
        let scale = (squared * inverse) / alignment;
        let cross = |a: [f32; 4], b: [f32; 4]| {
            let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
            [
                a[1].mul_add(b[2], -(b[1] * a[2])),
                y,
                -a[1].mul_add(b[0], -(b[1] * a[0])),
                y,
            ]
        };
        relative = cross(cross(normal, normalized), up).map(|v| v * scale);
    }
    std::array::from_fn(|i| relative[i] + surface[i])
}
#[cfg(any(test, debug_assertions))]
pub fn validate_instructions() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/stairs_flag.json"
    ))?;
    let cases = fixture["cases"]
        .as_array()
        .context("Missing source cases")?;
    ensure!(cases.len() == 480, "Incomplete source stairs corpus");
    for c in cases {
        let settings = Settings {
            enabled: c["enabled"].as_bool().unwrap(),
            height: c["extent"].as_f64().unwrap() as f32,
            ..Default::default()
        };
        assert_eq!(
            settings.retains(
                c["motion"].as_i64().unwrap() as i32,
                c["support"].as_u64().unwrap() as u32,
                c["distance"].as_f64().unwrap() as f32
            ),
            c["flag"].as_u64().unwrap() != 0,
            "{c}"
        );
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/stairs_vertical.json"
    ))?;
    let cases = fixture["cases"]
        .as_array()
        .context("Missing source vertical cases")?;
    ensure!(cases.len() == 108, "Incomplete source vertical corpus");
    for c in cases {
        let vector = |key: &str| std::array::from_fn(|i| c[key][i].as_f64().unwrap() as f32);
        let value = |key: &str| c[key].as_f64().unwrap() as f32;
        let actual = above_stairs_velocity(
            vector("velocity"),
            vector("up"),
            value("vertical"),
            value("gravity"),
            value("dt"),
            c["enabled"].as_bool().unwrap(),
        );
        assert_eq!(
            actual.map(f32::to_bits),
            std::array::from_fn(|i| c["bits"][i].as_u64().unwrap() as u32),
            "{c}"
        );
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/ground_reprojection.json"
    ))?;
    let cases = fixture["cases"]
        .as_array()
        .context("Missing source reprojection cases")?;
    ensure!(cases.len() == 144, "Incomplete source reprojection corpus");
    for c in cases {
        let vector = |key: &str| std::array::from_fn(|i| c[key][i].as_f64().unwrap() as f32);
        let actual = reproject(
            vector("velocity"),
            vector("up"),
            vector("normal"),
            vector("surface"),
            c["disabled"].as_bool().unwrap(),
        );
        assert_eq!(
            actual.map(f32::to_bits),
            std::array::from_fn(|i| c["bits"][i].as_u64().unwrap() as u32),
            "{c}"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sliding_force_matches_original_instructions_and_retained_state() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ground_sliding.json"
        ))?;
        for case in fixture["cases"]
            .as_array()
            .context("Missing sliding cases")?
        {
            let scalar = |key: &str| case[key].as_f64().unwrap() as f32;
            let vector = |key: &str| std::array::from_fn(|i| case[key][i].as_f64().unwrap() as f32);
            let settings = Settings {
                gravity: scalar("gravity"),
                slope_multiplier: scalar("multiplier"),
                minimum_slope_gravity: scalar("minimum"),
                ..Default::default()
            };
            let mut force = scalar("force");
            let actual = sliding_velocity(
                vector("velocity"),
                vector("up"),
                scalar("incoming"),
                scalar("speed"),
                scalar("dt"),
                &settings,
                &mut force,
            );
            assert_eq!(
                serde_json::json!(actual.map(f32::to_bits)),
                case["bits"],
                "{case}"
            );
            assert_eq!(
                force.to_bits() as u64,
                case["force_bits"].as_u64().unwrap(),
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_instruction_corpus() -> Result<()> {
        validate_instructions()
    }
    #[test]
    fn native_defaults_and_invalid_configuration() -> Result<()> {
        let mut config = BTreeMap::new();
        let s = Settings::load(&config)?;
        assert!(!s.enabled);
        assert_eq!(s.height, 0.3);
        config.insert("CharacterControllerAnimationOnStairsFix".into(), "1".into());
        config.insert("PlayerStairHeight".into(), "0.30".into());
        let s = Settings::load(&config)?;
        assert!(s.retains(0, 0, 0.6));
        assert!(!s.retains(0, 0, f32::from_bits(0.6f32.to_bits() + 1)));
        assert!(!s.retains(1, 0, 0.));
        assert!(!s.retains(0, 2, 0.));
        assert!(!s.retains(10, 0, 0.));
        config.insert("PlayerStairHeight".into(), "NaN".into());
        assert!(Settings::load(&config).is_err());
        Ok(())
    }
}
