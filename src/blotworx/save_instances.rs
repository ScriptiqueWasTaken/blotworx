//! AISS2 instance changes: constructor183c80, lookup184154, save183f58.
//! Mood writes9fd30/+4; saved body paint writes37d340/+8. This is not a
//! serialized AI behavior tree. Native restore/application order remains open.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Entry {
    pub entity: u32,
    pub mood_bits: u32,
    pub painted: u8,
    pub reserved: [u8; 3],
}
#[derive(Clone, Debug, Default)]
pub struct State {
    entries: Vec<Entry>,
    index: BTreeMap<u32, usize>,
}
impl State {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn get(&self, entity: u32) -> Option<&Entry> {
        self.index.get(&entity).map(|&index| &self.entries[index])
    }
    ///184154: read-only misses allocate nothing; writes append in request order.
    /// The original appends ID, float0, byte0, not the entity's authored values.
    pub fn lookup(&mut self, entity: u32, read_only: bool) -> Result<Option<&mut Entry>> {
        let index = match self.index.get(&entity) {
            Some(&index) => index,
            None if read_only => return Ok(None),
            None => {
                ensure!(
                    self.entries.len() < u16::MAX as usize,
                    "AISS entry count exceeds native u16 storage"
                );
                let index = self.entries.len();
                self.entries.push(Entry {
                    entity,
                    mood_bits: 0,
                    painted: 0,
                    reserved: [0; 3],
                });
                self.index.insert(entity, index);
                index
            }
        };
        Ok(Some(&mut self.entries[index]))
    }
    pub fn set_painted(&mut self, entity: u32, value: u8) -> Result<()> {
        self.lookup(entity, false)?
            .context("AISS write lookup declined")?
            .painted = value;
        Ok(())
    }
    /// The caller must first execute MoodComponent's native value/material/event
    /// stages9fc70..9fd34. Merely writing this record does not change a live NPC.
    pub fn set_mood(&mut self, entity: u32, value: f32) -> Result<()> {
        self.lookup(entity, false)?
            .context("AISS write lookup declined")?
            .mood_bits = value.to_bits();
        Ok(())
    }
    pub fn reset(&mut self) {
        self.entries.clear();
        self.index.clear();
    }
    ///183f58 reads ENTITIES then DATA, rebuilding the ID-to-index lookup.
    /// Reject unsafe counts, truncated arrays and ambiguous duplicate keys before
    /// changing owned state. Valid native snapshots have unique keys.
    pub fn restore_payload(&mut self, payload: &[u8]) -> Result<()> {
        let records = super::save_game::records(payload)?;
        let count = records
            .iter()
            .find(|record| record.name == "ENTITIES")
            .context("AISS count missing")?
            .data;
        ensure!(count.len() >= 4, "Truncated AISS count");
        let count = u16::try_from(u32::from_be_bytes(count[..4].try_into()?))
            .context("AISS count exceeds native u16")? as usize;
        let data = records
            .iter()
            .find(|record| record.name == "DATA")
            .context("AISS data missing")?
            .data;
        let size = count.checked_mul(12).context("AISS array size overflow")?;
        ensure!(data.len() >= size, "Truncated AISS array");
        let mut restored = Self::default();
        for raw in data[..size].chunks_exact(12) {
            let entity = u32::from_be_bytes(raw[..4].try_into()?);
            ensure!(
                restored
                    .index
                    .insert(entity, restored.entries.len())
                    .is_none(),
                "Ambiguous duplicate AISS entity ID {entity:#x}"
            );
            restored.entries.push(Entry {
                entity,
                mood_bits: u32::from_be_bytes(raw[4..8].try_into()?),
                painted: raw[8],
                reserved: raw[9..12].try_into()?,
            });
        }
        *self = restored;
        Ok(())
    }
    ///184100 writes DATA first, ENTITIES second; preserve array order and bits.
    pub fn save_payload(&self) -> Result<Vec<u8>> {
        let count = u16::try_from(self.entries.len()).context("AISS count exceeds native u16")?;
        let mut data = Vec::with_capacity(self.entries.len() * 12);
        for entry in &self.entries {
            data.extend(entry.entity.to_be_bytes());
            data.extend(entry.mood_bits.to_be_bytes());
            data.push(entry.painted);
            data.extend(entry.reserved);
        }
        let mut output = Vec::new();
        super::save_game::append_record(&mut output, "data", &data, 4)?;
        super::save_game::append_record(
            &mut output,
            "entities",
            &u32::from(count).to_be_bytes(),
            4,
        )?;
        Ok(output)
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"entries":self.entries,"mood_runtime_producer_implemented":false,
            "body_paint_producer":"supported whole-model brush changes with ShouldSavePaintState",
            "restoration_implemented":false,"checkpoint_integrated":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_lookup_declines_and_writes_share_fields_without_reordering() -> Result<()> {
        let mut state = State::default();
        assert!(state.lookup(7, true)?.is_none());
        assert!(state.entries().is_empty());
        state.set_painted(7, 1)?;
        state.set_mood(2, -2.)?;
        state.set_mood(7, 1.)?;
        state.set_painted(2, 0x80)?;
        assert_eq!(
            state
                .entries()
                .iter()
                .map(|entry| entry.entity)
                .collect::<Vec<_>>(),
            [7, 2]
        );
        assert_eq!(
            (
                state.get(7).unwrap().mood_bits,
                state.get(7).unwrap().painted
            ),
            (1f32.to_bits(), 1)
        );
        assert_eq!(
            (
                state.get(2).unwrap().mood_bits,
                state.get(2).unwrap().painted
            ),
            ((-2f32).to_bits(), 128)
        );
        state.reset();
        assert!(state.get(7).is_none());
        assert!(state.entries().is_empty());
        Ok(())
    }
    #[test]
    fn restore_preserves_float_bits_raw_flags_padding_and_rejects_partial_overwrite() -> Result<()>
    {
        let mut state = State::default();
        state.set_mood(7, f32::from_bits(0x7fc01234))?;
        state.set_painted(7, 255)?;
        state.entries[0].reserved = [0xa5, 0xcd, 0xff];
        let payload = state.save_payload()?;
        let mut restored = State::default();
        restored.restore_payload(&payload)?;
        assert_eq!(restored.entries(), state.entries());
        assert_eq!(restored.save_payload()?, payload);
        let before = restored.entries.clone();
        assert!(restored
            .restore_payload(&payload[..payload.len() - 1])
            .is_err());
        assert_eq!(restored.entries, before);
        let mut corrupt = payload.clone();
        let record = super::super::save_game::records(&corrupt)?;
        let offset = record
            .iter()
            .find(|record| record.name == "ENTITIES")
            .unwrap()
            .data
            .as_ptr() as usize
            - corrupt.as_ptr() as usize;
        corrupt[offset..offset + 4].copy_from_slice(&65536u32.to_be_bytes());
        assert!(restored.restore_payload(&corrupt).is_err());
        assert_eq!(restored.entries, before);
        Ok(())
    }
    #[test]
    #[ignore = "requires original populated save in BLOTWORX_TEST_SAVE"]
    fn original_aiss_section_reencodes_byte_exact() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let archive = super::super::save_game::Archive::parse(&bytes)?;
        let section = archive.section(*b"AISS")?;
        assert_eq!(section.version, 2);
        let mut state = State::default();
        state.restore_payload(&section.payload)?;
        assert!(!state.entries().is_empty());
        assert_eq!(&state.save_payload()?, &*section.payload);
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
}
