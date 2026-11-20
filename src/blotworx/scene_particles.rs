//! Particle controllers serialized directly into a scene, rather than spawned
//! through FXComponent. Shares the authored particle decoder with named FX.
use super::{assets::World, brush_particles::Hose, resource_store::Store};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

fn is_player_drip(components: &[epic_mickey_lib_rs::scene_file::Component]) -> bool {
    components.iter().filter(|c| c.class_name == "InputOutputComponent")
        .flat_map(|c| &c.properties).any(|p| p.name == "EntityGroups"
            && p.value.as_array().is_some_and(|groups| groups.iter().any(|g|
                g.as_str().is_some_and(|name| name.eq_ignore_ascii_case("MK_drippy_effects")))))
}

pub(super) fn player_drip(components: &[epic_mickey_lib_rs::scene_file::Component]) -> bool {
    is_player_drip(components)
}

pub struct Instance {
    pub particles: Hose,
    pub enabled: bool,
}
pub struct DripRibbon {
    pub ribbon: super::brush_particles::Ribbon,
    pub enabled: bool,
    restart: [f32; 2],
    remaining: f32,
}
pub struct Runtime {
    pub drips: BTreeMap<usize, DripRibbon>,
    drip_models: BTreeMap<String, super::brush_particles::Ribbon>,
    resources: Store,
    prototypes: BTreeMap<String, Result<Option<Hose>, String>>,
    pub live: BTreeMap<usize, Instance>,
    seen: usize,
    pub errors: BTreeMap<usize, String>,
    empty: std::collections::BTreeSet<usize>,
    pub ribbon_only: std::collections::BTreeSet<usize>,
    ribbon_models: std::collections::BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_scene_steam_emits_and_script_stop_preserves_visibility() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        anyhow::ensure!(!world.demo_story.enabled,"Emitter regression must run without the demo adapter");
        anyhow::ensure!(!world.scripts.errors.iter().any(|error|error.contains("StopEmitters") || error.contains("StartEmitters")),
            "Authored startup emitter command failed: {:?}",world.scripts.errors);
        let startup_jet=world.scripts.resolve_name("gv_zonea_airlock.steam+marker_rotatable 01.SteamJet_Blast")
            .context("Original startup steam jet absent")?;
        anyhow::ensure!(world.demo_story.scene.emitter_requests.get(&startup_jet)==Some(&false),
            "OnSpawned did not stop its original steam jet before demo/story initialization");
        world.admit_scene_particles();
        let jets = world
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name.contains(".SteamJet_Blast"))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert!(!jets.is_empty());
        for entity in jets {
            anyhow::ensure!(
                world.scene_particles.live.contains_key(&entity),
                "Steam jet failed: {:?}",
                world.scene_particles.errors.get(&entity)
            );
        }
        let name = "gremlinvillage_zonea.Common_Steam_01 02";
        let entity = world
            .entities
            .iter()
            .position(|e| e.name == name)
            .context("Original steam absent")?;
        anyhow::ensure!(
            world.scene_particles.live.contains_key(&entity),
            "Steam did not load: {:?}",
            world.scene_particles.errors
        );
        world.run_callback_fixture(&format!("StartEmitters(GetEntity('{name}'))"))?;
        for _ in 0..60 {
            world.advance_scene_particles(super::super::STEP)?;
        }
        anyhow::ensure!(
            world.scene_particles.live[&entity]
                .particles
                .particle_count()
                > 0,
            "Original steam produced no particles"
        );
        let hidden = world.hidden_entities.contains(&entity);
        world.run_callback_fixture(&format!("StopEmitters(GetEntity('{name}'))"))?;
        assert!(!world.scene_particles.live[&entity].enabled);
        assert_eq!(
            world.hidden_entities.contains(&entity),
            hidden,
            "Stopping emission must not hide existing particles"
        );
        for _ in 0..300 {
            world.advance_scene_particles(super::super::STEP)?;
        }
        assert_eq!(
            world.scene_particles.live[&entity]
                .particles
                .particle_count(),
            0
        );
        world.run_callback_fixture(&format!("StartEmitters(GetEntity('{name}'))"))?;
        for _ in 0..60 {
            world.advance_scene_particles(super::super::STEP)?;
        }
        assert!(
            world.scene_particles.live[&entity]
                .particles
                .particle_count()
                > 0
        );
        println!("{}", world.scene_particles.summary());
        Ok(())
    }
}
impl Runtime {
    pub(super) fn admit_packs(&mut self,packs:&[String])->Result<()> {
        if self.resources.admit_packs(packs)?>0 {self.prototypes.retain(|_,prototype|prototype.is_ok());}
        Ok(())
    }
    pub fn new(root: &std::path::Path, packs: &[String]) -> Result<Self> {
        Ok(Self {
            drips: BTreeMap::new(),
            drip_models: BTreeMap::new(),
            resources: Store::new(root, packs)?,
            prototypes: BTreeMap::new(),
            live: BTreeMap::new(),
            seen: 0,
            errors: BTreeMap::new(),
            empty: Default::default(),
            ribbon_only: Default::default(),
            ribbon_models: Default::default(),
        })
    }
    pub fn set_enabled(&mut self, entity: usize, enabled: bool) -> Result<bool> {
        if let Some(error) = self.errors.get(&entity) {
            anyhow::bail!("Scene emitter: {error}");
        }
        if let Some(instance) = self.drips.get_mut(&entity) {
            instance.enabled = enabled;
            return Ok(true);
        }
        if let Some(instance) = self.live.get_mut(&entity) {
            instance.enabled = enabled;
            return Ok(true);
        }
        Ok(self.empty.contains(&entity))
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"instances":self.live.iter().map(|(entity,v)|serde_json::json!({"entity":entity,"enabled":v.enabled,"particles":v.particles.particle_count(),"constant_colors":v.particles.color_summary()})).collect::<Vec<_>>(),
            "drips":self.drips.iter().map(|(entity,v)|serde_json::json!({"entity":entity,"enabled":v.enabled,"particles":v.ribbon.particle_count(),"render":v.ribbon.render_probe()})).collect::<Vec<_>>(),
            "errors":self.errors,"ribbon_only_entities":self.ribbon_only,"unique_models":self.prototypes.len(),"resources":self.resources.summary(),
            "native_particle_parity_validated":false})
    }
}
impl World {
    pub(super) fn admit_scene_particles(&mut self) {
        let runtime = &mut self.scene_particles;
        for (entity, source) in self.entities.iter().enumerate().skip(runtime.seen) {
            if self.named_fx.live.contains_key(&entity) {
                continue;
            }
            let controller = source
                .components
                .iter()
                .find(|c| c.class_name == "ParticleSystemControllerComponent");
            // Animated NIFs can own particle systems without a separate
            // particle-controller component (the airlock steam does this).
            if controller.is_none() && !source.components.iter().any(|c| c.class_name == "AnimationComponentGB") {
                continue;
            }
            let result = (|| -> Result<Option<Instance>> {
                let Some(model) = source.model_asset.as_ref() else {
                    return Ok(None);
                };
                if !runtime.prototypes.contains_key(model) {
                    let mut ribbon_only = false;
                    let loaded = (|| -> Result<Option<Hose>> {
                        let file = runtime.resources.read(model)?;
                        let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
                            .map_err(anyhow::Error::msg)?;
                        let has_hose = nif.block_types.iter().any(|kind| {
                            kind.contains("ParticleSystem") && kind != "RibbonParticleSystem"
                        });
                        ribbon_only = nif
                            .block_types
                            .iter()
                            .any(|kind| kind == "RibbonParticleSystem");
                        if !has_hose {
                            if ribbon_only && is_player_drip(&source.components) {
                                let dependencies = runtime.resources.particle_files(&file.data)?;
                                runtime.drip_models.insert(model.clone(), super::brush_particles::Ribbon::load_drip(&file.data, &dependencies)?);
                            }
                            return Ok(None);
                        }
                        let dependencies = runtime.resources.particle_files(&file.data)?;
                        Ok(Some(Hose::load_attached_for_demo(
                            &file.data,
                            &dependencies,
                        )?))
                    })()
                    .map_err(|e| format!("{e:#}"));
                    if ribbon_only {
                        runtime.ribbon_models.insert(model.clone());
                    }
                    runtime.prototypes.insert(model.clone(), loaded);
                }
                let prototype = runtime.prototypes[model]
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let Some(prototype) = prototype else {
                    if let Some(ribbon) = runtime.drip_models.get(model) {
                        let enabled = self.demo_story.scene.emitter_requests.get(&entity).copied().unwrap_or(true);
                        let interval = |name: &str| controller.and_then(|c| c.properties.iter().find(|p|p.name==name))
                            .and_then(|p|p.value.as_f64()).unwrap_or(0.) as f32;
                        let restart = [interval("RestartTimeMin"), interval("RestartTimeMax")];
                        let remaining = restart[0] + (restart[1] - restart[0]) * self.game_random.draw() as f32 / 32767.;
                        runtime.drips.insert(entity, DripRibbon { ribbon: ribbon.clone(), enabled, restart, remaining });
                    }
                    return Ok(None);
                };
                let enabled = controller.map(|controller| controller
                    .properties
                    .iter()
                    .find(|p| p.name == "StartEmittersEnabled")
                    .and_then(|p| p.value.as_bool())
                    .context("Particle controller lacks StartEmittersEnabled")).transpose()?.unwrap_or(true);
                let mut particles = prototype.instantiate()?;
                // Scene component registers override the NIF's defaults. In particular,
                // Mickey's nozzle supplies transparent constant color 1.
                if let Some(effects) = source.components.iter().find(|c| c.class_name == "R3MTEffectsComponent") {
                    for slot in 0..4 {
                        let key = format!("R3MTConstantColor_{slot}");
                        if effects.properties.iter().any(|p| p.name == format!("{key}_Enable") && p.value.as_bool() == Some(true)) {
                            let value = &effects.properties.iter().find(|p| p.name == key)
                                .context("Enabled particle color has no value")?.value;
                            let mut color = [0.; 4];
                            for (index, channel) in ["r", "g", "b", "a"].iter().enumerate() {
                                color[index] = value.get(*channel).and_then(|v| v.as_f64())
                                    .context("Invalid particle component color")? as f32;
                            }
                            particles.set_subgraph_constant(None, slot, color)?;
                        }
                    }
                }
                let enabled = self.demo_story.scene.emitter_requests.get(&entity).copied()
                    .unwrap_or(enabled || is_player_drip(&source.components));
                Ok(Some(Instance { particles, enabled }))
            })();
            match result {
                Ok(Some(instance)) => {
                    runtime.ribbon_only.remove(&entity);
                    runtime.live.insert(entity, instance);
                }
                Ok(None) => {
                    runtime.empty.insert(entity);
                    if self.entities[entity]
                        .model_asset
                        .as_ref()
                        .is_some_and(|model| runtime.ribbon_models.contains(model))
                    {
                        runtime.ribbon_only.insert(entity);
                    }
                }
                Err(error) => {
                    let message = format!("{}: {error:#}", source.name);
                    self.report
                        .unsupported
                        .push(format!("Scene particle playback: {message}"));
                    runtime.errors.insert(entity, message);
                }
            }
        }
        runtime.seen = self.entities.len();
    }
    pub(super) fn advance_scene_particles(&mut self, dt: f32) -> Result<()> {
        self.admit_scene_particles();
        let active = self
            .scene_particles
            .live
            .keys()
            .copied()
            .filter(|&entity| self.entity_scene_active(entity))
            .collect::<std::collections::BTreeSet<_>>();
        for (&entity, instance) in &mut self.scene_particles.live {
            if self.removed_entities.contains(&entity) || !active.contains(&entity) {
                continue;
            }
            instance
                .particles
                .advance_attached(
                    dt,
                    instance.enabled && !self.hidden_entities.contains(&entity),
                    self.entities[entity].transform,
                )
                .with_context(|| format!("Scene particles {}", self.entities[entity].name))?;
        }
        let active_drips = self.scene_particles.drips.keys().copied()
            .filter(|&entity| self.entity_scene_active(entity)).collect::<std::collections::BTreeSet<_>>();
        for (&entity, drip) in &mut self.scene_particles.drips {
            if self.removed_entities.contains(&entity) || !active_drips.contains(&entity) { continue; }
            if let Some(animation) = self.model_animation.get(&entity) {
                drip.ribbon.update_emitter_pose(&animation.world)?;
            }
            if drip.enabled && !self.hidden_entities.contains(&entity) && drip.restart[1] > 0. {
                drip.remaining -= dt;
                if drip.remaining <= 0. {
                    drip.ribbon.restart();
                    drip.remaining = drip.restart[0] + (drip.restart[1] - drip.restart[0]) * self.game_random.draw() as f32 / 32767.;
                }
            }
            drip.ribbon.advance(dt, drip.enabled && !self.hidden_entities.contains(&entity), self.entities[entity].transform)
                .with_context(|| format!("Drip ribbon {}", self.entities[entity].name))?;
        }
        self.scene_particles.drips.retain(|entity, _| !self.removed_entities.contains(entity));
        self.scene_particles
            .live
            .retain(|entity, _| !self.removed_entities.contains(entity));
        Ok(())
    }
}
