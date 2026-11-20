//! Native named-subgraph color publication, REL3ad140/3ac8ec/16ccf8.
use super::{actor_scripts::Scripts, scene_bounds::Hierarchy};
use anyhow::{ensure, Context, Result};

pub(super) trait Model {
    fn root(&mut self) -> Result<Option<u32>>;
    fn find(&mut self, root: u32, name: &str) -> Result<Option<u32>>;
    fn publish(&mut self, root: u32, slot: u8, rgba: [u8; 4]) -> Result<usize>;
}
pub(super) fn set_constant(
    model: &mut impl Model,
    name: Option<&str>,
    slot: i32,
    color: [f32; 4],
) -> Result<usize> {
    let Some(mut root) = model.root()? else {
        return Ok(0);
    };
    if !(0..4).contains(&slot) {
        return Ok(0);
    }
    let rgba = super::paint_counter::pack_color(color)?;
    if let Some(name) = name
        .map(|name| name.split('\0').next().unwrap_or(""))
        .filter(|name| !name.is_empty())
    {
        let Some(found) = model.find(root, name)? else {
            return Ok(0);
        };
        root = found;
    }
    model.publish(root, slot as u8, rgba)
}
pub(super) struct Graph<'a, F> {
    pub hierarchy: &'a Hierarchy,
    pub root: Option<u32>,
    pub visitor: F,
}
impl<F: FnMut(u32, u8, [u8; 4]) -> Result<bool>> Model for Graph<'_, F> {
    fn root(&mut self) -> Result<Option<u32>> {
        Ok(self.root)
    }
    fn find(&mut self, root: u32, name: &str) -> Result<Option<u32>> {
        self.hierarchy.find_named_node(root, name)
    }
    fn publish(&mut self, root: u32, slot: u8, rgba: [u8; 4]) -> Result<usize> {
        let mut pending = vec![root];
        let mut published = 0;
        while let Some(id) = pending.pop() {
            let node = self
                .hierarchy
                .nodes
                .get(&id)
                .context("Color subtree node missing")?;
            if node.authored_bound.is_some() {
                published += usize::from((self.visitor)(id, slot, rgba)?);
            }
            pending.extend(node.children.iter().rev().flatten().copied());
        }
        Ok(published)
    }
}
pub(super) fn model_root(hierarchy: &Hierarchy) -> Result<Option<u32>> {
    let mut roots = hierarchy.roots.iter().flatten().copied();
    let first = roots.next();
    ensure!(
        roots.next().is_none(),
        "Color publication needs the native root selection for multiple model roots"
    );
    Ok(first)
}

pub(super) struct Request {
    pub entity: Option<usize>,
    pub name: Option<String>,
    pub slot: i32,
    pub color: [f32; 4],
}
pub(super) fn parse(
    lua: &mlua::Lua,
    scripts: &Scripts,
    args: mlua::MultiValue,
) -> mlua::Result<Request> {
    use mlua::Value;
    if args.len() != 4 {
        return Err(mlua::Error::external("SetR3MTConstRegOnSubGraph requires entity, subgraph, register and vector4; native count diagnostics are not reconstructed"));
    }
    // Native pops vector, integer, string, entity, in that order.
    let Value::UserData(vector) = &args[3] else {
        return Err(mlua::Error::external(
            "Subgraph color requires vector4 userdata",
        ));
    };
    let color = vector.borrow::<super::lua_vector::Vector4>()?.0;
    let value = match &args[2] {
        Value::Boolean(value) => f64::from(u8::from(*value)),
        value => {
            if let Some(number) = lua.coerce_number(value.clone())? {
                number
            } else if let Value::String(text) = value {
                let text = text.to_str()?;
                let name = text.split('\0').next().unwrap_or("");
                lua.coerce_number(lua.globals().get::<Value>(name)?)?
                    .unwrap_or(0.)
            } else {
                0.
            }
        }
    }
    .trunc();
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return Err(mlua::Error::external(
            "Subgraph register integer requires native fctiwz special-value validation",
        ));
    }
    let name = lua
        .coerce_string(args[1].clone())?
        .map(|text| {
            text.to_str()
                .map(|text| text.split('\0').next().unwrap_or("").to_owned())
        })
        .transpose()?;
    let entity = scripts.resolve_native_entity_argument(lua, args[0].clone())?;
    Ok(Request {
        entity,
        name,
        slot: value as i32,
        color,
    })
}

impl super::assets::World {
    pub(super) fn set_subgraph_constant(
        &mut self,
        entity: usize,
        name: Option<&str>,
        slot: i32,
        color: [f32; 4],
    ) -> Result<usize> {
        if self.removed_entities.contains(&entity) {
            return Ok(0);
        }
        if let Some(effect) = self.named_fx.live.get_mut(&entity) {
            return effect.particles.set_subgraph_constant(name, slot, color);
        }
        if let Some(effect) = self.scene_particles.live.get_mut(&entity) {
            return effect.particles.set_subgraph_constant(name, slot, color);
        }
        let owner = self.entities.get(entity).context("Color owner missing")?;
        let Some(asset) = owner.model_asset.as_ref() else {
            return Ok(0);
        };
        let hierarchy = self
            .model_bounds
            .get(asset)
            .context("Color model hierarchy missing")?;
        let objects = &mut self.render_objects;
        let mut graph = Graph {
            hierarchy,
            root: model_root(hierarchy)?,
            visitor: |node, slot, rgba| {
                objects
                    .get_mut(&(entity, node))
                    .context("Color target is not an initialized render object")?
                    .publish(slot, rgba)
            },
        };
        set_constant(&mut graph, name, slot, color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_admission_and_byte_packing_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/subgraph_color.json"
        ))?;
        struct Target {
            root: bool,
            found: bool,
            trace: Vec<String>,
        }
        impl Model for Target {
            fn root(&mut self) -> Result<Option<u32>> {
                self.trace.push("model".into());
                Ok(self.root.then_some(1))
            }
            fn find(&mut self, _: u32, _: &str) -> Result<Option<u32>> {
                self.trace.push("find".into());
                Ok(self.found.then_some(2))
            }
            fn publish(&mut self, root: u32, slot: u8, _: [u8; 4]) -> Result<usize> {
                self.trace.push(format!("constant:{root}:{slot}"));
                Ok(1)
            }
        }
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 40);
        for case in fixture["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let mut target = Target {
                root: case["root"].as_bool().unwrap(),
                found: name == "found",
                trace: Vec::new(),
            };
            set_constant(
                &mut target,
                match name {
                    "null" => None,
                    "empty" => Some(""),
                    _ => Some(name),
                },
                case["slot"].as_i64().unwrap() as i32,
                [0.337, 0.467, 0.306, 0.],
            )?;
            assert_eq!(serde_json::to_value(target.trace)?, case["trace"], "{case}");
        }
        assert_eq!(fixture["packed"].as_array().unwrap().len(), 6);
        for case in fixture["packed"].as_array().unwrap() {
            let color = std::array::from_fn(|i| case["color"][i].as_f64().unwrap() as f32);
            assert_eq!(
                serde_json::to_value(super::super::paint_counter::pack_color(color)?)?,
                case["rgba"]
            );
        }
        let mut absent = Target {
            root: false,
            found: true,
            trace: Vec::new(),
        };
        assert_eq!(
            set_constant(&mut absent, None, 1, [f32::NAN; 4])?,
            0,
            "A missing model must decline before conversion"
        );
        absent.root = true;
        assert_eq!(
            set_constant(&mut absent, None, -1, [f32::NAN; 4])?,
            0,
            "An invalid register must decline before conversion"
        );
        Ok(())
    }
    #[test]
    fn lua_arguments_preserve_native_coercions_and_parse_vector_first() -> Result<()> {
        use mlua::Value;
        let lua = mlua::Lua::new();
        super::super::lua_vector::install(&lua)?;
        let resolver = Scripts::new(&[(0, "ColorOwner".into())])?;
        lua.globals().set("ColorSlot", "2.8")?;
        for (text, slot) in [
            ("true", 1),
            ("false", 0),
            ("'1.8'", 1),
            ("'ColorSlot'", 2),
            ("'MissingSlot'", 0),
            ("nil", 0),
        ] {
            let args = lua
                .load(format!(
                    "return 'ColorOwner', 'mesh\\0trailing', {text}, vector4(.337,.467,.306,0)"
                ))
                .eval()?;
            let request = parse(&lua, &resolver, args)?;
            assert_eq!(request.entity, Some(0));
            assert_eq!(request.slot, slot);
            assert_eq!(request.name.as_deref(), Some("mesh"));
        }
        let args = lua.load("return nil,nil,1,vector4(0,0,0,0)").eval()?;
        assert!(parse(&lua, &resolver, args)?.entity.is_none());
        assert!(parse(
            &lua,
            &resolver,
            mlua::MultiValue::from_vec(vec![Value::Nil, Value::Nil, Value::Nil, Value::Nil])
        )
        .is_err());
        Ok(())
    }
}
