//! Attribute output modifier, retail DOL 0x80206f0c..0x80206fbc.
//! This modifies a generator output, not persistent controller variables.
use super::behavior::Document;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Dense scalar output track. Presence is the native track's on-fraction,
/// independently accumulated from its scalar normalization weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub values: Vec<f32>,
    pub on_fraction: f32,
}
/// Dense scalar branch of 0x80203240, normalized by 0x802037d8.
/// Preserve two FMA accumulations and reciprocal-then-multiply normalization.
/// Sparse/palette tracks have different rules and must not use this function.
pub fn blend(from: &Track, to: &Track, weight: f32) -> Result<Track> {
    weighted(&[(from, 1. - weight), (to, weight)])
}
/// Same dense-track accumulation, retaining all contributors until the single
/// normalization call at DOL 0x801f7f74 -> 0x802037d8.
pub fn weighted(inputs: &[(&Track, f32)]) -> Result<Track> {
    let count = inputs
        .first()
        .context("No attribute blend inputs")?
        .0
        .values
        .len();
    let mut output = Track {
        values: vec![0.; count],
        on_fraction: 0.,
    };
    let mut normalization = 0_f32;
    for &(input, weight) in inputs {
        ensure!(
            input.values.len() == count,
            "Attribute track dimensions differ"
        );
        ensure!(weight.is_finite(), "Non-finite attribute blend weight");
        if input.on_fraction > 0. {
            output.on_fraction = weight.mul_add(input.on_fraction, output.on_fraction);
            if !input.values.is_empty() {
                normalization += weight;
                for (out, &value) in output.values.iter_mut().zip(&input.values) {
                    *out = weight.mul_add(value, *out);
                }
            }
        }
    }
    if output.on_fraction > 0. && normalization != 0. {
        let reciprocal = 1. / normalization;
        for value in &mut output.values {
            *value *= reciprocal;
        }
    }
    Ok(output)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    attribute_index: i32,
    attribute_value: f32,
}
pub struct Modifier {
    pub enabled: bool,
    assignments: Vec<Assignment>,
}
impl Modifier {
    /// `None` represents an output without the attribute track (track count
    /// <= 10 in the original). A missing mapping is the identity mapping.
    /// The graph scheduler resolves enable and bindings before calling modify.
    pub fn modify(&self, output: Option<&mut [f32]>, mapping: Option<&[i32]>) -> Result<()> {
        let Some(output) = output else {
            return Ok(());
        };
        for assignment in &self.assignments {
            let index = assignment.attribute_index;
            if index == -1 {
                continue;
            }
            ensure!(
                index >= 0,
                "Invalid negative animation attribute index {index}"
            );
            let index = match mapping {
                Some(map) => *map
                    .get(index as usize)
                    .context("Animation attribute mapping is too short")?,
                None => index,
            };
            // Retail does not check negative mapped indices before its store.
            // Reject malformed mappings instead of reproducing memory corruption.
            ensure!(
                index >= 0,
                "Invalid mapped animation attribute index {index}"
            );
            if let Some(value) = output.get_mut(index as usize) {
                *value = assignment.attribute_value;
            }
        }
        Ok(())
    }
}
pub fn compile(graph: &Document) -> Result<BTreeMap<String, Modifier>> {
    graph
        .objects
        .iter()
        .filter(|(_, o)| o.klass == "hkbAttributeModifier")
        .map(|(id, o)| {
            ensure!(
                o.fields["variableBindingSet"].is_null(),
                "Attribute modifier bindings require runtime member storage: {id}"
            );
            let enabled = o.fields["enable"]
                .as_bool()
                .context("Missing attribute modifier enable")?;
            let assignments = serde_json::from_value(o.fields["assignments"].clone())?;
            Ok((
                id.clone(),
                Modifier {
                    enabled,
                    assignments,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_attributes_match_original_entry_return_capture_bit_for_bit() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/attribute_blend.json"
        ))?;
        let read = |v: &serde_json::Value| -> Track {
            Track {
                values: v["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect(),
                on_fraction: f32::from_bits(v["on_fraction"].as_u64().unwrap() as u32),
            }
        };
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        for sample in samples {
            let weight = sample["weight"].as_f64().unwrap() as f32;
            let clock = super::super::transition_timing::BlendingClock {
                time: weight,
                remaining: 1. - weight,
            };
            let actual = clock
                .output(
                    &super::super::generator_output::Output {
                        transforms: vec![],
                        attributes: read(&sample["source"]),
                    },
                    &super::super::generator_output::Output {
                        transforms: vec![],
                        attributes: read(&sample["target"]),
                    },
                    false,
                    0.,
                    1,
                )?
                .attributes;
            let expected = read(&sample["result"]);
            assert_eq!(actual.on_fraction.to_bits(), expected.on_fraction.to_bits());
            assert_eq!(
                actual
                    .values
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                expected
                    .values
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }
    #[test]
    fn dense_transition_attributes_normalize_only_contributing_tracks() -> Result<()> {
        let from = Track {
            values: vec![0.63, 6.43, 0.5],
            on_fraction: 1.,
        };
        let to = Track {
            values: vec![2., 5., 0.5],
            on_fraction: 1.,
        };
        let result = blend(&from, &to, 0.5)?;
        assert_eq!(
            result,
            Track {
                values: vec![1.315, 5.715, 0.5],
                on_fraction: 1.
            }
        );
        let absent = Track {
            values: vec![99.; 3],
            on_fraction: 0.,
        };
        assert_eq!(
            blend(&absent, &to, 0.25)?,
            Track {
                values: to.values.clone(),
                on_fraction: 0.25
            }
        );
        assert_eq!(
            blend(&absent, &to, 0.)?,
            Track {
                values: vec![0.; 3],
                on_fraction: 0.
            }
        );
        let mut partial = from.clone();
        partial.on_fraction = 0.5;
        assert_eq!(blend(&partial, &to, 0.5)?.values, result.values);
        assert_eq!(blend(&partial, &to, 0.5)?.on_fraction, 0.75);
        Ok(())
    }
    #[test]
    fn assignments_map_in_order_and_preserve_other_output_attributes() -> Result<()> {
        let modifier = Modifier {
            enabled: true,
            assignments: vec![
                Assignment {
                    attribute_index: -1,
                    attribute_value: 99.,
                },
                Assignment {
                    attribute_index: 0,
                    attribute_value: 2.,
                },
                Assignment {
                    attribute_index: 1,
                    attribute_value: 5.,
                },
                Assignment {
                    attribute_index: 0,
                    attribute_value: 3.,
                },
            ],
        };
        let mut output = [0.63, 6.43, 0.5];
        modifier.modify(Some(&mut output), Some(&[1, 0]))?;
        assert_eq!(output, [5., 3., 0.5]);
        modifier.modify(None, Some(&[]))?;
        modifier.modify(Some(&mut output), Some(&[9, 8]))?;
        assert_eq!(output, [5., 3., 0.5]);
        assert!(modifier.modify(Some(&mut output), Some(&[-1, 0])).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn original_movement_modifiers_produce_authored_run_walk_attributes() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let modifiers = compile(&graph)?;
        assert_eq!(modifiers.len(), 2);
        let mut output = [0.63, 6.43, 0.5];
        assert!(modifiers["2:38624"].enabled);
        modifiers["2:38624"].modify(Some(&mut output), None)?;
        assert_eq!(output, [2., 5., 0.5]);
        modifiers["2:43600"].modify(Some(&mut output), None)?;
        assert_eq!(output, [2., 2.5, 0.5]);
        Ok(())
    }
}
