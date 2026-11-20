//! EM1 Wii sound-bank framing. Payload semantics are deliberately separate.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[path = "audio_attenuation.rs"]
pub mod attenuation;
#[path = "audio_container.rs"]
pub mod container;
#[path = "audio_positioning.rs"]
pub mod positioning;
#[path = "audio_selection.rs"]
pub mod selection;
/// ASCII event-name path verified at DOL 0x80368f58..0x80369060:
/// lowercase A-Z, then FNV-1 (multiply before XOR). The original wide-name
/// conversion's non-ASCII/locale behavior is not yet reproduced.
pub fn event_id(name: &str) -> Result<u32> {
    ensure!(
        name.is_ascii() && !name.contains('\0'),
        "Non-ASCII or embedded-NUL audio names require original string conversion"
    );
    Ok(name.bytes().fold(0x811c9dc5u32, |hash, b| {
        hash.wrapping_mul(0x01000193) ^ b.to_ascii_lowercase() as u32
    }))
}
pub struct Object<'a> {
    pub kind: u32,
    pub payload: &'a [u8],
}
pub struct Bank<'a> {
    pub id: u32,
    pub language: u32,
    pub feedback: bool,
    pub objects: BTreeMap<u32, Object<'a>>,
    pub media: BTreeMap<u32, &'a [u8]>,
}
/// Version-46 codec source header. Node parameters are preserved, not applied.
/// Layout reference: wwiser/parser/wparser.py CAkBankMgr__LoadSource (<=46).
/// Retail dry-fire fixture verifies source ID, bank ID and absolute memory
/// range against DIDX, then decodes the resolved media. This does not validate
/// native playback, voice limits, bus routing or remaining node settings.
pub struct Source<'a> {
    pub plugin: u32,
    pub stream_type: u32,
    pub sample_rate: u32,
    pub format_bits: u32,
    pub media_id: u32,
    pub file_id: u32,
    pub memory: Option<(u32, u32)>,
    pub flags: u8,
    pub node_parameters: &'a [u8],
    /// Borrowed plugin initialization payload for native types2/5. Parsing it
    /// does not mean the corresponding source generator is implemented.
    pub plugin_parameters: Option<&'a [u8]>,
}
impl<'a> Object<'a> {
    pub fn source(&self) -> Result<Source<'a>> {
        ensure!(self.kind == 2, "Object is not a sound");
        let b = self.payload;
        let plugin = word(b, 0)?;
        let kind = plugin & 15;
        ensure!(
            matches!(kind, 0 | 1 | 2 | 5),
            "Unsupported source plugin type {kind}"
        );
        let stream_type = word(b, 4)?;
        ensure!(
            kind != 1 || stream_type <= 2,
            "Unknown codec source stream type {stream_type}"
        );
        let sample_rate = word(b, 8)?;
        let format_bits = word(b, 12)?;
        let media_id = word(b, 16)?;
        let file_id = word(b, 20)?;
        let (memory, pos) = if stream_type == 1 {
            (None, 24)
        } else {
            (Some((word(b, 24)?, word(b, 28)?)), 32)
        };
        let flags = *b.get(pos).context("Missing source flags")?;
        let (plugin_parameters, next) = if matches!(kind, 2 | 5) {
            let size = word(b, pos + 1)? as usize;
            let first = pos
                .checked_add(5)
                .context("Plugin parameter offset overflow")?;
            let end = first
                .checked_add(size)
                .context("Plugin parameter size overflow")?;
            (
                Some(
                    b.get(first..end)
                        .context("Truncated source plugin parameters")?,
                ),
                end,
            )
        } else {
            (None, pos + 1)
        };
        let node_parameters = b.get(next..).context("Missing sound node parameters")?;
        Ok(Source {
            plugin,
            stream_type,
            sample_rate,
            format_bits,
            media_id,
            file_id,
            memory,
            flags,
            node_parameters,
            plugin_parameters,
        })
    }
}
/// Common node prefix for nodes without attached FX. Later settings remain
/// explicit bytes until decoded; this is not a default playback configuration.
pub struct NodePrefix<'a> {
    pub override_fx: u8,
    pub bus: u32,
    pub parent: u32,
    pub priority: [u8; 4],
    pub ranges: [[f32; 3]; 4],
    pub state_group: u32,
    pub remaining: &'a [u8],
}
/// Version-46 advanced settings, without applying parent override rules.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct VoiceSettings {
    pub virtual_queue: u8,
    pub kill_newest: bool,
    pub maximum_instances: u16,
    pub below_threshold: u8,
    pub override_maximum: bool,
    pub override_virtual: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Parameters {
    pub id: u32,
    pub parent: u32,
    pub bus: u32,
    pub position: positioning::Positioning,
    pub voices: VoiceSettings,
}
impl<'a> NodePrefix<'a> {
    pub fn positioning_voice_settings(
        &self,
    ) -> Result<(positioning::Positioning, VoiceSettings, &'a [u8])> {
        positioning::parse(self.remaining)
    }
    /// Compatibility entry for the direct preview's inherited-source path.
    /// Full local positioning is decoded above; this preview cannot render it.
    pub fn inherited_positioning_voice_settings(&self) -> Result<(VoiceSettings, &'a [u8])> {
        let (position, voices, remaining) = self.positioning_voice_settings()?;
        ensure!(
            position == positioning::Positioning::Inherited,
            "Direct preview requires inherited source positioning"
        );
        Ok((voices, remaining))
    }
    pub fn parse(b: &'a [u8]) -> Result<Self> {
        ensure!(b.len() >= 66, "Truncated audio node prefix");
        ensure!(
            b[1] == 0,
            "Audio nodes with attached FX are not yet supported"
        );
        let mut ranges = [[0.; 3]; 4];
        for (i, values) in ranges.iter_mut().enumerate() {
            for (j, value) in values.iter_mut().enumerate() {
                *value = f32::from_bits(word(b, 14 + (i * 3 + j) * 4)?);
                ensure!(value.is_finite(), "Non-finite audio node parameter");
            }
        }
        Ok(Self {
            override_fx: b[0],
            bus: word(b, 2)?,
            parent: word(b, 6)?,
            priority: b[10..14].try_into()?,
            ranges,
            state_group: word(b, 62)?,
            remaining: &b[66..],
        })
    }
}
pub struct SoundTail {
    pub state_sync: u8,
    pub states: Vec<(u32, u8, u32)>,
    pub feedback: Option<(u32, Option<[f32; 6]>)>,
    pub rtpc: Vec<Rtpc>,
    pub loops: [i16; 3],
}
#[derive(Clone, Debug, PartialEq)]
pub struct RtpcPoint {
    pub x: f32,
    pub y: f32,
    pub interpolation: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Rtpc {
    pub plugin: u32,
    pub rendered: u8,
    pub control: u32,
    pub parameter: u32,
    pub curve: u32,
    pub scaling: u8,
    pub points: Vec<RtpcPoint>,
}
impl Rtpc {
    pub fn parse_list(mut b: &[u8]) -> Result<(Vec<Self>, &[u8])> {
        ensure!(b.len() >= 2, "Missing sound RTPC count");
        let count = u16::from_be_bytes([b[0], b[1]]) as usize;
        b = &b[2..];
        ensure!(count <= b.len() / 20, "Truncated node RTPC list");
        let mut rtpc = Vec::with_capacity(count);
        for _ in 0..count {
            ensure!(b.len() >= 20, "Truncated node RTPC header");
            let plugin = word(b, 0)?;
            let rendered = b[4];
            let control = word(b, 5)?;
            let parameter = word(b, 9)?;
            let curve = word(b, 13)?;
            let scaling = b[17];
            let count = u16::from_be_bytes([b[18], b[19]]) as usize;
            b = &b[20..];
            ensure!(count <= b.len() / 12, "Truncated node RTPC points");
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                let x = f32::from_bits(word(b, 0)?);
                let y = f32::from_bits(word(b, 4)?);
                ensure!(x.is_finite() && y.is_finite(), "Non-finite node RTPC point");
                points.push(RtpcPoint {
                    x,
                    y,
                    interpolation: word(b, 8)?,
                });
                b = &b[12..];
            }
            rtpc.push(Rtpc {
                plugin,
                rendered,
                control,
                parameter,
                curve,
                scaling,
                points,
            });
        }
        Ok((rtpc, b))
    }
}
/// Shared suffix of the native8033bb70 reader. The unconsumed bytes belong to
/// the concrete sound/container class, not to this common parameter block.
pub struct NodeTail {
    pub state_sync: u8,
    pub states: Vec<(u32, u8, u32)>,
    pub rtpc: Vec<Rtpc>,
    pub feedback: Option<(u32, Option<[f32; 6]>)>,
}
impl NodeTail {
    pub fn parse(mut b: &[u8], has_feedback: bool) -> Result<(Self, &[u8])> {
        ensure!(b.len() >= 3, "Truncated sound state header");
        let state_sync = b[0];
        let count = u16::from_be_bytes([b[1], b[2]]) as usize;
        b = &b[3..];
        let mut states = Vec::new();
        ensure!(count <= b.len() / 9, "Truncated sound state list");
        for _ in 0..count {
            states.push((word(b, 0)?, b[4], word(b, 5)?));
            b = &b[9..];
        }
        let (rtpc, remaining) = Rtpc::parse_list(b)?;
        b = remaining;
        let feedback = if has_feedback {
            let bus = word(b, 0)?;
            b = &b[4..];
            let values = if bus != 0 {
                let mut values = [0.; 6];
                for value in &mut values {
                    *value = f32::from_bits(word(b, 0)?);
                    b = &b[4..];
                    ensure!(value.is_finite(), "Non-finite feedback parameter");
                }
                Some(values)
            } else {
                None
            };
            Some((bus, values))
        } else {
            None
        };
        Ok((
            Self {
                state_sync,
                states,
                rtpc,
                feedback,
            },
            b,
        ))
    }
}
impl SoundTail {
    pub fn parse(b: &[u8], has_feedback: bool) -> Result<Self> {
        let (node, b) = NodeTail::parse(b, has_feedback)?;
        ensure!(b.len() == 6, "Missing or trailing sound loop parameters");
        let loops = std::array::from_fn(|i| i16::from_be_bytes([b[i * 2], b[i * 2 + 1]]));
        Ok(Self {
            state_sync: node.state_sync,
            states: node.states,
            rtpc: node.rtpc,
            feedback: node.feedback,
            loops,
        })
    }
}
/// Version-46 common action header. Type-specific parameters stay borrowed
/// until interpreted; a parsed header is not an executable action.
pub struct Action<'a> {
    pub kind: u32,
    pub target: u32,
    pub delay: [i32; 3],
    pub subsection_size: u32,
    pub parameters: &'a [u8],
}
pub struct PlayAction {
    pub transition: [i32; 3],
    pub fade_flags: u8,
    /// DOL 0x803535a0 advances the input by 16 bytes without reading them.
    pub reserved: [u8; 16],
    pub exceptions: Vec<u32>,
    pub bank: u32,
}
/// CAkActionMute's transition/exception payload. The class uses the same
/// 16-byte skipping reader as Play, but has no trailing Play bank ID.
/// CAkActionStop's inherited Active action payload. Object-scoped sound
/// execution is distinct from the global, all-object and exception modes.
pub struct StopAction {
    pub transition: [i32; 3],
    pub curve: u8,
    pub reserved: [u8; 16],
    pub exceptions: Vec<u32>,
}
impl StopAction {
    pub fn transition_time(&self, random: impl FnMut() -> u16) -> Result<i32> {
        transition_time(self.transition, random)
    }
}
pub struct MuteAction {
    pub transition: [i32; 3],
    pub curve: u8,
    pub reserved: [u8; 16],
    pub exceptions: Vec<u32>,
}
/// CAkActionSetVolume common loader80351fe0 and value reader80352510.
/// Mode remains raw; mixer/SIS execution must preserve its native semantics.
#[derive(Debug, PartialEq)]
pub struct VolumeAction {
    pub transition: [i32; 3],
    pub curve: u8,
    pub mode: u32,
    pub value: [f32; 3],
    pub exceptions: Vec<u32>,
}
impl VolumeAction {
    pub fn transition_time(&self,random:impl FnMut()->u16)->Result<i32>{transition_time(self.transition,random)}
    /// Value RNG precedes transition RNG in803521f0..80352288. Equal value
    /// modifiers skip rand; the native uses double division/multiplication.
    pub fn randomized_value(&self, mut random: impl FnMut() -> u16) -> Result<f32> {
        let [base, minimum, maximum] = self.value;
        let width = maximum - minimum;
        let offset = if width == 0. {
            0.
        } else {
            let sample = random();
            ensure!(sample <= 32767, "Audio RNG must provide a 15-bit sample");
            (sample as f64 / 32767. * width as f64) as f32
        };
        Ok(base + (offset + minimum))
    }
}
impl MuteAction {
    pub fn transition_time(&self, random: impl FnMut() -> u16) -> Result<i32> {
        transition_time(self.transition, random)
    }
}
impl PlayAction {
    /// DOL 0x803532f0: double arithmetic, add 0.5 then truncate, including
    /// inverted ranges. The original skips rand() when both modifiers match.
    pub fn transition_time(&self, mut random: impl FnMut() -> u16) -> Result<i32> {
        transition_time(self.transition, &mut random)
    }
}
fn transition_time(transition: [i32; 3], mut random: impl FnMut() -> u16) -> Result<i32> {
    let [base, minimum, maximum] = transition;
    let width = maximum.wrapping_sub(minimum);
    let offset = if width == 0 {
        0
    } else {
        let sample = random();
        ensure!(sample <= 32767, "Audio RNG must provide a 15-bit sample");
        (0.5 + (sample as f64 / 32767.) * width as f64) as i32
    };
    Ok(base.wrapping_add(offset).wrapping_add(minimum))
}
impl Action<'_> {
    pub(super) fn set_state(&self) -> Result<(u32, u32)> {
        ensure!(self.kind == 0x12020 && self.parameters.len() == 8, "Invalid Set State action");
        Ok((word(self.parameters, 0)?, word(self.parameters, 4)?))
    }
    pub fn is_float_parameter(&self) -> bool {
        matches!(self.kind, 0xa010 | 0xb010 | 0xa011 | 0xb011 | 0xe010 | 0xf010)
    }
    pub fn parameter(&self)->Result<VolumeAction> {
        ensure!(self.is_float_parameter(),"Unsupported float parameter {:08x}",self.kind);
        self.float_parameter()
    }
    pub fn global_volume(&self) -> Result<VolumeAction> {
        ensure!(
            matches!(self.kind, 0xa010 | 0xb010),
            "Unsupported global volume action type {:08x}",
            self.kind
        );
        self.float_parameter()
    }
    fn float_parameter(&self)->Result<VolumeAction> {
        let b = self.parameters;
        ensure!(b.len() >= 33, "Truncated volume action");
        let count = word(b, 29)? as usize;
        ensure!(
            count.checked_mul(4).and_then(|n| n.checked_add(33)) == Some(b.len()),
            "Malformed volume exception list or trailing bytes"
        );
        let value = [
            f32::from_bits(word(b, 17)?),
            f32::from_bits(word(b, 21)?),
            f32::from_bits(word(b, 25)?),
        ];
        ensure!(
            value.iter().all(|v| v.is_finite()),
            "Non-finite volume parameter"
        );
        Ok(VolumeAction {
            transition: [word(b, 0)? as i32, word(b, 4)? as i32, word(b, 8)? as i32],
            curve: b[12] & 31,
            mode: word(b, 13)?,
            value,
            exceptions: (0..count)
                .map(|i| word(b, 33 + i * 4))
                .collect::<Result<_>>()?,
        })
    }
    /// DOL 0x803533fc..0x80353428 shifts all three serialized delays by five.
    /// These are loaded units, not a claim about scheduler wall-clock units.
    pub fn loaded_delay(&self) -> [i32; 3] {
        self.delay.map(|v| v.wrapping_shl(5))
    }
    /// CAkAction delay getter DOL80353150. Uses the same native arithmetic
    /// as transition randomization, after converting the three loaded words.
    pub fn delay_samples(&self, random: impl FnMut() -> u16) -> Result<u32> {
        Ok(transition_time(self.loaded_delay(), random)? as u32)
    }
    /// Observed version-46 Play type. Other action types must be interpreted
    /// separately, not converted into a generic play request.
    pub fn play(&self) -> Result<PlayAction> {
        ensure!(
            self.kind == 0x4011,
            "Unsupported play action type {:08x}",
            self.kind
        );
        let b = self.parameters;
        ensure!(b.len() >= 37, "Truncated Play action");
        let transition = [word(b, 0)? as i32, word(b, 4)? as i32, word(b, 8)? as i32];
        let fade_flags = b[12];
        let reserved = b[13..29].try_into()?;
        let count = word(b, 29)? as usize;
        let bank_offset = count
            .checked_mul(4)
            .and_then(|n| n.checked_add(33))
            .context("Play exception size overflow")?;
        ensure!(
            bank_offset.checked_add(4) == Some(b.len()),
            "Malformed Play exception list or trailing bytes"
        );
        let exceptions = (0..count)
            .map(|i| word(b, 33 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        Ok(PlayAction {
            transition,
            fade_flags,
            reserved,
            exceptions,
            bank: word(b, bank_offset)?,
        })
    }
    /// DOL8034d110 reads transition words/curve,803535a0 skips16 bytes,
    /// and80353a60 reads the exception IDs. Preserve, do not interpret as Play.
    pub fn object_stop(&self) -> Result<StopAction> {
        ensure!(
            matches!(self.kind, 0x1010 | 0x1011),
            "Unsupported object stop action type {:08x}",
            self.kind
        );
        let b = self.parameters;
        ensure!(b.len() >= 33, "Truncated Stop action");
        let transition = [word(b, 0)? as i32, word(b, 4)? as i32, word(b, 8)? as i32];
        let curve = b[12] & 31;
        let reserved = b[13..29].try_into()?;
        let count = word(b, 29)? as usize;
        ensure!(
            count.checked_mul(4).and_then(|n| n.checked_add(33)) == Some(b.len()),
            "Malformed Stop exception list or trailing bytes"
        );
        let exceptions = (0..count)
            .map(|i| word(b, 33 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        Ok(StopAction {
            transition,
            curve,
            reserved,
            exceptions,
        })
    }
    pub fn global_mute(&self) -> Result<MuteAction> {
        ensure!(
            matches!(self.kind, 0x6010 | 0x7010),
            "Unsupported global node mute action type {:08x}",
            self.kind
        );
        let b = self.parameters;
        ensure!(b.len() >= 33, "Truncated Mute action");
        let transition = [word(b, 0)? as i32, word(b, 4)? as i32, word(b, 8)? as i32];
        // DOL8035203c packs only the low five bits into action+14.
        let curve = b[12] & 31;
        let reserved = b[13..29].try_into()?;
        let count = word(b, 29)? as usize;
        ensure!(
            count.checked_mul(4).and_then(|n| n.checked_add(33)) == Some(b.len()),
            "Malformed Mute exception list or trailing bytes"
        );
        let exceptions = (0..count)
            .map(|i| word(b, 33 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        Ok(MuteAction {
            transition,
            curve,
            reserved,
            exceptions,
        })
    }
}
impl<'a> Object<'a> {
    pub fn action(&self) -> Result<Action<'a>> {
        ensure!(self.kind == 3, "Object is not an action");
        let b = self.payload;
        let kind = word(b, 0)?;
        let target = word(b, 4)?;
        let delay = [word(b, 8)? as i32, word(b, 12)? as i32, word(b, 16)? as i32];
        let subsection_size = word(b, 20)?;
        let parameters = b.get(24..).context("Missing action parameters")?;
        ensure!(
            (subsection_size as usize) <= parameters.len(),
            "Truncated action subsection"
        );
        // Retail Play contains a bank ID beyond subsection_size. Preserve all
        // bytes rather than assuming that count covers the whole payload.
        Ok(Action {
            kind,
            target,
            delay,
            subsection_size,
            parameters,
        })
    }
}
fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at.checked_add(4).context("Bank offset overflow")?)
            .context("Truncated bank integer")?
            .try_into()
            .unwrap(),
    ))
}
impl<'a> Bank<'a> {
    /// Read one node without eagerly traversing ancestors. Native queries can
    /// stop at a local override even when an unused ancestor is unavailable.
    pub fn parameter(&self, id: u32) -> Result<Parameters> {
        let object = self
            .objects
            .get(&id)
            .with_context(|| format!("Missing audio parameter ancestor {id:08x}"))?;
        let bytes = match object.kind {
            2 => object.source()?.node_parameters,
            // Native derived readers8032a880/8032ea10/8031f760/80328510
            // share the common parameter reader and positioning virtual slots.
            5 | 6 | 7 | 9 => object.payload,
            kind => anyhow::bail!(
                "Audio parameter ancestor {id:08x} class {kind} needs its native reader"
            ),
        };
        let prefix = NodePrefix::parse(bytes)?;
        let (position, voices, _) = prefix.positioning_voice_settings()?;
        Ok(Parameters {
            id,
            parent: prefix.parent,
            bus: prefix.bus,
            position,
            voices,
        })
    }
    /// Decode the actual sound/container ancestry, including parent-owned
    /// positioning. Unsupported parent classes must not disappear into a 2D
    /// default. The caller supplies the same loaded-bank object view as Play.
    pub fn parameter_chain(&self, start: u32) -> Result<Vec<Parameters>> {
        let mut id = start;
        let mut seen = std::collections::BTreeSet::new();
        let mut chain = Vec::new();
        while id != 0 {
            ensure!(
                seen.insert(id),
                "Audio parameter ancestry cycle at {id:08x}"
            );
            let parameters = self.parameter(id)?;
            id = parameters.parent;
            chain.push(parameters);
        }
        ensure!(!chain.is_empty(), "Empty audio parameter ancestry");
        Ok(chain)
    }
    pub fn named_event_actions(&self, name: &str) -> Result<Vec<u32>> {
        self.event_actions(event_id(name)?)
    }
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let bank=super::audio_bank_framing::Bank::parse(bytes,super::audio_bank_framing::Format::Wii46)?;
        let mut objects=BTreeMap::new();
        for object in bank.objects {
            ensure!(objects.insert(object.id,Object {kind:object.kind,payload:object.payload}).is_none(),"Duplicate HIRC identity {:08x}",object.id);
        }
        let mut media=BTreeMap::new();
        for record in bank.media {
            ensure!(media.insert(record.id,record.bytes).is_none(),"Duplicate media identity {:08x}",record.id);
        }
        Ok(Self {id:word(bank.header,4)?,language:word(bank.header,8)?,feedback:word(bank.header,12)?&1!=0,objects,media})
    }
    /// Version 46 event records contain a u32 count followed by action IDs.
    pub fn event_actions(&self, id: u32) -> Result<Vec<u32>> {
        let object = self.objects.get(&id).context("Missing audio event")?;
        ensure!(object.kind == 4, "Object is not an event");
        let count = word(object.payload, 0)? as usize;
        ensure!(
            count.checked_mul(4).and_then(|n| n.checked_add(4)) == Some(object.payload.len()),
            "Malformed event action list"
        );
        (0..count)
            .map(|i| word(object.payload, 4 + i * 4))
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_source_headers_preserve_generator_parameters_and_node_boundary() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_source.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let hex = case["encoded"].as_str().unwrap();
            let mut bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let length = bytes.len();
            bytes.extend([0xaa, 0xbb]);
            let object = Object {
                kind: 2,
                payload: &bytes,
            };
            if case["status"] != 1 {
                assert!(object.source().is_err());
                continue;
            }
            let source = object.source()?;
            assert_eq!(source.node_parameters, [0xaa, 0xbb]);
            assert_eq!(case["consumed"].as_u64().unwrap() as usize, length);
            assert_eq!(source.plugin as u64, case["plugin"].as_u64().unwrap());
            assert_eq!(
                source.sample_rate as u64,
                case["sample_rate"].as_u64().unwrap()
            );
            assert_eq!(source.media_id as u64, case["media"].as_u64().unwrap());
            assert_eq!(source.file_id as u64, case["file"].as_u64().unwrap());
            let memory = source.memory.unwrap_or((0, 0));
            assert_eq!(serde_json::json!([memory.0, memory.1]), case["memory"]);
            assert_eq!(
                source.plugin_parameters.is_some(),
                matches!(source.plugin & 15, 2 | 5)
            );
            let parameters = source
                .plugin_parameters
                .unwrap_or(&[])
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            assert_eq!(parameters, case["parameter_bytes"]);
            for n in 0..length {
                assert!(
                    Object {
                        kind: 2,
                        payload: &bytes[..n]
                    }
                    .source()
                    .is_err(),
                    "Truncated source accepted at {n}: {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn header_only_bank_is_distinct_from_truncated_hierarchy() -> Result<()> {
        let mut bytes = b"BKHD".to_vec();
        bytes.extend(40u32.to_be_bytes());
        for word in [46u32, 7, 0, 0, 0, 0, 0, 0, 0, 0] {
            bytes.extend(word.to_be_bytes());
        }
        let bank = Bank::parse(&bytes)?;
        assert_eq!(bank.id, 7);
        assert!(bank.objects.is_empty() && bank.media.is_empty());
        bytes.extend(b"HIRC");
        bytes.extend(0u32.to_be_bytes());
        assert!(
            Bank::parse(&bytes).is_err(),
            "A present but truncated HIRC must not become an empty hierarchy"
        );
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_source.json"
        ))?;
        assert!(fixture["eof"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["bytes_read"] == 0 && c["read_status"] == 1 && c["bank_status"] == 1));
        Ok(())
    }
    #[test]
    fn native_container_readers_share_the_common_parameter_prefix() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_container_parameters.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(case["skip_identity_bytes"], 4);
            assert_eq!(case["position_loader"], "0x80336bb0");
            assert_eq!(case["advanced_loader"], "0x80336f10");
            let mut payload = vec![0u8; 66];
            payload.extend([0, 3, 0, 0, 1, 2, 1, 1]);
            let bank = Bank {
                id: 0,
                language: 0,
                feedback: false,
                objects: BTreeMap::from([(
                    1,
                    Object {
                        kind: case["kind"].as_u64().unwrap() as u32,
                        payload: &payload,
                    },
                )]),
                media: BTreeMap::new(),
            };
            let chain = bank.parameter_chain(1)?;
            assert_eq!(chain.len(), 1);
            assert_eq!(chain[0].voices.virtual_queue, 3);
            assert_eq!(chain[0].voices.maximum_instances, 1);
            assert_eq!(chain[0].voices.below_threshold, 2);
            assert!(chain[0].voices.override_virtual && chain[0].voices.override_maximum);
            assert_eq!(chain[0].position, positioning::Positioning::Inherited);
        }
        Ok(())
    }
    #[test]
    fn event_names_follow_native_ascii_casefold_and_fnv1() {
        assert_eq!(event_id("Play_sfx_ammo_dry").unwrap(), 0xd76ae37e);
        assert_eq!(
            event_id("PLAY_SFX_AMMO_DRY").unwrap(),
            event_id("play_sfx_ammo_dry").unwrap()
        );
        assert_eq!(event_id("").unwrap(), 0x811c9dc5);
        assert!(event_id("a\0b").is_err());
        assert!(event_id("é").is_err());
    }
    #[test]
    fn transition_randomization_matches_native_call_and_rounding_rules() {
        let mut play = PlayAction {
            transition: [10, 2, 2],
            fade_flags: 4,
            reserved: [0; 16],
            exceptions: vec![],
            bank: 0,
        };
        assert_eq!(
            play.transition_time(|| panic!("Equal endpoints consumed RNG"))
                .unwrap(),
            12
        );
        play.transition = [10, -2, 2];
        assert_eq!(play.transition_time(|| 0).unwrap(), 8);
        assert_eq!(play.transition_time(|| 32767).unwrap(), 12);
        play.transition = [0, 2, -2];
        assert_eq!(play.transition_time(|| 32767).unwrap(), -1);
        assert!(play.transition_time(|| 32768).is_err());
        play.transition = [i32::MAX, 1, 1];
        assert_eq!(play.transition_time(|| panic!()).unwrap(), i32::MIN);
    }
    #[test]
    fn action_header_preserves_signed_delays_and_rejects_truncation() {
        let mut b = Vec::new();
        for value in [0x4011u32, 7, (-1i32) as u32, (-20i32) as u32, 30, 0] {
            b.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            Object {
                kind: 3,
                payload: &b
            }
            .action()
            .unwrap()
            .delay,
            [-1, -20, 30]
        );
        assert_eq!(
            Object {
                kind: 3,
                payload: &b
            }
            .action()
            .unwrap()
            .loaded_delay(),
            [-32, -640, 960]
        );
        for n in 0..24 {
            assert!(Object {
                kind: 3,
                payload: &b[..n]
            }
            .action()
            .is_err());
        }
        b[23] = 1;
        assert!(Object {
            kind: 3,
            payload: &b
        }
        .action()
        .is_err());
        assert!(Object {
            kind: 4,
            payload: &b
        }
        .action()
        .is_err());
    }
    #[test]
    fn source_distinguishes_streamed_and_embedded_headers() {
        let mut b = Vec::new();
        for value in [0x50001u32, 1, 16000, 0x02400004, 123, 456] {
            b.extend_from_slice(&value.to_be_bytes());
        }
        b.extend_from_slice(&[2, 99]);
        let object = Object {
            kind: 2,
            payload: &b,
        };
        let source = object.source().unwrap();
        assert_eq!(source.memory, None);
        assert_eq!(source.flags, 2);
        assert_eq!(source.node_parameters, [99]);
        for n in 0..25 {
            assert!(Object {
                kind: 2,
                payload: &b[..n]
            }
            .source()
            .is_err());
        }
        b[7] = 0;
        assert!(Object {
            kind: 2,
            payload: &b
        }
        .source()
        .is_err());
        b[3] = 2;
        assert!(Object {
            kind: 2,
            payload: &b
        }
        .source()
        .is_err());
    }
    #[test]
    fn node_prefix_does_not_invent_defaults_for_unparsed_layouts() {
        let mut b = [0u8; 66];
        assert!(NodePrefix::parse(&b).is_ok());
        for n in 0..66 {
            assert!(NodePrefix::parse(&b[..n]).is_err());
        }
        b[1] = 1;
        assert!(NodePrefix::parse(&b).is_err());
        b[1] = 0;
        b[14..18].copy_from_slice(&f32::NAN.to_bits().to_be_bytes());
        assert!(NodePrefix::parse(&b).is_err());
    }
    #[test]
    fn malformed_bank_is_rejected() {
        for bytes in [
            &b""[..],
            &b"BKHD\0\0\0\x10"[..],
            &b"BKHD\0\0\0\x04\0\0\0\x2e"[..],
        ] {
            assert!(Bank::parse(bytes).is_err());
        }
    }
    #[test]
    fn media_ranges_are_relative_to_data_and_cannot_escape() {
        fn chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
            out.extend_from_slice(id);
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(body);
        }
        let make = |offset: u32, size: u32| {
            let mut out = Vec::new();
            chunk(
                &mut out,
                b"BKHD",
                &[0, 0, 0, 46, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            );
            chunk(&mut out, b"HIRC", &0u32.to_be_bytes());
            let mut record = 7u32.to_be_bytes().to_vec();
            record.extend_from_slice(&offset.to_be_bytes());
            record.extend_from_slice(&size.to_be_bytes());
            chunk(&mut out, b"DIDX", &record);
            chunk(&mut out, b"DATA", b"abcdef");
            out
        };
        let bytes = make(2, 3);
        let bank = Bank::parse(&bytes).unwrap();
        assert_eq!(bank.media[&7], b"cde");
        assert!(Bank::parse(&make(5, 2)).is_err());
        assert!(Bank::parse(&make(u32::MAX, u32::MAX)).is_err());
    }
    #[test]
    #[ignore = "requires imported EM1 BLOTWORX_TEST_DATA"]
    fn retail_global_dry_event_links_to_action() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        assert_eq!(bank.objects.len(), 1816);
        assert_eq!(bank.id, 0x57572dac);
        assert_eq!(bank.language, 0);
        assert!(!bank.feedback);
        assert_eq!(bank.media.len(), 186);
        let candidate = bank.media[&0x17d3ec7f];
        assert_eq!(candidate.len(), 6176);
        assert_eq!(&candidate[..4], b"RIFX");
        assert_eq!(&candidate[8..12], b"WAVE");
        assert_eq!(bank.event_actions(0xd76ae37e)?, [0x3d07767c]);
        assert_eq!(
            bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?,
            [0x3d07767c]
        );
        assert_eq!(bank.objects[&0x3d07767c].kind, 3);
        let action = bank.objects[&0x3d07767c].action()?;
        assert_eq!(action.kind, 0x4011);
        assert_eq!(action.target, 0x09075f8e);
        assert_eq!(action.delay, [0; 3]);
        assert_eq!(action.subsection_size, 33);
        assert_eq!(action.parameters.len(), 37);
        let play = action.play()?;
        assert_eq!(play.transition, [0; 3]);
        assert_eq!(play.fade_flags, 4);
        assert!(play.exceptions.is_empty());
        assert_eq!(play.bank, bank.id);
        assert_eq!(&play.reserved[..4], &1u32.to_be_bytes());
        assert_eq!(&play.reserved[4..], &[0u8; 12]);
        assert_eq!(&action.parameters[33..], &0x57572dacu32.to_be_bytes());
        assert_eq!(bank.objects[&action.target].kind, 2);
        let source = bank.objects[&action.target].source()?;
        assert_eq!(source.plugin, 0x00050001);
        assert_eq!(source.stream_type, 0);
        assert_eq!(source.sample_rate, 16000);
        assert_eq!(source.format_bits, 0x02400004);
        assert_eq!(source.media_id, 0x17d3ec7f);
        assert_eq!(source.file_id, 0x57572dac);
        assert_eq!(source.memory, Some((0x9b940, 6176)));
        assert_eq!(source.flags, 0);
        assert!(!source.node_parameters.is_empty());
        let node = NodePrefix::parse(source.node_parameters)?;
        assert_eq!(node.parent, 0x24bad405);
        assert_eq!(node.bus, 0);
        assert_eq!(node.ranges, [[0.; 3]; 4]);
        assert_eq!(node.priority, [50, 0, 0, 246]);
        assert_eq!(node.override_fx, 0);
        assert_eq!(node.state_group, 0);
        let (voices, remaining) = node.inherited_positioning_voice_settings()?;
        assert_eq!(
            voices,
            VoiceSettings {
                virtual_queue: 0,
                kill_newest: true,
                maximum_instances: 1,
                below_threshold: 0,
                override_maximum: true,
                override_virtual: false
            }
        );
        let tail = SoundTail::parse(remaining, bank.feedback)?;
        assert_eq!(tail.state_sync, 0);
        assert!(tail.states.is_empty());
        assert!(tail.feedback.is_none());
        assert_eq!(tail.loops, [1, 0, 0]);
        assert!(!node.remaining.is_empty());
        let parent = NodePrefix::parse(bank.objects[&node.parent].payload)?;
        assert_eq!(parent.parent, 0x30a919a5);
        let root = NodePrefix::parse(bank.objects[&parent.parent].payload)?;
        assert_eq!(root.parent, 0);
        assert_eq!(root.bus, 0x37c2a949);
        assert_eq!(root.ranges[0], [-10., 0., 0.]);
        let (offset, size) = source.memory.unwrap();
        assert_eq!(
            bank.media[&source.media_id],
            &bytes[offset as usize..(offset + size) as usize]
        );
        let decoded = super::super::audio_dsp::Media::parse(bank.media[&source.media_id])?;
        assert_eq!(decoded.sample_rate, source.sample_rate);
        assert_eq!(decoded.decode()?.len(), 10416);
        assert!(bank.event_actions(0x3d07767c).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod stop_action_tests {
    use super::*;
    #[test]
    fn stop_wire_layout_preserves_active_fields_and_rejects_other_scopes() -> Result<()> {
        let mut bytes = Vec::new();
        for value in [500u32, 0, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(0xe4);
        bytes.extend_from_slice(&[0x5a; 16]);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&7u32.to_be_bytes());
        bytes.extend_from_slice(&9u32.to_be_bytes());
        let action = Action {
            kind: 0x1011,
            target: 3,
            delay: [0; 3],
            subsection_size: bytes.len() as u32,
            parameters: &bytes,
        };
        let stop = action.object_stop()?;
        assert_eq!(stop.transition, [500, 0, 0]);
        assert_eq!(stop.curve, 4);
        assert_eq!(stop.reserved, [0x5a; 16]);
        assert_eq!(stop.exceptions, [7, 9]);
        assert_eq!(
            stop.transition_time(|| panic!("Equal modifiers consumed RNG"))?,
            500
        );
        for length in 0..bytes.len() {
            let truncated = Action {
                parameters: &bytes[..length],
                ..action
            };
            assert!(truncated.object_stop().is_err());
        }
        let other = Action {
            kind: 0x1010,
            ..action
        };
        assert!(other.object_stop().is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(Action {
            parameters: &trailing,
            ..action
        }
        .object_stop()
        .is_err());
        Ok(())
    }
}
