//! Version46 CAkAttenuation payload, native reader8031ead0..8031f200.
//! Definitions and native cone conversion; not spatial mixing or RTPC execution.
use super::{Object, Rtpc, RtpcPoint};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub scaling: u8,
    pub points: Vec<RtpcPoint>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Attenuation {
    pub cone_flag: u8,
    /// Authored cone floats. Native conversion affects the first two angles;
    /// the remaining two scalars are copied without transformation.
    pub cone: Option<[f32; 4]>,
    pub mapping: [u8; 5],
    pub curves: Vec<Conversion>,
    pub rtpc: Vec<Rtpc>,
}
fn word(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        b.get(at..at.checked_add(4).context("Attenuation offset overflow")?)
            .context("Truncated attenuation integer")?
            .try_into()
            .unwrap(),
    ))
}
impl<'a> Object<'a> {
    pub fn attenuation(&self) -> Result<Attenuation> {
        ensure!(self.kind == 14, "Object is not attenuation");
        Attenuation::parse(self.payload)
    }
}
impl Attenuation {
    pub fn parse(mut b: &[u8]) -> Result<Self> {
        let cone_flag = *b.first().context("Missing attenuation cone flag")?;
        b = &b[1..];
        let cone = if cone_flag != 0 {
            let mut values = [0.; 4];
            for value in &mut values {
                *value = f32::from_bits(word(b, 0)?);
                ensure!(value.is_finite(), "Non-finite attenuation cone");
                b = &b[4..];
            }
            Some(values)
        } else {
            None
        };
        ensure!(b.len() >= 6, "Truncated attenuation mappings");
        let mapping = b[..5].try_into().unwrap();
        let count = b[5] as usize;
        b = &b[6..];
        // The native object has exactly five conversion-table slots.
        ensure!(
            count <= 5,
            "Attenuation exceeds the native five conversion-table slots"
        );
        let mut curves = Vec::with_capacity(count);
        for _ in 0..count {
            ensure!(b.len() >= 3, "Truncated attenuation conversion header");
            let scaling = b[0];
            let count = u16::from_be_bytes([b[1], b[2]]) as usize;
            b = &b[3..];
            ensure!(
                count <= b.len() / 12,
                "Truncated attenuation conversion points"
            );
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                let x = f32::from_bits(word(b, 0)?);
                let y = f32::from_bits(word(b, 4)?);
                ensure!(
                    x.is_finite() && y.is_finite(),
                    "Non-finite attenuation point"
                );
                points.push(RtpcPoint {
                    x,
                    y,
                    interpolation: word(b, 8)?,
                });
                b = &b[12..];
            }
            curves.push(Conversion { scaling, points });
        }
        let (rtpc, remaining) = Rtpc::parse_list(b)?;
        ensure!(remaining.is_empty(), "Trailing attenuation data");
        Ok(Self {
            cone_flag,
            cone,
            mapping,
            curves,
            rtpc,
        })
    }
    pub fn mapped_curve(&self, slot: usize) -> Result<Option<&Conversion>> {
        let index = *self
            .mapping
            .get(slot)
            .context("Attenuation mapping slot is outside the native five slots")?;
        if index == 255 {
            Ok(None)
        } else {
            Ok(Some(self.curves.get(index as usize).context(
                "Attenuation mapping references an absent conversion table",
            )?))
        }
    }
    /// Successful native definitions with supplied allocation. This is not a
    /// claim that curve evaluation or spatial playback is implemented.
    pub fn validate_native_definition(&self) -> Result<()> {
        ensure!(
            !self.curves.is_empty() || !self.rtpc.is_empty(),
            "Native attenuation loader returns2 for an empty definition"
        );
        ensure!(
            self.curves.iter().all(|curve| !curve.points.is_empty())
                && self.rtpc.iter().all(|rtpc| !rtpc.points.is_empty()),
            "Native attenuation conversion requires nonempty points"
        );
        for slot in 0..5 {
            self.mapped_curve(slot)?;
        }
        self.native_cone()?;
        Ok(())
    }
    /// Native duplicate key is(parameter,curve), not the controlling RTPC ID.
    /// Deletion followed by append means last-occurrence order is retained.
    /// Keep the serialized definitions separately for lossless inspection.
    pub fn runtime_rtpc(&self) -> Vec<&Rtpc> {
        self.runtime_rtpc_indices()
            .into_iter()
            .map(|i| &self.rtpc[i])
            .collect()
    }
    pub fn runtime_rtpc_indices(&self) -> Vec<usize> {
        let last: BTreeMap<_, _> = self
            .rtpc
            .iter()
            .enumerate()
            .map(|(i, rtpc)| ((rtpc.parameter, rtpc.curve), i))
            .collect();
        self.rtpc
            .iter()
            .enumerate()
            .filter(|(i, rtpc)| last[&(rtpc.parameter, rtpc.curve)] == *i)
            .map(|(i, _)| i)
            .collect()
    }
    pub fn native_cone(&self) -> Result<Option<[f32; 4]>> {
        self.cone
            .map(|mut values| -> Result<_> {
                values[0] = half_angle(values[0])?;
                values[1] = half_angle(values[1])?;
                Ok(values)
            })
            .transpose()
    }
}
///80377fa0 followed by the loader's half-angle multiply. Preserve its actual
/// quotient subtraction; replacing it with a conventional modulo changes360.
pub fn half_angle(angle: f32) -> Result<f32> {
    let quotient = angle * f32::from_bits(0x3b360b61);
    ensure!(
        quotient.is_finite() && quotient >= i32::MIN as f32 && quotient < 2147483648.,
        "Cone angle needs unverified native invalid-conversion/FPSCR behavior"
    );
    let reduced = (angle as f64 - quotient.trunc() as i32 as f64) as f32;
    Ok((reduced * f32::from_bits(0x3c8efa35)) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn points(points: &[RtpcPoint]) -> Value {
        json!(points
            .iter()
            .map(|point| [point.x.to_bits(), point.y.to_bits(), point.interpolation])
            .collect::<Vec<_>>())
    }
    #[test]
    fn original_attenuation_definitions_cones_and_rtpc_replacement() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_attenuation.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let hex = case["encoded"].as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let attenuation = Attenuation::parse(&bytes)?;
            assert_eq!(
                json!(attenuation
                    .native_cone()?
                    .map(|values| values.map(f32::to_bits))),
                case["cone_bits"],
                "case {index}"
            );
            assert_eq!(json!(attenuation.mapping), case["mapping"]);
            let curves: Vec<_> = attenuation
                .curves
                .iter()
                .map(|curve| json!({"scaling":curve.scaling,"points":points(&curve.points)}))
                .collect();
            let rtpc:Vec<_>=attenuation.runtime_rtpc().into_iter().map(|rtpc|json!({"control":rtpc.control,"parameter":rtpc.parameter,"curve":rtpc.curve,"scaling":rtpc.scaling,"points":points(&rtpc.points)})).collect();
            assert_eq!(json!(curves), case["curves"], "case {index}");
            assert_eq!(json!(rtpc), case["rtpc"], "case {index}");
            assert_eq!(
                attenuation.validate_native_definition().is_ok(),
                case["status"] == 1,
                "case {index}"
            );
            for end in 0..bytes.len() {
                assert!(
                    Attenuation::parse(&bytes[..end]).is_err(),
                    "accepted truncated case {index} at {end}"
                );
            }
            let mut trailing = bytes;
            trailing.push(0);
            assert!(Attenuation::parse(&trailing).is_err());
        }
        Ok(())
    }
}
