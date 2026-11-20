//! Live static-contact adapter. Box3D selects candidate triangles; the recovered
//! GSK query supplies source features, radii and authored Havok edge welding.
//! Candidate order, manifold persistence and the mover solver remain backend work.
use super::{native_gsk, native_hk_transform as transform, shape_welding};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, sync::Arc};

pub struct Source {
    pub owner: usize,
    pub resource: Arc<Vec<super::collision::StaticMesh>>,
    pub part: usize,
    pub welding: Arc<BTreeMap<usize, shape_welding::Prepared>>,
    pub body: usize,
    pub triangle_start: usize,
    pub initial: transform::Transform,
}
pub struct Resources {
    support: native_gsk::ConvexSupport,
    radius: f32,
    sources: BTreeMap<usize, Source>,
    caches: BTreeMap<(usize, usize), native_gsk::Cache>,
    pub counts: [u64; 3], // accepted, rejected, native status4
}
impl Resources {
    pub fn append(&mut self, sources: Vec<Source>) -> Result<()> {
        let mut incoming = BTreeMap::new();
        for source in sources {
            ensure!(
                !self.sources.contains_key(&source.owner),
                "Native contact owner already registered"
            );
            ensure!(
                incoming.insert(source.owner, source).is_none(),
                "Duplicate incoming native contact owner"
            );
        }
        self.sources.extend(incoming);
        Ok(())
    }
    pub fn new(
        support: &native_gsk::ConvexSupport,
        radius: f32,
        sources: Vec<Source>,
    ) -> Result<Self> {
        ensure!(
            radius.is_finite() && radius >= 0.,
            "Invalid native character radius"
        );
        let mut map = BTreeMap::new();
        for source in sources {
            ensure!(
                map.insert(source.owner, source).is_none(),
                "Duplicate native contact owner"
            );
        }
        Ok(Self {
            support: native_gsk::ConvexSupport {
                packed_vertices: support.packed_vertices.clone(),
                vertex_count: support.vertex_count,
                stored_center: support.stored_center,
            },
            radius,
            sources: map,
            caches: BTreeMap::new(),
            counts: [0; 3],
        })
    }
    fn query(
        &mut self,
        owner: usize,
        triangle: usize,
        position: [f32; 3],
        delta: [f32; 7],
        tolerance: f32,
        table: &shape_welding::ContactTable,
    ) -> Result<Option<shape_welding::ClosestQuery>> {
        let Some(source) = self.sources.get(&owner) else {
            return Ok(None);
        };
        ensure!(
            position
                .iter()
                .chain(&delta)
                .chain([&tolerance])
                .all(|v| v.is_finite())
                && tolerance >= 0.,
            "Invalid live character query frame"
        );
        let part = source
            .resource
            .get(source.part)
            .context("Native contact part disappeared")?;
        let local = triangle
            .checked_sub(source.triangle_start)
            .context("Native contact precedes source part")?;
        let cache_key = (owner, triangle);
        let mut triangle = match source.welding.get(&source.body) {
            Some(welding) => part.prepared_triangle_contact(local, welding)?,
            None => part.authored_triangle_contact(local)?,
        };
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let opposing = transform::from_entity(identity, position);
        let mut movement = transform::from_entity(identity, delta[..3].try_into()?);
        transform::set_rotation(&mut movement, delta[3..].try_into()?);
        let frame = transform::compose(&movement, &source.initial);
        let relative = transform::relative(&opposing, &frame);
        let cache = match self.caches.entry(cache_key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut cache = native_gsk::Cache([0; 12]);
                cache.initialize_triangle(
                    &mut self.support,
                    triangle.support.vertices,
                    &relative,
                )?;
                entry.insert(cache)
            }
        };
        Ok(Some(shape_welding::query_cached_triangle_contact(
            cache,
            &mut self.support,
            &mut triangle.support,
            &opposing,
            &frame,
            self.radius,
            triangle.radius,
            tolerance,
            true,
            &mut [[0.; 4]; 4],
            None,
            true,
            triangle.mode,
            triangle.word,
            table,
        )?))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game assets"]
    fn original_player_stands_and_jumps_through_live_native_static_contacts() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut controller = super::super::controller::Controller::new(
            super::super::physics::Player::new(world.report.spawn, world.report.motion.clone()),
        );
        for _ in 0..120 {
            controller.advance_with(
                &world.collision,
                1. / 30.,
                true,
                Default::default(),
                |_, _| Ok::<_, anyhow::Error>(()),
            )?;
        }
        ensure!(
            controller.player.grounded && controller.player.respawns == 0,
            "Native live contact path did not retain original spawn support"
        );
        let counts = world
            .collision
            .native_character_contact_counts()
            .context("Live native contact adapter absent")?;
        ensure!(
            counts[0] > 0,
            "Controller never consumed an original static contact"
        );
        controller.advance_with(
            &world.collision,
            1. / 30.,
            true,
            super::super::controller::Input {
                jump_pressed: true,
                jump_held: true,
                ..Default::default()
            },
            |_, _| Ok::<_, anyhow::Error>(()),
        )?;
        ensure!(
            controller.player.ground_jump_started && controller.player.velocity[1] > 0.,
            "Original static contact did not permit jump departure"
        );
        let mut maximum_y = controller.player.position[1];
        for tick in 0..180 {
            controller.advance_with(
                &world.collision,
                1. / 30.,
                true,
                super::super::controller::Input {
                    movement: [0., 1.],
                    jump_pressed: tick % 30 == 0,
                    jump_held: tick % 30 < 6,
                    ..Default::default()
                },
                |_, _| Ok::<_, anyhow::Error>(()),
            )?;
            maximum_y = maximum_y.max(controller.player.position[1]);
        }
        ensure!(
            controller.player.respawns == 0
                && controller.player.position.iter().all(|v| v.is_finite()),
            "Live native contact traversal failed"
        );
        println!("Original-scene wall/jump input scenario: final {:?}, peak Y {maximum_y}; original trajectory comparison pending",controller.player.position);
        println!("Live original static contacts: accepted/rejected/unresolved {counts:?}");
        Ok(())
    }
}
pub struct ContextState<'a> {
    pub resources: &'a mut Resources,
    pub owners: &'a [usize],
    pub table: &'a shape_welding::ContactTable,
    pub error: Option<String>,
}
/// Returns2 for a non-static source still using the explicit Box3D path;0
/// suppresses a native rejected/unresolved contact;1 publishes a native contact.
pub unsafe extern "C" fn resolve(
    mesh: i32,
    triangle: i32,
    position: *const f32,
    delta: *const f32,
    tolerance: f32,
    output: *mut f32,
    context: *mut std::ffi::c_void,
) -> i32 {
    let state = unsafe { &mut *(context as *mut ContextState<'_>) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i32> {
        let owner = *state
            .owners
            .get(usize::try_from(mesh)?)
            .context("Native contact shape owner missing")?;
        if triangle < 0 {
            return Ok(2);
        };
        let position: [f32; 3] = unsafe { std::slice::from_raw_parts(position, 3) }.try_into()?;
        let delta: [f32; 7] = unsafe { std::slice::from_raw_parts(delta, 7) }.try_into()?;
        match state.resources.query(
            owner,
            triangle as usize,
            position,
            delta,
            tolerance,
            state.table,
        )? {
            None => Ok(2),
            Some(shape_welding::ClosestQuery::NoContact) => {
                state.resources.counts[1] += 1;
                Ok(0)
            }
            Some(shape_welding::ClosestQuery::Unresolved) => {
                state.resources.counts[2] += 1;
                Ok(0)
            }
            Some(shape_welding::ClosestQuery::Contact(contact)) => {
                let out = unsafe { std::slice::from_raw_parts_mut(output, 7) };
                out[..3].copy_from_slice(&contact.position[..3]);
                out[3..].copy_from_slice(&contact.separating);
                state.resources.counts[0] += 1;
                Ok(1)
            }
        }
    }));
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            state.error.get_or_insert_with(|| format!("{error:#}"));
            -1
        }
        Err(_) => {
            state
                .error
                .get_or_insert_with(|| "Native character contact query panicked".into());
            -1
        }
    }
}
