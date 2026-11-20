//! Native Lua AnimVarInt2e9e08 and AnimVarFloat2e9c80, live setter2f3718/34cc.
use super::{
    action_event_dispatch::{EventRegistry, Host},
    assets::World,
    entity_events::{Event, Variable},
};
use anyhow::{ensure, Context, Result};

#[derive(Default, serde::Serialize)]
pub struct Trace {
    pub requests: u64,
    pub mapped_writes: u64,
    pub last: Option<Write>,
}
#[derive(serde::Serialize)]
pub struct Write {
    pub event: Variable,
    pub name: Option<String>,
    pub local: Option<usize>,
    pub notification_admitted: Option<bool>,
}
impl Trace {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"requests":self.requests,"mapped_writes":self.mapped_writes,"last":self.last,
            "native_listener_initialization_complete":false,"native_frame_order_verified":false})
    }
}
impl World {
    pub(super) fn npc_animation_state_lua(
        &mut self,
        lua: &mlua::Lua,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        let state = super::score_state::integer_argument(
            lua,
            args.pop_back().unwrap_or(mlua::Value::Nil),
        )?;
        if let Some(entity) = self.scripts.resolve_world_argument(
            &args.pop_back().unwrap_or(mlua::Value::Nil),
        )?.filter(|entity| !self.removed_entities.contains(entity)) {
            // EM1's verified animation table maps slot 0x30 to VAR_NPC_State.
            self.write_animation_variable(entity, 0x30, 1, state as u32)
                .map_err(mlua::Error::external)?;
            // The caged prefab uses a non-supporting collision layer. Keep its
            // authored pose on the preview backend until the cage's release
            // callback explicitly enables motion.
            if state == 1 && self.relative_prefab_entity(entity, ".caged_gremlin")
                .map_err(mlua::Error::external)? == Some(entity)
                && self.character_physics.get(&entity).is_some_and(Result::is_ok)
            {
                self.clear_character_linear_velocity(entity)
                    .and_then(|()| self.set_entity_motion_enabled(entity, false))
                    .map_err(mlua::Error::external)?;
            }
        }
        Ok(mlua::MultiValue::new())
    }

    pub(super) fn animation_variable_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if args.len() != 3 {
            return Err(mlua::Error::external(format!("{name} requires entity, native variable ID and value; native count diagnostics are not reconstructed")));
        }
        // Native stack order: value, ID, then entity. Float uses lua_tonumber,
        // not the integer converter's bool/global-name extension.
        let value = args.pop_back().unwrap();
        let (kind, bits) = if name == "AnimVarInt" {
            (1, super::score_state::integer_argument(lua, value)? as u32)
        } else {
            let value = lua.coerce_number(value)?.unwrap_or(0.) as f32;
            (2, if value.is_nan() { 0 } else { value.to_bits() })
        };
        let id = super::score_state::integer_argument(lua, args.pop_back().unwrap())?;
        let entity = self
            .scripts
            .resolve_world_argument(&args.pop_back().unwrap())?
            .filter(|entity| !self.removed_entities.contains(entity));
        if let Some(entity) = entity {
            self.write_animation_variable(entity, id, kind, bits)
                .map_err(mlua::Error::external)?;
        }
        Ok(mlua::MultiValue::new())
    }
    pub(super) fn write_animation_variable(
        &mut self,
        entity: usize,
        id: i32,
        value_type: u32,
        bits: u32,
    ) -> Result<Option<usize>> {
        let owner = self
            .entities
            .get(entity)
            .context("Animation variable target is not owned")?;
        let components = owner
            .components
            .iter()
            .filter(|component| component.class_name == "AnimationComponent")
            .count();
        if components == 0 {
            return Ok(None);
        }
        ensure!(
            components == 1,
            "Animation variable component selection requires native arbitration"
        );
        let catalog = self
            .animation_events
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?;
        let name = catalog.variable_name(id);
        let local = if entity == self.player_entity {
            name.and_then(|name| self.player_graph.state.write_native_variable(name, bits))
        } else if self.authored_havok.contains(entity) {
            match name {
                Some(name) => self
                    .authored_havok
                    .write_native_variable(entity, name, bits)?,
                None => None,
            }
        } else if let Some((_, actor)) = self.havok_actors.get_mut(&entity) {
            name.and_then(|name| actor.prepared.state.write_native_variable(name, bits))
        } else {
            anyhow::bail!(
                "Animation variable target {} has no initialized Havok graph",
                owner.name
            )
        };
        // Even an unmapped/negative native ID emits kind0b when the character
        // graph exists. No feedback is emitted for absent components/graphs.
        let event = Variable {
            source: Some(entity),
            header: None,
            value_type,
            index: id,
            bits,
        };
        let trace = &mut self.animation_variable_changes;
        trace.requests = trace.requests.wrapping_add(1);
        trace.mapped_writes = trace.mapped_writes.wrapping_add(u64::from(local.is_some()));
        trace.last = Some(Write {
            event: event.clone(),
            name: name.map(str::to_owned),
            local,
            notification_admitted: None,
        });
        struct Receivers;
        impl Host<Event> for Receivers {
            fn receive(
                &mut self,
                _: &mut EventRegistry<Event>,
                listener: u32,
                _: &Event,
            ) -> Result<()> {
                anyhow::bail!("Native animation-variable listener {listener} is not connected")
            }
        }
        let admission = self
            .entity_event_registry
            .dispatch(&event.into(), None, &mut Receivers)?;
        self.animation_variable_changes
            .last
            .as_mut()
            .unwrap()
            .notification_admitted =
            Some(admission == super::action_event_dispatch::Admission::Delivered);
        Ok(local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn native_io_initializes_gus_animation_state_before_demo_start() -> Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.level",&egui::Context::default())?;
        ensure!(!world.demo_story.enabled,"Startup must not rely on enabling demo behavior");
        let gus=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
        let io=world.entities[gus].components.iter().find(|c|c.class_name=="InputOutputComponent").context("Gus IO absent")?;
        let field=|name:&str|io.properties.iter().find(|p|p.name==name).and_then(|p|p.value.as_array());
        let inputs=field("Input").context("Gus inputs absent")?;
        let index=inputs.iter().position(|v|v.as_str()==Some("AnimVarInt")).context("Original variant setter absent")?;
        let parameter=field("Parameters").and_then(|v|v.get(index)).and_then(|v|v.as_str()).context("Variant parameters absent")?;
        let parts=super::super::actor_io::parameters(parameter);
        ensure!(parts.len()==2,"Unexpected original animation row: {parameter}");
        let expected=parts[1].trim().parse::<i32>()? as u32;
        let snapshot=world.authored_havok.snapshot(gus)?;
        let variable=snapshot["variables"].as_array().context("Gus variables absent")?.iter().find(|v|v["name"]==parts[0]).context("Authored Gus animation variable missing")?;
        ensure!(variable["bits"].as_u64()==Some(expected as u64),"Startup variant did not reach actor: {variable}, expected {expected}");
        ensure!(world.animation_variable_changes.mapped_writes>0,"Startup performed no native mapped variable write");
        ensure!(!world.report.unsupported.iter().any(|line|line.starts_with("Authored IO ") && line.contains("AnimVarInt")),"Startup still rejects native IO arguments");
        println!("Original Gus startup {parameter} -> {variable}");
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_lua_variables_reach_mickey_and_placed_actor_graphs_with_native_raw_word_semantics(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture("assert(VAR_Cutscene==24 and VAR_Mood_Type==17 and VAR_In_Conversation==72); assert(select('#',AnimVarInt(GetPlayer(),VAR_Cutscene,2))==0)")?;
        let local = world.player_graph.state.index("VAR_Cutscene")?;
        assert_eq!(world.player_graph.state.words()[local], 2);
        assert_eq!(
            world
                .animation_variable_changes
                .last
                .as_ref()
                .unwrap()
                .local,
            Some(local)
        );
        world.run_callback_fixture("BwxVariable=-4.9; AnimVarInt(GetPlayer(),VAR_Cutscene,'BwxVariable'); AnimVarFloat(GetPlayer(),VAR_Dot_Of_Aim_Angle,-0.0)")?;
        assert_eq!(world.player_graph.state.words()[local], (-4i32) as u32);
        let speed = world.player_graph.state.index("VAR_Dot_Of_Aim_Angle")?;
        assert_eq!(world.player_graph.state.words()[speed], (-0f32).to_bits());
        world.run_callback_fixture("AnimVarFloat(GetPlayer(),VAR_Dot_Of_Aim_Angle,0/0)")?;
        assert_eq!(world.player_graph.state.words()[speed], 0);
        world.run_callback_fixture("AnimVarFloat(GetPlayer(),VAR_Dot_Of_Aim_Angle,'not numeric'); AnimVarInt(GetPlayer(),VAR_Dot_Of_Aim_Angle,1065353216)")?;
        assert_eq!(
            world.player_graph.state.words()[speed],
            1f32.to_bits(),
            "Native integer setter stores raw words even on real variables"
        );
        let actor = world
            .authored_havok
            .entities()
            .into_iter()
            .find(|&entity| {
                world
                    .authored_havok
                    .graph(entity)
                    .ok()
                    .and_then(|graph| graph.single("hkbBehaviorGraphStringData").ok())
                    .and_then(|strings| strings["variableNames"].as_array())
                    .is_some_and(|names| {
                        names
                            .iter()
                            .any(|name| name.as_str() == Some("VAR_Mood_Type"))
                    })
            })
            .context("No original actor with mood variable")?;
        let actor_local = world
            .authored_havok
            .graph(actor)?
            .single("hkbBehaviorGraphStringData")?["variableNames"]
            .as_array()
            .unwrap()
            .iter()
            .position(|name| name.as_str() == Some("VAR_Mood_Type"))
            .unwrap();
        world.run_callback_fixture(&format!(
            "assert(select('#',AnimVarInt(GetEntity({:?}),VAR_Mood_Type,2))==0)",
            world.entities[actor].name
        ))?;
        let state = world
            .authored_havok
            .snapshots()
            .into_iter()
            .find(|(entity, _, _, _)| *entity == actor)
            .unwrap();
        assert_eq!(state.2[actor_local], 2);
        assert_eq!(
            world
                .animation_variable_changes
                .last
                .as_ref()
                .unwrap()
                .local,
            Some(actor_local)
        );
        let words = world.player_graph.state.words().to_vec();
        let count = world.animation_variable_changes.requests;
        world.run_callback_fixture("AnimVarInt(GetPlayer(),-1,3); AnimVarInt(GetPlayer(),2147483647,4); AnimVarFloat(GetPlayer(),VAR_Forward_Speed,7)")?;
        assert_eq!(world.player_graph.state.words(), words);
        assert_eq!(world.animation_variable_changes.requests, count + 3);
        assert!(world
            .animation_variable_changes
            .last
            .as_ref()
            .unwrap()
            .local
            .is_none());
        world.run_callback_fixture("AnimVarInt('no such actor',VAR_Cutscene,1); AnimVarInt(GetEntity('gremlinvillage_zonea.CheckPointMarker 01'),VAR_Cutscene,1); assert(not pcall(AnimVarInt,GetPlayer(),VAR_Cutscene))")?;
        assert_eq!(world.animation_variable_changes.requests, count + 3);
        Ok(())
    }
}
