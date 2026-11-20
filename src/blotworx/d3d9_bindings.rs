//! Authored effect binding annotations, distinct from shader uniform names.
//! NTM/NTMIndex selects a NiTexturingProperty array slot, not ShaderMap::GetID.
use anyhow::{ensure,Context,Result};
use std::collections::BTreeMap;
use super::d3d9_effect::{Data,Effect,Value};
#[derive(Debug,serde::Serialize)]
#[serde(untagged)]
pub(super) enum Atom {Text(Option<String>),Integer(i32)}
#[derive(Debug,serde::Serialize)]
pub(super) struct Binding {pub parameter:usize,pub annotations:BTreeMap<&'static str,Atom>}
#[derive(Debug,PartialEq,Eq,serde::Serialize)]
pub(super) enum TextureOrigin {
    Null,
    ShaderMap {index:u32},BaseMap,
    AutomaticShaderMap,
    NamedDefault {resource:String,map:Option<String>,index:Option<i32>},
    Object {name:String,index:Option<i32>},
    OtherMap {name:String,index:Option<i32>},
    Unassigned {var_type:Option<String>},
}
const FIELDS:[&str;9]=["VarType","DefinedMapping","Object","ObjectProperty","ObjectIndex","Space","NTM","NTMIndex","Name"];
pub(super) fn string(data:&[u8],effect:&Effect,value:&Value)->Result<Option<String>> {
    let Data::Objects(ids)=&value.data else{anyhow::bail!("Effect binding string is not an object")};
    ensure!(ids.len()==1,"Effect binding string is an array");
    let object=effect.objects.iter().find(|o|o.id==ids[0]);
    let object=match object {None if ids[0]==0=>return Ok(None),None=>anyhow::bail!("Effect binding string object is absent"),Some(object)=>object};
    if object.payload.is_empty(){return Ok(None);}
    ensure!(object.source_kind.is_none(),"Effect binding string requires expression evaluation");
    let bytes=data.get(object.payload.clone()).context("Effect binding string outside resource")?;
    let bytes=bytes.strip_suffix(&[0]).context("Unterminated effect binding string")?;
    ensure!(!bytes.contains(&0),"Embedded terminator in effect binding string");
    Ok(Some(std::str::from_utf8(bytes)?.to_owned()))
}
pub(super) fn inspect(data:&[u8],effect:&Effect)->Result<Vec<Binding>> {
    effect.parameters.iter().enumerate().map(|(parameter,p)|{
        let mut annotations=BTreeMap::new();
        for value in &p.annotations {
            let Some(name)=value.name.as_deref() else{continue;};
            let Some(&field)=FIELDS.iter().find(|field|field.eq_ignore_ascii_case(name)) else{continue;};
            let atom=match value.kind {
                4=>Atom::Text(string(data,effect,value)?),
                2=>{let Data::Words(words)=&value.data else{anyhow::bail!("Effect binding integer is not numeric")};ensure!(words.len()==1,"Effect binding integer is not scalar");Atom::Integer(words[0] as i32)},
                kind=>anyhow::bail!("Effect binding annotation {field} has unsupported type {kind}"),
            };
            // The supplied effect parser consumes annotation order and compares
            // names without case. Preserve its later-assignment behavior.
            annotations.insert(field,atom);
        }
        Ok(Binding {parameter,annotations})
    }).collect()
}
impl Binding {
    pub fn text(&self,name:&str)->Result<Option<&str>> {
        match self.annotations.get(name){None=>Ok(None),Some(Atom::Text(value))=>Ok(value.as_deref()),_=>anyhow::bail!("Effect annotation {name} is not text")}
    }
    pub fn integer(&self,name:&str)->Result<Option<i32>> {
        match self.annotations.get(name){None=>Ok(None),Some(Atom::Integer(value))=>Ok(Some(*value)),_=>anyhow::bail!("Effect annotation {name} is not an integer")}
    }
    pub fn texture_origin(&self)->Result<TextureOrigin> {
        // Object annotations override NTM in the supporting D3DX effect parser.
        if let Some(name)=self.text("Object")?{return Ok(TextureOrigin::Object {name:name.into(),index:self.integer("ObjectIndex")?});}
        if let Some(resource)=self.text("Name")? {
            // Resolving the name can succeed or fall back to an NTM/default
            // shader slot. Automatic indices depend on earlier load results,
            // so this recipe must be executed by the resource owner in order.
            return Ok(TextureOrigin::NamedDefault {resource:resource.into(),map:self.text("NTM")?.map(str::to_owned),index:self.integer("NTMIndex")?});
        }
        if let Some(name)=self.text("NTM")? {
            let index=self.integer("NTMIndex")?;
            if name.eq_ignore_ascii_case("shader") {
                let index=index.unwrap_or(-1) as u32;
                return Ok(if index==u32::MAX {TextureOrigin::AutomaticShaderMap}else{TextureOrigin::ShaderMap {index}});
            }
            if name.eq_ignore_ascii_case("base"){return Ok(TextureOrigin::BaseMap);}
            return Ok(TextureOrigin::OtherMap {name:name.into(),index});
        }
        Ok(if self.text("VarType")?.is_none(){TextureOrigin::AutomaticShaderMap}else{TextureOrigin::Unassigned {var_type:self.text("VarType")?.map(str::to_owned)}})
    }
}

/// Resource lookup is supplied by the scene/material owner. None means a
/// genuine absent texture, not a substitute white image or a decode failure.
pub(super) trait Textures {
    type Texture;
    fn named(&mut self,name:&str)->Result<Option<Self::Texture>>;
    fn shader_map(&mut self,index:u32)->Result<Option<Self::Texture>>;
    fn base_map(&mut self)->Result<Option<Self::Texture>>;
    fn object(&mut self,name:&str,index:Option<i32>)->Result<Option<Self::Texture>>;
    fn other_map(&mut self,name:&str,index:Option<i32>)->Result<Option<Self::Texture>>;
}
fn map_texture<R:Textures>(receiver:&mut R,map:Option<&str>,index:Option<i32>,automatic:&mut u32)->Result<Option<R::Texture>> {
    let map=map.unwrap_or("shader");
    if map.eq_ignore_ascii_case("shader") {
        let mut index=index.unwrap_or(-1) as u32;
        if index==u32::MAX {index=*automatic;*automatic=automatic.checked_add(1).context("Automatic shader-map index overflow")?;}
        receiver.shader_map(index)
    }else if map.eq_ignore_ascii_case("base"){receiver.base_map()}else{receiver.other_map(map,index)}
}
/// Replay declarations, not pass sampler order: failed named loads can consume
/// automatic shader slots even for parameters the selected pass does not use.
pub(super) fn resolve_textures<R:Textures>(effect:&Effect,bindings:&[Binding],receiver:&mut R)->Result<BTreeMap<usize,Option<R::Texture>>> {
    ensure!(bindings.len()==effect.parameters.len(),"Effect binding table length differs");
    let mut result=BTreeMap::new();let mut automatic=0u32;
    for (index,parameter) in effect.parameters.iter().enumerate(){
        if !(5..=9).contains(&parameter.value.kind){continue;}
        ensure!(parameter.value.kind!=6,"One-dimensional texture parameters require their resource adapter");
        ensure!(parameter.value.elements<=1,"Texture arrays require their resource adapter");
        ensure!(bindings[index].parameter==index,"Effect binding parameter order differs");
        let value=match bindings[index].texture_origin()? {
            TextureOrigin::Null=>None,
            TextureOrigin::ShaderMap {index}=>receiver.shader_map(index)?,
            TextureOrigin::AutomaticShaderMap=>map_texture(receiver,None,None,&mut automatic)?,
            TextureOrigin::BaseMap=>receiver.base_map()?,
            TextureOrigin::Object {name,index}=>receiver.object(&name,index)?,
            TextureOrigin::OtherMap {name,index}=>receiver.other_map(&name,index)?,
            TextureOrigin::NamedDefault {resource,map,index}=>{
                // An explicit NTM request still owns the material binding.
                // Its named UI/default resource must not replace that map.
                if let Some(map)=map.as_deref() {
                    if map.eq_ignore_ascii_case("shader")&&index.unwrap_or(-1)==-1 {
                        ensure!(receiver.named(&resource)?.is_none(),"Named resource plus automatic explicit NTM requires descriptor admission");
                    }
                    map_texture(receiver,Some(map),index,&mut automatic)?
                }else{match receiver.named(&resource)? {Some(texture)=>Some(texture),None=>map_texture(receiver,None,index,&mut automatic)?}}
            },
            TextureOrigin::Unassigned {var_type}=>anyhow::bail!("Texture parameter requires its {var_type:?} resource owner"),
        };
        result.insert(index,value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::d3d9_effect::{Parameter,Value};
    fn binding(index:usize,values:&[(&'static str,Atom)])->Binding {Binding {parameter:index,annotations:values.iter().map(|(k,v)|(*k,match v {Atom::Text(v)=>Atom::Text(v.clone()),Atom::Integer(v)=>Atom::Integer(*v)})).collect()}}
    fn text(value:&str)->Atom {Atom::Text(Some(value.into()))}
    struct Receiver {calls:Vec<String>,fail_named:bool}
    impl Textures for Receiver {
        type Texture=String;
        fn named(&mut self,name:&str)->Result<Option<String>> {self.calls.push(format!("name:{name}"));ensure!(!self.fail_named,"Decoder failed");Ok((name=="found").then(||name.into()))}
        fn shader_map(&mut self,index:u32)->Result<Option<String>> {self.calls.push(format!("shader:{index}"));Ok(Some(format!("shader:{index}")))}
        fn base_map(&mut self)->Result<Option<String>> {self.calls.push("base".into());Ok(Some("base".into()))}
        fn object(&mut self,name:&str,index:Option<i32>)->Result<Option<String>> {self.calls.push(format!("object:{name}:{index:?}"));Ok(None)}
        fn other_map(&mut self,name:&str,_:Option<i32>)->Result<Option<String>> {anyhow::bail!("Unsupported map {name}")}
    }
    fn effect(count:usize)->Effect {Effect {parameters:(0..count).map(|_|Parameter {value:Value {kind:5,class:4,name:None,semantic:None,elements:0,columns:0,rows:0,data:Data::Objects(vec![0])},flags:0,annotations:vec![]}).collect(),techniques:vec![],objects:vec![],object_slots:1,trailing_bytes:0}}
    #[test]
    fn named_resolution_and_automatic_indices_follow_declaration_order()->Result<()> {
        let effect=effect(5);let bindings=vec![binding(0,&[("Name",text("found"))]),binding(1,&[("NTM",text("shader")),("NTMIndex",Atom::Integer(7))]),binding(2,&[("Name",text("missing"))]),binding(3,&[]),binding(4,&[("Object",text("ProjectedShadowMap")),("ObjectIndex",Atom::Integer(0))])];
        let mut receiver=Receiver {calls:vec![],fail_named:false};let result=resolve_textures(&effect,&bindings,&mut receiver)?;
        assert_eq!(receiver.calls,["name:found","shader:7","name:missing","shader:0","shader:1","object:ProjectedShadowMap:Some(0)"]);
        assert_eq!(result[&3].as_deref(),Some("shader:1"));assert!(result[&4].is_none());Ok(())
    }
    #[test]
    fn decode_errors_are_not_treated_as_missing_named_resources() {
        let mut receiver=Receiver {calls:vec![],fail_named:true};
        assert!(resolve_textures(&effect(1),&[binding(0,&[("Name",text("bad"))])],&mut receiver).is_err());
        assert_eq!(receiver.calls,["name:bad"]);
    }
    #[test]
    fn explicit_material_map_is_not_replaced_by_a_named_default()->Result<()> {
        let mut receiver=Receiver {calls:vec![],fail_named:true};
        let binding=binding(0,&[("Name",text("found")),("NTM",text("shader")),("NTMIndex",Atom::Integer(2))]);
        let result=resolve_textures(&effect(1),&[binding],&mut receiver)?;
        assert_eq!(result[&0].as_deref(),Some("shader:2"));assert_eq!(receiver.calls,["shader:2"]);Ok(())
    }
}
