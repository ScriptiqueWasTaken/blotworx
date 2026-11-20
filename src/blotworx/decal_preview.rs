//! Live impact decal adapter using the retained NIF hierarchy and retail queue.
//! Full projectile stimulus callbacks, deforming receiver geometry and moving
//! decal attachments still require integration and original-reference checks.
use super::{
    assets::World,
    brush::Ink,
    decal_geometry::{Collected, Query, Transform},
    decal_queue,
};
use crate::render_support::viewport3d::GpuDrawCommand;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
type V = [f32; 3];
const IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];
#[derive(Clone)]
struct Request {
    template: String,
    material: String,
    position: V,
    normal: V,
    radius: f32,
    entity: usize,
    root: u32,
}
struct Mesh {
    draw: GpuDrawCommand,
    attached: bool,
    render: super::render_object::State,
}
pub struct View<'a> {
    pub world: &'a World,
    pub contacts: &'a [super::brush_preview::Contact],
}
pub struct Preview {
    queue: decal_queue::Queue<Request, Mesh>,
    pub projected: u64,
    pub rejected: u64,
    pub empty: u64,
}
impl Default for Preview {
    fn default() -> Self {
        Self {
            queue: decal_queue::Queue::new(),
            projected: 0,
            rejected: 0,
            empty: 0,
        }
    }
}
// Dynamic-body initialization and decals consume the same world-owned C rand
// stream. Complete boot ordering and other consumers remain unverified.
impl Preview {
    pub fn update(
        &mut self,
        world: View<'_>,
        dt: f32,
        random: &mut super::game_random::State,
    ) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "Invalid decal preview timestep");
        if dt == 0. {
            return Ok(());
        }
        let mut renderer = Renderer {
            world: &world,
            random,
            projected: &mut self.projected,
            empty: &mut self.empty,
        };
        self.queue.update(dt, false, &mut renderer)?;
        for contact in world.contacts {
            let settings = world
                .world
                .report
                .brush_settings
                .as_ref()
                .context("Missing brush settings")?;
            let name =
                &settings.projectile_templates[if contact.ink == Ink::Paint { 0 } else { 1 }];
            let Some(target) = world.world.collision_entity(contact.hit.owner) else {
                self.rejected += 1;
                continue;
            };
            let body = target.projectile_target_body();
            // Selected rigid-body components override the shape's real bit.
            // Otherwise retain the imported filter's bit; never invent it.
            let shape_real = if matches!(
                body,
                Some(
                    super::projectile_stimulus::TargetBody::Dynamic
                        | super::projectile_stimulus::TargetBody::Phantom
                        | super::projectile_stimulus::TargetBody::Static
                )
            ) {
                false // Not read by projectile_real_surface for these bodies.
            } else {
                world
                    .world
                    .collision
                    .simulation_filter(contact.hit.shape)
                    .context("Decal receiver lacks shape material real flag")?
                    & 0x10000
                    != 0
            };
            let stimuli = world
                .world
                .brush_stimuli
                .get(name)
                .context("Missing projectile stimuli")?;
            let emitter = super::projectile_stimulus::Emitter {
                stimuli: stimuli.clone(),
                amount_scale: settings.value(
                    contact.ink,
                    "StimulusScale",
                    0,
                    super::ability_value::NEUTRAL_PLAYSTYLE,
                )?,
                projectile: contact.projectile,
                owner: None,
                tag: String::new(),
            };
            if !emitter.requests_surface_effect(
                stimuli.types.len(),
                body,
                target.projectile_real_surface(shape_real)?,
            )? {
                self.rejected += 1;
                continue;
            }
            let component = world
                .world
                .brush_decals
                .get(name)
                .context("Missing projectile decal component")?;
            let request = component.request(-1, contact.hit.position, contact.hit.normal, || {
                random.draw()
            });
            let Some(request) = request else {
                self.rejected += 1;
                continue;
            };
            let entity = world
                .world
                .entities
                .iter()
                .position(|candidate| std::ptr::eq(candidate, target))
                .context("Decal receiver lost entity identity")?;
            // Physics-only receivers still accept gameplay stimuli, but have
            // no render surface on which a cosmetic decal can be projected.
            let Some(hierarchy) = world.world.entity_bound_hierarchy(entity)? else {
                self.rejected += 1;
                continue;
            };
            let roots: Vec<_> = hierarchy.roots.iter().flatten().copied().collect();
            ensure!(
                roots.len() == 1,
                "Decal receiver requires a single model root"
            );
            ensure!(
                request.radius.is_finite() && request.radius > 0.,
                "Invalid decal preview radius"
            );
            let payload = Request {
                template: name.clone(),
                material: request.material.into(),
                position: request.position,
                normal: request.normal,
                radius: request.radius,
                entity,
                root: roots[0],
            };
            if !self.queue.enqueue(Some(payload), request.lifetime) {
                self.rejected += 1;
            }
        }
        Ok(())
    }
    pub fn append(&self, draws: &mut Vec<GpuDrawCommand>) -> Result<()> {
        for mesh in self
            .queue
            .entries
            .iter()
            .filter_map(|entry| entry.mesh.as_ref())
            .filter(|mesh| mesh.attached)
        {
            let mut draw = mesh.draw.clone();
            mesh.render.apply(&mut draw)?;
            draws.push(draw);
        }
        Ok(())
    }
    pub fn visible(&self) -> usize {
        self.queue
            .entries
            .iter()
            .filter(|e| e.mesh.as_ref().is_some_and(|m| m.attached))
            .count()
    }
}
struct Renderer<'a, 'b> {
    world: &'a View<'b>,
    random: &'a mut super::game_random::State,
    projected: &'a mut u64,
    empty: &'a mut u64,
}
impl decal_queue::Renderer<Request, Mesh> for Renderer<'_, '_> {
    fn project(
        &mut self,
        entry: &decal_queue::Entry<Request, Mesh>,
        children: &mut decal_queue::Children<Request, Mesh>,
    ) -> Result<Option<Mesh>> {
        let request = &entry.payload;
        let world = self.world.world;
        let entity = &world.entities[request.entity];
        let hierarchy = world
            .entity_bound_hierarchy(request.entity)?
            .context("Decal model hierarchy disappeared")?;
        let (transform, _) = mesh_space(entity.transform, request.position)?;
        let parent = epic_mickey_lib_rs::nif_file::NiTransform {
            rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                rows: transform.rotation,
            },
            scale: transform.scale,
            translation: epic_mickey_lib_rs::nif_file::Vector3 {
                x: transform.translation[0],
                y: transform.translation[1],
                z: transform.translation[2],
            },
        };
        let bounds = world.entity_world_bounds(request.entity, &parent, &BTreeMap::new())?;
        let scene = ReceiverScene {
            hierarchy,
            bounds: &bounds,
        };
        let mut meshes = Vec::new();
        super::decal_targets::collect(
            &scene,
            request.root,
            request.position,
            request.radius,
            request.radius * request.radius,
            &mut meshes,
            &mut |root| {
                let mut child = request.clone();
                child.root = root;
                children.enqueue(child)
            },
        )?;
        let mut collected = Collected::new(request.normal);
        'meshes: for mesh in meshes {
            for &index in entity
                .mesh_draws
                .get(&mesh)
                .context("Decal target mesh has no draw binding")?
            {
                let draw = &world.draws[index];
                ensure!(
                    draw.v_data.len() % 12 == 0,
                    "Invalid decal target vertex layout"
                );
                let (transform, local) = mesh_space(draw.trans_mat, request.position)?;
                let query =
                    Query::for_mesh(local, request.radius, 1. / transform.scale, 0., transform);
                if !collected.interleaved_region(
                    &draw.v_data,
                    12,
                    &draw.indices,
                    &query,
                    &mut BTreeMap::new(),
                )? {
                    break 'meshes;
                }
            }
        }
        // Output target is world space. Host sin/cos is explicit preview math,
        // not the missing retail trigonometric provider.
        log::debug!(target:"blotworx::decal","projection center={:?} vertices={} normal={:?} nearest={:?}",request.position,collected.positions.len(),collected.weighted_normal,collected.nearest_normal);
        let geometry = collected.project(
            request.position,
            request.radius,
            1.,
            None,
            || self.random.draw() as i32,
            |a| Ok(a.sin_cos()),
        )?;
        let Some(geometry) = geometry else {
            *self.empty += 1;
            return Ok(None);
        };
        let component = &world.brush_decals[&request.template];
        let material = component
            .templates
            .get(&request.material)
            .context("Missing decal material")?;
        log::debug!(target:"blotworx::decal","projected positions/uv {:?}",geometry.positions.iter().zip(&geometry.uv).take(6).collect::<Vec<_>>());
        let draw = material.draw(geometry, IDENTITY)?;
        log::debug!(target:"blotworx::decal","bounds {:?} {:?} samplers {:?}",draw.aabb_min,draw.aabb_max,
            draw.material_images.iter().map(|i|(i.sampler,i.matrix)).collect::<Vec<_>>());
        Ok(Some(Mesh {
            draw,
            attached: false,
            render: super::render_object::State::generated_decal(),
        }))
    }
    fn created(&mut self, entry: &mut decal_queue::Entry<Request, Mesh>) -> Result<()> {
        let mesh = entry.mesh.as_mut().context("Decal callback without mesh")?;
        ensure!(!mesh.attached, "Decal attached twice");
        mesh.attached = true;
        *self.projected += 1;
        Ok(())
    }
    fn animate(&mut self, mesh: &mut Mesh, _: f32) -> Result<()> {
        // Template::draw rejects controller/interpolator blocks. These static
        // materials have no animation, but must remain attached until expiry.
        ensure!(mesh.attached, "Detached decal remained in update queue");
        Ok(())
    }
    fn expire(&mut self, mesh: &mut Mesh) -> Result<()> {
        ensure!(mesh.attached, "Detached decal expired twice");
        mesh.attached = false;
        Ok(())
    }
}
struct ReceiverScene<'a> {
    hierarchy: &'a super::scene_bounds::Hierarchy,
    bounds: &'a BTreeMap<u32, super::scene_bounds::Sphere>,
}
impl super::decal_targets::Scene for ReceiverScene<'_> {
    type Id = u32;
    fn object(&self, id: u32) -> Result<super::decal_targets::Object<u32>> {
        let node = self
            .hierarchy
            .nodes
            .get(&id)
            .context("Decal traversal node missing")?;
        let bound = self
            .bounds
            .get(&id)
            .context("Decal traversal world bound missing")?;
        Ok(super::decal_targets::Object {
            kind: match node.class_name.as_str() {
                "NiMesh" => super::decal_targets::Kind::Mesh,
                "NiNode" => super::decal_targets::Kind::Node,
                _ => super::decal_targets::Kind::Other,
            },
            decal_type: node.decal_type,
            flags: node.flags,
            center: bound.center,
            radius: bound.radius,
            // EM1 RTTI constructors803e19b8 and804215f0 name these classes.
            separate_controller: node.controllers.iter().any(|c| {
                matches!(
                    c.class_name.as_str(),
                    "NiControllerManager" | "NiTransformController"
                )
            }),
            children: node.children.clone(),
        })
    }
}
/// Decompose the preview's flattened affine matrix. Reject shear, nonuniform
/// scale and reflection rather than pretending these are a NiTransform.
fn mesh_space(matrix: [f32; 16], world: V) -> Result<(Transform, V)> {
    ensure!(
        matrix.iter().chain(world.iter()).all(|v| v.is_finite()),
        "Invalid decal target transform"
    );
    ensure!(
        matrix[3] == 0. && matrix[7] == 0. && matrix[11] == 0. && matrix[15] == 1.,
        "Non-affine decal target"
    );
    let scale = (matrix[0] * matrix[0] + matrix[1] * matrix[1] + matrix[2] * matrix[2]).sqrt();
    ensure!(scale > 0., "Singular decal target");
    let rotation: [[f32; 3]; 3] =
        std::array::from_fn(|row| std::array::from_fn(|col| matrix[col * 4 + row] / scale));
    for a in 0..3 {
        for b in a..3 {
            let dot: f32 = (0..3).map(|r| rotation[r][a] * rotation[r][b]).sum();
            ensure!(
                (dot - if a == b { 1. } else { 0. }).abs() < 0.001,
                "Nonuniform/sheared decal target unsupported by preview"
            );
        }
    }
    let r = rotation;
    let determinant = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    ensure!(
        determinant > 0.,
        "Reflected decal target unsupported by preview"
    );
    let translation = [matrix[12], matrix[13], matrix[14]];
    let delta: V = std::array::from_fn(|i| world[i] - translation[i]);
    let local = std::array::from_fn(|i| {
        r[2][i].mul_add(delta[2], r[0][i].mul_add(delta[0], r[1][i] * delta[1])) / scale
    });
    Ok((
        Transform {
            rotation,
            scale,
            translation,
        },
        local,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires imported original game assets"]
    fn physics_only_receiver_declines_decal_without_aborting_gameplay() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let (owner, entity, shape) = (0..world.collision.next_owner()?)
            .find_map(|owner| {
                let target = world.collision_entity(owner)?;
                if !target.projectile_real_surface(true).ok()? {
                    return None;
                }
                let entity = world
                    .entities
                    .iter()
                    .position(|e| std::ptr::eq(e, target))?;
                Some((owner, entity, world.collision.owner_shapes(owner).next()?))
            })
            .context("No real receiver for owned regression fixture")?;
        // Model-less proxy fixture; the immutable original assets are intact.
        world.entities[entity].model_asset = None;
        let contact = super::super::brush_preview::Contact {
            ink: Ink::Paint,
            projectile: 0,
            hit: super::super::physics::SweepHit {
                shape,
                source_triangle: None,
                fraction: 0.,
                position: [0.; 3],
                normal: [0., 1., 0.],
                owner,
            },
        };
        let mut preview = Preview::default();
        let mut random = world.game_random;
        preview.update(
            View {
                world: &world,
                contacts: &[contact],
            },
            super::super::STEP,
            &mut random,
        )?;
        assert_eq!(preview.rejected, 1);
        assert!(preview.queue.entries.is_empty());
        assert_ne!(
            random, world.game_random,
            "Fixture never reached decal creation"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires imported original game assets"]
    fn original_toon_contacts_do_not_enqueue_real_surface_decals() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut tested = 0;
        for (&owner, &entity) in &world.model_paint_owners {
            let target = &world.entities[entity];
            if target.projectile_real_surface(false)? {
                continue;
            }
            let Some(shape) = world.collision.owner_shapes(owner).next() else {
                continue;
            };
            let contact = super::super::brush_preview::Contact {
                ink: Ink::Paint,
                projectile: 0,
                hit: super::super::physics::SweepHit {
                    shape,
                    source_triangle: None,
                    fraction: 0.,
                    position: target.transform[12..15].try_into()?,
                    normal: [0., 1., 0.],
                    owner,
                },
            };
            let mut preview = Preview::default();
            let mut random = world.game_random;
            preview.update(
                View {
                    world: &world,
                    contacts: &[],
                },
                1. / 30.,
                &mut random,
            )?;
            preview.update(
                View {
                    world: &world,
                    contacts: &[contact],
                },
                1. / 30.,
                &mut random,
            )?;
            ensure!(
                preview.queue.entries.is_empty()
                    && preview.rejected == 1
                    && random == world.game_random,
                "Toon receiver {} created a real-surface decal or consumed decal RNG",
                target.name
            );
            tested += 1;
        }
        ensure!(
            tested > 0,
            "Regression did not exercise an imported toon receiver"
        );
        println!("Validated live decal gate on {tested} original toon collision owners (synthetic contacts)");
        Ok(())
    }
    #[test]
    #[ignore = "requires imported original game assets"]
    fn inspect_original_splat_texture() -> Result<()> {
        let root = std::env::var("BLOTWORX_TEST_DATA")
            .context("Set BLOTWORX_TEST_DATA to imported files")?;
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            std::path::Path::new(&root).join("packfiles/_dynamic.pak"),
        )?);
        let file = pak
            .files
            .iter()
            .find(|f| f.path.to_ascii_lowercase().contains("gen_inksplat_tex"))
            .context("Missing original splat texture")?;
        let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
            .map_err(anyhow::Error::msg)?;
        std::fs::write(
            "target/decal-texture-layout.json",
            serde_json::to_vec_pretty(&nif.blocks)?,
        )?;
        let (rgba, width, height) = crate::render_support::nif::decode_image_bytes(&file.data)
            .context("Splat texture decode failed")?;
        ensure!(
            rgba.len() == width as usize * height as usize * 4,
            "Invalid splat texture storage"
        );
        image::save_buffer(
            "target/decal-texture-decoded.png",
            &rgba,
            width,
            height,
            image::ColorType::Rgba8,
        )?;
        println!(
            "Splat {}: {}x{} alpha {:?}..{:?}",
            file.path,
            width,
            height,
            rgba.chunks_exact(4).map(|p| p[3]).min(),
            rgba.chunks_exact(4).map(|p| p[3]).max()
        );
        Ok(())
    }
    #[test]
    fn preview_transform_retains_rotation_translation_and_scale() -> Result<()> {
        let matrix = [
            0., 0., -2., 0., 0., 2., 0., 0., 2., 0., 0., 0., 10., 20., 30., 1.,
        ];
        let (t, p) = mesh_space(matrix, [18., 26., 26.])?;
        assert_eq!(t.scale, 2.);
        assert_eq!(p, [2., 3., 4.]);
        let mut shear = matrix;
        shear[4] = 1.;
        assert!(mesh_space(shear, [0.; 3]).is_err());
        let mut reflection = IDENTITY;
        reflection[0] = -1.;
        assert!(mesh_space(reflection, [0.; 3]).is_err());
        Ok(())
    }
    #[test]
    fn recovered_limits_preserve_single_inverse_scale_factor() {
        let q = Query::for_mesh(
            [0.; 3],
            2.,
            0.5,
            0.,
            Transform {
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                scale: 2.,
                translation: [0.; 3],
            },
        );
        assert_eq!(q.first_vertex_limit_squared, 18.0625);
        assert_eq!(q.triangle_limit_squared, 4.);
    }
}
