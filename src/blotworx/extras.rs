//! EM1 extras owner: CLB loader112a18, query112984, EXTR113064/1130f4.
//! Full checkpoint orchestration, unlock notification delivery and legacy saves
//! remain separate work; a query must never invent an unknown extra.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Entry {
    pub kind: String,
    pub name: String,
    pub thumbnail: String,
    pub asset: String,
    pub unlocked: bool,
    /// Native entry+11 / EXTR bit1; its UI meaning is not established.
    pub secondary_flag: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct State {
    pub entries: Vec<Entry>,
    #[serde(skip)]
    names: BTreeMap<String, usize>,
}
impl State {
    pub fn load(bytes: &[u8]) -> Result<Self> {
        Ok(Self::load_database(bytes)?.1)
    }
    pub fn load_database(bytes: &[u8]) -> Result<(super::collectibles::State, Self)> {
        struct Reader<'a> {
            bytes: &'a [u8],
            at: usize,
        }
        impl Reader<'_> {
            fn word(&mut self) -> Result<u32> {
                let value = u32::from_be_bytes(
                    self.bytes
                        .get(self.at..self.at + 4)
                        .context("Truncated CLB count")?
                        .try_into()?,
                );
                self.at += 4;
                Ok(value)
            }
            fn text(&mut self) -> Result<String> {
                let header = self
                    .bytes
                    .get(self.at..self.at + 2)
                    .context("Truncated CLB string header")?;
                let data = self
                    .bytes
                    .get(self.at + 2..)
                    .context("Truncated CLB string")?;
                let end = data
                    .iter()
                    .position(|&b| b == 0)
                    .context("Unterminated CLB string")?;
                let text = std::str::from_utf8(&data[..end])?.to_owned();
                let length = if end == 0 { 0 } else { end + 1 };
                let size = (length + 2).div_ceil(4) * 4;
                ensure!(
                    header == [size as u8, length as u8],
                    "Invalid CLB string lengths"
                );
                self.at = self
                    .at
                    .checked_add(size)
                    .context("CLB string size overflow")?;
                ensure!(self.at <= self.bytes.len(), "Truncated CLB string padding");
                Ok(text)
            }
        }
        let mut r = Reader { bytes, at: 0 };
        ensure!(
            r.word()? == 3,
            "Unsupported EM1 collectible database version"
        );
        let collectibles = r.word()? as usize;
        ensure!(
            collectibles <= u16::MAX as usize && collectibles <= (bytes.len() - r.at) / 12,
            "Invalid CLB collectible count"
        );
        let mut ordinary = Vec::with_capacity(collectibles);
        for _ in 0..collectibles {
            let kind = r.text()?;
            let name = r.text()?;
            let icon = r.text()?;
            ordinary.push(super::collectibles::Entry {
                kind,
                name,
                icon,
                count: 0,
            });
        }
        let count = r.word()? as usize;
        ensure!(
            count <= u16::MAX as usize && count <= (bytes.len() - r.at) / 16,
            "Invalid CLB extras count"
        );
        let mut entries = Vec::with_capacity(count);
        let mut names = BTreeMap::new();
        for index in 0..count {
            // Native112bbc..112cdc: type is FIRST, name SECOND. The old
            // library's Extra field labels invert these two serialized fields.
            let kind = r.text()?;
            let name = r.text()?.trim_matches(' ').to_owned();
            let thumbnail = r.text()?;
            let asset = r.text()?;
            names.insert(name.clone(), index); // native table insert overwrites duplicate names
            entries.push(Entry {
                kind,
                name,
                thumbnail,
                asset,
                unlocked: false,
                secondary_flag: false,
            });
        }
        ensure!(r.at == bytes.len(), "Trailing CLB data");
        Ok((
            super::collectibles::State::new(ordinary),
            Self { entries, names },
        ))
    }
    /// Keep traces compact: asset metadata already lives in the imported CLB.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"defined":self.entries.len(),
            "unlocked":self.entries.iter().filter(|e|e.unlocked).map(|e|&e.name).collect::<Vec<_>>(),
            "secondary_flag":self.entries.iter().filter(|e|e.secondary_flag).map(|e|&e.name).collect::<Vec<_>>()})
    }
    /// Native112d60 writes BOTH flag bytes when the named extra exists.
    /// SetUnlockedExtra's optional boolean controls notification, not ownership.
    pub fn unlock(&mut self, name: &str) -> Option<&Entry> {
        let &index = self.names.get(name)?;
        let entry = &mut self.entries[index];
        entry.unlocked = true;
        entry.secondary_flag = true;
        Some(entry)
    }
    /// Native112984 tests the exact interned name, returning false if absent.
    pub fn has(&self, name: &str) -> bool {
        self.names
            .get(name)
            .is_some_and(|&index| self.entries[index].unlocked)
    }
    fn header_flags<'a>(&self, bytes: &'a [u8]) -> Result<&'a [u8]> {
        ensure!(
            bytes.len() >= super::save_game::HEADER_SIZE
                && bytes.starts_with(b"GAME")
                && bytes[4..8] == 16u32.to_be_bytes(),
            "Expected GAME v16 header"
        );
        // Next fixed header field begins at1d8. Reject overflowing this native
        // allocation instead of writing over ordinary collectible state.
        ensure!(
            self.entries.len() <= 0x80,
            "Extras exceed native GAME header allocation"
        );
        Ok(&bytes[0x158..0x158 + self.entries.len()])
    }
    /// REL1130f4 receives the complete GAME buffer, not the EXTR section.
    /// Zero bits do not revoke existing flags. No other gameplay state loads.
    pub fn restore_save_header(&mut self, bytes: &[u8]) -> Result<()> {
        self.header_flags(bytes)?;
        for (entry, &flags) in self.entries.iter_mut().zip(&bytes[0x158..]) {
            entry.unlocked |= flags & 1 != 0;
            entry.secondary_flag |= flags & 2 != 0;
        }
        Ok(())
    }
    /// REL113064 writes into GAME+158 via25e238; EXTR1 gets no payload.
    /// Existing high bits survive on the unlocked branch, just as native.
    pub fn write_save_header(&self, bytes: &mut [u8]) -> Result<()> {
        self.header_flags(bytes)?;
        for (byte, entry) in bytes[0x158..].iter_mut().zip(&self.entries) {
            if entry.unlocked {
                *byte |= 1;
            } else {
                *byte = 0;
            }
            if entry.secondary_flag {
                *byte |= 2;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(3u32.to_be_bytes());
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(2u32.to_be_bytes());
        for value in [
            "Film",
            " Reel_Name ",
            "thumb.nif",
            "movie.bik",
            "Art",
            "Art_Name",
            "art.nif",
            "art.dds",
        ] {
            let length = value.len() + 1;
            let size = (length + 2).div_ceil(4) * 4;
            bytes.extend([size as u8, length as u8]);
            bytes.extend(value.as_bytes());
            bytes.push(0);
            bytes.resize(bytes.len() + size - length - 2, 0);
        }
        bytes
    }
    #[test]
    fn exact_names_native_field_order_and_monotonic_restore() -> Result<()> {
        let mut state = State::load(&database())?;
        assert_eq!(
            (&*state.entries[0].name, &*state.entries[0].kind),
            ("Reel_Name", "Film")
        );
        let mut header = vec![0; super::super::save_game::HEADER_SIZE];
        header[..4].copy_from_slice(b"GAME");
        header[4..8].copy_from_slice(&16u32.to_be_bytes());
        header[0x158..0x15a].copy_from_slice(&[1, 2]);
        assert!(!state.has("Reel_Name"));
        state.restore_save_header(&header)?;
        assert!(state.has("Reel_Name"));
        assert!(!state.has("reel_name"));
        assert!(!state.has(" Reel_Name "));
        assert!(!state.has("Film"));
        assert!(!state.has("Art_Name"));
        state.write_save_header(&mut header)?;
        assert_eq!(&header[0x158..0x15a], &[1, 2]);
        header[0x158..0x15a].copy_from_slice(&[2, 1]);
        state.restore_save_header(&header)?;
        state.write_save_header(&mut header)?;
        assert_eq!(&header[0x158..0x15a], &[3, 3]);
        header[0x158..0x15a].fill(0);
        state.restore_save_header(&header)?;
        state.write_save_header(&mut header)?;
        assert_eq!(&header[0x158..0x15a], &[3, 3]);
        assert!(state.restore_save_header(&header[..0x319]).is_err());
        header[7] = 15;
        assert!(state.restore_save_header(&header).is_err());
        Ok(())
    }
    #[test]
    fn save_header_preserves_other_owners_and_native_reused_flag_bits() -> Result<()> {
        let mut state = State::load(&database())?;
        state.entries[0].unlocked = true;
        let mut header = vec![0xa4; super::super::save_game::HEADER_SIZE];
        header[..4].copy_from_slice(b"GAME");
        header[4..8].copy_from_slice(&16u32.to_be_bytes());
        header[0x158] = 0x82;
        let before = header.clone();
        state.write_save_header(&mut header)?;
        assert_eq!(
            header[0x158], 0x83,
            "Unlocked branch must retain old secondary/high bits"
        );
        assert_eq!(header[0x159], 0, "Locked branch must clear the entire byte");
        assert_eq!(header[..0x158], before[..0x158]);
        assert_eq!(header[0x15a..], before[0x15a..]);
        Ok(())
    }
    #[test]
    fn malformed_clb_is_rejected_without_panicking() {
        let bytes = database();
        for length in 0..bytes.len() {
            assert!(State::load(&bytes[..length]).is_err());
        }
        let mut bad = bytes;
        bad[12] = 0;
        assert!(State::load(&bad).is_err());
    }
}
