//! Native float conversion-table evaluation80363380..8036350c, shared by
//! attenuation and RTPC definitions. No inferred sorting or interpolation.
use super::{
    audio_bank::{
        attenuation::{Attenuation, Conversion},
        Rtpc, RtpcPoint,
    },
    audio_transition::Tables,
};
use anyhow::{ensure, Result};

pub fn evaluate(points: &[RtpcPoint], scaling: u8, input: f32, tables: &Tables) -> Result<f32> {
    ensure!(
        !input.is_nan(),
        "NaN conversion input needs original FPSCR/payload handling"
    );
    // A null/empty native table preserves the input before output scaling.
    let mut value = input;
    for (index, point) in points.iter().enumerate() {
        if points.len() == 1 || input <= point.x || index == points.len() - 1 {
            value = point.y;
            break;
        }
        let next = &points[index + 1];
        if input < next.x {
            value = match point.interpolation {
                9 => point.y,
                4 => point.y + ((input - point.x) / (next.x - point.x)) * (next.y - point.y),
                mode => tables.interpolate(
                    (input - point.x) / (next.x - point.x),
                    point.y,
                    next.y,
                    mode,
                )?,
            };
            break;
        }
    }
    tables.scale_conversion(value, scaling)
}
impl Conversion {
    pub fn evaluate(&self, input: f32, tables: &Tables) -> Result<f32> {
        evaluate(&self.points, self.scaling, input, tables)
    }
}
impl Rtpc {
    pub fn evaluate(&self, input: f32, tables: &Tables) -> Result<f32> {
        evaluate(&self.points, self.scaling, input, tables)
    }
}
impl Attenuation {
    /// Absence remains distinct from any numeric result. The spatial caller
    /// supplies the native default for the particular curve role.
    pub fn evaluate(&self, slot: usize, input: f32, tables: &Tables) -> Result<Option<f32>> {
        self.mapped_curve(slot)?
            .map(|curve| curve.evaluate(input, tables))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn points(value: &serde_json::Value) -> Vec<RtpcPoint> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|point| RtpcPoint {
                x: f32::from_bits(point[0].as_u64().unwrap() as u32),
                y: f32::from_bits(point[1].as_u64().unwrap() as u32),
                interpolation: point[2].as_u64().unwrap() as u32,
            })
            .collect()
    }
    #[test]
    fn original_conversion_boundaries_shapes_scaling_and_point_order() -> Result<()> {
        let tables = Tables::retail()?;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_conversion.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let conversion = Conversion {
                points: points(&case["points"]),
                scaling: case["scaling"].as_u64().unwrap() as u8,
            };
            let input = f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
            let output = conversion.evaluate(input, &tables)?;
            assert_eq!(
                output.to_bits(),
                case["output_bits"].as_u64().unwrap() as u32,
                "case {index}, {case}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_owned_attenuation_tables_match_at_authored_points_and_between_them() -> Result<()> {
        let tables = Tables::retail()?;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_conversion.json"
        ))?;
        for table in fixture["owned"].as_array().unwrap() {
            let conversion = Conversion {
                points: points(&table["points"]),
                scaling: table["scaling"].as_u64().unwrap() as u8,
            };
            // RTPC uses this same native conversion layout and scaling switch.
            let rtpc = Rtpc {
                plugin: 0,
                rendered: 0,
                control: 1,
                parameter: 2,
                curve: 3,
                scaling: conversion.scaling,
                points: conversion.points.clone(),
            };
            for sample in table["samples"].as_array().unwrap() {
                let input = f32::from_bits(sample[0].as_u64().unwrap() as u32);
                let expected = sample[1].as_u64().unwrap() as u32;
                assert_eq!(
                    conversion.evaluate(input, &tables)?.to_bits(),
                    expected,
                    "owned table {}, input {input}",
                    table["sha256"]
                );
                assert_eq!(
                    rtpc.evaluate(input, &tables)?.to_bits(),
                    expected,
                    "RTPC table {}, input {input}",
                    table["sha256"]
                );
            }
        }
        Ok(())
    }
}
