//! Scene graph operations shared by placement and duplication tools.
use anyhow::{bail, Context, Result};
use epic_mickey_lib_rs::scene_file::{Entity, ID};
use std::collections::HashMap;

/// Allocate from both namespaces: component IDs share the entity ID space.
pub fn next_id(objects: &[Entity]) -> Result<u32> {
    objects
        .iter()
        .flat_map(|e| {
            std::iter::once(e.link_id.to_u32())
                .chain(e.components.iter().map(|c| c.link_id.to_u32()))
        })
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .context("Scene ID space exhausted")
}

/// Clone a selection as a graph. Internal references follow the copies;
/// references outside the selection retain their original targets.
pub fn clone_graph(objects: &[Entity], sources: &[Entity]) -> Result<Vec<Entity>> {
    let mut next = next_id(objects)?;
    let mut ids = HashMap::new();
    for source in sources {
        for id in std::iter::once(source.link_id).chain(source.components.iter().map(|c| c.link_id))
        {
            if id.to_u32() == 0 {
                continue;
            }
            if ids.insert(id.to_u32(), next).is_some() {
                bail!("Selection contains duplicate IDs");
            }
            next = next.checked_add(1).context("Scene ID space exhausted")?;
        }
    }
    let remap = |id: ID| {
        ids.get(&id.to_u32())
            .map(|v| ID::from_u32(*v))
            .unwrap_or(id)
    };
    let names: HashMap<_, _> = sources
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                format!("{}_{}", e.name, remap(e.link_id).to_u32()),
            )
        })
        .collect();
    let mut result = sources.to_vec();
    for entity in &mut result {
        entity.link_id = remap(entity.link_id);
        entity.master_link_id = remap(entity.master_link_id);
        entity.name = format!("{}_{}", entity.name, entity.link_id.to_u32());
        for component in &mut entity.components {
            component.link_id = remap(component.link_id);
            component.master_link_id = remap(component.master_link_id);
            // Template IDs identify engine classes, not instances.
            for property in &mut component.properties {
                if named_link(&component.class_name, property) {
                    rewrite_names(&mut property.value, &names);
                }
                if property.class_name == "Entity Pointer" {
                    remap_pointer(&mut property.value, &ids);
                }
            }
        }
    }
    Ok(result)
}

fn remap_pointer(value: &mut serde_json::Value, ids: &HashMap<u32, u32>) {
    if let Some(values) = value.as_array_mut() {
        for value in values {
            remap_pointer(value, ids);
        }
    } else if let Some(id) = value.as_u64().and_then(|v| u32::try_from(v).ok()) {
        if let Some(new) = ids.get(&id) {
            *value = (*new).into();
        }
    }
}

/// Materialize component instances linked to their master definitions, matching
/// the engine's cooked scene structure even when no property is overridden.
pub fn instantiate(objects: &[Entity], master_index: usize) -> Result<Entity> {
    let master = objects
        .get(master_index)
        .context("Master no longer exists")?;
    let id = next_id(objects)?;
    let mut definitions = Vec::new();
    let component_by_id: HashMap<_, _> = objects
        .iter()
        .flat_map(|e| &e.components)
        .filter(|c| c.link_id.to_u32() != 0)
        .map(|c| (c.link_id.to_u32(), c))
        .collect();
    let mut inherited_components = std::collections::HashSet::new();
    let mut current = master;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(current.link_id.to_u32()) {
            bail!("Cyclic master inheritance");
        }
        for component in &current.components {
            if inherited_components.contains(&component.link_id.to_u32()) {
                continue;
            }
            definitions.push(component);
            // A derived component replaces its entire ancestor chain, not just
            // its immediate master. Keep unrelated components of the same class.
            let mut ancestor = component;
            let mut ancestry = std::collections::HashSet::new();
            loop {
                let id = ancestor.link_id.to_u32();
                if !ancestry.insert(id) {
                    bail!("Cyclic component inheritance");
                }
                inherited_components.insert(id);
                let parent = ancestor.master_link_id.to_u32();
                if parent == 0 {
                    break;
                }
                let Some(next) = component_by_id.get(&parent) else {
                    break;
                };
                ancestor = next;
            }
        }
        if current.master_link_id.to_u32() == 0 {
            break;
        }
        current = objects
            .iter()
            .find(|e| e.link_id == current.master_link_id)
            .context("Missing inherited master; load its palette before placing this object")?;
    }
    let mut components = Vec::new();
    for (index, definition) in definitions.into_iter().enumerate() {
        let mut component = definition.clone();
        component.master_link_id = definition.link_id;
        component.link_id = ID::from_u32(
            id.checked_add(index as u32 + 1)
                .context("Scene ID space exhausted")?,
        );
        component.properties.clear();
        components.push(component);
    }
    Ok(Entity::new(
        master.class_name.clone(),
        format!("{}_{}", master.name, id),
        ID::from_u32(id),
        master.link_id,
        master.unknown,
        master.unknown_em2,
        components,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::scene_file::{Component, Property};
    fn entity(id: u32) -> Entity {
        Entity::new(
            "JPSGeneralEntity".into(),
            "Object".into(),
            ID::from_u32(id),
            ID::new(0),
            0,
            0,
            vec![Component::new(
                "JPSTransformationComponent".into(),
                "Transformation".into(),
                ID::from_u32(4),
                ID::from_u32(id + 100),
                ID::new(0),
                vec![Property::new(
                    "Entity Pointer".into(),
                    "Target".into(),
                    false,
                    false,
                    false,
                    2.into(),
                )],
            )],
        )
    }
    #[test]
    fn copies_internal_links_and_preserves_class_ids() {
        let objects = vec![entity(1), entity(2)];
        let copied = clone_graph(&objects, &objects).unwrap();
        assert_eq!(copied[0].link_id.to_u32(), 103);
        assert_eq!(
            copied[0].components[0].properties[0].value,
            serde_json::json!(copied[1].link_id.to_u32())
        );
        assert_eq!(copied[0].components[0].template_id.to_u32(), 4);
        let single = clone_graph(&objects, &objects[..1]).unwrap();
        assert_eq!(
            single[0].components[0].properties[0].value,
            serde_json::json!(2)
        );
    }
    #[test]
    fn named_event_links_follow_copies_without_changing_unrelated_strings() {
        let mut objects = vec![entity(1), entity(2)];
        objects[0].name = "Sender".into();
        objects[1].name = "Receiver".into();
        objects[0].components[0].class_name = "InputOutputComponent".into();
        objects[0].components[0].properties = vec![
            Property::new(
                "String".into(),
                "Target".into(),
                false,
                false,
                false,
                serde_json::json!(["Receiver", "Outside"]),
            ),
            Property::new(
                "String".into(),
                "Parameters".into(),
                false,
                false,
                false,
                serde_json::json!(["Receiver"]),
            ),
        ];
        let copies = clone_graph(&objects, &objects).unwrap();
        assert_eq!(
            copies[0].components[0].properties[0].value,
            serde_json::json!([copies[1].name, "Outside"])
        );
        assert_eq!(
            copies[0].components[0].properties[1].value,
            serde_json::json!(["Receiver"])
        );
    }
    #[test]
    fn instance_uses_master_transform_and_unused_ids() {
        let objects = vec![entity(1)];
        let instance = instantiate(&objects, 0).unwrap();
        assert_eq!(instance.link_id.to_u32(), 102);
        assert_eq!(instance.components[0].link_id.to_u32(), 103);
        assert_eq!(instance.components[0].master_link_id.to_u32(), 101);
        assert_eq!(instance.components[0].template_id.to_u32(), 4);
    }

    #[test]
    fn deep_prefabs_keep_only_most_derived_controller_and_preserve_distinct_components() {
        let mut base = entity(1);
        base.components[0].class_name = "BasicAIControllerComponent".into();
        let mut middle = entity(2);
        middle.master_link_id = base.link_id;
        middle.components[0].class_name = "BasicAIControllerComponent".into();
        middle.components[0].master_link_id = base.components[0].link_id;
        let mut derived = entity(3);
        derived.master_link_id = middle.link_id;
        derived.components[0].class_name = "BasicAIControllerComponent".into();
        derived.components[0].master_link_id = middle.components[0].link_id;
        let mut separate = base.components[0].clone();
        separate.link_id = ID::from_u32(104);
        separate.name = "Separate controller".into();
        base.components.push(separate);
        let objects = vec![base, middle, derived];
        let instance = instantiate(&objects, 2).unwrap();
        assert_eq!(instance.components.len(), 2);
        assert_eq!(instance.components[0].master_link_id.to_u32(), 103);
        assert_eq!(instance.components[1].master_link_id.to_u32(), 104);
        assert!(instance.components.iter().all(|c| c.properties.is_empty()));
        let mut cyclic = objects;
        cyclic[0].components[0].master_link_id = ID::from_u32(103);
        assert!(instantiate(&cyclic, 2)
            .unwrap_err()
            .to_string()
            .contains("Cyclic component"));
    }
}

pub(crate) fn rewrite_names(v: &mut serde_json::Value, map: &HashMap<String, String>) {
    if let Some(a) = v.as_array_mut() {
        for v in a {
            rewrite_names(v, map)
        }
    } else if let Some(s) = v.as_str() {
        if let Some(new) = map.get(s) {
            *v = new.clone().into();
        }
    }
}
pub(crate) fn named_link(c: &str, p: &epic_mickey_lib_rs::scene_file::Property) -> bool {
    p.template || (c == "InputOutputComponent" && p.name == "Target")
}
