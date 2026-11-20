//! EM1 mono/stereo 2D panning,80380550..803808e4. Output columns
//! are the original four-channel matrix; device downmix is a separate stage.
use anyhow::{ensure, Result};
/// Output-matrix dB conversion80378440, distinct from transition803760f0.
pub fn output_gain(db: f32) -> Result<f32> {
    ensure!(!db.is_nan(), "NaN output gain needs native FPSCR validation");
    if db <= f32::from_bits(0xc2c0999a) {
        return Ok(0.);
    }
    if db >= 0. {
        return Ok(1.);
    }
    static TABLE: std::sync::OnceLock<Result<Vec<f32>, String>> = std::sync::OnceLock::new();
    let table = TABLE
        .get_or_init(|| {
            let bits: Vec<u32> = serde_json::from_str(include_str!(
                "../../tools/blotworx/fixtures/audio_output_gain_table.json"
            ))
            .map_err(|error| error.to_string())?;
            if bits.len() != 98 {
                return Err("Incomplete native output gain table".into());
            }
            Ok(bits.into_iter().map(f32::from_bits).collect())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let index = 97. + db;
    let whole = index.trunc() as usize;
    let fraction = index - whole as f32;
    let upper = table[whole + 1] * fraction;
    let lower = table[whole] * (1. - fraction);
    Ok(lower + upper)
}
fn root(value: f32) -> Result<f32> {
    if value <= 0. {
        return Ok(0.);
    }
    ensure!(
        value.is_normal() && value.is_finite(),
        "Panner square root requires unverified FPSCR behavior"
    );
    let half = 0.5 * value;
    let mut inverse = super::pose_blend::estimate(value);
    inverse = inverse * (1.5 - half * (inverse * inverse));
    inverse = inverse * (1.5 - half * (inverse * inverse));
    Ok(value * inverse)
}
pub fn matrix(enabled: bool, source_mask: u32, x: f32, y: f32) -> Result<Vec<[f32; 4]>> {
    ensure!(x.is_finite() && y.is_finite(), "Nonfinite panner input");
    let channels = source_mask.count_ones();
    ensure!(
        matches!(channels, 1 | 2),
        "Native 2D panner source layout is not verified beyond mono/stereo"
    );
    // Native mono always uses the positional matrix, even with panner off.
    // The enable byte only gates the stereo-source branch.
    if channels == 2 && !enabled {
        return Ok(vec![[1., 0., 0., 0.], [0., 1., 0., 0.]]);
    }
    let a = 1. - x;
    let b = 1. - y;
    let values = [root(a * y)?, root(x * y)?, root(a * b)?, root(x * b)?];
    if channels == 1 {
        Ok(vec![values])
    } else {
        Ok(vec![
            [values[0], 0., values[2], 0.],
            [0., values[1], 0., values[3]],
        ])
    }
}
pub fn from_authored(enabled: bool, source_mask: u32, pan: [f32; 2]) -> Result<Vec<[f32; 4]>> {
    //8038b428..8038b458: separate adds then multiplies, not host FMA.
    let x = (100. + pan[0]) * f32::from_bits(0x3ba3d70a);
    let y = (100. + pan[1]) * f32::from_bits(0x3ba3d70a);
    matrix(enabled, source_mask, x, y)
}
/// Mode-zero output routing,8038c110..8038c1c8. Input includes the
/// per-voice gain. Rear energy contributes half to its corresponding side.
/// AX integer volume conversion and its 768-sample ramps are separate stages.
pub fn stereo_fold(quad: [f32; 4]) -> Result<[f32; 2]> {
    ensure!(
        quad.iter().all(|value| value.is_finite()),
        "Nonfinite output matrix"
    );
    let side = |front: f32, rear: f32| -> Result<f32> {
        // Original rear products/add are double precision; front square and
        // the final energy are rounded to single precision before frsqrte.
        let front_squared = front * front;
        let rear_squared = (0.5f64 * rear as f64) * rear as f64;
        root((front_squared as f64 + rear_squared) as f32)
    };
    Ok([side(quad[0], quad[2])?, side(quad[1], quad[3])?])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_output_gain_bits() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_stereo_fold.json"
        ))?;
        for case in fixture["gain_cases"].as_array().unwrap() {
            let db = f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
            assert_eq!(
                output_gain(db)?.to_bits(),
                case["output_bits"].as_u64().unwrap() as u32,
                "gain {db}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_stereo_output_fold_bits() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_stereo_fold.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let quad =
                serde_json::from_value::<[u32; 4]>(case["input_bits"].clone())?.map(f32::from_bits);
            assert_eq!(
                serde_json::json!(stereo_fold(quad)?.map(f32::to_bits)),
                case["output_bits"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_authored_pan_normalization_and_channel_matrix_bits() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_panner.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let pan =
                serde_json::from_value::<[u32; 2]>(case["pan_bits"].clone())?.map(f32::from_bits);
            let matrix = from_authored(
                case["enabled"].as_bool().unwrap(),
                case["mask"].as_u64().unwrap() as u32,
                pan,
            )?;
            assert_eq!(
                serde_json::json!(matrix
                    .iter()
                    .map(|row| row.map(f32::to_bits))
                    .collect::<Vec<_>>()),
                case["matrix_bits"],
                "case {case}"
            );
        }
        Ok(())
    }
    #[test]
    fn unsupported_panner_inputs_are_explicit() {
        assert!(matrix(true, 0, 0.5, 0.5).is_err());
        assert!(matrix(true, 7, 0.5, 0.5).is_err());
        assert!(matrix(true, 1, f32::NAN, 0.5).is_err());
    }
}
