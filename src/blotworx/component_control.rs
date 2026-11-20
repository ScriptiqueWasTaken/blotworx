//! Native Enable/Disable dispatch, REL262230. This flag is separate from
//! visibility and motion. Constructor flags and full update admission remain open.
use super::assets::World;
use anyhow::{ensure, Context, Result};
#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub disabled: std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, bool>>,
    pub ai_commands: u64,
    pub ai_brain_notifications: u64,
    pub last_ai_owner: Option<usize>,
    pub named_requests: u64,
    pub named_matches: u64,
    pub last_named: Option<NamedRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_basic_ai_initialization_publishes_facing_before_start_disabled()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let name="gremlinvillage_zonea.Gremlin_Variant4 01";
        let entity=world.scripts.resolve_name(name).context("Original Gremlin absent")?;
        assert!(world.npc_relocations.get(&entity).is_some_and(|s|s.basic_ai_initialized),
            "Normal World initialization never bound BasicAI");
        let index=world.entities[entity].components.iter().position(|c|c.class_name=="BasicAIControllerComponent").unwrap();
        world.entities[entity].components[index].properties.iter_mut()
            .find(|p|p.name=="AIController Start Disabled").context("Startup flag missing")?.value=serde_json::Value::Bool(true);
        world.npc_relocations.remove(&entity);
        world.component_control.disabled.entry(entity).or_default().remove(&index);
        world.entities[entity].runtime_rotation=[0.,0.,1.,0.,1.,0.,-1.,0.,0.];
        let shape={let body=world.character_physics.get_mut(&entity).unwrap().as_mut()
            .map_err(|e|anyhow::anyhow!("{e}"))?;
            body.motion.current.set_linear_velocity([2.,5.,-3.,0.]);body.shape};
        world.collision.set_simulation_velocity(shape,[2.,5.,-3.])?;
        let before=world.collision.simulation_pose(shape)?;
        world.initialize_basic_ai_controller(entity)?;
        let state=&world.npc_relocations[&entity];
        assert!(state.basic_ai_initialized);
        assert_eq!(state.facing,[1.,0.,0.]);assert_eq!(state.input_direction,[1.,0.,0.]);
        assert!(world.ai_component_disabled(entity,"BasicAIControllerComponent"));
        assert_eq!(world.collision.simulation_pose(shape)?.linear,[0.;3]);
        assert_eq!(world.collision.simulation_pose(shape)?.position,before.position);
        assert_eq!(world.character_physics[&entity].as_ref().unwrap().motion.current.linear_velocity(),[0.;4]);
        world.collision.set_simulation_velocity(shape,[9.,8.,7.])?;
        world.initialize_basic_ai_controller(entity)?;
        assert_eq!(world.collision.simulation_pose(shape)?.linear,[9.,8.,7.],"Initialization replayed on an existing component");
        let native=super::super::component_names::runtime_name("BasicAIControllerComponent").context("Native AI name missing")?;
        world.run_callback_fixture(&format!("EnableComponent(GetEntity({name:?}),{native:?})"))?;
        assert!(!world.ai_component_disabled(entity,"BasicAIControllerComponent"));
        assert_eq!(world.collision.simulation_pose(shape)?.linear,[9.,8.,7.]);
        world.run_callback_fixture(&format!("DisableComponent(GetEntity({name:?}),{native:?})"))?;
        assert_eq!(world.collision.simulation_pose(shape)?.linear,[0.;3]);
        assert_eq!(world.character_physics[&entity].as_ref().unwrap().motion.current.linear_velocity(),[0.;4]);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_ai_disable_clears_solver_and_retained_launch_velocity() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level", &egui::Context::default())?;
        let name = "gremlinvillage_zonea.SpatterMelee 01";
        let entity = world.entities.iter().position(|e| e.name == name).context("Spatter missing")?;
        let component = world.entities[entity].components.iter().position(|c|
            c.class_name == "BasicAIControllerComponent").context("AI controller missing")?;
        let property = world.entities[entity].components[component].properties.iter_mut()
            .find(|p| p.name == "AIController Start Disabled").context("Authored flag missing")?;
        property.value = serde_json::Value::Bool(true);
        world.component_control.disabled.entry(entity).or_default().remove(&component);
        assert!(world.ai_component_disabled(entity, "BasicAIControllerComponent"));
        world.run_callback_fixture(&format!("AIController_SetDisabled(GetEntity({name:?}), false)"))?;
        assert!(!world.ai_component_disabled(entity, "BasicAIControllerComponent"));
        world.run_callback_fixture(&format!("EnableMotion(GetEntity({name:?}))"))?;
        let body = world.character_physics.get_mut(&entity).unwrap().as_mut()
            .map_err(|e|anyhow::anyhow!("{e}"))?;
        let shape = body.shape;
        body.motion.current.set_linear_velocity([2., 5., -3., 0.]);
        world.collision.set_simulation_velocity(shape, [2., 5., -3.])?;
        world.run_callback_fixture(&format!("AIController_SetDisabled(GetEntity({name:?}), true)"))?;
        assert_eq!(world.collision.simulation_pose(shape)?.linear, [0.;3]);
        assert_eq!(world.character_physics[&entity].as_ref().unwrap().motion.current.linear_velocity(), [0.;4]);
        world.run_callback_fixture(&format!("DisableMotion(GetEntity({name:?})); EnableMotion(GetEntity({name:?}))"))?;
        assert_eq!(world.collision.simulation_pose(shape)?.linear, [0.;3]);
        Ok(())
    }
    #[test]
    fn native_names_cover_observed_scene_types_and_match_recovery() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/component_names.json"
        ))?;
        for (class, value) in fixture["components"]
            .as_object()
            .context("Missing name fixture")?
        {
            assert_eq!(
                super::super::component_names::runtime_name(class),
                value["name"].as_str()
            );
        }
        let observed: serde_json::Value =
            serde_json::from_str(include_str!("../data/components.json"))?;
        for class in observed["components"]
            .as_object()
            .context("Missing observed components")?
            .keys()
        {
            ensure!(
                super::super::component_names::runtime_name(class).is_some(),
                "Unmapped observed class {class}"
            );
        }
        assert_eq!(
            super::super::component_names::runtime_name("JPSTransformationComponent"),
            Some("JPSTransformation")
        );
        assert_eq!(
            super::super::component_names::runtime_name("AnimationComponent"),
            Some("Animation")
        );
        assert_eq!(
            super::super::component_names::runtime_name("MoodComponent"),
            Some("Mood")
        );
        assert_eq!(
            super::super::component_names::runtime_name("PatrolNodeComponent"),
            Some("PatrolNode")
        );
        Ok(())
    }
    #[test]
    fn named_selection_uses_first_exact_runtime_name() -> Result<()> {
        let classes = [
            "JPSTransformationComponent",
            "AnimationComponent",
            "UsableComponent",
            "UsableComponent",
        ];
        assert_eq!(find_named(classes, b"Usable")?, Some(2));
        for name in [
            b"usable".as_slice(),
            b"UsableComponent",
            b"Transformation",
            b"Unknown",
            b"",
        ] {
            assert_eq!(find_named(classes, name)?, None);
        }
        assert_eq!(
            find_named(["UsableComponent", "UnreconstructedComponent"], b"Usable")?,
            Some(0)
        );
        assert!(find_named(["UnreconstructedComponent", "UsableComponent"], b"Usable").is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_named_components_share_flags_without_hiding_or_stopping_bodies() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let entity = world
            .entities
            .iter()
            .position(|entity| {
                entity
                    .components
                    .iter()
                    .any(|c| c.class_name == "UsableComponent")
                    && entity
                        .components
                        .iter()
                        .any(|c| c.class_name == "AnimationComponent")
            })
            .context("Original Usable missing")?;
        let component = world.entities[entity]
            .components
            .iter()
            .position(|c| c.class_name == "UsableComponent")
            .unwrap();
        let name = world.entities[entity].name.clone();
        let hidden = world.hidden_entities.clone();
        let random = world.game_random;
        let draws = world.draws.len();
        world.run_callback_fixture(&format!(
            "assert(select('#',DisableComponent(GetEntity({name:?}), 'Usable'))==0)"
        ))?;
        assert_eq!(world.component_control.disabled[&entity][&component], true);
        world.run_callback_fixture(&format!(
            "assert(select('#',EnableComponent({name:?}, 'Usable\\0ignored'))==0)"
        ))?;
        assert_eq!(world.component_control.disabled[&entity][&component], false);
        let flags = world.component_control.disabled.clone();
        world.run_callback_fixture(&format!("DisableComponent({name:?},'usable'); DisableComponent({name:?},true); DisableComponent(nil,'Usable'); DisableComponent('BwxMissing','Usable'); DisableComponent({name:?},42); DisableComponent({name:?},'UsableComponent'); DisableComponent()"))?;
        assert_eq!(world.component_control.disabled, flags);
        assert_eq!(world.hidden_entities, hidden);
        assert_eq!(world.game_random, random);
        assert_eq!(world.draws.len(), draws);
        // The callback diagnoses excess args but reads from the top, like retail.
        world.run_callback_fixture(&format!("DisableComponent('ignored',{name:?},'Usable')"))?;
        assert_eq!(world.component_control.disabled[&entity][&component], true);
        // Only the first matching component is changed, including duplicates.
        let duplicate = world.entities[entity].components.len();
        let duplicate_component = world.entities[entity].components[component].clone();
        world.entities[entity].components.push(duplicate_component);
        world.run_callback_fixture(&format!("EnableComponent({name:?},'Usable')"))?;
        assert!(!world.component_control.disabled[&entity].contains_key(&duplicate));
        world.entities[entity].components.pop();
        // Whole-entity Disable is ignored for the current main player.
        let player_flags = world
            .component_control
            .disabled
            .get(&world.player_entity)
            .cloned();
        world.run_callback_fixture("Disable(GetPlayer())")?;
        assert_eq!(
            world.component_control.disabled.get(&world.player_entity),
            player_flags.as_ref()
        );
        let npc = world
            .entities
            .iter()
            .position(|entity| entity.name == "gremlinvillage_zonea.SpatterMelee 01")
            .context("Original Spatter missing")?;
        world.run_callback_fixture(
            "EnableMotion(GetEntity('gremlinvillage_zonea.SpatterMelee 01'))",
        )?;
        assert_eq!(world.basic_motion_suspended(npc), Some(false));
        world.run_callback_fixture("DisableComponent(GetEntity('gremlinvillage_zonea.SpatterMelee 01'),'Basic Motion Controller')")?;
        assert_eq!(world.basic_motion_suspended(npc), Some(true));
        assert_eq!(
            world.character_physics[&npc]
                .as_ref()
                .unwrap()
                .motion
                .current
                .motion_type(),
            7,
            "Named controller disable must not silently become DisableMotion"
        );
        world.run_callback_fixture("DisableMotion(GetEntity('gremlinvillage_zonea.SpatterMelee 01')); EnableComponent(GetEntity('gremlinvillage_zonea.SpatterMelee 01'),'Basic Motion Controller')")?;
        assert_eq!(world.basic_motion_suspended(npc), Some(false));
        assert_eq!(
            world.character_physics[&npc]
                .as_ref()
                .unwrap()
                .motion
                .current
                .motion_type(),
            4
        );
        // A known name with an unverified override fails, rather than writing a
        // guessed flag. No broad component callback support is claimed.
        let previous = world.component_control.disabled.clone();
        assert!(world
            .run_callback_fixture(&format!("DisableComponent({name:?},'Animation')"))
            .is_err());
        assert_eq!(world.component_control.disabled, previous);
        world.retire_entity(entity)?;
        assert!(!world.component_control.disabled.contains_key(&entity));
        world.run_callback_fixture(&format!("DisableComponent({name:?},'Usable')"))?;
        Ok(())
    }
}
#[derive(Debug, serde::Serialize)]
pub struct NamedRequest {
    pub entity: Option<usize>,
    pub name: String,
    pub disabled: bool,
    pub component: Option<usize>,
}
///312d74 enumerates component interfaces in their original order, compares
/// interned names exactly, invokes the FIRST match and returns no Lua values.
fn find_named<'a>(
    classes: impl IntoIterator<Item = &'a str>,
    name: &[u8],
) -> Result<Option<usize>> {
    for (index, class) in classes.into_iter().enumerate() {
        let native = super::component_names::runtime_name(class).with_context(|| {
            format!("Native component instance name for {class} is not reconstructed")
        })?;
        if native.as_bytes() == name {
            return Ok(Some(index));
        }
    }
    Ok(None)
}
fn accepts(class: &str) -> Result<()> {
    ensure!(
        matches!(
            class,
            "JPSTransformationComponent"
                | "InputOutputComponent"
                | "GameEventsComponent"
                | "TemplateSpawnerComponent"
                | "ShooterComponent"
                | "RotatorComponent"
        ),
        "Native enable-interface classification/callback for {class} is not implemented"
    );
    Ok(())
}
impl World {
    /// BasicAI virtual setter300524. Publish the flag before touching the
    /// optional body, preserving native nontransactional callback order.
    pub(super) fn apply_basic_ai_disabled(&mut self,entity:usize,index:usize,disabled:bool)->Result<()> {
        ensure!(self.entities.get(entity).and_then(|e|e.components.get(index))
            .is_some_and(|c|c.class_name=="BasicAIControllerComponent"),"Invalid BasicAI disable receiver");
        self.component_control.disabled.entry(entity).or_default().insert(index,disabled);
        if disabled && self.character_physics.get(&entity).is_some_and(|body|body.is_ok()) {
            self.clear_character_linear_velocity(entity)?;
        }
        Ok(())
    }
    /// Canonical base byte20 bit80, including BasicMotionController's inherited
    ///3bafc setter. AIBrain byte48 and physics body motion are separate states.
    pub(super) fn set_component_disabled(
        &mut self,
        entity: usize,
        index: usize,
        disabled: bool,
    ) -> Result<()> {
        ensure!(
            !self.removed_entities.contains(&entity),
            "Component target was retired"
        );
        let class = &self
            .entities
            .get(entity)
            .context("Unknown component owner")?
            .components
            .get(index)
            .context("Unknown component index")?
            .class_name;
        if class=="BasicAIControllerComponent" {
            return self.apply_basic_ai_disabled(entity,index,disabled);
        }
        if !self.demo_story.enabled
            && !matches!(
                class.as_str(),
                "UsableComponent" | "BasicMotionControllerComponent"
            )
        {
            accepts(class)?;
        }
        self.component_control
            .disabled
            .entry(entity)
            .or_default()
            .insert(index, disabled);
        if class == "RotatorComponent" {
            let id = self.entities[entity].components[index].link_id.to_u32();
            if let Some(rotator) = self.rotators.get_mut(&(entity, id)) {
                rotator.disabled = disabled;
            }
        }
        if class == "TemplateSpawnerComponent" {
            self.spawners.entry(entity).or_default().disabled = Some(disabled);
        }
        Ok(())
    }
    pub(super) fn ai_component_disabled(&self, entity:usize, class:&str)->bool {
        let Some((index, component)) = self.entities.get(entity).and_then(|e|
            e.components.iter().enumerate().find(|(_, c)| c.class_name == class)) else {
            return false;
        };
        self.component_control.disabled.get(&entity).and_then(|flags| flags.get(&index))
            .copied().unwrap_or_else(|| {
                class == "BasicAIControllerComponent" && component.properties.iter()
                    .find(|p| p.name == "AIController Start Disabled")
                    .and_then(|p| p.value.as_bool()).unwrap_or(false)
            })
    }
    ///184874 controls both interfaces;300e00 controls only the AI controller.
    /// These flags are not AIBrain's cutscene byte48 or the global manager gate.
    pub(super) fn ai_disabled_lua(&mut self,lua:&mlua::Lua,api:&str,mut args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        if args.len()!=2 {self.report.warnings.push(format!("{api}: expected two arguments; preserving native stack-pop order"));}
        let disabled=super::actor_scripts::native_boolean(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?;
        let Some(entity)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|e|!self.removed_entities.contains(e)) else {return Ok(mlua::MultiValue::new())};
        self.component_control.ai_commands=self.component_control.ai_commands.wrapping_add(1);
        self.component_control.last_ai_owner=Some(entity);
        if let Some(index)=self.entities[entity].components.iter().position(|c|c.class_name=="BasicAIControllerComponent") {
            self.apply_basic_ai_disabled(entity,index,disabled).map_err(mlua::Error::external)?;
        }
        if api=="AI_SetDisabled" {
            if let Some(index)=self.entities[entity].components.iter().position(|c|c.class_name=="AIBrainComponent") {
                let previous=self.component_control.disabled.entry(entity).or_default().insert(index,disabled).unwrap_or(false);
                if previous!=disabled {
                    //2e0288 dispatches ordinary action7/8 synchronously.
                    self.cutscene_actions.dispatch(super::action_event_queue::ActionEvent {
                        source:Some(entity),header:None,action:if disabled {7}else{8},vector_bits:[0;4],reference:None
                    },&mut self.entity_event_registry,&mut self.ai_brains).map_err(mlua::Error::external)?;
                    self.component_control.ai_brain_notifications=self.component_control.ai_brain_notifications.wrapping_add(1);
                }
                if let Some(brain)=self.ai_brains.get_mut(&entity) {brain.reevaluation_requested=true;}
                let limitation="AI disable flags and ordinary action7/8 notifications are connected. Native brain target/root reset, complete behavior selection and AI-controller output reset parity remain incomplete.";
                if !self.report.unsupported.iter().any(|s|s==limitation) {self.report.unsupported.push(limitation.into());}
            }
        }
        Ok(mlua::MultiValue::new())
    }
    pub(super) fn basic_motion_suspended(&self, entity: usize) -> Option<bool> {
        let index = self
            .entities
            .get(entity)?
            .components
            .iter()
            .position(|c| c.class_name == "BasicMotionControllerComponent")?;
        self.component_control
            .disabled
            .get(&entity)?
            .get(&index)
            .copied()
    }
    pub(super) fn named_component_lua(
        &mut self,
        lua: &mlua::Lua,
        api: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if args.len() != 2 {
            self.report.warnings.push(format!("{api}: native argument count diagnostic ({} arguments); continued with source stack order",args.len()));
        }
        //278968 reads/pops the name first;278bec then reads/pops the entity.
        // lua_isstring accepts numbers. Non-string values supply empty text;
        // C-string interning ignores bytes after the first NUL.
        let name = lua.coerce_string(args.pop_back().unwrap_or(mlua::Value::Nil))?;
        let bytes = name.as_ref().map(|s| s.as_bytes());
        let name = bytes
            .as_deref()
            .unwrap_or_default()
            .split(|b| *b == 0)
            .next()
            .unwrap_or_default();
        let entity = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|entity| !self.removed_entities.contains(entity));
        let disabled = api == "DisableComponent";
        self.component_control.named_requests =
            self.component_control.named_requests.wrapping_add(1);
        self.component_control.last_named = Some(NamedRequest {
            entity,
            name: String::from_utf8_lossy(name).into_owned(),
            disabled,
            component: None,
        });
        if let Some(entity) = entity {
            let index = find_named(
                self.entities[entity]
                    .components
                    .iter()
                    .map(|c| c.class_name.as_str()),
                name,
            )
            .map_err(mlua::Error::external)?;
            if let Some(index) = index {
                self.set_component_disabled(entity, index, disabled)
                    .map_err(mlua::Error::external)?;
                self.component_control.named_matches =
                    self.component_control.named_matches.wrapping_add(1);
                self.component_control
                    .last_named
                    .as_mut()
                    .unwrap()
                    .component = Some(index);
                let limitation="Named component commands update verified native disabled callbacks. Usable selection/input/indicator lifecycle and common component-update admission remain incomplete.";
                if !self
                    .report
                    .unsupported
                    .iter()
                    .any(|line| line == limitation)
                {
                    self.report.unsupported.push(limitation.into());
                }
            }
        }
        Ok(mlua::MultiValue::new())
    }
    pub(super) fn set_entity_disabled(&mut self, entity: usize, disabled: bool) -> Result<()> {
        ensure!(
            !self.removed_entities.contains(&entity),
            "Enable/Disable target was retired"
        );
        let target = self
            .entities
            .get(entity)
            .context("Unknown Enable/Disable target")?;
        //262280..298: whole-entity Disable declines the current main player.
        // Named component commands have no such guard.
        if disabled && entity == self.player_entity {
            return Ok(());
        }
        let components = target
            .components
            .iter()
            .map(|component| component.class_name.clone())
            .collect::<Vec<_>>();
        let shooter = components.iter().any(|class| class == "ShooterComponent");
        // Native snapshots count, then visits interfaces in component order.
        for (index, class) in components.iter().enumerate() {
            if !self.demo_story.enabled {
                accepts(class)?;
            }
            if class == "TemplateSpawnerComponent" && shooter && !disabled {
                continue;
            }
            self.set_component_disabled(entity, index, disabled)?;
        }
        let limitation="Native component disabled flags are connected for classified callbacks; constructor enablement, common component-update admission and automatic spawner scheduling remain incomplete.";
        if !self
            .report
            .unsupported
            .iter()
            .any(|line| line == limitation)
        {
            self.report.unsupported.push(limitation.into());
        }
        Ok(())
    }
}
