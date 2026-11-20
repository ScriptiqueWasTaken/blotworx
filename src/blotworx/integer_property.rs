//! EM1 REL265970: integer property admission and signed Lua result conversion.
use super::assets::World;
use mlua::{Lua, MultiValue, Value};

impl World {
    pub(super) fn entity_liveness_lua(&self, lua:&Lua, name:&str, args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=1 {return Err(mlua::Error::external(format!("{name} requires one entity")));}
        let owner=self.scripts.resolve_native_entity_argument(lua,args[0].clone())?
            .filter(|owner|!self.removed_entities.contains(owner));
        let result=if name=="IsCharacter" {
            owner.is_some_and(|owner|super::entity_component_lookup::has_character_component(
                self.entities[owner].components.iter().map(|component|component.class_name.as_str())))
        } else if name=="IsPlayer" {
            let current=self.scripts.current_player_entity().map_err(mlua::Error::external)?;
            super::entity_liveness::player(owner,current)
        } else if let Some(owner)=owner {
            let health=if self.entities[owner].components.iter().any(|c|c.class_name=="HealthComponent") {
                let authored=self.authored_health.get(&owner).ok_or_else(||mlua::Error::external("IsAlive has no initialized HealthComponent fields"))?;
                let state=self.runtime_health.get(&owner).ok_or_else(||mlua::Error::external("IsAlive has no initialized health runtime"))?;
                Some(super::health::alive(state.health,u8::from(authored.infinite)))
            } else {None};
            let counter=self.paint_counters.get(&owner);
            if counter.is_none() && self.entities[owner].components.iter().any(|c|c.class_name=="PaintThinnerCounterComponent") {
                return Err(mlua::Error::external("IsAlive has no initialized paint/thinner counter"));
            }
            //269ec0 passes require-component=true. Health success deliberately
            //skips status evaluation, just like298a30; status2 alone is thinned.
            super::entity_liveness::alive(true,true,health,counter.is_some(),||counter.unwrap().status())
        } else {false};
        Ok(MultiValue::from_vec(vec![Value::Boolean(result)]))
    }

    pub(super) fn set_script_health(&mut self, owner: usize, value: i32) -> mlua::Result<()> {
        let Some(component) = self.entities[owner].components.iter().position(|c|c.class_name=="HealthComponent") else {
            return Ok(());
        };
        let fields=&self.entities[owner].components[component].properties;
        let maximum=fields.iter().find(|p|p.name=="MaxHealth").and_then(|p|p.value.as_i64())
            .and_then(|v|i32::try_from(v).ok()).ok_or_else(||mlua::Error::external("Health setter has no resolved MaxHealth"))?;
        let field=fields.iter().position(|p|p.name=="Health").ok_or_else(||mlua::Error::external("Health setter has no resolved Health"))?;
        let state=self.runtime_health.get_mut(&owner).ok_or_else(||mlua::Error::external("Health setter has no initialized runtime"))?;
        if state.set_health(value,maximum) {
            self.entities[owner].components[component].properties[field].value=value.into();
        }
        Ok(())
    }
    /// REL322618/322778 address the Health integer property, sharing its setter
    /// with generic property access. They do not synthesize damage stimuli.
    pub(super) fn health_lua(&mut self, lua:&Lua, name:&str, args:MultiValue)->mlua::Result<MultiValue> {
        let setter=name=="SetHealth";
        let required=if setter {2} else {1};
        if args.len()<required {return Err(mlua::Error::external("Health API requires an entity and, for SetHealth, an integer"));}
        let target=args.len()-required;
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args[target].clone())?
            .filter(|i|!self.removed_entities.contains(i)) else {
                if setter {return Ok(MultiValue::new());}
                return Err(mlua::Error::external("GetHealth requires a live entity; native invalid-target behavior is not safely reconstructed"));
            };
        if setter {
            let value=super::score_state::integer_argument(lua,args[target+1].clone())?;
            self.set_script_health(owner,value)?;
            Ok(MultiValue::new())
        } else {
            let state=self.runtime_health.get(&owner).ok_or_else(||mlua::Error::external("GetHealth requires an initialized HealthComponent"))?;
            Ok(MultiValue::from_vec(vec![Value::Number(state.health as f64)]))
        }
    }
    pub(super) fn get_script_integer_property(
        &self,
        lua: &Lua,
        args: MultiValue,
    ) -> mlua::Result<MultiValue> {
        if args.len() != 2 && args.len() != 3 {
            return Err(mlua::Error::external(
                "GetPropertyInt requires entity, property and optional index",
            ));
        }
        let index = if let Some(value) = args.get(2) {
            let value = lua.coerce_number(value.clone())?.unwrap_or(0.).trunc();
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(mlua::Error::external(
                    "Integer property index exceeds native conversion range",
                ));
            }
            value as i32 as u32
        } else {
            0
        };
        let Some(property) = lua.coerce_string(args[1].clone())? else {
            return Ok(MultiValue::new());
        };
        let property = property.to_str()?;
        let property = property.split('\0').next().unwrap_or("");
        let Some(owner) = self
            .scripts
            .resolve_native_entity_argument(lua, args[0].clone())?
            .filter(|owner| !self.removed_entities.contains(owner))
        else {
            return Ok(MultiValue::new());
        };
        let entity = &self.entities[owner];
        let Some((component, field)) = entity.components.iter().find_map(|component| {
            component
                .properties
                .iter()
                .find(|field| field.name == property)
                .map(|field| (component, field))
        }) else {
            return Ok(MultiValue::new());
        };
        if !matches!(
            field.class_name.as_str(),
            "Integer" | "Unsigned Integer" | "Short" | "Unsigned Short"
        ) {
            return Ok(MultiValue::new());
        }
        // These components retain reflected integer storage; Health itself is
        // updated by the damage system and must come from its live runtime.
        let collision_layer = property == "Collision Layer"
            && matches!(
                component.class_name.as_str(),
                "PhysicsRigidBodyCharacterComponent" | "PhysicsRigidBodyStaticComponent"
            );
        if !collision_layer
            && !matches!(
                component.class_name.as_str(),
                "HealthComponent" | "InventoryComponent" | "TemplateSpawnerComponent"
            )
        {
            return Err(mlua::Error::external(format!(
                "Native integer getter for {}.{property} is not connected",
                component.class_name
            )));
        }
        let value = if property == "Health"
            && component.class_name == "HealthComponent"
            && index == 0
        {
            self.runtime_health
                .get(&owner)
                .ok_or_else(|| mlua::Error::external("Health getter has no initialized runtime"))?
                .health as i64
        } else {
            let value = if let Some(array) = field.value.as_array() {
                array.get(index as usize)
            } else if index == 0 {
                Some(&field.value)
            } else {
                None
            };
            value.and_then(serde_json::Value::as_i64).ok_or_else(|| mlua::Error::external(format!("{property} has no integer at index {index}; native failed-getter output is unverified")))?
        };
        let value = match field.class_name.as_str() {
            "Short" => value as i16 as i32,
            "Unsigned Short" => value as u16 as i32,
            _ => value as u32 as i32,
        };
        Ok(MultiValue::from_vec(vec![Value::Number(value as f64)]))
    }
}
