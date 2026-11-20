//! Native SetPropertyEntity wrapper26704c: last optional index, target and
//! owner names/handles, exact property-name/type admission, zero Lua results.
use mlua::{Lua, MultiValue, Value};
impl super::assets::World {
    /// REL2662c0: wrong/missing property types return zero values; admitted
    /// entity fields return exactly one pointer or nil, including failed indices.
    ///267250 initializes count to zero, then invokes the reflected count getter.
    ///144a64 returns one for a scalar, live array size or zero for an absent pool.
    pub(super) fn property_element_count(&self, owner:usize, property:&str)->mlua::Result<usize> {
        if property=="Child Entities" {if let Some(slots)=self.live_child_slots.get(&owner) {return Ok(slots.len());}}
        for component in &self.entities[owner].components {
            if let Some(field)=component.properties.iter().find(|p|p.name==property) {
                return Ok(field.value.as_array().map_or(1,Vec::len));
            }
            if let Some(field)=crate::component_schema::CATALOG.components.get(&component.class_name)
                .and_then(|schema|schema.fields.get(property)) {
                let mut arrays=field.variants.iter().map(|v|v["value"].is_array());
                let Some(array)=arrays.next() else {return Err(mlua::Error::external("Property count has no reconstructed descriptor"));};
                if arrays.any(|v|v!=array) {return Err(mlua::Error::external("Property count descriptor has ambiguous scalar/array storage"));}
                return Ok(usize::from(!array));
            }
        }
        Ok(0)
    }
    pub(super) fn property_count_lua(&self,lua:&Lua,mut args:MultiValue)->mlua::Result<MultiValue> {
        let property=lua.coerce_string(args.pop_back().unwrap_or(Value::Nil))?;
        let owner=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(Value::Nil))?
            .filter(|i|!self.removed_entities.contains(i));
        let count=if let (Some(owner),Some(property))=(owner,property) {
            self.property_element_count(owner,property.to_str()?.split('\0').next().unwrap_or(""))?
        } else {0};
        Ok(MultiValue::from_vec(vec![Value::Number(count as f64)]))
    }
    ///311cf4 ->3120b8: unsigned bounds check followed by the Child Entities
    ///pointer getter. Null/stale entries retain their positions and yield nil.
    pub(super) fn child_index_lua(&self,lua:&Lua,mut args:MultiValue)->mlua::Result<MultiValue> {
        let index=super::score_state::integer_argument(lua,args.pop_back().unwrap_or(Value::Nil))? as u32;
        let owner_value=args.pop_back().unwrap_or(Value::Nil);
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,owner_value.clone())?
            .filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::from_vec(vec![Value::Nil]))};
        if !self.entities[owner].components.iter().any(|c|c.class_name=="ChildComponent")
            || index as usize>=self.property_element_count(owner,"Child Entities")? {
            return Ok(MultiValue::from_vec(vec![Value::Nil]));
        }
        let mut value=self.get_entity_property_lua(lua,MultiValue::from_vec(vec![owner_value,
            Value::String(lua.create_string("Child Entities")?),Value::Number(index as f64)]))?;
        Ok(MultiValue::from_vec(vec![value.pop_front().unwrap_or(Value::Nil)]))
    }
    pub(super) fn get_entity_property_lua(&self, lua:&Lua, args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=2 && args.len()!=3 {
            return Err(mlua::Error::external("GetPropertyEntity requires entity, property and optional index"));
        }
        let index=if let Some(value)=args.get(2) {
            super::score_state::integer_argument(lua,value.clone())? as u32
        } else {0};
        let Some(property)=lua.coerce_string(args[1].clone())? else {return Ok(MultiValue::new())};
        let text=property.to_str()?;let property=text.split('\0').next().unwrap_or("");
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args[0].clone())?
            .filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::new())};
        let entity=&self.entities[owner];
        for component in &entity.components {
            let field=component.properties.iter().find(|p|p.name==property);
            let declared=crate::component_schema::CATALOG.components.get(&component.class_name)
                .and_then(|schema|schema.fields.get(property));
            if field.is_none() && declared.is_none() {continue;}
            let entity_type=field.map(|p|p.class_name=="Entity Pointer").unwrap_or_else(||
                declared.unwrap().variants.iter().any(|v|v["class_name"]=="Entity Pointer"));
            if !entity_type {return Ok(MultiValue::new());}
            let live_target=if component.class_name=="ChildComponent" && property=="Child Entities" {
                self.live_child_slots.get(&owner).map(|slots|slots.get(index as usize).copied().flatten())
            } else if component.class_name=="JPSTransformationComponent" && property=="Parent Entity" {
                // The attachment cache owns the current parent, including
                // runtime children; prefab inheritance is a separate relation.
                self.transform_hierarchy.get(&owner).map(|local|Some(local.parent))
            } else if component.class_name=="RotatorComponent" && property=="RotateToEntity" {
                self.rotators.get(&(owner,component.link_id.to_u32())).and_then(|v|v.entity_target)
            } else {None};
            let target=if index!=0 && !(component.class_name=="ChildComponent" && property=="Child Entities" && self.live_child_slots.contains_key(&owner)) && field.is_none_or(|p|!p.value.is_array()) {None}
            else if let Some(target)=live_target {target}
            else {
                let reference=match field {
                    Some(field)=> {
                        let value=if let Some(array)=field.value.as_array() {array.get(index as usize)}
                            else if index==0 {Some(&field.value)} else {None};
                        value.map(|v|v.as_u64().and_then(|v|u32::try_from(v).ok())
                            .ok_or_else(||mlua::Error::external("Invalid authored entity reference"))).transpose()?.unwrap_or(0)
                    }
                    None if component.class_name=="JPSTransformationComponent" && property=="Parent Entity"=>0,
                    None=>return Err(mlua::Error::external(format!("Entity property {}.{property} default is not reconstructed",component.class_name))),
                };
                if reference==0 {None} else {
                    let mut matching=self.entities.iter().enumerate().filter_map(|(i,candidate)|
                        (candidate.scene==entity.scene && candidate.id==reference && !self.removed_entities.contains(&i)).then_some(i));
                    let target=matching.next();
                    if matching.next().is_some() {return Err(mlua::Error::external("Ambiguous authored entity reference in its source scene"));}
                    target
                }
            }.filter(|i|!self.removed_entities.contains(i) && self.scripts.has_entity(*i));
            return Ok(MultiValue::from_vec(vec![match target {
                Some(target)=>self.scripts.entity_value(target)?,None=>Value::Nil
            }]));
        }
        Ok(MultiValue::new())
    }
    pub(super) fn set_entity_property_lua(
        &mut self,
        lua: &Lua,
        mut args: MultiValue,
    ) -> mlua::Result<MultiValue> {
        if args.len() != 3 && args.len() != 4 {
            return Err(mlua::Error::external(
                "SetPropertyEntity requires entity, property, target and optional index",
            ));
        }
        let index = if args.len() == 4 {
            super::score_state::integer_argument(lua, args.pop_back().unwrap())?
        } else {
            0
        };
        let target = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap())?
            .filter(|entity| !self.removed_entities.contains(entity));
        let property = lua.coerce_string(args.pop_back().unwrap())?;
        let Some(property) = property else {
            return Ok(MultiValue::new());
        };
        let text = property.to_str()?;
        let property = text.split('\0').next().unwrap_or("");
        let Some(owner) = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap())?
            .filter(|entity| !self.removed_entities.contains(entity))
        else {
            return Ok(MultiValue::new());
        };
        for component in &self.entities[owner].components {
            let authored = component
                .properties
                .iter()
                .find(|field| field.name == property);
            let declared = crate::component_schema::CATALOG
                .components
                .get(&component.class_name)
                .and_then(|schema| schema.fields.get(property));
            if authored.is_none() && declared.is_none() {
                continue;
            }
            let entity_type = authored
                .map(|field| field.class_name == "Entity Pointer")
                .unwrap_or_else(|| {
                    declared
                        .unwrap()
                        .variants
                        .iter()
                        .any(|variant| variant["class_name"] == "Entity Pointer")
                });
            if !entity_type {
                return Ok(MultiValue::new());
            }
            if component.class_name != "RotatorComponent" || property != "RotateToEntity" {
                return Err(mlua::Error::external(format!(
                    "Native entity property {}.{property} is not connected",
                    component.class_name
                )));
            }
            if index != 0 {
                return Err(mlua::Error::external(
                    "Indexed entity property setter is not reconstructed",
                ));
            }
            let id = component.link_id.to_u32();
            let instance = self
                .rotators
                .get_mut(&(owner, id))
                .ok_or_else(|| mlua::Error::external("Entity target rotator is not initialized"))?;
            instance.entity_target = Some(target);
            return Ok(MultiValue::new());
        }
        Ok(MultiValue::new())
    }
}
