//! A successful report write does not mean the contained preview probes passed.
pub fn failures(report: &serde_json::Value) -> Vec<String> {
    let Some(fields) = report.as_object() else {
        return vec!["Smoke report is not an object".into()];
    };
    fields
        .iter()
        .filter(|(key, _)| key.ends_with("_probe") || key.as_str() == "smoke")
        .filter_map(|(key, value)| {
            value
                .get("error")
                .filter(|error| !error.is_null())
                .map(|error| {
                    format!(
                        "{key}: {}",
                        error
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| error.to_string())
                    )
                })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contained_probe_errors_fail_even_when_loading_and_report_writing_succeed() {
        let report = serde_json::json!({"meshes":532,"moving_landing_animation_probe":{"error":"Movement through landing remained in idle"},"ear_pose_probe":{"error":null},"metadata":{"error":"unrelated diagnostic"}});
        assert_eq!(
            failures(&report),
            ["moving_landing_animation_probe: Movement through landing remained in idle"]
        );
        assert!(failures(
            &serde_json::json!({"ear_pose_probe":{"error":null},"smoke":{"completed":90}})
        )
        .is_empty());
        assert!(!failures(&serde_json::Value::Null).is_empty());
    }
}
