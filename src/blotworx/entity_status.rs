//! StatusComponent.Invisible is a stored status property, not model hiding or
//! VAR_In_Conversation. Retail presentation12fcc selects this component via
//!13068/13088 and writes its bool property through13110/13138.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Component, Property};

fn component(components: &[Component]) -> Result<Option<usize>> {
    let mut matching = components
        .iter()
        .enumerate()
        .filter(|(_, c)| c.class_name == "StatusComponent");
    let index = matching.next().map(|(i, _)| i);
    ensure!(
        matching.next().is_none(),
        "Multiple Status components require native component-slot arbitration"
    );
    Ok(index)
}
fn invisible(component: &Component) -> Result<Option<usize>> {
    let mut matching = component
        .properties
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == "Invisible");
    let index = matching.next().map(|(i, _)| i);
    ensure!(
        matching.next().is_none(),
        "Duplicate Invisible status property"
    );
    if let Some(i) = index {
        ensure!(
            component.properties[i].class_name == "Boolean",
            "Invisible status property is not Boolean"
        );
    }
    Ok(index)
}
/// Generated descriptor3c5218..3c5284 names Invisible and initializes its
/// default byte14 to zero. Absent authored storage therefore reads false.
pub fn read(components: &[Component]) -> Result<Option<bool>> {
    let Some(i) = component(components)? else {
        return Ok(None);
    };
    let c = &components[i];
    match invisible(c)? {
        Some(p) => Ok(Some(
            c.properties[p]
                .value
                .as_bool()
                .context("Invisible status value is not a boolean")?,
        )),
        None => Ok(Some(false)),
    }
}
/// Allocate runtime backing on first write, as145c40/144488 does for a
/// declared property with no authored value. Original asset bytes stay intact.
pub fn write(components: &mut [Component], value: bool) -> Result<bool> {
    let Some(i) = component(components)? else {
        return Ok(false);
    };
    let c = &mut components[i];
    match invisible(c)? {
        Some(p) => {
            ensure!(
                c.properties[p].value.is_boolean(),
                "Invisible status value is not a boolean"
            );
            c.properties[p].value = value.into();
        }
        None => c.properties.push(Property::new(
            "Boolean".into(),
            "Invisible".into(),
            false,
            false,
            false,
            value.into(),
        )),
    }
    Ok(true)
}
impl super::assets::World {
    /// Retail Lua265798/2666bc. Unsupported reflected owners fail explicitly.
    pub(super) fn bool_property_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        use mlua::{MultiValue, Value};
        let setter = name == "SetPropertyBool";
        let required = if setter { 3 } else { 2 };
        if args.len() != required && args.len() != required + 1 {
            return Err(mlua::Error::external(format!("{name} requires entity, property{} and optional index; native count diagnostics are not reconstructed",if setter {", value"}else{""})));
        }
        let index = if args.len() > required {
            super::score_state::integer_argument(lua, args.pop_back().unwrap())?
        } else {
            0
        };
        let value = if setter {
            Some(super::actor_scripts::native_boolean(
                lua,
                args.pop_back().unwrap(),
            )?)
        } else {
            None
        };
        let property = lua.coerce_string(args.pop_back().unwrap())?;
        let Some(property) = property else {
            return Ok(MultiValue::new());
        };
        let property = property.to_str()?;
        let property = property.split('\0').next().unwrap_or("");
        let Some(owner) = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap())?
            .filter(|owner| !self.removed_entities.contains(owner))
        else {
            return Ok(MultiValue::new());
        };
        if property == "RelayDisabled" {
            let Some(disabled)=self.relay_disabled(owner).map_err(mlua::Error::external)? else {return Ok(MultiValue::new())};
            if let Some(value)=value {
                if index==0 {self.set_relay_disabled(owner,value).map_err(mlua::Error::external)?;}
                return Ok(MultiValue::new());
            }
            if index!=0 {return Err(mlua::Error::external("Indexed relay Boolean getter output is unverified"));}
            return Ok(MultiValue::from_vec(vec![Value::Boolean(disabled)]));
        }
        if self.demo_story.enabled && property == "Is Mover" {
            if let Some(c) = self.entities[owner]
                .components
                .iter()
                .position(|c| c.class_name == "PhysicsRigidBodyCharacterComponent")
            {
                let field = self.entities[owner].components[c]
                    .properties
                    .iter()
                    .position(|p| p.name == property)
                    .ok_or_else(|| {
                        mlua::Error::external("Character Is Mover default is unresolved")
                    })?;
                if let Some(value) = value {
                    if index == 0 {
                        if self
                            .character_physics
                            .get(&owner)
                            .is_some_and(|body| body.is_ok())
                        {
                            self.suspend_character_motion(owner, value)
                                .map_err(mlua::Error::external)?;
                        }
                        self.entities[owner].components[c].properties[field].value = value.into();
                    }
                    return Ok(MultiValue::new());
                }
                let value = self.entities[owner].components[c].properties[field]
                    .value
                    .as_bool()
                    .ok_or_else(|| mlua::Error::external("Character Is Mover must be Boolean"))?;
                return Ok(MultiValue::from_vec(vec![Value::Boolean(value)]));
            }
        }
        //Native type admission precedes getter/setter invocation. Do not fall
        //through a wrong type, or ignore a declared but omitted earlier field.
        let mut supported = false;
        for c in &mut self.entities[owner].components {
            let field = c.properties.iter().position(|p| p.name == property);
            if field.is_some_and(|i| c.properties[i].class_name != "Boolean") {
                return Ok(MultiValue::new());
            }
            if property == "ForceInvulnerable"
                && matches!(c.class_name.as_str(), "HealthComponent" | "PaintThinnerCounterComponent")
            {
                // Both damage paths read this live property when admitting a hit.
                if let Some(value) = value {
                    if index == 0 {
                        if let Some(i) = field {
                            c.properties[i].value = value.into();
                        } else {
                            c.properties.push(Property::new(
                                "Boolean".into(), property.into(), false, false, false, value.into(),
                            ));
                        }
                    }
                    return Ok(MultiValue::new());
                }
                if index != 0 {
                    return Err(mlua::Error::external("Indexed boolean getter output is not verified"));
                }
                let forced = field.map(|i| c.properties[i].value.as_bool()
                    .ok_or_else(|| mlua::Error::external("ForceInvulnerable is not Boolean")))
                    .transpose()?.unwrap_or(false);
                return Ok(MultiValue::from_vec(vec![Value::Boolean(forced)]));
            }
            if let Some(field) = field {
                if property == "InvulnerableToPaintThinner"
                    && matches!(c.class_name.as_str(),
                        "PhysicsRigidBodyStaticComponent" | "PhysicsRigidBodyDynamicComponent")
                {
                    if let Some(value) = value {
                        if index == 0 {
                            c.properties[field].value = value.into();
                            if let Some(paint) = self.model_paint.get_mut(&owner) {
                                paint.invulnerable = value;
                            }
                        }
                        return Ok(MultiValue::new());
                    }
                    if index != 0 {
                        return Err(mlua::Error::external("Indexed boolean getter output is not verified"));
                    }
                    let invulnerable = c.properties[field].value.as_bool().ok_or_else(|| {
                        mlua::Error::external("Physics InvulnerableToPaintThinner must be Boolean")
                    })?;
                    return Ok(MultiValue::from_vec(vec![Value::Boolean(invulnerable)]));
                }
                if !setter && property == "Is Painted"
                    && matches!(c.class_name.as_str(),
                        "PhysicsRigidBodyStaticComponent" | "PhysicsRigidBodyDynamicComponent")
                {
                    if index != 0 {
                        return Err(mlua::Error::external("Indexed boolean getter output is not verified"));
                    }
                    // Model paint transitions update this same live property.
                    let painted = c.properties[field].value.as_bool()
                        .ok_or_else(|| mlua::Error::external("Physics Is Painted must be Boolean"))?;
                    return Ok(MultiValue::from_vec(vec![Value::Boolean(painted)]));
                }
            }
            if field.is_some() {
                if (c.class_name == "StatusComponent" && property == "Invisible")
                    || (c.class_name == "AnimationComponent" && property == "IgnoreAudioEvents")
                {
                    supported = true;
                    break;
                }
                return Err(mlua::Error::external(format!(
                    "Native boolean property {}.{property} is not connected",
                    c.class_name
                )));
            }
            if c.class_name == "StatusComponent" && property == "Invisible" {
                supported = true;
                break;
            }
            if c.class_name == "AnimationComponent" && property == "IgnoreAudioEvents" {
                supported = true;
                break;
            }
            if let Some(field) = crate::component_schema::CATALOG
                .components
                .get(&c.class_name)
                .and_then(|c| c.fields.get(property))
            {
                if field.variants.iter().all(|v| v["class_name"] != "Boolean") {
                    return Ok(MultiValue::new());
                }
                return Err(mlua::Error::external(format!(
                    "Native omitted boolean property {}.{property} is not connected",
                    c.class_name
                )));
            }
        }
        if !supported {
            return Ok(MultiValue::new());
        }
        if let Some(value) = value {
            //Scalar setter13138 legitimately declines nonzero indices.
            if index == 0 {
                if property == "IgnoreAudioEvents" {
                    write_ignore_audio(&mut self.entities[owner].components, value)
                        .map_err(mlua::Error::external)?;
                } else {
                    write(&mut self.entities[owner].components, value)
                        .map_err(mlua::Error::external)?;
                }
            }
            Ok(MultiValue::new())
        } else {
            //265798 does not initialize its output byte. Do not invent a
            //false value for a declined indexed getter until its route is verified.
            if index != 0 {
                return Err(mlua::Error::external(
                    "Indexed boolean getter output is not verified",
                ));
            }
            let value = if property == "IgnoreAudioEvents" {
                read_ignore_audio(&self.entities[owner].components)
            } else {
                read(&self.entities[owner].components)
            }
            .map_err(mlua::Error::external)?
            .ok_or_else(|| mlua::Error::external("Admitted Boolean component disappeared"))?;
            Ok(MultiValue::from_vec(vec![Value::Boolean(value)]))
        }
    }
    /// Concrete story presentation slot24. Native null/absent Status targets
    /// decline. Do not route this to Hide(), collision removal or AnimVarInt.
    pub(super) fn set_story_player_invisible(&mut self, value: bool) -> Result<bool> {
        let entity = self.player_entity;
        if self.removed_entities.contains(&entity) {
            return Ok(false);
        }
        let Some(player) = self.entities.get_mut(entity) else {
            return Ok(false);
        };
        write(&mut player.components, value)
    }
}
/// Descriptor2e7378/2e750c installs IgnoreAudioEvents with default byte14=0.
/// Runtime scalar writes allocate backing; original source assets stay intact.
fn audio_field(components: &[Component]) -> Result<Option<(usize, Option<usize>)>> {
    let mut owners = components
        .iter()
        .enumerate()
        .filter(|(_, c)| c.class_name == "AnimationComponent");
    let Some((owner, c)) = owners.next() else {
        return Ok(None);
    };
    ensure!(
        owners.next().is_none(),
        "Multiple Animation components require native slot arbitration"
    );
    let mut fields = c
        .properties
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == "IgnoreAudioEvents");
    let field = fields.next().map(|(i, _)| i);
    ensure!(
        fields.next().is_none(),
        "Duplicate IgnoreAudioEvents property"
    );
    if let Some(i) = field {
        ensure!(
            c.properties[i].class_name == "Boolean" && c.properties[i].value.is_boolean(),
            "Invalid IgnoreAudioEvents Boolean"
        );
    }
    Ok(Some((owner, field)))
}
pub fn read_ignore_audio(components: &[Component]) -> Result<Option<bool>> {
    Ok(audio_field(components)?.map(|(owner, field)| {
        field.map_or(false, |i| {
            components[owner].properties[i].value.as_bool().unwrap()
        })
    }))
}
pub fn write_ignore_audio(components: &mut [Component], value: bool) -> Result<bool> {
    let Some((owner, field)) = audio_field(components)? else {
        return Ok(false);
    };
    if let Some(i) = field {
        components[owner].properties[i].value = value.into();
    } else {
        components[owner].properties.push(Property::new(
            "Boolean".into(),
            "IgnoreAudioEvents".into(),
            false,
            false,
            false,
            value.into(),
        ));
    }
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::scene_file::ID;
    fn status() -> Component {
        Component::new(
            "StatusComponent".into(),
            "Status".into(),
            ID::new(1),
            ID::new(1),
            ID::new(0),
            vec![],
        )
    }
    #[test]
    fn generated_default_lazy_storage_and_absent_component_are_distinct() -> Result<()> {
        let mut values = vec![];
        assert_eq!(read(&values)?, None);
        assert!(!write(&mut values, true)?);
        values.push(status());
        assert_eq!(read(&values)?, Some(false));
        assert!(values[0].properties.is_empty());
        assert!(write(&mut values, true)?);
        assert_eq!(read(&values)?, Some(true));
        assert_eq!(values[0].properties.len(), 1);
        assert!(write(&mut values, false)?);
        assert_eq!(read(&values)?, Some(false));
        assert_eq!(values[0].properties.len(), 1);
        values.push(status());
        assert!(read(&values).is_err());
        assert!(write(&mut values, true).is_err());
        Ok(())
    }
    #[test]
    fn malformed_authored_values_are_not_silently_replaced() -> Result<()> {
        let mut values = vec![status()];
        values[0].properties.push(Property::new(
            "Integer".into(),
            "Invisible".into(),
            false,
            false,
            false,
            0.into(),
        ));
        assert!(read(&values).is_err());
        assert!(write(&mut values, true).is_err());
        assert_eq!(values[0].properties[0].value, serde_json::json!(0));
        values[0].properties[0].class_name = "Boolean".into();
        assert!(read(&values).is_err());
        assert!(write(&mut values, true).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_player_status_write_preserves_render_and_collision_visibility() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let player = world.player_entity;
        let before = read(&world.entities[player].components)?
            .context("Original player has no Status component")?;
        let _player = world.take_startup_player();
        world.attach_player_body()?;
        let body = world
            .player_physics
            .as_ref()
            .context("Original player has no admitted collision body")?
            .owner;
        let in_world = world.collision.owner_in_world(body)?;
        let hidden = world.hidden_entities.clone();
        let draws = world.hidden_entity_draws.clone();
        let variables = world.player_graph.state.words().to_vec();
        assert!(world.set_story_player_invisible(!before)?);
        assert_eq!(read(&world.entities[player].components)?, Some(!before));
        assert_eq!(world.hidden_entities, hidden);
        assert_eq!(world.hidden_entity_draws, draws);
        assert_eq!(
            world.collision.owner_in_world(body)?,
            in_world,
            "Status write changed collision membership"
        );
        assert_eq!(
            world.player_graph.state.words(),
            variables,
            "Status write changed animation variables"
        );
        assert!(world.set_story_player_invisible(before)?);
        assert_eq!(read(&world.entities[player].components)?, Some(before));
        world.run_callback_fixture(
            r#"
            local e=GetPlayer()
            local initial=GetPropertyBool(e,'Invisible')
            assert(select('#',GetPropertyBool(e,'Invisible'))==1)
            assert(select('#',SetPropertyBool(e,'Invisible',true))==0)
            assert(GetPropertyBool(e,'Invisible')==true)
            SetPropertyBool(e,'Invisible','false');assert(not GetPropertyBool(e,'Invisible'))
            SetPropertyBool(e,'Invisible',1);assert(GetPropertyBool(e,'Invisible'))
            SetPropertyBool(e,'Invisible',0);assert(not GetPropertyBool(e,'Invisible'))
            SetPropertyBool(e,'Invisible',true,2);assert(not GetPropertyBool(e,'Invisible'))
            SetPropertyBool(e,'Invisible',true,-1);assert(not GetPropertyBool(e,'Invisible'))
            BwxBoolPropertyIndex=0;SetPropertyBool(e,'Invisible',true,'BwxBoolPropertyIndex')
            assert(GetPropertyBool(e,'Invisible'..string.char(0)..'ignored'))
            assert(select('#',GetPropertyBool('BwxNoEntity','Invisible'))==0)
            assert(select('#',GetPropertyBool(e,'BwxNoProperty'))==0)
            assert(select('#',SetPropertyBool(e,'BwxNoProperty',true))==0)
            assert(not pcall(GetPropertyBool,e,'Invisible',1))
            SetPropertyBool(e,'Invisible',initial)
        "#,
        )?;
        assert_eq!(read(&world.entities[player].components)?, Some(before));
        assert_eq!(world.hidden_entities, hidden);
        assert_eq!(world.hidden_entity_draws, draws);
        assert_eq!(world.collision.owner_in_world(body)?, in_world);
        assert_eq!(world.player_graph.state.words(), variables);
        Ok(())
    }
}
