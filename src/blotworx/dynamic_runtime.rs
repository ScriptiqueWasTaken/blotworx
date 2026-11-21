//! Shared-world Box3D compatibility simulation. This is an integration stage,
//! not a claim of Havok solver, controller interaction or lifecycle parity.
use super::{assets::World, body_filter, native_hk_transform as transform};
use anyhow::{Context, Result};
use epic_mickey_lib_rs::nif_file::{Matrix3, NiTransform, Vector3};
use std::collections::BTreeMap;
#[cfg(test)]
mod random_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_level_loading_preserves_supplied_shared_random_stream() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let ctx = egui::Context::default();
        let baseline = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &ctx,
        )?;
        let mut supplied = super::super::game_random::State::default();
        for _ in 0..17 {
            supplied.draw();
        }
        let mut expected = supplied;
        for _ in 0..baseline.game_random.draws {
            expected.draw();
        }
        let loaded = super::super::assets::load_with_random(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &ctx,
            None,
            None,
            supplied,
        )?;
        assert_eq!(
            loaded.game_random, expected,
            "Loading restarted or lost the runtime stream"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_body_constructor_consumes_shared_rng_for_nonzero_velocity_axes() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let source = *world
            .dynamic_constructions
            .iter()
            .find(|(_, state)| {
                state.completed
                    && state
                        .bodies
                        .iter()
                        .any(|body| !matches!(body.motion.current.motion_type(), 4 | 5))
            })
            .context("Expected an initialized source body")?
            .0;
        // Reconstruct one owned runtime record with controlled initial velocity
        // parameters; source assets and installed collision remain untouched.
        let old = world.dynamic_constructions.remove(&source).unwrap();
        let parameters = super::super::dynamic_body::InitialVelocity {
            minimum: [1., -2., 3.],
            maximum: [5., 0., 3.],
            minimum_speed: 0.,
        };
        world.dynamic_bodies[source].initial_velocity = Ok(parameters);
        let before = world.game_random;
        let mut expected_random = before;
        let mut expected_velocity = None;
        for body in &old.bodies {
            if !matches!(body.motion.current.motion_type(), 4 | 5) {
                expected_velocity = Some(parameters.sample(|| expected_random.draw() as u16)?);
            }
        }
        world.construct_dynamic_sources(source..source + 1)?;
        let state = world
            .dynamic_constructions
            .get(&source)
            .context("Randomized source rejected")?;
        anyhow::ensure!(state.completed, "Randomized constructor incomplete");
        assert_eq!(world.game_random, expected_random);
        assert!(world.game_random.draws > before.draws);
        for body in &state.bodies {
            assert_eq!(body.linear_velocity(), expected_velocity.unwrap());
        }
        Ok(())
    }
}
impl World {
    /// Resolve the same model pose used for rendering, including exporter and
    /// entity scale in translations. Physics retains a rigid, unscaled frame.
    pub(super) fn rigid_model_node_frame(
        &self,
        entity: usize,
        node: u32,
        animated: bool,
    ) -> Result<transform::Transform> {
        let hierarchy = self
            .entity_bound_hierarchy(entity)?
            .context("Rigid-body binding has no model")?;
        let empty = BTreeMap::new();
        let poses = if !animated {
            &empty
        } else if let Some(actor) = self
            .actor_playback
            .as_ref()
            .filter(|actor| actor.entity == entity)
        {
            &actor.pose.transforms
        } else if let Some(pose) = self
            .actors
            .get(&entity)
            .and_then(|actor| actor.pose.as_ref())
        {
            &pose.transforms
        } else if let Some(animation) = self.model_animation.get(&entity) {
            &animation.poses
        } else {
            &empty
        };
        let frames = hierarchy.base_world_transforms_with_poses(
            Some(&self.rigid_model_parent(entity)),
            poses,
        )?;
        Ok(Self::rigid_frame(
            frames.get(&node).context("Bound rigid-body node missing")?,
        ))
    }

    fn rigid_model_parent(&self, entity: usize) -> NiTransform {
        let owner = &self.entities[entity];
        NiTransform {
            rotation: Matrix3 {
                rows: std::array::from_fn(|r| {
                    std::array::from_fn(|c| owner.runtime_rotation[r * 3 + c])
                }),
            },
            translation: Vector3 {
                x: owner.transform[12],
                y: owner.transform[13],
                z: owner.transform[14],
            },
            scale: owner.authored_scale,
        }
    }

    fn rigid_frame(frame: &NiTransform) -> transform::Transform {
        transform::from_entity(
            std::array::from_fn(|i| frame.rotation.rows[i / 3][i % 3]),
            [
                frame.translation.x,
                frame.translation.y,
                frame.translation.z,
            ],
        )
    }

    fn bind_dynamic_model_nodes(
        &self,
        source: usize,
        state: &mut super::dynamic_body::Construction,
    ) -> Result<()> {
        let imported = &self.dynamic_bodies[source];
        let Some(graph) = self.entity_bound_hierarchy(imported.entity)? else {
            return Ok(());
        };
        let parts = imported
            .geometry
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        if state.layouts.iter().all(|layout| {
            parts[layout.parts[0]]
                .body_name
                .as_deref()
                .is_none_or(str::is_empty)
        }) {
            return Ok(());
        }
        let components = &self.entities[imported.entity].components;
        let is_mover = components
            .iter()
            .find(|c| c.link_id.to_u32() == imported.component)
            .and_then(|c| c.properties.iter().find(|p| p.name == "Is Mover"))
            .and_then(|p| p.value.as_bool())
            .unwrap_or(false);
        // A sequence-driven mover's Havok hull is exported in its starting
        // pose. The NIF locals can instead contain a later animation frame.
        // Sample the authored start ID even if map-load logic changed it.
        let reference_pose = if is_mover {
            (|| {
                let resources = self.actor_sequences.get(&imported.entity)?;
                let bindings = self.actor_bindings.get(&imported.entity)?;
                let actor = components
                    .iter()
                    .find(|c| c.class_name == "AnimationComponentGB")?;
                let id = actor
                    .properties
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case("Active Sequence ID"))
                    .and_then(|p| p.value.as_u64())
                    .unwrap_or(0) as u32;
                resources.get(id)?;
                bindings.sample(id, 0.).ok()
            })()
        } else {
            None
        };
        let empty = BTreeMap::new();
        let frames = graph.base_world_transforms_with_poses(
            Some(&self.rigid_model_parent(imported.entity)),
            reference_pose.as_ref().map_or(&empty, |pose| &pose.transforms),
        )?;
        for (index, layout) in state.layouts.iter().enumerate() {
            let Some(name) = parts[layout.parts[0]]
                .body_name
                .as_deref()
                .filter(|n| !n.is_empty())
            else {
                continue;
            };
            let mut node = None;
            for root in graph.roots.iter().flatten() {
                if let Some(found) = graph.find_named_node(*root, name)? {
                    node = Some(found);
                    break;
                }
            }
            if let Some(node) = node {
                let frame = Self::rigid_frame(
                    frames.get(&node).context("Bound rigid-body node missing")?,
                );
                // Preserve the authored collision-to-node offset: collision
                // pivots need not be identical to the rendered mesh pivot.
                state.node_bindings[index] = Some(super::dynamic_body::NodeBinding {
                    node,
                    offset: transform::relative(&frame, &layout.initial_transform),
                });
            }
        }
        Ok(())
    }
    /// Construct a newly imported range through the original component setup.
    /// Initial loading and runtime factories share this path. World admission
    /// follows construction and must not be replaced with query-only geometry.
    pub fn construct_dynamic_sources(&mut self, sources: std::ops::Range<usize>) -> Result<()> {
        anyhow::ensure!(
            sources.start <= sources.end && sources.end <= self.dynamic_bodies.len(),
            "Invalid dynamic source range"
        );
        anyhow::ensure!(
            sources
                .clone()
                .all(|source| !self.dynamic_constructions.contains_key(&source)),
            "Dynamic source already constructed"
        );
        for source in sources {
            let entity = self.dynamic_bodies[source].entity;
            let result = (|| -> Result<Option<super::dynamic_body::Construction>> {
                let mut state = match super::dynamic_body::Construction::new(
                    &self.dynamic_bodies[source],
                    &self.entities[entity].physics_transform(),
                )? {
                    Some(state) => state,
                    None => return Ok(None),
                };
                // Nonzero opaque owner token allocated from this World entity's
                // stable index. No Wii address is dereferenced or fabricated.
                let owner = entity.checked_add(1).context("Entity token overflow")?;
                let mut random = self.game_random;
                let initialized =
                    self.initialize_dynamic_system_records(source, &mut state, Some(owner), || {
                        random.draw() as u16
                    });
                // Keep consumed draws even when a later constructor stage fails.
                self.game_random = random;
                if let Err(error) = initialized {
                    self.report.unsupported.push(format!(
                        "Dynamic constructor {}: {error:#}",
                        self.entities[entity].name
                    ));
                }
                self.bind_dynamic_model_nodes(source, &mut state)?;
                Ok(Some(state))
            })();
            match result {
                Ok(Some(state)) => {
                    self.dynamic_constructions.insert(source, state);
                }
                Ok(None) => {}
                Err(error) => self.report.unsupported.push(format!(
                    "Dynamic construction {}: {error:#}",
                    self.entities[entity].name
                )),
            }
        }
        Ok(())
    }

    /// Admit newly constructed sources without reattaching existing bodies.
    /// next_owner is shared with the world's other collider classes.
    pub fn admit_dynamic_sources(
        &mut self,
        sources: std::ops::Range<usize>,
        next_owner: &mut usize,
    ) -> Result<()> {
        *next_owner = (*next_owner).max(self.collision.next_owner()?);
        anyhow::ensure!(
            sources.start <= sources.end && sources.end <= self.dynamic_bodies.len(),
            "Invalid admission source range"
        );
        anyhow::ensure!(
            self.dynamic_constructions
                .range(sources.clone())
                .all(|(_, state)| state.bodies.iter().all(|body| body.shapes.is_empty())),
            "Dynamic source already attached"
        );
        if self.dynamic_queries.is_none() {
            self.dynamic_queries = Some(super::physics::Collision::with_triangle_owners(
                vec![],
                vec![],
                vec![],
            )?);
        }
        let dynamic_queries = self.dynamic_queries.as_mut().unwrap();
        for (&source, state) in self.dynamic_constructions.range_mut(sources.clone()) {
            if !state.completed {
                continue;
            }
            let parts = self.dynamic_bodies[source]
                .geometry
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.clone()))?;
            let entity = self.dynamic_bodies[source].entity;
            let selected = *self.dynamic_bodies[source]
                .selected_system
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?;
            let system = self.dynamic_bodies[source]
                .systems
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .iter()
                .find(|s| Some(s.offset) == selected)
                .context("Missing selected admission system")?;
            let component = self.entities[entity]
                .components
                .iter()
                .find(|c| c.link_id.to_u32() == self.dynamic_bodies[source].component)
                .context("Admission component missing")?;
            let flag = |name: &str| {
                component
                    .properties
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value.as_bool())
                    .with_context(|| format!("Missing admission {name}"))
            };
            let is_mover = flag("Is Mover")?;
            let requests_dynamic = super::dynamic_body::requests_dynamic(
                is_mover,
                flag("Is Real")?,
                flag("Is Painted")?,
            );
            // Authored movers can keyframe a body relative to the entity when
            // its collision asset provides a local pose and no matching NIF node.
            let keyframed_system = state.bodies.len() > 1
                && !requests_dynamic
                && state.bodies.iter().zip(&state.node_bindings).all(|(body, node)| {
                    node.is_some()
                        || (is_mover && body.auxiliary.user_data.as_ref().is_some_and(|data| {
                            data.word_04 == 0 && data.dynamic_binding.is_some()
                        }))
                });
            let eligible = (state.bodies.len() == 1 || keyframed_system)
                && (self.entities[entity].transform_parent_id()?.is_none()
                    || self.transform_hierarchy.contains_key(&entity))
                && self
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, child)| {
                        child.scene == self.entities[entity].scene
                            && child.transform_parent_id().ok().flatten()
                                == Some(self.entities[entity].id)
                    })
                    .all(|(child, _)| self.transform_hierarchy.contains_key(&child))
                && system.system.phantoms.iter().all(Option::is_none)
                && system.system.constraints.iter().all(Option::is_none)
                && system.system.actions.iter().all(Option::is_none);
            // Entity identity is shared; each body retains its own shape group,
            // motion record, material and animation binding in the solver.
            let runtime_owner = if eligible {
                let owner = *next_owner;
                *next_owner = next_owner
                    .checked_add(1)
                    .context("Collision owner overflow")?;
                Some(owner)
            } else {
                None
            };
            for (body_index, (body, layout)) in
                state.bodies.iter_mut().zip(&state.layouts).enumerate()
            {
                let diagnostic_owner = self.dynamic_query_owners.len();
                let owner = runtime_owner.unwrap_or(diagnostic_owner);
                let collision = if eligible {
                    &mut self.collision
                } else {
                    &mut *dynamic_queries
                };
                layout
                    .attach_constructed_queries(body, parts, collision, owner)
                    .with_context(|| {
                        format!(
                            "Dynamic query {} body {:#x}",
                            self.dynamic_bodies[source].asset, layout.body_offset
                        )
                    })?;
                self.dynamic_query_owners.push((source, body_index));
                if eligible {
                    for &shape in &body.shapes {
                        collision.set_simulation_filter(shape, state.filters[body_index])?;
                        collision.enable_simulation_contact_events(shape)?;
                    }
                    if matches!(body.motion.current.motion_type(), 2 | 3 | 6 | 7) {
                        body.admit_native_mass(collision)?;
                    }
                    collision.set_simulation_body(
                        &body.shapes,
                        &body.motion.current,
                        state.materials[body_index],
                        system.system.active != 0,
                    )?;
                    self.dynamic_runtime_sources.insert(source, owner);
                    collision.set_owner_solid(owner, state.filters[body_index] & 0x8000 != 0)?;
                }
            }
            if eligible {
                let has_parent = self.entities[entity].transform_parent_id()?.is_some();
                let component = self.entities[entity]
                    .components
                    .iter_mut()
                    .find(|c| c.link_id.to_u32() == self.dynamic_bodies[source].component)
                    .context("Admission component missing")?;
                let boolean = |name: &str| -> Result<bool> {
                    component
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .and_then(|p| p.value.as_bool())
                        .with_context(|| format!("Missing admission {name}"))
                };
                let dynamic = super::dynamic_body::requests_dynamic(
                    boolean("Is Mover")?,
                    boolean("Is Real")?,
                    boolean("Is Painted")?,
                );
                let records = super::body_activation::PreAdmission {
                    bodies: &mut state.bodies,
                    component,
                    // Initial DOL806ba500 verified 3f800000; retained mutable
                    // constructor constants require their future runtime owner.
                    gravity_factor_word: 0x3f800000,
                };
                let mut backend = super::body_activation::Admitted {
                    records,
                    world: &mut self.collision,
                    materials: &state.materials,
                };
                super::body_activation::apply(&mut backend, dynamic, has_parent, false)?;
                // OnMapLoad can hide an entity before its solver construction.
                // Preserve that request instead of silently reviving its body.
                if self.hidden_entities.contains(&entity) {
                    self.collision
                        .set_owner_in_world(self.dynamic_runtime_sources[&source], false)?;
                }
            } else {
                self.report.unsupported.push(format!("Dynamic simulation {}: multi-body, attachment or non-body system lifecycle requires native binding",self.entities[entity].name));
            }
        }
        Ok(())
    }

    /// Apply gameplay motion commands to the same bodies used by collision
    /// queries and presentation. Activation caches/restores authored velocity;
    /// hiding a mesh or toggling query solidity is not a motion change.
    pub fn set_entity_motion_enabled(&mut self, entity: usize, enabled: bool) -> Result<()> {
        anyhow::ensure!(
            !self.removed_entities.contains(&entity),
            "Motion requested on a removed entity"
        );
        if entity==self.player_entity {return self.suspend_player_motion(!enabled)}
        let target = self.entities.get(entity).context("Unknown motion target")?;
        let components = target
            .components
            .iter()
            .filter(|c| c.class_name == "PhysicsRigidBodyDynamicComponent")
            .map(|c| c.link_id.to_u32())
            .collect::<Vec<_>>();
        let has_parent = target.transform_parent_id()?.is_some();
        let character = target
            .components
            .iter()
            .any(|c| c.class_name == "PhysicsRigidBodyCharacterComponent");
        let controller = target
            .components
            .iter()
            .enumerate()
            .find(|(_, c)| {
                matches!(
                    c.class_name.as_str(),
                    "BasicMotionControllerComponent"
                        | "BasicAIControllerComponent"
                        | "MickeyPlayerControllerComponent"
                        | "MickeyPlayerMotionControllerComponent"
                )
            })
            .map(|(index, c)| (index, c.class_name.clone()));
        // REL2995b0 visits dynamic, character, then controller. A target with
        // none of these legitimately does nothing; a missing binding does not.
        // Reject unsupported constructions before changing any admitted body.
        let sources = components
            .iter()
            .map(|component| {
                self.dynamic_runtime_sources
                    .keys()
                    .copied()
                    .find(|&source| {
                        self.dynamic_bodies[source].entity == entity
                            && self.dynamic_bodies[source].component == *component
                    })
                    .context("Motion target has an unsupported/unadmitted physics component")
            })
            .collect::<Result<Vec<_>>>()?;
        for source in sources {
            let state = self
                .dynamic_constructions
                .get_mut(&source)
                .context("Motion construction disappeared")?;
            let component = self.entities[entity]
                .components
                .iter_mut()
                .find(|c| c.link_id.to_u32() == self.dynamic_bodies[source].component)
                .context("Motion component disappeared")?;
            let records = super::body_activation::PreAdmission {
                bodies: &mut state.bodies,
                component,
                gravity_factor_word: 0x3f800000,
            };
            let mut backend = super::body_activation::Admitted {
                records,
                world: &mut self.collision,
                materials: &state.materials,
            };
            // REL3835bc inverts the command's suspension boolean, then calls
            // 38330c, whose parent+68 guard is independent of that request.
            super::body_activation::apply(&mut backend, enabled, has_parent, false)?;
        }
        if character {
            self.suspend_character_motion(entity, !enabled)?;
        }
        if let Some((index, controller)) = controller {
            if self.demo_story.enabled && character && controller == "BasicAIControllerComponent" {
                // The compatibility character body above owns motion here.
                // Disabling the whole AI component would also stop conversations.
                return Ok(());
            }
            anyhow::ensure!(entity!=self.player_entity && controller=="BasicMotionControllerComponent",
                "Native controller motion suspension requires {controller} modifier/flag delivery (REL299608..299690)");
            // Native inherited virtual+2f0 writes suspension into base+20 bit80.
            // Main-player modifier slot3 and controller+218 take another path.
            self.set_component_disabled(entity, index, !enabled)?;
        }
        Ok(())
    }
    /// Whole-model paint reaches the same owned body/cache used by simulation.
    /// REL3830e4 applies activation before the base paint/material setter.
    /// Constructor byte+a8 is the retained mover_material_seen flag
    /// (382a3c..a50). Deferred locks/callbacks remain separate requirements.
    pub fn paint_admitted_dynamic_model(&mut self, entity: usize, painted: bool) -> Result<()> {
        let has_parent = self
            .entities
            .get(entity)
            .context("Unknown painted body")?
            .transform_parent_id()?
            .is_some();
        for &source in self.dynamic_runtime_sources.keys() {
            if self.dynamic_bodies[source].entity != entity {
                continue;
            }
            let state = self
                .dynamic_constructions
                .get_mut(&source)
                .context("Painted construction disappeared")?;
            let component = self.entities[entity]
                .components
                .iter_mut()
                .find(|c| c.link_id.to_u32() == self.dynamic_bodies[source].component)
                .context("Painted body component missing")?;
            let records = super::body_activation::PreAdmission {
                bodies: &mut state.bodies,
                component,
                gravity_factor_word: 0x3f800000,
            };
            let mut backend = super::body_activation::Admitted {
                records,
                world: &mut self.collision,
                materials: &state.materials,
            };
            if !state.mover_material_seen || !painted {
                super::body_activation::apply(&mut backend, painted, has_parent, false)?;
            }
            let property = backend
                .records
                .component
                .properties
                .iter_mut()
                .find(|p| p.name == "Is Painted")
                .context("Painted body flag missing")?;
            anyhow::ensure!(property.value.is_boolean(), "Invalid painted body flag");
            property.value = serde_json::json!(painted);
            for filter in &mut state.filters {
                *filter = body_filter::change_layer(
                    *filter,
                    *filter & 31,
                    u8::from(painted),
                    u8::from(*filter & 0x10000 != 0),
                );
            }
        }
        Ok(())
    }
    pub fn initial_static_simulation_filter(&self, owner: usize) -> Result<u32> {
        if let Some(piece) = self.jigsaw_owners.get(&owner) {
            let instance = &self.jigsaw_instances[piece.instance];
            // The current Jigsaw Binding installs fixed query bodies (5).
            // Native child material-filter routing uses this exact word.
            return self.entities[instance.entity].jigsaw_states[instance.node]
                .filter(piece.piece, 5)
                .context("Jigsaw solver piece missing");
        }
        let part = self
            .imported_static_part(owner)
            .context("Unknown static solver owner")?;
        let entity = self
            .collision_entity(owner)
            .context("Static solver owner has no entity")?;
        let Some(component) = entity
            .components
            .iter()
            .find(|c| c.class_name == "PhysicsRigidBodyStaticComponent")
        else {
            // Other collider classes have their own initialization. Preserve
            // imported data, rather than fabricating a layer or flag override.
            return Ok(part.body_filter_info);
        };
        let value = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("{}: unresolved solver {name}", entity.name))
        };
        let layer = i32::try_from(
            value("Collision Layer")?
                .as_i64()
                .context("Invalid solver layer")?,
        )? as u32;
        let real = u8::from(
            value("Is Real")?
                .as_bool()
                .context("Invalid solver real flag")?,
        );
        let painted = u8::from(
            value("Is Painted")?
                .as_bool()
                .context("Invalid solver paint flag")?,
        );
        Ok(body_filter::change_layer(
            part.body_filter_info,
            layer,
            painted,
            real,
        ))
    }
    /// Called once at the shared simulation boundary. Rendering, projectile
    /// casts and character support now observe the same admitted body poses.
    pub fn advance_dynamic_physics(&mut self, dt: f32) -> Result<()> {
        self.prepare_scene_motion(dt)?;
        let attachments = self.character_physics.keys().copied()
            .filter(|entity| self.transform_hierarchy.contains_key(entity))
            .collect::<Vec<_>>();
        for entity in attachments {
            self.sync_keyframed_character_attachment(entity)?;
        }
        self.prepare_character_gravity(dt)?;
        if self.dynamic_runtime_sources.is_empty()
            && self.player_physics.is_none()
            && !self.character_physics.values().any(Result::is_ok)
        {
            self.dynamic_runtime_time += dt;
            return Ok(());
        }
        // REL2d27f4..281c initializes shared gravity; REL240b10..b70 copies
        // it into gameplay world cinfo without replacing it with player gravity.
        self.collision.step_simulation(
            dt,
            self.report.physics_world.gravity,
            &self.dynamic_runtime_filter,
        )?;
        self.publish_character_physics(dt)?;
        for (&source, &owner) in &self.dynamic_runtime_sources {
            if !self.collision.owner_in_world(owner)? {
                continue;
            }
            let state = self
                .dynamic_constructions
                .get_mut(&source)
                .context("Admitted construction disappeared")?;
            let count = state.bodies.len();
            for body in &mut state.bodies {
                if body.motion.current.motion_type() == 5 {
                    continue;
                }
                let pose = self
                    .collision
                    .simulation_pose(*body.shapes.first().context("Admitted shape missing")?)?;
                body.accept_simulation_pose(pose, self.dynamic_runtime_time, dt)?;
                // The entity is the target owner for keyframed motion. Do not
                // replace its authored rotator target with solver integration drift.
                let entity_index = self.dynamic_bodies[source].entity;
                if body.motion.current.motion_type() == 4
                    || self.transform_hierarchy.get(&entity_index).is_some_and(|node| node.socket.is_some())
                {
                    continue;
                }
                anyhow::ensure!(
                    count == 1,
                    "Dynamic multi-body model writeback requires node pose publication"
                );
                // Single-body binding. Multi-body/named-node/skeletal bindings
                // remain excluded rather than selecting an arbitrary body.
                let identity =
                    transform::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
                let inverse = transform::relative(&state.layouts[0].initial_transform, &identity);
                let delta = transform::compose(&body.state().transform, &inverse);
                let entity = &mut self.entities[self.dynamic_bodies[source].entity];
                let world = transform::compose(&delta, &state.entity_pose);
                entity.runtime_rotation = std::array::from_fn(|i| world[(i % 3) * 4 + i / 3]);
                entity.transform =
                    super::render_transform::from_havok(&world, entity.authored_scale);
                for draw in entity.draws.clone() {
                    self.draws[draw].trans_mat = entity.transform;
                }
                super::rotator_runtime::accept_physics_pose(
                    self.dynamic_bodies[source].entity,
                    &mut self.transform_hierarchy,
                    &mut self.entities,
                    &mut self.draws,
                )?;
            }
        }
        let contacts = self.collision.simulation_begin_contacts()?;
        for contact in contacts {
            self.record_ai_projectile_contact(&contact);
            for side in 0..2 {
                // REL3a033c..434: the listener's first non-character contact
                // latches even if Event on Collision is disabled. It does not
                // require an upward normal and does not reset when separating.
                if contact.motion_types[1 - side] == 7 {
                    continue;
                }
                let Some((&source, _)) = self
                    .dynamic_runtime_sources
                    .iter()
                    .find(|(_, owner)| **owner == contact.owners[side])
                else {
                    continue;
                };
                if !self.dynamic_ground_contacts.insert(source) {
                    continue;
                }
                let entity = self.dynamic_bodies[source].entity;
                let component = self.entities[entity]
                    .components
                    .iter()
                    .find(|component| {
                        component.link_id.to_u32() == self.dynamic_bodies[source].component
                    })
                    .context("Contact component missing")?;
                let enabled = component
                    .properties
                    .iter()
                    .find(|property| property.name == "Event on Collision")
                    .and_then(|property| property.value.as_bool())
                    .context("Contact event flag missing")?;
                if !enabled {
                    continue;
                }
                let other = self
                    .collision_entity(contact.owners[1 - side])
                    .context("Contact entity missing")?;
                let activator = self
                    .entities
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, other))
                    .context("Contact entity identity missing")?;
                // REL381470..5ac forwards GroundPass (0x73) to the owner's
                // OnCollisionGround output (0x8020), retaining the other entity.
                self.emit_entity_output(entity, "OnCollisionGround", activator);
            }
        }
        self.dynamic_runtime_time += dt;
        Ok(())
    }
}
#[cfg(debug_assertions)]
pub(super) fn validate_dbc_hall_door() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let root = std::path::Path::new(&root);
    let ctx = egui::Context::default();
    let mut world = super::assets::load(root, "Levels/DBC_Start_1st.Level", &ctx)?;
    world.enable_demo_story()?;
    world.run_callback_fixture(
        "ActivateScene('Environments/ThePark/DarkBeautyCastle/GSA/dbc_1st_zoneb.gsa')")?;
    world.advance_models_with_delta(world.animation_time + super::STEP, super::STEP)?;
    let find = |world: &World, name| world.scripts.resolve_name(name).context("Missing hallway entity");
    let door = find(&world, "dbc_1st_zoneb_dynamic.DBC__1st_ZoneB_HallDoor01 01")?;
    let gear = find(&world, "dbc_1st_zoneb_dynamic.DBC__1st_ZoneB_HallDoorGear01 01")?;
    let source = world.dynamic_bodies.iter().position(|body| body.entity == door)
        .context("Hallway door physics source missing")?;
    let messages = |world: &World| world.report.unsupported.iter()
        .filter(|line| line.contains("HallDoor01")).cloned().collect::<Vec<_>>();
    let owner = *world.dynamic_runtime_sources.get(&source)
        .with_context(|| format!("Hallway door physics is unavailable: {:?}", messages(&world)))?;
    let snapshot = |world: &World| -> Result<Vec<_>> {
        world.dynamic_constructions[&source].bodies.iter().enumerate().map(|(index, body)| {
            let construction = &world.dynamic_constructions[&source];
            let binding = construction.node_bindings[index];
            let expected = binding.map(|binding| world.rigid_model_node_frame(door, binding.node, true)
                .map(|frame| transform::compose(&frame, &binding.offset))).transpose()?;
            let body_pose = world.collision.simulation_pose(body.shapes[0])?;
            Ok((body_pose.position, expected.map(|pose| [pose[12], pose[13], pose[14]]),
                body.motion.current.motion_type(), body.auxiliary.user_data.as_ref().map(|data| data.byte_08)))
        }).collect()
    };
    let advance = |world: &mut World| -> Result<()> {
        world.advance_models_with_delta(world.animation_time + super::STEP, super::STEP)?;
        world.advance_dynamic_physics(super::STEP)?;
        world.advance_jigsaw_effects(super::STEP)
    };
    for _ in 0..4 { advance(&mut world)?; }
    let closed = snapshot(&world)?;
    println!("Closed doorway {owner}: {closed:?}");
    let leaf_indices = world.dynamic_constructions[&source].layouts.iter().enumerate()
        .filter_map(|(index, layout)| {
            let parts = world.dynamic_bodies[source].geometry.as_ref().ok()?;
            parts[layout.parts[0]].body_name.as_deref()
                .filter(|name| name.starts_with("REF:HallDoor01_Main|"))?;
            Some(index)
        }).collect::<Vec<_>>();
    anyhow::ensure!(leaf_indices.len() == 2, "Expected two door leaves");
    let [first, second] = [closed[leaf_indices[0]].0, closed[leaf_indices[1]].0];
    let x = second[0] - first[0];
    let z = second[2] - first[2];
    let horizontal = x.hypot(z);
    anyhow::ensure!(horizontal > 0.1, "Closed door leaves have no lateral spacing");
    let across = [-z / horizontal, 0., x / horizontal];
    let passage_hits = |world: &World| -> Result<Vec<usize>> {
        let center = std::array::from_fn::<_, 3, _>(|axis| (first[axis] + second[axis]) * 0.5);
        let mut hits = Vec::new();
        for ratio in [0.3, 0.5, 0.7] {
            let origin = std::array::from_fn(|axis| center[axis] + (ratio - 0.5)
                * (second[axis] - first[axis]) - across[axis]);
            hits.extend(world.collision.ray_hits(origin, across.map(|v| v * 2.))?
                .into_iter().map(|hit| hit.owner));
        }
        hits.sort_unstable();
        hits.dedup();
        Ok(hits)
    };
    let blocked = passage_hits(&world)?;
    println!("Closed passage hits: {blocked:?}");
    let paint_owner = *world.model_paint.get(&gear).context("Hallway gear paint model missing")?
        .owners.first().context("Hallway gear collision owner missing")?;
    anyhow::ensure!(world.brush_model_hit(paint_owner, true, [10000.; 3])?, "Gear rejected paint");
    for _ in 0..180 { advance(&mut world)?; }
    let open = snapshot(&world)?;
    println!("Opened doorway {owner}: {open:?}; diagnostics: {:?}", messages(&world));
    let open_hits = passage_hits(&world)?;
    println!("Open passage hits: {:?}", open_hits.iter().map(|part| (*part,
        world.collision_entity(*part).map(|entity| entity.name.as_str()))).collect::<Vec<_>>());
    for (index, (after, expected, _, _)) in open.into_iter().enumerate() {
        let expected = expected.context("Missing hallway door model-node binding")?;
        anyhow::ensure!((0..3).all(|axis| (after[axis] - expected[axis]).abs() < 0.005),
            "Hallway solver body detached from animated pose: {after:?} vs {expected:?}");
        if leaf_indices.contains(&index) {
            anyhow::ensure!((0..3).any(|axis| (after[axis] - closed[index].0[axis]).abs() > 0.1),
                "Hallway solver body stayed at its closed position");
        }
    }
    anyhow::ensure!(blocked.contains(&owner) && !open_hits.contains(&owner),
        "Hallway door failed the closed/open passage ray test");
    let trim = find(&world, "dbc_1st_zoneb_static.DBC__1st_ZoneB_Toon01 01")?;
    let second_door = find(&world, "dbc_1st_zoneb_dynamic.DBC__1st_ZoneB_HallDoor02 01")?;
    let second_hull = find(&world, "dbc_1st_zoneb_static.DBC__1st_ZoneB_HallDoor02_Collision 01")?;
    let trim_pieces = world.jigsaw_owners.iter().filter_map(|(&owner, piece)| {
        let instance = &world.jigsaw_instances[piece.instance];
        (instance.entity == trim
            && world.entities[trim].jigsaw_states[instance.node].painted[piece.piece]).then_some(owner)
    }).collect::<Vec<_>>();
    anyhow::ensure!(trim_pieces.len() > 1, "Authored second-door trim has no painted pieces");
    let (last, first) = trim_pieces.split_last().unwrap();
    for &piece in first {
        anyhow::ensure!(world.brush_jigsaw_contact(piece, false)?, "Trim rejected thinner");
    }
    for _ in 0..60 { advance(&mut world)?; }
    anyhow::ensure!(!world.removed_entities.contains(&second_hull), "Partially thinned trim removed the doorway");
    anyhow::ensure!(world.brush_jigsaw_contact(*last, false)?, "Final trim piece rejected thinner");
    for _ in 0..100 { advance(&mut world)?; }
    anyhow::ensure!(world.removed_entities.contains(&second_hull), "Second doorway still has its static hull");
    let open_id = world.actor_sequences[&second_door].find_name("Open");
    anyhow::ensure!(world.actors[&second_door].active_id() == Some(open_id), "Second door never played its falling sequence");
    println!("PASS: fully thinned hallway trim plays the second door's Open sequence and removes its static blocker");
    Ok(())
}

#[cfg(debug_assertions)]
pub(super) fn validate_dbc_courtyard_armor() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world = super::assets::load(
        std::path::Path::new(&root),
        "Levels/DBC_Start_1st.Level",
        &egui::Context::default(),
    )?;
    world.enable_demo_story()?;
    world.run_callback_fixture(
        "ActivateScene('Environments/ThePark/DarkBeautyCastle/GSA/dbc_1st_zonec.gsa')",
    )?;
    world.advance_models_with_delta(world.animation_time + super::STEP, super::STEP)?;
    world.advance_dynamic_physics(super::STEP)?;

    let armor = world.scripts.resolve_name("dbc_scaletest.spawner_thinnerammo 08.Spawner_ThinnerAmmo_Toon")
        .context("Missing courtyard armor")?;
    let source = world.dynamic_bodies.iter().position(|body| body.entity == armor)
        .context("Courtyard armor physics source missing")?;
    let owner = *world.dynamic_runtime_sources.get(&source)
        .context("Courtyard armor has no gameplay collision owner")?;
    let state = &world.dynamic_constructions[&source];
    anyhow::ensure!(state.bodies.len() == 2, "Expected both authored armor bodies");
    anyhow::ensure!(world.collision.owner_in_world(owner)?, "Armor hull is inactive");
    let mut positions = Vec::new();
    for body in &state.bodies {
        let shape = *body.shapes.first().context("Missing armor shape")?;
        anyhow::ensure!(world.collision.simulation_filter_word(shape)? & 0x8000 != 0,
            "Armor shape is missing its solid collision flag");
        positions.push(world.collision.simulation_pose(shape)?.position);
    }
    let mut hits = std::collections::HashSet::new();
    for position in positions {
        world.dispatch_spin(position, &mut hits)?;
    }
    anyhow::ensure!(hits.contains(&armor), "Spin did not hit the authored armor");
    world.advance_models_with_delta(world.animation_time + super::STEP, super::STEP)?;
    world.run_callback_fixture(
        r#"assert(GetPrefabData("dbc_scaletest.spawner_thinnerammo 08.Spawner_ThinnerAmmo_Toon").SpawnerFired == "True",
            "The original armor callback did not fire")"#,
    )?;
    println!("PASS: both authored armor bodies are solid and the armor spin runs its pickup callback");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_opening_door_has_live_animated_collision() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let source = world
            .dynamic_bodies
            .iter()
            .position(|b| {
                world.entities[b.entity].name == "gremlinvillage_zonea_dynamic.GV_ZoneA_Door03 01"
            })
            .context("Opening door missing")?;
        let imported = &world.dynamic_bodies[source];
        let entity = imported.entity;
        anyhow::ensure!(
            world.dynamic_runtime_sources.contains_key(&source),
            "Opening door collision was not admitted"
        );
        let owner = world.dynamic_runtime_sources[&source];
        assert_eq!(world.dynamic_constructions[&source].bodies.len(), 2);
        assert!(world.dynamic_constructions[&source]
            .node_bindings
            .iter()
            .all(Option::is_some));
        let blocked = |world: &World| -> Result<bool> {
            Ok(world
                .collision
                .ray_hits([6., 1.5, 2.], [3., 0., 0.])?
                .iter()
                .any(|hit| hit.owner == owner))
        };
        assert!(
            blocked(&world)?,
            "Closed door has no collision across the passage"
        );
        let walk = |world: &World| {
            let mut player =
                super::super::physics::Player::new([5.5, 0.035, 2.], world.report.motion.clone());
            for _ in 0..40 {
                world.collision.with_player_queries(|| {
                    player.step(&world.collision, [1., 0., 0.], false, super::super::STEP)
                });
            }
            player.position
        };
        let closed_position = walk(&world);
        assert!(
            closed_position[0] < 7.,
            "Mickey walked through the closed door: {closed_position:?}"
        );
        let initial = world.dynamic_constructions[&source]
            .bodies
            .iter()
            .map(|b| b.state().transform)
            .collect::<Vec<_>>();
        world.request_actor_sequence(entity, "Open")?;
        for tick in 1..=45 {
            world.advance_models(tick as f32 * super::super::STEP)?;
            world.advance_dynamic_physics(super::super::STEP)?;
        }
        for (index, body) in world.dynamic_constructions[&source]
            .bodies
            .iter()
            .enumerate()
        {
            let binding = world.dynamic_constructions[&source].node_bindings[index].unwrap();
            let expected = transform::compose(
                &world.rigid_model_node_frame(entity, binding.node, true)?,
                &binding.offset,
            );
            let actual = world.collision.simulation_pose(body.shapes[0])?;
            for axis in 0..3 {
                assert!(
                    (actual.position[axis] - expected[12 + axis]).abs() < 0.005,
                    "Door body {index} is detached from animated node: {:?} vs {:?}",
                    actual.position,
                    &expected[12..15]
                );
            }
            assert!(
                (actual.position[1] - initial[index][13]).abs() > 0.5,
                "Door half did not move"
            );
        }
        assert!(
            !blocked(&world)?,
            "Opened door collision still blocks passage"
        );
        let open_position = walk(&world);
        assert!(
            open_position[0] > 8.,
            "Mickey could not cross the opened doorway: {open_position:?}"
        );
        std::fs::write(
            "target/demo-door-owned.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "entity":world.entities[entity].name,"bodies":2,"closed_walk_end":closed_position,"open_walk_end":open_position,
            "animated_body_position_tolerance_m":0.005,"original_game_reference_comparison":false}))?,
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_shared_physics_boundary_steps_once_and_is_presentation_independent() -> Result<()> {
        use super::super::{
            controller::{Controller, Input, TickPhase},
            physics::Player,
        };
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let run = |hz: u32| -> Result<_> {
            let mut world = super::super::assets::load(
                std::path::Path::new(&root),
                "Levels/GV_ZoneA_Start.level",
                &egui::Context::default(),
            )?;
            anyhow::ensure!(
                !world.dynamic_runtime_sources.is_empty(),
                "No original shared solver bodies"
            );
            let mut controller =
                Controller::new(Player::new(world.report.spawn, world.report.motion.clone()));
            let mut steps = 0;
            for _ in 0..hz {
                controller.advance_with_tick_phases(
                    &mut world,
                    |w| &w.collision,
                    1. / hz as f64,
                    true,
                    Input {
                        movement: [0., 1.],
                        ..Default::default()
                    },
                    |phase, player, _, _, _, world| -> Result<()> {
                        if phase == TickPhase::SimulatePhysics {
                            assert_eq!(
                                player.position, player.previous,
                                "World stepped after manual movement"
                            );
                            world.advance_dynamic_physics(super::super::STEP)?;
                            steps += 1;
                        }
                        Ok(())
                    },
                )?;
            }
            assert_eq!(
                (controller.ticks, steps),
                (30, 30),
                "Shared solver must run once per committed tick"
            );
            let expected = (0..30).fold(0f32, |time, _| time + super::super::STEP);
            assert_eq!(
                world.dynamic_runtime_time, expected,
                "Shared world was advanced twice"
            );
            let poses = world
                .dynamic_runtime_sources
                .keys()
                .map(|source| {
                    let shape = world.dynamic_constructions[source].bodies[0].shapes[0];
                    world
                        .collision
                        .simulation_pose(shape)
                        .map(|p| (p.position, p.orientation, p.linear, p.angular))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((
                controller.player.position,
                controller.player.velocity,
                controller.player.grounded,
                controller.player.respawns,
                poses,
            ))
        };
        // Exact same native-engine simulation, different host frame grouping.
        // This is not an original-game/Havok numerical acceptance test.
        assert_eq!(run(10)?, run(120)?);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_dynamic_models_paint_and_share_solver_query_poses() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        anyhow::ensure!(
            !world.dynamic_runtime_sources.is_empty(),
            "Original scene admitted no bodies"
        );
        let models = world
            .dynamic_runtime_sources
            .iter()
            .filter_map(|(&source, &owner)| {
                let entity = world.dynamic_bodies[source].entity;
                world
                    .model_paint
                    .get(&entity)
                    .filter(|model| !model.invulnerable && !model.states.is_empty())
                    .map(|_| (source, owner, entity))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            !models.is_empty(),
            "Original scene exposed no dynamic toon model"
        );
        for &(source, owner, entity) in &models {
            let original_mode = world.dynamic_constructions[&source].bodies[0]
                .motion
                .current
                .motion_type();
            let sticky = world.dynamic_constructions[&source].mover_material_seen;
            world.brush_model_hit(owner, true, [10000.; 3])?;
            anyhow::ensure!(
                world.model_paint[&entity].painted,
                "Paint did not reach dynamic model"
            );
            if sticky {
                anyhow::ensure!(
                    world.dynamic_constructions[&source].bodies[0]
                        .motion
                        .current
                        .motion_type()
                        == original_mode,
                    "Painting released an authored mover"
                );
            }
            world.brush_model_hit(owner, false, [10000.; 3])?;
            anyhow::ensure!(
                !world.model_paint[&entity].painted,
                "Thinner did not reach dynamic model"
            );
            for &shape in &world.dynamic_constructions[&source].bodies[0].shapes {
                anyhow::ensure!(
                    world.collision.shape_owner(shape) == Some(owner),
                    "Dynamic brush owner aliased another namespace"
                );
                anyhow::ensure!(
                    world
                        .collision
                        .simulation_filter(shape)
                        .is_some_and(|word| word & 0x8000 == 0),
                    "Thinner left the solver filter painted"
                );
            }
            world.brush_model_hit(owner, true, [10000.; 3])?;
        }
        for _ in 0..12 {
            world.advance_dynamic_physics(1. / 30.)?;
            world.advance_jigsaw_effects(1. / 30.)?;
        }
        for (&source, _) in &world.dynamic_runtime_sources {
            let body = &world.dynamic_constructions[&source].bodies[0];
            let pose = world.collision.simulation_pose(body.shapes[0])?;
            for &shape in &body.shapes {
                let part = world.collision.simulation_pose(shape)?;
                assert_eq!(part.position, pose.position);
                assert_eq!(part.orientation, pose.orientation);
            }
            if body.motion.current.motion_type() != 5 {
                assert_eq!(&body.state().transform[12..15], &pose.position);
                assert_eq!(body.state().orientation, pose.orientation);
                assert_eq!(&body.linear_velocity()[..3], &pose.linear);
            }
        }
        println!("{} original dynamic toon models paint/thin; {} single-body systems share solver/query/owned pose",models.len(),world.dynamic_runtime_sources.len());
        Ok(())
    }
}
