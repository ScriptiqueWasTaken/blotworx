//! EM1 RibbonQuadGenerator stream and UV generation. Geometry interpolation,
//! emission history and brush attachment are separate from this stream stage.
use anyhow::{ensure, Context, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Generator {
    pub version: u16,
    pub uv_mode: u32,
    pub subdivisions: u16,
    pub interpolation_parameter: f32,
}
impl Generator {
    /// REL155ee4: base facing-quad generator is twelve bytes; version 2
    /// adds a signed-short subdivision count and a float to the UV mode.
    pub fn load(bytes: &[u8], little: bool) -> Result<Self> {
        ensure!(bytes.len() >= 18, "Truncated ribbon generator");
        let short = |at| {
            let b = bytes[at..at + 2].try_into().unwrap();
            if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let word = |at| {
            let b = bytes[at..at + 4].try_into().unwrap();
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let version = short(12);
        let (subdivisions, interpolation_parameter) = if version >= 2 {
            ensure!(bytes.len() == 24, "Invalid versioned ribbon generator size");
            (short(18), f32::from_bits(word(20)))
        } else {
            ensure!(bytes.len() == 18, "Invalid legacy ribbon generator size");
            (1, 1.)
        };
        ensure!(
            subdivisions > 0 && subdivisions <= i16::MAX as u16,
            "Invalid signed ribbon subdivision count"
        );
        ensure!(
            interpolation_parameter.is_finite(),
            "Non-finite ribbon interpolation parameter"
        );
        Ok(Self {
            version,
            uv_mode: word(14),
            subdivisions,
            interpolation_parameter,
        })
    }
    pub fn section_count(&self, particles: u32) -> Result<usize> {
        if particles == 0 {
            return Ok(0);
        }
        usize::try_from(
            (particles - 1)
                .checked_mul(u32::from(self.subdivisions))
                .and_then(|n| n.checked_add(1))
                .context("Ribbon section count overflow")?,
        )
        .context("Ribbon section count exceeds host")
    }
    /// REL15a5dc..15a8b0 writes two UVs per interpolated cross-section.
    /// Mode zero spans the current trail, mode one spans its capacity;
    /// remaining modes advance one U unit per section. Do not stretch all
    /// three modes to the current trail, which makes the texture swim.
    pub fn texcoords(&self, particles: u32, capacity: u32) -> Result<Vec<[f32; 2]>> {
        ensure!(particles <= capacity, "Ribbon live count exceeds capacity");
        if particles <= 1 {
            return Ok(Vec::new());
        }
        let sections = self.section_count(particles)?;
        let denominator = match self.uv_mode {
            0 => (sections - 1) as f32,
            1 => (self.section_count(capacity)? - 1) as f32,
            _ => 1.,
        };
        let vertices = sections
            .checked_mul(2)
            .context("Ribbon vertex count overflow")?;
        let mut result = Vec::new();
        result.try_reserve_exact(vertices)?;
        for section in 0..sections {
            let u = section as f32 / denominator;
            result.extend([[u, 0.], [u, 1.]]);
        }
        Ok(result)
    }
}

/// REL1585c4 samples this authored, four-decimal table rather than
/// reevaluating Hermite polynomials. Words copied from REL4bb680..4bb7ff.
const BASIS: [[u32; 4]; 24] = [
    [0x3f800000, 0x00000000, 0x00000000, 0x00000000],
    [0x3f7eb1c4, 0x3d1ce076, 0x3ba71de7, 0xbaded289],
    [0x3f7af4f1, 0x3d8f5c29, 0x3ca161e5, 0xbbd1b717],
    [0x3f74fdf4, 0x3dc3fe5d, 0x3d3020c5, 0xbc6075f7],
    [0x3f6d07c8, 0x3decf41f, 0x3d97c1be, 0xbcbd3c36],
    [0x3f634d6a, 0x3e05bc02, 0x3de594af, 0xbd0ce704],
    [0x3f580347, 0x3e0ff972, 0x3e1ff2e5, 0xbd401a37],
    [0x3f4b5dcc, 0x3e15cfab, 0x3e5288ce, 0xbd76fd22],
    [0x3f3d9e84, 0x3e17a787, 0x3e84c2f8, 0xbd97c1be],
    [0x3f2f0069, 0x3e160419, 0x3ea1ff2e, 0xbdb404ea],
    [0x3f1fb15b, 0x3e113405, 0x3ec09d49, 0xbdcf7660],
    [0x3f0ff972, 0x3e09ba5e, 0x3ee00d1b, 0xbde90ff9],
    [0x3f000000, 0x3e000000, 0x3f000000, 0xbe000000],
    [0x3ee00d1b, 0x3de90ff9, 0x3f0ff972, 0xbe09ba5e],
    [0x3ec09d49, 0x3dcf7660, 0x3f1fb15b, 0xbe113405],
    [0x3ea1ff2e, 0x3db404ea, 0x3f2f0069, 0xbe160419],
    [0x3e84c2f8, 0x3d97c1be, 0x3f3d9e84, 0xbe17a787],
    [0x3e5288ce, 0x3d76fd22, 0x3f4b5dcc, 0xbe15cfab],
    [0x3e200d1b, 0x3d401a37, 0x3f57fcb9, 0xbe0ff972],
    [0x3de594af, 0x3d0ce704, 0x3f634d6a, 0xbe05bc02],
    [0x3d97c1be, 0x3cbd3c36, 0x3f6d07c8, 0xbdecf41f],
    [0x3d3020c5, 0x3c6075f7, 0x3f74fdf4, 0xbdc3fe5d],
    [0x3ca161e5, 0x3bd1b717, 0x3f7af4f1, 0xbd8f5c29],
    [0x3ba71de7, 0x3aded289, 0x3f7eb1c4, 0xbd1ce076],
];
impl Generator {
    /// Interpolate paired edge positions or paired normals after emission has
    /// produced each source cross-section. REL158620 uses integer 24/segments;
    /// source assets use divisors of 24. Other values need a compatibility
    /// decision because the original loop writes a different vertex count.
    pub fn interpolate_pairs(&self, points: &[[f32; 6]]) -> Result<Vec<[f32; 6]>> {
        ensure!(
            points.iter().flatten().all(|v| v.is_finite()),
            "Non-finite ribbon samples"
        );
        if points.len() < 2 || self.subdivisions == 1 {
            return Ok(points.to_vec());
        }
        let subdivisions = usize::from(self.subdivisions);
        ensure!(
            subdivisions <= 24 && 24 % subdivisions == 0,
            "Ribbon subdivision count does not divide the native interpolation table"
        );
        let count = self.section_count(u32::try_from(points.len())?)?;
        let mut output = Vec::new();
        output.try_reserve_exact(count)?;
        let scale = 0.5 * self.interpolation_parameter;
        for segment in 0..points.len() - 1 {
            let a = points[segment];
            let b = points[segment + 1];
            let previous = points[segment.saturating_sub(1)];
            let next = points[(segment + 2).min(points.len() - 1)];
            let start_tangent = std::array::from_fn::<_, 6, _>(|i| (b[i] - previous[i]) * scale);
            let end_tangent = std::array::from_fn::<_, 6, _>(|i| (next[i] - a[i]) * scale);
            output.push(a);
            for step in 1..subdivisions {
                let weights = BASIS[step * (24 / subdivisions)].map(f32::from_bits);
                // Separate paired-single multiplies/adds at REL158b34..158d44.
                // In particular do not contract these operations into FMAs.
                output.push(std::array::from_fn(|i| {
                    let initial = a[i] * weights[0] + start_tangent[i] * weights[1];
                    let endpoint = initial + b[i] * weights[2];
                    endpoint + end_tangent[i] * weights[3]
                }));
            }
        }
        output.push(*points.last().unwrap());
        ensure!(
            output.len() == count && output.iter().flatten().all(|v| v.is_finite()),
            "Invalid ribbon interpolation output"
        );
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation_preserves_source_points_and_endpoint_tangents() -> Result<()> {
        let generator = Generator {
            version: 2,
            uv_mode: 0,
            subdivisions: 6,
            interpolation_parameter: 1.,
        };
        let points = [[0.; 6], [6.; 6], [0.; 6]];
        let output = generator.interpolate_pairs(&points)?;
        assert_eq!(output.len(), 13);
        assert_eq!([output[0], output[6], output[12]], points);
        // At halfway to a turning point, source endpoints use a half-delta
        // tangent while the centered tangent at the peak is zero.
        assert_eq!(output[3], [3.375; 6]);
        assert_eq!(output[9], [3.375; 6]);
        assert_eq!(
            output[1][0].to_bits(),
            (3. * f32::from_bits(BASIS[4][1]) + 6. * f32::from_bits(BASIS[4][2])).to_bits()
        );
        assert!(Generator {
            subdivisions: 5,
            ..generator.clone()
        }
        .interpolate_pairs(&points)
        .is_err());
        assert_eq!(
            Generator {
                subdivisions: 1,
                ..generator
            }
            .interpolate_pairs(&points)?,
            points
        );
        Ok(())
    }
    #[test]
    fn authored_generator_retains_subdivisions_and_uv_modes() -> Result<()> {
        let bytes = [
            0, 0, 0, 1, 128, 48, 0, 0, 0, 1, 128, 64, 0, 2, 0, 0, 0, 0, 0, 6, 63, 128, 0, 0,
        ];
        let mut generator = Generator::load(&bytes, false)?;
        assert_eq!(
            (generator.subdivisions, generator.interpolation_parameter),
            (6, 1.)
        );
        let uv = generator.texcoords(3, 5)?;
        assert_eq!(uv.len(), 26);
        assert_eq!(uv[12], [0.5, 0.]);
        assert_eq!(uv[25], [1., 1.]);
        generator.uv_mode = 1;
        assert_eq!(generator.texcoords(3, 5)?[25], [0.5, 1.]);
        generator.uv_mode = 2;
        assert_eq!(generator.texcoords(3, 5)?[25], [12., 1.]);
        assert!(generator.texcoords(1, 5)?.is_empty());
        assert!(generator.texcoords(6, 5).is_err());
        assert!(Generator::load(&bytes[..23], false).is_err());
        Ok(())
    }
}
