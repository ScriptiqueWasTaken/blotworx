//! Scalar channels in Wii Havok 7 spline blocks. Compressed payload scalars are LE.
use super::animation_metadata::View;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
#[derive(Debug)]
enum Track {
    Constant(f32),
    Spline {
        degree: usize,
        knots: Vec<u8>,
        points: Vec<f32>,
    },
}
impl Track {
    fn sample(&self, frame: f32) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Spline {
                degree: p,
                knots: u,
                points: c,
            } => {
                let n = c.len() - 1;
                let t = frame.clamp(u[*p] as f32, u[n + 1] as f32);
                let span = if t >= u[n + 1] as f32 {
                    n
                } else {
                    (*p..=n)
                        .find(|&i| t >= u[i] as f32 && t < (u[i + 1] as f32))
                        .unwrap_or(*p)
                };
                let mut d = c[span - *p..=span].to_vec();
                for r in 1..=*p {
                    for j in (r..=*p).rev() {
                        let i = span - *p + j;
                        let lo = u[i] as f32;
                        let hi = u[i + *p - r + 1] as f32;
                        let alpha = if hi == lo { 0. } else { (t - lo) / (hi - lo) };
                        d[j] = (1. - alpha) * d[j - 1] + alpha * d[j];
                    }
                }
                d[*p]
            }
        }
    }
}
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).context("Scalar offset overflow")?;
        let data = self
            .data
            .get(self.at..end)
            .context("Truncated scalar track")?;
        self.at = end;
        Ok(data)
    }
    fn align(&mut self) -> Result<()> {
        let n = (4 - self.at % 4) % 4;
        self.take(n)?;
        Ok(())
    }
    fn real(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "Non-finite scalar value");
        Ok(v)
    }
}
fn read_track(cursor: &mut Cursor<'_>, packed: u8) -> Result<Track> {
    let mask = packed & !6;
    let quant = (packed >> 1) & 3;
    ensure!(quant <= 1, "Unsupported scalar quantization {quant}");
    match mask {
        0 => Ok(Track::Constant(0.)),
        1 => Ok(Track::Constant(cursor.real()?)),
        16 => {
            let n = u16::from_le_bytes(cursor.take(2)?.try_into()?) as usize;
            let degree = cursor.take(1)?[0] as usize;
            ensure!(degree <= n, "Spline degree exceeds control points");
            let knots = cursor.take(n + degree + 2)?.to_vec();
            cursor.align()?;
            ensure!(
                knots.windows(2).all(|w| w[0] <= w[1]) && knots[degree] < knots[n + 1],
                "Invalid scalar knot vector"
            );
            let min = cursor.real()?;
            let max = cursor.real()?;
            let mut points = Vec::with_capacity(n + 1);
            for _ in 0..=n {
                let q = if quant == 0 {
                    cursor.take(1)?[0] as f32 / 255.
                } else {
                    u16::from_le_bytes(cursor.take(2)?.try_into()?) as f32 / 65535.
                };
                points.push(min + (max - min) * q);
            }
            cursor.align()?;
            Ok(Track::Spline {
                degree,
                knots,
                points,
            })
        }
        _ => anyhow::bail!("Unsupported scalar mask {mask:#x}"),
    }
}
pub struct FloatClip {
    blocks: Vec<Vec<Track>>,
    frame_duration: f32,
    block_duration: f32,
    block_inverse_duration: f32,
    pub binding: Vec<i16>,
}
impl FloatClip {
    pub fn load(file: &HkxFile) -> Result<Self> {
        let view = View::new(file)?;
        let base = view.object("hkaSplineCompressedAnimation")?;
        let transforms = view.word(base + 16)? as usize;
        let floats = view.word(base + 20)? as usize;
        let count = view.word(base + 44)? as usize;
        ensure!(count > 0, "No spline blocks");
        let frame_duration = f32::from_bits(view.word(base + 64)?);
        let block_duration = f32::from_bits(view.word(base + 56)?);
        let block_inverse_duration = f32::from_bits(view.word(base + 60)?);
        ensure!(
            frame_duration.is_finite()
                && frame_duration > 0.
                && block_duration.is_finite()
                && block_duration > 0.
                && block_inverse_duration.is_finite()
                && block_inverse_duration > 0.,
            "Invalid spline timing"
        );
        let array = |offset: usize| -> Result<Vec<usize>> {
            let n = view.word(base + offset + 4)? as usize;
            ensure!(n == count, "Spline block offset count mismatch");
            let start = view.pointer(base + offset)?;
            ensure!(
                n <= view.data.len().saturating_sub(start) / 4,
                "Truncated offset array"
            );
            (0..n)
                .map(|i| Ok(view.word(start + i * 4)? as usize))
                .collect()
        };
        let starts = array(68)?;
        let float_starts = array(80)?;
        let data_start = view.pointer(base + 116)?;
        let length = view.word(base + 120)? as usize;
        let data = view
            .data
            .get(data_start..data_start.checked_add(length).context("Data overflow")?)
            .context("Truncated spline payload")?;
        let mut blocks = Vec::with_capacity(count);
        for i in 0..count {
            let end = starts.get(i + 1).copied().unwrap_or(length);
            let block = data
                .get(starts[i]..end)
                .context("Invalid spline block range")?;
            let mask_start = transforms.checked_mul(4).context("Mask offset overflow")?;
            let masks = block
                .get(
                    mask_start
                        ..mask_start
                            .checked_add(floats)
                            .context("Mask size overflow")?,
                )
                .context("Truncated float masks")?;
            let mut cursor = Cursor {
                data: block,
                at: float_starts[i],
            };
            ensure!(cursor.at <= block.len(), "Invalid scalar block offset");
            blocks.push(
                masks
                    .iter()
                    .map(|&m| read_track(&mut cursor, m))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        let binding = super::animation_metadata::float_binding(file)?;
        ensure!(
            binding.len() == floats,
            "Float track binding count mismatch"
        );
        Ok(Self {
            blocks,
            frame_duration,
            block_duration,
            block_inverse_duration,
            binding,
        })
    }
    /// Applies authored track bindings over an explicit caller-provided base pose.
    /// Unanimated slots retain that pose; this method does not invent defaults or blending.
    pub fn sample_slots(&self, time: f32, base: &[f32]) -> Result<Vec<f32>> {
        bind_slots(&self.sample(time)?, &self.binding, base)
    }
    /// Returns authored track values; binding to skeleton slots is explicit at the caller.
    pub fn sample(&self, time: f32) -> Result<Vec<f32>> {
        ensure!(time.is_finite() && time >= 0., "Invalid scalar sample time");
        let block = ((time * self.block_inverse_duration) as usize).min(self.blocks.len() - 1);
        let frame = (time - block as f32 * self.block_duration) / self.frame_duration;
        let values: Vec<f32> = self.blocks[block].iter().map(|t| t.sample(frame)).collect();
        ensure!(
            values.iter().all(|v| v.is_finite()),
            "Non-finite scalar sample result"
        );
        Ok(values)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_and_identity_masks() {
        let bytes = 0.375f32.to_le_bytes();
        let mut c = Cursor {
            data: &bytes,
            at: 0,
        };
        assert_eq!(read_track(&mut c, 3).unwrap().sample(100.), 0.375);
        assert_eq!(read_track(&mut c, 2).unwrap().sample(100.), 0.);
        assert!(read_track(&mut c, 3).is_err());
    }
    #[test]
    fn quantized_linear_spline() {
        let mut b = vec![1, 0, 1, 0, 0, 10, 10, 0];
        b.extend(2f32.to_le_bytes());
        b.extend(6f32.to_le_bytes());
        b.extend([0, 255, 0, 0]);
        let curve = read_track(&mut Cursor { data: &b, at: 0 }, 16).unwrap();
        assert_eq!(curve.sample(0.), 2.);
        assert_eq!(curve.sample(5.), 4.);
        assert_eq!(curve.sample(10.), 6.);
        assert!(read_track(
            &mut Cursor {
                data: &b[..8],
                at: 0
            },
            16
        )
        .is_err());
    }
}
#[cfg(test)]
mod scalar_regressions {
    use super::*;
    #[test]
    fn quantized_16bit_curve_preserves_endpoint_values() {
        let mut b = vec![1, 0, 1, 0, 0, 10, 10, 0];
        b.extend((-0.25f32).to_le_bytes());
        b.extend(0.75f32.to_le_bytes());
        b.extend([0, 0, 255, 255]);
        let curve = read_track(&mut Cursor { data: &b, at: 0 }, 18).unwrap();
        assert_eq!(curve.sample(0.), -0.25);
        assert_eq!(curve.sample(5.), 0.25);
        assert_eq!(curve.sample(10.), 0.75);
    }
    #[test]
    fn cubic_repeated_endpoint_knots() {
        let curve = Track::Spline {
            degree: 3,
            knots: vec![0, 0, 0, 0, 10, 10, 10, 10],
            points: vec![0., 0., 1., 1.],
        };
        assert_eq!(curve.sample(0.), 0.);
        assert_eq!(curve.sample(5.), 0.5);
        assert_eq!(curve.sample(10.), 1.);
    }
}

#[cfg(test)]
mod block_boundary_tests {
    use super::*;
    #[test]
    fn boundary_selects_next_block_and_resets_local_frame() {
        let clip = FloatClip {
            blocks: vec![
                vec![Track::Spline {
                    degree: 1,
                    knots: vec![0, 0, 10, 10],
                    points: vec![0., 1.],
                }],
                vec![Track::Spline {
                    degree: 1,
                    knots: vec![0, 0, 10, 10],
                    points: vec![1., 2.],
                }],
            ],
            frame_duration: 0.1,
            block_duration: 1.,
            block_inverse_duration: 1.,
            binding: vec![0],
        };
        assert!((clip.sample(0.999).unwrap()[0] - 0.999).abs() < 1e-6);
        assert_eq!(clip.sample(1.).unwrap(), vec![1.]);
        assert!((clip.sample(1.001).unwrap()[0] - 1.001).abs() < 1e-6);
        assert_eq!(clip.sample(1.5).unwrap(), vec![1.5]);
        assert!(clip.sample(f32::NAN).is_err());
        assert!(clip.sample(-1.).is_err());
    }
    #[test]
    fn constant_channels_can_change_between_blocks() {
        let clip = FloatClip {
            blocks: vec![vec![Track::Constant(0.25)], vec![Track::Constant(0.75)]],
            frame_duration: 0.1,
            block_duration: 1.,
            block_inverse_duration: 1.,
            binding: vec![0],
        };
        assert_eq!(clip.sample(0.999).unwrap(), vec![0.25]);
        assert_eq!(clip.sample(1.).unwrap(), vec![0.75]);
    }
}

fn bind_slots(values: &[f32], binding: &[i16], base: &[f32]) -> Result<Vec<f32>> {
    ensure!(
        values.len() == binding.len(),
        "Scalar value/binding count mismatch"
    );
    ensure!(
        base.iter().chain(values).all(|v| v.is_finite()),
        "Non-finite scalar pose"
    );
    let mut output = base.to_vec();
    let mut assigned = vec![false; base.len()];
    for (&value, &slot) in values.iter().zip(binding) {
        if slot == -1 {
            continue;
        }
        ensure!(
            slot >= 0 && (slot as usize) < output.len(),
            "Invalid scalar slot {slot}"
        );
        let i = slot as usize;
        ensure!(!assigned[i], "Duplicate scalar slot {slot}");
        assigned[i] = true;
        output[i] = value;
    }
    Ok(output)
}
#[cfg(test)]
mod binding_tests {
    use super::*;
    #[test]
    fn sparse_reordered_binding_preserves_base_slots() {
        assert_eq!(
            bind_slots(&[0.5, 0.75, 1.], &[2, -1, 0], &[0.1, 0.2, 0.3, 0.4]).unwrap(),
            vec![1., 0.2, 0.5, 0.4]
        );
    }
    #[test]
    fn rejects_ambiguous_or_invalid_bindings() {
        assert!(bind_slots(&[1., 2.], &[0, 0], &[0.]).is_err());
        assert!(bind_slots(&[1.], &[2], &[0.]).is_err());
        assert!(bind_slots(&[1.], &[-2], &[0.]).is_err());
        assert!(bind_slots(&[1.], &[], &[0.]).is_err());
        assert!(bind_slots(&[1.], &[0], &[f32::NAN]).is_err());
    }
}
