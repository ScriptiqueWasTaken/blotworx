//! Behavior attributes, REL2df2a4 ->14a24c. Actual AIType ancestry is searched
//! before Default; a missing Default property is registered once, not a fresh
//! caller-local fallback. Keys are handles into the shared live config map.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;

fn c_string(value: &str) -> &str {
    value.split('\0').next().unwrap_or("")
}
fn bounded(value: &str) -> Result<&str> {
    let end = value.len().min(255);
    ensure!(value.is_char_boundary(end), "Native AI config buffer splits a UTF-8 character; byte-string configuration is not connected");
    Ok(&value[..end])
}

/// The formatting domain used by shipped behavior/property symbols. A '%' in
/// either symbol becomes a format directive in native's second snprintf, so it
/// requires the original varargs semantics rather than literal substitution.
pub fn resolve(
    settings: &mut BTreeMap<String, String>,
    behavior: &str,
    ai_type: &str,
    property: &str,
    default: &str,
) -> Result<String> {
    let (behavior, property) = (c_string(behavior), c_string(property));
    ensure!(
        !behavior.contains('%') && !property.contains('%'),
        "AI behavior/property format directives require native varargs handling"
    );
    let format = format!("BEH_{behavior}_%s_{property}");
    ensure!(
        format.len() <= 255,
        "Truncated AI attribute format requires native varargs handling"
    );
    let mut subtype = bounded(c_string(ai_type))?;
    loop {
        let key = bounded(&format!("BEH_{behavior}_{subtype}_{property}"))?.to_owned();
        if settings.contains_key(&key) {
            return Ok(key);
        }
        match subtype.rfind(':') {
            Some(at) => subtype = &subtype[..at],
            None => break,
        }
    }
    let key = bounded(&format!("BEH_{behavior}_Default_{property}"))?.to_owned();
    settings
        .entry(key.clone())
        .or_insert_with(|| c_string(default).to_owned());
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_and_default_registration_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_attributes.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut settings: BTreeMap<String, String> =
                serde_json::from_value(case["input"]["settings"].clone())?;
            let input = &case["input"];
            let key = resolve(
                &mut settings,
                input["behavior"].as_str().unwrap(),
                input["ai_type"].as_str().unwrap_or(""),
                input["property"].as_str().unwrap(),
                input["default"].as_str().unwrap(),
            )?;
            assert_eq!(key, case["key"].as_str().unwrap(), "{input}");
            assert_eq!(serde_json::to_value(settings)?, case["settings"], "{input}");
        }
        Ok(())
    }
    #[test]
    fn fallback_is_shared_and_bound_handles_do_not_retarget_on_new_overrides() -> Result<()> {
        let mut settings = BTreeMap::new();
        let key = resolve(
            &mut settings,
            "Conversation",
            "Gremlin:Gus",
            "PlayStartupAnimation",
            "false",
        )?;
        assert_eq!(key, "BEH_Conversation_Default_PlayStartupAnimation");
        assert_eq!(
            resolve(
                &mut settings,
                "Conversation",
                "Cow",
                "PlayStartupAnimation",
                "true"
            )?,
            key
        );
        assert_eq!(
            settings[&key], "false",
            "A second caller replaced the shared fallback"
        );
        settings.insert(
            "BEH_Conversation_Gremlin_PlayStartupAnimation".into(),
            "true".into(),
        );
        assert_eq!(
            settings[&key], "false",
            "An already-bound handle silently retargeted"
        );
        assert_eq!(
            resolve(
                &mut settings,
                "Conversation",
                "Gremlin:Gus",
                "PlayStartupAnimation",
                "false"
            )?,
            "BEH_Conversation_Gremlin_PlayStartupAnimation"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_binds_authored_conversation_overrides_and_live_property_handles() -> Result<()>
    {
        use anyhow::Context as _;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        let owner = world
            .scripts
            .resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus absent")?;
        let slot = *world.ai_brains[&owner]
            .conversations
            .keys()
            .next()
            .context("Original Conversation absent")?;
        let configuration = world.ai_brains[&owner].conversations[&slot]
            .configuration
            .as_ref()
            .context("Published Conversation lacks configuration")?;
        assert_eq!(
            configuration.turn_to_face_player,
            "BEH_Conversation_Default_TurnToFacePlayer"
        );
        assert_eq!(
            configuration.play_startup_animation,
            "BEH_Conversation_Gremlin_PlayStartupAnimation"
        );
        assert_eq!(
            configuration.play_resolve_animation,
            "BEH_Conversation_Gremlin_PlayResolveAnimation"
        );
        let values = configuration.values(&world.runtime_settings)?;
        assert!(
            values.turn_to_face_player
                && values.play_startup_animation
                && values.play_resolve_animation
        );
        let startup_key = configuration.play_startup_animation.clone();
        world.runtime_settings.insert(startup_key, "false".into());
        assert!(
            !world.ai_brains[&owner].conversations[&slot]
                .configuration
                .as_ref()
                .unwrap()
                .values(&world.runtime_settings)?
                .play_startup_animation,
            "Configuration was a stale bool snapshot"
        );
        let mut generic = super::super::ai_conversation::Conversation::default();
        generic.configure("Unknown:Subtype", &mut world.runtime_settings)?;
        let values = generic
            .configuration
            .as_ref()
            .unwrap()
            .values(&world.runtime_settings)?;
        assert!(
            values.turn_to_face_player
                && !values.play_startup_animation
                && !values.play_resolve_animation
        );
        Ok(())
    }
}
