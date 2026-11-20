//! PC ChildComponent9b5830/9b5040: ordered template clones, separate from FX
//! registrationad1cf0. Source scenes/assets stay immutable on disk.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    packfile::VirtualFile,
    scene_file::{Component, Entity, SceneFile, ID},
};
use std::collections::HashMap;
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Link {
    pub parent_scene: usize,
    pub parent: u32,
    pub child_scene: usize,
    pub child: u32,
    pub template: String,
    pub resource: String,
}
#[derive(Default, serde::Serialize)]
pub(super) struct Graph {
    pub links: Vec<Link>,
    pub errors: Vec<Failure>,
    fx_registrations: Vec<Effect>,
    #[serde(skip)]
    pub templates: std::sync::Arc<Templates>,
}
#[derive(Debug, serde::Serialize)]
pub(super) struct Failure {
    pub scene: usize,
    pub parent: u32,
    pub message: String,
}
#[derive(serde::Serialize)]
struct Effect {
    scene: usize,
    owner: u32,
    component: u32,
    template: String,
    fire_and_forget: bool,
    resource: Option<String>,
}
struct Definition {
    entity: Entity,
    resource: String,
}
/// Resolved source templates shared by direct children and runtime factories.
/// Stored once; callers share the index instead of reparsing the palettes.
#[derive(Default)]
pub(super) struct Templates {
    definitions: HashMap<String, Vec<Definition>>,
}
pub(super) struct Spawn {
    pub entity: Entity,
    pub resource: String,
}
impl Templates {
    pub fn load(files: &[VirtualFile]) -> Result<Self> {
        let mut definitions: HashMap<String, Vec<Definition>> = HashMap::new();
        for file in files.iter().filter(|f| {
            f.path
                .replace('\\', "/")
                .to_ascii_lowercase()
                .starts_with("palettes/")
                && f.path.to_ascii_lowercase().ends_with(".bin")
        }) {
            let source = super::scene_loading::parse_scene(&file.path, &file.data)?;
            let components = super::actor_io::component_index(&source.objects);
            for prototype in &source.objects {
                let mut entity = prototype.clone();
                entity.components =
                    crate::render_support::scene::get_effective_components(prototype, &source.objects);
                super::actor_io::inherit_connections(&mut entity.components, &components)?;
                for c in &mut entity.components {
                    c.master_link_id = ID::new(0);
                }
                entity.master_link_id = ID::new(0);
                // The same master can be embedded in more than one palette. Only coalesce
                // exact source identities with equivalent resolved fields, not same names.
                let entries = definitions.entry(key(&entity.name)).or_default();
                let equivalent = entries.iter().any(|d| {
                    d.entity.to_dict(source.version.clone())
                        == entity.to_dict(source.version.clone())
                });
                if !equivalent {
                    entries.push(Definition {
                        entity,
                        resource: file.path.clone(),
                    });
                }
            }
        }
        Ok(Self { definitions })
    }
    fn get(&self, name: &str) -> Option<&Vec<Definition>> {
        self.definitions.get(name)
    }
    pub fn maximum_identity(&self) -> u32 {
        self.definitions
            .values()
            .flatten()
            .flat_map(|d| {
                std::iter::once(d.entity.link_id.to_u32())
                    .chain(d.entity.components.iter().map(|c| c.link_id.to_u32()))
            })
            .max()
            .unwrap_or(0)
    }
    /// Value-stage clone. World/component phase dispatch remains the caller's
    /// responsibility; compound prefab member cloning stays explicit.
    pub fn instantiate(&self, name: &str, next: &mut u32) -> Result<Spawn> {
        let choices = self
            .get(&key(name))
            .with_context(|| format!("Runtime template is absent: {name}"))?;
        ensure!(choices.len() == 1, "Ambiguous runtime template {name}");
        let definition = &choices[0];
        let mut entity = definition.entity.clone();
        ensure!(
            !entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSPrefabComponent"),
            "Runtime prefab member cloning is not implemented: {name}"
        );
        entity.link_id = allocate(next)?;
        for component in &mut entity.components {
            component.link_id = allocate(next)?;
        }
        entity.name = format!("{}_{}", entity.name, entity.link_id.to_u32());
        Ok(Spawn {
            entity,
            resource: definition.resource.clone(),
        })
    }
}
fn names(components: &[Component], class: &str, property: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for c in components.iter().filter(|c| c.class_name == class) {
        if let Some(p) = c.properties.iter().find(|p| p.name == property) {
            for n in p
                .value
                .as_array()
                .with_context(|| format!("Invalid {class}.{property}"))?
            {
                out.push(
                    n.as_str()
                        .context("Invalid template name")?
                        .split('\0')
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
        }
    }
    Ok(out)
}
fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}
/// Allocates entity AND component identities globally, never once per scene.
fn allocate(next: &mut u32) -> Result<ID> {
    *next = next
        .checked_add(1)
        .context("Runtime entity/component identity overflow")?;
    Ok(ID::from_u32(*next))
}
pub(super) fn expand(
    scenes: &mut Vec<SceneFile>,
    scene_names: &mut Vec<String>,
    files: &[VirtualFile],
) -> Result<Graph> {
    ensure!(
        scenes.len() == scene_names.len(),
        "Child source identities differ"
    );
    let Some(first) = scenes.first() else {
        return Ok(Graph::default());
    };
    let mut children = SceneFile::new(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ID::new(0),
        first.version.clone(),
    );
    let child_scene = scenes.len();
    let mut next = scenes
        .iter()
        .flat_map(|s| &s.objects)
        .flat_map(|e| {
            std::iter::once(e.link_id.to_u32())
                .chain(e.components.iter().map(|c| c.link_id.to_u32()))
        })
        .max()
        .unwrap_or(0);
    let definitions = Templates::load(files)?;
    next = next.max(definitions.maximum_identity());
    let mut graph = Graph::default();
    for (scene_index, source) in scenes.iter().enumerate() {
        for owner in source
            .objects
            .iter()
            .filter(|e| source.scene.contains(&e.link_id))
        {
            let components =
                crate::render_support::scene::get_effective_components(owner, &source.objects);
            let requests = names(&components, "ChildComponent", "Child Templates")?;
            let pose = crate::render_support::scene::get_entity_transform(owner, &source.objects);
            let mut stack = requests
                .into_iter()
                .rev()
                .map(|name| {
                    (
                        scene_index,
                        owner.link_id.to_u32(),
                        pose,
                        name,
                        Vec::<String>::new(),
                    )
                })
                .collect::<Vec<_>>();
            while let Some((parent_scene, parent, parent_pose, name, mut ancestry)) = stack.pop() {
                if name.is_empty() {
                    continue;
                }
                let result = (|| -> Result<()> {
                    let normalized = key(&name);
                    ensure!(
                        !ancestry.contains(&normalized),
                        "Recursive child chain {} -> {name}",
                        ancestry.join(" -> ")
                    );
                    let spawned = definitions.instantiate(&name, &mut next)?;
                    let mut entity = spawned.entity;
                    let resource = spawned.resource;
                    let mut pose =
                        crate::render_support::scene::get_entity_transform(&entity, &[entity.clone()]);
                    pose.translation = parent_pose.translation;
                    pose.rotation = parent_pose.rotation;
                    //9b58cc..9b58d9 clears persistence after child creation, not source flags.
                    entity.unknown &= !0x10;
                    crate::render_support::scene::set_entity_transform(&mut entity, pose);
                    let nested = names(&entity.components, "ChildComponent", "Child Templates")?;
                    let child = entity.link_id.to_u32();
                    children.scene.push(entity.link_id);
                    children.objects.push(entity);
                    graph.links.push(Link {
                        parent_scene,
                        parent,
                        child_scene,
                        child,
                        template: name.clone(),
                        resource,
                    });
                    ancestry.push(normalized);
                    stack.extend(
                        nested
                            .into_iter()
                            .rev()
                            .map(|name| (child_scene, child, pose, name, ancestry.clone())),
                    );
                    Ok(())
                })();
                if let Err(error) = result {
                    graph.errors.push(Failure {
                        scene: parent_scene,
                        parent,
                        message: format!("{error:#}"),
                    });
                }
            }
        }
    }
    if !children.objects.is_empty() {
        scenes.push(children);
        scene_names.push("<runtime child entities>".into());
    }
    //ad1cf0 only registers these master/flag pairs. Persistent entries must
    //NOT be instantiated until StartChildFX; doing so activates dormant FX.
    for (scene_index, source) in scenes.iter().enumerate() {
        for entity in source
            .objects
            .iter()
            .filter(|e| source.scene.contains(&e.link_id))
        {
            for component in
                crate::render_support::scene::get_effective_components(entity, &source.objects)
                    .iter()
                    .filter(|c| c.class_name == "FXComponent")
            {
                let templates =
                    names(std::slice::from_ref(component), "FXComponent", "FXTemplate")?;
                let flags = component
                    .properties
                    .iter()
                    .find(|p| p.name == "FXTemplateFireAndForget")
                    .and_then(|p| p.value.as_array());
                for (index, template) in templates.into_iter().enumerate() {
                    let candidates = definitions.get(&key(&template));
                    let resource = candidates
                        .filter(|c| c.len() == 1)
                        .map(|c| c[0].resource.clone());
                    graph.fx_registrations.push(Effect {
                        scene: scene_index,
                        owner: entity.link_id.to_u32(),
                        component: component.link_id.to_u32(),
                        template,
                        fire_and_forget: flags
                            .and_then(|v| v.get(index))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        resource,
                    });
                }
            }
        }
    }
    graph.templates = std::sync::Arc::new(definitions);
    Ok(graph)
}
impl Graph {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"instances":self.links,"errors":self.errors,"fx_registrations":self.fx_registrations,
  "scope":"PC direct Child Templates cloned in authored depth-first order; live attachment is a separate owner; child lifecycle events and on-demand FX creation remain incomplete"})
    }
}
