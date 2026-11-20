//! Typed fields of native 80-byte positioning objects. Constructor80354370,
//! definition copy80333d38..80333df0 and attenuation setters80354590/a0.
//! Array handles are borrowed from definitions; cached attenuation is released
//! explicitly by the runtime resource host, not by an invented deep clone.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug)]
pub struct Storage {
    pub byte11: u8,
    pub scalar_bits: [u32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Array {
    pub handle: Option<u32>,
    pub count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub path_id: u32,
    pub kind: u32,
    pub attenuation_id: u32,
    pub spatialized: u8,
    pub byte11: u8,
    /// Native+14/+18/+1c/+20. Constructor leaves these allocator bits intact.
    pub scalar_bits: [u32; 4],
    pub position_bits: [u32; 3],
    /// Native+30..+33: path flag, dynamic, follow and looping bytes.
    pub flags: [u8; 4],
    pub path_mode: u32,
    pub transition: i32,
    pub vertices: Array,
    pub paths: Array,
    pub cached_attenuation: Option<u32>,
}
impl State {
    pub fn new(storage: Storage) -> Self {
        Self {
            path_id: 0,
            kind: 0,
            attenuation_id: 0,
            spatialized: 0,
            byte11: storage.byte11,
            scalar_bits: storage.scalar_bits,
            position_bits: [0, 0, 1f32.to_bits()],
            flags: [0, 1, 1, 0],
            path_mode: 0,
            transition: 0,
            vertices: Array {
                handle: None,
                count: 0,
            },
            paths: Array {
                handle: None,
                count: 0,
            },
            cached_attenuation: None,
        }
    }
    /// Native copy retains all field values, including the cached reference and
    /// borrowed array identities. Reference ownership must follow the native
    /// host's rules; this operation does not AddRef or duplicate arrays.
    pub fn copy_definition(&mut self, definition: &Self) {
        self.clone_from(definition);
    }
    pub fn apply_attenuation_values(&mut self, values: [f32; 2]) {
        self.scalar_bits[2] = values[0].to_bits();
        self.scalar_bits[3] = values[1].to_bits();
    }
    pub fn initialization_view(&self) -> super::audio_voice_init::Position {
        super::audio_voice_init::Position {
            attenuation_id: self.attenuation_id,
            path_id: self.path_id,
            mode: self.kind,
            path_enabled: self.paths.handle.is_some(),
            force_path: self.path_mode & 4 != 0,
        }
    }
    /// Base destructor803543f0 releases only the cached attenuation resource.
    /// Path arrays are borrowed. Caller frees the position afterward; this
    /// operation, like the native destructor, must not be called twice.
    pub fn release_cached(&self, host: &mut impl ResourceControl) -> Result<()> {
        if let Some(resource) = self.cached_attenuation {
            host.release_attenuation(resource)?;
        }
        Ok(())
    }
}
pub trait ResourceControl {
    fn release_attenuation(&mut self, resource: u32) -> Result<()>;
}

struct Slot {
    state: State,
    destroyed: bool,
}
#[derive(Default)]
pub struct Positions {
    definitions: BTreeMap<u32, State>,
    live: BTreeMap<u32, Slot>,
    next: u32,
}
impl Positions {
    pub fn register_definition(&mut self, id: u32, state: State) -> Result<()> {
        ensure!(
            self.definitions.get(&id).is_none_or(|old| old == &state),
            "Changing live positioning definition {id:08x} requires native invalidation"
        );
        self.definitions.entry(id).or_insert(state);
        Ok(())
    }
    pub fn allocate(&mut self, storage: Storage) -> Result<u32> {
        self.next = self
            .next
            .checked_add(1)
            .context("Position handle space exhausted")?;
        self.live.insert(
            self.next,
            Slot {
                state: State::new(storage),
                destroyed: false,
            },
        );
        Ok(self.next)
    }
    pub fn get(&self, id: u32) -> Result<&State> {
        let slot = self.live.get(&id).context("Unknown position handle")?;
        ensure!(!slot.destroyed, "Position was destroyed");
        Ok(&slot.state)
    }
    pub fn get_mut(&mut self, id: u32) -> Result<&mut State> {
        let slot = self.live.get_mut(&id).context("Unknown position handle")?;
        ensure!(!slot.destroyed, "Position was destroyed");
        Ok(&mut slot.state)
    }
    pub fn copy_definition(&mut self, id: u32, definition: u32) -> Result<()> {
        let definition = self
            .definitions
            .get(&definition)
            .context("Unknown positioning definition")?
            .clone();
        self.get_mut(id)?.copy_definition(&definition);
        Ok(())
    }
    pub fn destroy(&mut self, id: u32, host: &mut impl ResourceControl) -> Result<()> {
        let slot = self.live.get_mut(&id).context("Unknown position handle")?;
        ensure!(!slot.destroyed, "Position was already destroyed");
        slot.destroyed = true;
        slot.state.release_cached(host)
    }
    pub fn free(&mut self, id: u32) -> Result<()> {
        ensure!(
            self.live
                .get(&id)
                .context("Unknown position handle")?
                .destroyed,
            "Freeing position before resource destruction"
        );
        self.live.remove(&id);
        Ok(())
    }
    pub fn live_count(&self) -> usize {
        self.live.len()
    }
}
pub trait Nodes {
    fn node(&mut self, id: u32) -> Result<super::audio_voice_position::Node>;
    fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()>;
}
/// Concrete storage adapter for the recovered acquisition flow. Heap/pool
/// pressure admission and definition-array lifetime remain backend services.
pub struct Runtime<'a, H> {
    pub positions: &'a mut Positions,
    pub nodes: &'a mut H,
    pub storage: Storage,
}
impl<H: Nodes> super::audio_voice_position::Control for Runtime<'_, H> {
    type Position = u32;
    fn node(&mut self, id: u32) -> Result<super::audio_voice_position::Node> {
        self.nodes.node(id)
    }
    fn allocate_position(&mut self) -> Result<Option<u32>> {
        Ok(Some(self.positions.allocate(self.storage)?))
    }
    fn copy_position(&mut self, position: u32, definition: u32) -> Result<()> {
        self.positions.copy_definition(position, definition)
    }
    fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()> {
        self.nodes.resolve_pan(node, object, voice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Released {
        calls: Vec<u32>,
        fail: bool,
    }
    impl ResourceControl for Released {
        fn release_attenuation(&mut self, resource: u32) -> Result<()> {
            self.calls.push(resource);
            ensure!(!self.fail, "Injected attenuation release error");
            Ok(())
        }
    }
    #[test]
    fn original_position_constructor_copy_setters_and_cached_release() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_position_state.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let fill = case["fill"].as_u64().unwrap() as u8;
            let mut state = State::new(Storage {
                byte11: fill,
                scalar_bits: [u32::from_be_bytes([fill; 4]); 4],
            });
            assert_eq!(
                serde_json::to_value(&state)?,
                case["constructed"],
                "case {index}"
            );
            let definition: State = serde_json::from_value(case["definition"].clone())?;
            state.copy_definition(&definition);
            assert_eq!(
                serde_json::to_value(&state)?,
                case["copied"],
                "case {index}"
            );
            state.apply_attenuation_values([2.5, 7.25]);
            assert_eq!(
                serde_json::to_value(&state)?,
                case["updated"],
                "case {index}"
            );
            let mut released = Released {
                calls: Vec::new(),
                fail: false,
            };
            state.release_cached(&mut released)?;
            assert_eq!(json!(released.calls), case["release_calls"]);
            assert_eq!(serde_json::to_value(&state)?, case["after_release"]);
        }
        Ok(())
    }
    #[test]
    fn acquisition_uses_concrete_position_storage_and_releases_cached_resource_once() -> Result<()>
    {
        struct Source;
        impl Nodes for Source {
            fn node(&mut self, _: u32) -> Result<super::super::audio_voice_position::Node> {
                Ok(super::super::audio_voice_position::Node {
                    parent: None,
                    override_positioning: true,
                    spatial: Some(17),
                })
            }
            fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()> {
                assert_eq!((node, object, voice), (17, 42, 7));
                Ok(())
            }
        }
        let storage = Storage {
            byte11: 0xcd,
            scalar_bits: [0xcdcdcdcd; 4],
        };
        let mut positions = Positions::default();
        let mut definition = State::new(storage);
        definition.kind = 2;
        definition.path_id = 123;
        definition.attenuation_id = 55;
        definition.path_mode = 4;
        definition.paths = Array {
            handle: Some(88),
            count: 3,
        };
        definition.vertices = Array {
            handle: Some(89),
            count: 4,
        };
        positions.register_definition(17, definition.clone())?;
        let mut id = None;
        super::super::audio_voice_position::acquire(
            Some(17),
            &mut id,
            42,
            0,
            7,
            &mut Runtime {
                positions: &mut positions,
                nodes: &mut Source,
                storage,
            },
        )?;
        let id = id.unwrap();
        assert_eq!(positions.get(id)?, &definition);
        let view = positions.get(id)?.initialization_view();
        assert_eq!((view.mode, view.path_id, view.attenuation_id), (2, 123, 55));
        assert!(view.path_enabled && view.force_path);
        positions.get_mut(id)?.cached_attenuation = Some(99);
        positions.get_mut(id)?.apply_attenuation_values([2.5, 7.25]);
        assert_eq!(positions.get(id)?.paths, definition.paths);
        assert_eq!(positions.get(id)?.vertices, definition.vertices);
        let mut released = Released {
            calls: Vec::new(),
            fail: true,
        };
        assert!(positions.destroy(id, &mut released).is_err());
        assert!(positions.destroy(id, &mut released).is_err());
        assert_eq!(released.calls, [99]);
        assert!(positions.get(id).is_err());
        positions.free(id)?;
        assert_eq!(positions.live_count(), 0);
        assert!(positions.free(id).is_err());
        let next = positions.allocate(storage)?;
        assert_ne!(next, id);
        assert!(positions.free(next).is_err());
        Ok(())
    }
}
