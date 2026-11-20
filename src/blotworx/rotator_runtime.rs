//! Live authored rotators and native keyframe-target publication.
//! Position-target commands, parent propagation and keyframed collision use
//! the same pose publication. Entity tracking and complete event routing remain
//! explicit gaps.
use super::{assets::World, native_hk_transform as hk, native_matrix as matrix, rotator};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

pub struct Local {
    pub parent: usize,
    pub position: [f32; 3],
    pub rotation: [f32; 9],
    pub socket: Option<super::skeletal_children::Socket>,
}
pub struct Instance {
    pub state: rotator::State,
    pub disabled: bool,
    pub local_axis: bool,
    pub rotate_to_position: bool,
    /// None retains the authored scene reference; Some(None) is an explicit
    /// native null assignment. World indices are runtime identities only.
    pub entity_target: Option<Option<usize>>,
}
fn value<'a>(component: &'a Component, name: &str) -> Result<&'a serde_json::Value> {
    component
        .properties
        .iter()
        .find(|p| p.name == name)
        .map(|p| &p.value)
        .with_context(|| format!("{} missing {name}", component.class_name))
}
fn float(component: &Component, name: &str) -> Result<f32> {
    let n = value(component, name)?
        .as_f64()
        .with_context(|| format!("Invalid rotator float {name}"))? as f32;
    ensure!(n.is_finite(), "Nonfinite rotator {name}");
    Ok(n)
}
fn boolean(component: &Component, name: &str) -> Result<bool> {
    value(component, name)?
        .as_bool()
        .with_context(|| format!("Invalid rotator boolean {name}"))
}
fn point(component: &Component) -> Result<[f32; 3]> {
    let value = value(component, "Axis of Rotation")?;
    let mut result = [0.; 3];
    for (i, key) in ["x", "y", "z"].into_iter().enumerate() {
        result[i] = value
            .get(key)
            .and_then(|v| v.as_f64())
            .context("Invalid rotator axis")? as f32;
    }
    ensure!(
        result.iter().all(|v| v.is_finite()),
        "Nonfinite rotator axis"
    );
    Ok(result)
}
fn parameters(component: &Component) -> Result<rotator::Parameters> {
    let integer = |name| -> Result<i32> {
        Ok(i32::try_from(
            value(component, name)?
                .as_i64()
                .context("Invalid rotator limit")?,
        )?)
    };
    Ok(rotator::Parameters {
        max_speed_degrees: float(component, "Max Speed")?,
        acceleration_degrees: float(component, "Acceleration")?,
        min_degrees: integer("MinRotationDegrees")?,
        max_degrees: integer("MaxRotationDegrees")?,
        auto_reverse: boolean(component, "AutoReverse")?,
    })
}
/// DOL805aea20/805ae9b0: retain the double estimate until each single operation.
fn axis_normalize(mut axis: [f32; 3]) -> Result<[f32; 3]> {
    if axis == [0.; 3] {
        axis = [0., 1., 0.];
    }
    let squared = axis[2].mul_add(axis[2], axis[0] * axis[0]) + axis[1] * axis[1];
    ensure!(squared.is_finite(), "Rotator axis magnitude overflow");
    if squared == 0. {
        return Ok([0.; 3]);
    }
    let estimate = super::pose_blend::estimate_double(squared);
    let square = super::pose_blend::square_estimate(estimate);
    let half = (estimate * 0.5) as f32;
    let correction = -(square as f64).mul_add(squared as f64, -3.) as f32;
    let factor = correction * half;
    if squared * factor > f32::from_bits(0x358637bd) {
        Ok(axis.map(|v| v * factor))
    } else {
        Ok([0.; 3])
    }
}
pub(super) fn parent_cache(
    entity: &super::assets::RuntimeEntity,
) -> super::native_transform_record::Transform {
    // Normal parent attachment354a14..354a5c copies pose and forces cache scale1.
    let mut pose = [0.; 13];
    pose[..9].copy_from_slice(&entity.runtime_rotation);
    pose[9..12].copy_from_slice(&entity.transform[12..15]);
    pose[12] = 1.;
    pose
}
fn publish(
    entity: usize,
    position: [f32; 3],
    rotation: [f32; 9],
    entities: &mut [super::assets::RuntimeEntity],
    draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
) {
    let e = &mut entities[entity];
    e.runtime_rotation = rotation;
    e.transform = super::render_transform::from_entity(rotation, position, e.authored_scale);
    for draw in e.draws.clone() {
        draws[draw].trans_mat = e.transform;
    }
}
fn apply_rotation(
    entity: usize,
    id: u32,
    local_axis: bool,
    delta: f32,
    hierarchy: &mut std::collections::BTreeMap<usize, Local>,
    entities: &mut [super::assets::RuntimeEntity],
    draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
) -> Result<()> {
    if delta == 0. {
        return Ok(());
    }
    let parent = hierarchy.get(&entity).map(|node| node.parent);
    let parent_rotation = parent.map(|parent| entities[parent].runtime_rotation);
    let runtime = &mut entities[entity];
    let component = runtime
        .components
        .iter()
        .find(|c| c.link_id.to_u32() == id)
        .context("Rotator component disappeared")?;
    let axis = point(component)?;
    let axis = if local_axis {
        matrix::transform(&runtime.runtime_rotation, axis)
    } else {
        axis
    };
    let (sin, cos) = super::native_angle::sin_cos_medium(delta as f64)?;
    let rotation = matrix::rotation(axis, sin as f32, cos as f32);
    let previous_rotation = runtime.runtime_rotation;
    let mut next = matrix::multiply(&rotation, &previous_rotation);
    ensure!(
        matrix::reorthogonalize(&mut next),
        "Rotator {} produced degenerate rotation",
        runtime.name
    );
    runtime.runtime_rotation = next;
    for column in 0..3 {
        for row in 0..3 {
            runtime.transform[column * 4 + row] = next[row * 3 + column] * runtime.authored_scale;
        }
    }
    for draw in runtime.draws.clone() {
        draws[draw].trans_mat = runtime.transform;
    }
    if let Some(parent_rotation) = parent_rotation {
        let local = hierarchy.get_mut(&entity).unwrap();
        local.rotation = if local.socket.as_ref().is_some_and(|socket| matches!(socket.mode, 3 | 4)) {
            matrix::attached_rotation(&local.rotation, &previous_rotation, &next)?
        } else {
            matrix::local_rotation(&parent_rotation, &next)
        };
    }
    propagate(entity, hierarchy, entities, draws)
}
pub(super) fn propagate(
    root: usize,
    hierarchy: &std::collections::BTreeMap<usize, Local>,
    entities: &mut [super::assets::RuntimeEntity],
    draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
) -> Result<()> {
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        let pose = parent_cache(&entities[parent]);
        for (&child, local) in hierarchy.iter().filter(|(_, node)| node.parent == parent) {
            if local.socket.is_some() {
                continue;
            }
            let (position, rotation) =
                super::native_transform_record::world_pose(&pose, local.position, &local.rotation);
            publish(child, position, rotation, entities, draws);
            pending.push(child);
        }
    }
    Ok(())
}
/// A simulated body's world pose owns its local pose too. Preserve that pose
/// when its parent next moves, and publish its children from the same boundary.
pub(super) fn accept_physics_pose(
    root: usize,
    hierarchy: &mut std::collections::BTreeMap<usize, Local>,
    entities: &mut [super::assets::RuntimeEntity],
    draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
) -> Result<()> {
    if let Some(local) = hierarchy.get_mut(&root) {
        let parent = parent_cache(&entities[local.parent]);
        let (position, rotation) = super::native_transform_record::local_pose(
            &parent,
            entities[root].transform[12..15].try_into().unwrap(),
            &entities[root].runtime_rotation,
        );
        local.position = position;
        local.rotation = rotation;
    }
    propagate(root, hierarchy, entities, draws)
}
impl World {
    /// Native3b3854 radians entry, also used by RobotArmGeneral5fa58.
    pub(super) fn instant_rotator_position(&mut self,entity:usize,radians:f32)->Result<()> {
        let Some(component)=self.entities[entity].components.iter().find(|c|c.class_name=="RotatorComponent") else {return Ok(())};
        let id=component.link_id.to_u32();
        let instance=self.rotators.get_mut(&(entity,id)).context("Instant rotator is not initialized")?;
        instance.state.speed=0.;
        let delta=radians-instance.state.angle;
        let local_axis=instance.local_axis;
        if delta!=0. {instance.state.angle+=delta;}
        apply_rotation(entity,id,local_axis,delta,&mut self.transform_hierarchy,&mut self.entities,&mut self.draws)
    }
    /// Retail wrappers264cf4/264e4c/264f70. Stop also consumes two Lua values,
    /// discards the numeric value, and only clears position-target mode.
    pub(super) fn rotator_command_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if args.len() != 2 {
            return Err(mlua::Error::external(format!(
                "{name} requires entity and angle"
            )));
        }
        let degrees = lua.coerce_number(args[1].clone())?.unwrap_or(0.) as f32;
        let radians = degrees * f32::from_bits(0x3c8efa35);
        if name != "StopRotateToPosition" && !radians.is_finite() {
            return Err(mlua::Error::external("Nonfinite rotator angle"));
        }
        let Some(entity) = self.scripts.resolve_world_argument(&args[0])? else {
            return Ok(mlua::MultiValue::new());
        };
        let Some(component) = self.entities[entity]
            .components
            .iter()
            .position(|c| c.class_name == "RotatorComponent")
        else {
            return Ok(mlua::MultiValue::new());
        };
        let id = self.entities[entity].components[component].link_id.to_u32();
        let instance = self
            .rotators
            .get_mut(&(entity, id))
            .context("Rotator command target is not initialized")
            .map_err(mlua::Error::external)?;
        match name {
            "StartRotateToPosition" => {
                instance.rotate_to_position = true;
                let property = self.entities[entity].components[component]
                    .properties
                    .iter_mut()
                    .find(|p| p.name == "RotateToPosition")
                    .context("Rotator target property missing")
                    .map_err(mlua::Error::external)?;
                if property.class_name != "Float" || property.value.is_array() {
                    return Err(mlua::Error::external(
                        "Invalid scalar rotator target property",
                    ));
                }
                property.value = serde_json::json!(radians);
            }
            "StopRotateToPosition" => {
                instance.rotate_to_position = false;
            }
            "InstantRotateToPosition" => {
                self.instant_rotator_position(entity,radians).map_err(mlua::Error::external)?;
            }
            _ => unreachable!(),
        }
        Ok(mlua::MultiValue::new())
    }
    /// Demo scalar bridge. The motion step reads acceleration on every tick;
    /// this retains current angular velocity instead of restarting the motor.
    pub(super) fn set_demo_rotator_acceleration(
        &mut self,
        entity: usize,
        value: f32,
    ) -> Result<()> {
        ensure!(value.is_finite(), "Nonfinite rotator acceleration");
        let component = self.entities[entity]
            .components
            .iter_mut()
            .find(|c| c.class_name == "RotatorComponent")
            .context("Acceleration target has no rotator")?;
        ensure!(
            self.rotators
                .contains_key(&(entity, component.link_id.to_u32())),
            "Acceleration target rotator is not initialized"
        );
        float(component, "Acceleration")?;
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Acceleration")
            .unwrap()
            .value = serde_json::json!(value);
        Ok(())
    }
    /// Rotator setter3b2658 ->3b1be4 ->123ec4 copies one float to
    /// component+38. Getter3b1bec ->3313a8 returns that same slot. It does
    /// not overwrite speed+44: update3b31bc approaches the new value later.
    pub(super) fn set_rotator_speed(&mut self, entity: usize, speed: f32) -> Result<()> {
        ensure!(
            speed.is_finite() && !self.removed_entities.contains(&entity),
            "Invalid rotator speed target/value"
        );
        let runtime = self
            .entities
            .get_mut(entity)
            .context("Rotator speed target missing")?;
        let matching = runtime
            .components
            .iter()
            .enumerate()
            .flat_map(|(c, component)| {
                component
                    .properties
                    .iter()
                    .enumerate()
                    .filter_map(move |(p, property)| {
                        (property.name == "Max Speed").then_some((c, p))
                    })
            })
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1,
            "Max Speed setter requires one unambiguous native property"
        );
        let (c, p) = matching[0];
        let component = &mut runtime.components[c];
        ensure!(
            component.class_name == "RotatorComponent",
            "Max Speed setter for {} is not connected",
            component.class_name
        );
        ensure!(
            self.rotators
                .contains_key(&(entity, component.link_id.to_u32())),
            "Rotator speed setter lacks initialized runtime"
        );
        float(component, "Max Speed")?;
        component.properties[p].value = serde_json::json!(speed);
        Ok(())
    }
    pub(super) fn bind_transform_hierarchy(&mut self) -> Result<()> {
        self.bind_transform_hierarchy_range(0..self.entities.len())
    }
    pub(super) fn bind_transform_hierarchy_range(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Result<()> {
        for entity in range.clone() {
            let Some(id) = self.entities[entity].transform_parent_id()? else {
                continue;
            };
            let result = (|| -> Result<_> {
                let e = &self.entities[entity];
                let parent = self
                    .entities
                    .iter()
                    .position(|p| p.scene == e.scene && p.id == id)
                    .context("Parent handle not loaded in this scene")?;
                ensure!(entity != parent, "Self-parented transform");
                let c = e
                    .components
                    .iter()
                    .find(|c| c.class_name == "JPSTransformationComponent")
                    .context("Parent lacks transform component")?;
                let socket = super::skeletal_children::Socket::load(c)?;
                ensure!(
                    !boolean(c, "DisallowRotation")?,
                    "Disallowed child rotation needs native override publication"
                );
                let position = crate::render_support::scene::parse_point3(value(c, "Local Translation")?)
                    .context("Invalid local translation")?;
                let raw = crate::render_support::scene::parse_matrix3(value(c, "Local Rotation")?)
                    .context("Invalid local rotation")?;
                let rotation = std::array::from_fn(|i| raw[i / 3][i % 3]);
                Ok((
                    entity,
                    Local {
                        parent,
                        position,
                        rotation,
                        socket,
                    },
                ))
            })();
            match result {
                Ok((entity, node)) => {
                    self.transform_hierarchy.insert(entity, node);
                }
                Err(error) => self.report.unsupported.push(format!(
                    "Transform hierarchy {}: {error:#}",
                    self.entities[entity].name
                )),
            }
        }
        for &entity in self.transform_hierarchy.keys() {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = entity;
            while let Some(node) = self.transform_hierarchy.get(&current) {
                ensure!(seen.insert(current), "Cycle in transform hierarchy");
                current = node.parent;
            }
        }
        // Serialize world and local pose separately. Resolve parents first.
        let mut initialized = std::collections::BTreeSet::new();
        let pending = self
            .transform_hierarchy
            .keys()
            .filter(|entity| range.contains(entity))
            .count();
        while initialized.len() < pending {
            let before = initialized.len();
            for (&child, local) in &self.transform_hierarchy {
                if !range.contains(&child)
                    || initialized.contains(&child)
                    || (range.contains(&local.parent)
                        && self.transform_hierarchy.contains_key(&local.parent)
                        && !initialized.contains(&local.parent))
                {
                    continue;
                }
                if local.socket.is_some() {
                    initialized.insert(child);
                    continue;
                }
                let pose = parent_cache(&self.entities[local.parent]);
                let (position, rotation) = super::native_transform_record::world_pose(
                    &pose,
                    local.position,
                    &local.rotation,
                );
                publish(
                    child,
                    position,
                    rotation,
                    &mut self.entities,
                    &mut self.draws,
                );
                self.entities[child].authored_rotation = rotation;
                self.entities[child].authored_transform = self.entities[child].transform;
                initialized.insert(child);
            }
            ensure!(initialized.len() > before, "Unresolved transform hierarchy");
        }
        self.update_skeletal_children_range(range.clone())?;
        // Body construction/admission needs the pose after initial attachment
        // binding, not the temporary pose used while cloning its template.
        // As with ordinary parents above, capture this once for new entities;
        // later animation updates must not replace the physics baseline.
        for child in range {
            if self
                .transform_hierarchy
                .get(&child)
                .is_some_and(|local| local.socket.is_some())
            {
                self.entities[child].authored_rotation = self.entities[child].runtime_rotation;
                self.entities[child].authored_transform = self.entities[child].transform;
            }
        }
        Ok(())
    }
    pub(super) fn bind_rotators(&mut self) -> Result<()> {
        self.bind_rotators_range(0..self.entities.len())
    }
    pub(super) fn bind_rotators_range(&mut self, range: std::ops::Range<usize>) -> Result<()> {
        for entity in range {
            let has_children = self.entities.iter().any(|child| {
                child.scene == self.entities[entity].scene
                    && child.transform_parent_id().ok().flatten() == Some(self.entities[entity].id)
            });
            let indices = self.entities[entity]
                .components
                .iter()
                .enumerate()
                .filter_map(|(i, c)| (c.class_name == "RotatorComponent").then_some(i))
                .collect::<Vec<_>>();
            for index in indices {
                let result = (|| -> Result<_> {
                    ensure!(
                        self.entities[entity].transform_parent_id()?.is_none()
                            || self.transform_hierarchy.contains_key(&entity),
                        "Rotator parent cache is not connected"
                    );
                    ensure!(
                        !has_children
                            || self
                                .entities
                                .iter()
                                .enumerate()
                                .filter(|(_, child)| child.scene == self.entities[entity].scene
                                    && child.transform_parent_id().ok().flatten()
                                        == Some(self.entities[entity].id))
                                .all(|(child, _)| self.transform_hierarchy.contains_key(&child)),
                        "Rotator child cache is not connected"
                    );
                    let component = &mut self.entities[entity].components[index];
                    let p = parameters(component)?;
                    let instance = Instance {
                        state: rotator::State::new(
                            p.max_speed_degrees,
                            boolean(component, "Start Reversed")?,
                        )?,
                        disabled: boolean(component, "RCStartDisabled")?,
                        local_axis: boolean(component, "LocalAxisOfRotation")?,
                        rotate_to_position: false,
                        entity_target: None,
                    };
                    let axis = axis_normalize(point(component)?)?;
                    component
                        .properties
                        .iter_mut()
                        .find(|p| p.name == "Axis of Rotation")
                        .unwrap()
                        .value = serde_json::json!({"x":axis[0],"y":axis[1],"z":axis[2]});
                    Ok(((entity, component.link_id.to_u32()), instance))
                })();
                match result {
                    Ok((key, state)) => {
                        self.rotators.insert(key, state);
                    }
                    Err(error) => self
                        .report
                        .unsupported
                        .push(format!("Rotator {}: {error:#}", self.entities[entity].name)),
                }
            }
        }
        self.report.geometry.push(
            serde_json::json!({"kind":"rotator_runtime","bound":self.rotators.len(),
            "target_inputs_and_event_registry_complete":false,"original_runtime_validated":false}),
        );
        Ok(())
    }
    /// Exactly once at a simulation boundary, before character preparation.
    pub fn prepare_scene_motion(&mut self, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt > 0., "Invalid scene-motion step");
        if let Some((time, prepared_dt)) = self.scene_motion_prepared_at {
            if time == self.dynamic_runtime_time {
                ensure!(
                    dt == prepared_dt,
                    "Scene motion prepared with a different timestep"
                );
                return Ok(());
            }
        }
        for (&(entity, id), instance) in &mut self.rotators {
            if self.removed_entities.contains(&entity)
                || !self.scene_resources.is_active(&self.entities[entity].scene)
            {
                continue;
            }
            if instance.disabled {
                continue;
            }
            let runtime = &self.entities[entity];
            let component = runtime
                .components
                .iter()
                .find(|c| c.link_id.to_u32() == id)
                .context("Rotator component disappeared")?;
            let tracked = match instance.entity_target {
                Some(target) => target,
                None => {
                    let reference = value(component, "RotateToEntity")?
                        .as_u64()
                        .context("Invalid rotator entity reference")?;
                    if reference == 0 {
                        None
                    } else {
                        self.entities.iter().position(|candidate| {
                            candidate.scene == runtime.scene && u64::from(candidate.id) == reference
                        })
                    }
                }
            }
            .filter(|target| !self.removed_entities.contains(target));
            let mut outside_range = false;
            let target = if let Some(tracked) = tracked {
                let axis = point(component)?;
                let axis = if instance.local_axis {
                    matrix::transform(&runtime.runtime_rotation, axis)
                } else {
                    axis
                };
                let source = runtime.transform[12..15].try_into().unwrap();
                let destination = self
                    .entities
                    .get(tracked)
                    .context("Rotator target identity missing")?
                    .transform[12..15]
                    .try_into()
                    .unwrap();
                let angle = super::rotator_tracking::target_angle(
                    instance.state.angle,
                    axis,
                    &runtime.runtime_rotation,
                    source,
                    destination,
                    float(component, "RotateToEntityDistance")?,
                )?;
                outside_range = angle.is_none();
                Some(angle.unwrap_or(instance.state.angle))
            } else {
                instance
                    .rotate_to_position
                    .then(|| float(component, "RotateToPosition"))
                    .transpose()?
            };
            let mut step = instance
                .state
                .tick(parameters(component)?, dt, false, target)?;
            //3b3768 returns zero delta with the requested-angle slot still
            //containing angle+freeDelta. It does not signal target completion.
            if outside_range {
                step.completion_event = false;
            }
            if step.completion_event {
                //3b3158..3170: source=owner, header=null; ordinary broadcast
                // precedes applying the final matrix. Keep the shared registry
                // and authored IO delays rather than fabricating an activator.
                self.deferred_event_runtime.dispatch_ordinary_output(
                    &mut self.entity_event_registry,
                    super::entity_events::Basic {event_kind:0x0d,source:Some(entity),header:None},
                    &mut self.outputs, &self.entities, &self.scene_resources, self.animation_time,
            &mut self.ai_brains,&self.robot_arm_controls,&self.cutscene_actions,
                )?;
            }
            apply_rotation(
                entity,
                id,
                instance.local_axis,
                step.delta,
                &mut self.transform_hierarchy,
                &mut self.entities,
                &mut self.draws,
            )?;
        }
        self.prepare_keyframe_targets(dt)?;
        self.scene_motion_prepared_at = Some((self.dynamic_runtime_time, dt));
        Ok(())
    }
    fn prepare_keyframe_targets(&mut self, dt: f32) -> Result<()> {
        let identity = hk::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        for &source in self.dynamic_runtime_sources.keys() {
            let entity_index = self.dynamic_bodies[source].entity;
            let targets = self.dynamic_constructions[&source]
                .node_bindings
                .iter()
                .map(|binding| {
                    binding
                        .map(|binding| {
                            self.rigid_model_node_frame(entity_index, binding.node, true)
                                .map(|frame| hk::compose(&frame, &binding.offset))
                        })
                        .transpose()
                })
                .collect::<Result<Vec<_>>>()?;
            let state = self
                .dynamic_constructions
                .get_mut(&source)
                .context("Keyframe construction missing")?;
            for (index, body) in state.bodies.iter_mut().enumerate() {
                if body.motion.current.motion_type() != 4 {
                    continue;
                }
                let Some(data) = &body.auxiliary.user_data else {
                    continue;
                };
                if data.byte_08 != 0 {
                    continue;
                }
                ensure!(
                    data.word_04 == 0,
                    "Named-node keyframe binding is not connected"
                );
                let binding = data
                    .dynamic_binding
                    .context("Keyframe body lacks original local transform")?;
                let offset = binding.local_transform.map(f32::from_bits);
                let entity = &self.entities[self.dynamic_bodies[source].entity];
                let mut target = entity.physics_transform();
                let near = (0..16)
                    .filter(|i| i % 4 != 3)
                    .all(|i| (offset[i] - identity[i]).abs() <= 0.001);
                if let Some(node_target) = targets[index] {
                    target = node_target;
                } else if !near {
                    target = hk::compose(&target, &offset);
                }
                let q = hk::rotation_quaternion(&target)?;
                hk::set_rotation(&mut target, q);
                let q = hk::rotation_quaternion_raw(&target)?;
                let world = &mut self.collision;
                let before = (
                    body.linear_velocity().map(f32::to_bits),
                    body.angular_velocity().map(f32::to_bits),
                );
                body.update_target(target[12..16].try_into().unwrap(), q, 1. / dt, |body| {
                    world.wake_simulation_body(
                        *body.shapes.first().context("Keyframe shape missing")?,
                    )
                })?;
                if before
                    != (
                        body.linear_velocity().map(f32::to_bits),
                        body.angular_velocity().map(f32::to_bits),
                    )
                {
                    world.set_simulation_velocities(
                        &body.shapes,
                        body.linear_velocity()[..3].try_into().unwrap(),
                        body.angular_velocity()[..3].try_into().unwrap(),
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod socket_regressions {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn dbc_eye_motor_preserves_its_clock_while_its_scene_is_inactive() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level", &egui::Context::default())?;
        let eye = world.scripts.resolve_name("dbc_1st_zonea_dynamic.robotArm_eyePivot 01")
            .context("Original eye pivot missing")?;
        let key = *world.rotators.keys().find(|(owner, _)| *owner == eye)
            .context("Original eye motor missing")?;
        let scene = world.entities[eye].scene.clone();
        world.rotators.get_mut(&key).unwrap().disabled = false;
        let before = world.rotators[&key].state;
        let pose = world.entities[eye].transform;
        world.scene_resources.deactivated(&scene);
        world.prepare_scene_motion(super::super::STEP)?;
        let after = world.rotators[&key].state;
        assert_eq!(before.angle.to_bits(), after.angle.to_bits());
        assert_eq!(before.speed.to_bits(), after.speed.to_bits());
        assert_eq!(pose, world.entities[eye].transform);
        world.scene_resources.published(&scene);
        world.dynamic_runtime_time += super::super::STEP;
        world.prepare_scene_motion(super::super::STEP)?;
        assert_ne!(before.angle.to_bits(), world.rotators[&key].state.angle.to_bits());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn dbc_eye_rotation_survives_socket_publication() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level", &egui::Context::default())?;
        let eye = world.entities.iter().position(|e| e.name == "dbc_1st_zonea_dynamic.robotArm_eyePivot 01")
            .context("Original eye pivot missing")?;
        let (&(_, id), _) = world.rotators.iter().find(|((owner, _), _)| *owner == eye)
            .context("Original eye rotator missing")?;
        ensure!(world.transform_hierarchy[&eye].socket.as_ref().is_some_and(|s| s.mode == 4),
            "Expected original bone-relative eye pivot");
        world.update_skeletal_children_range_strict(eye..eye + 1)?;
        for _ in 0..60 {
            apply_rotation(eye, id, true, 0.02, &mut world.transform_hierarchy,
                &mut world.entities, &mut world.draws)?;
            let expected = world.entities[eye].runtime_rotation;
            world.update_skeletal_children_range_strict(eye..eye + 1)?;
            let error = expected.iter().zip(world.entities[eye].runtime_rotation)
                .map(|(a,b)| (a-b).abs()).fold(0f32, f32::max);
            ensure!(error < 0.0001, "Socket publication reapplied bone rotation: {error}");
        }
        Ok(())
    }
}

#[cfg(any(test, debug_assertions))]
pub(super) fn validate_original_commands() -> Result<()> {
    use epic_mickey_lib_rs::scene_file::{Component, Property, ID};
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world = super::assets::load(
        std::path::Path::new(&root),
        "Levels/GV_ZoneA_Start.level",
        &egui::Context::default(),
    )?;
    world.advance_models(0.)?;
    let (&(entity, id), _) = world
        .rotators
        .iter()
        .find(|(&(e, _), i)| {
            i.state.speed > 0.
                && world.dynamic_runtime_sources.keys().any(|s| {
                    world.dynamic_bodies[*s].entity == e
                        && world.dynamic_constructions[s]
                            .bodies
                            .iter()
                            .any(|b| b.motion.current.motion_type() == 4)
                })
        })
        .context("Original rotator with admitted keyframed body missing")?;
    let shape = world
        .dynamic_runtime_sources
        .keys()
        .filter(|s| world.dynamic_bodies[**s].entity == entity)
        .flat_map(|s| world.dynamic_constructions[s].bodies.iter())
        .find(|b| b.motion.current.motion_type() == 4)
        .context("Missing driven body")?
        .shapes[0];
    let before_body = world.collision.simulation_pose(shape)?.orientation;
    for instance in world.rotators.values_mut() {
        instance.disabled = true;
    }
    world.rotators.get_mut(&(entity, id)).unwrap().disabled = false;
    let name = world.entities[entity].name.clone();
    let component = world.entities[entity]
        .components
        .iter_mut()
        .find(|c| c.link_id.to_u32() == id)
        .unwrap();
    for (key, value) in [
        ("Max Speed", serde_json::json!(90.)),
        ("Acceleration", serde_json::json!(0.)),
        ("MaxRotationDegrees", serde_json::json!(0)),
        ("MinRotationDegrees", serde_json::json!(0)),
        ("AutoReverse", serde_json::json!(false)),
    ] {
        component
            .properties
            .iter_mut()
            .find(|p| p.name == key)
            .context("Original rotator parameter missing")?
            .value = value;
    }
    world.run_callback_fixture("bwx_position_hits=0; function BwxPositionReached() assert(_self()==_activator()); bwx_position_hits=bwx_position_hits+1 end")?;
    let fields = [
        ("Output", serde_json::json!(["OnReachedPosition"])),
        ("Input", serde_json::json!(["BwxPositionReached"])),
        ("Target", serde_json::json!(["_self"])),
        ("Parameters", serde_json::json!([""])),
        ("DelayWait", serde_json::json!([0.])),
        ("IoFlags", serde_json::json!([0])),
    ];
    let io = Component::new(
        "InputOutputComponent".into(),
        "position fixture".into(),
        ID { id: 0 },
        ID { id: 0 },
        ID { id: 0 },
        fields
            .into_iter()
            .map(|(name, value)| {
                Property::new(String::new(), name.into(), false, false, false, value)
            })
            .collect(),
    );
    world.outputs = super::actor_io::Outputs::default();
    world.outputs.bind(entity, &name, &[io])?;
    let original = world.entities[entity].runtime_rotation;
    world.run_callback_fixture(&format!(
        "assert(select('#',InstantRotateToPosition({name:?},30))==0)"
    ))?;
    let radians = |n: f32| n * f32::from_bits(0x3c8efa35);
    ensure!(
        world.rotators[&(entity, id)].state.angle == radians(30.)
            && world.rotators[&(entity, id)].state.speed == 0.,
        "Instant command lost native angle/speed"
    );
    ensure!(
        world.entities[entity].runtime_rotation != original,
        "Instant rotation did not update the visible owner"
    );
    let instant = world.entities[entity].runtime_rotation;
    world.run_callback_fixture(&format!(
        "assert(select('#',StartRotateToPosition({name:?},60))==0)"
    ))?;
    ensure!(
        world.entities[entity].runtime_rotation == instant,
        "Start command teleported the model"
    );
    for tick in 1..=60 {
        world.advance_dynamic_physics(super::STEP)?;
        world.advance_models_with_delta(tick as f32 * super::STEP, super::STEP)?;
    }
    ensure!(
        world.rotators[&(entity, id)].state.angle == radians(60.),
        "Position command never reached its target"
    );
    world.run_callback_fixture(
        "assert(bwx_position_hits>0,'position completion never reached owner IO')",
    )?;
    ensure!(
        world.collision.simulation_pose(shape)?.orientation != before_body,
        "Target rotation never reached the collider"
    );
    ensure!(
        world.entities[entity]
            .draws
            .clone()
            .all(|d| world.draws[d].trans_mat == world.entities[entity].transform),
        "Visible draws diverged from owner pose"
    );
    world.run_callback_fixture(&format!("InstantRotateToPosition({name:?},10)"))?;
    ensure!(
        world.rotators[&(entity, id)].rotate_to_position,
        "Instant command incorrectly cancelled active targeting"
    );
    let target = float(
        world.entities[entity]
            .components
            .iter()
            .find(|c| c.link_id.to_u32() == id)
            .unwrap(),
        "RotateToPosition",
    )?;
    ensure!(
        target == radians(60.),
        "Instant command overwrote retained target property"
    );
    world.run_callback_fixture(&format!("StopRotateToPosition({name:?},0/0)"))?;
    ensure!(
        !world.rotators[&(entity, id)].rotate_to_position,
        "Stop command did not clear target mode"
    );
    let stopped = world.rotators[&(entity, id)].state.angle;
    world.advance_dynamic_physics(super::STEP)?;
    ensure!(
        world.rotators[&(entity, id)].state.angle > stopped,
        "Stop incorrectly stopped the free rotator motor"
    );
    let player = world.player_entity;
    // The original free wheel has no tracking range. Configure its reflected
    // runtime distance explicitly before exercising the entity-target mode.
    world.entities[entity]
        .components
        .iter_mut()
        .find(|c| c.link_id.to_u32() == id)
        .unwrap()
        .properties
        .iter_mut()
        .find(|p| p.name == "RotateToEntityDistance")
        .context("Tracking distance missing")?
        .value = serde_json::json!(100.);
    world.run_callback_fixture(&format!("assert(select('#',SetPropertyEntity(GetEntity({name:?}),'RotateToEntity',GetPlayer()))==0); SetPropertyEntity(GetEntity({name:?}),'MissingEntityProperty',GetPlayer()); SetPropertyEntity('no such entity','RotateToEntity',GetPlayer())"))?;
    ensure!(
        world.rotators[&(entity, id)].entity_target == Some(Some(player)),
        "Entity setter did not retain actual player identity"
    );
    let source: [f32; 3] = world.entities[entity].transform[12..15].try_into().unwrap();
    let mut tracking_poses = Vec::new();
    for offset in [[3., 0., 4.], [-4., 0., -3.]] {
        world.entities[player].transform[12..15]
            .copy_from_slice(&std::array::from_fn::<_, 3, _>(|i| source[i] + offset[i]));
        for _ in 0..60 {
            world.dynamic_runtime_time += super::STEP;
            world.scene_motion_prepared_at = None;
            world.prepare_scene_motion(super::STEP)?;
        }
        tracking_poses.push(world.entities[entity].runtime_rotation);
    }
    ensure!(tracking_poses[0]!=tracking_poses[1],"Tracking did not follow the moving player's actual position: angle={}, poses={tracking_poses:?}",world.rotators[&(entity,id)].state.angle);
    let previous = world.entities[entity].runtime_rotation;
    world.entities[player].transform[12..15].copy_from_slice(&[
        source[0] + 10000.,
        source[1],
        source[2],
    ]);
    world.dynamic_runtime_time += super::STEP;
    world.scene_motion_prepared_at = None;
    world.prepare_scene_motion(super::STEP)?;
    ensure!(
        world.entities[entity].runtime_rotation == previous,
        "Out-of-range entity tracking did not hold its pose"
    );
    world.run_callback_fixture(&format!(
        "SetPropertyEntity(GetEntity({name:?}),'RotateToEntity',nil)"
    ))?;
    ensure!(
        world.rotators[&(entity, id)].entity_target == Some(None),
        "Explicit null did not clear entity tracking"
    );
    world.dynamic_runtime_time += super::STEP;
    world.scene_motion_prepared_at = None;
    world.prepare_scene_motion(super::STEP)?;
    ensure!(
        world.entities[entity].runtime_rotation != previous,
        "Cleared tracking did not restore the authored free motor"
    );
    std::fs::write(
        "target/rotator-original-command-validation.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"entity":name,"start_reached":true,"instant_visible":true,"collider_followed":true,"completion_io":true,"stop_resumes_free_rotation":true,"instant_retains_target":true}),
        )?,
    )?;
    std::fs::write(
        "target/rotator-entity-tracking-validation.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"entity":name,"player_target":player,"tracking_poses":tracking_poses,"out_of_range_holds":true,"null_resumes_free_rotation":true,"complete_entity_property_api":false,"original_runtime_pose_parity":false}),
        )?,
    )?;
    println!(
        "PASS: original rotator commands, live entity tracking, collider and owner completion IO"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_rotator_limit_emits_owner_io_once_and_holds_final_pose() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{Component, Property, ID};
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let (&(entity, id), _) = world
            .rotators
            .iter()
            .find(|(&(e, _), i)| {
                i.state.speed > 0. && world.entities[e].name.contains("GV_ZoneA_GearShredder01 ")
            })
            .context("Original rotating gear missing")?;
        let original_speed = world.rotators[&(entity, id)].state.speed;
        world.set_rotator_speed(entity, 75.)?;
        assert_eq!(
            world.rotators[&(entity, id)].state.speed,
            original_speed,
            "Float setter bypassed native speed approach"
        );
        assert!(world.set_rotator_speed(entity, f32::NAN).is_err());
        // Owned-instance limit/IO fixture; immutable scene and body assets keep
        // their original values. The actual World dispatch path is exercised.
        let component = world.entities[entity]
            .components
            .iter_mut()
            .find(|c| c.link_id.to_u32() == id)
            .unwrap();
        for (name, value) in [
            ("Max Speed", serde_json::json!(75.)),
            ("MaxRotationDegrees", serde_json::json!(1)),
            ("MinRotationDegrees", serde_json::json!(0)),
            ("AutoReverse", serde_json::json!(false)),
        ] {
            component
                .properties
                .iter_mut()
                .find(|p| p.name == name)
                .unwrap()
                .value = value;
        }
        world.run_callback_fixture("bwx_rotator_reached=0; function BwxRotatorReached() assert(_activator()==nil, 'bwx fabricated rotator activator'); GetPosition(_self()); bwx_rotator_reached=bwx_rotator_reached+1 end; function BwxVerifyZero() assert(bwx_rotator_reached==0, 'bwx premature completion') end; function BwxVerifyOne() assert(bwx_rotator_reached==1, 'bwx completion missing or duplicated') end")?;
        // Install fixture callbacks synchronously: original coroutine removal
        // can defer a scheduled tail task. Keep normal source scheduling intact
        // and exercise completion via actual owner IO dispatch below.
        world.advance_models(0.)?;
        world.dispatch_script_callback("BwxVerifyZero", entity)?;
        let fields = [
            (
                "Output",
                serde_json::json!(["OnReachedPosition", "OnReachedPosition"]),
            ),
            (
                "Input",
                serde_json::json!(["SetPropertyFloat", "BwxRotatorReached"]),
            ),
            ("Target", serde_json::json!(["_self", "_self"])),
            ("Parameters", serde_json::json!(["Max Speed,0", ""])),
            ("DelayWait", serde_json::json!([0., 0.])),
            ("IoFlags", serde_json::json!([0, 0])),
        ];
        let io = Component::new(
            "InputOutputComponent".into(),
            "limit fixture".into(),
            ID { id: 0 },
            ID { id: 0 },
            ID { id: 0 },
            fields
                .into_iter()
                .map(|(name, value)| {
                    Property::new(String::new(), name.into(), false, false, false, value)
                })
                .collect(),
        );
        world.outputs = super::super::actor_io::Outputs::default();
        world
            .outputs
            .bind(entity, &world.entities[entity].name, &[io])?;
        world.bind_deferred_output_events(entity)?;
        let before_reached = world.deferred_event_runtime.reached_position_deliveries;
        world.advance_dynamic_physics(super::super::STEP)?;
        assert_eq!(world.deferred_event_runtime.reached_position_deliveries, before_reached + 1);
        let final_rotation = world.entities[entity].runtime_rotation;
        assert_ne!(final_rotation, world.entities[entity].authored_rotation);
        assert_eq!(world.rotators[&(entity, id)].state.speed, 0.);
        world.advance_models(super::super::STEP)?;
        assert_eq!(
            float(
                world.entities[entity]
                    .components
                    .iter()
                    .find(|c| c.link_id.to_u32() == id)
                    .unwrap(),
                "Max Speed"
            )?,
            0.,
            "Reached-position IO failed to invoke the native speed setter"
        );
        world.dispatch_script_callback("BwxVerifyOne", entity)?;
        for tick in 2..=5 {
            world.advance_dynamic_physics(super::super::STEP)?;
            assert_eq!(
                world.entities[entity].runtime_rotation, final_rotation,
                "Completed gear kept turning"
            );
            assert!(
                world
                    .outputs
                    .ready(tick as f32 * super::super::STEP)
                    .is_empty(),
                "Completion repeated without motion"
            );
            world.advance_models(tick as f32 * super::super::STEP)?;
            world.dispatch_script_callback("BwxVerifyOne", entity)?;
        }
        assert!(
            !world
                .report
                .unsupported
                .iter()
                .any(|m| m.contains("OnReachedPosition -> BwxRotatorReached")
                    || m.contains("OnReachedPosition -> SetPropertyFloat")),
            "{:?}",
            world
                .report
                .unsupported
                .iter()
                .filter(|m| m.contains("OnReachedPosition"))
                .collect::<Vec<_>>()
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_rotators_drive_render_targets_and_live_colliders() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let authored = world
            .entities
            .iter()
            .flat_map(|e| &e.components)
            .filter(|c| c.class_name == "RotatorComponent")
            .count();
        eprintln!(
            "GV rotators: authored={authored} bound={}",
            world.rotators.len()
        );
        for (&(entity, id), instance) in &world.rotators {
            eprintln!(
                "Rotator {} id={id} speed={} disabled={} local={}",
                world.entities[entity].name,
                instance.state.speed,
                instance.disabled,
                instance.local_axis
            );
        }
        for message in world
            .report
            .unsupported
            .iter()
            .filter(|m| m.starts_with("Rotator ") || m.starts_with("Transform hierarchy "))
        {
            eprintln!("{message}");
        }
        // The startup loader now includes the authored airlock part. Its
        // two doors and valve add three components to the original core30.
        assert_eq!(authored, 33);
        let airlock = world
            .entities
            .iter()
            .filter(|e| e.name.starts_with("gv_zonea_airlock."))
            .flat_map(|e| &e.components)
            .filter(|c| c.class_name == "RotatorComponent")
            .count();
        assert_eq!(airlock, 3);
        assert_eq!(authored - airlock, 30);
        assert_eq!(
            world.rotators.len(),
            authored,
            "Some authored GV rotators failed to bind"
        );
        let driven = world
            .rotators
            .iter()
            .filter(|(_, i)| !i.disabled && i.state.speed != 0.)
            .map(|(&(entity, _), _)| entity)
            .collect::<Vec<_>>();
        assert!(!driven.is_empty(), "GV contains no active rotators");
        let initial = driven
            .iter()
            .map(|&entity| (entity, world.entities[entity].runtime_rotation))
            .collect::<Vec<_>>();
        let bodies = world
            .dynamic_runtime_sources
            .keys()
            .copied()
            .filter(|source| driven.contains(&world.dynamic_bodies[*source].entity))
            .collect::<Vec<_>>();
        assert!(
            !bodies.is_empty(),
            "No live colliders belong to active rotators"
        );
        let before = bodies
            .iter()
            .map(|&s| {
                (
                    s,
                    world
                        .collision
                        .simulation_pose(world.dynamic_constructions[&s].bodies[0].shapes[0])
                        .unwrap()
                        .orientation,
                )
            })
            .collect::<Vec<_>>();
        let platforms = driven
            .iter()
            .copied()
            .filter(|&e| world.entities[e].name.contains("GV_ZoneA_Platform01 "))
            .map(|e| {
                (
                    e,
                    world.entities[e].transform,
                    world.entities[e].runtime_rotation,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            platforms.len(),
            4,
            "Wheel assembly lost an attached platform"
        );
        for &(e, _, _) in &platforms {
            assert!(
                world.transform_hierarchy.contains_key(&e),
                "Platform has no parent binding"
            );
            assert!(
                bodies.iter().any(|&s| world.dynamic_bodies[s].entity == e),
                "Platform collider was not admitted"
            );
        }
        for tick in 0..120 {
            world.prepare_scene_motion(super::super::STEP)?;
            let targets = driven
                .iter()
                .map(|&e| (e, world.entities[e].runtime_rotation))
                .collect::<Vec<_>>();
            // A repeated call at the same boundary must not rotate twice.
            world.prepare_scene_motion(super::super::STEP)?;
            for &(e, r) in &targets {
                assert_eq!(world.entities[e].runtime_rotation, r);
            }
            world.advance_dynamic_physics(super::super::STEP)?;
            for &(e, r) in &targets {
                assert_eq!(
                    world.entities[e].runtime_rotation, r,
                    "Physics replaced the rotator-owned target"
                );
                for draw in world.entities[e].draws.clone() {
                    assert_eq!(world.draws[draw].trans_mat, world.entities[e].transform);
                }
            }
            if tick == 0 {
                assert!(
                    bodies.iter().any(|s| super::super::physics::length(
                        world
                            .collision
                            .simulation_pose(world.dynamic_constructions[s].bodies[0].shapes[0])
                            .unwrap()
                            .angular
                    ) > 0.01),
                    "Rotating geometry left its colliders without angular motion"
                );
            }
        }
        assert!(initial
            .iter()
            .all(|&(e, r)| world.entities[e].runtime_rotation != r));
        assert!(before.iter().any(|&(s, q)| world
            .collision
            .simulation_pose(world.dynamic_constructions[&s].bodies[0].shapes[0])
            .unwrap()
            .orientation
            != q));
        for (e, transform, rotation) in platforms {
            let current = &world.entities[e];
            assert!(
                super::super::physics::length(std::array::from_fn(
                    |i| current.transform[12 + i] - transform[12 + i]
                )) > 0.1,
                "Attached platform did not orbit with its wheel"
            );
            let error = current
                .runtime_rotation
                .iter()
                .zip(rotation)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                error < 0.001,
                "Counter-rotating platform tipped instead of remaining upright: {error}"
            );
        }
        assert!((world.dynamic_runtime_time - 4.).abs() < 1e-5);
        Ok(())
    }
}
