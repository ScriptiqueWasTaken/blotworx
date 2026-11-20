//! RobotArmGeneral idle requests: Lua5e470 ->5d8c0 ->animation2f3718.
//! Idle writes and animation feedback, not the complete arm attack AI.
use super::assets::World;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub const IDLE_VARIABLE: i32 = 0x17; // Verified common catalog: VAR_AI_Variant.

/// Per-instance robot-arm controls configured by the DBC gameplay scripts.
/// These are runtime AI values: they do not alter the cooked scene component.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Runtime {
    pub attack_types: [AttackType;32],
    pub enabled_mask: u32,
    pub maximum_attack_type: i32,
    pub current_attack_type: i32,
    pub phase: u32,
    #[serde(skip)]
    pub attack_clock: super::locomotion_actions::ClockStamp,
    pub spotlight_entity: Option<usize>,
    #[serde(flatten)]
    pub spotlight: super::robot_arm_spotlight::State,
    pub smash_request: Option<SmashRequest>,
    pub electrocute_request: Option<ElectrocuteRequest>,
    pub sweep_speed_key: Option<String>,
    pub tracking_speed_key: Option<String>,
}
impl Default for Runtime {
    fn default()->Self {Self {attack_types:[AttackType::default();32],enabled_mask:0,maximum_attack_type:-1,current_attack_type:-1,phase:0,attack_clock:Default::default(),spotlight_entity:None,spotlight:Default::default(),smash_request:None,electrocute_request:None,sweep_speed_key:None,tracking_speed_key:None}}
}
impl Runtime {
    pub(super) fn configure(&mut self, ai_type: &str, settings: &mut BTreeMap<String,String>) -> Result<()> {
        if self.sweep_speed_key.is_none() {
            self.sweep_speed_key = Some(super::ai_attributes::resolve(settings,"RobotArmGeneral",ai_type,"SpeedToRotateToPositionForSweeps","180.f")?);
        }
        if self.tracking_speed_key.is_none() {
            self.tracking_speed_key = Some(super::ai_attributes::resolve(settings,"RobotArmGeneral",ai_type,"SpeedToRotateWhenTrackingPlayer","15.f")?);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct AttackType {
    pub angle_degrees: f32,
    pub delay_seconds: f32,
    pub cooldown_seconds: f32,
}
impl Default for AttackType {fn default()->Self {Self {angle_degrees:-1.,delay_seconds:0.5,cooldown_seconds:1.}}}
#[derive(Clone, Debug, serde::Serialize)]
pub struct SmashRequest {
    pub angle_degrees: f32,
    pub destination: Option<usize>,
    pub attack_type: i32,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct ElectrocuteRequest {
    pub attack_type: i32,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Idle {
    pub requested: i32,
    pub current: i32,
    pub force_idle_events: u64,
}
impl Default for Idle {
    fn default() -> Self {
        Self {
            requested: -1,
            current: -1,
            force_idle_events: 0,
        }
    }
}
impl Idle {
    /// REL5d0a4..5d104,5f790. Requests wait for the clip's ChangeIdle event;
    /// an immediate restart would truncate authored fidgets/transitions.
    fn receive(&mut self, event: u32, phase:u32) -> bool {
        if event == 0xc1 {
            self.current = self.requested;
        }
        if event == 0x7c && matches!(phase, 0 | 3) && self.requested != self.current {
            self.current = self.requested;
            self.force_idle_events += 1;
            return true;
        }
        false
    }
}
impl World {
    pub(super) fn robot_arm_control_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        let expected = if name == "RequestRobotArmSmash" {
            4
        } else if name == "RequestRobotArmElectrocute" {
            3
        } else {
            2
        };
        if matches!(
            name,
            "SetRobotArmAttackTypeAngleInfo"
                | "SetRobotArmAttackTypeDelayInfo"
                | "SetRobotArmAttackTypeCooldownInfo"
                | "SetRobotArmAttackTypeEnabled"
        ) {
            if args.len() != 3 {
                return Err(mlua::Error::external(format!(
                    "{name} requires arm, attack type, and value"
                )));
            }
            let value = args.pop_back().unwrap();
            let attack_type = super::score_state::integer_argument(lua, args.pop_back().unwrap())?;
            if !(0..=31).contains(&attack_type) {
                return Err(mlua::Error::external(
                    "Robot-arm attack type is out of range",
                ));
            }
            let owner = self
                .scripts
                .resolve_world_argument(&args.pop_back().unwrap())?
                .filter(|owner| !self.removed_entities.contains(owner) && self.robot_arm_slot(*owner).is_some());
            if let Some(owner) = owner {
                let state = self.robot_arm_controls.entry(owner).or_default();
                let attack = &mut state.attack_types[attack_type as usize];
                match name {
                    "SetRobotArmAttackTypeAngleInfo" => {
                        let angle = lua
                            .coerce_number(value)?
                            .ok_or_else(|| mlua::Error::external("Invalid robot-arm angle"))?
                            as f32;
                        if !angle.is_finite() {
                            return Err(mlua::Error::external("Nonfinite robot-arm angle"));
                        }
                        attack.angle_degrees = angle;
                    }
                    "SetRobotArmAttackTypeDelayInfo" | "SetRobotArmAttackTypeCooldownInfo" => {
                        let delay = lua
                            .coerce_number(value)?
                            .ok_or_else(|| mlua::Error::external("Invalid robot-arm delay"))?
                            as f32;
                        if !delay.is_finite() || delay < 0. {
                            return Err(mlua::Error::external("Invalid robot-arm delay"));
                        }
                        if name=="SetRobotArmAttackTypeCooldownInfo" {attack.cooldown_seconds=delay;} else {attack.delay_seconds=delay;}
                    }
                    _ => {
                        let enabled=super::actor_scripts::native_boolean(lua,value)?;
                        if enabled {state.enabled_mask|=1u32<<attack_type;state.maximum_attack_type=state.maximum_attack_type.max(attack_type);}
                        else {state.enabled_mask&=!(1u32<<attack_type);}
                    },
                }
            }
            return Ok(mlua::MultiValue::new());
        }
        if name == "RequestRobotArmElectrocute" {
            if args.len() != 3 {
                return Err(mlua::Error::external(
                    "RequestRobotArmElectrocute requires arm, attack type, and enabled flag",
                ));
            }
            let enabled = super::actor_scripts::native_boolean(lua, args.pop_back().unwrap())?;
            let attack_type = super::score_state::integer_argument(lua, args.pop_back().unwrap())?;
            if !(-1..=31).contains(&attack_type) {
                return Err(mlua::Error::external(
                    "Robot-arm attack type is out of range",
                ));
            }
            if let Some(owner) = self
                .scripts
                .resolve_world_argument(&args.pop_back().unwrap())?
                .filter(|owner| !self.removed_entities.contains(owner) && self.robot_arm_slot(*owner).is_some())
            {
                self.robot_arm_controls
                    .entry(owner)
                    .or_default()
                    .electrocute_request = Some(ElectrocuteRequest {
                    attack_type,
                    enabled,
                });
                self.robot_arm_request_changed(owner).map_err(mlua::Error::external)?;
            }
            return Ok(mlua::MultiValue::new());
        }
        if args.len() != expected {
            return Err(mlua::Error::external(format!(
                "{name} requires {expected} arguments"
            )));
        }
        let tail = args.pop_back().unwrap();
        let (owner_value, owner) = if name == "RequestRobotArmSmash" {
            let attack_type = super::score_state::integer_argument(lua, tail)?;
            if !(-1..=31).contains(&attack_type) {
                return Err(mlua::Error::external(
                    "Robot-arm attack type is out of range",
                ));
            }
            let target_value = args.pop_back().unwrap();
            let angle_value = args.pop_back().unwrap();
            let angle = lua
                .coerce_number(angle_value)?
                .ok_or_else(|| mlua::Error::external("Invalid robot-arm smash angle"))?
                as f32;
            if !angle.is_finite() {
                return Err(mlua::Error::external("Nonfinite robot-arm smash angle"));
            }
            let owner_value = args.pop_back().unwrap();
            let owner = self
                .scripts
                .resolve_world_argument(&owner_value)?
                .filter(|owner| !self.removed_entities.contains(owner) && self.robot_arm_slot(*owner).is_some());
            if let Some(owner) = owner {
                let destination = self.scripts.resolve_world_argument(&target_value)?;
                self.robot_arm_controls
                    .entry(owner)
                    .or_default()
                    .smash_request = Some(SmashRequest {
                    angle_degrees: angle,
                    destination,
                    attack_type,
                });
                self.robot_arm_request_changed(owner).map_err(mlua::Error::external)?;
            }
            return Ok(mlua::MultiValue::new());
        } else {
            let owner_value = args.pop_back().unwrap();
            let owner = self
                .scripts
                .resolve_world_argument(&owner_value)?
                .filter(|owner| !self.removed_entities.contains(owner) && self.robot_arm_slot(*owner).is_some());
            (owner_value, owner)
        };
        let _ = owner_value;
        if let Some(owner) = owner {
            match name {
                "SetRobotArmSpotlightEntity" => {
                    let target = self.scripts.resolve_world_argument(&tail)?;
                    self.robot_arm_controls.entry(owner).or_default().spotlight_entity = target;
                }
                "SetRobotArmSpotlightDisabled" => {
                    let disabled = super::actor_scripts::native_boolean(lua, tail)?;
                    let status = self.robot_arm_controls.entry(owner).or_default().spotlight.status;
                    self.apply_robot_arm_spotlight(owner, status, disabled).map_err(mlua::Error::external)?;
                }
                "SetRobotArmSpotlightStatus" => {
                    let status = super::score_state::integer_argument(lua, tail)?;
                    let disabled = self.robot_arm_controls.entry(owner).or_default().spotlight.disabled;
                    self.apply_robot_arm_spotlight(owner, status, disabled).map_err(mlua::Error::external)?;
                }
                _ => {
                    return Err(mlua::Error::external(format!(
                        "Unknown robot-arm API {name}"
                    )))
                }
            }
        }
        Ok(mlua::MultiValue::new())
    }
    pub(super) fn receive_robot_arm_animation(&mut self, owner: usize, event: u32) -> Result<()> {
        if self
            .ai_brains
            .get(&owner)
            .is_some_and(|brain| brain.behaviors.iter().any(|name| name == "RobotArmGeneral"))
        {
            //5d0ac..5d0c0 publishes spotlight state before copying the idle
            //request; retain that order if a publication callback reenters.
            if event == 0xc1 {
                let disabled = self.robot_arm_controls.entry(owner).or_default().spotlight.disabled;
                self.apply_robot_arm_spotlight(owner, 1, disabled)?;
            }
            if event == 0xce && self.robot_arm_controls.get(&owner).is_some_and(|state|state.phase==5) {
                self.robot_arm_timer(owner,0.)?;
                self.finish_robot_arm_smash(owner)?;
            }
            let restart = self
                .robot_arm_idle_requests
                .entry(owner)
                .or_default()
                .receive(event, self.robot_arm_controls.get(&owner).map_or(0, |state|state.phase));
            if restart {
                self.send_native_actor_animation_event(owner, 0xc2)?;
            }
        }
        Ok(())
    }
    pub(super) fn robot_arm_idle_lua(
        &mut self,
        lua: &mlua::Lua,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if args.len() != 2 {
            return Err(mlua::Error::external("RequestRobotArmChangeIdle requires entity and idle ID; native argument diagnostics are not reconstructed"));
        }
        let idle = super::score_state::integer_argument(lua, args.pop_back().unwrap())?;
        let owner = self
            .scripts
            .resolve_world_argument(&args.pop_back().unwrap())?
            .filter(|owner| !self.removed_entities.contains(owner) && self.robot_arm_slot(*owner).is_some());
        if let Some(owner) = owner {
            // Native5db04 selects this concrete supplemental behavior, not AIType.
            if self
                .ai_brains
                .get(&owner)
                .is_some_and(|brain| brain.behaviors.iter().any(|name| name == "RobotArmGeneral"))
            {
                self.write_animation_variable(owner, IDLE_VARIABLE, 1, idle as u32)
                    .map_err(mlua::Error::external)?;
                self.robot_arm_idle_requests
                    .entry(owner)
                    .or_default()
                    .requested = idle;
            }
        }
        Ok(mlua::MultiValue::new())
    }
}
impl World {
    pub(super) fn apply_robot_arm_spotlight(&mut self, owner: usize, status: i32, disabled: bool) -> Result<()> {
        if !self.ai_brains.get(&owner).is_some_and(|brain| brain.behaviors.iter().any(|name| name == "RobotArmGeneral")) {
            return Ok(());
        }
        let effective = self.robot_arm_controls.entry(owner).or_default().spotlight.transition(status, disabled);
        if let Some(effective) = effective {
            super::robot_arm_spotlight::publish(effective, &mut SpotlightBinding { world: self, owner })?;
        }
        Ok(())
    }
}
struct SpotlightBinding<'a> { world: &'a mut World, owner: usize }
impl super::robot_arm_spotlight::Host for SpotlightBinding<'_> {
    fn resolve(&mut self) -> Result<Option<usize>> {
        Ok(self.world.robot_arm_controls.get(&self.owner).and_then(|state| state.spotlight_entity)
            .filter(|entity| !self.world.removed_entities.contains(entity)))
    }
    fn parent(&mut self, entity: usize) -> Result<Option<usize>> { self.world.current_transform_parent(entity) }
    fn rotator_enabled(&mut self, entity: Option<usize>, enabled: bool) -> Result<()> {
        if let Some(entity) = entity {
            if let Some(component) = self.world.entities[entity].components.iter().position(|c| c.class_name == "RotatorComponent") {
                self.world.set_component_disabled(entity, component, !enabled)?;
            }
        }
        Ok(())
    }
    fn hide_resolved(&mut self, hidden: bool) -> Result<()> {
        if let Some(entity) = self.resolve()? { self.world.set_entity_hidden(entity, hidden)?; }
        Ok(())
    }
    fn audio(&mut self, entity: usize, event: &str) -> Result<()> { self.world.post_audio_from_game(entity,event); Ok(()) }
    fn color(&mut self, entity: usize, slot: i32, color: [f32;4]) -> Result<()> {
        self.world.set_subgraph_constant(entity, None, slot, color)?;
        Ok(())
    }
}

impl World {
    pub(super) fn start_robot_arm_turn(&mut self, owner: usize, degrees: f32, speed: f32) -> Result<()> {
        super::robot_arm_turn::start(Some(owner), degrees, speed, &mut TurnBinding { world: self, owner })
    }
    /// Native5f2cc completion operation. The full arm phase/task scheduler must
    /// choose its invocation; an arbitrary animation event is not completion.
    pub(super) fn restore_robot_arm_turn(&mut self, owner: usize) -> Result<()> {
        super::robot_arm_turn::restore(Some(owner), &mut TurnBinding { world: self, owner })
    }
}
struct TurnBinding<'a> { world: &'a mut World, owner: usize }
impl TurnBinding<'_> {
    fn id(&self, entity: usize) -> Result<(usize,u32)> {
        let components=self.world.entities[entity].components.iter().filter(|c|c.class_name=="RotatorComponent").collect::<Vec<_>>();
        ensure!(components.len()==1,"Arm turn requires an unambiguous rotator");
        let id=components[0].link_id.to_u32();
        ensure!(self.world.rotators.contains_key(&(entity,id)),"Arm rotator runtime is not initialized");
        Ok((entity,id))
    }
}
impl super::robot_arm_turn::Host for TurnBinding<'_> {
    fn rotator(&mut self, entity: Option<usize>) -> Result<Option<usize>> {
        let Some(entity)=entity else {return Ok(None)};
        if !self.world.entities[entity].components.iter().any(|c|c.class_name=="RotatorComponent") {return Ok(None);}
        self.id(entity)?;
        Ok(Some(entity))
    }
    fn target(&mut self, rotator: usize, entity: Option<usize>) -> Result<()> {
        let id=self.id(rotator)?;
        self.world.rotators.get_mut(&id).unwrap().entity_target=Some(entity);
        Ok(())
    }
    fn speed(&mut self, rotator: usize, degrees: f32) -> Result<()> {self.world.set_rotator_speed(rotator,degrees)}
    fn position_mode(&mut self, rotator: usize, enabled: bool) -> Result<()> {
        let id=self.id(rotator)?;self.world.rotators.get_mut(&id).unwrap().rotate_to_position=enabled;Ok(())
    }
    fn angle(&mut self, rotator: usize, radians: f32) -> Result<()> {
        let (_,id)=self.id(rotator)?;
        ensure!(radians.is_finite(),"Nonfinite arm turn property requires native float storage support");
        let property=self.world.entities[rotator].components.iter_mut().find(|c|c.link_id.to_u32()==id).unwrap()
            .properties.iter_mut().find(|p|p.name=="RotateToPosition").context("Arm turn angle property missing")?;
        ensure!(property.class_name=="Float" && !property.value.is_array(),"Invalid arm turn angle property");
        property.value=serde_json::json!(radians);
        Ok(())
    }
    fn player(&mut self) -> Result<Option<usize>> {self.world.scripts.current_player_entity()}
    fn tracking_speed(&mut self) -> Result<f32> {
        let key=self.world.robot_arm_controls.get(&self.owner).and_then(|state|state.tracking_speed_key.as_ref()).context("Arm tracking-speed attribute is unbound")?;
        let value=self.world.runtime_settings.get(key).context("Bound arm tracking-speed attribute disappeared")?;
        super::config_value::decimal_float(value).map_err(anyhow::Error::msg)
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_robot_arm_turn_uses_owned_rotator_and_bound_tracking_speed() -> Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let name="dbc_1st_zonea_dynamic.Robot_Arm";
        let owner=world.scripts.resolve_name(name).context("Original arm absent")?;
        let id=*world.rotators.keys().find(|(entity,_)|*entity==owner).context("Original arm rotator absent")?;
        let tracking=world.robot_arm_controls[&owner].tracking_speed_key.clone().context("Tracking speed not bound during construction")?;
        let sweep=world.robot_arm_controls[&owner].sweep_speed_key.clone().context("Sweep speed not bound during construction")?;
        ensure!(tracking!=sweep,"Independent native speed attributes were aliased");
        world.run_callback_fixture(&format!("SetPropertyEntity({name:?},'RotateToEntity',GetPlayer())"))?;
        let before=world.rotators[&id].state;
        let rotation=world.entities[owner].runtime_rotation;
        world.start_robot_arm_turn(owner,44.,180.)?;
        let turn=&world.rotators[&id];
        ensure!(turn.entity_target==Some(None) && turn.rotate_to_position,"Fixed-angle turn still tracks an entity");
        ensure!(turn.state.angle==before.angle && turn.state.speed==before.speed,"Turn reset current motor position/velocity");
        ensure!(world.entities[owner].runtime_rotation==rotation,"Turn teleported the arm rotation");
        world.run_callback_fixture(&format!("assert(GetPropertyFloat({name:?},'Max Speed')==180)"))?;
        let property=world.entities[owner].components.iter().find(|c|c.class_name=="RotatorComponent").unwrap()
            .properties.iter().find(|p|p.name=="RotateToPosition").context("Angle property absent")?;
        ensure!(property.value.as_f64().unwrap() as f32==44.*f32::from_bits(0x3c8efa35),"Fixed angle has wrong units");
        // Mutate the already bound live attribute, independently of the sweep
        // attribute, to prove restore reads the canonical tracking handle.
        world.runtime_settings.insert(tracking.clone(),"22.5f".into());
        world.runtime_settings.insert(sweep.clone(),"99.f".into());
        world.restore_robot_arm_turn(owner)?;
        let turn=&world.rotators[&id];
        ensure!(!turn.rotate_to_position && turn.entity_target==Some(world.scripts.current_player_entity()?),"Restore did not bind the current player");
        ensure!(turn.state.angle==before.angle && turn.state.speed==before.speed,"Restore reset motor state");
        world.run_callback_fixture(&format!("assert(GetPropertyFloat({name:?},'Max Speed')==22.5)"))?;
        std::fs::write("target/dbc-actor-fix/turn-world.json",serde_json::to_vec_pretty(&serde_json::json!({"owner":name,"tracking_key":tracking,"sweep_key":sweep,"restored_speed":22.5,"motor_pose_preserved":true,"complete_arm_scheduler":false}))?)?;
        Ok(())
    }
}

#[cfg(test)]
mod spotlight_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_dbc_spotlight_controls_reach_beam_motors_and_materials() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level", &egui::Context::default())?;
        let name = "dbc_1st_zonea_dynamic.Robot_Arm";
        let owner = world.scripts.resolve_name(name).context("Original arm missing")?;
        let beam_name = "dbc_1st_zonea_dynamic.DBC_SpotlightProxyA1 01";
        let beam = world.scripts.resolve_name(beam_name).context("Original beam missing")?;
        let parent = world.current_transform_parent(beam)?.context("Beam parent missing")?;
        let grandparent = world.current_transform_parent(parent)?.context("Beam grandparent missing")?;
        let motors = [parent,grandparent].map(|entity| world.rotators.keys().find(|(owner,_)|*owner==entity).copied());
        ensure!(motors.iter().all(Option::is_some), "Original spotlight tracking rotators absent");
        world.run_callback_fixture(&format!("SetRobotArmSpotlightEntity({name:?},{beam_name:?}); SetRobotArmSpotlightStatus({name:?},2)"))?;
        let color = |world: &World, rgba: [f32;4]| -> Result<()> {
            let packed=super::super::paint_counter::pack_color(rgba)?;
            let objects=world.render_objects.iter().filter(|((entity,_),_)|*entity==beam).collect::<Vec<_>>();
            ensure!(!objects.is_empty(),"Original beam render objects absent");
            ensure!(objects.iter().all(|(_,object)|object.constant_colors[1]==Some(packed)),"Spotlight color did not reach original render objects");
            Ok(())
        };
        color(&world,[1.,0.,0.,1.])?;
        ensure!(motors.iter().flatten().all(|id|world.rotators[id].disabled),"Red attack beam still tracks");
        ensure!(!world.hidden_entities.contains(&beam),"Red beam hidden");
        world.run_callback_fixture(&format!("SetRobotArmSpotlightStatus({name:?},1)"))?;
        color(&world,[0.5;4])?;
        ensure!(motors.iter().flatten().all(|id|!world.rotators[id].disabled),"White beam motors not restored");
        let posts=world.audio_playback.game_post_requests;
        world.run_callback_fixture(&format!("SetRobotArmSpotlightStatus({name:?},1)"))?;
        ensure!(world.audio_playback.game_post_requests==posts,"Unchanged status reposted audio");
        world.run_callback_fixture(&format!("SetRobotArmSpotlightDisabled({name:?},true)"))?;
        ensure!(world.hidden_entities.contains(&beam),"Disabled spotlight remains visible");
        let posts=world.audio_playback.game_post_requests;
        world.run_callback_fixture(&format!("SetRobotArmSpotlightStatus({name:?},2)"))?;
        ensure!(world.hidden_entities.contains(&beam) && world.audio_playback.game_post_requests==posts,"Status bypassed disabled override");
        world.run_callback_fixture(&format!("SetRobotArmSpotlightDisabled({name:?},false)"))?;
        ensure!(!world.hidden_entities.contains(&beam),"Enabling did not restore beam");
        color(&world,[1.,0.,0.,1.])?;
        ensure!(motors.iter().flatten().all(|id|world.rotators[id].disabled),"Enabling forgot retained red status");
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_original_idle() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("BLOTWORX_TEST_DATA required")?;
    let mut world = super::assets::load(
        std::path::Path::new(&root),
        "Levels/DBC_Start_1st.Level",
        &egui::Context::default(),
    )?;
    let owner = world
        .entities
        .iter()
        .position(|entity| entity.name == "dbc_1st_zonea_dynamic.Robot_Arm")
        .context("Original arm missing")?;
    ensure!(
        world.authored_havok.contains(owner),
        "Original arm graph not initialized: {:?}",
        world
            .report
            .unsupported
            .iter()
            .filter(|line| line.starts_with("Authored Havok actor"))
            .collect::<Vec<_>>()
    );
    ensure!(
        world
            .animation_events
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?
            .variable_name(IDLE_VARIABLE)
            == Some("VAR_AI_Variant"),
        "Native idle variable differs"
    );
    let name = world.entities[owner].name.clone();
    let mut writes = Vec::new();
    for idle in [-1, 0, 1, 2, 3, i32::MIN, i32::MAX] {
        world.run_callback_fixture(&format!(
            "assert(select('#',RequestRobotArmChangeIdle(GetEntity({name:?}),{idle}))==0)"
        ))?;
        ensure!(
            world
                .robot_arm_idle_requests
                .get(&owner)
                .is_some_and(|state| state.requested == idle),
            "Idle request not retained"
        );
        let write = world
            .animation_variable_changes
            .last
            .as_ref()
            .context("Idle did not reach native animation setter")?;
        let local = write
            .local
            .context("Original arm does not map VAR_AI_Variant")?;
        ensure!(
            write.event.index == IDLE_VARIABLE && write.event.bits == idle as u32,
            "Idle setter arguments differ"
        );
        let word = world.authored_havok.variable_word(owner, local)?;
        ensure!(
            word == idle as u32,
            "Actual arm graph variable was not changed"
        );
        writes.push(serde_json::json!({"idle":idle,"local":local,"graph_word":word}));
    }
    let before = world.robot_arm_idle_requests.clone();
    world.run_callback_fixture(
        "RequestRobotArmChangeIdle('no such arm',2); RequestRobotArmChangeIdle(GetPlayer(),2)",
    )?;
    ensure!(
        before == world.robot_arm_idle_requests,
        "Missing/non-arm owner changed arm requests"
    );
    let mut poses = Vec::new();
    let mut state_reports = Vec::new();
    for idle in [0, 1] {
        world.run_callback_fixture(&format!(
            "RequestRobotArmChangeIdle(GetEntity({name:?}),{idle})"
        ))?;
        for _ in 0..180 {
            let (events, unsupported) = match world.authored_havok.step_and_export(owner) {
                Ok(step) => step,
                Err(error) => {
                    std::fs::write(
                        format!("target/robot-arm-idle-{idle}-snapshot.json"),
                        serde_json::to_vec_pretty(&world.authored_havok.snapshot(owner)?)?,
                    )?;
                    return Err(error.context(format!("Arm idle{idle} graph update")));
                }
            };
            world.deliver_actor_exports(owner, events, unsupported);
        }
        std::fs::write(
            format!("target/robot-arm-idle-{idle}-snapshot.json"),
            serde_json::to_vec_pretty(&world.authored_havok.snapshot(owner)?)?,
        )?;
        let state = world
            .robot_arm_idle_requests
            .get(&owner)
            .context("Arm idle state disappeared")?;
        ensure!(
            state.current == idle && state.force_idle_events > 0,
            "Arm did not re-enter requested idle on its actual animation feedback: {state:?}"
        );
        state_reports.push(serde_json::to_value(state)?);
        let mut draws = world.draws.clone();
        world.apply_model_poses(&mut draws)?;
        poses.push(
            world.entities[owner]
                .draws
                .clone()
                .map(|draw| draws[draw].v_data.clone())
                .collect::<Vec<_>>(),
        );
    }
    ensure!(
        poses[0] != poses[1],
        "Idle0 and idle1 still render the same arm pose"
    );
    std::fs::write(
        "target/robot-arm-idle-validation.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"owner":name,"writes":writes,"feedback_states":state_reports,"different_rendered_poses":true,"graph_advanced_ticks":360,"native_entry":"REL5e470/5d8c0;5d0c8/5f790","complete_robot_ai":false,"original_runtime_pose_parity":false}),
        )?,
    )?;
    eprintln!("RobotArm idle: original graph writes, actual ChangeIdle feedback and different rendered poses PASS");
    Ok(())
}
