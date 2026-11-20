//! Integration for the explicitly provisional brush preview. Uses recovered
//! camera/range geometry, but nearest static collision is not the original
//! Cursor3D collector/filter/target selection and must not be treated as such.
use super::{
    cursor,
    physics::{Collision, V},
};
use anyhow::{ensure, Result};

pub fn target(collision: &Collision, ray: cursor::Ray, owner: V, range: f32) -> Result<V> {
    ensure!(
        range.is_finite() && range > 0.,
        "Invalid brush preview range"
    );
    let end = cursor::preferred_endpoint(owner, range, ray.origin, ray.direction);
    let vector = super::brush::target_direction(ray.origin, end);
    ensure!(
        end.iter().all(|v| v.is_finite()),
        "Invalid brush preview endpoint"
    );
    if vector.distance == 0. {
        return Ok(end);
    }
    // Physical obstruction rays exclude erased jigsaws. Brush aiming must
    // retain that geometry, as projectile sweeps do, or camera/emitter parallax
    // sends restoration shots toward a surface behind the intended piece.
    // Nearest-hit selection remains provisional, not the native Cursor3D policy.
    let translation = super::physics::sub(end, ray.origin);
    let hit = collision
        .with_player_queries(|| collision.ray_hits(ray.origin, translation))?
        .into_iter()
        .filter(|hit| hit.fraction > 0.)
        .min_by(|a, b| a.fraction.total_cmp(&b.fraction));
    Ok(hit.map_or(end, |hit| hit.position))
}

/// Swept-sphere compatibility tracing, not the retail collision callback. Uses
/// authored speed, lifetime, emission cadence, geometry and body filter fields.
/// Production admission resolves live body/collection filters before selecting
/// the nearest accepted hit. Original rigid-body/group lifecycle and collision
/// callback ordering remain open.
/// The firing host owns ammo consumption; this collector fabricates no events. World integration applies
/// provisional jigsaw material/collision endpoints to these contacts.
#[derive(Default)]
pub struct Projectiles {
    pub pool: super::brush::ProjectilePool,
    pub emitted: u64,
    pub contacts: u64,
    pub expired: u64,
    pub last_contact: Option<Contact>,
    pub step_contacts: Vec<Contact>,
    radii: Vec<f32>,
    filters: Vec<u32>,
}
#[derive(Clone)]
pub struct Contact {
    pub ink: super::brush::Ink,
    pub projectile: u64,
    pub hit: super::physics::SweepHit,
}
impl Projectiles {
    pub fn spawn(
        &mut self,
        report: &super::assets::Report,
        ink: super::brush::Ink,
        owner: u64,
        origin: V,
        target: V,
    ) -> Result<()> {
        use anyhow::Context;
        let settings = report
            .brush_settings
            .as_ref()
            .context("Missing brush settings")?;
        let channel = if ink == super::brush::Ink::Paint {
            0
        } else {
            1
        };
        let name = &settings.projectile_templates[channel];
        let template = report
            .brush_templates
            .get(name)
            .context("Missing projectile template")?;
        let components = template["components"]
            .as_array()
            .context("Missing projectile components")?;
        let body = components
            .iter()
            .find(|c| c["class"] == "PhysicsRigidBodyDynamicComponent")
            .context("Missing projectile body")?;
        let hull = body["properties"]
            .as_array()
            .context("Missing projectile body properties")?
            .iter()
            .find(|p| p["name"] == "RB Hull File Path")
            .and_then(|p| p["value"].as_str())
            .context("Missing projectile hull")?;
        let sphere = report
            .brush_shapes
            .get(hull)
            .context("Missing projectile sphere")?;
        let properties = body["properties"]
            .as_array()
            .context("Missing projectile body properties")?;
        let property = |name: &str| {
            properties
                .iter()
                .find(|p| p["name"] == name)
                .map(|p| &p["value"])
                .with_context(|| format!("Projectile body lacks {name}"))
        };
        let layer = u32::try_from(
            property("Collision Layer")?
                .as_u64()
                .context("Invalid projectile collision layer")?,
        )?;
        ensure!(layer < 32, "Projectile layer outside native matrix");
        let painted = property("Is Painted")?
            .as_bool()
            .context("Invalid projectile painted flag")?;
        let real = property("Is Real")?
            .as_bool()
            .context("Invalid projectile real flag")?;
        // Preserve imported group/subgroup fields, as the common native body
        // layer setter does. Full spawned-body/group lifecycle is still open.
        let filter = super::body_filter::change_layer(
            sphere.body_filter,
            layer,
            u8::from(painted),
            u8::from(real),
        );
        // Preview uses base upgrade, neutral playstyle and zero launch variance.
        // Do not describe these inputs as the player's recovered profile/RNG.
        let velocity = super::brush::aim_velocity(
            origin,
            target,
            settings.value(ink, "Speed", 0, super::ability_value::NEUTRAL_PLAYSTYLE)?,
        );
        ensure!(
            origin.iter().chain(velocity.iter()).all(|n| n.is_finite()),
            "Invalid projectile launch"
        );
        let next = self.pool.entries.len() as u64;
        let projectile = self.pool.launch(
            settings,
            ink,
            0,
            super::ability_value::NEUTRAL_PLAYSTYLE,
            owner,
            origin,
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            || Ok(next),
        )?;
        projectile.set_launch_velocity(velocity, false, 1.);
        let index = projectile.entity as usize;
        if index == self.radii.len() {
            self.radii.push(sphere.radius);
        } else {
            self.radii[index] = sphere.radius;
        }
        if index == self.filters.len() {
            self.filters.push(filter);
        } else {
            self.filters[index] = filter;
        }
        self.emitted += 1;
        Ok(())
    }
    pub fn advance_filtered(
        &mut self,
        collision: &Collision,
        dt: f32,
        mut admits: impl FnMut(u32, super::physics::SphereCandidate) -> Result<bool>,
    ) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "Invalid projectile timestep");
        self.step_contacts.clear();
        // A paused preview performs no query or lifecycle work.
        if dt == 0. {
            return Ok(());
        }
        for projectile in &mut self.pool.entries {
            if !projectile.active {
                continue;
            }
            let translation = projectile.body_velocity.map(|v| v * dt);
            let index = projectile.entity as usize;
            let filter = *self
                .filters
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("Projectile body filter is missing"))?;
            let hits = collision.with_player_queries(|| {
                collision.sphere_hits_filtered(
                    projectile.position,
                    translation,
                    self.radii[index],
                    |candidate| admits(filter, candidate),
                )
            })?;
            if let Some(hit) = hits
                .into_iter()
                .min_by(|a, b| a.fraction.total_cmp(&b.fraction))
            {
                let hit = super::physics::SweepHit {
                    shape: hit.shape,
                    source_triangle: hit.source_triangle,
                    fraction: hit.fraction,
                    position: hit.position,
                    normal: hit.normal,
                    owner: hit.owner,
                };
                projectile.position =
                    std::array::from_fn(|i| projectile.position[i] + translation[i] * hit.fraction);
                projectile.hit = true;
                projectile.active = false;
                self.contacts += 1;
                let contact = Contact {
                    ink: projectile.ink,
                    projectile: projectile.entity,
                    hit,
                };
                self.last_contact = Some(contact.clone());
                self.step_contacts.push(contact);
            } else {
                projectile.position = super::physics::add(projectile.position, translation);
            }
        }
        let expired = &mut self.expired;
        self.pool.update_lifetimes(dt, |_| *expired += 1);
        Ok(())
    }
    #[cfg(test)]
    fn advance(&mut self, collision: &Collision, dt: f32) -> Result<()> {
        self.advance_filtered(collision, dt, |_, _| Ok(true))
    }
    pub fn lines(&self, lines: &mut Vec<crate::render_support::viewport3d::GpuLineSegment>) {
        use crate::render_support::viewport3d::GpuLineSegment as Line;
        let color = |ink| {
            if ink == super::brush::Ink::Paint {
                [0.2, 0.6, 1., 1.]
            } else {
                [0.5, 1., 0.1, 1.]
            }
        };
        for p in self.pool.entries.iter().filter(|p| p.active) {
            lines.push(Line {
                start: p.position,
                end: super::physics::add(p.position, p.body_velocity.map(|v| v / 30.)),
                color: color(p.ink),
            });
        }
        if let Some(c) = &self.last_contact {
            lines.push(Line {
                start: c.hit.position,
                end: super::physics::add(c.hit.position, c.hit.normal.map(|v| v * 0.4)),
                color: color(c.ink),
            });
            for axis in 0..3 {
                let mut start = c.hit.position;
                let mut end = start;
                start[axis] -= 0.08;
                end[axis] += 0.08;
                lines.push(Line {
                    start,
                    end,
                    color: color(c.ink),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_player_collision_shell_does_not_consume_projectile() -> Result<()> {
        let face = |y| super::super::physics::Triangle {
            p: [[-10., y, -10.], [0., y, 10.], [10., y, -10.]],
        };
        let mut world =
            Collision::with_triangle_owners(vec![face(2.), face(0.)], vec![0.; 2], vec![14, 20])?;
        world.set_simulation_filter(0, 0x1800e)?;
        world.set_simulation_filter(1, 0x18014)?;
        let filter = super::super::physics_filter::GameFilter {
            matrix: super::super::physics_filter::Matrix::retail(),
            ..Default::default()
        };
        let mut projectiles = trace([0., -90., 0.]);
        projectiles.advance_filtered(&world, 1. / 30., |source, candidate| {
            Ok(
                filter.collision_info(source, world.simulation_filter_word(candidate.shape)?) >> 24
                    != 0,
            )
        })?;
        assert_eq!(projectiles.contacts, 1);
        assert_eq!(projectiles.last_contact.as_ref().unwrap().hit.owner, 20);
        assert!(!projectiles.pool.entries[0].active);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_paint_and_thinner_pass_spatter_player_shell_and_hit_character() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        // Keyframed attachment poses are committed by the shared physics
        // boundary, after the model/child update, as in normal gameplay.
        for tick in 1..=3 {
            world.advance_models(tick as f32 * super::super::STEP)?;
            world.advance_dynamic_physics(super::super::STEP)?;
        }
        let entity = world
            .entities
            .iter()
            .position(|e| e.name == "gremlinvillage_zonea.SpatterMelee 02")
            .context("Spatter absent")?;
        let body = world.character_physics[&entity]
            .as_ref()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let owner = body.owner;
        let position = world.collision.simulation_pose(body.shape)?.position;
        // Find an exposed original shell/character overlap, rather than
        // assuming its animated attachment is centered at an arbitrary height.
        let mut path = None;
        'search: for height in [0.15, 0.3, 0.45, 0.6, 0.9, 1.2] {
            for angle in 0..16 {
                let (s, c) = (angle as f32 * std::f32::consts::TAU / 16.).sin_cos();
                let origin = [
                    position[0] - 3. * c,
                    position[1] + height,
                    position[2] - 3. * s,
                ];
                let target = [
                    position[0] + 3. * c,
                    position[1] + height,
                    position[2] + 3. * s,
                ];
                let hits = world.collision.sphere_hits(
                    origin,
                    super::super::physics::sub(target, origin),
                    0.025,
                )?;
                let Some(character) = hits.iter().find(|h| h.owner == owner) else {
                    continue;
                };
                if let Some(shell) = hits.iter().find(|h| {
                    h.fraction < character.fraction
                        && world
                            .collision_entity(h.owner)
                            .is_some_and(|e| e.name == "spattermelee_playercollision")
                }) {
                    path = Some((
                        origin,
                        target,
                        world.collision.simulation_filter_word(shell.shape)?,
                    ));
                    break 'search;
                }
            }
        }
        let (origin,target,shell_word)=path.with_context(||format!("Original cast did not intersect foreground player-collision attachment; shells {:?}",
            world.dynamic_runtime_sources.iter().filter(|(source,_)|world.entities[world.dynamic_bodies[**source].entity].name=="spattermelee_playercollision")
                .map(|(source,owner)|(*owner,world.collision.owner_in_world(*owner),world.dynamic_constructions[source].bodies.iter().map(|b|
                    world.collision.simulation_pose(b.shapes[0])).collect::<Vec<_>>())).collect::<Vec<_>>()))?;
        assert_eq!(shell_word & 31, 14);
        let mut results = Vec::new();
        for ink in [
            super::super::brush::Ink::Paint,
            super::super::brush::Ink::Thinner,
        ] {
            world.projectile_preview = Projectiles::default();
            world.projectile_preview.spawn(
                &world.report,
                ink,
                world.player_entity as u64,
                origin,
                target,
            )?;
            let source = world.projectile_preview.filters[0];
            assert_eq!(
                world
                    .dynamic_runtime_filter
                    .collision_info(source, shell_word)
                    >> 24,
                0
            );
            for _ in 0..20 {
                world.advance_brush_projectiles(super::super::STEP)?;
                if world.projectile_preview.contacts > 0 {
                    break;
                }
            }
            let hit = world
                .projectile_preview
                .last_contact
                .as_ref()
                .context("Projectile missed character")?;
            assert_eq!(
                hit.hit.owner,
                owner,
                "Projectile hit {:?} instead of the character",
                world.collision_entity(hit.hit.owner).map(|e| &e.name)
            );
            results.push(serde_json::json!({"ink":format!("{ink:?}"),"source_filter":source,"hit_owner":owner,"hit_position":hit.hit.position}));
        }
        std::fs::write(
            "target/demo-projectile-filter-owned.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "entity":world.entities[entity].name,"origin":origin,"target":target,"shell_filter":shell_word,
            "results":results,"original_runtime_comparison":false,
            "scope":"Isolated original character and attachment contacts, not a full gameplay route or complete projectile lifecycle"}))?,
        )?;
        Ok(())
    }
    fn trace(velocity: V) -> Projectiles {
        let mut runtime = Projectiles::default();
        let shot = runtime
            .pool
            .acquire::<anyhow::Error>(
                super::super::brush::Ink::Paint,
                12,
                [0., 3., 0.],
                [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                || Ok(0),
            )
            .unwrap();
        shot.set_launch_velocity(velocity, false, 1.);
        shot.lifetime.remaining = 0.5;
        runtime.radii.push(0.025);
        runtime.filters.push(0x1800b);
        runtime
    }
    fn floor() -> Collision {
        Collision::with_triangle_owners(
            vec![super::super::physics::Triangle {
                p: [[-100., 0., -100.], [0., 0., 100.], [100., 0., -100.]],
            }],
            vec![0.],
            vec![37],
        )
        .unwrap()
    }
    #[test]
    fn aim_retains_erased_targets_and_respects_nearer_geometry() -> Result<()> {
        use super::super::physics::Triangle;
        let face = |y| Triangle {
            p: [[-10., y, -10.], [0., y, 10.], [10., y, -10.]],
        };
        let mut world =
            Collision::with_triangle_owners(vec![face(0.), face(-2.)], vec![0.; 2], vec![7, 8])?;
        let ray = cursor::Ray {
            origin: [0., 2., 0.],
            direction: [0., -1., 0.],
        };
        let painted = target(&world, ray, [0., 1., 0.], 10.)?;
        world.set_owner_solid(7, false)?;
        assert_eq!(target(&world, ray, [0., 1., 0.], 10.)?, painted);
        assert_eq!(world.ray(ray.origin, ray.direction, 10.), Some(4.));
        // A foreground solid face must still prevent aiming at the erased one.
        world.add_static_triangles(&[face(1.)], 0., 9)?;
        assert!((target(&world, ray, [0., 1., 0.], 10.)?[1] - 1.).abs() < 0.0001);
        Ok(())
    }

    #[test]
    fn fast_shot_records_first_contact_once_and_pause_is_inert() -> Result<()> {
        let world = floor();
        let mut runtime = trace([0., -120., 0.]);
        runtime.advance(&world, 0.)?;
        assert_eq!(runtime.pool.entries[0].position, [0., 3., 0.]);
        assert_eq!(runtime.pool.entries[0].lifetime.remaining, 0.5);
        runtime.advance(&world, 1. / 30.)?;
        assert_eq!(runtime.contacts, 1);
        assert_eq!(runtime.last_contact.as_ref().unwrap().hit.owner, 37);
        assert!(!runtime.pool.entries[0].active);
        assert!(runtime.pool.entries[0].position[1] > 0.);
        runtime.advance(&world, 1. / 30.)?;
        assert_eq!(runtime.contacts, 1);
        assert_eq!(runtime.expired, 0);
        Ok(())
    }
    #[test]
    fn missed_shot_moves_expires_and_invalid_time_does_not_mutate() -> Result<()> {
        let world = floor();
        let mut runtime = trace([0., 22.5, 0.]);
        assert!(runtime.advance(&world, f32::NAN).is_err());
        assert!(runtime.advance(&world, -1.).is_err());
        assert_eq!(runtime.pool.entries[0].position, [0., 3., 0.]);
        runtime.advance(&world, 0.25)?;
        assert_eq!(runtime.pool.entries[0].position, [0., 8.625, 0.]);
        runtime.advance(&world, 0.3)?;
        assert_eq!(runtime.expired, 1);
        assert_eq!(runtime.contacts, 0);
        assert!(!runtime.pool.entries[0].active);
        let position = runtime.pool.entries[0].position;
        runtime.advance(&world, 0.1)?;
        assert_eq!(runtime.expired, 1);
        assert_eq!(runtime.pool.entries[0].position, position);
        Ok(())
    }
    #[test]
    fn aim_uses_surface_or_authored_range_endpoint() {
        let world = Collision::new(vec![super::super::physics::Triangle {
            p: [[-100., 0., -100.], [0., 0., 100.], [100., 0., -100.]],
        }])
        .unwrap();
        let ray = cursor::Ray {
            origin: [0., 2., 0.],
            direction: [0., -1., 0.],
        };
        let hit = target(&world, ray, [0., 1., 0.], 10.).unwrap();
        assert!(hit[1].abs() < 0.0001);
        let ray = cursor::Ray {
            origin: [0., 2., 0.],
            direction: [0., 1., 0.],
        };
        let miss = target(&world, ray, [0., 1., 0.], 10.).unwrap();
        assert!((miss[1] - 11.).abs() < 0.0001);
        assert!(target(&world, ray, [0., 1., 0.], f32::NAN).is_err());
    }
}
