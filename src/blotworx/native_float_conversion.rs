//! Shared native numeric conversion; available without optional UI features.
/// DOL8000d778: truncate to unsigned int, clamp negatives to zero and
///saturate positive overflow/NaN to UINT_MAX. Rust's NaN cast differs.
pub fn unsigned(value:f32)->u32 {
    if value<0. {0}
    else if value.is_nan() || value as f64>=4294967296. {u32::MAX}
    else {value as u32}
}
#[cfg(test)]
mod tests {
    #[test]
    fn unsigned_conversion_matches_original()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ui_effect_pixels.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(super::unsigned(f32::from_bits(case["input_bits"].as_u64().unwrap() as u32)) as u64,
                case["pixel"].as_u64().unwrap(),"{case}");
        }
        Ok(())
    }
}
