//! Version46 positioning/advanced loaders: retail DOL80336bb0/80336f10.
//! These are authored parameters, not a spatial mixer or path playback engine.
use super::VoiceSettings;
use anyhow::{ensure, Context, Result};

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum Positioning {
    Inherited,
    Local {
        center: i16,
        pan: [f32; 2],
        mode: Mode,
    },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum Mode {
    TwoD {
        panner: bool,
    },
    ThreeD {
        kind: u32,
        attenuation: u32,
        spatialized: bool,
        source: SpatialSource,
    },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub enum SpatialSource {
    Game {
        dynamic: bool,
    },
    User {
        mode: u32,
        looping: bool,
        transition: i32,
        follow: bool,
        vertices: Vec<Vertex>,
        paths: Vec<Path>,
    },
    /// The native loader retains other numeric types without extra payload.
    /// Their playback semantics remain unsupported; do not reinterpret as 2D.
    Other,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub duration: i32,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Path {
    pub first: u32,
    pub count: u32,
    pub range: [f32; 2],
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .at
            .checked_add(n)
            .context("Audio positioning length overflow")?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .context("Truncated audio positioning/advanced settings")?;
        self.at = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> Result<bool> {
        Ok(self.byte()? != 0)
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn float(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.word()?);
        ensure!(v.is_finite(), "Non-finite audio positioning parameter");
        Ok(v)
    }
    fn count(&mut self, stride: usize) -> Result<usize> {
        let n = self.word()? as usize;
        ensure!(
            n <= (self.bytes.len() - self.at) / stride,
            "Truncated audio path array"
        );
        Ok(n)
    }
}
pub fn parse(bytes: &[u8]) -> Result<(Positioning, VoiceSettings, &[u8])> {
    let mut r = Reader { bytes, at: 0 };
    //80336be8..bfc normalizes the entire byte, not just its low bit.
    let position = if !r.flag()? {
        Positioning::Inherited
    } else {
        //80336c14 truncates the authored 32-bit center to the stored halfword.
        let center = r.word()? as i16;
        let pan = [r.float()?, r.float()?];
        let mode = if !r.flag()? {
            Mode::TwoD { panner: r.flag()? }
        } else {
            let kind = r.word()?;
            let attenuation = r.word()?;
            let spatialized = r.flag()?;
            let source = match kind {
                3 => SpatialSource::Game { dynamic: r.flag()? },
                2 => {
                    let mode = r.word()?;
                    let looping = r.flag()?;
                    let transition = r.word()? as i32;
                    let follow = r.flag()?;
                    let n = r.count(16)?;
                    let mut vertices = Vec::with_capacity(n);
                    for _ in 0..n {
                        vertices.push(Vertex {
                            position: [r.float()?, r.float()?, r.float()?],
                            duration: r.word()? as i32,
                        });
                    }
                    let n = r.count(8)?;
                    let mut paths = Vec::with_capacity(n);
                    for _ in 0..n {
                        paths.push(Path {
                            first: r.word()?,
                            count: r.word()?,
                            range: [0.; 2],
                        });
                    }
                    // The native loader consumes ranges even with no vertices.
                    for path in &mut paths {
                        path.range = [r.float()?, r.float()?];
                    }
                    SpatialSource::User {
                        mode,
                        looping,
                        transition,
                        follow,
                        vertices,
                        paths,
                    }
                }
                _ => SpatialSource::Other,
            };
            Mode::ThreeD {
                kind,
                attenuation,
                spatialized,
                source,
            }
        };
        Positioning::Local { center, pan, mode }
    };
    let virtual_queue = r.byte()?;
    let kill_newest = r.flag()?;
    let maximum_instances = u16::from_be_bytes(r.take(2)?.try_into().unwrap());
    let below_threshold = r.byte()?;
    let override_maximum = r.flag()?;
    let override_virtual = r.flag()?;
    let voices = VoiceSettings {
        virtual_queue,
        kill_newest,
        maximum_instances,
        below_threshold,
        override_maximum,
        override_virtual,
    };
    Ok((position, voices, &bytes[r.at..]))
}
///80336a30..80336ad0: inherit virtual-queue/below-threshold policy until an
/// override, or use the root's own settings. Native unrolls four ancestors
/// before tail recursion; that is not a four-level hierarchy limit.
pub fn virtual_policy<'a>(
    nodes: impl IntoIterator<Item = &'a VoiceSettings>,
) -> Result<&'a VoiceSettings> {
    let mut root = None;
    for node in nodes {
        if node.override_virtual {
            return Ok(node);
        }
        root = Some(node);
    }
    root.context("Missing audio virtual-policy ancestor")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_positioning.json"
        ))
        .unwrap()
    }
    #[test]
    fn original_positioning_and_advanced_reader_states_match() -> Result<()> {
        for case in fixture()["cases"].as_array().unwrap() {
            let hex = case["encoded"].as_str().unwrap();
            let encoded = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let mut input = encoded.clone();
            input.extend([0xab, 0xcd]);
            let (position, voices, tail) = parse(&input)?;
            assert_eq!(tail, [0xab, 0xcd]);
            assert_eq!(
                case["position_bytes"].as_u64().unwrap() as usize + 7,
                encoded.len()
            );
            assert_eq!(serde_json::to_value(&voices)?, case["voices"]);
            let mut state = serde_json::json!({"override":position!=Positioning::Inherited});
            if let Positioning::Local { center, pan, mode } = position {
                state["center_bits"] = (center as u16).into();
                state["pan_bits"] = serde_json::json!(pan.map(f32::to_bits));
                match mode {
                    Mode::TwoD { panner } => state["panner"] = panner.into(),
                    Mode::ThreeD {
                        kind,
                        attenuation,
                        spatialized,
                        source,
                    } => {
                        state["kind"] = kind.into();
                        state["attenuation"] = attenuation.into();
                        state["spatialized"] = spatialized.into();
                        match source {
                            SpatialSource::Game { dynamic } => state["dynamic"] = dynamic.into(),
                            SpatialSource::User {
                                mode,
                                looping,
                                transition,
                                follow,
                                vertices,
                                paths,
                            } => {
                                state["mode"] = mode.into();
                                state["looping"] = looping.into();
                                state["transition"] = (transition as u32).into();
                                state["follow"] = follow.into();
                                let calls = case["path_service"].as_array().unwrap();
                                // Native only allocates path items when both
                                // input arrays are nonempty; the bank parser
                                // retains authored arrays even without that allocation.
                                let ranges = if calls.is_empty() {
                                    vec![]
                                } else {
                                    let call = &calls[0];
                                    assert_eq!(
                                        vertices.len() as u64,
                                        call["vertices"].as_u64().unwrap()
                                    );
                                    assert_eq!(paths.len() as u64, call["paths"].as_u64().unwrap());
                                    let bytes = vertices
                                        .iter()
                                        .flat_map(|v| {
                                            v.position
                                                .iter()
                                                .flat_map(|x| x.to_bits().to_be_bytes())
                                                .chain(v.duration.to_be_bytes())
                                        })
                                        .map(|b| format!("{b:02x}"))
                                        .collect::<String>();
                                    assert_eq!(bytes, call["vertex_data"]);
                                    let bytes = paths
                                        .iter()
                                        .flat_map(|p| {
                                            p.first
                                                .to_be_bytes()
                                                .into_iter()
                                                .chain(p.count.to_be_bytes())
                                        })
                                        .map(|b| format!("{b:02x}"))
                                        .collect::<String>();
                                    assert_eq!(bytes, call["path_data"]);
                                    paths
                                        .iter()
                                        .map(|p| p.range.map(f32::to_bits))
                                        .collect::<Vec<_>>()
                                };
                                state["ranges"] = serde_json::json!(ranges);
                            }
                            SpatialSource::Other => {}
                        }
                    }
                }
            }
            assert_eq!(state, case["state"], "{case}");
            for n in 0..encoded.len() {
                assert!(
                    parse(&encoded[..n]).is_err(),
                    "Accepted truncated settings at {n}: {case}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn virtual_policy_matches_original_deep_ancestry_and_override_gates() -> Result<()> {
        for case in fixture()["virtual_inheritance"].as_array().unwrap() {
            let nodes = case["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|n| VoiceSettings {
                    virtual_queue: n["virtual_queue"].as_u64().unwrap() as u8,
                    below_threshold: n["below_threshold"].as_u64().unwrap() as u8,
                    override_virtual: n["override_virtual"].as_bool().unwrap(),
                    kill_newest: false,
                    maximum_instances: 0,
                    override_maximum: false,
                })
                .collect::<Vec<_>>();
            let actual = virtual_policy(&nodes)?;
            assert_eq!(
                u64::from(actual.virtual_queue),
                case["virtual_queue"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(actual.below_threshold),
                case["below_threshold"].as_u64().unwrap()
            );
        }
        assert!(virtual_policy(&[]).is_err());
        Ok(())
    }
}
