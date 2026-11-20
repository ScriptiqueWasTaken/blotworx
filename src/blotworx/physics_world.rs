//! Recovered world fields: REL24518/240884, DOL802df8dc..802df9d8
//! and sweep setup8025a020..8025a180. Admission/pair production remain separate.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Conversion {
    pub low: [f32; 4],
    pub high: [f32; 4],
    pub scale: [f32; 4],
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Configuration {
    pub minimum: [f32; 4],
    pub maximum: [f32; 4],
    pub gravity: [f32; 3],
    pub collision_tolerance: f32,
    pub world_conversion: Conversion,
    pub sweep_conversion: Conversion,
}
impl Default for Configuration {
    fn default() -> Self {
        Self::retail()
    }
}
impl Configuration {
    /// REL24518 supplies5000 on all axes;240b1c..b68 applies0.5.
    pub fn retail() -> Self {
        Self::from_bounds([-2500., -2500., -2500., -0.], [2500., 2500., 2500., 0.])
            .expect("Verified retail world bounds")
    }
    pub fn from_bounds(minimum: [f32; 4], maximum: [f32; 4]) -> Result<Self> {
        ensure!(
            minimum.iter().chain(&maximum).all(|v| v.is_finite()),
            "Nonfinite physics world bounds"
        );
        let span: [f32; 3] = std::array::from_fn(|i| maximum[i] - minimum[i]);
        ensure!(
            span.iter().all(|&v| v > 0. && v.is_finite()),
            "Invalid physics world extent"
        );
        let mut low = [0.; 4];
        let mut high32 = [0.; 4];
        let mut scale32 = [0.; 4];
        let mut high16 = [0.; 4];
        let mut scale16 = [0.; 4];
        for i in 0..3 {
            // Keep separate f32 divide/multiply/add operations. The source
            // does not fuse them, and its high offsets use different biases.
            let reciprocal = 1.0_f32 / span[i];
            low[i] = -minimum[i];
            scale32[i] = reciprocal * f32::from_bits(0x4efffc00);
            let bias32 = span[i] * f32::from_bits(0x30000200);
            high32[i] = low[i] + bias32;
            scale16[i] = reciprocal * f32::from_bits(0x477ffc00);
            let bias16 = span[i] * f32::from_bits(0x37800200);
            high16[i] = low[i] + bias16;
        }
        ensure!(
            low.iter()
                .chain(&high32)
                .chain(&scale32)
                .chain(&high16)
                .chain(&scale16)
                .all(|v| v.is_finite()),
            "Physics world conversion overflow"
        );
        Ok(Self {
            minimum,
            maximum,
            gravity: [0., f32::from_bits(0xc11ccccd), 0.],
            collision_tolerance: f32::from_bits(0x3dcccccd),
            world_conversion: Conversion {
                low,
                high: high32,
                scale: scale32,
            },
            sweep_conversion: Conversion {
                low,
                high: high16,
                scale: scale16,
            },
        })
    }
    /// Shape phantom calcAabb802beeb8 multiplies the world's input+8 by0.5
    /// before invoking the original shape getter. This is not a distance test.
    pub fn shape_phantom_tolerance(&self) -> f32 {
        self.collision_tolerance * 0.5
    }
    /// Floating-AABB sweep update802580f0..80258264. This path clips with
    /// ps_sel, adds the native65536 float bias, then extracts endpoint bits.
    /// It is distinct from the uint32-cache path's shift/add-one conversion.
    pub fn sweep_from_aabb(&self, minimum: [f32; 3], maximum: [f32; 3]) -> Result<SweepAabb> {
        ensure!(
            (0..3).all(|i| minimum[i].is_finite()
                && maximum[i].is_finite()
                && minimum[i] <= maximum[i]),
            "Invalid floating broadphase AABB"
        );
        let convert = |value: f32, offset: f32, scale: f32| {
            let shifted = value + offset;
            let value = shifted * scale;
            let upper = f32::from_bits(0x477ffc00);
            let value = if upper - value >= 0. { value } else { upper };
            let value = if value - 0.0 >= 0. { value } else { 0. };
            (value + f32::from_bits(0x47800000)).to_bits()
        };
        Ok(SweepAabb {
            minimum: std::array::from_fn(|i| {
                (convert(
                    minimum[i],
                    self.sweep_conversion.low[i],
                    self.sweep_conversion.scale[i],
                ) >> 7)
                    & 0xfffe
            }),
            maximum: std::array::from_fn(|i| {
                ((convert(
                    maximum[i],
                    self.sweep_conversion.high[i],
                    self.sweep_conversion.scale[i],
                ) >> 7)
                    & 0xffff)
                    | 1
            }),
        })
    }
}

/// Native32-bit temporaries before storage into sweep nodes. Preserve the
/// full PPC max word even for out-of-contract input; do not silently clamp it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepAabb {
    pub minimum: [u32; 3],
    pub maximum: [u32; 3],
}
pub fn sweep_from_uint32(minimum: [u32; 3], maximum: [u32; 3]) -> SweepAabb {
    // DOL8025afdc..8025b08c. The special maximum is checked before add/OR.
    SweepAabb {
        minimum: minimum.map(|v| (v >> 15) & 0xfffe),
        maximum: maximum.map(|v| {
            let value = v >> 15;
            let value = if value == 0xffff {
                value
            } else {
                value.wrapping_add(1)
            };
            value | 1
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_bounds_reject_invalid_extents_without_clipping() {
        for (min, max) in [
            ([0.; 4], [0.; 4]),
            ([0.; 4], [f32::INFINITY; 4]),
            ([1.; 4], [-1.; 4]),
            ([-f32::MAX; 4], [f32::MAX; 4]),
        ] {
            assert!(Configuration::from_bounds(min, max).is_err());
        }
        let config = Configuration::retail();
        assert_eq!(config.shape_phantom_tolerance().to_bits(), 0x3d4ccccd);
        assert_eq!(config.minimum[3].to_bits(), 0x80000000);
        assert_eq!(config.world_conversion.low[3].to_bits(), 0);
        assert!(config.sweep_from_aabb([f32::NAN; 3], [1.; 3]).is_err());
        assert!(config.sweep_from_aabb([1.; 3], [-1.; 3]).is_err());
    }
    #[test]
    fn original_world_and_sweep_instruction_reference_matches_exact_bits() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "world_configuration_instruction_reference.json"
        ))?;
        assert_eq!(
            data["source_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        assert_eq!(data["configuration"].as_array().unwrap().len(), 4);
        assert_eq!(data["endpoints"].as_array().unwrap().len(), 4);
        let words = |value: &serde_json::Value| -> Vec<u32> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect()
        };
        for case in data["configuration"].as_array().unwrap() {
            let min: Vec<_> = words(&case["minimum"])
                .into_iter()
                .map(f32::from_bits)
                .collect();
            let max: Vec<_> = words(&case["maximum"])
                .into_iter()
                .map(f32::from_bits)
                .collect();
            let config =
                Configuration::from_bounds(min.try_into().unwrap(), max.try_into().unwrap())?;
            for (name, actual) in [
                ("world_low", config.world_conversion.low),
                ("world_high", config.world_conversion.high),
                ("world_scale", config.world_conversion.scale),
                ("sweep_low", config.sweep_conversion.low),
                ("sweep_high", config.sweep_conversion.high),
                ("sweep_scale", config.sweep_conversion.scale),
            ] {
                assert_eq!(
                    actual.map(f32::to_bits).as_slice(),
                    words(&case[name]).as_slice(),
                    "{}: {name}",
                    case["name"]
                );
            }
        }
        for case in data["endpoints"].as_array().unwrap() {
            let result = sweep_from_uint32(
                words(&case["minimum"]).try_into().unwrap(),
                words(&case["maximum"]).try_into().unwrap(),
            );
            assert_eq!(
                result.minimum.as_slice(),
                words(&case["sweep_minimum"]).as_slice()
            );
            assert_eq!(
                result.maximum.as_slice(),
                words(&case["sweep_maximum"]).as_slice()
            );
        }
        assert_eq!(data["float_endpoints"].as_array().unwrap().len(), 6);
        for case in data["float_endpoints"].as_array().unwrap() {
            let min = words(&case["minimum"])
                .into_iter()
                .map(f32::from_bits)
                .collect::<Vec<_>>();
            let max = words(&case["maximum"])
                .into_iter()
                .map(f32::from_bits)
                .collect::<Vec<_>>();
            let result = Configuration::retail()
                .sweep_from_aabb(min.try_into().unwrap(), max.try_into().unwrap())?;
            assert_eq!(
                result.minimum.as_slice(),
                words(&case["sweep_minimum"]).as_slice(),
                "{} minimum",
                case["name"]
            );
            assert_eq!(
                result.maximum.as_slice(),
                words(&case["sweep_maximum"]).as_slice(),
                "{} maximum",
                case["name"]
            );
        }
        Ok(())
    }
}
