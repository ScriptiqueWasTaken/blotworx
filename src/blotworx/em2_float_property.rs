//! PC float script wrappers 100cf10/100e500, component dispatch 972ba0.
//! Only independently verified descriptors are admitted. Other float setters
//! may invoke runtime callbacks and must not become arbitrary JSON writes.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Component, Property};
use mlua::{FromLua, Value};

pub struct Request {
    pub entity: Value,
    name: String,
    index: i32,
    value: Option<f32>,
}
impl Request {
    pub fn parse(lua: &mlua::Lua, setter: bool, args: mlua::MultiValue) -> mlua::Result<Self> {
        let required = if setter { 3 } else { 2 };
        if args.len() != required && args.len() != required + 1 {
            return Err(mlua::Error::external("Float property API has invalid argument count"));
        }
        let name = String::from_lua(args[1].clone(), lua)?;
        Ok(Self {
            entity: args[0].clone(),
            name: name.split('\0').next().unwrap_or_default().into(),
            index: if args.len() > required { i32::from_lua(args[required].clone(), lua)? } else { 0 },
            value: if setter { Some(f64::from_lua(args[2].clone(), lua)? as f32) } else { None },
        })
    }
    pub fn apply(self, _lua: &mlua::Lua, components: &mut [Component]) -> mlua::Result<mlua::MultiValue> {
        let result = self.access(components).map_err(mlua::Error::external)?;
        Ok(mlua::MultiValue::from_vec(result.map(|v| Value::Number(f64::from(v))).into_iter().collect()))
    }
    fn access(&self, components: &mut [Component]) -> Result<Option<f32>> {
        // First reflected property wins. Include the verified constructor
        // descriptor even when the cooked instance omits its default value.
        let owner = components.iter().position(|c| {
            c.properties.iter().any(|p| p.name == self.name)
                || (c.class_name == "OnGroundStateComponent" && self.name == "Max Move Speed")
                || (c.class_name == "UsableComponent" && self.name == "UsableRadius")
        }).with_context(|| format!("Unreconstructed PC float descriptor {}", self.name))?;
        let component = &mut components[owner];
        let property = component.properties.iter().position(|p| p.name == self.name);
        if property.is_some_and(|i| component.properties[i].class_name != "Float") {
            return Ok(None); // Wrapper returns zero values for a non-float type.
        }
        let default=match (component.class_name.as_str(),self.name.as_str()) {
            ("OnGroundStateComponent","Max Move Speed")=>6., // a69f90
            ("UsableComponent","UsableRadius")=>0., // b0e6c0, b10680/b106a0
            _=>anyhow::bail!("Unreconstructed PC float setter/getter {}.{}",component.class_name,self.name),
        };
        ensure!(property.is_none_or(|i| !component.properties[i].value.is_array()), "Invalid component scalar");
        if self.index != 0 {
            // 972ba0 does not invoke the scalar setter. The getter wrapper
            // initializes its temporary to zero and pushes it even on failure.
            return Ok(self.value.is_none().then_some(0.));
        }
        if let Some(value) = self.value {
            ensure!(value.is_finite(), "Nonfinite PC float storage is unsupported");
            if let Some(i) = property {
                component.properties[i].value = serde_json::json!(value);
            } else {
                component.properties.push(Property::new("Float".into(), self.name.clone(), false, false, false, serde_json::json!(value)));
            }
            // a743e0 and b10680 plainly store their fast scalars. Neither
            // sends an event, restarts a state, or clamps the value.
            Ok(None)
        } else {
            let value = property.map(|i| component.properties[i].value.as_f64().context("Invalid component scalar")).transpose()?.unwrap_or(default);
            Ok(Some(value as f32))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn component(properties: Vec<Property>) -> Component {
        Component::new("OnGroundStateComponent".into(), "Ground".into(), epic_mickey_lib_rs::scene_file::ID::new(0), epic_mickey_lib_rs::scene_file::ID::new(1), epic_mickey_lib_rs::scene_file::ID::new(0), properties)
    }
    fn request(value: Option<f32>, index: i32) -> Request {
        Request { entity: Value::Nil, name: "Max Move Speed".into(), index, value }
    }
    #[test]
    fn constructor_default_script_write_and_nonzero_scalar_index_match_pc() -> Result<()> {
        let mut components = [component(vec![])];
        assert_eq!(request(None, 0).access(&mut components)?, Some(6.));
        assert_eq!(request(Some(2.5), 0).access(&mut components)?, None);
        assert_eq!(request(None, 0).access(&mut components)?, Some(2.5));
        assert_eq!(request(Some(99.), 1).access(&mut components)?, None);
        assert_eq!(request(None, 1).access(&mut components)?, Some(0.));
        assert_eq!(request(None, -1).access(&mut components)?, Some(0.));
        assert_eq!(request(None, 0).access(&mut components)?, Some(2.5));
        request(Some(-3.), 0).access(&mut components)?;
        assert_eq!(request(None, 0).access(&mut components)?, Some(-3.));
        Ok(())
    }
    #[test]
    fn first_nonfloat_property_wins_and_unverified_owners_fail() {
        let mut earlier = component(vec![Property::new("String".into(), "Max Move Speed".into(), false, false, false, serde_json::json!("x"))]);
        earlier.class_name = "Other".into();
        assert_eq!(request(Some(3.), 0).access(&mut [earlier]).unwrap(), None);
        let mut unknown = component(vec![Property::new("Float".into(), "Max Move Speed".into(), false, false, false, serde_json::json!(9.))]);
        unknown.class_name = "Unknown".into();
        assert!(request(Some(3.), 0).access(&mut [unknown]).is_err());
    }
}
