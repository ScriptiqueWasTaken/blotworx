//! Lua265c68: String type admission, getter dispatch and one-result semantics.
//! Custom/omitted getter adapters remain explicit, rather than reading a stale
//! editor field as if every native component stored its strings identically.
use super::assets::World;
use mlua::{Lua, MultiValue, Value};

impl World {
    pub(super) fn get_script_string_property(
        &self,
        lua: &Lua,
        args: MultiValue,
    ) -> mlua::Result<MultiValue> {
        if args.len() != 2 && args.len() != 3 {
            return Err(mlua::Error::external("GetPropertyString requires entity, property and optional index; native count diagnostics are not reconstructed"));
        }
        // Native consumes index, property name, then entity from the Lua top.
        let index = if let Some(value) = args.get(2) {
            let value = lua.coerce_number(value.clone())?.unwrap_or(0.).trunc();
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(mlua::Error::external(
                    "String property index requires native fctiwz validation for this value",
                ));
            }
            value as i32 as u32
        } else {
            0
        };
        let property = lua.coerce_string(args[1].clone())?;
        let Some(property) = property else {
            return Ok(MultiValue::new());
        };
        let property = property.to_str()?;
        let property = property.split('\0').next().unwrap_or("");
        let Some(owner) = self
            .scripts
            .resolve_native_entity_argument(lua, args[0].clone())?
        else {
            return Ok(MultiValue::new());
        };
        if self.removed_entities.contains(&owner) {
            return Ok(MultiValue::new());
        }
        let entity = self
            .entities
            .get(owner)
            .ok_or_else(|| mlua::Error::external("String getter owner is missing"))?;
        let first = entity.components.iter().find_map(|component| {
            component
                .properties
                .iter()
                .find(|field| field.name == property)
                .map(|field| (component, field))
        });
        let Some((component, field)) = first else {
            if entity.components.iter().any(|component| {
                crate::component_schema::CATALOG
                    .components
                    .get(&component.class_name)
                    .is_some_and(|definition| definition.fields.contains_key(property))
            }) {
                return Err(mlua::Error::external(format!("Native string getter/default for omitted field {property} is not reconstructed")));
            }
            return Ok(MultiValue::new());
        };
        if field.class_name != "String" {
            return Ok(MultiValue::new());
        }
        if field.value.is_array() {
            if component.class_name == "InventoryComponent" && property == "Sketch" {
                // Inventory's registered Sketch getter+288 reads one indexed
                // interned string. A failed read leaves the wrapper's NULL.
                let value = field.value.get(index as usize).and_then(|v|v.as_str());
                return Ok(MultiValue::from_vec(vec![match value {
                    Some(value) => Value::String(lua.create_string(value.split('\0').next().unwrap_or(""))?),
                    None => Value::Nil,
                }]));
            }
            return Err(mlua::Error::external(format!(
                "Native string array getter for {}.{property} is not connected",
                component.class_name
            )));
        }
        //131988 declines a scalar's nonzero index.265de0 ignores that
        //boolean. DOL80696390 is NULL;2784ac ->3f688c pushes ONE nil.
        if index != 0 {
            return Ok(MultiValue::from_vec(vec![Value::Nil]));
        }
        let value = match (component.class_name.as_str(), property) {
            ("AIBrainComponent", "AIType") => self
                .ai_brains
                .get(&owner)
                .ok_or_else(|| mlua::Error::external("AIType getter has no initialized AI brain"))?
                .ai_type
                .as_str(),
            ("TemplateSpawnerComponent", "Spawn Group") | ("JPSTransformationComponent", "Bone Attach Name") => field
                .value
                .as_str()
                .ok_or_else(|| mlua::Error::external("Spawn Group is not a scalar string"))?,
            _ => {
                return Err(mlua::Error::external(format!(
                    "Native string getter for {}.{property} is not connected",
                    component.class_name
                )))
            }
        };
        Ok(MultiValue::from_vec(vec![Value::String(
            lua.create_string(value.split('\0').next().unwrap_or(""))?,
        )]))
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_string_getter_preserves_native_admission_and_result_count() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let (&owner, brain) = world
            .ai_brains
            .iter()
            .find(|(_, brain)| brain.ai_type == "Gremlin:Variant4")
            .context("Original Gremlin AI type missing")?;
        let name = world.entities[owner].name.clone();
        let expected = brain.ai_type.clone();
        world.run_callback_fixture(&format!(
            r#"
            local e=GetEntity({name:?})
            assert(select('#',GetPropertyString(e,'AIType'))==1)
            assert(GetPropertyString(e,'AIType')=={expected:?})
            assert(GetPropertyString({name:?},'AIType',0)=={expected:?})
            assert(GetPropertyString(e,'AIType','0.9')=={expected:?})
            assert(GetPropertyString(e,'AIType',1)==nil)
            assert(select('#',GetPropertyString(e,'AIType',-1))==1)
            assert(GetPropertyString(e,'AIType',-1)==nil)
            assert(GetPropertyString(e,'AIType'..string.char(0)..'ignored')=={expected:?})
            assert(select('#',GetPropertyString(e,'Collision Layer'))==0)
            assert(select('#',GetPropertyString(e,'BwxMissingString'))==0)
            assert(select('#',GetPropertyString('BwxMissingEntity','AIType'))==0)
            assert(select('#',GetPropertyString(false,'AIType'))==0)
        "#
        ))?;
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/string_getter.json"
        ))?;
        for case in reference["cases"].as_array().unwrap() {
            let entity = if case["entity"].as_bool().unwrap() {
                name.as_str()
            } else {
                "BwxMissingEntity"
            };
            let property = if !case["found"].as_bool().unwrap() {
                "BwxMissingString"
            } else if case["string"].as_bool().unwrap() {
                "AIType"
            } else {
                "Collision Layer"
            };
            let index = case["index"].as_u64().unwrap() as u32 as i32;
            let count = case["result"]["returns"].as_u64().unwrap();
            let value = case["result"]["output"].get(0).and_then(|v| v.as_str());
            let check = if count == 1 {
                format!(
                    "assert(f()=={})",
                    value
                        .map(|v| format!("{v:?}"))
                        .unwrap_or_else(|| "nil".into())
                )
            } else {
                String::new()
            };
            world.run_callback_fixture(&format!("local function f() return GetPropertyString({entity:?},{property:?},{index}) end; assert(select('#',f())=={count}); {check}"))?;
        }
        // Unsupported real getters must remain visible, not silently return
        // editor metadata. A supported property shadowed by an earlier wrong
        // type also must not fall through to a later matching component.
        let component = world.entities[owner]
            .components
            .iter()
            .position(|c| c.class_name == "AIBrainComponent")
            .unwrap();
        let property = world.entities[owner].components[component]
            .properties
            .iter_mut()
            .find(|p| p.name == "AIType")
            .unwrap();
        property.class_name = "Integer".into();
        world.run_callback_fixture(&format!(
            "assert(select('#',GetPropertyString(GetEntity({name:?}),'AIType'))==0)"
        ))?;
        Ok(())
    }
}
