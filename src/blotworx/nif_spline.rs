//! Open cubic splines used by the compact Wii Gamebryo evaluators.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};

#[derive(Clone, Debug, serde::Serialize)]
pub struct CompactSpline {
    start: f32,
    inverse_duration: f32,
    width: usize,
    points: Vec<f32>,
}

pub(super) struct Source<'a> {
    start: f32,
    inverse_duration: f32,
    count: usize,
    compact: &'a [u8],
    little_endian: bool,
}

impl<'a> Source<'a> {
    pub(super) fn load(
        nif: &'a NifFile,
        interval: [f32; 2],
        data: u32,
        basis: u32,
    ) -> Result<Self> {
        fn block<'a>(nif: &'a NifFile, id: u32, kind: &str) -> Result<&'a [u8]> {
            match nif.blocks.get(id as usize) {
                Some(NiObject::Unknown(raw)) if raw.type_name == kind => Ok(&raw.raw_data),
                _ => anyhow::bail!("Missing {kind} block {id}"),
            }
        }
        let word = |bytes: &[u8], at: usize| -> Result<usize> {
            let value = bytes
                .get(at..at + 4)
                .context("Truncated spline count")?
                .try_into()?;
            Ok(if nif.source_is_little_endian {
                u32::from_le_bytes(value)
            } else {
                u32::from_be_bytes(value)
            } as usize)
        };
        let [start, stop] = interval;
        let duration = stop - start;
        ensure!(
            start.is_finite() && duration.is_finite() && duration > 0.,
            "Invalid spline interval"
        );
        let basis = block(nif, basis, "NiBSplineBasisData")?;
        let count = word(basis, 0)?;
        ensure!(
            basis.len() == 4 && count >= 4,
            "Invalid cubic spline basis count"
        );
        let data = block(nif, data, "NiBSplineData")?;
        let compact_header = word(data, 0)?
            .checked_mul(4)
            .and_then(|size| size.checked_add(4))
            .context("Invalid spline float count")?;
        ensure!(
            compact_header <= data.len().saturating_sub(4),
            "Truncated spline float array"
        );
        let packed_count = word(data, compact_header)?;
        let compact = &data[compact_header + 4..];
        ensure!(
            packed_count.checked_mul(2) == Some(compact.len()),
            "Invalid compact spline array size"
        );
        Ok(Self {
            start,
            inverse_duration: duration.recip(),
            count,
            compact,
            little_endian: nif.source_is_little_endian,
        })
    }

    pub(super) fn channel(
        &self,
        handle: u32,
        width: usize,
        offset: f32,
        half_range: f32,
    ) -> Result<CompactSpline> {
        ensure!(
            handle != 0xffff && offset.is_finite() && half_range.is_finite(),
            "Invalid compact spline channel"
        );
        let first = (handle as usize)
            .checked_mul(2)
            .context("Spline offset overflow")?;
        let end = self
            .count
            .checked_mul(width)
            .and_then(|n| n.checked_mul(2))
            .and_then(|size| first.checked_add(size))
            .context("Spline channel size overflow")?;
        let bytes = self
            .compact
            .get(first..end)
            .context("Spline channel exceeds control points")?;
        let points = bytes
            .chunks_exact(2)
            .map(|point| {
                let point = if self.little_endian {
                    i16::from_le_bytes([point[0], point[1]])
                } else {
                    i16::from_be_bytes([point[0], point[1]])
                };
                (f32::from(point) / 32767.).mul_add(half_range, offset)
            })
            .collect::<Vec<_>>();
        ensure!(
            points.iter().all(|point| point.is_finite()),
            "Nonfinite spline control point"
        );
        Ok(CompactSpline {
            start: self.start,
            inverse_duration: self.inverse_duration,
            width,
            points,
        })
    }
}

impl CompactSpline {
    pub(super) fn sample<const N: usize>(&self, time: f32) -> Result<[f32; N]> {
        ensure!(
            N == self.width && time.is_finite(),
            "Invalid spline sampling channel or time"
        );
        let segments = self.points.len() / N - 3;
        let x = ((time - self.start) * self.inverse_duration).clamp(0., 1.) * segments as f32;
        let first = (x as usize).min(segments - 1);
        let span = first + 3;
        let knot = |index: usize| index.saturating_sub(3).min(segments) as f32;
        // Evaluate the four nonzero cubic basis functions by local knot recurrence.
        let mut weights = [1., 0., 0., 0.];
        for depth in 1..=3 {
            let mut carry = 0.;
            for index in 0..depth {
                let left = knot(span + 1 - depth + index);
                let right = knot(span + 1 + index);
                let part = weights[index] * (right - left).recip();
                weights[index] = (right - x).mul_add(part, carry);
                carry = (x - left) * part;
            }
            weights[depth] = carry;
        }
        Ok(std::array::from_fn(|component| {
            let part = |index: usize| self.points[(first + index) * N + component];
            weights[3].mul_add(
                part(3),
                weights[2].mul_add(part(2), weights[1].mul_add(part(1), weights[0] * part(0))),
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_sampling_matches_retail_fill_routine() -> Result<()> {
        // Synthetic control points run through retail Wii DOL 803c0310. The
        // reference combines its four native weights and decoded point values.
        let times = [0., 0.03125, 0.125, 0.33333334, 0.625, 0.875, 1.];
        for (count, expected) in [
            (
                4,
                [
                    -0.56252861,
                    -0.49377017,
                    -0.29207255,
                    0.07911461,
                    0.20380525,
                    -0.30998622,
                    -0.85985518,
                ],
            ),
            (
                5,
                [
                    -0.56252861,
                    -0.42922427,
                    -0.08471500,
                    0.34004216,
                    0.00536230,
                    -0.40811612,
                    -0.12561800,
                ],
            ),
            (
                6,
                [
                    -0.56252861,
                    -0.36579553,
                    0.10277022,
                    0.42808635,
                    -0.31993487,
                    -0.05667963,
                    0.60861921,
                ],
            ),
            (
                8,
                [
                    -0.56252861,
                    -0.24566594,
                    0.35691268,
                    -0.16549479,
                    -0.03371884,
                    0.92408755,
                    -0.42294461,
                ],
            ),
            (
                67,
                [
                    -0.56252861,
                    -0.44318217,
                    0.62885682,
                    0.81977444,
                    -0.04258858,
                    0.87170786,
                    0.39640111,
                ],
            ),
        ] {
            let compact: Vec<u8> = (0..count)
                .flat_map(|i| ((((i * 19247 + 8191) % 65535) as i32 - 32767) as i16).to_be_bytes())
                .collect();
            let spline = Source {
                start: 0.,
                inverse_duration: 1.,
                count,
                compact: &compact,
                little_endian: false,
            }
            .channel(0, 1, 0.375, 1.25)?;
            for (time, reference) in times.into_iter().zip(expected) {
                let [actual] = spline.sample(time)?;
                // The Wii compiler's specialized cubic polynomials and this
                // recurrence have different float32 evaluation order.
                assert!(
                    (actual - reference).abs() <= 2e-6,
                    "{count} at {time}: {actual} != {reference}"
                );
            }
        }
        Ok(())
    }
}
