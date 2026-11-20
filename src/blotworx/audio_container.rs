//! Version46 actor-mixer and random/sequence container definitions.
//! Native readers8031f760/8032a880. Decoding is not playlist execution: random
//! history, global/per-object ownership, transitions and continuous playback
//! must be handled by the audio runtime, never flattened into a first child.
use super::{positioning::Positioning, NodePrefix, NodeTail, Object, VoiceSettings};
use anyhow::{ensure, Context, Result};

pub struct Common<'a> {
    pub prefix: NodePrefix<'a>,
    pub position: Positioning,
    pub voices: VoiceSettings,
    pub tail: NodeTail,
}
impl<'a> Common<'a> {
    fn parse(bytes: &'a [u8], feedback: bool) -> Result<(Self, &'a [u8])> {
        let prefix = NodePrefix::parse(bytes)?;
        let (position, voices, bytes) = prefix.positioning_voice_settings()?;
        let (tail, bytes) = NodeTail::parse(bytes, feedback)?;
        Ok((
            Self {
                prefix,
                position,
                voices,
                tail,
            },
            bytes,
        ))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub loops: u16,
    pub transition: [f32; 3],
    pub avoid_repeat: u16,
    pub transition_mode: u8,
    pub random_mode: u8,
    pub mode: u8,
    /// The native reader advances past this byte without loading it.
    pub unused_weight: u8,
    pub reset_playlist: u8,
    pub restart_backward: u8,
    pub continuous: u8,
    pub global: u8,
}
fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at.checked_add(4).context("Container offset overflow")?)
            .context("Truncated container integer")?
            .try_into()
            .unwrap(),
    ))
}
impl Settings {
    pub fn parse(bytes: &[u8]) -> Result<(Self, &[u8])> {
        ensure!(bytes.len() >= 24, "Truncated random/sequence settings");
        let transition = [
            f32::from_bits(word(bytes, 2)?),
            f32::from_bits(word(bytes, 6)?),
            f32::from_bits(word(bytes, 10)?),
        ];
        ensure!(
            transition.iter().all(|v| v.is_finite()),
            "Non-finite random/sequence transition"
        );
        Ok((
            Self {
                loops: u16::from_be_bytes([bytes[0], bytes[1]]),
                transition,
                avoid_repeat: u16::from_be_bytes([bytes[14], bytes[15]]),
                transition_mode: bytes[16],
                random_mode: bytes[17],
                mode: bytes[18],
                unused_weight: bytes[19],
                reset_playlist: bytes[20],
                restart_backward: bytes[21],
                continuous: bytes[22],
                global: bytes[23],
            },
            &bytes[24..],
        ))
    }
}
fn children(bytes: &[u8]) -> Result<(Vec<u32>, &[u8])> {
    let count = word(bytes, 0)? as usize;
    let bytes = &bytes[4..];
    ensure!(count <= bytes.len() / 4, "Truncated container children");
    let list = (0..count)
        .map(|i| word(bytes, i * 4))
        .collect::<Result<Vec<_>>>()?;
    Ok((list, &bytes[count * 4..]))
}
#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistItem {
    pub sound: u32,
    pub weight: u8,
}
pub struct RandomSequence<'a> {
    pub common: Common<'a>,
    pub settings: Settings,
    pub children: Vec<u32>,
    pub playlist: Vec<PlaylistItem>,
}
pub struct ActorMixer<'a> {
    pub common: Common<'a>,
    pub children: Vec<u32>,
}
/// Class6, EM1 loader8032ea44..8032f1d8. Definitions retain authored order;
/// runtime lookup merges groups (duplicate members are errors) and replaces
/// repeated node parameters.
pub struct Switch<'a> {
    pub common: Common<'a>,
    pub group_kind: u32,
    pub group: u32,
    pub default: u32,
    pub continuous: bool,
    pub children: Vec<u32>,
    pub groups: Vec<SwitchGroup>,
    pub parameters: Vec<SwitchParameter>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchGroup {
    pub id: u32,
    pub nodes: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchParameter {
    pub node: u32,
    pub first_only: bool,
    pub continue_playback: bool,
    pub on_switch: u32,
    pub fade_out: i32,
    pub fade_in: i32,
}
impl SwitchParameter {
    /// The native loader preserves the low three allocator bits; only these
    /// five bits belong to the serialized flags. Do not invent their meaning.
    pub fn native_flags(&self, storage: u8) -> u8 {
        (storage & 7)
            | ((self.on_switch as u8 & 7) << 5)
            | if self.first_only { 16 } else { 0 }
            | if self.continue_playback { 8 } else { 0 }
    }
}
impl Switch<'_> {
    pub fn runtime_groups(&self) -> Vec<SwitchGroup> {
        let mut output: Vec<SwitchGroup> = Vec::new();
        for group in &self.groups {
            let index = output
                .iter()
                .position(|entry| entry.id == group.id)
                .unwrap_or_else(|| {
                    output.push(SwitchGroup {
                        id: group.id,
                        nodes: Vec::new(),
                    });
                    output.len() - 1
                });
            for &node in &group.nodes {
                if !output[index].nodes.contains(&node) {
                    output[index].nodes.push(node);
                }
            }
        }
        output
    }
    pub fn runtime_parameters(&self) -> Vec<SwitchParameter> {
        let mut output: Vec<SwitchParameter> = Vec::new();
        for parameter in &self.parameters {
            if let Some(previous) = output.iter_mut().find(|entry| entry.node == parameter.node) {
                *previous = parameter.clone();
            } else {
                output.push(parameter.clone());
            }
        }
        output
    }
}
impl<'a> Object<'a> {
    pub fn unmodulated_layer_children(&self, feedback: bool) -> Result<Vec<u32>> {
        ensure!(self.kind == 9, "Object is not a layer container");
        let (_, bytes) = Common::parse(self.payload, feedback)?;
        let (children, layers) = children(bytes)?;
        // The Global bank's thinner impact container retains a child list
        // followed by zero modulation layers. Layer curves remain separate.
        ensure!(layers == [0; 4], "Authored audio layer modulation is not connected");
        Ok(children)
    }
    pub fn switch_container(&self, feedback: bool) -> Result<Switch<'a>> {
        ensure!(self.kind == 6, "Object is not a switch container");
        let (common, bytes) = Common::parse(self.payload, feedback)?;
        ensure!(bytes.len() >= 13, "Truncated switch settings");
        let group_kind = word(bytes, 0)?;
        let group = word(bytes, 4)?;
        let default = word(bytes, 8)?;
        let continuous = bytes[12] != 0;
        let (child_ids, mut bytes) = children(&bytes[13..])?;
        let count = word(bytes, 0)? as usize;
        bytes = &bytes[4..];
        ensure!(count <= bytes.len() / 8, "Truncated switch groups");
        let mut groups = Vec::with_capacity(count);
        let mut memberships =
            std::collections::BTreeMap::<u32, std::collections::BTreeSet<u32>>::new();
        for _ in 0..count {
            let id = word(bytes, 0)?;
            let (nodes, remaining) = children(&bytes[4..])?;
            ensure!(
                nodes.iter().all(|&node| node != 0),
                "Native switch group rejects zero node ID"
            );
            let membership = memberships.entry(id).or_default();
            for &node in &nodes {
                ensure!(
                    membership.insert(node),
                    "Native switch group rejects duplicate node ID (status57)"
                );
            }
            groups.push(SwitchGroup { id, nodes });
            bytes = remaining;
        }
        let count = word(bytes, 0)? as usize;
        bytes = &bytes[4..];
        ensure!(
            count <= bytes.len() / 18 && count * 18 == bytes.len(),
            "Truncated or trailing switch parameters"
        );
        let mut parameters = Vec::with_capacity(count);
        for entry in bytes.chunks_exact(18) {
            parameters.push(SwitchParameter {
                node: word(entry, 0)?,
                first_only: entry[4] != 0,
                continue_playback: entry[5] != 0,
                on_switch: word(entry, 6)?,
                fade_out: word(entry, 10)? as i32,
                fade_in: word(entry, 14)? as i32,
            });
        }
        Ok(Switch {
            common,
            group_kind,
            group,
            default,
            continuous,
            children: child_ids,
            groups,
            parameters,
        })
    }
    pub fn random_sequence(&self, feedback: bool) -> Result<RandomSequence<'a>> {
        ensure!(self.kind == 5, "Object is not a random/sequence container");
        let (common, bytes) = Common::parse(self.payload, feedback)?;
        let (settings, bytes) = Settings::parse(bytes)?;
        let (children, bytes) = children(bytes)?;
        ensure!(bytes.len() >= 2, "Missing random/sequence playlist count");
        let count = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        let bytes = &bytes[2..];
        ensure!(
            count <= bytes.len() / 5,
            "Truncated random/sequence playlist"
        );
        ensure!(count * 5 == bytes.len(), "Trailing random/sequence data");
        let playlist = (0..count)
            .map(|i| {
                Ok(PlaylistItem {
                    sound: word(bytes, i * 5)?,
                    weight: bytes[i * 5 + 4],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(RandomSequence {
            common,
            settings,
            children,
            playlist,
        })
    }
    pub fn actor_mixer(&self, feedback: bool) -> Result<ActorMixer<'a>> {
        ensure!(self.kind == 7, "Object is not an actor-mixer");
        let (common, bytes) = Common::parse(self.payload, feedback)?;
        let (children, bytes) = children(bytes)?;
        ensure!(bytes.is_empty(), "Trailing actor-mixer data");
        Ok(ActorMixer { common, children })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Result<serde_json::Value> {
        Ok(serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_containers.json"
        ))?)
    }
    fn bytes(value: &serde_json::Value) -> Vec<u8> {
        let hex = value.as_str().unwrap();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    fn common() -> Vec<u8> {
        vec![0; 66 + 1 + 7 + 5]
    }
    #[test]
    fn original_switch_loader_preserves_groups_replacement_flags_and_consumption() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_switch.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let encoded = bytes(&case["encoded"]);
            let mut payload = common();
            payload.extend(&encoded);
            let object = Object {
                kind: 6,
                payload: &payload,
            };
            if case["status"] != 1 {
                assert_eq!(case["status"], 57);
                assert!(object.switch_container(false).is_err());
                continue;
            }
            let switch = object.switch_container(false)?;
            assert_eq!(case["status"], 1);
            assert_eq!(case["consumed"], encoded.len());
            assert_eq!(case["group_kind"], switch.group_kind);
            assert_eq!(case["group"], switch.group);
            assert_eq!(case["default"], switch.default);
            assert_eq!(case["continuous"], switch.continuous);
            assert_eq!(case["children"], serde_json::json!(switch.children));
            assert_eq!(
                case["groups"],
                serde_json::json!(switch
                    .runtime_groups()
                    .iter()
                    .map(|group| serde_json::json!({"id":group.id,"nodes":group.nodes}))
                    .collect::<Vec<_>>())
            );
            assert_eq!(case["parameters"],serde_json::json!(switch.runtime_parameters().iter().map(|p|serde_json::json!({"node":p.node,"fade_out":p.fade_out,"fade_in":p.fade_in,"flags":p.native_flags(0)})).collect::<Vec<_>>()));
            for end in 0..payload.len() {
                assert!(
                    Object {
                        kind: 6,
                        payload: &payload[..end]
                    }
                    .switch_container(false)
                    .is_err(),
                    "Accepted truncated switch at {end}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_switch_containers_decode_and_mickey_steps_use_authored_groups() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let directory = std::path::Path::new(&root).join("Audio/Wii");
        let mut count = 0;
        let mut footsteps = 0;
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("bnk"))
            {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let bank = super::super::Bank::parse(&bytes)?;
            for (&id, object) in &bank.objects {
                if object.kind == 6 {
                    let switch = object
                        .switch_container(bank.feedback)
                        .with_context(|| format!("{} switch {id:08x}", path.display()))?;
                    let _ = switch.runtime_groups();
                    let _ = switch.runtime_parameters();
                    count += 1;
                }
            }
            if path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Global.bnk"))
            {
                for event in ["Play_sfx_mickey_run", "Play_sfx_mickey_walk"] {
                    let ids = bank.named_event_actions(event)?;
                    ensure!(ids.len() == 1, "Unexpected footstep action count");
                    let action = bank.objects[&ids[0]].action()?;
                    let switch = bank.objects[&action.target].switch_container(bank.feedback)?;
                    ensure!(
                        !switch.children.is_empty() && !switch.groups.is_empty(),
                        "Footstep container lost its authored choices"
                    );
                    footsteps += 1;
                }
            }
        }
        ensure!(
            count > 0 && footsteps == 2,
            "Original switch/footstep coverage missing"
        );
        println!(
            "Decoded {count} bank-local switch containers; verified both Mickey footstep groups"
        );
        Ok(())
    }
    #[test]
    fn settings_match_original_loaded_fields_and_consumption() -> Result<()> {
        let fixture = fixture()?;
        for case in fixture["headers"].as_array().unwrap() {
            let encoded = bytes(&case["encoded"]);
            let (settings, remaining) = Settings::parse(&encoded)?;
            assert!(remaining.is_empty());
            assert_eq!(case["consumed"], encoded.len());
            assert_eq!(case["loops"], settings.loops);
            assert_eq!(case["avoid_repeat"], settings.avoid_repeat);
            for (i, value) in settings.transition.iter().enumerate() {
                assert_eq!(case["transition_bits"][i], value.to_bits());
            }
            for (name, value) in [
                ("mode", settings.mode),
                ("transition_mode", settings.transition_mode),
                ("random_mode", settings.random_mode),
                ("reset_playlist", settings.reset_playlist),
                ("restart_backward", settings.restart_backward),
                ("continuous", settings.continuous),
                ("global_scope", settings.global),
            ] {
                assert_eq!(case[name], value);
            }
            assert_eq!(settings.unused_weight, 99);
            for n in 0..encoded.len() {
                assert!(Settings::parse(&encoded[..n]).is_err());
            }
        }
        Ok(())
    }
    #[test]
    fn children_and_playlists_preserve_native_input_order_and_byte_weights() -> Result<()> {
        let fixture = fixture()?;
        for case in fixture["children"].as_array().unwrap() {
            let encoded = bytes(&case["encoded"]);
            let mut payload = common();
            payload.extend(&encoded);
            let mixer = Object {
                kind: 7,
                payload: &payload,
            }
            .actor_mixer(false)?;
            assert_eq!(serde_json::json!(mixer.children), case["children"]);
            assert_eq!(case["consumed"], encoded.len());
            for n in 0..payload.len() {
                assert!(Object {
                    kind: 7,
                    payload: &payload[..n]
                }
                .actor_mixer(false)
                .is_err());
            }
        }
        for case in fixture["playlists"].as_array().unwrap() {
            let encoded = bytes(&case["encoded"]);
            let mut payload = common();
            let mut settings = [0; 24];
            settings[18] = case["mode"].as_u64().unwrap() as u8;
            payload.extend(settings);
            payload.extend([0; 4]);
            payload.extend(&encoded);
            let container = Object {
                kind: 5,
                payload: &payload,
            }
            .random_sequence(false)?;
            let pairs = container
                .playlist
                .iter()
                .map(|item| [item.sound, item.weight as u32])
                .collect::<Vec<_>>();
            assert_eq!(serde_json::json!(pairs), case["playlist"]);
            assert_eq!(case["consumed"], encoded.len());
            assert_eq!(
                case["weighted"],
                container.playlist.iter().any(|item| item.weight != 50)
            );
            for n in 0..payload.len() {
                assert!(Object {
                    kind: 5,
                    payload: &payload[..n]
                }
                .random_sequence(false)
                .is_err());
            }
            payload.push(0);
            assert!(Object {
                kind: 5,
                payload: &payload
            }
            .random_sequence(false)
            .is_err());
        }
        Ok(())
    }
    #[test]
    fn rtpc_headers_and_points_match_original_callback_arguments() -> Result<()> {
        let fixture = fixture()?;
        for case in fixture["rtpc"].as_array().unwrap() {
            let encoded = bytes(&case["encoded"]);
            let mut payload = vec![0; 3];
            payload.extend(&encoded);
            payload.extend([0; 6]);
            let sound = super::super::SoundTail::parse(&payload, false)?;
            assert_eq!(sound.rtpc.len(), 1);
            let rtpc = &sound.rtpc[0];
            assert_eq!(rtpc.plugin, word(&encoded, 2)?);
            assert_eq!(rtpc.rendered, encoded[6]);
            assert_eq!(rtpc.control, 0x11223344);
            assert_eq!(rtpc.parameter, word(&encoded, 11)?);
            assert_eq!(rtpc.curve, 0x55667788);
            assert_eq!(rtpc.scaling, 2);
            assert_eq!(case["consumed"], encoded.len());
            for call in case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["kind"] != "changed")
            {
                assert_eq!(call["control"], rtpc.control);
                assert_eq!(call["parameter"], rtpc.parameter);
                assert_eq!(call["curve"], rtpc.curve);
                assert_eq!(call["scaling"], rtpc.scaling);
                assert_eq!(call["count"], rtpc.points.len());
                let native = bytes(&call["points"]);
                for (point, native) in rtpc.points.iter().zip(native.chunks_exact(12)) {
                    assert_eq!(point.x.to_bits(), word(native, 0)?);
                    assert_eq!(point.y.to_bits(), word(native, 4)?);
                    assert_eq!(point.interpolation, word(native, 8)?);
                }
            }
            for n in 0..payload.len() {
                assert!(super::super::SoundTail::parse(&payload[..n], false).is_err());
            }
        }
        // The first variable-length curve can leave less than one complete
        // header, even when the initial fixed-header minimum check passes.
        let mut malformed = vec![0; 3];
        malformed.extend(2u16.to_be_bytes());
        let mut header = [0; 20];
        header[19] = 2;
        malformed.extend(header);
        malformed.extend([0; 28]);
        assert!(NodeTail::parse(&malformed, false).is_err());
        Ok(())
    }
}
