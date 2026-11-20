//! Lifecycle/payload subset of base voice constructor80357c40..80357ee4.
//! This does not reconstruct all parameter-binding/mixer fields or allocate a
//! derived source voice. Native node/object services are supplied.
use super::{audio_sound::Priority, audio_voice_init, audio_voice_term};
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Input {
    pub voice: u32,
    pub node: u32,
    pub source: u32,
    pub object: u32,
    /// Constructor r7: six words copied to voice+40..+54. Word4 is the
    /// playing ID used by initialization. Other words remain uninterpreted.
    pub context: [u32; 6],
    /// Constructor r8: seventeen words copied to voice+8c..+cc.
    pub parameters: [u32; 17],
    pub word64: u32,
    pub priority: Priority,
    pub flag: u8,
    pub word_d0: u32,
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::{json, Value};
    struct Host {
        calls: Vec<&'static str>,
        references: u32,
        active: u32,
        node_head: Option<u32>,
    }
    impl Control for Host {
        fn retain_object(&mut self, _: u32) {
            self.calls.push("retain_object");
            self.references = self.references.wrapping_add(1);
        }
        fn acquire_node(&mut self, _: u32) {
            self.calls.push("acquire_node");
        }
        fn link_voice(&mut self, _: u32, voice: u32) -> Option<u32> {
            self.calls.push("link_voice");
            self.node_head.replace(voice)
        }
        fn activate_object(&mut self, _: u32) {
            self.calls.push("activate_object");
            self.active = self.active.wrapping_add(1);
        }
    }
    #[test]
    fn original_constructor_lifecycle_payload_flags_and_ownership() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_create.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let fill = case["fill"].as_u64().unwrap() as u8;
            let previous = case["previous"].as_u64().unwrap() as u32;
            let mut host = Host {
                calls: Vec::new(),
                references: 3,
                active: 0,
                node_head: if previous == 0 { None } else { Some(previous) },
            };
            let constructed = construct(
                serde_json::from_value(case["input"].clone())?,
                Storage {
                    flags60: fill,
                    flags_db: fill,
                    media_size: u32::from_be_bytes([fill; 4]),
                },
                &mut host,
            );
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
            let mut expected = case["output"].clone();
            expected["lifecycle"]["path_flags"] = json!(0);
            let result = json!({"lifecycle":constructed.lifecycle,"context":constructed.context,"parameters":constructed.parameters,
                "word64":constructed.word64,"word_d0":constructed.word_d0,"priority_bits":[constructed.priority.current.to_bits(),constructed.priority.base.to_bits(),constructed.priority.distance_offset.to_bits()],
                "flags60":constructed.flags60,"flags_db":constructed.flags_db,"previous":constructed.previous_voice,
                "object_references":host.references,"object_active":host.active,"node_head":host.node_head.unwrap_or(0)});
            assert_eq!(result, expected, "case {index}");
        }
        Ok(())
    }
}
/// Constructor reads allocator contents in these fields. Do not claim their
/// preserved bits or untouched media-size word are defined by construction.
#[derive(Clone, Copy, Debug)]
pub struct Storage {
    pub flags60: u8,
    pub flags_db: u8,
    pub media_size: u32,
}
#[derive(Clone, Debug)]
pub struct Constructed {
    pub lifecycle: audio_voice_term::State,
    pub bindings: super::audio_voice_bindings::State,
    pub context: [u32; 6],
    pub parameters: [u32; 17],
    pub word64: u32,
    pub word_d0: u32,
    pub priority: Priority,
    pub flags60: u8,
    pub flags_db: u8,
    pub previous_voice: Option<u32>,
}
impl Constructed {
    pub fn initialize<
        H: audio_voice_init::Control
            + super::audio_voice_bindings::Control
            + super::audio_voice_rtpc::Control,
    >(
        &mut self,
        initial_path: [u32; 2],
        playing: &mut super::audio_playing::Playing,
        host: &mut H,
    ) -> anyhow::Result<u32> {
        let voice = &mut self.lifecycle.voice;
        let (node, source, object) = (voice.node, voice.source, voice.object);
        let bindings = &mut self.bindings;
        let flags_db = &mut self.flags_db;
        let result = audio_voice_init::initialize_with_stages(
            voice,
            initial_path,
            playing,
            host,
            |stage, _, host| match stage {
                audio_voice_init::Stage::Bindings => super::audio_voice_bindings::initialize(
                    bindings, flags_db, node, source, object, host,
                ),
                audio_voice_init::Stage::Subscriptions => {
                    super::audio_voice_rtpc::initialize(bindings, node, object, host)
                }
            },
        );
        self.lifecycle.rtpc_targets = [self.bindings.node_resource, self.bindings.source_resource];
        result
    }
    /// Keeps termination's RTPC targets synchronized even if an attach service
    /// reports a Rust error after a cloned resource has already been stored.
    pub fn initialize_bindings(
        &mut self,
        host: &mut impl super::audio_voice_bindings::Control,
    ) -> anyhow::Result<u32> {
        let voice = &self.lifecycle.voice;
        let result = super::audio_voice_bindings::initialize(
            &mut self.bindings,
            &mut self.flags_db,
            voice.node,
            voice.source,
            voice.object,
            host,
        );
        self.lifecycle.rtpc_targets = [self.bindings.node_resource, self.bindings.source_resource];
        result
    }
    pub fn release_bindings(
        &mut self,
        host: &mut impl super::audio_voice_bindings::Control,
    ) -> anyhow::Result<()> {
        let result = super::audio_voice_bindings::release(&mut self.bindings, host);
        self.lifecycle.rtpc_targets = [self.bindings.node_resource, self.bindings.source_resource];
        result
    }
}
/// Native operations are void for already-resolved valid node/object handles.
/// A backend must not invent fallible allocation here: allocation precedes this
/// constructor. It must provide actual reference/list/activation semantics.
pub trait Control {
    fn retain_object(&mut self, object: u32);
    fn acquire_node(&mut self, node: u32);
    fn link_voice(&mut self, node: u32, voice: u32) -> Option<u32>;
    fn activate_object(&mut self, object: u32);
}
pub fn construct(input: Input, storage: Storage, host: &mut impl Control) -> Constructed {
    let voice = audio_voice_init::State {
        voice: input.voice,
        node: input.node,
        source: input.source,
        object: input.object,
        playing_id: input.context[4],
        callback_flags: 0,
        positioning: None,
        path: None,
        path_id: 0,
        media: [0, storage.media_size],
    };
    let lifecycle = audio_voice_term::State {
        voice,
        count_flags: input.flag & 1,
        control_flags: 0,
        rtpc_targets: [None; 2],
        transition_users: [None; 2],
        path_flags: 0,
        parameter_array: None,
        resource18: None,
        resource14c: None,
    };
    host.retain_object(input.object);
    host.acquire_node(input.node);
    let previous_voice = host.link_voice(input.node, input.voice);
    host.activate_object(input.object);
    Constructed {
        lifecycle,
        bindings: super::audio_voice_bindings::State::default(),
        context: input.context,
        parameters: input.parameters,
        word64: input.word64,
        word_d0: input.word_d0,
        priority: input.priority,
        flags60: storage.flags60 & 0x3f,
        flags_db: (storage.flags_db & 0x3f) | if input.word_d0 != 0 { 0x40 } else { 0 },
        previous_voice,
    }
}
