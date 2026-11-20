//! Native HUD effect ownership: REL20f65c/20f94c/20fcd8/210634.
//! The fourth SPAWN_EFFECT argument selects a scene-graph layer, not scale.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, serde::Serialize)]
pub enum Command {
    Spawn {
        id: u32,
        x: f32,
        y: f32,
        layer: i32,
        name: String,
    },
    Update {
        id: u32,
        x: f32,
        y: f32,
    },
    Despawn {
        id: u32,
    },
    Emitters {
        id: u32,
        enabled: bool,
    },
}
pub fn recognizes(name: &str) -> bool {
    matches!(
        name,
        "SPAWN_EFFECT" | "UPDATE_EFFECT" | "DESPAWN_EFFECT" | "STOP_EMITTERS" | "START_EMITTERS"
    )
}
/// Admission for complete, finite, whitespace-separated authored commands.
/// sscanf's malformed/partial numeric-token behavior is not approximated.
pub fn parse(name: &str, args: &str) -> Result<Command> {
    ensure!(
        args.is_ascii(),
        "HUD command locale/Unicode sscanf behavior is not admitted"
    );
    let words = args
        .split('\0')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    let word = |i: usize| {
        words
            .get(i)
            .copied()
            .context("Truncated HUD effect command")
    };
    let id = word(0)?
        .parse::<i32>()
        .context("HUD effect ID is not a complete signed integer")? as u32;
    let point = |i| -> Result<f32> {
        let n = word(i)?.parse::<f32>()?;
        ensure!(
            n.is_finite(),
            "Nonfinite HUD command coordinate requires native sscanf validation"
        );
        Ok(n)
    };
    Ok(match name {
        "SPAWN_EFFECT" => Command::Spawn {
            id,
            x: point(1)?,
            y: point(2)?,
            layer: word(3)?.parse()?,
            name: word(4)?.into(),
        },
        "UPDATE_EFFECT" => Command::Update {
            id,
            x: point(1)?,
            y: point(2)?,
        },
        "DESPAWN_EFFECT" => Command::Despawn { id },
        "STOP_EMITTERS" => Command::Emitters { id, enabled: false },
        "START_EMITTERS" => Command::Emitters { id, enabled: true },
        _ => anyhow::bail!("Unknown HUD effect command: {name}"),
    })
}
pub use super::native_float_conversion::unsigned as pixel;
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Projection {
    pub frustum: [f32; 6],
    pub viewport: [f32; 4],
    pub y: u32,
    pub orthographic: bool,
}
///20f9f0..20fae4 copies the main camera's intervals/viewport, forces an
/// ORTHOGRAPHIC HUD camera, and applies format1's special frustum/Y correction.
/// Native platform format and renderer-wide status are explicit caller inputs.
pub fn projection(
    mut frustum: [f32; 6],
    viewport: [f32; 4],
    mut y: u32,
    tv_format: u32,
    wide: bool,
) -> Projection {
    if tv_format == 1 {
        frustum[2] = f32::from_bits(0x3eeb851f);
        frustum[3] = f32::from_bits(0xbeeb851f);
        let side = if wide {
            f32::from_bits(0x3f3da134)
        } else {
            f32::from_bits(0x3f051eb8)
        };
        frustum[0] = -side;
        frustum[1] = side;
        let extra = f32::from_bits(0x3df3b646) * (y as f32);
        y = y.wrapping_add(extra.trunc() as u32);
    }
    Projection {
        frustum,
        viewport,
        y,
        orthographic: true,
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Instance<H> {
    pub handle: H,
    pub x: u32,
    pub y: u32,
}
pub trait Backend {
    type Handle: Copy;
    /// Clone/register/reset the actual resource, then attach to layer0..3.
    /// Unregistered names return None, as native1e7f00 does. Other layer values
    /// still create a resource but do not attach it to any of the four roots.
    fn clone_effect(&mut self, name: &str, layer: i32) -> Result<Option<Self::Handle>>;
    fn emitters(&mut self, handle: Self::Handle, enabled: bool) -> Result<()>;
    /// Native20fe68 detaches from all four roots and releases the resource
    /// before210634 removes/frees its UI identifier entry.
    fn release(&mut self, handle: Self::Handle) -> Result<()>;
}
pub struct State<H> {
    instances: BTreeMap<u32, Instance<H>>,
    buckets: [std::collections::VecDeque<u32>; 37],
}
impl<H> Default for State<H> {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
            buckets: std::array::from_fn(|_| Default::default()),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Spawned,
    Duplicate,
    Unregistered,
    Updated,
    Missing,
    Released,
    Emitters,
}
impl<H: Copy> State<H> {
    fn insert(&mut self, id: u32, instance: Instance<H>) {
        self.instances.insert(id, instance);
        self.buckets[(id % 37) as usize].push_front(id);
    }
    pub fn native_order(&self) -> impl Iterator<Item = (u32, &Instance<H>)> {
        self.buckets
            .iter()
            .flatten()
            .map(|id| (*id, &self.instances[id]))
    }
    pub fn command(
        &mut self,
        command: &Command,
        backend: &mut impl Backend<Handle = H>,
    ) -> Result<Outcome> {
        Ok(match command {
            Command::Spawn {
                id,
                x,
                y,
                layer,
                name,
            } => {
                if self.instances.contains_key(id) {
                    Outcome::Duplicate
                } else if let Some(handle) = backend.clone_effect(name, *layer)? {
                    self.insert(
                        *id,
                        Instance {
                            handle,
                            x: pixel(*x),
                            y: pixel(*y),
                        },
                    );
                    Outcome::Spawned
                } else {
                    Outcome::Unregistered
                }
            }
            Command::Update { id, x, y } => {
                if let Some(instance) = self.instances.get_mut(id) {
                    instance.x = pixel(*x);
                    instance.y = pixel(*y);
                    Outcome::Updated
                } else {
                    Outcome::Missing
                }
            }
            Command::Despawn { id } => {
                if let Some(instance) = self.instances.get(id) {
                    backend.release(instance.handle)?;
                    self.instances.remove(id);
                    self.buckets[(id % 37) as usize].retain(|v| v != id);
                    Outcome::Released
                } else {
                    Outcome::Missing
                }
            }
            Command::Emitters { id, enabled } => {
                if let Some(instance) = self.instances.get(id) {
                    backend.emitters(instance.handle, *enabled)?;
                    Outcome::Emitters
                } else {
                    Outcome::Missing
                }
            }
        })
    }
}

#[derive(Default)]
pub struct Catalog {
    pub paths: BTreeMap<String, String>,
    pub disabled: Vec<String>,
}
impl Catalog {
    ///1e7834 installs c0734/c06d4 on the resource map, just as FXComponent
    ///does. Preserve registered spelling but compare native C-string identity.
    pub fn resolve(&self, name: &str) -> Result<Option<(&str, &str)>> {
        let key = super::named_effects::lookup_key(name)?;
        if let Some((registered, path)) = self
            .paths
            .iter()
            .find(|(registered, _)| registered.eq_ignore_ascii_case(&key))
        {
            return Ok(Some((registered.as_str(), path.as_str())));
        }
        ensure!(self.disabled.iter().any(|registered|registered.eq_ignore_ascii_case(&key)),
            "The global FX namespace for {name} has not been admitted; it cannot be treated as an unregistered HUD slot");
        Ok(None)
    }
    /// Native20eb74..20ec78: default count0, default path empty; no compacting
    /// skipped indices and no assumption that every generated label is bound.
    pub fn hud(settings: &BTreeMap<String, String>) -> Result<Self> {
        let count = settings
            .get("HUDEffectCount")
            .map(|v| v.parse::<i32>())
            .transpose()?
            .unwrap_or(0);
        let mut catalog = Self::default();
        for i in 0..count {
            let name = format!("HUDEffect{i:03}");
            match settings.get(&name).filter(|s| !s.is_empty()) {
                Some(path) => {
                    catalog.paths.insert(name, path.clone());
                }
                None => catalog.disabled.push(name),
            }
        }
        Ok(catalog)
    }
}

/// Decode each actually requested native model once. This is preparation,
/// not a successful replacement for cloning, timing, attachment or rendering.
pub struct Resources {
    pub catalog: Catalog,
    store: super::resource_store::Store,
    prepared: BTreeMap<String, Arc<Prepared>>,
}
pub struct Prepared {
    pub summary: serde_json::Value,
    // Retain actual model/prototype data for the eventual native overlay owner,
    // rather than discarding it after producing a diagnostic inventory.
    pub model: epic_mickey_lib_rs::nif_file::NifFile,
    pub hierarchy: Arc<super::scene_bounds::Hierarchy>,
    pub dependencies: Vec<epic_mickey_lib_rs::packfile::VirtualFile>,
    pub particles: Result<Option<super::brush_particles::Hose>, String>,
    clone_hierarchy: Option<Arc<super::scene_bounds::Hierarchy>>,
    emitter_plan: Result<Option<Arc<super::emitter_control::Plan>>, String>,
    lifecycle: Result<Option<Arc<super::controller_lifecycle::Plan>>, String>,
}
impl Resources {
    pub fn load(root: &std::path::Path) -> Result<Self> {
        let mut settings = BTreeMap::new();
        for (name, command) in [("ConfigFiles.ini", false), ("cmdline.txt", true)] {
            match std::fs::read_to_string(root.join(name)) {
                Ok(text) => settings.extend(crate::render_support::game_config::runtime_settings(
                    &text, command,
                )),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("Read HUD settings {name}")),
            }
        }
        let catalog = Catalog::hud(&settings)?;
        // The admitted always-loaded namespace precedes template resources.
        let text = std::fs::read_to_string(root.join("always_loaded.txt"))?;
        let mut packs = text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with(';'))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !packs.iter().any(|p| p.eq_ignore_ascii_case("r3mt.pak")) {
            packs.push("r3mt.pak".into());
        }
        Ok(Self {
            catalog,
            store: super::resource_store::Store::new(root, &packs)?,
            prepared: BTreeMap::new(),
        })
    }
    pub fn prepare(&mut self, name: &str) -> Result<Option<&Prepared>> {
        let Some((name, path)) = self.catalog.resolve(name)? else {
            return Ok(None);
        };
        if !self.prepared.contains_key(name) {
            let file = self.store.read(path)?;
            let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
                .map_err(anyhow::Error::msg)?;
            let graph = Arc::new(super::scene_bounds::Hierarchy::load(&nif)?);
            //1e7fb8 compares807192e0 (NiAVObject), not NiNode80765ae0.
            // A mesh can be the stream's first object too.
            let root = clone_root(&nif)?;
            let mut clone_hierarchy = root
                .map(|root| {
                    if graph.roots.len() == 1 && graph.roots[0] == Some(root) {
                        Ok(graph.clone())
                    } else {
                        graph.subtree(root).map(Arc::new)
                    }
                })
                .transpose()?;
            let lifecycle = clone_hierarchy
                .as_ref()
                .map(|graph| {
                    super::controller_lifecycle::Plan::load(&nif, graph, root.expect("Clone root"))
                        .map(Arc::new)
                })
                .transpose()
                .map_err(|error| format!("{error:#}"));
            let mut template_controllers = lifecycle
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .map(|plan| plan.controller_ids())
                .unwrap_or_default();
            if let Some(runtime_graph) = &mut clone_hierarchy {
                for node in Arc::make_mut(runtime_graph).nodes.values_mut() {
                    for c in &mut node.controllers {
                        template_controllers.insert(c.block);
                        c.flags = super::nif_runtime::effect_template_flags(c.flags);
                    }
                }
            }
            let emitter_plan = clone_hierarchy
                .as_ref()
                .map(|graph| {
                    super::emitter_control::Plan::load(graph, root.expect("Clone root"))
                        .map(Arc::new)
                })
                .transpose()
                .map_err(|error| format!("{error:#}"));
            let animation = super::nif_runtime::Instance::load(&graph)?;
            let dependencies = self.store.particle_files(&file.data)?;
            let particles = if nif.block_types.iter().any(|n| n.contains("ParticleSystem")) {
                super::brush_particles::Hose::load_attached(&file.data, &dependencies)
                    .map(|mut particles| {
                        particles.prepare_effect_template(&template_controllers);
                        Some(particles)
                    })
                    .map_err(|error| format!("{error:#}"))
            } else {
                Ok(None)
            };
            let particle_summary = match &particles {
                Ok(Some(p)) => {
                    serde_json::json!({"prepared":true,"warnings":p.warnings,"runtime_parity_verified":false})
                }
                Ok(None) => serde_json::json!({"particle_systems":false}),
                Err(error) => serde_json::json!({"prepared":false,"error":error}),
            };
            let materials=nif.blocks.iter().enumerate().filter(|(_,b)|matches!(b,epic_mickey_lib_rs::nif_file::NiObject::NiPSParticleSystem(_)))
                .map(|(index,_)|{let surface=crate::render_support::nif::find_mesh_surface(&nif,index);let m=surface.material;
                    serde_json::json!({"node":index,"shader":m.shader,"legacy_diffuse":m.legacy_diffuse,"legacy_colors":m.legacy_colors,
                        "legacy_alpha_flags":m.legacy_alpha_flags,"legacy_alpha_threshold":m.legacy_alpha_threshold,
                        "legacy_depth_state":m.legacy_depth_flags,"legacy_texture_state":m.legacy_texture_flags,"legacy_stencil_state":m.legacy_stencil_flags,
                        "vertex_color_state":format!("{:?}",m.vertex_color_flags),"has_vertex_colors":m.has_vertex_colors})}).collect::<Vec<_>>();
            let lifecycle_summary = match &lifecycle {
                Ok(Some(plan)) => {
                    serde_json::json!({"prepared":true,"controllers":plan.entries.iter().map(|e|(e.block,&e.class_name,e.property)).collect::<Vec<_>>()})
                }
                Ok(None) => serde_json::json!({"model_root":false}),
                Err(error) => serde_json::json!({"prepared":false,"error":error}),
            };
            let summary = serde_json::json!({"path":file.path,"blocks":nif.blocks.len(),"block_types":nif.block_types,"controller_startup":lifecycle_summary,
                "hierarchy_nodes":graph.nodes.len(),"transform_animation":animation.is_some(),"material_dependencies":dependencies.iter().map(|d|&d.path).collect::<Vec<_>>(),"particles":particle_summary,"materials":materials});
            self.prepared.insert(
                name.into(),
                Arc::new(Prepared {
                    summary,
                    model: nif,
                    hierarchy: graph,
                    dependencies,
                    particles,
                    clone_hierarchy,
                    emitter_plan,
                    lifecycle,
                }),
            );
        }
        Ok(self.prepared.get(name).map(Arc::as_ref))
    }
    pub fn instantiate(&mut self, name: &str) -> Result<Option<ModelInstance>> {
        if self.prepare(name)?.is_none() {
            return Ok(None);
        }
        let (registered, _) = self
            .catalog
            .resolve(name)?
            .context("Prepared registration missing")?;
        let template = self
            .prepared
            .get(registered)
            .context("Prepared model missing")?
            .clone();
        ModelInstance::new(template)
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"configured":self.catalog.paths,"unregistered":self.catalog.disabled,"prepared":self.prepared.iter().map(|(n,p)|(n,&p.summary)).collect::<BTreeMap<_,_>>(),"resources":self.store.summary(),"overlay_renderer_connected":false})
    }
}

fn clone_root(nif: &epic_mickey_lib_rs::nif_file::NifFile) -> Result<Option<u32>> {
    let Some(root) = nif.roots.first().copied().flatten() else {
        return Ok(None);
    };
    let block = nif
        .blocks
        .get(root as usize)
        .context("Invalid first streamed model object")?;
    if !super::nif_types::is_a(super::nif_types::block_name(nif, root)?, "NiAVObject")? {
        return Ok(None);
    }
    ensure!(
        block.as_av_object().is_some(),
        "Undecoded NiAVObject prefix for {}",
        block.type_name()
    );
    Ok(Some(root))
}

/// The main camera contributes its lens/viewport. The persistent HUD camera
/// contributes its own world transform; native20f9a8 doesn't copy the main
/// camera transform. Its renderer target must be supplied by its actual owner.
pub struct FrameCamera {
    pub frustum: [f32; 6],
    pub viewport: [f32; 4],
    pub tv_format: u32,
    pub wide: bool,
    pub hud_transform: epic_mickey_lib_rs::nif_file::NiTransform,
    pub target_size: [u32; 2],
}
#[derive(Default)]
pub struct PlacementState {
    pub projection: Option<Projection>,
    //20fb08 ignores WindowPointToRay's boolean, retaining both old outputs.
    // Initial native scratch contents have not been admitted as zeroes.
    pub ray: Option<super::cursor::Ray>,
}
impl PlacementState {
    pub fn place(
        &mut self,
        model: &mut ModelInstance,
        x: u32,
        y: u32,
        camera: &FrameCamera,
    ) -> Result<()> {
        let ray = self.window_ray(x, y, camera)?;
        model.place(
            ray.origin,
            ray.direction,
            camera.hud_transform.rotation.rows,
        )
    }
    pub fn window_ray(
        &mut self,
        x: u32,
        y: u32,
        camera: &FrameCamera,
    ) -> Result<super::cursor::Ray> {
        let p = projection(
            camera.frustum,
            camera.viewport,
            y,
            camera.tv_format,
            camera.wide,
        );
        let r = camera.hud_transform.rotation.rows;
        let t = &camera.hud_transform.translation;
        let unproject = super::cursor::Camera {
            position: [t.x, t.y, t.z],
            left: [r[0][0], r[1][0], r[2][0]],
            up: [r[0][1], r[1][1], r[2][1]],
            forward: [r[0][2], r[1][2], r[2][2]],
            viewport: super::cursor::Rect {
                left: p.viewport[0],
                right: p.viewport[1],
                top: p.viewport[2],
                bottom: p.viewport[3],
            },
            frustum: super::cursor::Rect {
                left: p.frustum[0],
                right: p.frustum[1],
                top: p.frustum[2],
                bottom: p.frustum[3],
            },
            orthographic: true,
        };
        let y = p.y;
        self.projection = Some(p);
        if let Some(ray) = unproject.window_ray([x as i32, y as i32], camera.target_size)? {
            self.ray = Some(ray);
        }
        self.ray.context("HUD unprojection rejected the first point; original retained scratch ray is unavailable")
    }
}
///2107b8 reads these DOUBLE fields once after all placements, then narrows.
/// Names preserve native field identities until the caller binds its clock.
pub struct FrameTimes {
    pub at10: f64,
    pub at20: f64,
}
impl FrameTimes {
    ///14b5bc stores accumulated unscaled time at10;14b60c stores
    /// accumulated hardware time at20. Scaled gameplay time is field18.
    pub fn from_clock(clock: &super::game_clock::Clock) -> Self {
        Self {
            at10: clock.unscaled_seconds,
            at20: clock.hardware_seconds,
        }
    }
}
pub trait FrameServices {
    /// Queried separately for each effect, as native20f9a8 does. Missing main
    /// camera skips placement only; root updates must still execute.
    fn camera(&mut self, id: u32) -> Result<Option<FrameCamera>>;
    fn times(&mut self) -> Result<FrameTimes>;
    ///16cdf0 sequence update is distinct from NiAVObject::Update. It must
    /// not be silently replaced by a second direct-transform update.
    fn sequences(&mut self, layer: usize, models: &mut Models, time: f32) -> Result<()>;
    fn update_root(&mut self, layer: usize, models: &mut Models, time: f32) -> Result<()>;
}

/// Mutable graph/controller/particle state belongs to one clone. The NIF,
/// decoded texture data and selected-root hierarchy remain immutable/shared.
pub struct ModelInstance {
    pub template: Arc<Prepared>,
    pub hierarchy: Arc<super::scene_bounds::Hierarchy>,
    pub particles: Option<super::brush_particles::Hose>,
    pub animation: Option<super::nif_runtime::Instance>,
    pub world: BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>,
    root: u32,
    locals: BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>,
}
impl ModelInstance {
    ///804bf484 uses one supplied time for all controller Starts. Validate all
    /// playback owners first, including property controllers, then preserve
    /// the native traversal and repeated/shared-controller calls.
    pub fn start_animations(&mut self, time: f32) -> Result<()> {
        ensure!(time.is_finite(), "Invalid controller startup time");
        let plan = self
            .template
            .lifecycle
            .as_ref()
            .map_err(|e| anyhow::anyhow!("Controller startup: {e}"))?
            .as_ref()
            .context("No cloned controller lifecycle")?;
        for entry in &plan.entries {
            let transform = self
                .animation
                .as_ref()
                .is_some_and(|owner| owner.owns_controller(entry.block));
            let particle = self
                .particles
                .as_ref()
                .is_some_and(|owner| owner.owns_controller(entry.block));
            ensure!(
                usize::from(transform) + usize::from(particle) == 1,
                "{} #{} (property {:?}) requires one actual controller owner",
                entry.class_name,
                entry.block,
                entry.property
            );
        }
        plan.start(time, |block, time| {
            if let Some(owner) = self
                .animation
                .as_mut()
                .filter(|owner| owner.owns_controller(block))
            {
                owner.start_controller(block, time)
            } else {
                self.particles
                    .as_mut()
                    .context("Particle controller owner missing")?
                    .start_controller(block, time)
            }
        })
    }
    /// Continuous model-owned preview with the application's absolute time
    /// and random owner. Controller traversal/simulator parity is still under
    /// recovery; this is not full NiAVObject::Update admission.
    pub fn preview_at(&mut self, time: f32, random: &mut super::game_random::State) -> Result<()> {
        self.update_direct_transforms(time)?;
        if let Some(particles) = &mut self.particles {
            particles.advance_attached_at(
                time,
                [
                    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                ],
                random,
            )?;
        }
        Ok(())
    }
    /// Native command traversal with a caller-owned application clock.
    /// Validate ownership before changing anything: additional emitter float
    /// controller classes require their own actual playback owners.
    pub fn emitters(&mut self, enabled: bool, clock: impl FnMut() -> Result<f64>) -> Result<()> {
        let plan = self
            .template
            .emitter_plan
            .as_ref()
            .map_err(|error| anyhow::anyhow!("Emitter traversal: {error}"))?
            .as_ref()
            .context("No cloned emitter traversal")?;
        for id in plan.emitters() {
            ensure!(
                self.particles
                    .as_ref()
                    .is_some_and(|p| p.supports_emitter_command(id)),
                "Emitter controller {id} has no playback owner"
            );
        }
        plan.execute(enabled, clock, |id, time| {
            self.particles
                .as_mut()
                .context("Missing particle owner")?
                .emitter_command(id, time)
        })
    }
    fn new(template: Arc<Prepared>) -> Result<Option<Self>> {
        let Some(hierarchy) = template.clone_hierarchy.clone() else {
            return Ok(None);
        };
        let root = hierarchy.roots[0].context("Selected model root missing")?;
        let particles = template
            .particles
            .as_ref()
            .map_err(|error| anyhow::anyhow!("Particle prototype: {error}"))?
            .as_ref()
            .map(|prototype| prototype.instantiate_subtree(hierarchy.clone()))
            .transpose()?;
        let animation = super::nif_runtime::Instance::load(&hierarchy)?;
        let world = hierarchy.base_world_transforms(None)?;
        let locals = hierarchy
            .nodes
            .iter()
            .map(|(id, node)| (*id, node.local.clone()))
            .collect();
        Ok(Some(Self {
            template,
            hierarchy,
            particles,
            animation,
            world,
            root,
            locals,
        }))
    }
    ///20fb20..20fb98 replaces the root's local transform. REL684c is fabs,
    /// not a clock getter; the authored input at4bcf88 is exactly float0.1.
    /// World publication belongs to the subsequent root update, not placement.
    /// In particular an unattached clone must retain its previous world frame.
    pub fn place(
        &mut self,
        origin: [f32; 3],
        direction: [f32; 3],
        rotation: [[f32; 3]; 3],
    ) -> Result<()> {
        let transform = placement(origin, direction, rotation)?;
        self.locals.insert(self.root, transform);
        Ok(())
    }
    /// Direct transform-controller writes affect the current mutable local
    /// state. Their native call order relative to placement is the caller's
    /// responsibility; an unchanged controller must not reapply an old pose.
    pub fn update_direct_transforms(&mut self, time: f32) -> Result<()> {
        if let Some(animation) = &mut self.animation {
            animation.update_with_local_state(
                &self.hierarchy,
                time,
                &mut self.locals,
                |_, _| Ok(()),
            )?;
            self.world = animation.world.clone();
            if let Some(particles) = &mut self.particles {
                particles.publish_model_transforms(&self.world)?;
            }
            Ok(())
        } else {
            self.resolve()
        }
    }
    pub fn resolve(&mut self) -> Result<()> {
        self.world = self.hierarchy.base_world_transforms_with_overrides(
            None,
            &BTreeMap::new(),
            &self.locals,
        )?;
        if let Some(particles) = &mut self.particles {
            particles.publish_model_transforms(&self.world)?;
        }
        Ok(())
    }
}
pub fn placement(
    origin: [f32; 3],
    direction: [f32; 3],
    rotation: [[f32; 3]; 3],
) -> Result<epic_mickey_lib_rs::nif_file::NiTransform> {
    ensure!(
        origin
            .iter()
            .chain(&direction)
            .chain(rotation.iter().flatten())
            .all(|v| v.is_finite()),
        "Invalid HUD placement ray/rotation"
    );
    let p: [f32; 3] = std::array::from_fn(|i| origin[i] + direction[i]);
    ensure!(p.iter().all(|v| v.is_finite()), "HUD placement overflow");
    Ok(epic_mickey_lib_rs::nif_file::NiTransform {
        translation: epic_mickey_lib_rs::nif_file::Vector3 {
            x: p[0],
            y: p[1],
            z: p[2],
        },
        rotation: epic_mickey_lib_rs::nif_file::Matrix3 { rows: rotation },
        scale: f32::from_bits(0x3dcccccd),
    })
}

/// The owner requires actual initialization, emitter and release services.
/// Supplying a clone alone must never silently acknowledge those operations.
pub trait ModelServices {
    /// The stable model handle exists before initialization/attachment. On
    /// failure, this service must clean up any partial external backend work.
    fn initialize(&mut self, handle: usize, model: &mut ModelInstance) -> Result<()>;
    fn emitters(&mut self, handle: usize, model: &mut ModelInstance, enabled: bool) -> Result<()>;
    fn release(&mut self, handle: usize, model: &mut ModelInstance) -> Result<()>;
}
#[derive(Default)]
pub struct Models {
    instances: BTreeMap<usize, ModelInstance>,
    layers: [Vec<Option<usize>>; 4],
    next: usize,
}
fn attach_slot(children: &mut Vec<Option<usize>>, handle: usize) {
    if let Some(slot) = children.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some(handle)
    } else {
        children.push(Some(handle));
    }
}
fn remove_slot(children: &mut Vec<Option<usize>>, index: usize) {
    children[index] = None;
    //803b7a34 shrinks by ONE when the removed index is the last active slot.
    // It does not recursively trim earlier empty slots.
    if index + 1 == children.len() {
        children.pop();
    }
}
impl Models {
    pub fn layer(&self, index: usize) -> Result<&[Option<usize>]> {
        self.layers
            .get(index)
            .map(Vec::as_slice)
            .context("Invalid HUD layer")
    }
    pub fn model(&self, handle: usize) -> Option<&ModelInstance> {
        self.instances.get(&handle)
    }
    pub fn model_mut(&mut self, handle: usize) -> Option<&mut ModelInstance> {
        self.instances.get_mut(&handle)
    }
    fn allocate(&mut self) -> Result<usize> {
        let handle = self
            .next
            .checked_add(1)
            .context("HUD model handle space exhausted")?;
        self.next = handle;
        Ok(handle)
    }
    fn insert(&mut self, handle: usize, model: ModelInstance, layer: i32) {
        self.instances.insert(handle, model);
        if let Some(children) = usize::try_from(layer)
            .ok()
            .and_then(|layer| self.layers.get_mut(layer))
        {
            attach_slot(children, handle);
        }
    }
    fn detach(&mut self, handle: usize) {
        for layer in &mut self.layers {
            if let Some(index) = layer.iter().position(|child| *child == Some(handle)) {
                remove_slot(layer, index);
            }
        }
    }
}
struct OwnedBackend<'a, S> {
    resources: &'a mut Resources,
    models: &'a mut Models,
    services: &'a mut S,
}
impl<S: ModelServices> Backend for OwnedBackend<'_, S> {
    type Handle = usize;
    fn clone_effect(&mut self, name: &str, layer: i32) -> Result<Option<usize>> {
        let Some(mut model) = self.resources.instantiate(name)? else {
            return Ok(None);
        };
        let handle = self.models.allocate()?;
        self.services.initialize(handle, &mut model)?;
        self.models.insert(handle, model, layer);
        Ok(Some(handle))
    }
    fn emitters(&mut self, handle: usize, enabled: bool) -> Result<()> {
        self.services.emitters(
            handle,
            self.models
                .instances
                .get_mut(&handle)
                .context("HUD model missing")?,
            enabled,
        )
    }
    fn release(&mut self, handle: usize) -> Result<()> {
        self.models.detach(handle);
        self.services.release(
            handle,
            self.models
                .instances
                .get_mut(&handle)
                .context("HUD model missing")?,
        )?;
        self.models.instances.remove(&handle);
        Ok(())
    }
}

pub struct Runtime {
    pub resources: Resources,
    pub models: Models,
    pub placement: PlacementState,
    state: State<usize>,
}
impl Runtime {
    pub fn load(root: &std::path::Path) -> Result<Self> {
        Ok(Self {
            resources: Resources::load(root)?,
            models: Models::default(),
            placement: PlacementState::default(),
            state: State::default(),
        })
    }
    ///2107b8: place in ID hash order, then update roots0,2,1,3 with two
    /// different clocks. Unattached models are positioned but not root-updated.
    pub fn frame_with_services(&mut self, services: &mut impl FrameServices) -> Result<()> {
        for (id, instance) in self.state.native_order() {
            if let Some(camera) = services.camera(id)? {
                let model = self
                    .models
                    .instances
                    .get_mut(&instance.handle)
                    .context("HUD frame model missing")?;
                self.placement
                    .place(model, instance.x, instance.y, &camera)?;
            }
        }
        let times = services.times()?;
        let at10 = times.at10 as f32;
        let at20 = times.at20 as f32;
        services.update_root(0, &mut self.models, at10)?;
        services.sequences(2, &mut self.models, at20)?;
        services.update_root(2, &mut self.models, at20)?;
        services.update_root(1, &mut self.models, at10)?;
        services.sequences(3, &mut self.models, at20)?;
        services.update_root(3, &mut self.models, at20)
    }
    pub fn command_with_services(
        &mut self,
        command: &Command,
        services: &mut impl ModelServices,
    ) -> Result<Outcome> {
        self.state.command(
            command,
            &mut OwnedBackend {
                resources: &mut self.resources,
                models: &mut self.models,
                services,
            },
        )
    }
    /// Explicit fallible cleanup while controller/render services are still
    /// alive. This uses normal per-ID release, including unattached models.
    pub fn clear_with_services(&mut self, services: &mut impl ModelServices) -> Result<()> {
        let ids = self
            .state
            .native_order()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        for id in ids {
            self.command_with_services(&Command::Despawn { id }, services)?;
        }
        Ok(())
    }
    pub fn command(&mut self, command: &Command) -> Result<Outcome> {
        struct Pending;
        impl ModelServices for Pending {
            fn initialize(&mut self, _: usize, _: &mut ModelInstance) -> Result<()> {
                anyhow::bail!("Native HUD model cloned, but controller initialization, clocks and overlay rendering are not connected")
            }
            fn emitters(&mut self, _: usize, _: &mut ModelInstance, _: bool) -> Result<()> {
                anyhow::bail!("Native HUD emitter owner is not connected")
            }
            fn release(&mut self, _: usize, _: &mut ModelInstance) -> Result<()> {
                anyhow::bail!("Native HUD scene-graph release services are not connected")
            }
        }
        self.command_with_services(command, &mut Pending)
    }
}

#[cfg(test)]
mod tests {

    fn live_camera_fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_effect_camera_live.json"
        ))
        .unwrap()
    }
    #[test]
    fn captured_native_camera_ray_and_model_transform_match_every_bit() -> anyhow::Result<()> {
        use super::*;
        let fixture = live_camera_fixture();
        let bits = |key: &str| {
            fixture[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap() as u32)
                .collect::<Vec<_>>()
        };
        let floats = |key: &str| {
            bits(key)
                .into_iter()
                .map(f32::from_bits)
                .collect::<Vec<_>>()
        };
        let local = floats("camera_world_bits");
        let transform = epic_mickey_lib_rs::nif_file::NiTransform {
            rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                rows: std::array::from_fn(|r| std::array::from_fn(|c| local[r * 3 + c])),
            },
            translation: epic_mickey_lib_rs::nif_file::Vector3 {
                x: local[9],
                y: local[10],
                z: local[11],
            },
            scale: local[12],
        };
        let dimensions = |key: &str| {
            fixture[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_u64().unwrap() as u32)
                .collect::<Vec<_>>()
        };
        let camera = FrameCamera {
            frustum: floats("frustum_bits").try_into().unwrap(),
            viewport: floats("viewport_bits").try_into().unwrap(),
            // The fixture is the already-propagated HUD camera. Format0
            // leaves this captured lens intact; platform binding is not tested.
            tv_format: 0,
            wide: true,
            hud_transform: transform,
            target_size: dimensions("renderer_target").try_into().unwrap(),
        };
        let pixel = dimensions("pixel");
        let mut state = PlacementState::default();
        let ray = state.window_ray(pixel[0], pixel[1], &camera)?;
        assert_eq!(
            ray.origin.map(f32::to_bits).as_slice(),
            bits("ray_origin_bits")
        );
        assert_eq!(
            ray.direction.map(f32::to_bits).as_slice(),
            bits("ray_direction_bits")
        );
        let model = placement(
            ray.origin,
            ray.direction,
            camera.hud_transform.rotation.rows,
        )?;
        let actual = model
            .rotation
            .rows
            .into_iter()
            .flatten()
            .chain([
                model.translation.x,
                model.translation.y,
                model.translation.z,
                model.scale,
            ])
            .map(f32::to_bits)
            .collect::<Vec<_>>();
        assert_eq!(actual, bits("model_local_bits"));
        // Invalid later points retain the last native scratch ray.
        let retained = state.window_ray(u32::MAX, 0, &camera)?;
        assert_eq!(
            retained.origin.map(f32::to_bits),
            ray.origin.map(f32::to_bits)
        );
        assert_eq!(
            retained.direction.map(f32::to_bits),
            ray.direction.map(f32::to_bits)
        );
        assert!(PlacementState::default()
            .window_ray(u32::MAX, 0, &camera)
            .is_err());
        Ok(())
    }
    #[test]
    fn original_movie_mixed_case_names_resolve_without_alias_registrations() -> anyhow::Result<()> {
        use super::*;
        let settings = BTreeMap::from([
            ("HUDEffectCount".into(), "11".into()),
            ("HUDEffect007".into(), "Sketch.nif_wii".into()),
        ]);
        let catalog = Catalog::hud(&settings)?;
        let command = live_camera_fixture()["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        let Command::Spawn { name, .. } = parse(
            command["command"].as_str().unwrap(),
            command["arguments"].as_str().unwrap(),
        )?
        else {
            panic!("Captured spawn missing")
        };
        assert_eq!(name, "HudEffect007");
        assert_eq!(
            catalog.resolve(&name)?,
            Some(("HUDEffect007", "Sketch.nif_wii"))
        );
        assert_eq!(
            catalog.resolve("hudeffect007\0ignored")?,
            catalog.resolve(&name)?
        );
        assert_eq!(catalog.resolve("HudEffect002")?, None);
        assert_eq!(catalog.paths.len(), 1);
        assert!(catalog.resolve("HudEffect099").is_err());
        assert!(catalog.resolve("HudEff\u{e9}ct007").is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_case_aliases_share_prepared_resources_and_keep_separate_instances(
    ) -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        let upper = resources
            .instantiate("HUDEffect007")?
            .context("Registered effect missing")?;
        let mixed = resources
            .instantiate("HudEffect007")?
            .context("Original movie spelling rejected")?;
        let lower = resources
            .instantiate("hudeffect007\0suffix")?
            .context("C-string alias rejected")?;
        assert!(Arc::ptr_eq(&upper.template, &mixed.template));
        assert!(Arc::ptr_eq(&upper.template, &lower.template));
        assert_eq!(resources.prepared.len(), 1);
        assert!(resources.prepared.contains_key("HUDEffect007"));
        assert!(resources.instantiate("hudeffect002")?.is_none());
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_checkpoint_vortex_advances_and_exports_actual_particle_draws() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        let mut model = resources
            .instantiate("HudEffect007")?
            .context("Checkpoint resource missing")?;
        let mut random = super::super::game_random::State::default();
        model.preview_at(0., &mut random)?;
        model.start_animations(12.25)?;
        let mut peak = 0;
        let mut exported = 0;
        for tick in 0..=90 {
            model.preview_at(12.25 + tick as f32 / 30., &mut random)?;
            let particles = model
                .particles
                .as_ref()
                .context("Checkpoint particle owner missing")?;
            peak = peak.max(particles.particle_count());
            let mut draws = Vec::new();
            particles.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
            for draw in draws {
                ensure!(
                    draw.preview.as_ref().is_some_and(|program| program.is_ok()),
                    "Checkpoint particle has no actual material program"
                );
                ensure!(
                    !draw.material_images.is_empty(),
                    "Checkpoint particle textures missing"
                );
                ensure!(
                    draw.v_data.iter().all(|v| v.is_finite()),
                    "Nonfinite checkpoint particle geometry"
                );
                exported += usize::from(!draw.indices.is_empty());
            }
        }
        ensure!(
            peak > 0 && exported > 0,
            "Checkpoint particle simulation/export produced no geometry"
        );
        std::fs::write(
            "target/ui-checkpoint-vortex-preview.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "peak_particles":peak,"exported_draws":exported,"frames":91,"random":random,
            "warnings":model.particles.as_ref().unwrap().warnings,
            "scope":"Original checkpoint model/controller/absolute-time particle path and prepared draw export. Native initialization, shared gameplay phase/RNG ownership and actual HUD GPU composition remain unaccepted.",
            "live_hud_effects_connected":false,"full_effect_parity_accepted":false}))?,
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_watch_particle_prefix_preserves_the_absent_skin_tag() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        let file = resources
            .store
            .read("Effects/_Shared/Sketch_Watch_Gears.nif_wii")?;
        let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            nif.version == 0x14060500 && nif.user_version == 9,
            "Wrong owned watch profile"
        );
        let epic_mickey_lib_rs::nif_file::NiObject::NiPSParticleSystem(ps) = &nif.blocks[8] else {
            anyhow::bail!("Watch particle prefix remained raw")
        };
        assert_eq!(ps.base.hw_skin_flag, 0);
        assert_eq!(ps.simulator_link, Some(23));
        assert_eq!(ps.generator_link, Some(30));
        let saved = nif.to_bytes().map_err(anyhow::Error::msg)?;
        let original = super::super::jigsaw::raw_blocks(&file.data)?;
        let written = super::super::jigsaw::raw_blocks(&saved)?;
        assert_eq!(original[8].len(), 240);
        assert_eq!(
            written[8], original[8],
            "Watch particle serialization inserted or dropped prefix data"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_all_configured_hud_models_start_their_actual_controllers() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        let names = resources.catalog.paths.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names.len(), 10);
        let mut random = super::super::game_random::State::default();
        let mut rows = Vec::new();
        let mut failed = Vec::new();
        for name in names {
            let result = (|| -> Result<serde_json::Value> {
                let mut model = resources
                    .instantiate(&name)?
                    .context("Configured HUD model unregistered")?;
                model.preview_at(0., &mut random)?;
                model.start_animations(12.25)?;
                let mut counts = Vec::new();
                let mut transform_writes = 0;
                let mut samples = Vec::new();
                for tick in 0..=60 {
                    let time = 12.25 + tick as f32 / 30.;
                    model.preview_at(time, &mut random)?;
                    counts.push(model.particles.as_ref().map_or(0, |p| p.particle_count()));
                    transform_writes =
                        transform_writes.max(model.animation.as_ref().map_or(0, |a| a.poses.len()));
                    if name == "HUDEffect006" && matches!(tick, 0 | 15 | 30) {
                        samples.push(serde_json::json!({"time":time,"animated_nodes":model.animation.as_ref().map(|a|a.affected.iter().map(|id|(*id,&model.hierarchy.nodes[id].name,super::super::nif_runtime::matrix(&model.world[id]))).collect::<Vec<_>>())}));
                    }
                }
                if model.animation.is_some() {
                    ensure!(
                        transform_writes > 0,
                        "Controller startup produced no transform writes"
                    );
                }
                Ok(
                    serde_json::json!({"name":name,"path":model.template.summary["path"],"started":true,
                    "controller_count":model.template.lifecycle.as_ref().unwrap().as_ref().unwrap().entries.len(),
                    "transform_targets_written":transform_writes,"peak_particles":counts.into_iter().max(),"watch_samples":samples}),
                )
            })();
            match result {
                Ok(row) => rows.push(row),
                Err(error) => {
                    failed.push(name.clone());
                    rows.push(serde_json::json!({"name":name,"started":false,"error":format!("{error:#}")}));
                }
            }
        }
        std::fs::write(
            "target/ui-all-hud-controller-startup.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"All configured original HUD models, supplied absolute time/global RNG. Controller startup/preview only; full clone initialization and live rendering remain unconnected.",
            "models":rows,"failed":failed,"random":random,"resources":resources.store.summary()}))?,
        )?;
        ensure!(failed.is_empty(),"HUD controller startup failures: {failed:?}; see target/ui-all-hud-controller-startup.json");
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_status_ping_exports_its_actual_particle_draw() -> anyhow::Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = super::Resources::load(&root)?;
        let mut model = resources
            .instantiate("HUDEffect003")?
            .context("Missing status ping")?;
        let mut random = super::super::game_random::State::default();
        model.preview_at(0., &mut random)?;
        model.start_animations(12.25)?;
        for tick in 0..=60 {
            let time = 12.25 + tick as f32 / 30.;
            model.preview_at(time, &mut random)?;
            let mut draws = Vec::new();
            model
                .particles
                .as_ref()
                .context("Missing particles")?
                .append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
            let Some(draw) = draws
                .first()
                .filter(|d| d.aabb_max[0] > d.aabb_min[0] && d.aabb_max[1] > d.aabb_min[1])
            else {
                continue;
            };
            let program = draw
                .preview
                .as_ref()
                .context("No draw program")?
                .as_ref()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            ensure!(
                !program.pinned_projection && draw.extra_uvs.is_none(),
                "Demonstration cannot supply pinned projection or additional UV channels"
            );
            ensure!(
                draw.material_images.len() == 1,
                "Demonstration requires exactly one actual material image"
            );
            let image = &draw.material_images[0];
            let prefix = "target/ui-status-ping";
            std::fs::write(format!("{prefix}.vert"), program.vertex.as_bytes())?;
            std::fs::write(format!("{prefix}.frag"), program.fragment.as_bytes())?;
            std::fs::write(format!("{prefix}.rgba"), image.rgba.as_slice())?;
            std::fs::write(
                format!("{prefix}.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                "scope":"Isolated original status-ping particle draw with supplied clock, RNG and camera axes. Provisional simulation; not gameplay or original-frame parity.",
                "model":model.template.summary["path"],"time":time,"width":image.width,"height":image.height,
                "texture_slot":image.slot,"texture_matrix":image.matrix,"uv_sets":program.uv_sets,
                "registers":program.presentation_registers(draw.material_registers.as_ref()),
                "blend":program.blend,"cull":program.cull,
                "sampler":{"min_filter":image.sampler.min_filter,"mag_filter":image.sampler.mag_filter,"wrap_u":image.sampler.wrap_u,"wrap_v":image.sampler.wrap_v}}))?,
            )?;
            std::fs::write(
                format!("{prefix}.mesh.json"),
                serde_json::to_vec(&serde_json::json!({
                "scope":"Actual source-driven preview particle vertices, UVs and colors; auto-fit camera, isolated effect.",
                "vertices":draw.v_data.as_ref(),"indices":draw.indices.as_ref()}))?,
            )?;
            return Ok(());
        }
        anyhow::bail!("Status ping produced no drawable geometry")
    }
    #[test]
    fn startup_reaches_transform_owner_and_rejects_missing_property_owner_before_mutation(
    ) -> anyhow::Result<()> {
        use super::*;
        use epic_mickey_lib_rs::nif_file::{
            NiMaterialPropertyData, NiNodeData, NiObject, NifFile, RawBlockData,
        };
        fn model(property: bool) -> Result<ModelInstance> {
            let controller = |name: &str, target: u32| {
                let mut bytes = u32::MAX.to_be_bytes().to_vec();
                bytes.extend(0x45u16.to_be_bytes());
                for bits in [1f32.to_bits(), 0, 0, 10f32.to_bits(), target, u32::MAX] {
                    bytes.extend(bits.to_be_bytes());
                }
                NiObject::Unknown(RawBlockData {
                    type_name: name.into(),
                    raw_data: bytes,
                })
            };
            let mut root = NiNodeData::default();
            root.base.transform.scale = 1.;
            root.base.base.controller_link = Some(3);
            if property {
                root.base.property_links = vec![Some(1)];
            }
            let mut material = NiMaterialPropertyData::default();
            material.base.controller_link = Some(2);
            let nif = NifFile {
                version: 0x14020007,
                roots: vec![Some(0)],
                blocks: vec![
                    NiObject::NiNode(root),
                    NiObject::NiMaterialProperty(material),
                    controller("NiAlphaController", 1),
                    controller("NiTransformController", 0),
                ],
                ..Default::default()
            };
            let mut graph = super::super::scene_bounds::Hierarchy::load(&nif)?;
            // Supplied resolved constant interpolator, as in the transform
            // owner regression; this isn't an asset decoding parity fixture.
            graph.nodes.get_mut(&0).unwrap().controllers[0].transform =
                Some(Ok(super::super::nif_transform::Interpolator {
                    translation: [1., 2., 3.],
                    rotation: [-f32::MAX; 4],
                    scale: -f32::MAX,
                    data: None,
                    channels: None,
                }));
            let lifecycle = super::super::controller_lifecycle::Plan::load(&nif, &graph, 0)?;
            let graph = Arc::new(graph);
            let template = Arc::new(Prepared {
                summary: serde_json::json!({}),
                model: nif,
                hierarchy: graph.clone(),
                dependencies: vec![],
                particles: Ok(None),
                clone_hierarchy: Some(graph),
                emitter_plan: Ok(None),
                lifecycle: Ok(Some(Arc::new(lifecycle))),
            });
            ModelInstance::new(template)?.context("Missing supplied model")
        }
        let mut random = super::super::game_random::State::default();
        let mut unsupported = model(true)?;
        assert!(unsupported
            .start_animations(12.25)
            .unwrap_err()
            .to_string()
            .contains("NiAlphaController"));
        unsupported.preview_at(12.25, &mut random)?;
        assert_eq!(
            unsupported.world[&0].translation.x, 0.,
            "Failure started an earlier controller before checking the property owner"
        );
        let mut supported = model(false)?;
        supported.preview_at(0., &mut random)?;
        assert_eq!(supported.world[&0].translation.x, 0.);
        supported.start_animations(12.25)?;
        supported.preview_at(12.25, &mut random)?;
        assert_eq!(supported.world[&0].translation.x, 1.);
        supported.place(
            [9., 0., 0.],
            [0., 0., 1.],
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        )?;
        supported.start_animations(12.5)?;
        supported.preview_at(12.5, &mut random)?;
        assert_eq!(
            supported.world[&0].translation.x, 9.,
            "Restart cleared the native base scaled-value cache"
        );
        supported.preview_at(12.75, &mut random)?;
        assert_eq!(supported.world[&0].translation.x, 1.);
        Ok(())
    }
    #[test]
    fn frame_clock_domains_preserve_paused_hud_time() -> anyhow::Result<()> {
        use super::super::game_clock::{Clock, Rate, Step};
        let mut clock = Clock::new(Rate::from_bus_clock(4000)?, 0);
        clock.sample(Step::Realtime, 100, 0.5);
        let running = super::FrameTimes::from_clock(&clock);
        assert_ne!(running.at10, clock.scaled_seconds);
        clock.sample(Step::Frozen, 600, 0.5);
        let paused = super::FrameTimes::from_clock(&clock);
        assert_eq!(paused.at10, running.at10);
        assert_eq!(paused.at20, running.at20 + 0.5);
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_absolute_effect_updates_preserve_clock_and_global_random_ownership(
    ) -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        let mut random = super::super::game_random::State::default();
        let mut observations = Vec::new();
        for name in ["HUDEffect000", "HUDEffect001"] {
            let mut model = resources.instantiate(name)?.unwrap();
            for (id, node) in &model.hierarchy.nodes {
                for c in &node.controllers {
                    let source = model.template.hierarchy.nodes[id]
                        .controllers
                        .iter()
                        .find(|source| source.block == c.block)
                        .unwrap();
                    assert_eq!(
                        c.flags,
                        super::super::nif_runtime::effect_template_flags(source.flags)
                    );
                }
            }
            let before = random;
            model.preview_at(0., &mut random)?;
            assert_eq!(
                random, before,
                "Zero-time initialization consumed unexpected random draws"
            );
            assert!(
                model.particles.as_ref().unwrap().instantiate().is_err(),
                "Updated zero-time state was reused as a clean prototype"
            );
            model.start_animations(12.25)?;
            let mut generated = false;
            let mut captured = name != "HUDEffect000";
            for tick in 0..120 {
                let time = 12.25 + tick as f32 / 60.;
                model.preview_at(time, &mut random)?;
                let count = model.particles.as_ref().unwrap().particle_count();
                generated |= count > 0;
                if !captured && count > 0 {
                    let mut draws = Vec::new();
                    model.particles.as_ref().unwrap().append(
                        [1., 0., 0.],
                        [0., 1., 0.],
                        &mut draws,
                    )?;
                    if let Some(draw) = draws.first().filter(|draw| {
                        draw.aabb_max[0] > draw.aabb_min[0] || draw.aabb_max[1] > draw.aabb_min[1]
                    }) {
                        std::fs::write(
                            "target/ui-absolute-health-loss.mesh.json",
                            serde_json::to_vec(&serde_json::json!({
                            "scope":"Prepared HUD controller template, supplied absolute application clock and shared retail RNG. Remaining simulation/initialization/render parity unvalidated; not gameplay.",
                            "application_time":time,"random":random,"particle_count":count,
                            "vertices":draw.v_data.as_ref(),"indices":draw.indices.as_ref()}))?,
                        )?;
                        captured = true;
                    }
                }
                let after = random;
                model.preview_at(time, &mut random)?;
                assert_eq!(random, after, "Repeated absolute time emitted again");
                assert_eq!(model.particles.as_ref().unwrap().particle_count(), count);
            }
            assert!(generated, "Absolute {name} playback generated no particles");
            assert!(
                captured,
                "Absolute {name} playback generated no drawable geometry"
            );
            let after = random;
            assert!(model
                .particles
                .as_mut()
                .unwrap()
                .advance_attached(
                    1. / 60.,
                    true,
                    [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
                )
                .is_err());
            assert!(
                model.preview_at(0., &mut random).is_err(),
                "Unimplemented backwards-clock reset was silently accepted"
            );
            assert_eq!(random, after);
            observations.push(serde_json::json!({"model":name,"before":before,"after":after,"generated":generated}));
        }
        assert!(
            random.draws > 0,
            "Owned profiles didn't exercise the shared random source"
        );
        let mut expected = super::super::game_random::State::default();
        for _ in 0..random.draws {
            expected.draw();
        }
        assert_eq!(random, expected);
        std::fs::write(
            "target/ui-absolute-effect-playback.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Owned health effects with supplied absolute application time and shared retail RNG. Simulator/init/render parity remains unvalidated.",
            "observations":observations,"final_random":random,"duplicate_times_consume_no_random":true,"backwards_reset_explicitly_unsupported":true}))?,
        )?;
        Ok(())
    }
    #[test]
    fn stream_root_accepts_meshes_and_does_not_search_later_roots() -> anyhow::Result<()> {
        use epic_mickey_lib_rs::nif_file::{
            NiMaterialPropertyData, NiMeshData, NiNodeData, NiObject, NifFile,
        };
        let mut nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiMesh(NiMeshData::default())],
            ..Default::default()
        };
        assert_eq!(super::clone_root(&nif)?, Some(0));
        nif.blocks = vec![
            NiObject::NiMaterialProperty(NiMaterialPropertyData::default()),
            NiObject::NiNode(NiNodeData::default()),
        ];
        nif.roots = vec![Some(0), Some(1)];
        assert_eq!(super::clone_root(&nif)?, None);
        nif.roots = vec![Some(1), Some(0)];
        assert_eq!(super::clone_root(&nif)?, Some(1));
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_frame_places_real_models_and_matches_native_root_schedule() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        struct Init;
        impl ModelServices for Init {
            fn initialize(&mut self, _: usize, _: &mut ModelInstance) -> Result<()> {
                Ok(())
            }
            fn emitters(&mut self, _: usize, _: &mut ModelInstance, _: bool) -> Result<()> {
                anyhow::bail!("Unexpected emitter command")
            }
            fn release(&mut self, _: usize, _: &mut ModelInstance) -> Result<()> {
                Ok(())
            }
        }
        struct Services {
            calls: Vec<serde_json::Value>,
            times: [f64; 2],
        }
        impl FrameServices for Services {
            fn camera(&mut self, id: u32) -> Result<Option<FrameCamera>> {
                self.calls.push(serde_json::json!(["place", id]));
                Ok(None)
            }
            fn times(&mut self) -> Result<FrameTimes> {
                Ok(FrameTimes {
                    at10: self.times[0],
                    at20: self.times[1],
                })
            }
            fn sequences(&mut self, layer: usize, _: &mut Models, time: f32) -> Result<()> {
                self.calls
                    .push(serde_json::json!(["sequences", layer, time.to_bits(), 1]));
                Ok(())
            }
            fn update_root(&mut self, layer: usize, _: &mut Models, time: f32) -> Result<()> {
                self.calls
                    .push(serde_json::json!(["update", layer, time.to_bits(), 1]));
                Ok(())
            }
        }
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ui_frame.json"))?;
        let cases = reference["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 12);
        for case in cases {
            let mut runtime = Runtime::load(&root)?;
            for (i, id) in case["ids"].as_array().unwrap().iter().enumerate() {
                runtime.command_with_services(
                    &Command::Spawn {
                        id: id.as_u64().unwrap() as u32,
                        x: (i * 9) as f32,
                        y: (i * 11) as f32,
                        layer: i as i32,
                        name: "HUDEffect000".into(),
                    },
                    &mut Init,
                )?;
            }
            let mut services = Services {
                calls: Vec::new(),
                times: [
                    case["times"][0].as_f64().unwrap(),
                    case["times"][1].as_f64().unwrap(),
                ],
            };
            runtime.frame_with_services(&mut services)?;
            let expected = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| {
                    if call[0] == "place" {
                        serde_json::json!(["place", call[1]])
                    } else {
                        call.clone()
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(services.calls, expected, "{case}");
        }
        let mut resources = Resources::load(&root)?;
        let mut model = resources.instantiate("HUDEffect000")?.unwrap();
        let root_id = model.root;
        let original = model.world[&root_id].clone();
        let mut placement = PlacementState::default();
        let camera = FrameCamera {
            frustum: [-4., 4., 2., -2., 0.1, 100.],
            viewport: [0., 1., 1., 0.],
            tv_format: 0,
            wide: false,
            hud_transform: epic_mickey_lib_rs::nif_file::NiTransform {
                translation: epic_mickey_lib_rs::nif_file::Vector3 {
                    x: 1.,
                    y: 2.,
                    z: 3.,
                },
                rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                    rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                },
                scale: 1.,
            },
            target_size: [800, 400],
        };
        assert!(placement.place(&mut model, 801, 0, &camera).is_err());
        assert_eq!(
            super::super::nif_runtime::matrix(&model.world[&root_id]),
            super::super::nif_runtime::matrix(&original)
        );
        placement.place(&mut model, 800, 0, &camera)?;
        assert_eq!(
            super::super::nif_runtime::matrix(&model.world[&root_id]),
            super::super::nif_runtime::matrix(&original),
            "Placement prematurely published the particle world frame"
        );
        let placed = &model.locals[&root_id];
        assert_eq!(
            [
                placed.translation.x,
                placed.translation.y,
                placed.translation.z
            ],
            [-3., 4., 4.]
        );
        assert_eq!(placed.scale.to_bits(), 0x3dcccccd);
        let previous = super::super::nif_runtime::matrix(placed);
        placement.place(&mut model, 801, 0, &camera)?;
        assert_eq!(
            super::super::nif_runtime::matrix(&model.locals[&root_id]),
            previous
        );
        // The same manager scratch ray is reused for another model too.
        let mut other = resources.instantiate("HUDEffect001")?.unwrap();
        placement.place(&mut other, u32::MAX, 0, &camera)?;
        assert_eq!(
            super::super::nif_runtime::matrix(&other.locals[&other.root]),
            previous
        );
        model.update_direct_transforms(0.)?;
        assert_eq!(
            super::super::nif_runtime::matrix(&model.world[&root_id]),
            previous,
            "Root update did not publish the placed local frame"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_emitter_commands_reach_independent_playback_owners() -> anyhow::Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = super::Resources::load(&root)?;
        for name in ["HUDEffect000", "HUDEffect001"] {
            let mut first = resources.instantiate(name)?.unwrap();
            let second = resources.instantiate(name)?.unwrap();
            let original = second.particles.as_ref().unwrap().emitter_command_state();
            assert!(!original.is_empty());
            let mut reads = 0;
            for _ in 0..2 {
                first.emitters(true, || {
                    reads += 1;
                    Ok(12.25)
                })?;
            }
            let expected = first
                .hierarchy
                .nodes
                .values()
                .map(|node| node.controllers.len())
                .sum::<usize>()
                * 2;
            assert_eq!(reads, expected);
            let identity = [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ];
            first
                .particles
                .as_mut()
                .unwrap()
                .advance_attached(0., false, identity)?;
            assert!(first
                .particles
                .as_ref()
                .unwrap()
                .emitter_command_state()
                .iter()
                .all(|(_, active, _, driven)| *active && *driven));
            let mut mirror = resources.instantiate(name)?.unwrap();
            for _ in 0..2 {
                mirror.emitters(true, || Ok(12.25))?;
            }
            let mut generated = false;
            let mut exported = name != "HUDEffect000";
            for step in 0..120 {
                first
                    .particles
                    .as_mut()
                    .unwrap()
                    .advance_attached(1. / 60., false, identity)?;
                mirror
                    .particles
                    .as_mut()
                    .unwrap()
                    .advance_attached(1. / 60., true, identity)?;
                let count = first.particles.as_ref().unwrap().particle_count();
                assert_eq!(
                    count,
                    mirror.particles.as_ref().unwrap().particle_count(),
                    "Preview flag suppressed native {name} playback"
                );
                if !exported && count > 0 {
                    let mut draws = Vec::new();
                    first.particles.as_ref().unwrap().append(
                        [1., 0., 0.],
                        [0., 1., 0.],
                        &mut draws,
                    )?;
                    let draw = draws
                        .first()
                        .context("Generated health-loss particles have no draw")?;
                    // The authored size curve starts at zero. Capture its
                    // first drawable sample, without changing that curve.
                    if draw.aabb_max[0] > draw.aabb_min[0] || draw.aabb_max[1] > draw.aabb_min[1] {
                        std::fs::write(
                            "target/ui-emitter-playback.mesh.json",
                            serde_json::to_vec(&serde_json::json!({
                        "scope":"Generated original health-loss effect after native START with preview enable=false. Supplied camera/clock, provisional preview RNG and simulation; not gameplay or original-pixel parity.",
                        "preview_sample":step+1,"preview_dt":1./60.,"particle_count":count,
                        "vertices":draw.v_data.as_ref(),"indices":draw.indices.as_ref()}))?,
                        )?;
                        exported = true;
                    }
                }
                generated |= count > 0;
            }
            assert!(generated, "{name} never generated particles");
            assert!(exported, "{name} never produced drawable particle geometry");
            first.emitters(false, || anyhow::bail!("STOP read clock"))?;
            first
                .particles
                .as_mut()
                .unwrap()
                .advance_attached(1. / 60., true, identity)?;
            assert!(first
                .particles
                .as_ref()
                .unwrap()
                .emitter_command_state()
                .iter()
                .all(|(_, active, _, driven)| !active && *driven));
            assert_eq!(
                second.particles.as_ref().unwrap().emitter_command_state(),
                original
            );
        }
        Ok(())
    }
    use super::*;
    #[test]
    fn layer_slots_follow_native_first_free_and_single_tail_removal() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_layer_slots.json"
        ))?;
        for c in fixture["cases"].as_array().unwrap() {
            let mut slots = c["initial"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| match id.as_u64().unwrap() {
                    0 => None,
                    v => Some(v as usize),
                })
                .collect::<Vec<_>>();
            if let Some(index) = c["remove"].as_u64() {
                remove_slot(&mut slots, index as usize);
            } else {
                attach_slot(&mut slots, 9);
            }
            assert_eq!(
                serde_json::json!(slots.iter().map(|id| id.unwrap_or(0)).collect::<Vec<_>>()),
                c["slots"],
                "{c}"
            );
            assert_eq!(
                slots.iter().flatten().count() as u64,
                c["used"].as_u64().unwrap()
            );
        }
        Ok(())
    }
    #[test]
    fn placement_copies_camera_matrix_and_replaces_scale_with_native_point_one() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_model_placement.json"
        ))?;
        for c in fixture["cases"].as_array().unwrap() {
            let vector = |key: &str| {
                std::array::from_fn(|i| f32::from_bits(c[key][i].as_u64().unwrap() as u32))
            };
            let rotation = std::array::from_fn(|r| {
                std::array::from_fn(|col| {
                    f32::from_bits(c["rotation"][r * 3 + col].as_u64().unwrap() as u32)
                })
            });
            let pose = placement(vector("origin"), vector("direction"), rotation)?;
            let output = pose
                .rotation
                .rows
                .into_iter()
                .flatten()
                .chain([
                    pose.translation.x,
                    pose.translation.y,
                    pose.translation.z,
                    pose.scale,
                ])
                .map(f32::to_bits)
                .collect::<Vec<_>>();
            assert_eq!(serde_json::json!(output), c["output"], "{c}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_models_share_templates_but_keep_placement_and_lifecycles_separate() -> Result<()> {
        struct Services {
            initialized: usize,
            released: usize,
            emissions: Vec<bool>,
            fail: bool,
        }
        impl ModelServices for Services {
            fn initialize(&mut self, handle: usize, _: &mut ModelInstance) -> Result<()> {
                ensure!(handle != 0, "No stable clone handle");
                ensure!(!self.fail, "Supplied initialization failure");
                self.initialized += 1;
                Ok(())
            }
            fn emitters(&mut self, _: usize, _: &mut ModelInstance, enabled: bool) -> Result<()> {
                self.emissions.push(enabled);
                Ok(())
            }
            fn release(&mut self, _: usize, _: &mut ModelInstance) -> Result<()> {
                self.released += 1;
                Ok(())
            }
        }
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut runtime = Runtime::load(&root)?;
        let mut services = Services {
            initialized: 0,
            released: 0,
            emissions: vec![],
            fail: false,
        };
        let spawn = |id, layer| Command::Spawn {
            id,
            x: 119.,
            y: 64.,
            layer,
            name: "HUDEffect000".into(),
        };
        for id in 0..5 {
            assert_eq!(
                runtime.command_with_services(&spawn(id, id as i32), &mut services)?,
                Outcome::Spawned
            );
        }
        assert_eq!(services.initialized, 5);
        assert_eq!(runtime.models.instances.len(), 5);
        for i in 0..4 {
            assert_eq!(runtime.models.layer(i)?, [Some(i + 1)]);
        }
        let first = &runtime.models.instances[&1];
        let second = &runtime.models.instances[&2];
        assert!(Arc::ptr_eq(&first.template, &second.template));
        assert!(Arc::ptr_eq(&first.hierarchy, &second.hierarchy));
        let base = second.world.clone();
        let template = first.template.hierarchy.base_world_transforms(None)?;
        runtime.models.instances.get_mut(&1).unwrap().place(
            [2., 3., 4.],
            [1., 0., 0.],
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        )?;
        let first = &runtime.models.instances[&1];
        assert_eq!(first.locals[&first.root].scale.to_bits(), 0x3dcccccd);
        assert_eq!(first.locals[&first.root].translation.x, 3.);
        assert_eq!(
            first.world, base,
            "Placement published world data before the root's update"
        );
        assert_eq!(runtime.models.instances[&2].world, base);
        assert_eq!(
            first.template.hierarchy.base_world_transforms(None)?,
            template
        );
        assert_eq!(
            runtime.command_with_services(&spawn(0, 3), &mut services)?,
            Outcome::Duplicate
        );
        assert_eq!(services.initialized, 5);
        for enabled in [true, true, false, false] {
            runtime.command_with_services(&Command::Emitters { id: 0, enabled }, &mut services)?;
        }
        assert_eq!(services.emissions, [true, true, false, false]);
        assert_eq!(
            runtime.command_with_services(&Command::Despawn { id: 0 }, &mut services)?,
            Outcome::Released
        );
        assert!(runtime.models.layer(0)?.is_empty());
        assert!(!runtime.models.instances.contains_key(&1));
        assert_eq!(services.released, 1);
        runtime.command_with_services(&spawn(10, 0), &mut services)?;
        assert_eq!(runtime.models.layer(0)?, [Some(6)]);
        services.fail = true;
        let count = runtime.models.instances.len();
        assert!(runtime
            .command_with_services(&spawn(11, 0), &mut services)
            .is_err());
        assert_eq!(runtime.models.instances.len(), count);
        assert!(!runtime.state.instances.contains_key(&11));
        // The actual component still has no live services and must fail,
        // leaving no identifier or unattached leaked model behind.
        assert!(runtime.command(&spawn(12, 0)).is_err());
        assert!(!runtime.state.instances.contains_key(&12));
        assert_eq!(runtime.models.instances.len(), count);
        services.fail = false;
        runtime.clear_with_services(&mut services)?;
        assert!(runtime.models.instances.is_empty() && runtime.state.instances.is_empty());
        assert_eq!(services.released, services.initialized);
        std::fs::write(
            "target/ui-model-ownership.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Owned model clone/layer lifecycle with supplied initialization/emitter/release services. Not native controller/clock or renderer admission.",
            "initialized":services.initialized,"released":services.released,"emitter_calls":services.emissions,"instances_before_clear":count,"instances_after_clear":runtime.models.instances.len(),
            "templates_shared":true,"mutable_placement_isolated":true,"native_scale_bits":0x3dcccccdu32,
            "missing_services_fail_without_adding_ids":true,"resources":runtime.resources.summary()}))?,
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_health_effects_prepare_real_default_and_r3mt_materials() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let mut resources = Resources::load(&root)?;
        for name in ["HUDEffect000", "HUDEffect001"] {
            let prepared = resources
                .prepare(name)?
                .context("Shipped health effect unregistered")?;
            ensure!(
                matches!(&prepared.particles, Ok(Some(_))),
                "{name}: {:?}",
                prepared.summary
            );
            if name == "HUDEffect000" {
                let (index, system) = prepared
                    .model
                    .blocks
                    .iter()
                    .enumerate()
                    .find_map(|(i, b)| {
                        if let epic_mickey_lib_rs::nif_file::NiObject::NiPSParticleSystem(p) = b {
                            Some((i, p))
                        } else {
                            None
                        }
                    })
                    .context("No health-loss particle system")?;
                let mut bindings =
                    crate::render_support::nif::find_mesh_surface(&prepared.model, index).material;
                bindings.has_vertex_colors = system.flag1 != 0;
                bindings.textures.retain(|t| t.shader_id.is_none());
                let program = crate::render_support::wii_default::compile_particle(&bindings)?;
                let texture = bindings
                    .textures
                    .first()
                    .context("No standard health-loss base map")?;
                let file =
                    crate::render_support::nif::find_pak_texture(&prepared.dependencies, &texture.path)
                        .context("Health-loss base texture unavailable")?;
                let (rgba, width, height) = crate::render_support::nif::decode_image_bytes(&file.data)
                    .context("Health-loss texture decode failed")?;
                // A GPU material demonstration, with supplied white vertex
                // color; this is not native emitter/clock or gameplay playback.
                std::fs::write(
                    "target/wii-default-health-loss.vert",
                    program.vertex.as_bytes(),
                )?;
                std::fs::write(
                    "target/wii-default-health-loss.frag",
                    program.fragment.as_bytes(),
                )?;
                std::fs::write("target/wii-default-health-loss.rgba", rgba)?;
                std::fs::write(
                    "target/wii-default-health-loss.json",
                    serde_json::to_vec_pretty(&serde_json::json!({
                    "scope":"Unanimated material demonstration, original texture and compiled native default program, supplied white particle color. Not an original-game parity or gameplay capture.",
                    "model":prepared.summary["path"],"texture":file.path,"width":width,"height":height,"vertex_colors":bindings.has_vertex_colors,
                    "blend":program.blend,"depth_test":program.depth_test,"depth_write":program.depth_write,"depth_func":program.depth_func,"cull":program.cull,
                    "sampler":{"min_filter":texture.sampler.min_filter,"mag_filter":texture.sampler.mag_filter,"wrap_u":texture.sampler.wrap_u,"wrap_v":texture.sampler.wrap_v}}))?,
                )?;
            }
        }
        ensure!(
            resources.prepare("HUDEffect002")?.is_none(),
            "Empty native slot became a resource"
        );
        std::fs::write(
            "target/wii-default-owned-materials.json",
            serde_json::to_vec_pretty(&resources.summary())?,
        )?;
        Ok(())
    }
    #[test]
    fn ownership_matches_original_hash_lookup_and_lifecycle() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_effect_ownership.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            struct B {
                resource: bool,
                calls: Vec<String>,
            }
            impl Backend for B {
                type Handle = u32;
                fn clone_effect(&mut self, name: &str, layer: i32) -> Result<Option<u32>> {
                    self.calls.push(format!("clone {name} layer {layer}"));
                    Ok(self.resource.then_some(100))
                }
                fn emitters(&mut self, _: u32, enabled: bool) -> Result<()> {
                    self.calls.push(format!(
                        "emitters {}",
                        if enabled { "True" } else { "False" }
                    ));
                    Ok(())
                }
                fn release(&mut self, handle: u32) -> Result<()> {
                    self.calls.push(format!("release {handle}"));
                    Ok(())
                }
            }
            let mut state = State::default();
            for row in case["initial"].as_array().unwrap() {
                state.insert(
                    row["id"].as_u64().unwrap() as u32,
                    Instance {
                        handle: row["handle"].as_u64().unwrap() as u32,
                        x: row["x"].as_u64().unwrap() as u32,
                        y: row["y"].as_u64().unwrap() as u32,
                    },
                );
            }
            let id = case["id"].as_u64().unwrap() as u32;
            let command = match case["operation"].as_str().unwrap() {
                "spawn" => Command::Spawn {
                    id,
                    x: 119.2966,
                    y: 64.6,
                    layer: 1,
                    name: "HUDEffect000".into(),
                },
                "update" => Command::Update {
                    id,
                    x: 119.2966,
                    y: 64.6,
                },
                "despawn" => Command::Despawn { id },
                "start" => Command::Emitters { id, enabled: true },
                "stop" => Command::Emitters { id, enabled: false },
                _ => unreachable!(),
            };
            let mut backend = B {
                resource: case["resource"] == true,
                calls: Vec::new(),
            };
            state.command(&command, &mut backend)?;
            let rows = state
                .instances
                .iter()
                .map(|(id, r)| serde_json::json!({"id":id,"handle":r.handle,"x":r.x,"y":r.y}))
                .collect::<Vec<_>>();
            assert_eq!(serde_json::json!(rows), case["instances"]);
            assert_eq!(serde_json::json!(backend.calls), case["calls"]);
            assert_eq!(
                state.instances.len() as u64,
                case["count"].as_u64().unwrap()
            );
            assert_eq!(state.native_order().count(), state.instances.len());
            assert_eq!(
                serde_json::json!(state.native_order().map(|(id, _)| id).collect::<Vec<_>>()),
                case["native_order"]
            );
        }
        Ok(())
    }
    #[test]
    fn projection_matches_original_platform_branches_and_float_stores() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_effect_projection.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let f = std::array::from_fn(|i| {
                f32::from_bits(case["frustum_bits"][i].as_u64().unwrap() as u32)
            });
            let v = std::array::from_fn(|i| {
                f32::from_bits(case["viewport_bits"][i].as_u64().unwrap() as u32)
            });
            let p = projection(
                f,
                v,
                case["y"].as_u64().unwrap() as u32,
                case["format"].as_u64().unwrap() as u32,
                case["wide"] == true,
            );
            assert_eq!(
                serde_json::json!(p.frustum.map(f32::to_bits)),
                case["output_frustum_bits"]
            );
            assert_eq!(
                serde_json::json!(p.viewport.map(f32::to_bits)),
                case["output_viewport_bits"]
            );
            assert_eq!(p.y as u64, case["output_y"].as_u64().unwrap());
            assert!(p.orthographic);
        }
        Ok(())
    }
    #[test]
    fn commands_and_config_keep_layer_gaps_and_missing_resources() -> Result<()> {
        let Command::Spawn {
            id,
            x,
            y,
            layer,
            name,
        } = parse("SPAWN_EFFECT", "-1 119.2966 64.6 1 HUDEffect000 ignored")?
        else {
            unreachable!()
        };
        assert_eq!(
            (id, pixel(x), pixel(y), layer, name.as_str()),
            (u32::MAX, 119, 64, 1, "HUDEffect000")
        );
        let values=crate::render_support::game_config::runtime_settings("HUDEffectCount=4\nHUDEffect000=off.nif\nHUDEffect001=gain.nif\n;HUDEffect002=disabled.nif\nHUDEffect003=ping.nif\n",false);
        let catalog = Catalog::hud(&values)?;
        assert_eq!(catalog.disabled, ["HUDEffect002"]);
        assert_eq!(catalog.paths.len(), 3);
        assert!(parse("SPAWN_EFFECT", "1 2 3 1").is_err());
        assert!(parse("UPDATE_EFFECT", "1 NaN 2").is_err());
        Ok(())
    }
}
