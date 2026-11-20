//! Observed EM1 component schemas. Omitted inherited fields remain omitted on disk.
use epic_mickey_lib_rs::scene_file::{Component, Property, SceneFile, ID};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};
#[derive(Default, Serialize, Deserialize)]
pub struct Catalog {
    pub archives: usize,
    pub scene_files: usize,
    pub unique_scenes: usize,
    pub components: BTreeMap<String, Definition>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub instances: usize,
    pub template_id: u32,
    pub template_ids: BTreeSet<u32>,
    pub fields: BTreeMap<String, Field>,
    pub source: String,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Field {
    pub variants: Vec<Value>,
    pub examples: usize,
    #[serde(default)]
    pub extensions: BTreeSet<String>,
}
pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/components.json")).expect("embedded component catalog")
});
pub fn friendly(class: &str) -> String {
    match class {
        "JPSSceneGraphComponent" => return "Model".into(),
        "AnimationComponent" => return "Animated model".into(),
        "AnimationComponentGB" => return "Gamebryo animation".into(),
        "PhysicsRigidBodyDynamicComponent" => return "Dynamic rigid body".into(),
        "PhysicsRigidBodyCharacterComponent" => return "Character collision".into(),
        "PhysicsPhantomComponent" => return "Trigger volume".into(),
        "InputOutputComponent" => return "Events and connections".into(),
        _ => {}
    }
    let s = class
        .trim_start_matches("JPS")
        .trim_start_matches("JPS")
        .trim_end_matches("Component");
    let mut out = String::new();
    let chars: Vec<_> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0
            && c.is_uppercase()
            && (chars[i - 1].is_lowercase()
                || chars.get(i + 1).is_some_and(|n| n.is_lowercase())
                    && chars[i - 1].is_uppercase())
        {
            out.push(' ')
        }
        out.push(*c)
    }
    out
}
impl Definition {
    pub fn instantiate(&self, class: &str, scene: &SceneFile) -> anyhow::Result<Component> {
        let id = scene
            .objects
            .iter()
            .flat_map(|e| {
                std::iter::once(e.link_id.to_u32())
                    .chain(e.components.iter().map(|c| c.link_id.to_u32()))
            })
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Scene ID space exhausted"))?;
        let mut properties: Vec<Property> = self
            .fields
            .values()
            .filter_map(|f| f.variants.first())
            .map(|v| observed_property(v))
            .collect();
        if class == "JPSTransformationComponent" {
            for p in &mut properties {
                match p.name.as_str() {
                    "Translation" | "Local Translation" => {
                        p.value = serde_json::json!({"x":0.,"y":0.,"z":0.})
                    }
                    "Rotation" | "Local Rotation" => {
                        p.value = serde_json::json!({"m":[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]})
                    }
                    "Scale" | "Local Scale" => p.value = serde_json::json!(1.),
                    _ => {}
                }
            }
        }
        Ok(Component::new(
            class.into(),
            self.name.clone(),
            ID::from_u32(self.template_id),
            ID::from_u32(id),
            ID::new(0),
            properties,
        ))
    }
}
/// Numeric fields on disk use IEEE single precision.
pub fn observed_property(v: &Value) -> Property {
    fn normalize(v: &mut Value) {
        match v {
            Value::Array(a) => {
                for v in a {
                    normalize(v)
                }
            }
            Value::Object(o) => {
                for v in o.values_mut() {
                    normalize(v)
                }
            }
            Value::Number(n) if n.is_f64() => {
                if let Some(f) = n.as_f64() {
                    *v = serde_json::json!((f as f32) as f64);
                }
            }
            _ => {}
        }
    }
    let mut p = Property::from_dict(v);
    normalize(&mut p.value);
    p
}
pub fn issues(scene: &SceneFile) -> Vec<String> {
    let mut result = vec![];
    let mut ids = BTreeSet::new();
    for e in &scene.objects {
        for id in std::iter::once(e.link_id.to_u32())
            .chain(e.components.iter().map(|c| c.link_id.to_u32()))
        {
            if id != 0 && !ids.insert(id) {
                result.push(format!("{}: duplicate object ID {id}", e.name));
            }
        }
        for c in &e.components {
            let Some(def) = CATALOG.components.get(&c.class_name) else {
                result.push(format!("{}: unknown component {}", e.name, c.class_name));
                continue;
            };
            // Retail scenes sometimes repeat identical inherited field records.
            // Preserve them; only conflicting repetitions are an authoring error.
            let mut names: BTreeMap<&String, &Property> = BTreeMap::new();
            for p in &c.properties {
                if names.insert(&p.name, p).is_some_and(|old| {
                    old.class_name != p.class_name
                        || old.asset != p.asset
                        || old.palette != p.palette
                        || old.template != p.template
                        || old.value != p.value
                }) {
                    result.push(format!(
                        "{} / {}: duplicate property {}",
                        e.name,
                        friendly(&c.class_name),
                        p.name
                    ));
                }
                if let Some(field) = def.fields.get(&p.name) {
                    if !field.variants.iter().any(|v| {
                        v["class_name"] == p.class_name
                            && v["asset"] == p.asset
                            && v["palette"] == p.palette
                            && v["template"] == p.template
                            && v["value"].is_array() == p.value.is_array()
                    }) {
                        result.push(format!(
                            "{} / {} / {}: storage does not match any observed schema",
                            e.name,
                            friendly(&c.class_name),
                            p.name
                        ));
                    }
                } else {
                    result.push(format!(
                        "{} / {}: unknown property {}",
                        e.name,
                        friendly(&c.class_name),
                        p.name
                    ));
                }
                let values: Vec<_> = if let Some(a) = p.value.as_array() {
                    a.iter().collect()
                } else {
                    vec![&p.value]
                };
                for value in values {
                    if !valid_value(&p.class_name, value) {
                        result.push(format!(
                            "{} / {} / {}: invalid {} value",
                            e.name,
                            friendly(&c.class_name),
                            p.name,
                            p.class_name
                        ));
                        break;
                    }
                }
            }
        }
    }
    result
}
fn valid_value(class: &str, v: &Value) -> bool {
    let finite = |v: &Value| {
        v.as_f64()
            .is_some_and(|n| n.is_finite() && (n as f32).is_finite())
    };
    match class {
        "Boolean" => v.is_boolean(),
        "Float" => finite(v),
        "Integer" => v.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        "Unsigned Integer" | "Entity Pointer" => {
            v.as_u64().is_some_and(|n| u32::try_from(n).is_ok())
        }
        "Short" => v.as_i64().is_some_and(|n| i16::try_from(n).is_ok()),
        "Unsigned Short" => v.as_u64().is_some_and(|n| u16::try_from(n).is_ok()),
        "Point2" => ["x", "y"].iter().all(|k| finite(&v[k])),
        "Point3" => ["x", "y", "z"].iter().all(|k| finite(&v[k])),
        "Color (RGB)" => ["r", "g", "b"].iter().all(|k| finite(&v[k])),
        "Color (RGBA)" => ["r", "g", "b", "a"].iter().all(|k| finite(&v[k])),
        "Matrix3" => (0..3).all(|i| (0..3).all(|j| finite(&v["m"][i][j]))),
        "String" => v.is_string(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::{
        file_manipulator::EndianType,
        scene_file::{Entity, SceneFileVersion},
    };
    #[test]
    fn all_observed_components_create_and_roundtrip_without_storage_errors() {
        assert!(CATALOG.archives >= 379);
        assert!(CATALOG.unique_scenes >= 870);
        assert_eq!(CATALOG.components.len(), 87);
        let mut scene = SceneFile::new(
            vec![Entity::new(
                "JPSGeneralEntity".into(),
                "test".into(),
                ID::from_u32(1),
                ID::new(0),
                0,
                0,
                vec![],
            )],
            vec![ID::from_u32(1)],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        for (class, def) in &CATALOG.components {
            let component = def.instantiate(class, &scene).unwrap();
            scene.objects[0].components.push(component);
        }
        assert!(issues(&scene).is_empty(), "{:?}", issues(&scene));
        let read = SceneFile::from_binary(&scene.to_binary(EndianType::BIG), EndianType::BIG);
        assert_eq!(read.objects[0].components.len(), 87);
        assert!(issues(&read).is_empty(), "{:?}", issues(&read));
        for (before, after) in scene.objects[0]
            .components
            .iter()
            .zip(&read.objects[0].components)
        {
            assert_eq!(before.template_id.to_u32(), after.template_id.to_u32());
            assert_eq!(
                before
                    .properties
                    .iter()
                    .map(|p| p.to_dict())
                    .collect::<Vec<_>>(),
                after
                    .properties
                    .iter()
                    .map(|p| p.to_dict())
                    .collect::<Vec<_>>()
            );
        }
        let c = &mut scene.objects[0].components[0];
        c.properties[0].class_name = "Unrecognized".into();
        assert!(!issues(&scene).is_empty());
    }
    #[test]
    fn inherited_fields_may_be_absent_but_unknown_fields_and_wrong_values_are_reported() {
        let mut scene = SceneFile::new(
            vec![Entity::new(
                "JPSGeneralEntity".into(),
                "test".into(),
                ID::from_u32(1),
                ID::new(0),
                0,
                0,
                vec![],
            )],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut c = CATALOG.components["HealthComponent"]
            .instantiate("HealthComponent", &scene)
            .unwrap();
        c.properties.clear();
        scene.objects[0].components.push(c);
        assert!(issues(&scene).is_empty());
        scene.objects[0].components[0]
            .properties
            .push(Property::new(
                "Float".into(),
                "Health".into(),
                false,
                false,
                false,
                "invalid".into(),
            ));
        assert!(!issues(&scene).is_empty());
    }
}

/// Display terminology never changes serialized property names.
pub fn field_label(name: &str) -> String {
    match name {
        "Translation" => "Position".into(),
        "Local Translation" => "Local position".into(),
        "NIF File Path" => "Model asset".into(),
        "RB Hull File Path" => "Collision asset".into(),
        "Behavior File Path" => "Animation behavior".into(),
        "KFM File Path" => "Animation manager".into(),
        "NextStopEntity" => "Next spline knot".into(),
        "DisallowRotation" => "Prevent rotation".into(),
        _ => friendly(name),
    }
}
