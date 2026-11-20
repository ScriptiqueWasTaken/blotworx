//! Retail LogicRelay Lua callbacks35f918..35fef4 and trigger35f704.
use anyhow::{Context,Result,ensure};
use mlua::{Lua,MultiValue,Value};
impl super::assets::World {
    pub(super) fn relay_disabled(&self,owner:usize)->Result<Option<bool>> {
        let Some(component)=self.entities[owner].components.iter().find(|c|c.class_name=="LogicRelayComponent") else {return Ok(None)};
        component.properties.iter().find(|p|p.name=="RelayDisabled").map(|p|{
            ensure!(p.class_name=="Boolean"&&!p.value.is_array(),"Invalid RelayDisabled property type");
            p.value.as_bool().context("Invalid RelayDisabled value")
        }).transpose().map(|value|Some(value.unwrap_or(false)))
    }
    pub(super) fn set_relay_disabled(&mut self,owner:usize,value:bool)->Result<()> {
        let component=self.entities[owner].components.iter_mut().find(|c|c.class_name=="LogicRelayComponent").context("Relay component missing")?;
        if let Some(field)=component.properties.iter_mut().find(|p|p.name=="RelayDisabled") {
            ensure!(field.class_name=="Boolean"&&!field.value.is_array(),"Invalid RelayDisabled type");field.value=value.into();
        } else {component.properties.push(epic_mickey_lib_rs::scene_file::Property::new("Boolean".into(),"RelayDisabled".into(),false,false,false,value.into()));}
        Ok(())
    }
    pub(super) fn relay_lua(&mut self,lua:&Lua,name:&str,mut args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=1 {self.report.warnings.push(format!("{name}: expected one argument; preserving native stack order"));}
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(Value::Nil))?.filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::new())};
        let Some(disabled)=self.relay_disabled(owner).map_err(mlua::Error::external)? else {return Ok(MultiValue::new())};
        if name=="RelayTrigger" {
            if !disabled {
                self.entity_event_queue.enqueue(&super::entity_events::Event::Basic(super::entity_events::Basic{event_kind:0x4e,source:Some(owner),header:None}),None).map_err(mlua::Error::external)?;
            }
        } else {
            let value=match name {"RelayEnable"=>false,"RelayDisable"=>true,_=>!disabled};
            self.set_relay_disabled(owner,value).map_err(mlua::Error::external)?;
        }
        Ok(MultiValue::new())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_relay()->Result<()> {
    use epic_mickey_lib_rs::scene_file::{Component,Property,ID};
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=super::assets::load(std::path::Path::new(&root),"Levels/2D_LonesomeGhost02.level",&egui::Context::default())?;
    let owner=world.entities.iter().position(|entity|entity.components.iter().any(|c|c.class_name=="LogicRelayComponent")).context("Original ghost level relay missing")?;
    world.process_deferred_entity_events()?;
    world.outputs.remove_owner(owner);world.retire_deferred_output_events(owner);
    let fields=[("Output",serde_json::json!(["OnRelayTrigger"])),("Input",serde_json::json!(["RelayProbe"])),("Target",serde_json::json!(["_self"])),("Parameters",serde_json::json!(["_activator"])),("DelayWait",serde_json::json!([0.])),("IoFlags",serde_json::json!([0])),("TargetGroup",serde_json::json!([""]))];
    let io=Component::new("InputOutputComponent".into(),"Probe".into(),ID::new(0),ID::new(0),ID::new(0),fields.into_iter().map(|(name,value)|Property::new(String::new(),name.into(),false,false,false,value)).collect());
    world.outputs.bind(owner,&world.entities[owner].name,&[io])?;world.bind_deferred_output_events(owner)?;
    let name=world.entities[owner].name.clone();
    world.run_callback_fixture("RelayCalls=0; function RelayProbe(entity,activator) assert(activator==nil,'Relay fabricated an activator'); RelayCalls=RelayCalls+1 end")?;
    world.run_callback_fixture(&format!("local r=GetEntity({name:?}); assert(OnRelayTrigger==78); RelayDisable(r); assert(GetPropertyBool(r,'RelayDisabled')); RelayTrigger(r); assert(RelayCalls==0)"))?;
    world.process_deferred_entity_events()?;world.run_callback_fixture("assert(RelayCalls==0)")?;
    world.run_callback_fixture(&format!("local r=GetEntity({name:?}); RelayEnable(r); assert(not GetPropertyBool(r,'RelayDisabled')); RelayTrigger(r); assert(RelayCalls==0)"))?;
    world.process_deferred_entity_events()?;world.run_callback_fixture("assert(RelayCalls==1)")?;
    world.run_callback_fixture(&format!("local r=GetEntity({name:?}); RelayToggle(r); RelayTrigger(r); assert(GetPropertyBool(r,'RelayDisabled')); RelayToggle(r); RelayTrigger(r); assert(select('#',RelayTrigger(nil))==0); RelayTrigger(GetPlayer())"))?;
    world.process_deferred_entity_events()?;world.run_callback_fixture("assert(RelayCalls==2)")?;
    world.run_callback_fixture(&format!("local r=GetEntity({name:?}); SetPropertyBool(r,'RelayDisabled',true); RelayTrigger(r)"))?;
    world.process_deferred_entity_events()?;world.run_callback_fixture("assert(RelayCalls==2)")?;
    println!("PASS: original Lonesome Ghost relay enable/disable/toggle/reflected Boolean, deferred IO, nullable activator and missing-target admission");
    Ok(())
}
