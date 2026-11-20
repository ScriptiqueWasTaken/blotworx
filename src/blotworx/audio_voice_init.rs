//! CAkPBI initialization80357f70..803582a4. Binding/positioning/path/media
//! services remain supplied; playing records are the production shared registry.
use super::audio_playing::Playing;
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct State {
    pub voice: u32,
    pub node: u32,
    pub source: u32,
    pub object: u32,
    pub playing_id: u32,
    pub callback_flags: u32,
    pub positioning: Option<u32>,
    pub path: Option<u32>,
    pub path_id: u32,
    pub media: [u32; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub attenuation_id: u32,
    pub path_id: u32,
    pub mode: u32,
    pub path_enabled: bool,
    pub force_path: bool,
}
/// Native RTPC subscription arguments at track+8/+c/+10/+1c/+14/+18.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub words: [u32; 6],
}
pub trait Control {
    fn initialize_parameters(&mut self, voice: u32) -> Result<u32>;
    fn initialize_effects(&mut self, voice: u32) -> Result<u32>;
    fn positioning(&mut self, node: u32, object: u32, voice: u32) -> Result<Option<u32>>;
    fn position(&mut self, position: u32) -> Result<Position>;
    fn cached_attenuation(&mut self, position: u32) -> Result<Option<u32>>;
    fn acquire_attenuation(&mut self, id: u32) -> Result<Option<u32>>;
    fn cache_attenuation(&mut self, position: u32, attenuation: Option<u32>) -> Result<()>;
    fn attenuation_values(&mut self, attenuation: u32) -> Result<[f32; 2]>;
    fn apply_attenuation_values(&mut self, position: u32, values: [f32; 2]) -> Result<()>;
    fn attenuation_bindings(&mut self, attenuation: u32) -> Result<Vec<Binding>>;
    fn subscribe(&mut self, voice: u32, object: u32, binding: Binding) -> Result<u32>;
    fn allocate_path(&mut self) -> Result<Option<u32>>;
    ///803383e0 returns the first spatial ancestor's path-state handle (+70),
    /// not the path-mode enum stored in the positioning definition.
    fn node_path_state(&mut self, node: u32) -> Result<u32>;
    fn initialize_path(&mut self, position: u32, path: u32, mode: u32) -> Result<u32>;
    fn release_path(&mut self, path: u32) -> Result<()>;
    fn attach_path(&mut self, path: u32, voice: u32) -> Result<u32>;
    fn set_path_node(&mut self, path: u32, node: u32) -> Result<()>;
    fn set_path_playing_id(&mut self, path: u32, playing_id: u32) -> Result<()>;
    fn acquire_media(&mut self, source: u32, voice: u32) -> Result<[u32; 2]>;
}
pub enum Stage {
    Bindings,
    Subscriptions,
}
pub fn initialize<H: Control>(
    state: &mut State,
    initial_path: [u32; 2],
    playing: &mut Playing,
    host: &mut H,
) -> Result<u32> {
    initialize_with_stages(
        state,
        initial_path,
        playing,
        host,
        |stage, voice, host| match stage {
            Stage::Bindings => host.initialize_parameters(voice),
            Stage::Subscriptions => host.initialize_effects(voice),
        },
    )
}
/// Lets a constructed voice supply recovered binding/subscription stages while retaining
/// the original playing-admission and subsequent initialization order.
pub fn initialize_with_stages<H: Control>(
    state: &mut State,
    initial_path: [u32; 2],
    playing: &mut Playing,
    host: &mut H,
    mut initialize_stage: impl FnMut(Stage, u32, &mut H) -> Result<u32>,
) -> Result<u32> {
    //8035c640 returns1 even when no callback record exists. It updates the
    // flags out-parameter only for a found record. ID zero bypasses it entirely.
    if state.playing_id == 0 {
        return Ok(2);
    }
    if let Some(flags) = playing.add_voice(state.playing_id) {
        state.callback_flags = flags;
    }
    let parameters = initialize_stage(Stage::Bindings, state.voice, host)?;
    if parameters != 1 {
        return Ok(parameters);
    }
    let mut status = initialize_stage(Stage::Subscriptions, state.voice, host)?;
    if status != 1 {
        return Ok(status);
    }
    state.positioning = None;
    state.positioning = host.positioning(state.node, state.object, state.voice)?;
    if let Some(position) = state.positioning {
        let definition = host.position(position)?;
        let attenuation = match host.cached_attenuation(position)? {
            Some(attenuation) => Some(attenuation),
            None => {
                let attenuation = host.acquire_attenuation(definition.attenuation_id)?;
                host.cache_attenuation(position, attenuation)?;
                attenuation
            }
        };
        if let Some(attenuation) = attenuation {
            let values = host.attenuation_values(attenuation)?;
            host.apply_attenuation_values(position, values)?;
            for binding in host.attenuation_bindings(attenuation)? {
                status = host.subscribe(state.voice, state.object, binding)?;
                if status != 1 {
                    return Ok(status);
                }
            }
        } else if definition.attenuation_id != 0 {
            return Ok(2);
        }
        let definition = host.position(position)?;
        if definition.mode == 2 && definition.path_enabled {
            let mut path_id = definition.path_id;
            if initial_path[0] != 0 && initial_path[1] == path_id {
                state.path = Some(initial_path[0]);
                state.path_id = initial_path[1];
            }
            if state.path.is_none() || definition.force_path {
                state.path = host.allocate_path()?;
                if let Some(path) = state.path {
                    let mode = host.node_path_state(state.node)?;
                    status = host.initialize_path(position, path, mode)?;
                    if status != 1 {
                        host.release_path(path)?;
                        state.path = None;
                        path_id = 0;
                    }
                    state.path_id = path_id;
                }
            }
        }
    }
    if let Some(path) = state.path {
        if host.attach_path(path, state.voice)? == 2 {
            state.path = None;
        } else {
            host.set_path_node(path, state.node)?;
            host.set_path_playing_id(path, state.playing_id)?;
        }
    }
    // The native code still acquires media after a path initialization failure;
    // its retained status is returned afterward for the caller to terminate.
    state.media = host.acquire_media(state.source, state.voice)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::super::audio_ingress::{Context, Registration};
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        config: &'a Value,
        calls: Vec<Value>,
        attenuation_refs: u32,
        cached: bool,
        subscription: usize,
    }
    impl Host<'_> {
        fn flag(&self, name: &str) -> bool {
            self.config[name].as_bool().unwrap()
        }
        fn number(&self, name: &str) -> u32 {
            self.config[name].as_u64().unwrap() as u32
        }
    }
    impl Control for Host<'_> {
        fn initialize_parameters(&mut self, _: u32) -> Result<u32> {
            self.calls.push(json!({"op":"parameters"}));
            Ok(self.number("parameters"))
        }
        fn initialize_effects(&mut self, _: u32) -> Result<u32> {
            self.calls.push(json!({"op":"effects"}));
            Ok(self.number("effects"))
        }
        fn positioning(&mut self, node: u32, object: u32, voice: u32) -> Result<Option<u32>> {
            assert_eq!((node, object, voice), (17, 42, 1));
            self.calls.push(json!({"op":"positioning"}));
            Ok(self.flag("position").then_some(1))
        }
        fn position(&mut self, _: u32) -> Result<Position> {
            Ok(Position {
                attenuation_id: self.number("attenuation_id"),
                path_id: 123,
                mode: self.number("mode"),
                path_enabled: self.flag("path_enabled"),
                force_path: self.flag("force_path"),
            })
        }
        fn cached_attenuation(&mut self, _: u32) -> Result<Option<u32>> {
            Ok(self.cached.then_some(2))
        }
        fn acquire_attenuation(&mut self, id: u32) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"acquire_attenuation","id":id}));
            Ok(if self.flag("attenuation_found") {
                self.attenuation_refs += 1;
                Some(2)
            } else {
                None
            })
        }
        fn cache_attenuation(&mut self, _: u32, attenuation: Option<u32>) -> Result<()> {
            self.cached = attenuation.is_some();
            Ok(())
        }
        fn attenuation_values(&mut self, _: u32) -> Result<[f32; 2]> {
            Ok([2.5, 7.25])
        }
        fn apply_attenuation_values(&mut self, _: u32, values: [f32; 2]) -> Result<()> {
            for (index, value) in values.into_iter().enumerate() {
                self.calls
                    .push(json!({"op":"attenuation_value","index":index,"value":value}));
            }
            Ok(())
        }
        fn attenuation_bindings(&mut self, _: u32) -> Result<Vec<Binding>> {
            self.calls.push(json!({"op":"attenuation_bindings"}));
            Ok((0..self.config["subscriptions"].as_array().unwrap().len())
                .map(|i| Binding {
                    words: [11, 22, 33, 66, 44, 55].map(|word| word + i as u32 * 100),
                })
                .collect())
        }
        fn subscribe(&mut self, voice: u32, object: u32, binding: Binding) -> Result<u32> {
            assert_eq!((voice, object), (1, 42));
            self.calls
                .push(json!({"op":"subscribe","words":binding.words}));
            let result = self.config["subscriptions"][self.subscription]
                .as_u64()
                .unwrap() as u32;
            self.subscription += 1;
            Ok(result)
        }
        fn allocate_path(&mut self) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"allocate_path"}));
            Ok(self.flag("allocate_path").then_some(99))
        }
        fn node_path_state(&mut self, _: u32) -> Result<u32> {
            self.calls.push(json!({"op":"node_path_mode"}));
            Ok(5)
        }
        fn initialize_path(&mut self, _: u32, path: u32, mode: u32) -> Result<u32> {
            assert_eq!((path, mode), (99, 5));
            self.calls.push(json!({"op":"initialize_path"}));
            Ok(self.number("initialize_path"))
        }
        fn release_path(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"release_path"}));
            Ok(())
        }
        fn attach_path(&mut self, path: u32, _: u32) -> Result<u32> {
            self.calls.push(json!({"op":"attach_path","path":path}));
            Ok(self.number("attach_path"))
        }
        fn set_path_node(&mut self, _: u32, node: u32) -> Result<()> {
            self.calls.push(json!({"op":"path_node","node":node}));
            Ok(())
        }
        fn set_path_playing_id(&mut self, _: u32, id: u32) -> Result<()> {
            self.calls.push(json!({"op":"path_playing_id","id":id}));
            Ok(())
        }
        fn acquire_media(&mut self, source: u32, voice: u32) -> Result<[u32; 2]> {
            assert_eq!((source, voice), (88, 1));
            self.calls.push(json!({"op":"media"}));
            Ok([0x12340000, 4096])
        }
    }
    #[test]
    fn original_voice_initialization_gates_attenuations_paths_media_and_registry() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_init.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let config = &case["config"];
            let mut host = Host {
                config,
                calls: vec![],
                attenuation_refs: 3,
                cached: config["cached"].as_bool().unwrap(),
                subscription: 0,
            };
            let mut state = State {
                voice: 1,
                node: 17,
                source: 88,
                object: 42,
                playing_id: host.number("playing_id"),
                callback_flags: 0x55667788,
                positioning: None,
                path: None,
                path_id: 0,
                media: [0; 2],
            };
            let mut playing = Playing::default();
            if host.flag("record") {
                playing.register(Registration {
                    event: 5,
                    context: Context {
                        playing_id: state.playing_id,
                        ..Default::default()
                    },
                    callback: 9,
                    cookie: 0,
                    flags: 0x10001,
                });
            }
            let initial =
                std::array::from_fn(|i| config["initial_path"][i].as_u64().unwrap() as u32);
            let status = initialize(&mut state, initial, &mut playing, &mut host)?;
            let calls = if state.playing_id != 0 {
                let mut calls = vec![json!({"op":"add_voice"})];
                calls.extend(host.calls);
                calls
            } else {
                host.calls
            };
            assert_eq!(json!(status), case["status"], "case {index}");
            assert_eq!(json!(state), case["state"], "case {index}");
            assert_eq!(json!(calls), case["calls"], "case {index}");
            assert_eq!(
                json!(host.attenuation_refs),
                case["attenuation_refs"],
                "case {index}"
            );
            assert_eq!(
                json!(host.cached),
                case["attenuation_cached"],
                "case {index}"
            );
            assert_eq!(
                json!(playing
                    .record(state.playing_id)
                    .map_or(0, |record| record.voices)),
                case["voice_count"],
                "case {index}"
            );
        }
        Ok(())
    }
}
