//! hkpWorldObject property bookkeeping, DOL802e0efc /802e0fbc.
//! Native allocator/capacity flags and world ownership remain backend work.
use anyhow::{ensure, Context, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: u32,
    pub padding: Option<u32>,
    pub value: u64,
}
#[derive(Default, Debug)]
pub struct Properties {
    storage: Vec<Entry>,
    count: usize,
}
/// Serialized Wii hkpProperty entries: key/padding/high/low,16 bytes each.
pub fn decode(data: &[u8], offset: usize, count: usize) -> Result<Vec<Entry>> {
    ensure!(
        count <= i32::MAX as usize,
        "Property count outside native signed32 range"
    );
    let size = count
        .checked_mul(16)
        .context("Property array size overflow")?;
    let end = offset
        .checked_add(size)
        .context("Property array offset overflow")?;
    let bytes = data
        .get(offset..end)
        .context("Truncated Havok body property array")?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .context("Imported property allocation failed")?;
    for row in bytes.chunks_exact(16) {
        entries.push(Entry {
            key: u32::from_be_bytes(row[..4].try_into()?),
            padding: Some(u32::from_be_bytes(row[4..8].try_into()?)),
            value: u64::from_be_bytes(row[8..16].try_into()?),
        });
    }
    Ok(entries)
}
impl Properties {
    pub fn from_entries(entries: Vec<Entry>) -> Result<Self> {
        ensure!(
            entries.len() <= i32::MAX as usize,
            "Property count outside native signed32 range"
        );
        Ok(Self {
            count: entries.len(),
            storage: entries,
        })
    }
    pub fn entries(&self) -> &[Entry] {
        &self.storage[..self.count]
    }
    pub fn entry(&self, key: u32) -> Option<Entry> {
        self.entries().iter().find(|p| p.key == key).copied()
    }
    /// Original addProperty keeps the first existing key/value unchanged.
    /// Unknown fresh padding is distinct from initialized zero padding.
    pub fn add(&mut self, key: u32, value: u64) -> Result<bool> {
        if self.entry(key).is_some() {
            return Ok(false);
        }
        ensure!(
            self.count < i32::MAX as usize,
            "Property count would leave native signed32 range"
        );
        if self.count == self.storage.len() {
            self.storage
                .try_reserve(1)
                .context("Body property storage allocation failed")?;
            self.storage.push(Entry {
                key,
                padding: None,
                value,
            });
        } else {
            let slot = &mut self.storage[self.count];
            slot.key = key;
            slot.value = value;
        }
        self.count += 1;
        Ok(true)
    }
    /// Native return is removed64-bit value, or zero when absent. Ordered
    /// compaction copies all16-byte entries and leaves the final backing slot.
    pub fn remove(&mut self, key: u32) -> u64 {
        let Some(index) = self.entries().iter().position(|p| p.key == key) else {
            return 0;
        };
        let value = self.storage[index].value;
        self.storage.copy_within(index + 1..self.count, index);
        self.count -= 1;
        value
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_property_decode_preserves_all_words_and_rejects_invalid_ranges() -> Result<()> {
        let data = [
            0, 0, 0x11, 0x30, 0xde, 0xad, 0xbe, 0xef, 0xff, 0xff, 0xff, 0xff, 0x80, 0, 0, 0,
        ];
        assert_eq!(
            decode(&data, 0, 1)?,
            vec![Entry {
                key: 0x1130,
                padding: Some(0xdeadbeef),
                value: 0xffffffff80000000
            }]
        );
        assert!(decode(&data, 0, 2).is_err());
        assert!(decode(&data, usize::MAX, 1).is_err());
        assert!(decode(&data, 0, i32::MAX as usize + 1).is_err());
        assert!(decode(&data, 16, 0)?.is_empty());
        Ok(())
    }
    #[test]
    fn properties_keep_first_values_order_duplicates_and_reused_padding() -> Result<()> {
        let mut p = Properties::from_entries(vec![
            Entry {
                key: 10,
                padding: Some(1),
                value: 55,
            },
            Entry {
                key: 20,
                padding: Some(2),
                value: 66,
            },
            Entry {
                key: 30,
                padding: Some(3),
                value: 77,
            },
        ])?;
        assert!(!p.add(20, 999)?);
        assert_eq!(p.entry(20).unwrap().value, 66);
        assert_eq!(p.remove(20), 66);
        assert_eq!(
            p.entries().iter().map(|e| e.key).collect::<Vec<_>>(),
            [10, 30]
        );
        assert!(p.add(40, u64::MAX)?);
        assert_eq!(
            p.entry(40),
            Some(Entry {
                key: 40,
                padding: Some(3),
                value: u64::MAX
            })
        );
        assert_eq!(p.remove(999), 0);
        assert!(p.add(0x1130, 0xffffffff80000000)?);
        assert_eq!(p.entry(0x1130).unwrap().padding, None);
        assert!(!p.add(0x1130, 1)?);
        assert_eq!(p.entry(0x1130).unwrap().value, 0xffffffff80000000);
        let mut duplicate = Properties::from_entries(vec![
            Entry {
                key: 1,
                padding: Some(7),
                value: 10,
            },
            Entry {
                key: 1,
                padding: Some(8),
                value: 20,
            },
        ])?;
        assert!(!duplicate.add(1, 30)?);
        assert_eq!(duplicate.remove(1), 10);
        assert_eq!(
            duplicate.entry(1),
            Some(Entry {
                key: 1,
                padding: Some(8),
                value: 20
            })
        );
        assert_eq!(duplicate.remove(1), 20);
        assert_eq!(duplicate.remove(1), 0);
        Ok(())
    }
}
