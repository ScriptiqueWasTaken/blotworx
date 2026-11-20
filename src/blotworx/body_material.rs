//! Original rigid-body coefficients, independent of motion and triangle filters.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    pub friction: f32,
    pub restitution: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Friction,
    Restitution,
}
///802a35d8..36c0: geometric mean with Gekko's estimate and one refinement,
/// then native contact-property quantization. Box3D's maximum restitution
/// rule would make a zero-restitution character bounce on authored platforms.
pub fn combine(a: f32, b: f32, kind: Property) -> Result<f32> {
    ensure!(
        a.is_finite() && b.is_finite() && a >= 0. && b >= 0.,
        "Unsupported material combination input"
    );
    let product = a * b;
    ensure!(product.is_finite(), "Material combination product overflow");
    let estimate = if product == 0. {
        f32::INFINITY
    } else {
        super::pose_blend::estimate(product)
    };
    let correction = (-product).mul_add(estimate * estimate, 1.);
    let refined = correction.mul_add(0.5 * estimate, estimate);
    let root = product * if refined >= 0. { refined } else { 0. };
    match kind {
        Property::Friction => Ok(super::native_motion_limits::decode(
            super::native_motion_limits::encode(root)?,
        )),
        Property::Restitution => {
            //80675468 is128, not255: the contact byte is unsigned Q1.7.
            let scaled = 128. * root;
            ensure!(
                scaled.is_finite() && scaled >= 0. && (scaled as f64) < 2147483648.,
                "Restitution combination requires unverified fctiwz overflow handling"
            );
            Ok(f32::from((scaled as i32) as u8) * (1. / 128.))
        }
    }
}
impl Coefficients {
    /// DOL 802b7c28/802b7c30 write body +90/+94 directly with stfs.
    /// No half-float packing, range clamp or motion-mode dispatch occurs.
    pub fn decode(data: &[u8], body: usize) -> Result<Self> {
        let start = body
            .checked_add(0x90)
            .context("Body material offset overflow")?;
        let end = start.checked_add(8).context("Body material end overflow")?;
        let bytes = data
            .get(start..end)
            .context("Truncated rigid-body material")?;
        let result = Self {
            friction: f32::from_be_bytes(bytes[..4].try_into()?),
            restitution: f32::from_be_bytes(bytes[4..].try_into()?),
        };
        ensure!(
            result.friction.is_finite() && result.restitution.is_finite(),
            "Nonfinite rigid-body material"
        );
        Ok(result)
    }
    /// REL 382970..382a38. Read each property at its actual stage; a negative
    /// value resolves the live config variable (with its original text default).
    /// Configuration accepts the full finite float range, not just [0,1].
    /// A later error preserves the preceding coefficient write.
    pub fn initialize(
        &mut self,
        mut property: impl FnMut(Property) -> Result<f32>,
        mut config: impl FnMut(&str, &str) -> Result<f32>,
    ) -> Result<()> {
        for (kind, key, default) in [
            (Property::Friction, "DefaultFrictionForAllObjects", "1.0"),
            (
                Property::Restitution,
                "DefaultRestitutionForAllObjects",
                "0.0",
            ),
        ] {
            let requested = property(kind)?;
            ensure!(
                requested.is_finite(),
                "Nonfinite body material property {kind:?}"
            );
            let value = if requested < 0. {
                config(key, default)?
            } else {
                requested
            };
            ensure!(
                value.is_finite(),
                "Nonfinite body material configuration {key}"
            );
            match kind {
                Property::Friction => self.friction = value,
                Property::Restitution => self.restitution = value,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_mix_matches_original_roots_and_quantized_restitution() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/body_material_mix.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let a = case["a"].as_f64().unwrap() as f32;
            let b = case["b"].as_f64().unwrap() as f32;
            let root = f32::from_bits(case["root_bits"].as_u64().unwrap() as u32);
            assert_eq!(
                combine(a, b, Property::Friction)?,
                super::super::native_motion_limits::decode(
                    super::super::native_motion_limits::encode(root)?
                ),
                "{case}"
            );
            assert_eq!(
                combine(a, b, Property::Restitution)?.to_bits(),
                (case["restitution_byte"].as_u64().unwrap() as f32 * (1f32 / 128.)).to_bits(),
                "{case}"
            );
        }
        assert_eq!(combine(0., 0.4, Property::Restitution)?, 0.);
        assert!(combine(f32::MAX, f32::MAX, Property::Friction).is_err());
        Ok(())
    }
    #[test]
    fn initialization_preserves_defaults_order_signed_zero_and_partial_writes() {
        let events = std::cell::RefCell::new(Vec::new());
        let mut material = Coefficients {
            friction: 0.25,
            restitution: 0.75,
        };
        material
            .initialize(
                |p| {
                    events.borrow_mut().push(format!("{p:?}"));
                    Ok(-1.)
                },
                |key, default| {
                    events.borrow_mut().push(format!("{key}={default}"));
                    Ok(if default == "1.0" { 2.5 } else { -0.25 })
                },
            )
            .unwrap();
        assert_eq!(
            *events.borrow(),
            [
                "Friction",
                "DefaultFrictionForAllObjects=1.0",
                "Restitution",
                "DefaultRestitutionForAllObjects=0.0"
            ]
        );
        assert_eq!(
            material,
            Coefficients {
                friction: 2.5,
                restitution: -0.25
            }
        );
        material
            .initialize(|_| Ok(-0.), |_, _| panic!("Zero is an explicit override"))
            .unwrap();
        assert_eq!(material.friction.to_bits(), 0x80000000);
        assert_eq!(material.restitution.to_bits(), 0x80000000);
        assert!(material
            .initialize(
                |p| match p {
                    Property::Friction => Ok(3.),
                    Property::Restitution => anyhow::bail!("Missing property"),
                },
                |_, _| unreachable!()
            )
            .is_err());
        assert_eq!(material.friction, 3.);
        assert_eq!(material.restitution.to_bits(), 0x80000000);
    }
    #[test]
    fn source_coefficients_are_big_endian_and_reject_invalid_storage() {
        let mut bytes = vec![0; 0x98];
        bytes[0x90..0x94].copy_from_slice(&0.375f32.to_be_bytes());
        bytes[0x94..].copy_from_slice(&1.25f32.to_be_bytes());
        assert_eq!(
            Coefficients::decode(&bytes, 0).unwrap(),
            Coefficients {
                friction: 0.375,
                restitution: 1.25
            }
        );
        assert!(Coefficients::decode(&bytes[..0x97], 0).is_err());
        assert!(Coefficients::decode(&bytes, usize::MAX).is_err());
        bytes[0x90..0x94].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(Coefficients::decode(&bytes, 0).is_err());
    }
}
