//! Authored placed actors use the same character graph/runtime as spawned
//! actors. AI/controller startup phases remain separate from graph playback.
use super::{
    assets::World,
    sketch_assets::{Actor, Animation},
};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
use std::{collections::BTreeMap, sync::Arc};
type V = [f32; 3];
struct Mesh {
    block: usize,
    positions: Vec<V>,
    normals: Vec<V>,
    sources: Vec<u32>,
    transform: [[f32; 4]; 4],
    vertices: Arc<Vec<f32>>,
}
struct Instance {
    animation: (String, String),
    model: String,
    actor: Actor,
    skin_frame: std::cell::RefCell<SkinFrame>,
}

#[derive(Default)]
struct SkinFrame {
    pose: Vec<[f32; 16]>,
    meshes: Vec<super::clip_preview::SkinnedMesh>,
}
#[derive(Default)]
pub struct Runtime {
    animations: BTreeMap<(String, String), Animation>,
    models: BTreeMap<String, Vec<Mesh>>,
    instances: BTreeMap<usize, Instance>,
    event_maps: BTreeMap<(String, String), BTreeMap<u32, i32>>,
    export_maps: BTreeMap<(String, String), BTreeMap<i32, u32>>,
    wii_source: Option<Arc<super::streamed_animation_resources::WiiClips>>,
}

impl Runtime {
    pub fn with_wii_source(root: &std::path::Path) -> Self {
        Self {
            wii_source: Some(Arc::new(super::streamed_animation_resources::WiiClips::new(root))),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_clocktower_watch_initializes_from_stripped_character_and_authored_default(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/MeanStreet_V1.level",
            &egui::Context::default(),
        )?;
        let entity = world
            .entities
            .iter()
            .position(|entity| entity.name == "hub_meanstreet.ClockTowerWatch")
            .context("Missing original watch")?;
        let instance = world
            .authored_havok
            .instances
            .get(&entity)
            .with_context(|| {
                format!(
                    "Watch did not initialize: {:?}",
                    world
                        .report
                        .unsupported
                        .iter()
                        .filter(|error| error.starts_with("Authored Havok actor"))
                        .collect::<Vec<_>>()
                )
            })?;
        let animation = &world.authored_havok.animations[&instance.animation];
        ensure!(
            animation.contract.default_character_selected,
            "Expected unnamed single-character source"
        );
        ensure!(
            animation
                .contract
                .character
                .to_ascii_lowercase()
                .ends_with("/characters/watch.hkx"),
            "Wrong authored character"
        );
        ensure!(
            animation.contract.clips.len() == 4,
            "Original watch clips were omitted"
        );
        ensure!(
            animation.contract.clips_outside_character_table == ["watch_tpose.hkx"],
            "Lost authored table discrepancy"
        );
        ensure!(
            !instance.actor.playback.pose.is_empty(),
            "Watch has no skeletal pose"
        );
        for _ in 0..30 {
            let errors = world.authored_havok.advance();
            ensure!(errors.is_empty(), "Actor playback failed: {errors:?}");
        }
        ensure!(
            world.authored_havok.contains(entity),
            "Watch was discarded during update"
        );
        let mut draws = world.draws.clone();
        world.authored_havok.apply(&world, &mut draws)?;
        ensure!(
            world.entities[entity]
                .draws
                .clone()
                .all(|draw| draws[draw].v_data.iter().all(|value| value.is_finite())),
            "Watch produced invalid rendering data"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_shared_mickey_assets_survive_player_resource_initialization() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/MeanStreet_V1.level",
            &egui::Context::default(),
        )?;
        let shared = world
            .entities
            .iter()
            .enumerate()
            .filter(|(index, entity)| {
                *index != world.player_entity
                    && entity.components.iter().any(|component| {
                        component.class_name == "AnimationComponent"
                            && component.properties.iter().any(|property| {
                                property.name == "CharacterName"
                                    && property.value.as_str() == Some("Mickey")
                            })
                    })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        ensure!(
            !shared.is_empty(),
            "Expected retail actors sharing Mickey's animation sources"
        );
        for &entity in &shared {
            ensure!(
                world.authored_havok.contains(entity),
                "Shared-source actor {} failed to initialize: {:?}",
                world.entities[entity].name,
                world
                    .report
                    .unsupported
                    .iter()
                    .filter(|error| error.starts_with("Authored Havok actor"))
                    .collect::<Vec<_>>()
            );
            let pose = world.authored_havok.bone_pose(entity, "SKEL_weapon")?;
            ensure!(
                pose.iter().all(|value| value.is_finite()),
                "Invalid shared actor bone"
            );
        }
        let mut preview = super::super::authored_preview::Preview::new(&mut world)?;
        preview.advance(&mut world, 1)?;
        ensure!(
            world
                .player_animations
                .clip("Mickey_mov_run_brush.hkx")?
                .info
                .num_tracks
                > 0,
            "Preparing shared actors damaged the player's lazy animation payload"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files and writes a compact diagnostic pose"]
    fn original_spatter_melee_02_live_pose_diagnostic() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let entity = world
            .entities
            .iter()
            .position(|entity| entity.name == "gremlinvillage_zonea.SpatterMelee 02")
            .context("Exact captured Spatter missing")?;
        world.enable_demo_abilities()?;
        world.recover_failed_preview_script(0);
        world.attach_player_body()?;
        let mut player = world.take_startup_player();
        world.advance_player_models(&mut player, super::super::STEP)?;
        let instance = world
            .authored_havok
            .instances
            .get(&entity)
            .context("Exact actor not initialized")?;
        let animation = &world.authored_havok.animations[&instance.animation];
        let bones=instance.actor.playback.pose.iter().zip(&animation.resources.rig.bones).enumerate().map(|(index,(pose,reference))|
            serde_json::json!({"index":index,"name":pose.name,"parent":pose.parent_index,
                "translation":pose.local_translation,"rotation":pose.local_rotation,"scale":pose.local_scale,
                "reference_translation":reference.local_translation,"reference_rotation":reference.local_rotation,
                "reference_scale":reference.local_scale})).collect::<Vec<_>>();
        let mut draws = world.draws.clone();
        world.apply_model_poses(&mut draws)?;
        let clips = instance
            .actor
            .prepared
            .plan
            .nodes
            .iter()
            .filter_map(|node| {
                instance
                    .actor
                    .prepared
                    .state
                    .clips
                    .get(&node.object)
                    .map(|definition| {
                        (
                            node.object.clone(),
                            definition.settings.animation_name.clone(),
                        )
                    })
            })
            .map(|(node, name)| {
                Ok((
                    name,
                    instance.actor.prepared.clips.get(&node)?.clock.local_time,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let identity = instance.animation.clone();
        let bone_count = animation.resources.rig.bones.len();
        let mut report = serde_json::json!({"scope":"Original assets; exact actor; one playable update; no original-runtime parity claim",
            "identity":instance.animation,"rig":animation.contract.rig,"bones":bones,
            "geometry":super::super::render_probe::entity_geometry(&world,&draws,&world.entities[entity].name)});
        let animation = world.authored_havok.animations.get_mut(&identity).unwrap();
        let mut clip_reports = Vec::new();
        for (name, time) in clips {
            let tracks = animation
                .resources
                .clip(&name)?
                .sample_tracks(bone_count, time)?;
            clip_reports.push(serde_json::json!({"name":name,"time":time,
                "upper_arm_scales":[tracks[27].as_ref().map(|pose|pose.scale),tracks[32].as_ref().map(|pose|pose.scale)]}));
        }
        report["active_clips"] = serde_json::json!(clip_reports);
        std::fs::write(
            "target/spatter-exact-pose.json",
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_actors_export_events_during_world_ticks_without_diagnostic_commands(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.scripts.ensure_global_module("@observe-original-animation-source",
            b"assert(type(HideGremlinWrench)=='function'); BwxAuthoredLuaMarkers=0; local original=HideGremlinWrench; function HideGremlinWrench(...) BwxAuthoredLuaMarkers=BwxAuthoredLuaMarkers+1; return original(...) end")?;
        let mut player =
            super::super::physics::Player::new(world.report.spawn, world.report.motion.clone());
        let initial = world.authored_havok.len();
        for tick in 1..=210 {
            let since = world.report.unsupported.len();
            world.advance_player_models(&mut player, tick as f32 * super::super::STEP)?;
            ensure!(
                !world.report.unsupported[since..]
                    .iter()
                    .any(|line| line.starts_with("Authored Havok actor")),
                "Actor graph/export failure at tick {tick}: {:?}",
                &world.report.unsupported[since..]
            );
        }
        ensure!(
            world.authored_havok.len() >= initial,
            "Automatic export retired a required original actor"
        );
        ensure!(
            world.controller_feedback.exported_events > 0,
            "Original gameplay actors emitted no mapped controller events"
        );
        world.scripts.ensure_global_module(
            "@verify-original-animation-source",
            b"assert(BwxAuthoredLuaMarkers>0, 'Original animation source never ran')",
        )?;
        ensure!(
            !world
                .report
                .unsupported
                .iter()
                .any(|message| message.contains("animation export 539:")),
            "Original Run_Lua export failed: {:?}",
            world.report.unsupported
        );
        println!(
            "Original authored actor exports {}, noise requests {}",
            world.controller_feedback.exported_events, world.controller_feedback.noise_requests
        );
        for message in &world.report.unsupported {
            if message.contains(" animation export ")
                || message.contains("no mapped native registration")
            {
                println!("{message}");
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn native_force_idle_reaches_original_actor_immediately_and_filters_unmapped_events(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let entity = world
            .authored_havok
            .instances
            .iter()
            .find(|(_, instance)| {
                instance.animation.1 == "Gremlin"
                    && world.authored_havok.event_maps[&instance.animation].contains_key(&0xc2)
            })
            .map(|(&entity, _)| entity)
            .context("Original Gremlin lacks Force_Idle reverse mapping")?;
        let instance = &world.authored_havok.instances[&entity];
        let local = world.authored_havok.event_maps[&instance.animation][&0xc2];
        assert_ne!(
            local, 0xc2,
            "Fixture must exercise different native/local IDs"
        );
        let animation = &world.authored_havok.animations[&instance.animation];
        assert_eq!(
            animation.contract.events[local as usize],
            "EVENT_Force_Idle"
        );
        let queued = instance.actor.playback.pending_events.len();
        let delivered = instance.actor.playback.delivered_events.len();
        let unhandled = (0..543)
            .find(|id| !world.authored_havok.event_maps[&instance.animation].contains_key(id))
            .context("Fixture must include a known but unhandled native event")?;
        assert!(world.send_native_actor_animation_event(entity, 0xc2)?);
        let actor = &world.authored_havok.instances[&entity].actor;
        assert_eq!(actor.playback.pending_events.len(), queued);
        assert_eq!(actor.playback.delivered_events.len(), delivered + 1);
        assert_eq!(actor.playback.delivered_events.last().unwrap().id, local);
        assert!(!world.send_native_actor_animation_event(entity, unhandled)?);
        assert!(world
            .send_native_actor_animation_event(entity, 543)
            .is_err());
        assert_eq!(
            world.authored_havok.instances[&entity]
                .actor
                .playback
                .delivered_events
                .len(),
            delivered + 1
        );
        for _ in 0..30 {
            ensure!(
                world.authored_havok.advance().is_empty(),
                "Native event broke actor update"
            );
        }
        let mut draws = world.draws.clone();
        world.authored_havok.apply(&world, &mut draws)?;
        ensure!(
            world.entities[entity]
                .draws
                .clone()
                .any(|draw| draws[draw].v_data != world.draws[draw].v_data),
            "Native actor event pose never reached render vertices"
        );
        // The diagnostic Lua path uses the same synchronous world bridge.
        let name = serde_json::to_string(&world.entities[entity].name)?;
        world.run_script(&format!(
            "assert(BwxNativeAnimationEvent(GetEntity({name}),194))"
        ))?;
        world.advance_models(world.animation_time)?;
        ensure!(
            !world
                .report
                .unsupported
                .iter()
                .any(|line| line.contains("BwxNativeAnimationEvent")),
            "Native event diagnostic did not reach the original actor"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_spatter_graph_initializes_advances_and_deforms_placed_draws() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        ensure!(
            !world
                .report
                .unsupported
                .iter()
                .any(|error| error.starts_with("Authored Havok actor")
                    && error.contains("Compiling modifier expression")),
            "Original GV actor expressions failed: {:?}",
            world
                .report
                .unsupported
                .iter()
                .filter(|error| error.starts_with("Authored Havok actor"))
                .collect::<Vec<_>>()
        );
        ensure!(
            world
                .authored_havok
                .instances
                .values()
                .any(|instance| instance.animation.1 == "Gremlin"),
            "Original Gremlin did not initialize: {:?}",
            world
                .report
                .unsupported
                .iter()
                .filter(|error| error.starts_with("Authored Havok actor"))
                .collect::<Vec<_>>()
        );
        let entity = world
            .authored_havok
            .instances
            .iter()
            .find(|(_, instance)| instance.animation.1 == "Spatter")
            .map(|(&entity, _)| entity)
            .with_context(|| {
                format!(
                    "Spatter graph initialization failed: {:?}",
                    world
                        .report
                        .unsupported
                        .iter()
                        .filter(|error| error.starts_with("Authored Havok actor"))
                        .collect::<Vec<_>>()
                )
            })?;
        let sample = |runtime: &Runtime| {
            runtime.instances[&entity]
                .actor
                .playback
                .pose
                .iter()
                .map(|bone| {
                    (
                        bone.local_translation,
                        bone.local_rotation,
                        bone.local_scale,
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = sample(&world.authored_havok);
        let gremlin = world
            .authored_havok
            .instances
            .iter()
            .find(|(_, i)| i.animation.1 == "Gremlin")
            .unwrap()
            .0;
        let gremlin = *gremlin;
        let gremlin_before = world.authored_havok.instances[&gremlin]
            .actor
            .playback
            .pose
            .iter()
            .map(|bone| {
                (
                    bone.local_translation,
                    bone.local_rotation,
                    bone.local_scale,
                )
            })
            .collect::<Vec<_>>();
        let mut visited_modifier_list = false;
        let key = world.authored_havok.instances[&entity].animation.clone();
        let event = world.authored_havok.animations[&key]
            .contract
            .events
            .iter()
            .find(|name| !name.is_empty())
            .context("Original actor has no named events")?
            .clone();
        let queued = world.authored_havok.instances[&entity]
            .actor
            .playback
            .pending_events
            .len();
        world.send_havok_actor_event(entity, &event)?;
        assert_eq!(
            world.authored_havok.instances[&entity]
                .actor
                .playback
                .pending_events
                .len(),
            queued + 1
        );
        for tick in 0..210 {
            // Exercise authored Lua/IO deliveries as the playable tick does,
            // rather than advancing isolated character graphs only.
            let since = world.report.unsupported.len();
            world.advance_models(tick as f32 * super::super::STEP)?;
            ensure!(
                !world.report.unsupported[since..]
                    .iter()
                    .any(|error| error.starts_with("Authored Havok actor")),
                "Authored actor event failed at tick {tick}: {:?}",
                &world.report.unsupported[since..]
            );
            let errors = world.authored_havok.advance();
            ensure!(
                errors.is_empty(),
                "Original actor graph update failed at tick {tick}: {errors:?}"
            );
            visited_modifier_list |= world.authored_havok.instances.values().any(|instance| {
                let graph = &world.authored_havok.animations[&instance.animation].graph;
                instance.actor.prepared.plan.nodes.iter().any(|node| {
                    node.effect.is_none() && graph.objects[&node.object].klass == "hkbModifierList"
                })
            });
        }
        ensure!(
            before != sample(&world.authored_havok),
            "Original placed Spatter remained in a static pose"
        );
        ensure!(
            gremlin_before
                != world.authored_havok.instances[&gremlin]
                    .actor
                    .playback
                    .pose
                    .iter()
                    .map(|bone| (
                        bone.local_translation,
                        bone.local_rotation,
                        bone.local_scale
                    ))
                    .collect::<Vec<_>>(),
            "Original placed Gremlin remained in a static pose"
        );
        ensure!(
            visited_modifier_list,
            "Original fixture never exercised modifier-list gameplay"
        );
        let mut draws = world.draws.clone();
        world.authored_havok.apply(&world, &mut draws)?;
        ensure!(
            world.entities[entity]
                .draws
                .clone()
                .any(|draw| draws[draw].v_data != world.draws[draw].v_data),
            "Spatter pose did not reach rendered vertices"
        );
        Ok(())
    }
}
fn key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}
impl Runtime {
    /// Model skin payloads needed by live authored-actor instances. Import
    /// also decodes skin blocks for static scene models, which this runtime
    /// never reads and should release after actor initialization.
    pub(super) fn skin_model_names(&self) -> std::collections::HashSet<String> {
        self.instances
            .values()
            .map(|instance| instance.model.clone())
            .collect()
    }
    pub(super) fn skin_model_for_entity(&self, entity: usize) -> Option<&str> {
        self.instances
            .get(&entity)
            .map(|instance| instance.model.as_str())
    }
    pub(super) fn cached_model_payload_bytes(&self, model: &str) -> usize {
        self.models.get(model).map_or(0, |meshes| {
            meshes
                .iter()
                .map(|mesh| {
                    mesh.positions.capacity() * std::mem::size_of::<V>()
                        + mesh.normals.capacity() * std::mem::size_of::<V>()
                        + mesh.sources.capacity() * std::mem::size_of::<u32>()
                        + mesh.vertices.capacity() * std::mem::size_of::<f32>()
                })
                .sum()
        })
    }

    pub(super) fn cached_model_payload_summary(&self) -> (usize, usize) {
        let bytes = self
            .models
            .keys()
            .map(|model| self.cached_model_payload_bytes(model))
            .sum();
        (self.models.len(), bytes)
    }

    pub(super) fn animation_payload_summary(&self) -> serde_json::Value {
        let mut clips = 0usize;
        let mut packed_clip_bytes = 0usize;
        let mut source_clip_bytes = 0usize;
        let mut decoded_clips = 0usize;
        let mut graph_objects = 0usize;
        for animation in self.animations.values() {
            graph_objects = graph_objects.saturating_add(animation.graph.objects.len());
            let summary = animation.resources.memory_summary();
            clips = clips.saturating_add(
                summary["clip_count"].as_u64().unwrap_or_default() as usize,
            );
            packed_clip_bytes = packed_clip_bytes.saturating_add(
                summary["compressed_unused_clip_bytes"]
                    .as_u64()
                    .unwrap_or_default() as usize,
            );
            source_clip_bytes = source_clip_bytes.saturating_add(
                summary["unused_clip_source_bytes"]
                    .as_u64()
                    .unwrap_or_default() as usize,
            );
            decoded_clips = decoded_clips.saturating_add(
                summary["decoded_clip_count"].as_u64().unwrap_or_default() as usize,
            );
        }
        serde_json::json!({
            "animation_set_count": self.animations.len(),
            "behavior_graph_object_count": graph_objects,
            "clip_count": clips,
            "compressed_unused_clip_bytes": packed_clip_bytes,
            "unused_clip_source_bytes": source_clip_bytes,
            "decoded_clip_count": decoded_clips
        })
    }

    pub(super) fn snapshot(&self, entity: usize) -> Result<serde_json::Value> {
        let instance = self
            .instances
            .get(&entity)
            .context("Actor snapshot target missing")?;
        let animation = self
            .animations
            .get(&instance.animation)
            .context("Actor snapshot resources missing")?;
        let prepared = &instance.actor.prepared;
        let machines=prepared.machines.iter().filter(|(_,runtime)|runtime.active).map(|(id,runtime)|{
            let definition=&prepared.state.machines[id];let state=&definition.states[definition.state_indices[&runtime.current]];
            serde_json::json!({"node":id,"name":animation.graph.objects[id].fields["name"],"state":runtime.current,
                "state_name":state.info.name,"time":runtime.time,"echo_next_update":runtime.echo_next_update})
        }).collect::<Vec<_>>();
        let clips=prepared.plan.nodes.iter().filter_map(|node|{
            let definition=prepared.state.clips.get(&node.object)?;let instance=prepared.clips.get(&node.object).ok()?;
            Some(serde_json::json!({"node":node.object,"clip":definition.settings.animation_name,"time":instance.clock.local_time,
                "crop_start":instance.clock.crop_start,"crop_end":instance.clock.crop_end,"mode":definition.settings.mode}))
        }).collect::<Vec<_>>();
        let variables = animation
            .contract
            .variables
            .iter()
            .zip(prepared.state.words())
            .map(|(name, bits)| serde_json::json!({"name":name,"bits":bits}))
            .collect::<Vec<_>>();
        Ok(
            serde_json::json!({"variables":variables,"machines":machines,"clips":clips,"random_seed":prepared.random_seed,
            "observed_events":instance.actor.playback.observed_events,"active_nodes":prepared.plan.nodes.iter().map(|node|&node.object).collect::<Vec<_>>()}),
        )
    }
    #[cfg(debug_assertions)]
    pub(super) fn variable_word(&self, entity: usize, index: usize) -> Result<u32> {
        self.instances
            .get(&entity)
            .context("Variable actor missing")?
            .actor
            .prepared
            .state
            .words()
            .get(index)
            .copied()
            .context("Variable index missing")
    }
    pub fn len(&self) -> usize {
        self.instances.len()
    }
    pub fn contains(&self, entity: usize) -> bool {
        self.instances.contains_key(&entity)
    }
    pub(super) fn attachment_states(
        &self,
        entity: usize,
    ) -> Result<&super::attachment_modifier::Instances> {
        Ok(&self
            .instances
            .get(&entity)
            .context("Attachment actor is absent")?
            .actor
            .prepared
            .attachment_modifiers)
    }
    pub(super) fn attachment_summary(&self) -> serde_json::Value {
        serde_json::json!(self.instances.iter().filter(|(_,i)|!i.actor.prepared.attachment_modifiers.instances.is_empty())
            .map(|(&entity,i)|serde_json::json!({"entity":entity,"state":i.actor.prepared.attachment_modifiers.summary()})).collect::<Vec<_>>())
    }
    pub(super) fn attachment_counts(&self) -> (usize, u64) {
        self.instances
            .values()
            .flat_map(|i| i.actor.prepared.attachment_modifiers.instances.values())
            .fold((0, 0), |(active, attempts), i| {
                (
                    active + usize::from(i.active),
                    attempts.wrapping_add(i.attempts),
                )
            })
    }
    pub(super) fn write_native_variable(
        &mut self,
        entity: usize,
        name: &str,
        bits: u32,
    ) -> Result<Option<usize>> {
        let instance = self
            .instances
            .get_mut(&entity)
            .context("Variable target has no authored Havok actor")?;
        Ok(instance
            .actor
            .prepared
            .state
            .write_native_variable(name, bits))
    }
    pub(super) fn active_state_time(&self, entity: usize, name: &str) -> Option<f32> {
        let prepared = &self.instances.get(&entity)?.actor.prepared;
        prepared.machines.iter().find_map(|(id, runtime)| {
            if !runtime.active {
                return None;
            }
            let definition = &prepared.state.machines[id];
            let state = &definition.states[definition.state_indices[&runtime.current]];
            (state.info.name == name).then_some(runtime.time)
        })
    }
    pub(super) fn active_state_clip_finished(&self, entity: usize, name: &str) -> bool {
        let Some(instance) = self.instances.get(&entity) else {
            return false;
        };
        let prepared = &instance.actor.prepared;
        prepared.machines.iter().any(|(id, runtime)| {
            if !runtime.active {
                return false;
            }
            let definition = &prepared.state.machines[id];
            let state = &definition.states[definition.state_indices[&runtime.current]];
            state.info.name == name
                && prepared.clips.get(&state.info.generator.id).is_ok_and(|clip| clip.clock.at_end)
        })
    }

    pub(super) fn has_event(&self, entity: usize, name: &str) -> bool {
        self.instances.get(&entity)
            .and_then(|instance| self.animations.get(&instance.animation))
            .is_some_and(|animation| animation.contract.events.iter().any(|event| event == name))
    }

    pub fn send_event(&mut self, entity: usize, name: &str) -> Result<()> {
        let instance = self
            .instances
            .get_mut(&entity)
            .context("Named animation event target has no authored Havok actor")?;
        self.animations
            .get_mut(&instance.animation)
            .context("Actor animation resource disappeared")?
            .send_event(&mut instance.actor, name)
    }
    pub fn send_native_event(&mut self, entity: usize, id: u32) -> Result<bool> {
        let instance = self
            .instances
            .get_mut(&entity)
            .context("Native event target has no Havok actor")?;
        let map = self
            .event_maps
            .get(&instance.animation)
            .context("Native-to-local animation event map is unavailable")?;
        let Some(&local) = map.get(&id) else {
            return Ok(false);
        };
        let animation = self
            .animations
            .get_mut(&instance.animation)
            .context("Actor animation resource disappeared")?;
        instance.actor.playback.deliver_event(
            &animation.graph,
            &mut instance.actor.prepared,
            &mut animation.resources,
            super::state_machine::Event {
                id: local,
                payload: None,
            },
        )?;
        Ok(true)
    }
    #[cfg(test)]
    pub(super) fn snapshots(
        &self,
    ) -> Vec<(usize, Vec<([f32; 3], [f32; 4], [f32; 3])>, Vec<u32>, u32)> {
        self.instances
            .iter()
            .map(|(&entity, instance)| {
                (
                    entity,
                    instance
                        .actor
                        .playback
                        .pose
                        .iter()
                        .map(|bone| {
                            (
                                bone.local_translation,
                                bone.local_rotation,
                                bone.local_scale,
                            )
                        })
                        .collect(),
                    instance.actor.prepared.state.words().to_vec(),
                    instance.actor.prepared.random_seed,
                )
            })
            .collect()
    }
    pub fn remove(&mut self, entity: usize) {
        let Some(instance) = self.instances.remove(&entity) else {
            return;
        };
        if !self
            .instances
            .values()
            .any(|active| active.model == instance.model)
        {
            self.models.remove(&instance.model);
        }
        if !self
            .instances
            .values()
            .any(|active| active.animation == instance.animation)
        {
            self.animations.remove(&instance.animation);
            self.event_maps.remove(&instance.animation);
            self.export_maps.remove(&instance.animation);
        }
    }
    pub fn prepare(
        &mut self,
        world: &World,
        range: std::ops::Range<usize>,
        files: &[VirtualFile],
    ) -> Vec<String> {
        let find = |path: &str| {
            files.iter().find(|file| {
                key(&file.path) == key(path) || key(&file.path) == format!("{}_wii", key(path))
            })
        };
        let mut errors = Vec::new();
        for entity in range.filter(|&entity| entity != world.player_entity) {
            let runtime = &world.entities[entity];
            let components = runtime
                .components
                .iter()
                .filter(|c| c.class_name == "AnimationComponent")
                .collect::<Vec<_>>();
            if components.is_empty() {
                continue;
            }
            let result = (|| -> Result<()> {
                ensure!(
                    components.len() == 1,
                    "Multiple Havok animation components require native component arbitration"
                );
                ensure!(
                    !runtime
                        .components
                        .iter()
                        .any(|c| c.class_name == "AnimationComponentGB"),
                    "Mixed Havok/Gamebryo animation requires native arbitration"
                );
                ensure!(
                    !world.actor_has_local_nif_draws(entity),
                    "Havok actor with local NIF controller draws requires binding reconstruction"
                );
                let string = |name: &str| {
                    components[0]
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .and_then(|p| p.value.as_str())
                        .with_context(|| format!("Missing actor animation {name}"))
                };
                let project = string("Behavior File Path")?;
                let character = string("CharacterName")?;
                let identity = (key(project), character.to_owned());
                if !self.animations.contains_key(&identity) {
                    let reflection = &world
                        .report
                        .player_behavior
                        .as_ref()
                        .context("Missing player behavior reflection registry")?
                        .reflection;
                    let (contract, graph) = super::behavior::character_assets_with_reflection(
                        project,
                        character,
                        |path| find(path).map(|file| file.data.as_slice()),
                        reflection,
                    )?;
                    let mut resources =
                        super::animation_resources::Resources::load(&contract, |path| {
                            find(path).map(|file| file.data.clone())
                        })?;
                    resources.set_wii_source(self.wii_source.clone());
                    if let Ok(catalog) = &world.animation_events {
                        let map = catalog.graph_map(&contract.events)?;
                        self.export_maps.insert(
                            identity.clone(),
                            map.iter()
                                .map(|(&native, &local)| (local, native))
                                .collect(),
                        );
                        self.event_maps.insert(identity.clone(), map);
                    }
                    self.animations.insert(
                        identity.clone(),
                        Animation {
                            contract,
                            graph,
                            resources,
                        },
                    );
                }
                let model = runtime
                    .model_asset
                    .as_ref()
                    .context("Havok actor has no model")?;
                if !self.models.contains_key(model) {
                    let file = find(model).context("Havok model resource missing")?;
                    let nif = epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&file.data)
                        .map_err(anyhow::Error::msg)?;
                    let bind = world
                        .model_bounds
                        .get(model)
                        .context("Actor model hierarchy missing")?
                        .base_world_transforms_with_poses(None, &BTreeMap::new())?;
                    let mut meshes = Vec::new();
                    for (&block, draws) in &runtime.mesh_draws {
                        let raw = match nif.blocks.get(block as usize) {
                            Some(epic_mickey_lib_rs::nif_file::NiObject::NiMesh(mesh)) => mesh,
                            _ => anyhow::bail!("Actor mesh {block} is not a Wii NiMesh"),
                        };
                        let decoded = crate::wii_mesh::extract(&nif, raw)?;
                        let draw =
                            &world.draws[*draws.first().context("Actor mesh has no draw")?];
                        ensure!(
                            draw.indices.as_ref() == &decoded.mesh.indices,
                            "Actor skin geometry and draw topology differ for mesh {block}"
                        );
                        ensure!(
                            draw.v_data.len() == decoded.mesh.positions.len() * 12,
                            "Actor skin geometry and draw vertex counts differ for mesh {block}"
                        );
                        let transform = bind
                            .get(&block)
                            .context("Actor mesh bind transform missing")?;
                        let matrix = super::nif_runtime::matrix(transform);
                        meshes.push(Mesh {
                            block: block as usize,
                            positions: decoded.mesh.positions,
                            normals: decoded
                                .mesh
                                .normals
                                .context("Actor mesh lacks authored normals")?,
                            sources: decoded.source_indices,
                            transform: std::array::from_fn(|row| {
                                std::array::from_fn(|col| matrix[col * 4 + row])
                            }),
                            vertices: draw.v_data.clone(),
                        });
                    }
                    ensure!(!meshes.is_empty(), "Actor has no prepared meshes");
                    self.models.insert(model.clone(), meshes);
                }
                let actor = self.animations.get_mut(&identity).unwrap().instantiate()?;
                // Reject an incompatible rig/skin before publishing the instance.
                for mesh in &self.models[model] {
                    let skin=world.skins.get(model).and_then(|skins|skins.iter().find(|skin|skin.block==mesh.block))
                        .context("Actor mesh lacks a decoded skin; rigid attachments need native binding")?;
                    let pose = super::skin::model_pose(&actor.playback.pose, &[])?;
                    let palette = super::skin::palette_from_model_pose(skin, &pose)?;
                    super::skin::render_skinned_pose(skin,&palette,&mesh.vertices,&mesh.positions,&mesh.normals,&mesh.sources,mesh.transform)?;
                }
                ensure!(
                    !self.instances.contains_key(&entity),
                    "Havok actor already initialized"
                );
                self.instances.insert(
                    entity,
                    Instance {
                        animation: identity,
                        model: model.clone(),
                        actor,
                        skin_frame: Default::default(),
                    },
                );
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(format!(
                    "Authored Havok actor {}:{}: {error:#}",
                    runtime.scene, runtime.name
                ));
            }
        }
        errors
    }
    pub fn advance(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut failed = Vec::new();
        for (&entity, instance) in &mut self.instances {
            if let Err(error) = self
                .animations
                .get_mut(&instance.animation)
                .unwrap()
                .advance(&mut instance.actor, 1)
            {
                errors.push(format!("Authored Havok actor {entity} update: {error:#}"));
                failed.push(entity);
            }
        }
        for entity in failed {
            self.remove(entity);
        }
        errors
    }
    pub fn entities(&self) -> Vec<usize> {
        self.instances.keys().copied().collect()
    }
    pub fn active_entities(&self, mut is_active: impl FnMut(usize) -> bool) -> Vec<usize> {
        self.instances
            .keys()
            .copied()
            .filter(|&entity| is_active(entity))
            .collect()
    }
    pub fn graph(&self, entity: usize) -> Result<&super::behavior::Document> {
        let instance = self
            .instances
            .get(&entity)
            .context("Actor payload target disappeared")?;
        Ok(&self
            .animations
            .get(&instance.animation)
            .context("Actor payload graph disappeared")?
            .graph)
    }
    pub fn bone_pose(&self, entity: usize, name: &str) -> Result<super::actor_bone_query::Pose> {
        let instance = self
            .instances
            .get(&entity)
            .context("Attachment actor disappeared")?;
        let animation = self
            .animations
            .get(&instance.animation)
            .context("Attachment animation disappeared")?;
        super::skeletal_children::bone_pose(
            &animation.resources,
            &instance.actor.playback.pose,
            name,
        )
    }
    pub fn shadow_bone_pose(
        &self,
        entity: usize,
        name: &str,
    ) -> Result<Option<super::actor_bone_query::Pose>> {
        let instance = self
            .instances
            .get(&entity)
            .context("Shadow actor disappeared")?;
        let animation = self
            .animations
            .get(&instance.animation)
            .context("Shadow animation disappeared")?;
        let bones = &animation.resources.rig.bones;
        let selected = if name.is_empty() {
            bones.get(2)
        } else {
            bones
                .iter()
                .find(|bone| bone.name.eq_ignore_ascii_case(name))
        };
        selected
            .map(|bone| {
                super::skeletal_children::bone_pose(
                    &animation.resources,
                    &instance.actor.playback.pose,
                    &bone.name,
                )
            })
            .transpose()
    }
    pub fn step_and_export(
        &mut self,
        entity: usize,
    ) -> Result<(Vec<super::state_machine::Event>, Vec<String>)> {
        let instance = self
            .instances
            .get_mut(&entity)
            .context("Actor export target disappeared")?;
        let animation = self
            .animations
            .get_mut(&instance.animation)
            .context("Actor animation resource disappeared")?;
        animation.advance(&mut instance.actor, 1)?;
        let map = self
            .export_maps
            .get(&instance.animation)
            .context("Native actor export map is unavailable")?;
        let mut output = Vec::new();
        let mut unsupported = Vec::new();
        for frame in std::mem::take(&mut instance.actor.playback.emitted_frames) {
            output.extend(super::animation_event_queue::normalize(frame,|local| {
                if local<0 {return Ok(local);}
                if let Some(&native)=map.get(&local) {return Ok(native as i32);}
                animation.contract.events.get(local as usize).context("Emitted event exceeds graph event table")?;
                // The native external map yields -1 for graph-local events.
                Ok(-1)
            })?);
        }
        Ok((output, unsupported))
    }
    pub fn apply(
        &self,
        world: &World,
        draws: &mut [crate::render_support::viewport3d::GpuDrawCommand],
    ) -> Result<()> {
        for (&entity, instance) in &self.instances {
            if !world.entity_scene_active(entity) {
                continue;
            }
            let pose = super::skin::model_pose(&instance.actor.playback.pose, &[])?;
            let runtime = &world.entities[entity];
            let meshes = &self.models[&instance.model];
            let mut frame = instance.skin_frame.borrow_mut();
            if frame.pose != pose || frame.meshes.len() != meshes.len() {
                let skinned = meshes.iter().map(|mesh| {
                    let skin = world.skins[&instance.model].iter()
                        .find(|skin| skin.block == mesh.block).context("Actor skin disappeared")?;
                    let palette = super::skin::palette_from_model_pose(skin, &pose)?;
                    super::skin::render_skinned_pose(skin,&palette,&mesh.vertices,&mesh.positions,&mesh.normals,&mesh.sources,mesh.transform)
                        .map(super::clip_preview::SkinnedMesh::new)
                }).collect::<Result<Vec<_>>>()?;
                frame.pose = pose;
                frame.meshes = skinned;
            }
            for (mesh, skinned) in meshes.iter().zip(&frame.meshes) {
                for &draw in &runtime.mesh_draws[&(mesh.block as u32)] {
                    skinned.apply(&mut draws[draw]);
                }
            }
        }
        Ok(())
    }
}
