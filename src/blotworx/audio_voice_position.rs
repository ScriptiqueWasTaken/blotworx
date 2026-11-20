//! Position acquisition80333cc0 and pan resolution80336320. Definitions,
//! allocation/copy ownership and actual RTPC evaluation are supplied services.
use super::audio_sound::Radius;
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
pub struct Node {
    pub parent: Option<u32>,
    pub override_positioning: bool,
    pub spatial: Option<u32>,
}
pub trait Control {
    type Position: Copy;
    fn node(&mut self, node: u32) -> Result<Node>;
    /// Allocate and run the original positioning constructor, not a zero fill.
    fn allocate_position(&mut self) -> Result<Option<Self::Position>>;
    /// Preserve the original shallow definition copy/ownership semantics.
    fn copy_position(&mut self, position: Self::Position, definition: u32) -> Result<()>;
    fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()>;
}
pub fn acquire<H: Control>(
    mut node: Option<u32>,
    position: &mut Option<H::Position>,
    object: u32,
    copy_disabled: u32,
    voice: u32,
    host: &mut H,
) -> Result<Option<u32>> {
    let mut seen = BTreeSet::new();
    while let Some(id) = node {
        ensure!(
            seen.insert(id),
            "Audio positioning ancestry cycle at {id:08x}"
        );
        let current = host.node(id)?;
        if let Some(definition) = current.spatial {
            if position.is_none() {
                *position = host.allocate_position()?;
            }
            if copy_disabled == 0 {
                if let Some(position) = *position {
                    host.copy_position(position, definition)?;
                }
            }
        }
        if current.spatial.is_some() || current.override_positioning || current.parent.is_none() {
            host.resolve_pan(id, object, voice)?;
            return Ok(Some(id));
        }
        node = current.parent;
    }
    Ok(None)
}
struct BankControl<'a, 'b, H> {
    bank: &'a super::audio_bank::Bank<'b>,
    host: &'a mut H,
}
impl<H: Control> Control for BankControl<'_, '_, H> {
    type Position = H::Position;
    fn node(&mut self, id: u32) -> Result<Node> {
        use super::audio_bank::positioning::{Mode, Positioning};
        let parameters = self.bank.parameter(id)?;
        Ok(Node {
            parent: (parameters.parent != 0).then_some(parameters.parent),
            override_positioning: !matches!(parameters.position, Positioning::Inherited),
            spatial: matches!(
                parameters.position,
                Positioning::Local {
                    mode: Mode::ThreeD { .. },
                    ..
                }
            )
            .then_some(id),
        })
    }
    fn allocate_position(&mut self) -> Result<Option<Self::Position>> {
        self.host.allocate_position()
    }
    fn copy_position(&mut self, position: Self::Position, definition: u32) -> Result<()> {
        self.host.copy_position(position, definition)
    }
    fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()> {
        self.host.resolve_pan(node, object, voice)
    }
}
impl super::audio_bank::Bank<'_> {
    /// Bank-local lookup; the copy service receives the defining node ID as
    /// its definition key. Loaded cross-bank lookup remains the backend's job.
    pub fn acquire_voice_position<H: Control>(
        &self,
        node: u32,
        position: &mut Option<H::Position>,
        object: u32,
        copy_disabled: u32,
        voice: u32,
        host: &mut H,
    ) -> Result<Option<u32>> {
        acquire(
            Some(node),
            position,
            object,
            copy_disabled,
            voice,
            &mut BankControl { bank: self, host },
        )
    }
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Pan {
    pub pan: [f32; 2],
    pub panner: u8,
    pub center: u16,
}
pub struct PanNode {
    pub defaults: Pan,
    pub rtpc_mask: u32,
    pub spatial: bool,
}
pub trait PanControl {
    fn pan_node(&mut self, node: u32) -> Result<PanNode>;
    fn rtpc(&mut self, node: u32, parameter: u32, object: u32) -> Result<f32>;
    fn radius(&mut self, node: u32) -> Result<Radius>;
}
pub fn pan_parameters(node: u32, object: u32, host: &mut impl PanControl) -> Result<(Pan, bool)> {
    let definition = host.pan_node(node)?;
    let mut pan = definition.defaults;
    let mut dynamic = false;
    let mut radius = 0.;
    if definition.rtpc_mask & 0x00100000 != 0 {
        dynamic = true;
        pan.pan[0] = host.rtpc(node, 20, object)?;
        if definition.spatial {
            let queried = host.radius(node)?;
            radius = queried.value;
            if queried.found {
                pan.pan[0] *= radius / 100.;
            }
        }
    }
    if definition.rtpc_mask & 0x00200000 != 0 {
        dynamic = true;
        pan.pan[1] = host.rtpc(node, 21, object)?;
        if definition.spatial {
            if radius == 0. {
                let queried = host.radius(node)?;
                radius = queried.value;
                if !queried.found {
                    return Ok((pan, dynamic));
                }
            }
            // Original8033643c..448 loads/stores X, even in the Y branch.
            // Preserve this observable quirk instead of scaling Y here.
            pan.pan[0] *= radius / 100.;
        }
    }
    Ok((pan, dynamic))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        radius_index: usize,
        fail_copy: bool,
        cycle: bool,
    }
    impl Control for Host<'_> {
        type Position = u32;
        fn node(&mut self, id: u32) -> Result<Node> {
            self.calls.push(json!({"op":"node","id":id}));
            let nodes = self.case["nodes"].as_array().unwrap();
            let row = &nodes[id as usize - 1];
            Ok(Node {
                parent: if self.cycle {
                    Some(id)
                } else if (id as usize) < nodes.len() {
                    Some(id + 1)
                } else {
                    None
                },
                override_positioning: row[1].as_bool().unwrap(),
                spatial: row[0].as_bool().unwrap().then_some(id),
            })
        }
        fn allocate_position(&mut self) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"allocate"}));
            Ok((self.case["allocated"] == true).then_some(1))
        }
        fn copy_position(&mut self, _: u32, definition: u32) -> Result<()> {
            self.calls
                .push(json!({"op":"copy","definition":definition}));
            ensure!(!self.fail_copy, "Injected positioning copy error");
            Ok(())
        }
        fn resolve_pan(&mut self, node: u32, object: u32, voice: u32) -> Result<()> {
            assert_eq!((object, voice), (42, 43));
            self.calls.push(json!({"op":"resolve","node":node}));
            Ok(())
        }
    }
    impl PanControl for Host<'_> {
        fn pan_node(&mut self, _: u32) -> Result<PanNode> {
            self.calls.push(json!({"op":"pan_node"}));
            Ok(PanNode {
                defaults: Pan {
                    pan: [-7.5, 3.25],
                    panner: 255,
                    center: 65535,
                },
                rtpc_mask: self.case["mask"].as_u64().unwrap() as u32,
                spatial: self.case["spatial"].as_bool().unwrap(),
            })
        }
        fn rtpc(&mut self, _: u32, parameter: u32, object: u32) -> Result<f32> {
            assert_eq!(object, 42);
            self.calls.push(json!({"op":"rtpc","parameter":parameter}));
            Ok(match parameter {
                20 => 12.5,
                21 => -3.75,
                _ => unreachable!(),
            })
        }
        fn radius(&mut self, _: u32) -> Result<Radius> {
            self.calls.push(json!({"op":"radius"}));
            let radii = self.case["radii"].as_array().unwrap();
            let value = &radii[self.radius_index.min(radii.len() - 1)];
            self.radius_index += 1;
            Ok(Radius {
                found: value[0].as_bool().unwrap(),
                value: value[1].as_f64().unwrap() as f32,
            })
        }
    }
    fn host(case: &Value) -> Host<'_> {
        Host {
            case,
            calls: Vec::new(),
            radius_index: 0,
            fail_copy: false,
            cycle: false,
        }
    }
    #[test]
    fn original_position_acquisition_ancestry_allocate_copy_and_pan_order() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_position.json"
        ))?;
        for (index, case) in fixture["acquisitions"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut host = host(case);
            let mut position = (case["existing"] == true).then_some(1);
            let selected = acquire(
                (!case["nodes"].as_array().unwrap().is_empty()).then_some(1),
                &mut position,
                42,
                case["copy_disabled"].as_u64().unwrap() as u32,
                43,
                &mut host,
            )?;
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
            assert_eq!(position.is_some(), case["position"].as_bool().unwrap());
            let expected = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|call| call["op"] == "resolve")
                .map(|call| call["node"].as_u64().unwrap() as u32);
            assert_eq!(selected, expected);
        }
        Ok(())
    }
    #[test]
    fn original_pan_rtpc_radius_order_and_float_bits() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_position.json"
        ))?;
        for (index, case) in fixture["pans"].as_array().unwrap().iter().enumerate() {
            let mut host = host(case);
            let (pan, dynamic) = pan_parameters(1, 42, &mut host)?;
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
            assert_eq!(
                json!(pan.pan.map(f32::to_bits)),
                case["pan_bits"],
                "case {index}"
            );
            assert_eq!(json!(dynamic), case["dynamic"]);
            assert_eq!(pan.panner as u64, case["panner"].as_u64().unwrap());
            assert_eq!(pan.center as u64, case["center"].as_u64().unwrap());
        }
        Ok(())
    }
    #[test]
    fn invalid_ancestry_is_diagnosed_and_copy_errors_retain_allocated_position() -> Result<()> {
        let case = json!({"nodes":[[false,false]],"allocated":true});
        let mut cyclic = host(&case);
        cyclic.cycle = true;
        assert!(acquire(Some(1), &mut None, 42, 0, 43, &mut cyclic)
            .unwrap_err()
            .to_string()
            .contains("cycle"));
        let case = json!({"nodes":[[true,false]],"allocated":true});
        let mut failed = host(&case);
        failed.fail_copy = true;
        let mut position = None;
        assert!(acquire(Some(1), &mut position, 42, 0, 43, &mut failed).is_err());
        assert_eq!(position, Some(1));
        Ok(())
    }
    #[test]
    fn bank_position_lookup_stops_at_local_definition_without_reading_invalid_parent() -> Result<()>
    {
        use super::super::audio_bank::{Bank, Object};
        use std::collections::BTreeMap;
        fn node(parent: u32, mode: u8) -> Vec<u8> {
            let mut bytes = vec![0; 66];
            bytes[6..10].copy_from_slice(&parent.to_be_bytes());
            bytes.push(u8::from(mode != 0));
            if mode != 0 {
                bytes.extend([0; 12]);
                bytes.push(u8::from(mode == 3));
                if mode == 3 {
                    bytes.extend(3u32.to_be_bytes());
                    bytes.extend(55u32.to_be_bytes());
                    bytes.extend([0, 0]);
                } else {
                    bytes.push(1);
                }
            }
            bytes.extend([0; 7]);
            bytes.extend([0; 5]);
            bytes.extend(0u32.to_be_bytes());
            bytes
        }
        let inherited = node(2, 0);
        let local = node(0xdeadbeef, 2);
        let spatial = node(0xdeadbeef, 3);
        let mut bank = Bank {
            id: 7,
            language: 0,
            feedback: false,
            media: BTreeMap::new(),
            objects: BTreeMap::from([
                (
                    1,
                    Object {
                        kind: 7,
                        payload: &inherited,
                    },
                ),
                (
                    2,
                    Object {
                        kind: 7,
                        payload: &local,
                    },
                ),
            ]),
        };
        let case = json!({"nodes":[],"allocated":true});
        let mut two_d = host(&case);
        let mut position = None;
        assert_eq!(
            bank.acquire_voice_position(1, &mut position, 42, 0, 43, &mut two_d)?,
            Some(2)
        );
        assert!(position.is_none());
        assert_eq!(two_d.calls, vec![json!({"op":"resolve","node":2})]);
        bank.objects.insert(
            2,
            Object {
                kind: 7,
                payload: &spatial,
            },
        );
        let mut three_d = host(&case);
        assert_eq!(
            bank.acquire_voice_position(1, &mut position, 42, 0, 43, &mut three_d)?,
            Some(2)
        );
        assert_eq!(position, Some(1));
        assert_eq!(
            three_d.calls,
            vec![
                json!({"op":"allocate"}),
                json!({"op":"copy","definition":2}),
                json!({"op":"resolve","node":2})
            ]
        );
        Ok(())
    }
}
