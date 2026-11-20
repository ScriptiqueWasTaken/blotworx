//! Demo adapters for authored part switches and persistent presentation state.
use super::assets::World;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct State {
    pub culled_draws: std::collections::BTreeSet<usize>,
    pub streams: BTreeMap<String, Vec<String>>,
    pub stream_banks: BTreeMap<String, Vec<String>>,
    pub stream_resources:BTreeMap<String,Vec<super::scene_resources::Resource>>,
    pub audio_states: BTreeMap<String, String>,
    pub next_part: Option<String>,
    pub emitter_requests: BTreeMap<usize, bool>,
    pub effect_warnings: std::collections::BTreeSet<String>,
}
fn fields(bytes: &[u8]) -> BTreeMap<String, String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect()
}
fn stream_scenes(bytes: &[u8]) -> Vec<String> {
    // In a requested stream part, -i resources are dormant destinations, not
    // omissions. StreamPart explicitly activates the requested part.
    super::scene_resources::part_records(&String::from_utf8_lossy(bytes), false)
        .into_iter()
        .filter(|record| record.kind == 0)
        .map(|record| record.name)
        .collect()
}
fn stream_banks(bytes: &[u8]) -> Vec<String> {
    super::scene_resources::part_records(&String::from_utf8_lossy(bytes), false)
        .into_iter()
        .filter(|record| record.kind == 1)
        .map(|record| record.name)
        .collect()
}
impl World {
    pub(super) fn initialize_level_streams(&mut self) -> Result<()> {
        for name in [
            "StreamPart",
            "StreamInteriorZone",
            "StreamInterior",
            "StreamZones",
            "StreamInLevel",
            "DeactivateScene",
            "BwxIsLevelLoadPending",
        ] {
            self.scripts.bind_missing_world_api(name)?;
        }
        let root = super::assets::files_root(self.source_data_root());
        let path = std::fs::read_dir(root.join("packfiles"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("levels.pak"))
            })
            .context("Missing levels archive")?;
        let files = crate::pack_io::read(std::fs::read(path)?)?.pak.files;
        let read = |path: &str| -> Result<Vec<u8>> {
            let path = super::resource_path::canonical(path)?;
            let loose = root.join(&path);
            if loose.is_file() {
                return Ok(std::fs::read(loose)?);
            }
            Ok(files
                .iter()
                .find(|f| f.path.replace('\\', "/").eq_ignore_ascii_case(&path))
                .with_context(|| format!("Missing part descriptor {path}"))?
                .data
                .clone())
        };
        let values = fields(&read(&self.report.level)?);
        let Some(folder) = values.get("folder") else {
            return Ok(());
        };
        let folder=self.report.geometry.iter().find(|r|r["kind"]=="standalone_shared_context")
            .and_then(|r|r["parent_folder"].as_str()).unwrap_or(folder.as_str());
        let mut parts = BTreeMap::new();
        for (key, value) in &values {
            if (key.starts_with("stream") || key == "initial" || key == "interior")
                && !value.is_empty()
            {
                parts.insert(key.clone(), value.clone());
            }
        }
        // A .level names only its initial/interior parts. Later stream names
        // are referenced by Lua and live beside them in the same level folder.
        let prefix = format!("levels/{}/", folder.to_ascii_lowercase());
        for file in &files {
            let path = file.path.replace('\\', "/").to_ascii_lowercase();
            if let Some(stem) = path
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".part"))
                .filter(|s| !s.contains('/'))
            {
                parts.insert(stem.into(), stem.into());
            }
        }
        for (key, value) in &parts {
            let suffix = if value.to_ascii_lowercase().ends_with(".part") {
                ""
            } else {
                ".part"
            };
            let path = format!("Levels/{folder}/{value}{suffix}");
            let bytes = match read(&path).or_else(|error| {
                let original_folder=values.get("folder").context("Level has no folder")?;
                if original_folder.eq_ignore_ascii_case(folder) {return Err(error)}
                read(&format!("Levels/{original_folder}/{value}{suffix}"))
            }) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let message =
                        format!("Optional streamed part {path} is unavailable: {error:#}");
                    if !self.report.unsupported.contains(&message) {
                        self.report.unsupported.push(message);
                    }
                    continue;
                }
            };
            let scenes = stream_scenes(&bytes);
            let records=super::scene_resources::part_records(&String::from_utf8_lossy(&bytes),false);
            self.demo_story.scene.stream_resources.insert(key.clone(),records.clone());
            self.demo_story.scene.stream_resources.insert(value.trim_end_matches(".part").to_ascii_lowercase(),records);
            let banks = stream_banks(&bytes);
            self.demo_story
                .scene
                .streams
                .insert(key.clone(), scenes.clone());
            self.demo_story
                .scene
                .streams
                .insert(value.trim_end_matches(".part").to_ascii_lowercase(), scenes);
            self.demo_story
                .scene
                .stream_banks
                .insert(key.clone(), banks.clone());
            self.demo_story
                .scene
                .stream_banks
                .insert(value.trim_end_matches(".part").to_ascii_lowercase(), banks);
        }
        Ok(())
    }
    pub(super) fn demo_scene_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if name == "BwxIsLevelLoadPending" {
            let pending = if args.len() >= 2 {
                let path_value = lua
                    .coerce_string(args[1].clone())?
                    .ok_or_else(|| mlua::Error::external("Scene path must be text"))?;
                let path = path_value.to_str()?.to_owned();
                let wanted = super::scene_resources::scene_name(&path);
                self.scene_requests
                    .names()
                    .any(|name| super::scene_resources::scene_name(name) == wanted)
                    || !self.scene_resources.is_active(&wanted)
            } else {
                self.scene_requests.occupied() != 0
            };
            return Ok(mlua::MultiValue::from_vec(vec![mlua::Value::Boolean(
                pending,
            )]));
        }
        let result = (|| -> Result<()> {
            match name {
                "SetAppCulled" | "ClearAppCulled" => {
                    ensure!(args.len() == 1, "{name} requires one entity");
                    if let Some(entity) = self.scripts.resolve_world_argument(&args[0])? {
                        for draw in self.entities[entity].draws.clone() {
                            if name == "SetAppCulled" {
                                self.demo_story.scene.culled_draws.insert(draw);
                            } else {
                                self.demo_story.scene.culled_draws.remove(&draw);
                            }
                        }
                    }
                }
                "StreamPart" | "StreamInteriorZone" | "StreamInterior" | "StreamZones" => {
                    ensure!(!args.is_empty(), "{name} requires a part key");
                    let value_index = if matches!(name, "StreamInterior" | "StreamZones") {
                        1
                    } else {
                        0
                    };
                    ensure!(
                        args.len() > value_index,
                        "{name} requires an owner and part key"
                    );
                    let key = lua
                        .coerce_string(args[value_index].clone())?
                        .context("Part key must be text")?
                        .to_str()?
                        .to_ascii_lowercase();
                    ensure!(
                        self.demo_story.scene.streams.contains_key(&key),
                        "{name} references unknown level part {key}"
                    );
                    if let Some(banks) = self.demo_story.scene.stream_banks.get(&key).cloned() {
                        let root = super::assets::files_root(self.source_data_root());
                        for bank in banks {
                            let path = super::resource_path::canonical(&bank)
                                .map_err(mlua::Error::external)?;
                            let path = if path.to_ascii_lowercase().starts_with("audio/") {
                                path
                            } else {
                                format!("Audio/Wii/{path}")
                            };
                            let bytes = std::fs::read(root.join(&path)).map_err(|error| {
                                mlua::Error::external(anyhow::anyhow!(
                                    "Read streamed soundbank {path}: {error}"
                                ))
                            })?;
                            self.audio_playback
                                .register_bank(bytes.into())
                                .map_err(mlua::Error::external)?;
                        }
                    }
                    let scenes = self
                        .demo_story
                        .scene
                        .streams
                        .get(&key)
                        .with_context(|| format!("Unknown level part key {key}"))?
                        .clone();
                    self.demo_story.scene.next_part = Some(key.clone());
                    let slot=usize::from(matches!(name,"StreamInteriorZone"|"StreamInterior"));
                    let records=self.demo_story.scene.stream_resources.get(&key).context("Streamed part resource records absent")?.clone();
                    let outgoing=self.scene_resources.replace_part(slot,records);
                    self.level_resource.replace_part(slot,&key)?;
                    for scene in outgoing {
                        self.scene_requests.cancel(&scene);
                        self.deactivate_streamed_scene(&scene)?;
                        self.set_streamed_scene_lookup(&scene,false)?;
                    }
                    for scene in scenes {
                        let path = scene
                            .strip_suffix(".gsa")
                            .map(|stem| format!("{stem}.bin"))
                            .unwrap_or_else(|| scene.clone());
                        let loaded = self
                            .report
                            .scenes
                            .iter()
                            .any(|s| s.replace('\\', "/").eq_ignore_ascii_case(&path));
                        if loaded && self.scene_resources.is_active(&scene) {
                            continue;
                        }
                        ensure!(
                            self.scene_requests.request(&scene)
                                != super::scene_loading::Admission::Full,
                            "Scene queue full"
                        );
                    }
                }
                "StreamInLevel" => {
                    ensure!(
                        args.len() >= 2,
                        "StreamInLevel requires an owner and scene path"
                    );
                    let path_value = lua
                        .coerce_string(args[1].clone())?
                        .context("Scene path must be text")?;
                    let path = path_value.to_str()?.to_owned();
                    let path =
                        super::resource_path::canonical(&path).map_err(mlua::Error::external)?;
                    ensure!(
                        path.to_ascii_lowercase().ends_with(".gsa"),
                        "StreamInLevel expects a .gsa scene"
                    );
                    ensure!(
                        self.scene_requests.request(&path) != super::scene_loading::Admission::Full,
                        "Scene queue full"
                    );
                }
                "DeactivateScene" => {
                    if let Some(value) = args.front() {
                        if let Some(name) = lua.coerce_string(value.clone())? {
                            let name = name.to_str()?;
                            self.scene_requests.cancel(&name);
                            self.deactivate_streamed_scene(&name)
                                .map_err(mlua::Error::external)?;
                        }
                    }
                }
                "AudioSetSwitch" | "NativeAudioSetSwitch" => {
                    self.audio_set_switch_lua(lua, args)?;
                    return Ok(());
                }
                "AudioSetState" | "NativeAudioSetState" => {
                    ensure!(args.len() >= 2, "Audio state requires group and value");
                    let group = lua
                        .coerce_string(args[args.len() - 2].clone())?
                        .context("Audio group must be text")?
                        .to_str()?
                        .to_owned();
                    let state = lua
                        .coerce_string(args[args.len() - 1].clone())?
                        .context("Audio state must be text")?
                        .to_str()?
                        .to_owned();
                    // Keep the requested state; host audio currently lacks Wwise
                    // state-driven mixing. This must not abort gameplay scripts.
                    self.demo_story.scene.audio_states.insert(group, state);
                }
                "CounterAdd" | "CounterSubtract" => {
                    ensure!(!args.is_empty(), "Logic counter requires entity");
                    if let Some(entity) = self.scripts.resolve_world_argument(&args[0])? {
                        let amount = args
                            .get(1)
                            .map(|v| lua.coerce_number(v.clone()))
                            .transpose()?
                            .flatten()
                            .unwrap_or(1.) as i32;
                        let c = self.entities[entity]
                            .components
                            .iter_mut()
                            .find(|c| c.class_name == "LogicCounterComponent")
                            .context("Target has no logic counter")?;
                        let number = |key: &str| {
                            c.properties
                                .iter()
                                .find(|p| p.name == key)
                                .and_then(|p| p.value.as_i64())
                                .unwrap_or(0) as i32
                        };
                        let (old, min, max) = (
                            number("CounterValue"),
                            number("CounterMinValue"),
                            number("CounterMaxValue"),
                        );
                        ensure!(min <= max, "Invalid logic counter bounds");
                        let next = if name == "CounterAdd" {
                            old.saturating_add(amount)
                        } else {
                            old.saturating_sub(amount)
                        }
                        .clamp(min, max);
                        if let Some(p) = c.properties.iter_mut().find(|p| p.name == "CounterValue")
                        {
                            p.value = serde_json::json!(next);
                        }
                        if next != old {
                            let outputs = self.entities[entity]
                                .components
                                .iter()
                                .filter(|c| c.class_name == "InputOutputComponent")
                                .flat_map(|c| c.properties.iter())
                                .filter(|p| p.name == "Output")
                                .filter_map(|p| p.value.as_array())
                                .flatten()
                                .filter_map(|v| v.as_str())
                                .filter(|s| {
                                    let s = s.to_ascii_lowercase();
                                    next == max && s.contains("max")
                                        || next == min && s.contains("min")
                                        || s.contains("changed")
                                })
                                .map(str::to_owned)
                                .collect::<std::collections::BTreeSet<_>>();
                            for output in outputs {
                                self.emit_entity_output(entity, &output, entity);
                            }
                        }
                    }
                }
                "StartEmitters" | "StopEmitters" => {
                    if let Some(value) = args.front() {
                        if let Some(entity) = self.scripts.resolve_world_argument(value)? {
                            let enabled = name == "StartEmitters";
                            self.admit_scene_particles();
                            self.demo_story
                                .scene
                                .emitter_requests
                                .insert(entity, enabled);
                            let result = match self.named_fx.set_demo_enabled(
                                entity,
                                enabled,
                                self.frame_clock.clock.entity_seconds(true),
                            ) {
                                Ok(false) => self.scene_particles.set_enabled(entity, enabled),
                                result => result,
                            };
                            match result {
                                Ok(true) => {}
                                result
                                    if self.demo_story.enabled
                                        && self.entities[entity].components.iter().any(|c| {
                                            c.class_name == "ParticleSystemControllerComponent"
                                        }) =>
                                {
                                    let reason = match result {
                                        Err(error) => format!("{error:#}"),
                                        _ => "No drawable particle layers".into(),
                                    };
                                    // Visual limitations must not abort the remaining
                                    // outputs of a repaired pipe or broken-door script.
                                    self.demo_story.scene.effect_warnings.insert(format!(
                                        "{}: {reason}",
                                        self.entities[entity].name
                                    ));
                                }
                                Err(error) => return Err(error),
                                Ok(false) => anyhow::bail!(
                                    "{} has no supported particle controller",
                                    self.entities[entity].name
                                ),
                            }
                        }
                    }
                }
                "ForceThinned" => {
                    if let Some(value) = args.front() {
                        if let Some(entity) = self.scripts.resolve_world_argument(value)? {
                            let painted = false;
                            if let Some(counter) = self.paint_counters.get_mut(&entity) {
                                counter.value = if painted {
                                    counter.paint_threshold
                                } else {
                                    counter.thinner_threshold
                                };
                            }
                            let owners = self
                                .jigsaw_owners
                                .iter()
                                .filter_map(|(&owner, source)| {
                                    (self.jigsaw_instances[source.instance].entity == entity)
                                        .then_some(owner)
                                })
                                .collect::<Vec<_>>();
                            for owner in owners {
                                self.brush_jigsaw_contact(owner, painted)?;
                            }
                            let owners = self
                                .model_paint_owners
                                .iter()
                                .filter_map(|(&owner, &target)| (target == entity).then_some(owner))
                                .collect::<Vec<_>>();
                            for owner in owners {
                                self.brush_model_hit(owner, painted, [0., 1e10, 0.])?;
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
            Ok(())
        })();
        result.map_err(mlua::Error::external)?;
        Ok(mlua::MultiValue::new())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn stream_part_preserves_authored_soundbanks() {
        assert_eq!(
            super::stream_banks(b"zoneb.gsa -i\nLVL_FX_GV_STB_Airlock.bnk\nshared.gsa\n"),
            ["lvl_fx_gv_stb_airlock.bnk"]
        );
    }
    #[test]
    fn stream_part_includes_dormant_destination_scenes() {
        assert_eq!(
            super::stream_scenes(b"zoneb.gsa -i\nshared.gsa\nzonec.gsa -inactive\n"),
            ["zoneb.gsa", "shared.gsa", "zonec.gsa"]
        );
    }
}
