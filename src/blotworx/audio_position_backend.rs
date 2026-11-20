//! Concrete positioning side of the shared voice initializer. Bank-local node
//! lookup, acquisition and state access are recovered code; attenuation, pan,
//! paths, media and the remaining resource services belong to the backend.
use super::{
    audio_bank::Bank,
    audio_position_state::{self, Positions, State, Storage},
    audio_voice_bindings as bindings,
    audio_voice_init::{self, Binding, Position},
    audio_voice_rtpc as rtpc,
};
use anyhow::Result;
pub trait Services: audio_position_state::Nodes + audio_position_state::ResourceControl {
    /// Native80354650 forwards the path records/mode/looping from this state.
    /// Rust path entries use vertex indices, so the backend also needs the
    /// definition's vertex-array identity rather than a native memory pointer.
    /// node_state is the opaque803383e0 result, not a numeric path mode.
    fn initialize_path_data(&mut self, position: &State, path: u32, node_state: u32)
        -> Result<u32>;
}
pub struct Positioned<'a, 'b, H> {
    pub bank: &'a Bank<'b>,
    pub positions: &'a mut Positions,
    pub host: &'a mut H,
    pub storage: Storage,
}
impl<H: audio_voice_init::Control + Services> audio_voice_init::Control for Positioned<'_, '_, H> {
    fn initialize_parameters(&mut self, voice: u32) -> Result<u32> {
        self.host.initialize_parameters(voice)
    }
    fn initialize_effects(&mut self, voice: u32) -> Result<u32> {
        self.host.initialize_effects(voice)
    }
    fn positioning(&mut self, node: u32, object: u32, voice: u32) -> Result<Option<u32>> {
        let mut position = None;
        let result = self.bank.acquire_voice_position(
            node,
            &mut position,
            object,
            0,
            voice,
            &mut audio_position_state::Runtime {
                positions: self.positions,
                nodes: self.host,
                storage: self.storage,
            },
        );
        if let Err(error) = result {
            if let Some(id) = position {
                let destroyed = self.positions.destroy(id, self.host);
                let freed = self.positions.free(id);
                for result in [destroyed, freed] {
                    if let Err(cleanup) = result {
                        return Err(error.context(format!(
                            "Position acquisition cleanup also failed: {cleanup:#}"
                        )));
                    }
                }
            }
            return Err(error);
        }
        Ok(position)
    }
    fn position(&mut self, id: u32) -> Result<Position> {
        Ok(self.positions.get(id)?.initialization_view())
    }
    fn cached_attenuation(&mut self, id: u32) -> Result<Option<u32>> {
        Ok(self.positions.get(id)?.cached_attenuation)
    }
    fn cache_attenuation(&mut self, id: u32, attenuation: Option<u32>) -> Result<()> {
        self.positions.get_mut(id)?.cached_attenuation = attenuation;
        Ok(())
    }
    fn apply_attenuation_values(&mut self, id: u32, values: [f32; 2]) -> Result<()> {
        self.positions.get_mut(id)?.apply_attenuation_values(values);
        Ok(())
    }
    fn acquire_attenuation(&mut self, id: u32) -> Result<Option<u32>> {
        self.host.acquire_attenuation(id)
    }
    fn attenuation_values(&mut self, id: u32) -> Result<[f32; 2]> {
        self.host.attenuation_values(id)
    }
    fn attenuation_bindings(&mut self, id: u32) -> Result<Vec<Binding>> {
        self.host.attenuation_bindings(id)
    }
    fn subscribe(&mut self, voice: u32, object: u32, binding: Binding) -> Result<u32> {
        self.host.subscribe(voice, object, binding)
    }
    fn allocate_path(&mut self) -> Result<Option<u32>> {
        self.host.allocate_path()
    }
    fn node_path_state(&mut self, node: u32) -> Result<u32> {
        self.host.node_path_state(node)
    }
    fn initialize_path(&mut self, id: u32, path: u32, mode: u32) -> Result<u32> {
        self.host
            .initialize_path_data(self.positions.get(id)?, path, mode)
    }
    fn release_path(&mut self, path: u32) -> Result<()> {
        self.host.release_path(path)
    }
    fn attach_path(&mut self, path: u32, voice: u32) -> Result<u32> {
        self.host.attach_path(path, voice)
    }
    fn set_path_node(&mut self, path: u32, node: u32) -> Result<()> {
        self.host.set_path_node(path, node)
    }
    fn set_path_playing_id(&mut self, path: u32, id: u32) -> Result<()> {
        self.host.set_path_playing_id(path, id)
    }
    fn acquire_media(&mut self, source: u32, voice: u32) -> Result<[u32; 2]> {
        self.host.acquire_media(source, voice)
    }
}
impl<H: bindings::Control> bindings::Control for Positioned<'_, '_, H> {
    fn source(&mut self, source: u32) -> Result<bindings::Source> {
        self.host.source(source)
    }
    fn clone_resource(&mut self, template: u32) -> Result<Option<u32>> {
        self.host.clone_resource(template)
    }
    fn attach(&mut self, template: u32, resource: u32) -> Result<()> {
        self.host.attach(template, resource)
    }
    fn descriptor(&mut self, node: u32, object: u32) -> Result<bindings::Descriptor> {
        self.host.descriptor(node, object)
    }
    fn node_flag(&mut self, node: u32, object: u32) -> Result<u32> {
        self.host.node_flag(node, object)
    }
    fn detach(&mut self, resource: u32) -> Result<()> {
        self.host.detach(resource)
    }
    fn destroy(&mut self, resource: u32) -> Result<()> {
        self.host.destroy(resource)
    }
}
impl<H: rtpc::Control> rtpc::Control for Positioned<'_, '_, H> {
    fn node_records(&mut self, node: u32) -> Result<Option<std::sync::Arc<[rtpc::Record]>>> {
        self.host.node_records(node)
    }
    fn source_records(&mut self, node: u32) -> Result<Option<std::sync::Arc<[rtpc::Record]>>> {
        self.host.source_records(node)
    }
    fn subscribe_resource(
        &mut self,
        resource: u32,
        object: u32,
        words: [u32; 6],
        kind: u32,
    ) -> Result<u32> {
        self.host.subscribe_resource(resource, object, words, kind)
    }
}
#[cfg(test)]
mod tests {
    use super::super::audio_attenuation_resources::{Resources, Storage as AttStorage};
    use super::super::{
        audio_ingress::{Context, Registration},
        audio_playing::Playing,
        audio_position_paths::{self, Arrays},
        audio_voice_create::{self, Input},
    };
    use super::*;
    struct Host<'a> {
        arrays: &'a Arrays,
        resources: &'a mut Resources,
        pan_fail: bool,
        released: Vec<u32>,
        path_data: Option<State>,
        calls: Vec<&'static str>,
    }
    impl audio_position_state::Nodes for Host<'_> {
        fn node(&mut self, _: u32) -> Result<super::super::audio_voice_position::Node> {
            anyhow::bail!("Bank lookup was bypassed")
        }
        fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()> {
            assert_eq!((node, object, voice), (17, 42, 7));
            self.calls.push("pan");
            anyhow::ensure!(!self.pan_fail, "Injected pan error");
            Ok(())
        }
    }
    impl audio_position_state::ResourceControl for Host<'_> {
        fn release_attenuation(&mut self, resource: u32) -> Result<()> {
            self.released.push(resource);
            self.resources.release(resource)?;
            Ok(())
        }
    }
    impl Services for Host<'_> {
        fn initialize_path_data(&mut self, data: &State, path: u32, mode: u32) -> Result<u32> {
            assert_eq!((path, mode), (99, 5));
            assert_eq!(data.path_id, 17);
            assert_eq!(
                self.arrays.vertices(data.vertices.handle.unwrap())?[1].duration,
                -150
            );
            assert_eq!(
                self.arrays.paths(data.paths.handle.unwrap())?[0]
                    .as_ref()
                    .unwrap()
                    .range,
                [0.25, 0.75]
            );
            self.path_data = Some(data.clone());
            self.calls.push("path_data");
            Ok(1)
        }
    }
    fn unexpected<T>() -> Result<T> {
        anyhow::bail!("A concrete positioning operation reached the supplied stub")
    }
    impl audio_voice_init::Control for Host<'_> {
        fn initialize_parameters(&mut self, _: u32) -> Result<u32> {
            unexpected()
        }
        fn initialize_effects(&mut self, _: u32) -> Result<u32> {
            unexpected()
        }
        fn positioning(&mut self, _: u32, _: u32, _: u32) -> Result<Option<u32>> {
            unexpected()
        }
        fn position(&mut self, _: u32) -> Result<Position> {
            unexpected()
        }
        fn cached_attenuation(&mut self, _: u32) -> Result<Option<u32>> {
            unexpected()
        }
        fn cache_attenuation(&mut self, _: u32, _: Option<u32>) -> Result<()> {
            unexpected()
        }
        fn apply_attenuation_values(&mut self, _: u32, _: [f32; 2]) -> Result<()> {
            unexpected()
        }
        fn acquire_attenuation(&mut self, id: u32) -> Result<Option<u32>> {
            assert_eq!(id, 55);
            self.calls.push("attenuation");
            self.resources.acquire(id)
        }
        fn attenuation_values(&mut self, id: u32) -> Result<[f32; 2]> {
            self.resources.values(id)
        }
        fn attenuation_bindings(&mut self, id: u32) -> Result<Vec<Binding>> {
            self.resources.bindings(id)
        }
        fn subscribe(&mut self, _: u32, _: u32, _: Binding) -> Result<u32> {
            unexpected()
        }
        fn allocate_path(&mut self) -> Result<Option<u32>> {
            self.calls.push("allocate_path");
            Ok(Some(99))
        }
        fn node_path_state(&mut self, node: u32) -> Result<u32> {
            assert_eq!(node, 17);
            Ok(5)
        }
        fn initialize_path(&mut self, _: u32, _: u32, _: u32) -> Result<u32> {
            unexpected()
        }
        fn release_path(&mut self, _: u32) -> Result<()> {
            unexpected()
        }
        fn attach_path(&mut self, path: u32, voice: u32) -> Result<u32> {
            assert_eq!((path, voice), (99, 7));
            self.calls.push("attach_path");
            Ok(1)
        }
        fn set_path_node(&mut self, path: u32, node: u32) -> Result<()> {
            assert_eq!((path, node), (99, 17));
            Ok(())
        }
        fn set_path_playing_id(&mut self, path: u32, id: u32) -> Result<()> {
            assert_eq!((path, id), (99, 7));
            Ok(())
        }
        fn acquire_media(&mut self, source: u32, voice: u32) -> Result<[u32; 2]> {
            assert_eq!((source, voice), (88, 7));
            self.calls.push("media");
            Ok([0x12340000, 4096])
        }
    }
    impl bindings::Control for Host<'_> {
        fn source(&mut self, source: u32) -> Result<bindings::Source> {
            assert_eq!(source, 88);
            Ok(bindings::Source {
                kind: 0,
                id: 0,
                template: None,
            })
        }
        fn clone_resource(&mut self, _: u32) -> Result<Option<u32>> {
            unexpected()
        }
        fn attach(&mut self, _: u32, _: u32) -> Result<()> {
            unexpected()
        }
        fn descriptor(&mut self, _: u32, _: u32) -> Result<bindings::Descriptor> {
            Ok(bindings::Descriptor {
                id: u32::MAX,
                template: None,
                word: 0,
                flag: 0,
            })
        }
        fn node_flag(&mut self, _: u32, _: u32) -> Result<u32> {
            Ok(0)
        }
        fn detach(&mut self, _: u32) -> Result<()> {
            unexpected()
        }
        fn destroy(&mut self, _: u32) -> Result<()> {
            unexpected()
        }
    }
    impl rtpc::Control for Host<'_> {
        fn node_records(&mut self, _: u32) -> Result<Option<std::sync::Arc<[rtpc::Record]>>> {
            Ok(None)
        }
        fn source_records(&mut self, _: u32) -> Result<Option<std::sync::Arc<[rtpc::Record]>>> {
            Ok(None)
        }
        fn subscribe_resource(&mut self, _: u32, _: u32, _: [u32; 6], _: u32) -> Result<u32> {
            unexpected()
        }
    }
    impl audio_voice_create::Control for Host<'_> {
        fn retain_object(&mut self, _: u32) {}
        fn acquire_node(&mut self, _: u32) {}
        fn link_voice(&mut self, _: u32, _: u32) -> Option<u32> {
            None
        }
        fn activate_object(&mut self, _: u32) {}
    }
    fn storage() -> Storage {
        Storage {
            byte11: 0,
            scalar_bits: [0; 4],
        }
    }
    fn payload(kind: u32) -> Vec<u8> {
        let mut bytes = vec![0; 66];
        bytes.push(1);
        bytes.extend([0; 12]);
        bytes.push(1);
        bytes.extend(kind.to_be_bytes());
        bytes.extend(55u32.to_be_bytes());
        bytes.push(1);
        if kind == 3 {
            bytes.push(1);
        } else {
            bytes.extend(3u32.to_be_bytes());
            bytes.push(1);
            bytes.extend((-150i32).to_be_bytes());
            bytes.push(1);
            bytes.extend(4u32.to_be_bytes());
            for i in 0..4 {
                for value in [i as f32, 0., 1.] {
                    bytes.extend(value.to_be_bytes());
                }
                bytes.extend((250i32 + i).to_be_bytes());
            }
            bytes.extend(2u32.to_be_bytes());
            for (first, count) in [(0u32, 2u32), (2, 2)] {
                bytes.extend(first.to_be_bytes());
                bytes.extend(count.to_be_bytes());
            }
            for value in [0.25f32, 0.75, 1.25, 1.75] {
                bytes.extend(value.to_be_bytes());
            }
        }
        bytes.extend([0; 7]);
        bytes.extend([0; 5]);
        bytes.extend(0u32.to_be_bytes());
        bytes
    }
    fn input() -> Input {
        Input {
            voice: 7,
            node: 17,
            source: 88,
            object: 42,
            context: [0, 0, 0, 0, 7, 0],
            parameters: [0; 17],
            word64: 0,
            priority: super::super::audio_sound::Priority {
                current: 50.,
                base: 50.,
                distance_offset: 0.,
            },
            flag: 0,
            word_d0: 0,
        }
    }
    fn attenuation_payload() -> Vec<u8> {
        let mut bytes = vec![1];
        for value in [90f32, 180., 2.5, 7.25] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([0, 255, 255, 255, 255, 1]);
        bytes.push(0);
        bytes.extend(2u16.to_be_bytes());
        for (x, y) in [(0f32, 0f32), (100., -6.)] {
            bytes.extend(x.to_be_bytes());
            bytes.extend(y.to_be_bytes());
            bytes.extend(4u32.to_be_bytes());
        }
        bytes.extend(0u16.to_be_bytes());
        bytes
    }
    #[test]
    fn provisioned_bank_positions_reach_constructed_voice_initialization_with_shared_path_data(
    ) -> Result<()> {
        for kind in [3, 2] {
            let payload = payload(kind);
            let attenuation = attenuation_payload();
            let bank = Bank {
                id: 7,
                language: 0,
                feedback: false,
                media: Default::default(),
                objects: std::collections::BTreeMap::from([
                    (
                        17,
                        super::super::audio_bank::Object {
                            kind: 7,
                            payload: &payload,
                        },
                    ),
                    (
                        55,
                        super::super::audio_bank::Object {
                            kind: 14,
                            payload: &attenuation,
                        },
                    ),
                ]),
            };
            let mut arrays = Arrays::default();
            let mut definition = bank
                .provision_voice_position(17, storage(), &mut arrays)?
                .unwrap();
            assert_eq!(definition.status, 1);
            let mut positions = Positions::default();
            positions.register_definition(17, definition.state.clone())?;
            let mut resources = Resources::default();
            let resource = resources
                .create_from_bank(
                    &bank,
                    55,
                    AttStorage {
                        cone_bits: [0; 4],
                        flags61: 0,
                    },
                )?
                .unwrap();
            let mut host = Host {
                arrays: &arrays,
                resources: &mut resources,
                pan_fail: false,
                released: Vec::new(),
                path_data: None,
                calls: Vec::new(),
            };
            let mut voice = audio_voice_create::construct(
                input(),
                audio_voice_create::Storage {
                    flags60: 0,
                    flags_db: 0,
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
                voice.initialize(
                    [0; 2],
                    &mut playing,
                    &mut Positioned {
                        bank: &bank,
                        positions: &mut positions,
                        host: &mut host,
                        storage: storage()
                    }
                )?,
                1
            );
            let id = voice.lifecycle.voice.positioning.unwrap();
            let state = positions.get(id)?;
            assert_eq!(state.kind, kind);
            assert_eq!(state.path_id, 17);
            assert_eq!(state.cached_attenuation, Some(resource));
            assert_eq!(
                state.scalar_bits[2..],
                [2.5f32.to_bits(), 7.25f32.to_bits()]
            );
            assert_eq!(host.resources.references(resource), Some(2));
            assert_eq!(state.vertices, definition.state.vertices);
            assert_eq!(state.paths, definition.state.paths);
            assert_eq!(playing.record(7).unwrap().voices, 1);
            assert_eq!(voice.lifecycle.voice.callback_flags, 0x10001);
            assert_eq!(voice.lifecycle.voice.media, [0x12340000, 4096]);
            if kind == 2 {
                assert_eq!(voice.lifecycle.voice.path, Some(99));
                assert_eq!(voice.lifecycle.voice.path_id, 17);
                assert!(host.path_data.is_some());
            } else {
                assert!(voice.lifecycle.voice.path.is_none());
                assert!(host.path_data.is_none());
            }
            assert_eq!(host.calls.first(), Some(&"pan"));
            assert_eq!(host.calls.last(), Some(&"media"));
            positions.destroy(id, &mut host)?;
            positions.free(id)?;
            assert_eq!(host.released, [resource]);
            assert_eq!(host.resources.references(resource), Some(1));
            assert_eq!(positions.live_count(), 0);
            drop(host);
            assert_eq!(resources.release(resource)?, 0);
            assert_eq!(resources.live_resources(), 0);
            audio_position_paths::clear_paths(&mut definition.state, &mut arrays)?;
            assert_eq!(arrays.live_arrays(), 0);
        }
        Ok(())
    }
    #[test]
    fn partial_position_acquisition_errors_destroy_and_free_storage() -> Result<()> {
        let payload = payload(3);
        let bank = Bank {
            id: 7,
            language: 0,
            feedback: false,
            media: Default::default(),
            objects: std::collections::BTreeMap::from([(
                17,
                super::super::audio_bank::Object {
                    kind: 7,
                    payload: &payload,
                },
            )]),
        };
        for registered in [false, true] {
            let arrays = Arrays::default();
            let mut positions = Positions::default();
            if registered {
                let mut definition = State::new(storage());
                definition.cached_attenuation = Some(99);
                positions.register_definition(17, definition)?;
            }
            let mut resources = Resources::default();
            let mut host = Host {
                arrays: &arrays,
                resources: &mut resources,
                pan_fail: true,
                released: Vec::new(),
                path_data: None,
                calls: Vec::new(),
            };
            let result = audio_voice_init::Control::positioning(
                &mut Positioned {
                    bank: &bank,
                    positions: &mut positions,
                    host: &mut host,
                    storage: storage(),
                },
                17,
                42,
                7,
            );
            assert!(result.is_err());
            assert_eq!(positions.live_count(), 0);
            assert_eq!(host.released, if registered { vec![99] } else { vec![] });
        }
        Ok(())
    }
}
