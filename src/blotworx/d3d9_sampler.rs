//! Original PC effect texture routes and ordered sampler assignments.
//! These are D3D state descriptions, not guesses from texture filenames.
use anyhow::{ensure, Context, Result};
use super::{d3d9_constants::Table,d3d9_effect::{Data,Effect}};
#[derive(Debug,serde::Serialize)]
pub(super) struct State {pub state:u32,pub value:u32}
#[derive(Debug,serde::Serialize)]
pub(super) struct Sampler {
    pub register:u16,pub name:String,
    /// None means an explicit null texture assignment, never inheritance.
    pub texture_parameter:Option<usize>,pub states:Vec<State>,
    pub origin:super::d3d9_bindings::TextureOrigin,
}
pub(super) fn resolve(data:&[u8],effect:&Effect,table:Option<&Table>,bindings:&[super::d3d9_bindings::Binding])->Result<Vec<Sampler>> {
    let Some(table)=table else{return Ok(Vec::new());};let mut result=Vec::new();
    for constant in &table.constants {
        if constant.register_set!=3 {continue;}
        ensure!(constant.registers==1&&constant.ty.elements<=1,"Sampler arrays require their register-range binding");
        let mut parameters=effect.parameters.iter().enumerate().filter(|(_,p)|p.value.name.as_deref()==Some(&constant.name));
        let (_parameter_index,parameter)=parameters.next().context("Shader sampler parameter is absent")?;
        ensure!(parameters.next().is_none(),"Ambiguous shader sampler parameter");
        let Data::Sampler(assignments)=&parameter.value.data else {anyhow::bail!("Shader sampler parameter is not a sampler")};
        let mut texture=None;let mut states=Vec::new();
        for assignment in assignments {
            // Original inline sampler declarations carry 256 here. This is
            // not the hardware slot: that comes from the shader's CTAB.
            ensure!(assignment.index==256,"Unsupported sampler declaration marker");
            if assignment.kind==4 {
                // Microsoft FXO encodes Texture=NULL as this scalar integer
                // assignment, rather than a texture object with ID zero.
                if let Data::Words(words)=&assignment.value.data {
                    ensure!(assignment.value.kind==2&&assignment.value.class==2&&assignment.value.rows==1&&assignment.value.columns==1&&assignment.value.elements==0&&words==&[0],"Unsupported numeric sampler texture assignment");
                    texture=Some(None);continue;
                }
                let Data::Objects(ids)=&assignment.value.data else {anyhow::bail!("Sampler texture assignment is not an object")};
                ensure!(ids.len()==1,"Sampler texture assignment has multiple objects");
                let id=ids[0];
                let object=effect.objects.iter().find(|o|o.id==id);
                let name=match object {
                    None if id==0=>None,
                    None=>anyhow::bail!("Sampler texture object is absent"),
                    Some(object) if object.payload.is_empty()=>None,
                    Some(object)=>{
                        ensure!(object.source_kind==Some(1),"Sampler texture assignment requires expression evaluation");
                        let bytes=data.get(object.payload.clone()).context("Sampler texture name outside effect")?;
                        let bytes=bytes.strip_suffix(&[0]).context("Unterminated sampler texture name")?;
                        ensure!(!bytes.contains(&0),"Embedded terminator in sampler texture name");
                        Some(std::str::from_utf8(bytes)?)
                    },
                };
                let index=if let Some(name)=name {
                    let mut matches=effect.parameters.iter().enumerate().filter(|(_,p)|p.value.name.as_deref()==Some(name));
                    let (index,parameter)=matches.next().context("Sampler texture parameter is absent")?;
                    ensure!(matches.next().is_none(),"Ambiguous sampler texture parameter");
                    ensure!((5..=9).contains(&parameter.value.kind)&&parameter.value.class==4,"Sampler target is not a texture");
                    Some(index)
                }else{None};
                texture=Some(index);
            }else{
                ensure!((5..=17).contains(&assignment.kind),"Unknown effect sampler state");
                let Data::Words(words)=&assignment.value.data else {anyhow::bail!("Sampler state requires expression evaluation")};
                ensure!(words.len()==1,"Sampler state is not scalar");
                states.push(State {state:assignment.kind-4,value:words[0]});
            }
        }
        let texture_parameter=texture.context("Sampler requires inherited texture state")?;
        // D3DX resolves the sampler's Texture object to the texture parameter.
        // The supporting Gamebryo parser binds texture-typed parameters, not
        // sampler UI annotations. Four retail effects have stale sampler NTM
        // indices; using those would bind the detail normal as the base normal.
        let origin=if let Some(target)=texture_parameter {
            bindings.get(target).context("Texture binding metadata is absent")?.texture_origin()?
        }else{super::d3d9_bindings::TextureOrigin::Null};
        result.push(Sampler {register:constant.register,name:constant.name.clone(),texture_parameter,states,origin});
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{d3d9_constants::{Constant,Type},d3d9_effect::{Value,Parameter,Object,State as Assignment}};
    fn value(kind:u32,data:Data)->Value {Value {kind,class:4,name:None,semantic:None,elements:0,columns:0,rows:0,data}}
    fn fixture()->(Effect,Table){
        let mut sampler=value(12,Data::Sampler(vec![Assignment {kind:4,index:256,value:value(5,Data::Objects(vec![1]))},Assignment {kind:5,index:256,value:value(2,Data::Words(vec![1]))}]));sampler.name=Some("sampler".into());
        let mut texture=value(5,Data::Objects(vec![0]));texture.name=Some("diffuseMap".into());
        (Effect {parameters:vec![Parameter {value:sampler,flags:0,annotations:vec![]},Parameter {value:texture,flags:0,annotations:vec![]}],techniques:vec![],objects:vec![Object {id:1,payload:0..11,source_kind:Some(1)}],object_slots:2,trailing_bytes:0},Table {creator:String::new(),target:String::new(),flags:0,constants:vec![Constant {name:"sampler".into(),register_set:3,register:3,registers:1,ty:Type {class:4,kind:12,rows:1,columns:1,elements:1,members:vec![]},defaults:vec![]}]})
    }

    #[test]
    fn texture_parameter_wins_over_stale_sampler_ui_metadata()->Result<()> {
        use super::super::d3d9_bindings::{Atom,TextureOrigin};
        let (effect,table)=fixture();let mut bindings=super::super::d3d9_bindings::inspect(b"diffuseMap\0",&effect)?;
        for (binding,index) in bindings.iter_mut().zip([3,1]){binding.annotations.insert("NTM",Atom::Text(Some("shader".into())));binding.annotations.insert("NTMIndex",Atom::Integer(index));}
        let samplers=resolve(b"diffuseMap\0",&effect,Some(&table),&bindings)?;
        assert_eq!(samplers[0].origin,TextureOrigin::ShaderMap {index:1});Ok(())
    }
    #[test]
    fn hardware_slot_and_texture_identity_come_from_original_bindings()->Result<()> {
        let (effect,table)=fixture();let samplers=resolve(b"diffuseMap\0",&effect,Some(&table),&super::super::d3d9_bindings::inspect(b"diffuseMap\0",&effect)?)?;
        assert_eq!(samplers.len(),1);assert_eq!(samplers[0].register,3);assert_eq!(samplers[0].texture_parameter,Some(1));
        assert_eq!(samplers[0].states[0].state,1);assert_eq!(samplers[0].states[0].value,1);Ok(())
    }
    #[test]
    fn malformed_routes_fail_instead_of_guessing_a_texture() {
        let (mut effect,table)=fixture();let bindings=super::super::d3d9_bindings::inspect(b"diffuseMap\0",&effect).unwrap();assert!(resolve(b"diffuseMap",&effect,Some(&table),&bindings).is_err());
        effect.objects[0].source_kind=Some(2);assert!(resolve(b"diffuseMap\0",&effect,Some(&table),&bindings).is_err());
        effect.objects.clear();assert!(resolve(b"diffuseMap\0",&effect,Some(&table),&bindings).is_err());
        let Data::Sampler(states)=&mut effect.parameters[0].value.data else{unreachable!()};states.remove(0);
        assert!(resolve(&[],&effect,Some(&table),&bindings).is_err());
    }
}
