//! Authored weight switches driven by Box3D support and scene I/O.
use super::{assets::World, physics::Player};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Plates {
    scanned: usize,
    entries: BTreeMap<usize, Plate>,
}

struct Plate {
    component: usize,
    weight: f32,
    down_rate: f32,
    up_rate: f32,
    maximum: f32,
    player_only: bool,
    starts_disabled: bool,
    footprint: Option<[f32; 2]>,
    depth: f32,
    active: bool,
}

impl World {
    pub(super) fn advance_pressure_plates(&mut self, player: &Player, dt: f32) -> Result<()> {
        let plates = &mut self.pressure_plates;
        for (entity, source) in self.entities.iter().enumerate().skip(plates.scanned) {
            let Some((index, component)) = source.components.iter().enumerate()
                .find(|(_, c)| c.class_name == "PressurePlateComponent") else { continue };
            let field = |name| component.properties.iter().find(|p| p.name == name).map(|p| &p.value);
            let number = |name| -> Result<f32> {
                let value = field(name).and_then(|v| v.as_f64()).unwrap_or(0.) as f32;
                ensure!(value.is_finite() && value >= 0., "Invalid pressure plate {name}");
                Ok(value)
            };
            let footprint = source.components.iter().find(|body| body.class_name == "PhysicsRigidBodyDynamicComponent")
                .and_then(|body| body.properties.iter().find(|p| p.name == "ObbExtents"))
                .and_then(|p| p.value.as_array()?.first())
                .and_then(|obb| Some([obb.get("x")?.as_f64()? as f32, obb.get("z")?.as_f64()? as f32]));
            plates.entries.insert(entity, Plate {
                component: index, weight: number("Weight Requirement")?,
                down_rate: number("Descent Rate")?, up_rate: number("Climb Rate")?,
                maximum: number("Maximum Descent Distance")?,
                player_only: field("Player Only").and_then(|v| v.as_bool()).unwrap_or(false),
                starts_disabled: field("Starts Disabled").and_then(|v| v.as_bool()).unwrap_or(false),
                footprint,
                depth: 0., active: false,
            });
        }
        plates.scanned = self.entities.len();
        if plates.entries.is_empty() { return Ok(()); }

        let player_support = if player.grounded {
            self.collision.ray_hit(super::physics::add(player.position, [0., 0.12, 0.]), [0., -1., 0.], 0.30)
                .filter(|hit| hit.normal[1] > 0.5).and_then(|hit| self.collision_entity_index(hit.owner))
        } else { None };
        let entities = self.pressure_plates.entries.keys().copied().collect::<Vec<_>>();
        for entity in entities {
            if self.removed_entities.contains(&entity) || !self.entity_scene_active(entity) { continue; }
            let plate = &self.pressure_plates.entries[&entity];
            let disabled = self.component_control.disabled.get(&entity).and_then(|flags| flags.get(&plate.component))
                .copied().unwrap_or(plate.starts_disabled);
            let mut weight = 0.;
            let source = &self.entities[entity];
            let offset = super::physics::sub(player.position, [source.transform[12], source.transform[13], source.transform[14]]);
            // A flush plate can share its ground manifold with the surrounding
            // frame. In that case the cooked plate footprint disambiguates
            // which grounded actor is applying weight to the switch.
            let over_plate = (player.grounded || plate.active)
                && (-0.08..=0.46).contains(&offset[1]) && plate.footprint.is_some_and(|extents| {
                let r = source.runtime_rotation;
                let x = r[0] * offset[0] + r[6] * offset[2];
                let z = r[2] * offset[0] + r[8] * offset[2];
                x.abs() <= extents[0] * 0.5 && z.abs() <= extents[1] * 0.5
            });
            if !disabled && (player_support == Some(entity) || over_plate) {
                let inverse = self.player_physics.as_ref().map(|body| body.record.solver_inverse_mass())
                    .transpose()?.map_or(0., |(_, mass)| mass);
                weight = if inverse > 0. { inverse.recip() } else { plate.weight.max(1.) };
            }
            if !disabled && !plate.player_only && weight < plate.weight.max(f32::EPSILON) {
                let mut touching = BTreeSet::new();
                let plate_height = self.entities[entity].transform[13];
                let owners = self.dynamic_runtime_sources.iter().filter_map(|(&source, &owner)|
                    (self.dynamic_bodies[source].entity == entity).then_some(owner));
                for shape in owners.flat_map(|owner| self.collision.owner_shapes(owner)) {
                    for pair in self.collision.touching_shape_pairs(shape)? {
                        if let Some(other) = pair.into_iter().find(|other| *other != shape)
                            .and_then(|other| self.collision.shape_owner(other))
                            .and_then(|owner| self.collision_entity_index(owner))
                            .filter(|other| *other != self.player_entity && *other != entity
                                && self.entities.get(*other).is_some_and(|source| source.transform[13] > plate_height))
                        {
                            touching.insert(other);
                        }
                    }
                }
                for (source, construction) in &self.dynamic_constructions {
                    if !touching.contains(&self.dynamic_bodies[*source].entity) { continue; }
                    for body in &construction.bodies {
                        let (_, inverse) = body.motion.current.solver_inverse_mass()?;
                        if inverse > 0. { weight += inverse.recip(); }
                    }
                }
            }
            let occupied = !disabled && weight > 0. && weight >= plate.weight;
            let plate = self.pressure_plates.entries.get_mut(&entity).context("Missing pressure plate")?;
            let old_depth = plate.depth;
            plate.depth = if occupied {
                (plate.depth + dt * plate.down_rate).min(plate.maximum)
            } else {
                (plate.depth - dt * plate.up_rate).max(0.)
            };
            let delta = old_depth - plate.depth;
            let event = if occupied && !plate.active {
                plate.active = true;
                Some("OnStart")
            } else if !occupied && plate.depth == 0. && plate.active {
                plate.active = false;
                (!disabled).then_some("OnStop")
            } else { None };
            if delta != 0. {
                let source = &mut self.entities[entity];
                source.transform[13] += delta;
                for draw in source.draws.clone() { self.draws[draw].trans_mat = source.transform; }
                super::rotator_runtime::accept_physics_pose(entity, &mut self.transform_hierarchy,
                    &mut self.entities, &mut self.draws)?;
            }
            if let Some(event) = event { self.emit_entity_output(entity, event, entity); }
        }
        Ok(())
    }
}
