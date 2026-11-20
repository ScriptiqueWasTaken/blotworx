//! Base voice resource binding8035b250 and release8035b3b0.
//! Template/descriptor lookup, clone, attach, detach and destruction are services.
use anyhow::Result;
#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub kind: u8,
    pub id: u32,
    pub template: Option<u32>,
}
#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub id: u32,
    pub template: Option<u32>,
    pub word: u32,
    pub flag: u8,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct State {
    pub node_id: u32,
    pub node_resource: Option<u32>,
    pub node_word: u32,
    pub node_flag: u8,
    pub source_id: u32,
    pub source_resource: Option<u32>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            node_id: u32::MAX,
            node_resource: None,
            node_word: 0,
            node_flag: 0,
            source_id: u32::MAX,
            source_resource: None,
        }
    }
}
pub trait Control {
    fn source(&mut self, source: u32) -> Result<Source>;
    fn clone_resource(&mut self, template: u32) -> Result<Option<u32>>;
    fn attach(&mut self, template: u32, resource: u32) -> Result<()>;
    fn descriptor(&mut self, node: u32, object: u32) -> Result<Descriptor>;
    fn node_flag(&mut self, node: u32, object: u32) -> Result<u32>;
    fn detach(&mut self, resource: u32) -> Result<()>;
    fn destroy(&mut self, resource: u32) -> Result<()>;
}
pub fn initialize(
    state: &mut State,
    flags_db: &mut u8,
    node: u32,
    source: u32,
    object: u32,
    host: &mut impl Control,
) -> Result<u32> {
    let source = host.source(source)?;
    if source.kind == 2 {
        let Some(template) = source.template else {
            return Ok(2);
        };
        let Some(resource) = host.clone_resource(template)? else {
            return Ok(2);
        };
        state.source_id = source.id;
        state.source_resource = Some(resource);
        host.attach(template, resource)?;
    }
    let descriptor = host.descriptor(node, object)?;
    if descriptor.id != u32::MAX {
        // The original dereferences this template before its later null test.
        // Diagnose malformed host state rather than manufacturing success.
        let template = descriptor
            .template
            .ok_or_else(|| anyhow::anyhow!("Valid voice binding descriptor has no template"))?;
        if let Some(resource) = host.clone_resource(template)? {
            state.node_word = descriptor.word;
            state.node_id = descriptor.id;
            state.node_resource = Some(resource);
            state.node_flag = descriptor.flag;
            host.attach(template, resource)?;
        }
    }
    *flags_db = (*flags_db & 0x7f) | ((host.node_flag(node, object)? as u8 & 1) << 7);
    Ok(1)
}
pub fn release(state: &mut State, host: &mut impl Control) -> Result<()> {
    let mut first = None;
    for (id, resource) in [
        (&mut state.node_id, &mut state.node_resource),
        (&mut state.source_id, &mut state.source_resource),
    ] {
        if let Some(handle) = *resource {
            for result in [host.detach(handle), host.destroy(handle)] {
                if let Err(error) = result {
                    if first.is_none() {
                        first = Some(error);
                    }
                }
            }
            *id = u32::MAX;
            *resource = None;
        }
    }
    if let Some(error) = first {
        Err(error)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host {
        config: Value,
        calls: Vec<Value>,
        fail_attach: bool,
        fail_detach: bool,
        missing_template: bool,
    }
    impl Control for Host {
        fn source(&mut self, _: u32) -> Result<Source> {
            self.calls.push(json!({"op":"source"}));
            Ok(Source {
                kind: self.config["kind"].as_u64().unwrap() as u8,
                id: 55,
                template: if self.config["template_present"] == true {
                    Some(0x72040000)
                } else {
                    None
                },
            })
        }
        fn clone_resource(&mut self, template: u32) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"clone","template":template}));
            let (field, resource) = if template == 0x72040000 {
                ("source_clone", 0x72060000)
            } else {
                assert_eq!(template, 0x72050000);
                ("node_clone", 0x72070000)
            };
            Ok(if self.config[field] == true {
                Some(resource)
            } else {
                None
            })
        }
        fn attach(&mut self, template: u32, resource: u32) -> Result<()> {
            self.calls
                .push(json!({"op":"attach","template":template,"resource":resource}));
            anyhow::ensure!(!self.fail_attach, "Injected attach error");
            Ok(())
        }
        fn descriptor(&mut self, _: u32, _: u32) -> Result<Descriptor> {
            self.calls.push(json!({"op":"descriptor"}));
            Ok(Descriptor {
                id: if self.config["descriptor_present"] == true {
                    44
                } else {
                    u32::MAX
                },
                template: if self.missing_template {
                    None
                } else {
                    Some(0x72050000)
                },
                word: 123,
                flag: 255,
            })
        }
        fn node_flag(&mut self, _: u32, _: u32) -> Result<u32> {
            self.calls.push(json!({"op":"node_flag"}));
            Ok(self.config["node_flag"].as_u64().unwrap() as u32)
        }
        fn detach(&mut self, resource: u32) -> Result<()> {
            self.calls.push(json!({"op":"detach","resource":resource}));
            anyhow::ensure!(!self.fail_detach, "Injected detach error");
            Ok(())
        }
        fn destroy(&mut self, resource: u32) -> Result<()> {
            self.calls.push(json!({"op":"destroy","resource":resource}));
            Ok(())
        }
    }
    impl super::super::audio_voice_create::Control for Host {
        fn retain_object(&mut self, _: u32) {}
        fn acquire_node(&mut self, _: u32) {}
        fn link_voice(&mut self, _: u32, _: u32) -> Option<u32> {
            None
        }
        fn activate_object(&mut self, _: u32) {}
    }
    fn unexpected<T>() -> Result<T> {
        anyhow::bail!("Unexpected initialization service after explicit effects failure")
    }
    impl super::super::audio_voice_init::Control for Host {
        fn initialize_parameters(&mut self, _: u32) -> Result<u32> {
            anyhow::bail!("Constructor did not use recovered bindings")
        }
        fn initialize_effects(&mut self, _: u32) -> Result<u32> {
            anyhow::bail!("Constructor did not use recovered RTPC stage")
        }
        fn positioning(&mut self, _: u32, _: u32, _: u32) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"positioning"}));
            Ok(None)
        }
        fn position(&mut self, _: u32) -> Result<super::super::audio_voice_init::Position> {
            unexpected()
        }
        fn cached_attenuation(&mut self, _: u32) -> Result<Option<u32>> {
            unexpected()
        }
        fn acquire_attenuation(&mut self, _: u32) -> Result<Option<u32>> {
            unexpected()
        }
        fn cache_attenuation(&mut self, _: u32, _: Option<u32>) -> Result<()> {
            unexpected()
        }
        fn attenuation_values(&mut self, _: u32) -> Result<[f32; 2]> {
            unexpected()
        }
        fn apply_attenuation_values(&mut self, _: u32, _: [f32; 2]) -> Result<()> {
            unexpected()
        }
        fn attenuation_bindings(
            &mut self,
            _: u32,
        ) -> Result<Vec<super::super::audio_voice_init::Binding>> {
            unexpected()
        }
        fn subscribe(
            &mut self,
            _: u32,
            _: u32,
            _: super::super::audio_voice_init::Binding,
        ) -> Result<u32> {
            unexpected()
        }
        fn allocate_path(&mut self) -> Result<Option<u32>> {
            unexpected()
        }
        fn node_path_state(&mut self, _: u32) -> Result<u32> {
            unexpected()
        }
        fn initialize_path(&mut self, _: u32, _: u32, _: u32) -> Result<u32> {
            unexpected()
        }
        fn release_path(&mut self, _: u32) -> Result<()> {
            unexpected()
        }
        fn attach_path(&mut self, _: u32, _: u32) -> Result<u32> {
            unexpected()
        }
        fn set_path_node(&mut self, _: u32, _: u32) -> Result<()> {
            unexpected()
        }
        fn set_path_playing_id(&mut self, _: u32, _: u32) -> Result<()> {
            unexpected()
        }
        fn acquire_media(&mut self, _: u32, _: u32) -> Result<[u32; 2]> {
            self.calls.push(json!({"op":"media"}));
            Ok([0x12340000, 4096])
        }
    }
    impl super::super::audio_voice_rtpc::Control for Host {
        fn node_records(
            &mut self,
            _: u32,
        ) -> Result<Option<std::sync::Arc<[super::super::audio_voice_rtpc::Record]>>> {
            self.calls.push(json!({"op":"node_records"}));
            Ok(if self.config["rtpc_statuses"].is_array() {
                Some(std::sync::Arc::from(vec![
                    super::super::audio_voice_rtpc::Record {
                        id: 44,
                        rendered: 0,
                        words: [11, 22, 33, 44, 55, 66],
                    },
                ]))
            } else {
                None
            })
        }
        fn source_records(
            &mut self,
            _: u32,
        ) -> Result<Option<std::sync::Arc<[super::super::audio_voice_rtpc::Record]>>> {
            self.calls.push(json!({"op":"source_records"}));
            Ok(if self.config["rtpc_statuses"].is_array() {
                Some(std::sync::Arc::from(vec![
                    super::super::audio_voice_rtpc::Record {
                        id: 55,
                        rendered: 1,
                        words: [111, 122, 133, 144, 155, 166],
                    },
                ]))
            } else {
                None
            })
        }
        fn subscribe_resource(
            &mut self,
            resource: u32,
            object: u32,
            words: [u32; 6],
            kind: u32,
        ) -> Result<u32> {
            let statuses = self.config["rtpc_statuses"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Unexpected subscription"))?;
            let index = self
                .calls
                .iter()
                .filter(|call| call["op"] == "subscribe_resource")
                .count();
            let status = statuses[index % statuses.len()].as_u64().unwrap() as u32;
            self.calls.push(json!({"op":"subscribe_resource","resource":resource,"object":object,"words":words,"kind":kind}));
            Ok(status)
        }
    }
    #[test]
    fn original_binding_status_payload_flag_and_release_order() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_bindings.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut host = Host {
                config: case["config"].clone(),
                calls: Vec::new(),
                fail_attach: false,
                fail_detach: false,
                missing_template: false,
            };
            let mut state = State::default();
            let mut flags_db = host.config["initial_db"].as_u64().unwrap() as u8;
            assert_eq!(
                initialize(
                    &mut state,
                    &mut flags_db,
                    0x72010000,
                    0x72020000,
                    0x72030000,
                    &mut host
                )? as u64,
                case["status"].as_u64().unwrap(),
                "case {index}"
            );
            assert_eq!(
                serde_json::to_value(&state)?,
                case["initialized"],
                "case {index}"
            );
            assert_eq!(flags_db as u64, case["flags_db"].as_u64().unwrap());
            assert_eq!(json!(host.calls), case["initialize_calls"], "case {index}");
            host.calls.clear();
            release(&mut state, &mut host)?;
            assert_eq!(
                serde_json::to_value(&state)?,
                case["released"],
                "case {index}"
            );
            assert_eq!(json!(host.calls), case["release_calls"], "case {index}");
        }
        Ok(())
    }
    #[test]
    fn binding_errors_keep_acquired_resources_available_for_cleanup() -> Result<()> {
        let mut host = Host {
            config: json!({"kind":2,"template_present":true,"source_clone":true,"descriptor_present":true,"node_clone":true,"node_flag":1}),
            calls: Vec::new(),
            fail_attach: true,
            fail_detach: false,
            missing_template: false,
        };
        let mut state = State::default();
        let mut flags = 0;
        assert!(initialize(&mut state, &mut flags, 1, 2, 3, &mut host).is_err());
        assert_eq!(state.source_resource, Some(0x72060000));
        host.fail_detach = true;
        assert!(release(&mut state, &mut host).is_err());
        assert!(state.source_resource.is_none());
        assert_eq!(
            host.calls.last(),
            Some(&json!({"op":"destroy","resource":0x72060000u32}))
        );
        host.fail_attach = false;
        host.fail_detach = false;
        host.missing_template = true;
        host.config["kind"] = json!(0);
        assert!(initialize(&mut state, &mut flags, 1, 2, 3, &mut host)
            .unwrap_err()
            .to_string()
            .contains("no template"));
        Ok(())
    }
    #[test]
    fn constructor_binding_bridge_synchronizes_termination_targets_even_on_host_error() -> Result<()>
    {
        use super::super::audio_voice_create::{self, Storage};
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_create.json"
        ))?;
        let mut host = Host {
            config: json!({"kind":2,"template_present":true,"source_clone":true,"descriptor_present":true,"node_clone":true,"node_flag":1}),
            calls: Vec::new(),
            fail_attach: false,
            fail_detach: false,
            missing_template: false,
        };
        let mut voice = audio_voice_create::construct(
            serde_json::from_value(fixture["cases"][0]["input"].clone())?,
            Storage {
                flags60: 0,
                flags_db: 0,
                media_size: 0,
            },
            &mut host,
        );
        assert_eq!(voice.initialize_bindings(&mut host)?, 1);
        assert_eq!(
            voice.lifecycle.rtpc_targets,
            [Some(0x72070000), Some(0x72060000)]
        );
        assert_eq!(voice.flags_db, 0x80);
        voice.release_bindings(&mut host)?;
        assert_eq!(voice.lifecycle.rtpc_targets, [None; 2]);
        host.fail_attach = true;
        assert!(voice.initialize_bindings(&mut host).is_err());
        assert_eq!(voice.lifecycle.rtpc_targets, [None, Some(0x72060000)]);
        host.fail_detach = true;
        assert!(voice.release_bindings(&mut host).is_err());
        assert_eq!(voice.lifecycle.rtpc_targets, [None; 2]);
        Ok(())
    }
    #[test]
    fn constructed_voice_initialization_runs_recovered_bindings_after_playing_admission(
    ) -> Result<()> {
        use super::super::{
            audio_ingress::{Context, Registration},
            audio_playing::Playing,
            audio_voice_create::{self, Storage},
        };
        let constructors: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_create.json"
        ))?;
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_bindings.json"
        ))?;
        // Constructor clears bit7. Use reference cases whose incoming flags
        // are valid immediately after construction; bit6 comes from word+d0.
        for case in fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["config"]["initial_db"].as_u64().unwrap() < 128)
        {
            let mut host = Host {
                config: case["config"].clone(),
                calls: Vec::new(),
                fail_attach: false,
                fail_detach: false,
                missing_template: false,
            };
            let mut input: audio_voice_create::Input =
                serde_json::from_value(constructors["cases"][0]["input"].clone())?;
            input.word_d0 = if host.config["initial_db"].as_u64().unwrap() & 0x40 != 0 {
                1
            } else {
                0
            };
            let mut voice = audio_voice_create::construct(
                input,
                Storage {
                    flags60: 0,
                    flags_db: host.config["initial_db"].as_u64().unwrap() as u8,
                    media_size: 0,
                },
                &mut host,
            );
            let mut playing = Playing::default();
            playing.register(Registration {
                event: 17,
                context: Context {
                    object: 42,
                    playing_id: 7,
                    words: [0; 3],
                },
                callback: 1,
                cookie: 0,
                flags: 0x10001,
            });
            assert_eq!(
                voice.initialize([0; 2], &mut playing, &mut host)? as u64,
                case["status"].as_u64().unwrap()
            );
            let mut expected = case["initialize_calls"].as_array().unwrap().clone();
            if case["status"] == 1 {
                expected.extend([
                    json!({"op":"node_records"}),
                    json!({"op":"source_records"}),
                    json!({"op":"positioning"}),
                    json!({"op":"media"}),
                ]);
                assert_eq!(voice.lifecycle.voice.media, [0x12340000, 4096]);
            }
            assert_eq!(host.calls, expected);
            assert_eq!(serde_json::to_value(&voice.bindings)?, case["initialized"]);
            assert_eq!(voice.flags_db as u64, case["flags_db"].as_u64().unwrap());
            assert_eq!(voice.lifecycle.voice.callback_flags, 0x10001);
            assert_eq!(playing.record(7).unwrap().voices, 1);
            assert_eq!(playing.record(7).unwrap().actions, 1);
            assert_eq!(
                voice.lifecycle.rtpc_targets,
                [voice.bindings.node_resource, voice.bindings.source_resource]
            );
        }
        Ok(())
    }
    #[test]
    fn constructed_voice_rtpc_status_controls_positioning_and_media_without_early_subscription_exit(
    ) -> Result<()> {
        use super::super::{
            audio_playing::Playing,
            audio_voice_create::{self, Storage},
        };
        let constructors: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_create.json"
        ))?;
        for statuses in [[2, 1], [1, 15]] {
            let mut host = Host {
                config: json!({"kind":2,"template_present":true,"source_clone":true,"descriptor_present":true,"node_clone":true,"node_flag":1,"rtpc_statuses":statuses}),
                calls: Vec::new(),
                fail_attach: false,
                fail_detach: false,
                missing_template: false,
            };
            let mut voice = audio_voice_create::construct(
                serde_json::from_value(constructors["cases"][0]["input"].clone())?,
                Storage {
                    flags60: 0,
                    flags_db: 0,
                    media_size: 0,
                },
                &mut host,
            );
            assert_eq!(
                voice.initialize([0; 2], &mut Playing::default(), &mut host)?,
                statuses[1] as u32
            );
            let subscriptions = host
                .calls
                .iter()
                .filter(|call| call["op"] == "subscribe_resource")
                .collect::<Vec<_>>();
            assert_eq!(subscriptions.len(), 2);
            assert_eq!(subscriptions[0]["resource"], 0x72070000u32);
            assert_eq!(subscriptions[1]["resource"], 0x72060000u32);
            assert_eq!(
                host.calls.iter().any(|call| call["op"] == "media"),
                statuses[1] == 1
            );
            assert_eq!(
                host.calls.iter().any(|call| call["op"] == "positioning"),
                statuses[1] == 1
            );
            assert_eq!(
                voice.lifecycle.rtpc_targets,
                [Some(0x72070000), Some(0x72060000)]
            );
        }
        Ok(())
    }
}
