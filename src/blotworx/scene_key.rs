//! Scene-manager key1833f8, used by register180df0, query1808f0 and
//! removal180298. Resource filenames/queued requests have separate identities.
use std::borrow::Cow;
fn stem(path: &[u8]) -> &[u8] {
    let path = path.split(|&b| b == 0).next().unwrap_or_default();
    // Source strips backslashes first, then forward slashes in the remainder.
    let path = path.rsplit(|&b| b == b'\\').next().unwrap_or_default();
    let path = path.rsplit(|&b| b == b'/').next().unwrap_or_default();
    path.iter().rposition(|&b| b == b'.').map_or(path, |end| &path[..end])
}
pub fn bytes(path: &[u8]) -> Vec<u8> { stem(path).to_ascii_lowercase() }
pub fn key(path: &str) -> Cow<'_, str> {
    // ASCII delimiters cannot split a UTF8 code point; lowercase is ASCII-only.
    let stem = std::str::from_utf8(stem(path.as_bytes())).expect("Scene key UTF8 boundary");
    if stem.bytes().any(|b| b.is_ascii_uppercase()) { Cow::Owned(stem.to_ascii_lowercase()) }
    else { Cow::Borrowed(stem) }
}
#[cfg(test)]
mod tests {
    #[test]
    fn original_scene_key_path_order_case_and_last_extension_match() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../tools/blotworx/fixtures/scene_key.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let input = row["input"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>();
            let expected = row["output"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>();
            assert_eq!(super::bytes(&input), expected, "{input:?}");
            if let Ok(s) = std::str::from_utf8(&input) { assert_eq!(super::key(s).as_bytes(), expected); }
        }
        Ok(())
    }
}
