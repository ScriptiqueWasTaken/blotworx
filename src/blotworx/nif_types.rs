//! Runtime ancestry recovered from EM1 DOL registrations and a closed REL subset.
//! This is deliberately not a schema inferred from class-name suffixes.
use anyhow::{Context, Result};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(serde::Deserialize)]
struct Class {
    base: Option<String>,
}
#[derive(serde::Deserialize)]
struct Registry {
    classes: BTreeMap<String, Class>,
}
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry: Registry = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/em1_dol_rtti.json"
        ))
        .expect("Verified EM1 DOL RTTI fixture");
        let rel: Registry = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/em1_rel_rtti.json"
        ))
        .expect("Verified closed EM1 REL RTTI subset");
        for (name, class) in rel.classes {
            assert!(
                registry.classes.insert(name, class).is_none(),
                "Duplicate runtime type"
            );
        }
        registry
    })
}
pub fn block_name(nif: &epic_mickey_lib_rs::nif_file::NifFile, id: u32) -> Result<&str> {
    let object = nif
        .blocks
        .get(id as usize)
        .context("Missing NIF type block")?;
    Ok(nif
        .block_type_indices
        .get(id as usize)
        .and_then(|i| nif.block_types.get(*i as usize))
        .map(String::as_str)
        .unwrap_or_else(|| object.type_name()))
}
pub fn is_a(class: &str, base: &str) -> Result<bool> {
    let classes = &registry().classes;
    classes
        .get(base)
        .with_context(|| format!("Unverified EM1 runtime base type {base}"))?;
    let mut current = Some(class);
    for _ in 0..=classes.len() {
        let Some(name) = current else {
            return Ok(false);
        };
        let entry = classes
            .get(name)
            .with_context(|| format!("Unverified EM1 runtime type {name}"))?;
        if name == base {
            return Ok(true);
        }
        current = entry.base.as_deref();
    }
    anyhow::bail!("Cycle in EM1 runtime ancestry for {class}")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovered_ancestry_is_closed_and_acyclic() -> Result<()> {
        assert_eq!(registry().classes.len(), 368);
        for name in registry().classes.keys() {
            is_a(name, "NiPSEmitterCtlr")?;
        }
        assert!(is_a("NiPSEmitterSpeedCtlr", "NiPSEmitterCtlr")?);
        assert!(is_a("NiPSEmitParticlesCtlr", "NiPSEmitterCtlr")?);
        assert!(!is_a("NiPSFieldMagnitudeCtlr", "NiPSEmitterCtlr")?);
        assert!(!is_a("NiPSParticleSystem", "NiNode")?);
        assert!(is_a("NiSortAdjustNode", "NiNode")?);
        assert!(is_a("NonUniformParticleSystem", "NiPSParticleSystem")?);
        assert!(!is_a("NonUniformParticleSystem", "NiNode")?);
        assert!(is_a("NonUniformQuadGenerator", "NiMeshModifier")?);
        assert!(is_a("JPSCameraComponent", "NiObject").is_err());
        assert!(is_a("UnknownEmitterCtlr", "NiPSEmitterCtlr").is_err());
        Ok(())
    }
}
