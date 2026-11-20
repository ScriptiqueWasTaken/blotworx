//! REL23f228..23f3c4 linear-cast collector conversion.
//! Hit generation, body records and original sort order remain backend work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawContact {
    pub position: [u32; 4],
    /// XYZ normal and fraction at native offset+1c.
    pub normal: [u32; 4],
    pub collidables: [usize; 2],
    pub shape_keys: [u32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact {
    pub position: [u32; 4],
    pub normal: [u32; 4],
    pub collidable: usize,
    pub shape_key: u32,
    pub fraction: u32,
}
pub fn convert(query: usize, raw: RawContact) -> Contact {
    let side = usize::from(raw.collidables[0] == query);
    Contact {
        position: raw.position,
        normal: raw.normal,
        collidable: raw.collidables[side],
        shape_key: raw.shape_keys[side],
        fraction: raw.normal[3],
    }
}
/// Append in incoming order without deduplication or closest-hit reduction.
/// Allocation failure is explicit; prior caller storage remains unchanged.
pub fn append(query: usize, raw: &[RawContact], output: &mut Vec<Contact>) -> anyhow::Result<()> {
    output
        .try_reserve(raw.len())
        .map_err(|e| anyhow::anyhow!("Unable to allocate cast contacts: {e}"))?;
    output.extend(raw.iter().copied().map(|r| convert(query, r)));
    Ok(())
}
/// DOL80268be0: middle-pivot partition, recurse left then iterate right.
/// Ordered comparisons retain equal/NaN swaps; no total-order float sort.
pub fn sort_raw(raw: &mut [RawContact]) -> anyhow::Result<()> {
    sort_fractions(raw, |hit| f32::from_bits(hit.normal[3]))
}
/// Same partition sequence for ray records at DOL80269110: the different
/// record stride/copy size does not change ordered fraction comparisons.
pub(super) fn sort_fractions<T>(
    raw: &mut [T],
    fraction: impl Fn(&T) -> f32 + Copy,
) -> anyhow::Result<()> {
    fn range<T>(
        raw: &mut [T],
        mut low: i32,
        high: i32,
        fraction: impl Fn(&T) -> f32 + Copy,
    ) -> anyhow::Result<()> {
        loop {
            let middle = low.wrapping_add(high) >> 1;
            let pivot = fraction(
                raw.get(usize::try_from(middle)?)
                    .ok_or_else(|| anyhow::anyhow!("Native cast sort pivot outside array"))?,
            );
            let mut i = low;
            let mut j = high;
            loop {
                while fraction(&raw[i as usize]) < pivot {
                    i += 1;
                }
                while pivot < fraction(&raw[j as usize]) {
                    j -= 1;
                }
                if j >= i {
                    if j != i {
                        raw.swap(i as usize, j as usize);
                    }
                    j -= 1;
                    i += 1;
                }
                if i > j {
                    break;
                }
            }
            if low < j {
                range(raw, low, j, fraction)?;
            }
            if i >= high {
                return Ok(());
            }
            low = i;
        }
    }
    let count = i32::try_from(raw.len())?;
    if count > 1 {
        range(raw, 0, count - 1, fraction)?;
    }
    Ok(())
}
/// REL4bd138, loaded by constructor/reset1e21d0/23e0dc.
pub const EARLY_OUT: f32 = f32::from_bits(0x7f7fffee);
pub struct Collector {
    raw: Vec<RawContact>,
    query: usize,
    converted: Vec<Contact>,
    early_out: f32,
}
impl Default for Collector {
    fn default() -> Self {
        Self {
            raw: Vec::new(),
            query: 0,
            converted: Vec::new(),
            early_out: EARLY_OUT,
        }
    }
}
impl Collector {
    /// Query23ea88 clears converted count, invokes raw collector reset, then
    /// installs temporary query identity. Retain allocated buffers for reuse.
    pub fn begin_query(&mut self, query: usize) {
        self.converted.clear();
        self.raw.clear();
        self.early_out = EARLY_OUT;
        self.query = query;
    }
    pub fn raw(&self) -> &[RawContact] {
        &self.raw
    }
    pub fn early_out(&self) -> f32 {
        self.early_out
    }
    /// Host insertion invalidates converted cache; callers cannot accidentally
    /// mutate raw storage/query identity behind a ready converted count.
    pub fn push(&mut self, contact: RawContact) -> anyhow::Result<()> {
        self.raw
            .try_reserve(1)
            .map_err(|e| anyhow::anyhow!("Unable to allocate raw cast contact: {e}"))?;
        self.raw.push(contact);
        self.converted.clear();
        Ok(())
    }
    /// REL23f1a8: sort raw first, then invalidate converted count+1d4.
    pub fn sort(&mut self) -> anyhow::Result<()> {
        sort_raw(&mut self.raw)?;
        self.converted.clear();
        Ok(())
    }
    /// REL23f1e0: zero converted count triggers regeneration. It is the array's
    /// count, not a separate readiness flag. Empty raw results remain empty.
    pub fn contacts(&mut self) -> anyhow::Result<&[Contact]> {
        if self.converted.is_empty() {
            append(self.query, &self.raw, &mut self.converted)?;
        }
        Ok(&self.converted)
    }
    pub fn invalidate(&mut self) {
        self.converted.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sort_swaps_equal_fractions_and_invalidates_lazy_conversion() -> anyhow::Result<()> {
        let make = |id, fraction: f32| RawContact {
            position: [id as u32; 4],
            normal: [0, 0, 0, fraction.to_bits()],
            collidables: [100, id],
            shape_keys: [0, id as u32],
        };
        let mut collector = Collector::default();
        collector.begin_query(100);
        for i in 0..4 {
            collector.push(make(i, 0.5))?;
        }
        assert_eq!(
            collector
                .contacts()?
                .iter()
                .map(|c| c.collidable)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        collector.sort()?;
        assert_eq!(
            collector
                .contacts()?
                .iter()
                .map(|c| c.collidable)
                .collect::<Vec<_>>(),
            vec![2, 3, 0, 1]
        );
        assert_eq!(collector.contacts()?.len(), 4); // No duplicate rebuild after ready count.
        collector.begin_query(100);
        for hit in [make(7, 0.9), make(8, 0.1), make(9, 0.4), make(10, 0.2)] {
            collector.push(hit)?;
        }
        collector.sort()?;
        assert_eq!(
            collector
                .contacts()?
                .iter()
                .map(|c| c.collidable)
                .collect::<Vec<_>>(),
            vec![8, 10, 9, 7]
        );
        let mut unordered = vec![make(1, f32::NAN), make(2, 0.5), make(3, f32::NAN)];
        sort_raw(&mut unordered)?;
        assert_eq!(
            unordered
                .iter()
                .map(|c| c.collidables[1])
                .collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        collector.begin_query(100);
        assert!(collector.contacts()?.is_empty());
        Ok(())
    }
    #[test]
    fn collector_reuse_resets_both_counts_query_identity_and_exact_early_out() -> anyhow::Result<()>
    {
        let raw = RawContact {
            position: [0; 4],
            normal: [0; 4],
            collidables: [100, 200],
            shape_keys: [3, 4],
        };
        let mut collector = Collector::default();
        assert_eq!(collector.early_out().to_bits(), 0x7f7fffee);
        collector.begin_query(100);
        collector.push(raw)?;
        assert_eq!(collector.contacts()?[0].collidable, 200);
        let capacities = (collector.raw.capacity(), collector.converted.capacity());
        collector.early_out = 0.;
        collector.begin_query(300);
        assert_eq!(collector.early_out().to_bits(), 0x7f7fffee);
        assert!(collector.raw().is_empty());
        assert!(collector.contacts()?.is_empty());
        assert_eq!(
            (collector.raw.capacity(), collector.converted.capacity()),
            capacities
        );
        collector.push(raw)?;
        assert_eq!(collector.contacts()?[0].collidable, 100);
        collector.push(raw)?;
        assert_eq!(collector.contacts()?.len(), 2);
        Ok(())
    }
    #[test]
    fn collector_conversion_keeps_orientation_keys_padding_and_duplicate_hits() -> anyhow::Result<()>
    {
        let first = RawContact {
            position: [0x80000000, 1, 2, 0x7fc12345],
            normal: [3, 4, 5, 0x3f000001],
            collidables: [100, 200],
            shape_keys: [0xffff0001, 0xfffffffe],
        };
        let other = RawContact {
            collidables: [200, 100],
            shape_keys: [0x12345678, 9],
            ..first
        };
        let converted = convert(100, first);
        assert_eq!(converted.collidable, 200);
        assert_eq!(converted.shape_key, 0xfffffffe);
        assert_eq!(converted.position, first.position);
        assert_eq!(converted.normal, first.normal);
        assert_eq!(converted.fraction, 0x3f000001);
        assert_eq!(convert(100, other).shape_key, 0x12345678);
        // Native selects first when neither side matches; it doesn't invent a
        // missing-query rejection or silently remove the contact.
        assert_eq!(convert(300, first).collidable, 100);
        assert_eq!(
            convert(
                100,
                RawContact {
                    collidables: [100, 100],
                    ..first
                }
            )
            .shape_key,
            0xfffffffe
        );
        let mut output = vec![converted];
        append(100, &[other, first, first], &mut output)?;
        assert_eq!(output.len(), 4);
        assert_eq!(output[1].shape_key, 0x12345678);
        assert_eq!(output[2], converted);
        assert_eq!(output[3], converted);
        Ok(())
    }
}
