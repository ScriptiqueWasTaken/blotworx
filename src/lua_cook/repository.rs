//! Only manually reviewed, instruction-verified original sources are registered.
pub struct Entry {
    pub path: &'static str,
    pub bytecode_sha256: &'static str,
    pub source_sha256: &'static str,
    pub executable_sha256: &'static str,
    pub source: &'static str,
}
pub fn lookup(data: &[u8]) -> Option<&'static Entry> {
    let hash = super::content_hash(data);
    ENTRIES.iter().find(|e| e.bytecode_sha256 == hash)
}
pub fn lookup_version(data: &[u8], version: &str) -> Option<&'static Entry> {
    let hash = super::content_hash(data);
    ENTRIES
        .iter()
        .find(|e| e.bytecode_sha256 == hash && self::version(e) == version)
}
pub fn version(entry: &Entry) -> String {
    format!("reviewed-v1:{}", entry.source_sha256)
}
include!("repository_index.rs");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_sources_reproduce_verified_executable_content() {
        let mut hashes = std::collections::BTreeSet::new();
        for entry in ENTRIES {
            assert!(
                hashes.insert((entry.bytecode_sha256, entry.source_sha256)),
                "Duplicate original revision"
            );
            assert_eq!(
                super::super::content_hash(entry.source.as_bytes()),
                entry.source_sha256,
                "{}",
                entry.path
            );
            let compiled = super::super::compile(entry.source).unwrap();
            assert_eq!(
                super::super::executable_signature(&compiled).unwrap(),
                entry.executable_sha256,
                "{}",
                entry.path
            );
        }
    }
}
