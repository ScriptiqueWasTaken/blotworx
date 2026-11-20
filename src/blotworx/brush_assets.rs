//! Authored brush attachment graph. Definitions are shared; child/FX edges
//! retain their order and ownership for subsequent runtime instantiation.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Component, SceneFile};
use std::collections::BTreeMap;

#[derive(Clone, serde::Serialize)]
pub struct Definition {
    pub name: String,
    pub entity_id: u32,
    pub source_scene: String,
    pub children: Vec<String>,
    pub effects: Vec<Effect>,
    pub model: Option<String>,
    #[serde(skip)]
    pub components: Vec<Component>,
}

#[cfg(test)]
mod effect_template_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_effect_templates_retain_components_and_particle_resources() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut files = Vec::new();
        for name in ["_dynamic.pak", "r3mt.pak", "gremlinvillage_zonea.pak"] {
            files.extend(
                crate::pack_io::read(std::fs::read(root.join("packfiles").join(name))?)?
                    .pak
                    .files,
            );
        }
        let mut seen = std::collections::HashSet::new();
        let scenes = files
            .iter()
            .filter(|f| {
                seen.insert(f.path.to_ascii_lowercase())
                    && (f.path.to_ascii_lowercase().starts_with("palettes/")
                        || f.path
                            .to_ascii_lowercase()
                            .ends_with("/gremlinvillage_zonea.bin"))
                    && f.path.to_ascii_lowercase().ends_with(".bin")
            })
            .map(|f| {
                Ok((
                    f.path.clone(),
                    super::super::scene_loading::parse_scene(&f.path, &f.data)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut rows = Vec::new();
        for name in [
            "gremlinvariant4teleportoutfx",
            "gremlinvariant4teleportinfx",
        ] {
            let graph = Assets::effect(name, &scenes, |path| {
                files
                    .iter()
                    .find(|f| {
                        f.path.eq_ignore_ascii_case(path)
                            || f.path.eq_ignore_ascii_case(&format!("{path}_wii"))
                    })
                    .map(|f| f.data.clone())
                    .with_context(|| format!("Missing original effect model {path}"))
            })?;
            let mut models = Vec::new();
            for (path, bytes) in &graph.models {
                let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(bytes)
                    .map_err(anyhow::Error::msg)?;
                let playback = super::super::brush_particles::Hose::load_attached(bytes, &files);
                models.push(serde_json::json!({"path":path,"blocks":nif.blocks.iter().map(|b|b.type_name()).collect::<Vec<_>>(),
                    "playback_error":playback.err().map(|e|format!("{e:#}"))}));
            }
            let components=graph.definitions.iter().map(|(name,definition)|(name,definition.components.iter().map(|c|serde_json::json!({
                "class":c.class_name,"properties":c.properties.iter().map(|p|serde_json::json!({"name":p.name,"value":p.value})).collect::<Vec<_>>()
            })).collect::<Vec<_>>())).collect::<BTreeMap<_,_>>();
            rows.push(serde_json::json!({"template":name,"graph":graph,"components":components,"models":models}));
        }
        if let Some(path) = std::env::var_os("BLOTWORX_FX_AUDIT") {
            std::fs::write(path, serde_json::to_vec_pretty(&rows)?)?;
        }
        assert_eq!(rows.len(), 2);
        Ok(())
    }
}
#[derive(Clone, serde::Serialize)]
pub struct Effect {
    pub template: String,
    pub fire_and_forget: bool,
}
#[derive(serde::Serialize)]
pub struct Assets {
    pub root: String,
    pub streams: [String; 2],
    pub definitions: BTreeMap<String, Definition>,
    /// Owned source data survives the importer's temporary packfile storage.
    #[serde(skip)]
    pub models: BTreeMap<String, Vec<u8>>,
}
fn property<'a>(
    components: &'a [Component],
    class: &str,
    name: &str,
) -> Option<&'a serde_json::Value> {
    components
        .iter()
        .find(|c| c.class_name == class)?
        .properties
        .iter()
        .find(|p| p.name == name)
        .map(|p| &p.value)
}
fn names(components: &[Component], class: &str, name: &str) -> Result<Vec<String>> {
    let Some(value) = property(components, class, name) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .with_context(|| format!("{class}.{name} is not an array"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .with_context(|| format!("Invalid {class}.{name} entry"))
        })
        .collect()
}
impl Assets {
    /// Resource declarations only; runtime factories load model payloads on
    /// first use, rather than cooking every available effect during level load.
    pub fn declarations(
        name: &str,
        scenes: &[(String, SceneFile)],
    ) -> Result<BTreeMap<String, Definition>> {
        let mut graph = Self {
            root: name.to_ascii_lowercase(),
            streams: Default::default(),
            definitions: BTreeMap::new(),
            models: BTreeMap::new(),
        };
        graph.visit(name, scenes, &mut Vec::new())?;
        Ok(graph.definitions)
    }
    pub fn load(
        scenes: &[(String, SceneFile)],
        mut read: impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let mut result = Self {
            root: "magicbrush".into(),
            streams: ["paintstream".into(), "thinnerstream".into()],
            definitions: BTreeMap::new(),
            models: BTreeMap::new(),
        };
        result.visit("magicbrush", scenes, &mut Vec::new())?;
        // REL 0xa8ad4 resolves PaintStream / ThinnerStream on the brush
        // resource. Do not accidentally bind Mickey's surface-splash FX.
        let root = &result.definitions[&result.root];
        for stream in &result.streams {
            ensure!(
                root.children.iter().any(|c| c.eq_ignore_ascii_case(stream)),
                "Brush is missing child {stream}"
            );
        }
        for definition in result.definitions.values() {
            if let Some(path) = &definition.model {
                if !result.models.contains_key(path) {
                    result.models.insert(
                        path.clone(),
                        read(path).with_context(|| {
                            format!("Brush entity {} model {path}", definition.name)
                        })?,
                    );
                }
            }
        }
        Ok(result)
    }
    pub fn effect(
        name: &str,
        scenes: &[(String, SceneFile)],
        mut read: impl FnMut(&str) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let mut result = Self {
            root: name.to_ascii_lowercase(),
            streams: Default::default(),
            definitions: BTreeMap::new(),
            models: BTreeMap::new(),
        };
        result.visit(name, scenes, &mut Vec::new())?;
        for definition in result.definitions.values() {
            if let Some(path) = &definition.model {
                if !result.models.contains_key(path) {
                    result.models.insert(path.clone(), read(path)?);
                }
            }
        }
        Ok(result)
    }
    fn visit(
        &mut self,
        name: &str,
        scenes: &[(String, SceneFile)],
        stack: &mut Vec<String>,
    ) -> Result<()> {
        let key = name.to_ascii_lowercase();
        ensure!(
            !stack.contains(&key),
            "Recursive brush attachment: {} -> {name}",
            stack.join(" -> ")
        );
        if self.definitions.contains_key(&key) {
            return Ok(());
        }
        let global = scenes
            .iter()
            .find(|(path, _)| {
                path.replace('\\', "/")
                    .eq_ignore_ascii_case("Palettes/_Dynamic.bin")
            })
            .filter(|(_, scene)| {
                scene
                    .objects
                    .iter()
                    .any(|e| e.name.eq_ignore_ascii_case(name))
            });
        let mut dedicated = scenes.iter().filter(|(path, scene)| {
            let normalized = path.replace('\\', "/");
            normalized.split('/').last().is_some_and(|file| {
                file.rsplit_once('.').is_some_and(|(stem, extension)| {
                    stem.eq_ignore_ascii_case(name) && extension.eq_ignore_ascii_case("bin")
                })
            }) && normalized.to_ascii_lowercase().starts_with("palettes/")
                && scene.objects.iter().any(|entity| entity.name.eq_ignore_ascii_case(name))
        });
        let preferred = if global.is_some() {
            global
        } else {
            let exact = dedicated.next();
            ensure!(dedicated.next().is_none(), "Ambiguous dedicated effect palette for {name}");
            exact
        };
        let candidates: Vec<_> = preferred
            .into_iter()
            .chain(scenes.iter().filter(|_| preferred.is_none()))
            .collect();
        let mut matches = candidates.iter().flat_map(|(path, scene)| {
            scene
                .objects
                .iter()
                .filter(move |e| e.name.eq_ignore_ascii_case(name))
                .map(move |e| (path, scene, e))
        });
        let (source, scene, entity) = matches
            .next()
            .with_context(|| format!("Missing brush entity template {name}"))?;
        ensure!(
            matches.next().is_none(),
            "Ambiguous brush entity template {name}"
        );
        // IDs/master links are local to each palette. Flattening all objects
        // before inheritance resolution can bind an unrelated component.
        let components = crate::render_support::scene::get_effective_components(entity, &scene.objects);
        let children = names(&components, "ChildComponent", "Child Templates")?;
        let fx = names(&components, "FXComponent", "FXTemplate")?;
        let flags = property(&components, "FXComponent", "FXTemplateFireAndForget")
            .map(|v| v.as_array().context("Invalid FXTemplateFireAndForget"))
            .transpose()?;
        ensure!(
            fx.len() == flags.map_or(0, Vec::len),
            "Mismatched FX template flags on {name}"
        );
        let effects = fx
            .iter()
            .enumerate()
            .map(|(i, template)| {
                Ok(Effect {
                    template: template.clone(),
                    fire_and_forget: flags.unwrap()[i]
                        .as_bool()
                        .with_context(|| format!("Invalid FX flag {i} on {name}"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let model = super::assets::render_model_path(&components)?;
        stack.push(key.clone());
        for dependency in children.iter().chain(fx.iter()) {
            self.visit(dependency, scenes, stack)?;
        }
        stack.pop();
        self.definitions.insert(
            key,
            Definition {
                name: entity.name.clone(),
                entity_id: entity.link_id.to_u32(),
                source_scene: source.to_string(),
                children,
                effects,
                model,
                components,
            },
        );
        Ok(())
    }
}
