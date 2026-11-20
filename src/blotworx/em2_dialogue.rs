//! Explicit playtest presentation using original PC dictionary text. Voice
//! timing, speaking animations and native dialog-manager UI remain pending.
use anyhow::{ensure, Context, Result};
use std::{collections::HashMap, path::Path};
pub(super) struct Dictionary {
    pub source: String,
    seed: u32,
    entries: HashMap<u32, String>,
}
#[derive(Clone, serde::Serialize)]
pub(super) struct Line {
    pub speaker: String,
    pub text: String,
    pub text_key: String,
    pub audio_key: Option<String>,
}
impl Dictionary {
    pub fn load(root: &Path, language: &str) -> Result<Self> {
        let language = language.trim_matches('"').to_ascii_lowercase();
        let mut matches = Vec::new();
        for entry in std::fs::read_dir(root.join("Localize"))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with(&language) && name.ends_with("_win32.dct") {
                matches.push(entry.path());
            }
        }
        ensure!(
            matches.len() == 1,
            "Expected one configured dialogue dictionary for {language}, found {}",
            matches.len()
        );
        let path = &matches[0];
        let bytes = std::fs::read(path)?;
        let dct = std::panic::catch_unwind(|| epic_mickey_lib_rs::dct::DCT::from_binary(bytes))
            .map_err(|_| anyhow::anyhow!("Invalid dialogue dictionary {}", path.display()))?;
        Ok(Self {
            source: path
                .file_name()
                .context("Dictionary filename absent")?
                .to_string_lossy()
                .into_owned(),
            seed: dct.hash_seed,
            entries: dct
                .dialog_entries
                .into_iter()
                .map(|e| (e.hashed_key, e.text))
                .collect(),
        })
    }
    pub fn find(&self, key: &str) -> Option<&str> {
        self.entries
            .get(&epic_mickey_lib_rs::dct::epic_mickey_hash(
                key.as_bytes(),
                self.seed,
            ))
            .map(String::as_str)
    }
    pub fn line(&self, fields: &serde_json::Value) -> Result<Line> {
        let strings = fields["dialog"]["strings"]
            .as_array()
            .context("Dialogue string fields absent")?;
        ensure!(
            strings.len() == 7,
            "Unexpected Apprentice44 dialogue field count"
        );
        let text_key = strings[2].as_str().context("Dialogue text key absent")?;
        let speaker = strings[1]
            .as_str()
            .or(strings[0].as_str())
            .context("Dialogue speaker absent")?;
        Ok(Line {
            speaker: self.find(speaker).unwrap_or(speaker).into(),
            text: self
                .find(text_key)
                .with_context(|| format!("Missing dialogue text {text_key}"))?
                .into(),
            text_key: text_key.into(),
            audio_key: strings[5].as_str().map(str::to_owned),
        })
    }
}
