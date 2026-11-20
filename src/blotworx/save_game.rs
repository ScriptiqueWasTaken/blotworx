//! EM1 GAME v16 memory-save container (REL25ddcc..25eb6c).
//! This codec does not claim that all registered gameplay sections can be restored.
use anyhow::{ensure, Context, Result};
use std::{borrow::Cow, io::Read};

pub const HEADER_SIZE: usize = 0x320;
pub const LAST: [u8; 4] = *b"LAST";
fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset + 4)
            .context("Truncated save word")?
            .try_into()?,
    ))
}

pub struct Section<'a> {
    pub tag: [u8; 4],
    pub version: u32,
    pub payload: Cow<'a, [u8]>,
    raw: &'a [u8],
}
pub struct Archive<'a> {
    bytes: &'a [u8],
    pub sections: Vec<Section<'a>>,
    pub logical_size: usize,
}
impl<'a> Archive<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(bytes.len() >= HEADER_SIZE, "Truncated GAME save header");
        ensure!(bytes.starts_with(b"GAME"), "Not an EM1 GAME save");
        ensure!(
            word(bytes, 4)? == 16,
            "Unsupported GAME save version {}",
            word(bytes, 4)?
        );
        let mut sections = Vec::new();
        let mut cursor = HEADER_SIZE;
        loop {
            let header = bytes
                .get(cursor..cursor + 16)
                .context("Save has no complete LAST section")?;
            let tag = header[..4].try_into()?;
            let version = word(header, 4)?;
            let expanded = usize::try_from(word(header, 8)?)?;
            let stored = usize::try_from(word(header, 12)?)?;
            // Native temporary section buffer is 64 KiB; its writer rejects >=64 KiB.
            ensure!(
                expanded < 0x10000,
                "Save section exceeds native temporary buffer"
            );
            let end = cursor
                .checked_add(16)
                .and_then(|n| n.checked_add(stored))
                .context("Save section length overflow")?;
            let source = bytes
                .get(cursor + 16..end)
                .context("Truncated save section payload")?;
            let payload = if expanded == stored {
                Cow::Borrowed(source)
            } else {
                let mut decoder = flate2::read::ZlibDecoder::new(source);
                let mut output = Vec::new();
                decoder
                    .by_ref()
                    .take((expanded + 1) as u64)
                    .read_to_end(&mut output)
                    .context("Invalid compressed save section")?;
                ensure!(
                    output.len() == expanded,
                    "Save section decompressed length mismatch"
                );
                ensure!(
                    decoder.total_in() == stored as u64,
                    "Trailing compressed save data"
                );
                Cow::Owned(output)
            };
            sections.push(Section {
                tag,
                version,
                payload,
                raw: &bytes[cursor..end],
            });
            cursor = end;
            if tag == LAST {
                ensure!(
                    version == 1 && expanded == 0 && stored == 0,
                    "Invalid LAST save section"
                );
                break;
            }
        }
        Ok(Self {
            bytes,
            sections,
            logical_size: cursor,
        })
    }
    /// Preserve the complete existing buffer, including reserved header fields,
    /// unknown sections and unused tail. Never write the source file.
    /// Replacement uses the original manager's uncompressed-section path.
    pub fn replace_section(&self, index: usize, payload: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            index < self.sections.len() && self.sections[index].tag != LAST,
            "Invalid replacement section"
        );
        ensure!(
            payload.len() < 0x10000,
            "Replacement exceeds native section capacity"
        );
        let mut output = self.bytes.to_vec();
        let mut cursor = HEADER_SIZE;
        for (i, section) in self.sections.iter().enumerate() {
            let replacement;
            let raw = if i == index {
                let mut bytes = Vec::with_capacity(16 + payload.len());
                bytes.extend(section.tag);
                bytes.extend(section.version.to_be_bytes());
                bytes.extend((payload.len() as u32).to_be_bytes());
                bytes.extend((payload.len() as u32).to_be_bytes());
                bytes.extend(payload);
                replacement = bytes;
                &replacement[..]
            } else {
                section.raw
            };
            let end = cursor
                .checked_add(raw.len())
                .context("Save buffer size overflow")?;
            output
                .get_mut(cursor..end)
                .context("Replacement exceeds existing save buffer capacity")?
                .copy_from_slice(raw);
            cursor = end;
        }
        Ok(output)
    }
    /// Rebuild through the uncompressed memory writer used by25fd10. Preserve
    /// all section owners/order/versions and the supplied header/storage tail.
    /// This serializes an existing snapshot; it does not capture live gameplay.
    pub fn rebuild_memory(&self) -> Result<BuiltMemory> {
        let mut writer = MemoryWriter::from_storage(self.bytes.to_vec())?;
        for section in &self.sections {
            if section.tag == LAST {
                break;
            }
            writer.append_section(section.tag, section.version, &section.payload)?;
        }
        writer.finish()
    }
    pub fn optional_section(&self, tag: [u8; 4]) -> Result<Option<&Section<'a>>> {
        let mut found = self.sections.iter().filter(|section| section.tag == tag);
        let result = found.next();
        ensure!(found.next().is_none(), "Ambiguous duplicate save section");
        Ok(result)
    }
    pub fn section(&self, tag: [u8; 4]) -> Result<&Section<'a>> {
        self.optional_section(tag)?
            .context("Required save section missing")
    }
    pub fn header_string(&self, offset: usize, length: usize) -> Result<&str> {
        let bytes = self
            .bytes
            .get(offset..offset + length)
            .filter(|_| offset + length <= HEADER_SIZE)
            .context("Invalid save header string range")?;
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .context("Unterminated save header string")?;
        Ok(std::str::from_utf8(&bytes[..end])?)
    }
}

/// REL25de50 initializes only these header fields. Names, other metadata,
/// reserved bytes and allocation tail are deliberately left to their owners.
/// Call once on a newly allocated header, not on each checkpoint: doing so
/// would erase header-owned collectible/extra state between saves.
pub fn initialize_header(bytes: &mut [u8]) -> Result<()> {
    ensure!(
        bytes.len() >= HEADER_SIZE,
        "Truncated GAME header allocation"
    );
    bytes[..4].copy_from_slice(b"GAME");
    bytes[4..8].copy_from_slice(&16u32.to_be_bytes());
    for range in [
        8..12,
        0x2c0..0x2c5,
        0x158..0x1d8,
        0x298..0x2b1,
        0x1d8..0x298,
        0x2c5..0x2ce,
        0x2b4..0x2c0,
        0x2ce..0x30e,
        0x310..0x320,
    ] {
        bytes[range].fill(0);
    }
    Ok(())
}

/// REL26029c..2604d4 resets gameplay sections, retaining only the
/// header-owned progression and presentation records for the next run.
pub(super) fn complete_playthrough(source: &[u8]) -> Result<BuiltMemory> {
    let archive = Archive::parse(source)?;
    ensure!(source[0x2c4] == 0, "Save slot already records a completed playthrough");
    ensure!(archive.optional_section(*b"MICK")?.is_some(), "No campaign checkpoint to complete");

    let mut bytes = vec![0; source.len()];
    initialize_header(&mut bytes)?;
    for range in [0x158..0x298, 0x298..0x2b1, 0x2b4..0x2c0, 0x2c5..0x30e, 0x310..0x320] {
        bytes[range.clone()].copy_from_slice(&source[range]);
    }
    let playthrough = word(source, 0x2c0)?.wrapping_add(1);
    bytes[0x2c0..0x2c4].copy_from_slice(&playthrough.to_be_bytes());
    bytes[0x2c4] = 1;
    MemoryWriter::from_storage(bytes)?.finish()
}

pub struct BuiltMemory {
    pub bytes: Vec<u8>,
    pub logical_size: usize,
}
/// Native uncompressed section stream25e860/25e8e4 with the manager's LAST1
/// terminator25fef8. The caller supplies the real header and storage allocation;
/// no gameplay section or header owner is invented or silently omitted here.
/// Live owner coordination is in checkpoint.rs; restore and disk jobs remain
/// separate from successful memory-stream construction.
pub struct MemoryWriter {
    bytes: Vec<u8>,
    cursor: usize,
    failed: bool,
}
impl MemoryWriter {
    pub fn from_storage(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() >= HEADER_SIZE,
            "Memory save allocation is smaller than GAME header"
        );
        ensure!(
            bytes.starts_with(b"GAME") && word(&bytes, 4)? == 16,
            "Memory writer needs an initialized GAME16 header"
        );
        Ok(Self {
            bytes,
            cursor: HEADER_SIZE,
            failed: false,
        })
    }
    pub fn append_section(&mut self, tag: [u8; 4], version: u32, payload: &[u8]) -> Result<()> {
        let result = (|| {
            ensure!(
                !self.failed,
                "Memory save construction already failed; begin a new checkpoint"
            );
            ensure!(tag != LAST, "LAST is emitted by memory-save finalization");
            self.append(tag, version, payload)
        })();
        //25fe84..25fee0 aborts the manager before LAST on a section failure.
        // Never let ignored errors turn a partial stream into a valid snapshot.
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn append(&mut self, tag: [u8; 4], version: u32, payload: &[u8]) -> Result<()> {
        let length = aligned_payload_length(payload.len())?;
        let end = self
            .cursor
            .checked_add(16)
            .and_then(|offset| offset.checked_add(length))
            .context("Memory section length overflow")?;
        // The native code copies to caller-owned memory. Diagnose an invalid
        // destination capacity instead of writing outside its allocation.
        ensure!(
            end <= self.bytes.len(),
            "Memory save exceeds its supplied storage capacity"
        );
        let header = &mut self.bytes[self.cursor..self.cursor + 16];
        header[..4].copy_from_slice(&tag);
        header[4..8].copy_from_slice(&version.to_be_bytes());
        header[8..12].copy_from_slice(&(length as u32).to_be_bytes());
        header[12..16].copy_from_slice(&(length as u32).to_be_bytes());
        let start = self.cursor + 16;
        self.bytes[start..start + payload.len()].copy_from_slice(payload);
        //25ec00..25ed28 explicitly zeroes alignment bytes, not allocator data.
        self.bytes[start + payload.len()..end].fill(0);
        self.cursor = end;
        Ok(())
    }
    pub fn finish(mut self) -> Result<BuiltMemory> {
        ensure!(!self.failed, "Cannot finalize an incomplete memory save");
        self.append(LAST, 1, &[])?;
        Ok(BuiltMemory {
            bytes: self.bytes,
            logical_size: self.cursor,
        })
    }
}
fn aligned_payload_length(length: usize) -> Result<usize> {
    ensure!(
        length < 0x10000,
        "Save section exceeds native temporary buffer"
    );
    if length % 4 == 0 {
        return Ok(length);
    }
    let aligned = length
        .checked_add(3)
        .context("Section alignment overflow")?
        & !3;
    //25eb6c returns early for an already-aligned cursor; otherwise the write
    // branch checks aligned cursor PLUS its16-byte section header, strictly <64K.
    ensure!(
        aligned + 16 < 0x10000,
        "Native section alignment exceeds temporary capacity"
    );
    Ok(aligned)
}

/// Named records inside each section: u16 total length, u16 absolute data
/// offset in section, NUL-terminated case-folded name, alignment padding, data.
pub struct Record<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}
pub fn records(payload: &[u8]) -> Result<Vec<Record<'_>>> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < payload.len() {
        // EndSection aligns its final payload cursor to four bytes; these
        // bytes are allocator contents, not another named record.
        if payload.len() - cursor < 4 && payload.len() % 4 == 0 {
            break;
        }
        //25e7cc aligns each record header independently of its data type.
        // String records can end on any byte, unlike the numeric-only cases.
        cursor = cursor
            .checked_add(3)
            .context("Save record alignment overflow")?
            & !3;
        let header = payload
            .get(cursor..cursor + 4)
            .context("Truncated named save record")?;
        let length = u16::from_be_bytes(header[..2].try_into()?) as usize;
        let data = u16::from_be_bytes(header[2..].try_into()?) as usize;
        let end = cursor
            .checked_add(length)
            .context("Save record size overflow")?;
        ensure!(
            length >= 5 && end <= payload.len() && data >= cursor + 5 && data <= end,
            "Invalid named save record bounds"
        );
        let name = &payload[cursor + 4..data];
        let terminator = name
            .iter()
            .position(|&byte| byte == 0)
            .context("Unterminated named save record")?;
        result.push(Record {
            name: std::str::from_utf8(&name[..terminator])?,
            data: &payload[data..end],
        });
        cursor = end;
    }
    Ok(result)
}
pub fn append_record(
    output: &mut Vec<u8>,
    name: &str,
    data: &[u8],
    alignment: usize,
) -> Result<()> {
    ensure!(
        matches!(alignment, 1 | 4),
        "Unsupported native save record alignment"
    );
    ensure!(
        name.is_ascii() && !name.as_bytes().contains(&0),
        "Unsupported save record name encoding"
    );
    //25e414 always aligns the record start to4;25e530 separately aligns data.
    let start = aligned_payload_length(output.len())?;
    let header_end = start
        .checked_add(5 + name.len())
        .context("Save name length overflow")?;
    let data_start = header_end
        .checked_add(alignment - 1)
        .context("Save record alignment overflow")?
        / alignment
        * alignment;
    let end = data_start
        .checked_add(data.len())
        .context("Save record size overflow")?;
    ensure!(end < 0x10000, "Save record exceeds native section capacity");
    ensure!(
        data_start == header_end || data_start + 16 < 0x10000,
        "Save data alignment exceeds native temporary capacity"
    );
    let length = u16::try_from(end - start)?;
    let offset = u16::try_from(data_start)?;
    output.resize(start, 0);
    output.extend(length.to_be_bytes());
    output.extend(offset.to_be_bytes());
    output.extend(name.to_ascii_uppercase().as_bytes());
    output.push(0);
    output.resize(data_start, 0);
    output.extend(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_playthrough_retains_only_native_header_owners() -> Result<()> {
        let mut source = vec![0; 0x10000];
        initialize_header(&mut source)?;
        source[0x10..0x14].copy_from_slice(b"test");
        for range in [0x158..0x298, 0x298..0x2b1, 0x2b4..0x2c0, 0x2c5..0x30e, 0x310..0x320] {
            for index in range {
                source[index] = index as u8;
            }
        }
        source[0x2c0..0x2c4].copy_from_slice(&3u32.to_be_bytes());
        let mut writer = MemoryWriter::from_storage(source)?;
        writer.append_section(*b"MICK", 3, &[7, 8, 9, 10])?;
        let source = writer.finish()?.bytes;
        let completed = complete_playthrough(&source)?;
        let archive = Archive::parse(&completed.bytes)?;
        assert_eq!(archive.sections.len(), 1);
        assert_eq!(archive.sections[0].tag, LAST);
        assert_eq!(archive.header_string(0x10, 60)?, "");
        assert_eq!(word(&completed.bytes, 0x2c0)?, 4);
        assert_eq!(completed.bytes[0x2c4], 1);
        for range in [0x158..0x2b1, 0x2b4..0x2c0, 0x2c5..0x30e, 0x310..0x320] {
            assert_eq!(completed.bytes[range.clone()], source[range]);
        }
        assert!(complete_playthrough(&completed.bytes).is_err());
        Ok(())
    }

    #[test]
    fn header_initialization_matches_original_instruction_reference() -> Result<()> {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("save_header_instruction_reference.json"))?;
        assert_eq!(
            reference["rel_sha256"],
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        for case in reference["cases"]
            .as_array()
            .context("Missing original constructor cases")?
        {
            let size = case["size"].as_u64().context("Missing case size")? as usize;
            let mut bytes = match case["name"].as_str().context("Missing case name")? {
                "zero" => vec![0; size],
                "a5" => vec![0xa5; size],
                "pattern" => (0..size).map(|i| (i * 37 + 11) as u8).collect(),
                other => anyhow::bail!("Unknown constructor reference {other}"),
            };
            initialize_header(&mut bytes)?;
            let digest = ring::digest::digest(&ring::digest::SHA256, &bytes)
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(
                digest,
                case["sha256"].as_str().unwrap(),
                "Original header {}",
                case["name"]
            );
        }
        Ok(())
    }
    #[test]
    fn memory_writer_preserves_header_tail_order_versions_and_zeroes_padding() -> Result<()> {
        let mut storage = vec![0xa5; 0x1000];
        initialize_header(&mut storage)?;
        let header = storage[..HEADER_SIZE].to_vec();
        let mut writer = MemoryWriter::from_storage(storage)?;
        writer.append_section(*b"QUES", 4, b"abc")?;
        writer.append_section(*b"GLOB", 1, &[1, 2, 3, 4])?;
        writer.append_section(*b"EXTR", 1, &[])?;
        let result = writer.finish()?;
        let archive = Archive::parse(&result.bytes)?;
        assert_eq!(&result.bytes[..HEADER_SIZE], header);
        assert!(result.bytes[result.logical_size..]
            .iter()
            .all(|&byte| byte == 0xa5));
        assert_eq!(archive.logical_size, result.logical_size);
        assert_eq!(
            archive
                .sections
                .iter()
                .map(|s| (s.tag, s.version))
                .collect::<Vec<_>>(),
            vec![(*b"QUES", 4), (*b"GLOB", 1), (*b"EXTR", 1), (LAST, 1)]
        );
        assert_eq!(&*archive.sections[0].payload, b"abc\0");
        assert_eq!(&*archive.sections[1].payload, &[1, 2, 3, 4]);
        assert!(archive.sections[2].payload.is_empty());
        assert_eq!(archive.rebuild_memory()?.bytes, result.bytes);
        Ok(())
    }
    #[test]
    fn empty_slot_inspection_has_no_player_and_duplicate_optional_sections_reject() -> Result<()> {
        let mut storage = vec![0; 0x10000];
        initialize_header(&mut storage)?;
        let empty = MemoryWriter::from_storage(storage.clone())?.finish()?;
        let archive = Archive::parse(&empty.bytes)?;
        assert!(archive.optional_section(*b"MICK")?.is_none());
        assert!(archive.section(*b"MICK").is_err());
        assert_eq!(archive.rebuild_memory()?.bytes, empty.bytes);
        let mut writer = MemoryWriter::from_storage(storage)?;
        writer.append_section(*b"MICK", 3, &[])?;
        writer.append_section(*b"MICK", 3, &[])?;
        let duplicate = writer.finish()?;
        let archive = Archive::parse(&duplicate.bytes)?;
        assert!(archive.optional_section(*b"MICK").is_err());
        Ok(())
    }
    #[test]
    fn memory_writer_rejects_capacity_and_retains_native_alignment_boundary() -> Result<()> {
        for length in [0, 4, 0xffec, 0xfff0, 0xfffc] {
            assert_eq!(aligned_payload_length(length)?, length);
        }
        assert_eq!(aligned_payload_length(0xffeb)?, 0xffec);
        for length in [0xffed, 0xffef, 0xfff1, 0xffff, 0x10000] {
            assert!(aligned_payload_length(length).is_err(), "{length:#x}");
        }
        let mut storage = vec![0xa5; HEADER_SIZE + 16];
        initialize_header(&mut storage)?;
        let mut writer = MemoryWriter::from_storage(storage)?;
        let previous = writer.bytes.clone();
        assert!(writer.append_section(*b"GLOB", 1, &[0; 4]).is_err());
        assert_eq!(writer.bytes, previous);
        assert_eq!(writer.cursor, HEADER_SIZE);
        assert!(writer.append_section(*b"EXTR", 1, &[]).is_err());
        assert!(
            writer.finish().is_err(),
            "Failed write produced a valid partial archive"
        );
        assert_eq!(
            MemoryWriter::from_storage(previous.clone())?
                .finish()?
                .logical_size,
            HEADER_SIZE + 16
        );
        let mut writer = MemoryWriter::from_storage(previous)?;
        assert!(writer.append_section(LAST, 1, &[]).is_err());
        assert!(writer.finish().is_err());
        assert!(MemoryWriter::from_storage(vec![0; HEADER_SIZE]).is_err());
        assert!(initialize_header(&mut [0; HEADER_SIZE - 1]).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires an original EM1 save at BLOTWORX_TEST_SAVE"]
    fn original_save_rebuilds_with_native_uncompressed_memory_writer() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let source = Archive::parse(&bytes)?;
        let rebuilt = source.rebuild_memory()?;
        let copy = Archive::parse(&rebuilt.bytes)?;
        assert_eq!(&bytes[..HEADER_SIZE], &rebuilt.bytes[..HEADER_SIZE]);
        assert_eq!(source.sections.len(), copy.sections.len());
        for (original, copy) in source.sections.iter().zip(&copy.sections) {
            assert_eq!((original.tag, original.version), (copy.tag, copy.version));
            assert_eq!(
                &*original.payload, &*copy.payload,
                "Original memory section {:?}",
                original.tag
            );
        }
        if source
            .sections
            .iter()
            .all(|section| section.payload.len() + 16 == section.raw.len())
        {
            assert_eq!(
                bytes, rebuilt.bytes,
                "Native raw saves must rebuild byte-for-byte, including unused tail"
            );
        }
        assert_eq!(std::fs::read(path)?, bytes, "Source save was changed");
        Ok(())
    }
    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0xa5; 0x1000];
        bytes[..4].copy_from_slice(b"GAME");
        bytes[4..8].copy_from_slice(&16_u32.to_be_bytes());
        let mut cursor = HEADER_SIZE;
        for (tag, version, data) in [(*b"TEST", 3_u32, &b"abc"[..]), (LAST, 1, &b""[..])] {
            bytes[cursor..cursor + 4].copy_from_slice(&tag);
            bytes[cursor + 4..cursor + 8].copy_from_slice(&version.to_be_bytes());
            for n in [8, 12] {
                bytes[cursor + n..cursor + n + 4]
                    .copy_from_slice(&(data.len() as u32).to_be_bytes());
            }
            bytes[cursor + 16..cursor + 16 + data.len()].copy_from_slice(data);
            cursor += 16 + data.len();
        }
        bytes
    }
    #[test]
    fn replacement_preserves_header_unknown_sections_and_storage_tail() -> Result<()> {
        let original = fixture();
        let save = Archive::parse(&original)?;
        assert_eq!(save.sections.len(), 2);
        assert_eq!(save.replace_section(0, b"abc")?, original);
        let changed = save.replace_section(0, b"xyz")?;
        assert_eq!(&changed[..HEADER_SIZE], &original[..HEADER_SIZE]);
        assert_eq!(
            &changed[save.logical_size..],
            &original[save.logical_size..]
        );
        assert_eq!(
            &*Archive::parse(&changed)?.section(*b"TEST")?.payload,
            b"xyz"
        );
        Ok(())
    }
    #[test]
    fn named_records_use_absolute_offsets_and_native_alignment() -> Result<()> {
        let mut bytes = Vec::new();
        append_record(&mut bytes, "gdmarraysize", &3_u32.to_be_bytes(), 4)?;
        append_record(
            &mut bytes,
            "gdmarray",
            &[0, 2, 1, 0xff, 0xff, 0xfe, 0, 0xff],
            4,
        )?;
        assert_eq!(&bytes[..4], &[0, 24, 0, 20]);
        assert_eq!(&bytes[24..28], &[0, 24, 0, 40]);
        let parsed = records(&bytes)?;
        assert_eq!(parsed[0].name, "GDMARRAYSIZE");
        assert_eq!(parsed[1].data.len(), 8);
        bytes[26] = 0xff;
        assert!(records(&bytes).is_err());
        Ok(())
    }
    #[test]
    fn string_record_headers_align_independently_from_byte_aligned_data() -> Result<()> {
        let mut bytes = Vec::new();
        append_record(&mut bytes, "0", b"ab\0", 1)?;
        assert_eq!(&bytes[..6], &[0, 9, 0, 6, b'0', 0]);
        assert_eq!(bytes.len(), 9);
        append_record(&mut bytes, "1", b"cd\0", 1)?;
        assert_eq!(&bytes[9..12], &[0, 0, 0]);
        assert_eq!(&bytes[12..18], &[0, 9, 0, 18, b'1', 0]);
        append_record(&mut bytes, "n", &123u32.to_be_bytes(), 4)?;
        let parsed = records(&bytes)?;
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].data, b"ab\0");
        assert_eq!(parsed[1].data, b"cd\0");
        assert_eq!(parsed[2].data, &123u32.to_be_bytes());
        // Native scan skips inter-record padding even when it is nonzero.
        bytes[9..12].fill(0xa5);
        assert_eq!(records(&bytes)?.len(), 3);
        let mut boundary = vec![0; 65517];
        let before = boundary.clone();
        assert!(append_record(&mut boundary, "", &[], 1).is_err());
        assert_eq!(boundary, before);
        Ok(())
    }
    #[test]
    fn compressed_sections_validate_expansion_and_preserve_original_encoding() -> Result<()> {
        use std::io::Write;
        let data = vec![42; 256];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data)?;
        let compressed = encoder.finish()?;
        let mut bytes = fixture();
        let start = HEADER_SIZE;
        bytes[start + 8..start + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        bytes[start + 12..start + 16].copy_from_slice(&(compressed.len() as u32).to_be_bytes());
        bytes[start + 16..start + 16 + compressed.len()].copy_from_slice(&compressed);
        let end = start + 16 + compressed.len();
        bytes[end..end + 16].copy_from_slice(&[76, 65, 83, 84, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        let save = Archive::parse(&bytes)?;
        assert_eq!(&*save.sections[0].payload, &data);
        let replacement = save.replace_section(0, &data)?;
        assert_eq!(&*Archive::parse(&replacement)?.sections[0].payload, &data);
        bytes[start + 8..start + 12].copy_from_slice(&255_u32.to_be_bytes());
        assert!(Archive::parse(&bytes).is_err());
        Ok(())
    }
    #[test]
    fn malformed_lengths_and_missing_terminal_reject() -> Result<()> {
        let original = fixture();
        assert!(Archive::parse(&original[..HEADER_SIZE + 16]).is_err());
        let mut bytes = original.clone();
        bytes[HEADER_SIZE + 8..HEADER_SIZE + 12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Archive::parse(&bytes).is_err());
        assert!(records(&[0, 0, 0, 0]).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires an original EM1 save at BLOTWORX_TEST_SAVE"]
    fn original_memory_save_sections_and_lossless_rewrite() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(path)?;
        let save = Archive::parse(&bytes)?;
        assert_eq!(save.sections.last().unwrap().tag, LAST);
        for (index, section) in save
            .sections
            .iter()
            .enumerate()
            .filter(|(_, section)| section.tag != LAST)
        {
            // WRLD's numbered strings are also named records. Their headers
            // align to4 after variable-length, byte-aligned string payloads.
            if matches!(
                &section.tag,
                b"GLOB" | b"QUES" | b"MICK" | b"AISS" | b"WRLD"
            ) {
                let parsed = records(&section.payload)
                    .with_context(|| format!("Section {:?}", section.tag))?;
                for record in parsed {
                    ensure!(!record.name.is_empty(), "Empty original record name");
                }
            }
            if section.payload.len() + 16 == section.raw.len() {
                assert_eq!(save.replace_section(index, &section.payload)?, bytes);
            }
        }
        assert!(!save.section(*b"GLOB")?.payload.is_empty());
        Ok(())
    }
}
