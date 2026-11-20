//! Native SetPropertyVector callback REL266d18..267048.
//! Typed reflected storage; transform setters still require their live hooks.
use super::*;
impl World {
    pub(super) fn set_vector_property_lua(&mut self,lua:&mlua::Lua,mut args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        use mlua::Value;
        if args.len()!=3 && args.len()!=4 {
            return Err(mlua::Error::external("SetPropertyVector requires entity, property, vector and optional index"));
        }
        let index=if args.len()==4 {<i32 as mlua::FromLua>::from_lua(args.pop_back().unwrap(),lua)?} else {0};
        let Value::UserData(value)=args.pop_back().unwrap() else {return Err(mlua::Error::external("SetPropertyVector requires vector4 userdata"))};
        let lanes=value.borrow::<super::super::lua_vector::Vector4>()?.0;
        let property=lua.coerce_string(args.pop_back().unwrap())?.ok_or_else(||mlua::Error::external("Vector property name must be a string"))?;
        let name=property.to_str()?.split('\0').next().unwrap_or("").to_owned();
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap())?.filter(|i|!self.removed_entities.contains(i)) else {return Ok(mlua::MultiValue::new())};
        let mut found=self.entities[owner].components.iter().enumerate().find_map(|(c,component)|component.properties.iter().position(|p|p.name==name).map(|p|(c,p)));
        // The native class descriptor exists even when its value is omitted
        // from a cooked instance. A setter does not require a serialized default.
        if found.is_none() {
            let class=match name.as_str(){"UsableEffectOffset"=>Some("UsableComponent"),"Spawn Offset"=>Some("TemplateSpawnerComponent"),_=>None};
            if let Some(c)=class.and_then(|class|self.entities[owner].components.iter().position(|c|c.class_name==class)) {
                if index!=0 {return Ok(mlua::MultiValue::new())}
                if !lanes[..3].iter().all(|v|v.is_finite()) {
                    return Err(mlua::Error::external("Nonfinite reflected vector storage is not supported"));
                }
                self.entities[owner].components[c].properties.push(epic_mickey_lib_rs::scene_file::Property::new("Point3".into(),name.clone(),false,false,false,serde_json::json!({"x":0.,"y":0.,"z":0.})));
                found=Some((c,self.entities[owner].components[c].properties.len()-1));
            }
        }
        let Some((c,p))=found else {return Ok(mlua::MultiValue::new())};
        let component=&self.entities[owner].components[c];
        if component.class_name=="JPSTransformationComponent" {
            return Err(mlua::Error::external("SetPropertyVector transform fields require live transform setter integration"));
        }
        let property=&component.properties[p];
        let dimension=match property.class_name.as_str(){"Point2"=>2,"Point3"=>3,"Point4"=>4,"Color"|"ColorA"=>return Err(mlua::Error::external("Vector color-property setter binding is not connected")),_=>return Ok(mlua::MultiValue::new())};
        if !lanes[..dimension].iter().all(|v|v.is_finite()) {return Err(mlua::Error::external("Nonfinite reflected vector storage is not supported"))}
        let next=serde_json::Value::Object(["x","y","z","w"].into_iter().zip(lanes).take(dimension).map(|(k,v)|(k.into(),serde_json::json!(v))).collect());
        let property=&mut self.entities[owner].components[c].properties[p];
        if let Some(array)=property.value.as_array_mut() {
            if let Some(slot)=array.get_mut(index as u32 as usize) {*slot=next;}
        } else if index==0 {property.value=next;}
        Ok(mlua::MultiValue::new())
    }
}
