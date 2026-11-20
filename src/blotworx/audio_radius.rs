//! Native node radius query80333e90..80333ff8. No listener distance, object
//! scaling or spatial mixing is inferred here; those are separate operations.
use super::{
    audio_bank::{
        attenuation::Attenuation,
        positioning::{Mode, Positioning},
        Bank,
    },
    audio_sound::Radius,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub parent: Option<u32>,
    pub override_positioning: bool,
    pub attenuation: Option<u32>,
}
pub trait Control {
    type Attenuation;
    fn node(&mut self, id: u32) -> Result<Node>;
    fn acquire_attenuation(&mut self, id: u32) -> Result<Option<Self::Attenuation>>;
    fn last_distance_x(&mut self, attenuation: &Self::Attenuation) -> Result<Option<f32>>;
    fn release_attenuation(&mut self, attenuation: Self::Attenuation);
}
pub fn maximum_radius(mut node: Option<u32>, host: &mut impl Control) -> Result<Radius> {
    let none = Radius {
        found: false,
        value: 0.,
    };
    let mut seen = BTreeSet::new();
    while let Some(id) = node {
        ensure!(seen.insert(id), "Audio radius ancestry cycle at {id:08x}");
        let current = host.node(id)?;
        if let Some(id) = current.attenuation {
            let Some(attenuation) = host.acquire_attenuation(id)? else {
                return Ok(none);
            };
            let result = host.last_distance_x(&attenuation);
            host.release_attenuation(attenuation);
            return Ok(result?.map_or(none, |value| Radius { found: true, value }));
        }
        if current.override_positioning {
            return Ok(none);
        }
        node = current.parent;
    }
    Ok(none)
}
struct BankControl<'a, 'b> {
    bank: &'a Bank<'b>,
}
impl Control for BankControl<'_, '_> {
    type Attenuation = Attenuation;
    fn node(&mut self, id: u32) -> Result<Node> {
        let parameters = self.bank.parameter(id)?;
        let (override_positioning, attenuation) = match parameters.position {
            Positioning::Inherited => (false, None),
            Positioning::Local {
                mode: Mode::ThreeD { attenuation, .. },
                ..
            } => (true, Some(attenuation)),
            Positioning::Local { .. } => (true, None),
        };
        Ok(Node {
            parent: (parameters.parent != 0).then_some(parameters.parent),
            override_positioning,
            attenuation,
        })
    }
    fn acquire_attenuation(&mut self, id: u32) -> Result<Option<Attenuation>> {
        // Native attenuation has a separate class index. A same-ID resource
        // of another class does not become an attenuation resource.
        self.bank
            .objects
            .get(&id)
            .filter(|object| object.kind == 14)
            .map(|object| object.attenuation())
            .transpose()
    }
    fn last_distance_x(&mut self, attenuation: &Attenuation) -> Result<Option<f32>> {
        attenuation
            .mapped_curve(0)?
            .map(|curve| {
                curve
                    .points
                    .last()
                    .map(|point| point.x)
                    .context("Native attenuation distance table has no points")
            })
            .transpose()
    }
    fn release_attenuation(&mut self, _: Attenuation) {}
}
impl Bank<'_> {
    pub fn maximum_radius(&self, node: u32) -> Result<Radius> {
        maximum_radius(Some(node), &mut BankControl { bank: self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        references: u32,
    }
    impl Control for Host<'_> {
        type Attenuation = ();
        fn node(&mut self, id: u32) -> Result<Node> {
            self.calls.push(json!({"op":"node","id":id}));
            let nodes = self.case["nodes"].as_array().unwrap();
            let node = &nodes[id as usize - 1];
            Ok(Node {
                parent: ((id as usize) < nodes.len()).then_some(id + 1),
                override_positioning: node[0].as_bool().unwrap(),
                attenuation: node[1]
                    .as_bool()
                    .unwrap()
                    .then(|| self.case["attenuation_id"].as_u64().unwrap() as u32),
            })
        }
        fn acquire_attenuation(&mut self, id: u32) -> Result<Option<()>> {
            self.calls.push(json!({"op":"acquire","id":id}));
            Ok(if self.case["present"].as_bool().unwrap() {
                self.references += 1;
                Some(())
            } else {
                None
            })
        }
        fn last_distance_x(&mut self, _: &()) -> Result<Option<f32>> {
            ensure!(self.case["fail_last"] != true, "Supplied table failure");
            Ok(match self.case["mapping"].as_u64().unwrap() {
                0 => Some(-5.),
                1 => Some(37.25),
                255 => None,
                _ => unreachable!(),
            })
        }
        fn release_attenuation(&mut self, _: ()) {
            self.calls.push(json!({"op":"release"}));
            self.references -= 1;
        }
    }
    #[test]
    fn original_radius_inheritance_override_mapping_and_reference_order() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_radius.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut host = Host {
                case,
                calls: vec![],
                references: 3,
            };
            let start = (!case["nodes"].as_array().unwrap().is_empty()).then_some(1);
            let radius = maximum_radius(start, &mut host)?;
            assert_eq!(json!(radius.found), case["found"], "case {index}");
            assert_eq!(
                json!(radius.value.to_bits()),
                case["radius_bits"],
                "case {index}"
            );
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
            assert_eq!(json!(host.references), case["references"], "case {index}");
        }
        Ok(())
    }
    #[test]
    fn bank_radius_uses_local_definitions_without_reading_unused_ancestors() -> Result<()> {
        use super::super::audio_bank::Object;
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
        let two_d = node(0xdeadbeef, 2);
        let three_d = node(0xdeadbeef, 3);
        let mut attenuation = vec![0, 1, 255, 255, 255, 255, 2];
        for xs in [[1000f32, 500.], [100., 37.25]] {
            attenuation.push(0);
            attenuation.extend(2u16.to_be_bytes());
            for x in xs {
                attenuation.extend(x.to_be_bytes());
                attenuation.extend(0f32.to_be_bytes());
                attenuation.extend(4u32.to_be_bytes());
            }
        }
        attenuation.extend(0u16.to_be_bytes());
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
                        payload: &two_d,
                    },
                ),
                (
                    55,
                    Object {
                        kind: 14,
                        payload: &attenuation,
                    },
                ),
            ]),
        };
        bank.objects[&2].actor_mixer(false)?;
        assert!(bank.parameter_chain(1).is_err());
        let radius = bank.maximum_radius(1)?;
        assert!(!radius.found && radius.value == 0.);
        bank.objects.insert(
            2,
            Object {
                kind: 7,
                payload: &three_d,
            },
        );
        let radius = bank.maximum_radius(1)?;
        assert!(radius.found);
        assert_eq!(radius.value, 37.25);
        // A resource of the wrong class must not satisfy the attenuation index.
        bank.objects.insert(
            55,
            Object {
                kind: 3,
                payload: &[],
            },
        );
        assert!(!bank.maximum_radius(1)?.found);
        Ok(())
    }
    #[test]
    fn malformed_radius_graphs_error_and_failed_table_reads_release_resources() -> Result<()> {
        struct Cycle;
        impl Control for Cycle {
            type Attenuation = ();
            fn node(&mut self, id: u32) -> Result<Node> {
                Ok(Node {
                    parent: Some(id),
                    override_positioning: false,
                    attenuation: None,
                })
            }
            fn acquire_attenuation(&mut self, _: u32) -> Result<Option<()>> {
                anyhow::bail!("Unexpected attenuation lookup")
            }
            fn last_distance_x(&mut self, _: &()) -> Result<Option<f32>> {
                anyhow::bail!("Unexpected table read")
            }
            fn release_attenuation(&mut self, _: ()) {}
        }
        assert!(maximum_radius(Some(1), &mut Cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle"));
        let case = json!({"nodes":[[true,true]],"attenuation_id":55,"present":true,"mapping":0,"fail_last":true});
        let mut host = Host {
            case: &case,
            calls: vec![],
            references: 3,
        };
        assert!(maximum_radius(Some(1), &mut host).is_err());
        assert_eq!(host.references, 3);
        assert_eq!(host.calls.last().unwrap()["op"], "release");
        Ok(())
    }
}
