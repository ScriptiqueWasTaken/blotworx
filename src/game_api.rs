//! Retail-observed symbols, with provenance and explicit review status.
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Definition {
    pub path: String,
    pub line: usize,
    pub parameters: Vec<String>,
    pub reviewed: bool,
    #[serde(default)]
    pub documentation: String,
}
#[derive(Deserialize)]
pub struct Symbol {
    pub name: String,
    pub references: Vec<String>,
    pub definitions: Vec<Definition>,
}
#[derive(Deserialize)]
pub struct Catalog {
    pub scripts: usize,
    pub symbols: Vec<Symbol>,
}
pub fn catalog() -> &'static Catalog {
    static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("data/game_api.json")).expect("generated API catalog")
    })
}
