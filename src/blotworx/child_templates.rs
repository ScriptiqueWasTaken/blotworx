//! Runtime ChildComponent template expansion. Source objects remain immutable;
//! every child has its own runtime identity/components and ordered owner slot.
//! REL3121f4 visits authored template order;312aa0 spawns using owner pose,
//! then attaches the transform and312424 admits the child to its owner.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    packfile::VirtualFile,
    scene_file::{Component, Entity, SceneFile, ID},
};
use std::collections::{HashSet, VecDeque};
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
fn set(
    components: &mut [Component],
    class: &str,
    name: &str,
    value: serde_json::Value,
    scene: &SceneFile,
) -> Result<()> {
    let component = components
        .iter_mut()
        .find(|c| c.class_name == class)
        .with_context(|| format!("Child has no {class}"))?;
    if let Some(property) = component.properties.iter_mut().find(|p| p.name == name) {
        property.value = value;
    } else {
        let defaults =
            crate::component_schema::CATALOG.components[class].instantiate(class, scene)?;
        let mut property = defaults
            .properties
            .into_iter()
            .find(|p| p.name == name)
            .with_context(|| format!("Missing native child field {class}.{name}"))?;
        property.value = value;
        component.properties.push(property);
    }
    Ok(())
}
pub fn expand(
    path: &str,
    scene: &mut SceneFile,
    files: &[VirtualFile],
    globals: Option<&SceneFile>,
    report: &mut super::assets::Report,
) -> Result<()> {
    let palettes = files
        .iter()
        .filter(|f| {
            f.path
                .replace('\\', "/")
                .to_ascii_lowercase()
                .starts_with("palettes/")
                && f.path.to_ascii_lowercase().ends_with(".bin")
        })
        .map(|f| {
            Ok((
                f.path.clone(),
                super::scene_loading::parse_scene(&f.path, &f.data)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let placed: HashSet<_> = scene.scene.iter().copied().collect();
    let mut queue = scene
        .objects
        .iter()
        .enumerate()
        .filter(|(_, entity)| placed.contains(&entity.link_id))
        .map(|(index, _)| (index, Vec::<String>::new()))
        .collect::<VecDeque<_>>();
    let mut next = scene
        .objects
        .iter()
        .flat_map(|entity| std::iter::once(entity.link_id.to_u32()).chain(entity.components.iter().map(|component|component.link_id.to_u32())))
        .max()
        .unwrap_or(0);
    while let Some((owner, ancestry)) = queue.pop_front() {
        let root = scene.objects[owner].clone();
        let mut components = crate::render_support::scene::get_effective_components(&root, &scene.objects);
        let Some(templates) = property(&components, "ChildComponent", "Child Templates") else {
            continue;
        };
        let templates = templates
            .as_array()
            .context("Invalid Child Templates array")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .context("Invalid child template name")
            })
            .collect::<Result<Vec<_>>>()?;
        let mut children = property(&components, "ChildComponent", "Child Entities")
            .map(|value| {
                value
                    .as_array()
                    .cloned()
                    .context("Invalid Child Entities array")
            })
            .transpose()?
            .unwrap_or_default();
        for name in templates {
            if name.is_empty() {
                continue;
            }
            let result = (|| -> Result<(Entity, String)> {
                let normalized = name.to_ascii_lowercase();
                ensure!(
                    !ancestry.contains(&normalized),
                    "Recursive child template chain: {} -> {name}",
                    ancestry.join(" -> ")
                );
                let local = scene.objects.iter().find(|entity| {
                    !scene.scene.contains(&entity.link_id)
                        && entity.name.eq_ignore_ascii_case(&name)
                });
                let global = globals.and_then(|global| {
                    global
                        .objects
                        .iter()
                        .find(|entity| entity.name.eq_ignore_ascii_case(&name))
                        .map(|entity| (global, entity))
                });
                let (source, prototype, source_path) = if let Some((source, prototype)) = global {
                    (source, prototype, "Palettes/_Dynamic.bin".to_owned())
                } else if let Some(prototype) = local {
                    (&*scene, prototype, path.to_owned())
                } else {
                    let (source_path, source, prototype) = palettes
                        .iter()
                        .find_map(|(path, source)| {
                            source
                                .objects
                                .iter()
                                .find(|entity| entity.name.eq_ignore_ascii_case(&name))
                                .map(|entity| (path, source, entity))
                        })
                        .with_context(|| format!("Child template resource not present: {name}"))?;
                    (source, prototype, source_path.clone())
                };
                let mut child = prototype.clone();
                child.components =
                    crate::render_support::scene::get_effective_components(prototype, &source.objects);
                let io = super::actor_io::component_index(&source.objects);
                super::actor_io::inherit_connections(&mut child.components, &io)?;
                // The instantiated fields are now fully resolved in their
                // palette's namespace. Do not resolve these master IDs again
                // against unrelated components in the destination scene.
                for component in &mut child.components {
                    component.master_link_id = ID::new(0);
                }
                ensure!(
                    !child
                        .components
                        .iter()
                        .any(|c| c.class_name == "JPSPrefabComponent"),
                    "Child prefab namespace/member instantiation is not connected: {name}"
                );
                let transform = crate::render_support::scene::get_entity_transform(&root, &scene.objects);
                set(
                    &mut child.components,
                    "JPSTransformationComponent",
                    "Translation",
                    serde_json::json!({"x":transform.translation[0],"y":transform.translation[1],"z":transform.translation[2]}),
                    scene,
                )?;
                set(
                    &mut child.components,
                    "JPSTransformationComponent",
                    "Rotation",
                    serde_json::json!({"m":transform.rotation}),
                    scene,
                )?;
                set(
                    &mut child.components,
                    "JPSTransformationComponent",
                    "Parent Entity",
                    serde_json::json!(root.link_id.to_u32()),
                    scene,
                )?;
                next = next
                    .checked_add(1)
                    .context("Runtime child identity overflow")?;
                child.link_id = ID::new(next as u128);
                child.master_link_id = ID::new(0);
                Ok((child, source_path))
            })();
            match result {
                Ok((child, source_path)) => {
                    let id = child.link_id.to_u32();
                    children.push(serde_json::json!(id));
                    report.geometry.push(serde_json::json!({"kind":"child_template_instance","scene":path,"owner_id":root.link_id.to_u32(),"child_id":id,"template":name,"source":source_path,"native_phase_timing_validated":false}));
                    let index = scene.objects.len();
                    scene.scene.push(child.link_id);
                    scene.objects.push(child);
                    let mut chain = ancestry.clone();
                    chain.push(name.to_ascii_lowercase());
                    queue.push_back((index, chain));
                }
                Err(error) => report.unsupported.push(format!(
                    "Child instantiation {path}:{} ({name}): {error:#}",
                    root.name
                )),
            }
        }
        set(
            &mut components,
            "ChildComponent",
            "Child Entities",
            serde_json::Value::Array(children),
            scene,
        )?;
        // Only the runtime child-list field changes on an existing owner.
        // Preserve its master chain: player resource fallback and prefab
        // context resolution still depend on that original provenance.
        let child_component = components
            .into_iter()
            .find(|component| component.class_name == "ChildComponent")
            .context("Expanded owner lost its ChildComponent")?;
        if let Some(existing) = scene.objects[owner]
            .components
            .iter_mut()
            .find(|component| component.class_name == "ChildComponent")
        {
            *existing = child_component;
        } else {
            scene.objects[owner].components.push(child_component);
        }
    }
    Ok(())
}
