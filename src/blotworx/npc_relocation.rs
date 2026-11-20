//! Ordered native NPC relocation298568/29866c. AI facing/input publication,
//! body warps and child transforms share their actual World owners. This does
//! not supply AI navigation, controller reset or assigned spline rebuilding.
use super::{
    assets::World,
    entity_relocation::{self, Backend, Component, EntityBackend},
    native_hk_transform as hk,
};
use anyhow::{ensure, Context, Result};
use serde::Serialize;

fn rotation_columns(rows: [f32; 9]) -> [f32; 12] {
    std::array::from_fn(|i| {
        if i % 4 == 3 {
            0.
        } else {
            rows[(i % 4) * 3 + i / 4]
        }
    })
}
fn rotation_rows(columns: [f32; 12]) -> [f32; 9] {
    std::array::from_fn(|i| columns[(i % 3) * 4 + i / 3])
}

#[derive(Debug, Serialize)]
pub struct State {
    pub basic_ai_initialized: bool,
    pub facing: [f32; 3],          //BasicAI+30, initialized to zero at2ffcd4
    pub desired_speed: f32,        //BasicAI+44, initialized at2ffcf4
    pub stored_input: [f32; 2],    //BasicMotion+7c/80
    pub filtered_input: [f32; 2],  //BasicMotion+90/94
    pub input_direction: [f32; 3], //BasicMotion+b0
    pub rotation_cache: [f32; 9],  //BasicMotion+308, reset by2974ec/309e28
    pub input_flags: u32,
    pub saved_input_flags:u32,
    pub motion_state: u32,
    pub turn_rates: super::motion_turn_rates::Rates,
    pub input_scale: f32,
    pub moves: u64,
    pub last_target: Option<usize>,
    pub last_probe: Option<f32>,
    pub navigation_position:Option<[f32;3]>,
    /// Raw words retain native NaNs without JSON silently replacing them by null.
    pub navigation_angles:Option<[u32;3]>,
    pub navigation_forward:Option<[u32;3]>,
    pub navigation_speed:Option<f32>,
    pub navigation_floor:Option<(usize,super::ai_runtime_mesh::Floor)>,
    pub navigation_position_updates:u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            basic_ai_initialized: false,
            facing: [0.; 3],
            desired_speed: 0.,
            stored_input: [0.; 2],
            filtered_input: [0.; 2],
            input_direction: [0.; 3],
            rotation_cache: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            input_flags: 3,
            saved_input_flags:3,
            motion_state: 0,
            turn_rates:Default::default(),
            input_scale: 1.,
            moves: 0,
            last_target: None,
            last_probe: None,
            navigation_position:None,
            navigation_angles:None,
            navigation_forward:None,
            navigation_speed:None,
            navigation_floor:None,
            navigation_position_updates:0,
        }
    }
}
fn field<'a>(
    world: &'a World,
    entity: usize,
    class: &str,
    name: &str,
) -> Result<&'a serde_json::Value> {
    world
        .entities
        .get(entity)
        .context("Relocation entity absent")?
        .components
        .iter()
        .find(|c| c.class_name == class)
        .and_then(|c| c.properties.iter().find(|p| p.name == name))
        .map(|p| &p.value)
        .with_context(|| format!("Relocation requires {class}.{name}"))
}
impl World {
    /// REL2fff70 binds BasicMotion, reads actual axis2, publishes facing,
    /// then applies the authored disabled flag. Agent creation300038 is a
    /// separate stage and is not fabricated by this binding initialization.
    pub(super) fn initialize_basic_ai_controller(&mut self,entity:usize)->Result<()> {
        if self.npc_relocations.get(&entity).is_some_and(|s|s.basic_ai_initialized) {return Ok(());}
        let index=self.entities.get(entity).context("BasicAI owner absent")?.components.iter()
            .position(|c|c.class_name=="BasicAIControllerComponent").context("BasicAI component absent")?;
        ensure!(self.entities[entity].components.iter().any(|c|c.class_name=="BasicMotionControllerComponent"),
            "BasicAI motion-controller binding is not connected for this owner");
        self.npc_relocations.entry(entity).or_default();
        // The constructed controller starts enabled, even when its startup
        // flag is true. Let the initial facing reach the bound controller.
        self.component_control.disabled.entry(entity).or_default().insert(index,false);
        self.stop_npc_ai_turn(entity)?;
        let disabled=field(self,entity,"BasicAIControllerComponent","AIController Start Disabled")?
            .as_bool().context("Invalid BasicAI startup disable flag")?;
        self.apply_basic_ai_disabled(entity,index,disabled)?;
        self.npc_relocations.get_mut(&entity).unwrap().basic_ai_initialized=true;
        Ok(())
    }
    pub(super) fn initialize_basic_ai_controllers(&mut self,range:std::ops::Range<usize>) {
        for entity in range {
            if !self.entities[entity].components.iter().any(|c|c.class_name=="BasicAIControllerComponent") {continue;}
            if let Err(error)=self.initialize_basic_ai_controller(entity) {
                self.report.unsupported.push(format!("BasicAI initialization {}: {error:#}",self.entities[entity].name));
            }
        }
    }
    pub(super) fn publish_npc_ai_facing(&mut self, e: usize, forward: [f32; 3]) -> Result<()> {
        let index = self.entities.get(e).context("AI facing owner absent")?
            .components
            .iter()
            .position(|c| c.class_name == "BasicAIControllerComponent")
            .context("AI controller absent")?;
        let authored = field(
            self,
            e,
            "BasicAIControllerComponent",
            "AIController Start Disabled",
        )?
        .as_bool()
        .context("Invalid AI disable flag")?;
        let disabled = self
            .component_control
            .disabled
            .get(&e)
            .and_then(|v| v.get(&index))
            .copied()
            .unwrap_or(authored);
        // Native3001fc ->virtual2f4=56624: disabled or invalid facing declines.
        if disabled
            || !super::ai_controller_input::valid_facing([forward[0], forward[1], forward[2], 0.])
        {
            return Ok(());
        }
        self.npc_relocations.entry(e).or_default().facing = forward;
        self.publish_npc_motion_input(e,forward)
    }
    ///306d84 is also called directly by300ccc after a scalar update. Unlike
    ///3001fc, that path does not recheck BasicAI disablement or vector length.
    pub(super) fn publish_npc_motion_input(&mut self,e:usize,forward:[f32;3])->Result<()> {
        if !self.entities[e]
            .components
            .iter()
            .any(|c| c.class_name == "BasicMotionControllerComponent")
        {
            anyhow::bail!("AI facing requires its bound BasicMotion controller");
        }
        let class = "BasicMotionControllerComponent";
        let mode = i32::try_from(
            field(self, e, class, "InputFilterType")?
                .as_i64()
                .context("Invalid input filter type")?,
        )? as u32;
        let scalar = |name| -> Result<f32> {
            let x = field(self, e, class, name)?
                .as_f64()
                .context("Invalid input filter property")? as f32;
            ensure!(x.is_finite(), "Nonfinite input filter property");
            Ok(x)
        };
        let linear = scalar("InputLinearVelocity")?;
        let length = scalar("InputLengthVelocity")?;
        let angular = scalar("InputRotationVelocity")?;
        let delta = self.frame_clock.frame.scaled;
        let state = self.npc_relocations.get_mut(&e).unwrap();
        let prepared = super::ai_controller_input::prepare(
            0.,
            state.desired_speed,
            state.input_scale,
            state.input_flags,
            state.motion_state,
            mode,
        );
        state.stored_input = prepared.stored;
        state.filtered_input = if prepared.smoothing_mode == 2 {
            super::ai_controller_input::polar(
                state.filtered_input,
                prepared.effective,
                length,
                angular,
                delta,
            )?
        } else {
            super::ai_controller_input::smooth(&prepared, state.filtered_input, linear, delta)?
        };
        state.input_direction = forward;
        Ok(())
    }

    /// REL3004c0 reads the owner's actual forward axis and republishes it.
    /// A completed turn must not cancel concurrent translation or zero velocity.
    pub(super) fn stop_npc_ai_turn(&mut self, entity: usize) -> Result<()> {
        let rotation = self.entities.get(entity).context("Turn owner absent")?.runtime_rotation;
        self.publish_npc_ai_facing(entity, [rotation[2], rotation[5], rotation[8]])
    }

    fn prepare_npc_relocation(&mut self, entity:usize) -> Result<()> {
        ensure!(!self.removed_entities.contains(&entity),"Relocation owner was retired");
        let source = self
            .entities
            .get(entity)
            .context("NPC relocation source absent")?;
        for c in &source.components {
            ensure!(
                !matches!(
                    c.class_name.as_str(),
                    "PhysicsRigidBodyDynamicComponent"
                        | "PhysicsRigidBodyPhantomShapeComponent"
                        | "PhysicsPhantomComponent"
                        | "PhysicsCharacterControllerComponent"
                        | "MickeyPlayerControllerComponent"
                        | "MickeyPlayerMotionControllerComponent"
                        | "SplineTrackerComponent"
                ),
                "NPC relocation requires {} callbacks",
                c.class_name
            );
        }
        self.character_physics
            .get(&entity)
            .context("NPC relocation body missing")?
            .as_ref()
            .map_err(|e| anyhow::anyhow!("NPC relocation body unavailable: {e}"))?;
        self.npc_relocations.entry(entity).or_default();
        self.npc_relocations
            .get_mut(&entity)
            .unwrap()
            .last_probe = None;
        Ok(())
    }
    /// SetTransformation26b3e0 ->2985f4 ->29866c uses the same ordered
    /// component path as MoveToEntity, with reset=1 and no ground probe.
    pub(super) fn transform_npc_pose(&mut self,entity:usize,position:[f32;4],rotation:[f32;9])->Result<()> {
        self.prepare_npc_relocation(entity)?;
        entity_relocation::pose(entity,position,rotation_columns(rotation),1,&mut Bindings{world:self})?;
        let state=self.npc_relocations.get_mut(&entity).unwrap();
        state.moves=state.moves.wrapping_add(1);
        state.last_target=None;
        Ok(())
    }
    pub(super) fn move_npc_to_entity(&mut self, request: entity_relocation::Request) -> Result<()> {
        ensure!(!self.removed_entities.contains(&request.target),"Relocation destination was retired");
        self.prepare_npc_relocation(request.entity)?;
        let result = request.execute(&mut Bindings { world: self });
        if result.is_ok() {
            let state = self.npc_relocations.get_mut(&request.entity).unwrap();
            state.moves = state.moves.wrapping_add(1);
            state.last_target = Some(request.target);
        }
        result
    }
}
struct Bindings<'a> {
    world: &'a mut World,
}
impl Bindings<'_> {
    fn publish(&mut self, e: usize) {
        let entity = &self.world.entities[e];
        for draw in entity.draws.clone() {
            self.world.draws[draw].trans_mat = entity.transform;
        }
    }
}
impl Backend for Bindings<'_> {
    fn component(&mut self, e: usize, kind: Component) -> Result<Option<usize>> {
        let entity = self
            .world
            .entities
            .get(e)
            .context("Relocation component owner absent")?;
        let class = match kind {
            Component::Transform => "JPSTransformationComponent",
            Component::BasicAi => "BasicAIControllerComponent",
            Component::RigidBody => "PhysicsRigidBodyCharacterComponent",
            Component::SceneGraph => "AnimationComponent",
            Component::Lod => "LODComponent",
            Component::Controller => "BasicMotionControllerComponent",
            Component::Spline => "SplineFollowerComponent",
            Component::Phantom => "PhysicsPhantomComponent",
        };
        Ok(entity
            .components
            .iter()
            .any(|c| c.class_name == class)
            .then_some(e))
    }
    fn detach_parent(&mut self, e: usize) -> Result<()> {
        // Native relocation invokes the same3538a8(null) association path.
        self.world.change_transform_parent(e,None)
    }
    fn set_position(&mut self, e: usize, p: [f32; 4]) -> Result<()> {
        ensure!(
            p.iter().all(|x| x.is_finite()),
            "Nonfinite NPC relocation position"
        );
        self.world.entities[e].transform[12..15].copy_from_slice(&p[..3]);
        self.publish(e);
        Ok(())
    }
    fn set_rotation(&mut self, e: usize, r: [f32; 12]) -> Result<()> {
        ensure!(
            r.iter().all(|x| x.is_finite()),
            "Nonfinite NPC relocation rotation"
        );
        let rotation = rotation_rows(r);
        let entity = &mut self.world.entities[e];
        entity.runtime_rotation = rotation;
        entity.transform = super::render_transform::from_entity(
            rotation,
            entity.transform[12..15].try_into().unwrap(),
            entity.authored_scale,
        );
        self.publish(e);
        //2974ec resets the bound controller's temporary rotation matrix even
        // when the outer MoveToEntity request does not request a full reset.
        self.world
            .npc_relocations
            .get_mut(&e)
            .unwrap()
            .rotation_cache = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        Ok(())
    }
    fn update_children(&mut self, e: usize) -> Result<()> {
        super::rotator_runtime::propagate(
            e,
            &self.world.transform_hierarchy,
            &mut self.world.entities,
            &mut self.world.draws,
        )?;
        let mut descendants = std::collections::BTreeSet::from([e]);
        let mut pending = vec![e];
        while let Some(parent) = pending.pop() {
            for (&child, local) in &self.world.transform_hierarchy {
                if local.parent == parent && descendants.insert(child) {
                    pending.push(child);
                }
            }
        }
        // Bone attachments use the current actor pose, independently of mesh
        // visibility; do not translate a wrench by a guessed root offset.
        for child in descendants.into_iter().filter(|&child| child != e) {
            self.world
                .update_skeletal_children_range_strict(child..child + 1)?;
        }
        Ok(())
    }
    fn ai_facing(&mut self, e: usize, forward: [f32; 3]) -> Result<()> {
        self.world.publish_npc_ai_facing(e, forward)
    }
    fn rigid_body_pose(&mut self, e: usize, _: usize) -> Result<()> {
        let entity = &self.world.entities[e];
        let next = hk::from_entity(
            entity.runtime_rotation,
            entity.transform[12..15].try_into().unwrap(),
        );
        self.world.warp_character_body(e, next)
    }
    fn owner_time(&mut self, _: usize) -> Result<f32> {
        Ok(self.world.animation_time)
    }
    fn scene_graph_update(&mut self, e: usize, time: f32, flag: u32) -> Result<()> {
        ensure!(flag == 1, "Unexpected relocation scene graph flag");
        self.world.entities[e].model_asset.as_ref().context("Relocated actor has no model")?;
        self.world.refresh_relocated_model(e,time)
    }
    fn reset_lod(&mut self, e: usize) -> Result<()> {
        self.world.reset_relocated_lod(e)
    }
    fn reset_controller(&mut self, entity: usize, _: u32) -> Result<()> {
        //30a788 clears controller motion after the body warp. Without this,
        // Gus's teleport aborts the arm sequence or retains pre-teleport speed.
        self.world.clear_character_linear_velocity(entity)?;
        let state=self.world.npc_relocations.entry(entity).or_default();
        state.stored_input=[0.;2];
        state.filtered_input=[0.;2];
        //30a788 receives BasicMotion, not BasicAI. Its velocity reset does
        // not overwrite BasicAI+44: the next facing publication must retain
        // the requested walking speed across a relocation/reset.
        state.input_direction=[0.;3];
        state.motion_state=if state.last_probe.is_some_and(|distance|distance<4.) {0} else {2};
        state.rotation_cache=[1.,0.,0.,0.,1.,0.,0.,0.,1.];
        // Native animation-controller reset and support-state parity still
        // need validation; keep the currently requested cutscene animation.
        Ok(())
    }
    fn rebuild_spline(&mut self, e: usize) -> Result<()> {
        //3c1820 calls3c0950 twice. Both null resolved handles skip their
        // entire sampling/cache updates. Assigned splines require that port.
        for name in ["InitialKnotEntity", "LookAtEntity"] {
            ensure!(
                field(self.world, e, "SplineFollowerComponent", name)?.as_u64() == Some(0),
                "Assigned NPC spline/look-at relocation is not connected"
            );
        }
        for name in ["InitialKnotName", "LookAtTarget"] {
            ensure!(
                field(self.world, e, "SplineFollowerComponent", name)?.as_str() == Some(""),
                "Named NPC spline/look-at resolution is not connected"
            );
        }
        Ok(())
    }
    fn phantom_position(&mut self, _: usize, _: [f32; 4]) -> Result<()> {
        anyhow::bail!("NPC phantom relocation is not connected")
    }
}
impl EntityBackend for Bindings<'_> {
    fn entity_position(&mut self, e: usize) -> Result<[f32; 4]> {
        Ok(hk::entity_position_vector(
            self.world
                .entities
                .get(e)
                .context("Relocation destination absent")?
                .transform[12..15]
                .try_into()
                .unwrap(),
        ))
    }
    fn entity_rotation(&mut self, e: usize) -> Result<[f32; 12]> {
        let r = self
            .world
            .entities
            .get(e)
            .context("Relocation destination absent")?
            .runtime_rotation;
        //297214 ->23b998 transposes NiMatrix3 rows into padded columns.
        Ok(rotation_columns(r))
    }
    fn ground_probe(&mut self, e: usize, p: &mut [f32; 4], distance: f32) -> Result<f32> {
        let segment = super::controller_ground::prepare(p, distance);
        let world = &mut self.world;
        let flags = super::physics_filter::QueryFlags {
            bytes: [1, 1, 1, 0, 1],
            excluded_group: 0,
        };
        let collision = &world.collision;
        let filter = &mut world.dynamic_runtime_filter;
        let hits = collision.sphere_hits_game_query(
            segment.start[..3].try_into().unwrap(),
            super::physics::sub(
                segment.end[..3].try_into().unwrap(),
                segment.start[..3].try_into().unwrap(),
            ),
            segment.margin,
            filter,
            Some(flags),
            |shared, candidate| {
                Ok(shared.collision_info(
                    0x0060_0003,
                    collision.simulation_filter_word(candidate.shape)?,
                ))
            },
        )?;
        let mut collector = super::cast_contacts::Collector::default();
        let query = usize::MAX;
        collector.begin_query(query);
        for hit in hits {
            collector.push(super::cast_contacts::RawContact {
                position: [hit.position[0], hit.position[1], hit.position[2], 0.].map(f32::to_bits),
                normal: [hit.normal[0], hit.normal[1], hit.normal[2], hit.fraction]
                    .map(f32::to_bits),
                collidables: [query, hit.shape],
                shape_keys: [u32::MAX, u32::MAX],
            })?;
        }
        let d =
            super::controller_ground::select_collector(*p, distance, &mut collector, |shape| {
                collision.simulation_filter_word(shape)
            })?;
        world.npc_relocations.get_mut(&e).unwrap().last_probe = Some(d);
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_copies_match_original_instructions_including_zero_signs() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/npc_rotation.json"
        ))?;
        for c in fixture["cases"]
            .as_array()
            .context("Missing rotation reference cases")?
        {
            let rows = std::array::from_fn(|i| c["rows"][i].as_f64().unwrap() as f32);
            let columns = rotation_columns(rows);
            assert_eq!(
                columns.map(f32::to_bits).to_vec(),
                c["columns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                rotation_rows(columns).map(f32::to_bits).to_vec(),
                c["roundtrip"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_npc_lua_relocation_keeps_body_children_and_facing_together() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let npc = world
            .entities
            .iter()
            .position(|e| e.name == "gremlinvillage_zonea.Gremlin_Variant4 01")
            .context("Original gremlin absent")?;
        let child = world
            .child_entity_by_name(npc, "GremlinWrench")?
            .context("Original handheld child absent")?;
        let old_child = world.entities[child].transform;
        let old_authored = world.entities[npc].authored_transform;
        let old_record = world.character_physics[&npc]
            .as_ref()
            .unwrap()
            .motion
            .current
            .clone();
        let player = world.player_entity;
        // A non-symmetric rotation catches the NiMatrix3/padded-column conversion.
        world.entities[player].runtime_rotation = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
        let rotation = world.entities[player].runtime_rotation;
        let target: [f32; 3] = world.entities[player].transform[12..15].try_into().unwrap();
        world.run_callback_fixture("assert(select('#',MoveToEntity(GetEntity('gremlinvillage_zonea.Gremlin_Variant4 01'),GetPlayer(),false,false))==0)")?;
        assert_eq!(world.entities[npc].transform[12..15], target);
        assert_eq!(world.entities[npc].runtime_rotation, rotation);
        assert!(super::super::render_transform::is_affine(
            &world.entities[npc].transform
        ));
        assert_eq!(
            world.entities[npc].physics_transform()[15],
            0.,
            "Native query padding must remain native"
        );
        for draw in world.entities[npc].draws.clone() {
            assert!(
                super::super::render_transform::is_affine(&world.draws[draw].trans_mat),
                "Warped actor draw has invalid homogeneous coordinates"
            );
        }
        assert_eq!(world.entities[npc].authored_transform, old_authored);
        assert_ne!(
            world.entities[child].transform, old_child,
            "The attached wrench stayed behind"
        );
        let body = world.character_physics[&npc].as_ref().unwrap();
        assert_eq!(body.motion.current.swept_state().transform[12..15], target);
        assert_eq!(
            body.motion.current.linear_velocity(),
            old_record.linear_velocity()
        );
        assert_eq!(
            body.motion.current.angular_velocity(),
            old_record.angular_velocity()
        );
        assert_eq!(
            world.collision.simulation_pose(body.shape)?.position,
            target
        );
        let state = &world.npc_relocations[&npc];
        assert_eq!(state.moves, 1);
        assert_eq!(state.facing, [1., 0., 0.]);
        assert_eq!(state.input_direction, state.facing);
        assert_eq!(state.last_probe, None);
        assert_eq!(state.filtered_input, [0.; 2]);
        world.publish_character_physics(super::super::STEP)?;
        assert_eq!(
            world.entities[npc].transform[12..15],
            target,
            "Solver publication snapped the actor back"
        );
        world.run_callback_fixture(
            "TeleportToEntity(GetEntity('gremlinvillage_zonea.Gremlin_Variant4 01'),GetPlayer())",
        )?;
        assert_eq!(world.npc_relocations[&npc].moves, 2);
        assert!(world.npc_relocations[&npc].last_probe.is_some());
        let position = world.entities[npc].transform[12..15].to_vec();
        assert_eq!(
            world.character_physics[&npc]
                .as_ref()
                .unwrap()
                .motion
                .current
                .swept_state()
                .transform[12..15],
            position
        );
        world.npc_relocations.get_mut(&npc).unwrap().desired_speed = 0.375;
        world.run_callback_fixture("MoveToEntity(GetEntity('gremlinvillage_zonea.Gremlin_Variant4 01'),GetPlayer(),false,true)")?;
        assert_eq!(world.npc_relocations[&npc].stored_input,[0.;2]);
        assert_eq!(world.npc_relocations[&npc].filtered_input,[0.;2]);
        assert_eq!(world.npc_relocations[&npc].desired_speed,0.375);
        let shape = world.character_physics[&npc].as_ref().unwrap().shape;
        world.collision.set_simulation_velocity(shape, [2., 3., 4.])?;
        let before_pose = world.collision.simulation_pose(shape)?;
        world.stop_npc_ai_turn(npc)?;
        assert_eq!(world.npc_relocations[&npc].desired_speed, 0.375);
        assert_eq!(world.npc_relocations[&npc].input_direction, [1., 0., 0.]);
        assert_eq!(world.npc_relocations[&npc].stored_input[1], 0.375);
        let after_pose = world.collision.simulation_pose(shape)?;
        assert_eq!(after_pose.position, before_pose.position);
        assert_eq!(after_pose.linear, before_pose.linear);
        let socket = world
            .transform_hierarchy
            .get_mut(&child)
            .context("Wrench attachment absent")?
            .socket
            .as_mut()
            .context("Wrench socket absent")?;
        let mode = socket.mode;
        socket.mode = 2; // An explicitly unsupported attachment mode.
        for _ in 0..2 {
            assert!(world.run_callback_fixture("MoveToEntity(GetEntity('gremlinvillage_zonea.Gremlin_Variant4 01'),GetPlayer(),false,false)").is_err(),
                "Deduplicated attachment diagnostics must not turn a failed teleport into success");
        }
        world
            .transform_hierarchy
            .get_mut(&child)
            .unwrap()
            .socket
            .as_mut()
            .unwrap()
            .mode = mode;
        world.retire_entity(npc)?;
        assert!(!world.npc_relocations.contains_key(&npc));
        Ok(())
    }
}
