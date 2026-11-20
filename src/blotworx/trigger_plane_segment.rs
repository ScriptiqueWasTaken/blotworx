//! REL3cef60..3cef8c plane/segment admission, after normal projection.
//! Bounds and tracked-body selection are separate from this scalar predicate.
pub fn fraction(previous_distance: f32, current_distance: f32) -> Option<f32> {
    let denominator = previous_distance - current_distance;
    // REL4d0cf4. Strict comparison also declines unordered input.
    if !(denominator.abs() > f32::from_bits(0x38d1b717)) {
        return None;
    }
    let fraction = -current_distance / denominator;
    if fraction >= 0. && fraction <= 1. {
        Some(fraction)
    } else {
        None
    }
}

/// REL3cf0d4: event69 for a negative projected motion; otherwise6a.
/// Call only after segment admission, including an endpoint on the plane.
pub fn forward(previous_distance: f32, current_distance: f32) -> bool {
    previous_distance - current_distance < 0.
}

#[cfg(test)]
mod tests {
    #[test]
    fn plane_segment_admission_matches_original_inclusive_endpoints() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/trigger_plane_segment.json"
        )).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let bits = |name: &str| case[name].as_u64().unwrap() as u32;
            let result = super::fraction(f32::from_bits(bits("previous")), f32::from_bits(bits("current")));
            assert_eq!(result.map(f32::to_bits), case["fraction"].as_u64().map(|v| v as u32), "{case}");
            if result.is_some() {
                assert_eq!(super::forward(f32::from_bits(bits("previous")), f32::from_bits(bits("current"))), case["forward"].as_bool().unwrap(), "{case}");
            }
        }
        Ok(())
    }
}
