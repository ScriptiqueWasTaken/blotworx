//! Generated retail character capsules (REL37a760) and their owned bodies.
//! Controller gravity uses authored settings and solver-contact admission;
//! native state transitions, navigation and checkSupport parity remain open.
use super::{
    assets::World, body_motion::BodyMotion, native_hk_transform as transform,
    native_motion_record::MotionRecord,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub struct Body {
    pub shape: usize,
    pub owner: usize,
    pub motion: BodyMotion,
    pub orientation: [f32; 4],
    pub material: super::body_material::Coefficients,
    pub navigation_height: f32,
    gravity: Option<Gravity>,
    built_in_agent_profile: Option<&'static super::ai_agent_profiles::Definition>,
}
impl Body {
    pub(super) fn navigation_definition(&self)->Option<&'static super::ai_agent_profiles::Definition> {self.built_in_agent_profile}
    pub(super) fn has_navigation_profile(&self)->bool {self.built_in_agent_profile.is_some()}
}
#[derive(Clone, Copy)]
struct Gravity {
    up: f32,
    down: f32,
    terminal: f32,
    slope_cos: f32,
}
impl Gravity {
    fn integrate(self, velocity: f32, dt: f32) -> f32 {
        (velocity
            + if velocity > 0. {
                self.up * dt
            } else {
                self.down * dt
            })
        .max(self.terminal)
    }
}

/// REL29680c selects a configured height profile, not a world-space enum value.
pub(super) fn height(profile: i64, settings: &BTreeMap<String, String>) -> Result<f32> {
    let (name, default) = match profile {
        0 => ("ShortEntityHeight", 1.5),
        1 => ("MediumEntityHeight", 2.5),
        2 => ("TallEntityHeight", 3.5),
        3 => ("PlayerEntityHeight", 1.2),
        _ => anyhow::bail!("Unsupported native Entity Height profile {profile}"),
    };
    let value = settings
        .get(name)
        .map(|s| s.parse::<f64>().with_context(|| format!("Invalid {name}")))
        .transpose()?
        .map_or(default, |v| v as f32);
    ensure!(value.is_finite(), "Nonfinite {name}");
    Ok(value)
}
/// Preserve fnmsubs/fadds/fsubs rounding at37a824..844, including a reversed
/// segment when the authored height is less than twice the radius.
pub fn capsule_endpoints(radius: f32, height: f32) -> Result<[[f32; 3]; 2]> {
    ensure!(
        radius.is_finite() && radius > 0. && height.is_finite(),
        "Invalid generated character dimensions"
    );
    let span = (-2.0f32).mul_add(radius, height) - 0.1f32;
    let low = 0.05f32 + radius;
    let high = low + span;
    ensure!(
        low.is_finite() && high.is_finite(),
        "Generated character capsule overflow"
    );
    Ok([[0., low, 0.], [0., high, 0.]])
}

impl World {
    /// The compatibility solver and retained native motion must agree before
    /// a later motion4 -> motion7 replacement republishes the stored record.
    pub(super) fn clear_character_linear_velocity(&mut self,index:usize)->Result<()> {
        let body=self.character_physics.get_mut(&index).context("Character reset body missing")?
            .as_mut().map_err(|error|anyhow::anyhow!("Character reset body unavailable: {error}"))?;
        self.collision.set_simulation_velocity(body.shape,[0.;3])?;
        body.motion.current.set_linear_velocity([0.;4]);
        Ok(())
    }
    /// The body-only stage reached by37cd80 ->802b7bd4. Entity, children,
    /// AI and controller publications are the ordered caller's responsibility.
    pub(super) fn warp_character_body(
        &mut self,
        index: usize,
        next: transform::Transform,
    ) -> Result<()> {
        let body = self
            .character_physics
            .get_mut(&index)
            .context("Character warp requires its owned body")?
            .as_mut()
            .map_err(|error| anyhow::anyhow!("Character warp body unavailable: {error}"))?;
        body.motion.current.set_transform(next)?;
        body.orientation = body.motion.current.swept_state().orientation;
        self.collision.set_shape_transform(
            body.shape,
            next[12..15].try_into().unwrap(),
            body.orientation,
        )?;
        Ok(())
    }
    pub(super) fn character_physics_summary(&self) -> serde_json::Value {
        serde_json::json!({"bodies":self.character_physics.iter().map(|(&entity,result)|match result {
            Ok(body)=>serde_json::json!({"entity":entity,"name":self.entities[entity].name,"owner":body.owner,
                "shape":body.shape,"motion_type":body.motion.current.motion_type(),"quality":body.motion.quality,
                "filter_info":self.collision.simulation_filter(body.shape),
                "controller_suspended":self.basic_motion_suspended(entity),
                "built_in_agent_profile":body.built_in_agent_profile,"navigation_agent_constructed":false,
                "controller_gravity":body.gravity.map(|g|serde_json::json!({"up":g.up,"down":g.down,"terminal":g.terminal,
                    "support_admission":"Box3D touching walkable contacts; native checkSupport parity unverified"})),
                "position":body.motion.current.swept_state().transform[12..15],
                "in_world":self.collision.owner_in_world(body.owner).ok()}),
            Err(error)=>serde_json::json!({"entity":entity,"name":self.entities[entity].name,"error":error}),
        }).collect::<Vec<_>>(),"ai_navigation_updates_connected":false,"reference_runtime_validated":false})
    }
    pub(super) fn initialize_character_bodies(&mut self, range: std::ops::Range<usize>) {
        for entity in range {
            if entity == self.player_entity
                || !self.entities[entity]
                    .components
                    .iter()
                    .any(|c| c.class_name == "PhysicsRigidBodyCharacterComponent")
            {
                continue;
            }
            if self.character_physics.contains_key(&entity) {
                continue;
            }
            let result = self
                .admit_generated_character(entity)
                .map_err(|error| format!("{error:#}"));
            if let Err(error) = &result {
                self.report.unsupported.push(format!(
                    "Character body {}:{}: {error}",
                    self.entities[entity].scene, self.entities[entity].name
                ));
            }
            self.character_physics.insert(entity, result);
        }
    }
    fn admit_generated_character(&mut self, index: usize) -> Result<Body> {
        let entity = &self.entities[index];
        let components = entity
            .components
            .iter()
            .filter(|c| c.class_name == "PhysicsRigidBodyCharacterComponent")
            .collect::<Vec<_>>();
        ensure!(
            components.len() == 1,
            "Character requires exactly one resolved body component"
        );
        let component = components[0];
        let value = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("Character lacks {name}"))
        };
        let integer = |name: &str| {
            value(name)?
                .as_i64()
                .with_context(|| format!("Invalid character {name}"))
        };
        let scalar = |name: &str| {
            value(name)?
                .as_f64()
                .map(|v| v as f32)
                .with_context(|| format!("Invalid character {name}"))
        };
        let authored = integer("Collision Shape")?;
        let override_type = self
            .runtime_settings
            .get("CharacterPhysicsType")
            .map(|s| s.parse::<i64>())
            .transpose()
            .context("Invalid CharacterPhysicsType")?
            .unwrap_or(-1);
        let selector = if override_type == -1 {
            authored
        } else {
            override_type
        };
        ensure!(selector==0,"NPC character shape selector {selector} requires its native constructor; no replacement shape was fabricated");
        let radius =
            super::motion_ground::radius(integer("Entity Radius")?, &self.runtime_settings)?;
        let navigation_height=height(integer("Entity Height")?, &self.runtime_settings)?;
        let endpoints = capsule_endpoints(radius,navigation_height)?;
        let mut mass = scalar("Character Mass")?;
        if mass == 0. {
            mass = 1000.;
        }
        let friction = scalar("Friction")?;
        ensure!(
            mass.is_finite() && mass > 0. && friction.is_finite() && friction >= 0.,
            "Invalid character mass/friction"
        );
        let layer = integer("Collision Layer")?;
        ensure!((0..32).contains(&layer), "Invalid character layer");
        let real = value("Is Real")?
            .as_bool()
            .context("Invalid character real flag")?;
        //379d8c..da0 makes real character bodies painted regardless of the property.
        let painted = real
            || value("Is Painted")?
                .as_bool()
                .context("Invalid character paint flag")?;
        // Characters initialize before the static filter publication pass.
        // Reserve past source IDs as well as already admitted runtime bodies.
        let reserved = self
            .report
            .collision_parts
            .iter()
            .map(|part| part.body_filter_info >> 17)
            .chain(
                self.dynamic_bodies
                    .iter()
                    .flat_map(|source| source.geometry.as_ref().ok())
                    .flat_map(|parts| parts.iter())
                    .map(|part| part.body_filter_info >> 17),
            )
            .max()
            .unwrap_or(0);
        let group = self.collision.maximum_filter_group().max(reserved) + 1;
        ensure!(group < 32768, "Character filter groups exhausted");
        let filter = super::body_filter::pack(super::body_filter::Fields {
            layer: layer as u32,
            group,
            sub_a: 1,
            sub_b: 0,
            painted: u8::from(painted),
            real: u8::from(real),
        });
        let built_in_agent_profile=if entity.components.iter().any(|c|c.class_name=="BasicAIControllerComponent") {
            let name=super::ai_agent_profiles::descriptor(i32::try_from(integer("Entity Radius")?)?,i32::try_from(integer("Entity Height")?)?,1);
            super::ai_agent_profiles::Definition::built_in_biped(&name)
        } else {None};
        let initial = entity.physics_transform();
        let orientation = transform::rotation_quaternion_raw(&initial)?;
        let gravity = entity
            .components
            .iter()
            .find(|c| c.class_name == "BasicMotionControllerComponent")
            .map(|controller| -> Result<Gravity> {
                let read = |name: &str| {
                    controller
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .and_then(|p| p.value.as_f64())
                        .map(|v| v as f32)
                        .with_context(|| format!("NPC controller lacks valid {name}"))
                };
                let up = read("Character Gravity Up")?;
                let down = read("Character Gravity Down")?;
                let terminal = read("TerminalSpeed")?;
                let slope = scalar("Max Slope")?;
                ensure!(
                    [up, down, terminal, slope].iter().all(|v| v.is_finite())
                        && up <= 0.
                        && down <= 0.
                        && terminal < 0.
                        && (0.0..=90.0).contains(&slope),
                    "Invalid NPC gravity/slope settings"
                );
                Ok(Gravity {
                    up,
                    down,
                    terminal,
                    slope_cos: slope.to_radians().cos().max(0.),
                })
            })
            .transpose()?;
        let position = [initial[12], initial[13], initial[14]];
        let owner = self.collision.next_owner()?;
        let shape = self
            .collision
            .add_static_capsule(endpoints, radius, owner)?;
        self.collision.set_simulation_filter(shape, filter)?;
        let result = (|| -> Result<Body> {
            self.collision
                .set_shape_transform(shape, position, orientation)?;
            self.collision
                .set_native_body_mass(&[shape], 1. / mass, [0.; 3], [0.; 3])?;
            //2399cc/802f5668 uses character motion7, quality8 and zero local COM.
            let mut record = MotionRecord([0; 0x120]);
            record.0[8] = 7;
            record.0[0xcc..0xd0].copy_from_slice(&(1. / mass).to_be_bytes());
            record.0[0xbc] = super::native_motion_limits::encode(20.)?;
            record.0[0xbd] = super::native_motion_limits::encode(200.)?;
            record.0[0x11e..].copy_from_slice(&0x3f80u16.to_be_bytes());
            record.0[0xb8..0xbc].copy_from_slice(&0.05f32.to_be_bytes());
            record.set_swept_state(&super::native_motion_step::MotionState {
                transform: initial,
                previous_center: [position[0], position[1], position[2], 0.],
                center: [position[0], position[1], position[2], 0.],
                previous_orientation: orientation,
                orientation,
                local_center: [0.; 4],
                delta_angle: [0.; 4],
            });
            let material = super::body_material::Coefficients {
                friction,
                restitution: 0.,
            };
            self.collision
                .set_simulation_body(&[shape], &record, material, true)?;
            if self.hidden_entities.contains(&index) {
                self.collision.set_owner_in_world(owner, false)?;
            }
            self.report.geometry.push(serde_json::json!({"kind":"generated_character_body","entity":entity.name,
            "owner":owner,"shape":shape,"endpoints":endpoints,"radius":radius,"mass":mass,"filter_word":filter,
            "quality":8,"world_gravity_applied":false,"ai_controller_updates_connected":false,"reference_runtime_validated":false}));
            let message="Generated NPC capsules own shared solver bodies and motion suspension; AI/navigation velocities, per-character contact callbacks, native admission/task order and solver parity remain incomplete.";
            if !self.report.unsupported.iter().any(|line| line == message) {
                self.report.unsupported.push(message.into());
            }
            Ok(Body {
                shape,
                owner,
                motion: BodyMotion::new(record, 8, None)?,
                orientation,
                material,
                navigation_height,
                gravity,
                built_in_agent_profile,
            })
        })();
        if result.is_err() {
            self.collision.retire_owner(owner)?;
        }
        result
    }
    pub(super) fn suspend_character_motion(
        &mut self,
        entity: usize,
        suspended: bool,
    ) -> Result<()> {
        let body = self
            .character_physics
            .get_mut(&entity)
            .context("Character motion requires its owned body")?
            .as_mut()
            .map_err(|e| anyhow::anyhow!("Character motion body unavailable: {e}"))?;
        let change = body
            .motion
            .replace(if suspended { 4 } else { 7 }, 0x3f800000)?;
        if change == super::body_motion::Change::Unchanged {
            return Ok(());
        }
        if body.motion.current.motion_type() == 7 {
            let (inertia, mass) = body.motion.current.solver_inverse_mass()?;
            let center = body.motion.current.swept_state().local_center;
            self.collision.set_native_body_mass(
                &[body.shape],
                mass,
                inertia,
                [center[0], center[1], center[2]],
            )?;
        }
        self.collision.set_simulation_body(
            &[body.shape],
            &body.motion.current,
            body.material,
            true,
        )?;
        Ok(())
    }
    pub(super) fn publish_character_physics(&mut self, dt: f32) -> Result<()> {
        for (&index, result) in &mut self.character_physics {
            let Ok(body) = result else { continue };
            if !self.collision.owner_in_world(body.owner)? {
                continue;
            }
            let pose = self.collision.simulation_pose(body.shape)?;
            let mut state = body.motion.current.swept_state();
            state.previous_center = state.center;
            state.previous_center[3] = self.dynamic_runtime_time;
            state.previous_orientation = state.orientation;
            state.center = [pose.center[0], pose.center[1], pose.center[2], 1. / dt];
            state.orientation = pose.orientation;
            state.transform =
                transform::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], pose.position);
            transform::set_rotation(&mut state.transform, pose.orientation);
            body.motion.current.set_swept_state(&state);
            body.motion.current.set_linear_velocity([
                pose.linear[0],
                pose.linear[1],
                pose.linear[2],
                0.,
            ]);
            body.motion.current.set_angular_velocity([
                pose.angular[0],
                pose.angular[1],
                pose.angular[2],
                0.,
            ]);
            let entity = &mut self.entities[index];
            entity.runtime_rotation = std::array::from_fn(|i| state.transform[(i % 3) * 4 + i / 3]);
            entity.transform =
                super::render_transform::from_havok(&state.transform, entity.authored_scale);
            for draw in entity.draws.clone() {
                self.draws[draw].trans_mat = entity.transform;
            }
            super::rotator_runtime::accept_physics_pose(
                index,
                &mut self.transform_hierarchy,
                &mut self.entities,
                &mut self.draws,
            )?;
        }
        self.publish_ai_actor_positions()
    }
    /// Controller gravity is independent of AI steering/stun. Motion7 disables
    /// world gravity, so omitting this stage leaves knockback airborne forever.
    /// Contact admission is a Box3D compatibility path; native air/ground state
    /// transitions and full checkSupport parity remain separate requirements.
    pub(super) fn prepare_character_gravity(&mut self, dt: f32) -> Result<()> {
        ensure!(
            dt.is_finite() && dt > 0.,
            "Invalid character gravity timestep"
        );
        for (&entity, result) in &self.character_physics {
            let Ok(body) = result else { continue };
            let Some(gravity) = body.gravity else {
                continue;
            };
            if body.motion.current.motion_type() != 7
                || self.basic_motion_suspended(entity) == Some(true)
                || (self.demo_npc_active(entity) && self.ai_brains.get(&entity).is_some_and(|brain|brain.motion_suspended))
                || !self.collision.owner_in_world(body.owner)?
            {
                continue;
            }
            let mut velocity = self.collision.simulation_pose(body.shape)?.linear;
            // BasicAI's zero input is a stop request, not an unconstrained
            // dynamic body. The combat adapter already drives its actors, but
            // passive NPCs otherwise retain horizontal contact impulses forever.
            // Leave explicit knockback alone until its reaction has completed.
            let reacting = self.demo_story.ai.stunned_until.get(&entity)
                .is_some_and(|until| *until > self.animation_time);
            if !reacting && self.npc_relocations.get(&entity).is_some_and(|state|
                state.desired_speed == 0. && state.filtered_input == [0.; 2]) {
                velocity[0] = 0.;
                velocity[2] = 0.;
            }
            if self
                .collision
                .simulation_body_supported(body.shape, gravity.slope_cos)?
            {
                self.collision.set_simulation_velocity(body.shape, velocity)?;
                continue;
            }
            velocity[1] = gravity.integrate(velocity[1], dt);
            self.collision
                .set_simulation_velocity(body.shape, velocity)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn dbc_gus_idle_stops_contact_drift_without_cancelling_knockback() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root), "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        let entity = world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus absent")?;
        world.set_entity_motion_enabled(entity, true)?;
        let shape = world.character_physics[&entity].as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))?.shape;
        let controller = world.npc_relocations.get_mut(&entity)
            .context("Gus controller absent")?;
        controller.desired_speed = 0.;
        controller.filtered_input = [0.; 2];
        world.collision.set_simulation_velocity(shape, [3., 0., -2.])?;
        world.prepare_character_gravity(super::super::STEP)?;
        let stopped = world.collision.simulation_pose(shape)?.linear;
        assert_eq!([stopped[0], stopped[2]], [0., 0.], "Idle Gus retained contact drift");
        world.demo_story.ai.stunned_until.insert(entity, world.animation_time + 1.);
        world.collision.set_simulation_velocity(shape, [3., 0., -2.])?;
        world.prepare_character_gravity(super::super::STEP)?;
        let reacting = world.collision.simulation_pose(shape)?.linear;
        assert_eq!([reacting[0], reacting[2]], [3., -2.], "Idle stop cancelled knockback");
        Ok(())
    }
    #[test]
    fn controller_gravity_uses_authored_ascent_descent_and_terminal_speed() {
        let gravity = Gravity {
            up: -30.,
            down: -20.,
            terminal: -40.,
            slope_cos: 0.5,
        };
        assert_eq!(gravity.integrate(3., 1. / 30.), 2.);
        assert_eq!(gravity.integrate(-3., 1. / 30.), -3. - 20. / 30.);
        assert_eq!(gravity.integrate(-39.9, 1. / 30.), -40.);
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_spatter_knockback_gravity_returns_to_the_floor_while_stunned() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        let name = "gremlinvillage_zonea.SpatterMelee 02";
        let entity = world
            .entities
            .iter()
            .position(|e| e.name == name)
            .context("Spatter missing")?;
        world.run_callback_fixture(&format!("EnableMotion(GetEntity({name:?}))"))?;
        let shape = world.character_physics[&entity]
            .as_ref()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .shape;
        for _ in 0..30 {
            world.advance_dynamic_physics(super::super::STEP)?;
        }
        let initial = world.collision.simulation_pose(shape)?.position;
        let gravity = world.character_physics[&entity]
            .as_ref()
            .unwrap()
            .gravity
            .context("Spatter controller gravity absent")?;
        assert_eq!(
            (gravity.up, gravity.down, gravity.terminal),
            (-30., -20., -40.)
        );
        world.demo_spin_hit(entity, initial)?;
        // Isolate vertical response from lateral steering and ledge traversal.
        // This owned-body regression is not a continuous gameplay capture.
        world
            .collision
            .set_simulation_velocity(shape, [0., 3., 0.])?;
        world.advance_dynamic_physics(super::super::STEP)?;
        let first = world.collision.simulation_pose(shape)?;
        assert!(
            (first.linear[1] - 2.).abs() < 0.00001,
            "Gravity was skipped during stun: {:?}",
            first.linear
        );
        let mut peak = first.position[1];
        let mut samples = vec![first.position[1]];
        for tick in 1..90 {
            world.advance_dynamic_physics(super::super::STEP)?;
            let pose = world.collision.simulation_pose(shape)?;
            peak = peak.max(pose.position[1]);
            if tick % 3 == 0 {
                samples.push(pose.position[1]);
            }
        }
        let final_pose = world.collision.simulation_pose(shape)?;
        assert!(
            peak > initial[1] + 0.08,
            "Knockback never rose: {initial:?}, {peak}"
        );
        assert!(
            (final_pose.position[1] - initial[1]).abs() < 0.08,
            "Spatter failed to land: {initial:?} -> {:?}",
            final_pose.position
        );
        assert!(world
            .collision
            .simulation_body_supported(shape, gravity.slope_cos)?);
        std::fs::write(
            "target/demo-spatter-gravity-owned.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "entity":name,"initial":initial,"peak_y":peak,"final":final_pose.position,
            "first_vertical_velocity":first.linear[1],"sample_y_every_three_ticks":samples,
            "ticks":90,"scope":"Isolated vertical knockback on original NPC/body/floor; no original-runtime parity comparison"}))?,
        )?;
        world.set_entity_motion_enabled(entity, false)?;
        let before = world.collision.simulation_pose(shape)?;
        world.prepare_character_gravity(super::super::STEP)?;
        assert_eq!(
            world.collision.simulation_pose(shape)?.linear,
            before.linear
        );
        Ok(())
    }
    #[test]
    fn capsule_dimensions_match_original_scalar_instruction_results() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/character_capsule.json"
        ))?;
        for sample in data["samples"].as_array().unwrap() {
            let endpoints = capsule_endpoints(
                sample["radius"].as_f64().unwrap() as f32,
                sample["height"].as_f64().unwrap() as f32,
            )?;
            assert_eq!(
                endpoints[0][1].to_bits(),
                sample["low_bits"].as_u64().unwrap() as u32,
                "{sample}"
            );
            assert_eq!(
                endpoints[1][1].to_bits(),
                sample["high_bits"].as_u64().unwrap() as u32,
                "{sample}"
            );
        }
        assert!(capsule_endpoints(0., 1.).is_err());
        assert!(capsule_endpoints(f32::NAN, 1.).is_err());
        assert_eq!(height(0, &BTreeMap::new())?, 1.5);
        assert_eq!(
            height(
                3,
                &BTreeMap::from([("PlayerEntityHeight".into(), "1.75".into())])
            )?,
            1.75
        );
        assert!(height(4, &BTreeMap::new()).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_spatter_motion_owns_capsule_restores_mass_and_retires_cleanly() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let name = "gremlinvillage_zonea.SpatterMelee 01";
        let index = world
            .entities
            .iter()
            .position(|entity| entity.name == name)
            .context("Original Spatter absent")?;
        let body = world
            .character_physics
            .get(&index)
            .context("Spatter has no owned body")?
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        let (shape, owner) = (body.shape, body.owner);
        assert_eq!(
            world.collision_entity(owner).map(|e| e.id),
            Some(world.entities[index].id)
        );
        assert_eq!(
            body.motion.current.motion_type(),
            4,
            "Original map-load DisableMotion did not reach the NPC body"
        );
        assert_eq!(world.basic_motion_suspended(index), Some(true));
        assert!(
            !world
                .report
                .unsupported
                .iter()
                .any(|error| error.contains("OnMapLoad")
                    && error.contains(name)
                    && error.contains("motion")),
            "{:?}",
            world.report.unsupported
        );
        world.run_callback_fixture(&format!("EnableMotion(GetEntity({name:?}))"))?;
        assert_eq!(
            world.character_physics[&index]
                .as_ref()
                .unwrap()
                .motion
                .current
                .motion_type(),
            7
        );
        assert_eq!(world.basic_motion_suspended(index), Some(false));
        let record = world.character_physics[&index]
            .as_ref()
            .unwrap()
            .motion
            .current
            .clone();
        assert_eq!(record.solver_inverse_mass()?, ([0.; 3], 0.02));
        // Repeating commands must not overwrite the retained inertia model.
        world.run_callback_fixture(&format!("EnableMotion(GetEntity({name:?})); DisableMotion(GetEntity({name:?})); DisableMotion(GetEntity({name:?})); EnableMotion(GetEntity({name:?}))"))?;
        assert_eq!(
            world.character_physics[&index]
                .as_ref()
                .unwrap()
                .motion
                .current
                .solver_inverse_mass()?,
            ([0.; 3], 0.02)
        );
        world.set_entity_hidden(index, true)?;
        assert!(!world.collision.owner_in_world(owner)?);
        world.set_entity_hidden(index, false)?;
        assert!(world.collision.owner_in_world(owner)?);
        world.advance_dynamic_physics(1. / 30.)?;
        let pose = world.collision.simulation_pose(shape)?;
        assert_eq!(&world.entities[index].transform[12..15], &pose.position);
        let authored = world.entities[index].authored_transform;
        let before = world.character_physics[&index]
            .as_ref()
            .unwrap()
            .motion
            .current
            .clone();
        {
            let body = world
                .character_physics
                .get_mut(&index)
                .unwrap()
                .as_mut()
                .unwrap();
            let mut state = body.motion.current.swept_state();
            state.previous_center[3] = -12.5;
            state.center[3] = 30.;
            state.delta_angle = [1., 2., 3., 4.];
            body.motion.current.set_swept_state(&state);
        }
        world.dynamic_runtime_time = 91.;
        let owner_pose = world.entities[index].transform;
        // The complete root relocation is still correctly refused because
        // this retail NPC owns attached children. Verify that guard, then the
        // actual body-only stage without pretending child propagation ran.
        assert!(world
            .relocate_entity(
                index,
                [12., 20., -10.],
                [0., 0., 1., 0., 1., 0., -1., 0., 0.]
            )
            .is_err());
        world.warp_character_body(
            index,
            transform::from_entity([0., 0., 1., 0., 1., 0., -1., 0., 0.], [12., 20., -10.]),
        )?;
        let body = world.character_physics[&index].as_ref().unwrap();
        let moved = body.motion.current.swept_state();
        assert_eq!(moved.previous_center, [12., 20., -10., -12.5]);
        assert_eq!(moved.center, [12., 20., -10., 0.]);
        assert_eq!(moved.previous_orientation, moved.orientation);
        assert_eq!(moved.delta_angle, [0.; 4]);
        assert_eq!(
            body.motion.current.linear_velocity(),
            before.linear_velocity()
        );
        assert_eq!(
            body.motion.current.angular_velocity(),
            before.angular_velocity()
        );
        assert_eq!(
            world.collision.simulation_pose(shape)?.position,
            [12., 20., -10.]
        );
        assert_eq!(world.entities[index].authored_transform, authored);
        assert_eq!(
            world.entities[index].transform, owner_pose,
            "Body stage must not masquerade as complete owner relocation"
        );
        world.retire_entity(index)?;
        assert!(!world.character_physics.contains_key(&index));
        assert_eq!(world.basic_motion_suspended(index), None);
        assert!(world.set_entity_motion_enabled(index, true).is_err());
        Ok(())
    }
}
