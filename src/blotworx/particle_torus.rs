//! Local torus emission at DOL804f32c4..804f3398. Three ordered raw-derived
//! unit samples, native double sin/cos narrowed to float, then fused radius.
//! The owning emitter performs world position/velocity transformation separately.
use super::physics::V;
use anyhow::{ensure, Result};

pub(super) fn position(
    radius: f32,
    section_radius: f32,
    mut unit: impl FnMut() -> f32,
) -> Result<V> {
    ensure!(
        radius.is_finite() && section_radius.is_finite(),
        "Nonfinite torus dimensions"
    );
    //804f3298: zero MAJOR radius bypasses all three random draws, even when
    //the section radius is nonzero. A zero SECTION radius does not bypass them.
    if radius == 0. {
        return Ok([0.; 3]);
    }
    let mut random = || -> Result<f32> {
        let value = unit();
        ensure!(
            value.is_finite() && (0. ..=1.).contains(&value),
            "Invalid torus random sample"
        );
        Ok(value)
    };
    let section = section_radius * random()?;
    let phi = f32::from_bits(0x40c90fdb) * random()?;
    let theta = f32::from_bits(0x40c90fdb) * random()?;
    let (sin_phi, cos_phi) = super::native_angle::sin_cos_medium(phi as f64)?;
    let (sin_theta, cos_theta) = super::native_angle::sin_cos_medium(theta as f64)?;
    let radial = section.mul_add(cos_phi as f32, radius);
    let point = [
        radial * (cos_theta as f32),
        radial * (sin_theta as f32),
        section * (sin_phi as f32),
    ];
    ensure!(
        point.into_iter().all(f32::is_finite),
        "Torus emission overflow"
    );
    Ok(point)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_position_and_random_consumption_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/torus_position.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 228);
        for case in cases {
            let samples = case["raw"].as_array().unwrap();
            let mut draws = 0;
            let point = position(
                case["radius"].as_f64().unwrap() as f32,
                case["section_radius"].as_f64().unwrap() as f32,
                || {
                    let raw = samples[draws].as_u64().unwrap();
                    draws += 1;
                    raw as f32 / 32767.
                },
            )?;
            assert_eq!(draws, case["draws"].as_u64().unwrap() as usize);
            assert_eq!(
                point.map(f32::to_bits),
                std::array::from_fn(|i| case["bits"][i].as_u64().unwrap() as u32),
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn invalid_random_inputs_are_errors_and_zero_major_radius_consumes_none() {
        assert!(position(1., 1., || f32::NAN).is_err());
        assert!(position(1., 1., || 1.1).is_err());
        assert!(position(f32::INFINITY, 1., || 0.).is_err());
        assert_eq!(
            position(0., 1., || panic!("Zero radius consumed randomness")).unwrap(),
            [0.; 3]
        );
    }
}
