//! Preserve original NIF blocks that the library can read but cannot write.
use anyhow::{Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile, RawBlockData};

pub struct OriginalNif {
    pub bytes: Vec<u8>,
    pub blocks: Vec<std::ops::Range<usize>>,
    pub parsed: Vec<serde_json::Value>,
}

impl OriginalNif {
    pub fn capture(bytes: &[u8], nif: &NifFile) -> Result<Self> {
        Ok(Self {
            bytes: bytes.to_vec(),
            blocks: block_ranges(bytes)?,
            parsed: nif
                .blocks
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()?,
        })
    }
    pub fn save(&self, nif: &NifFile) -> Result<Vec<u8>> {
        anyhow::ensure!(
            nif.blocks.len() == self.blocks.len(),
            "Adding or removing NIF blocks requires updating all block references"
        );
        let current: Vec<_> = nif
            .blocks
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<_, _>>()?;
        let original = NifFile::load_bytes(&self.bytes).map_err(|e| anyhow::anyhow!(e))?;
        if current == self.parsed
            && nif.string_table == original.string_table
            && nif.roots == original.roots
        {
            return Ok(self.bytes.clone());
        }
        // Fixed-size edits to opaque blocks need no header or string-table rebuild.
        // Their serialized type names are metadata, not game string-table entries.
        if nif.string_table == original.string_table && nif.roots == original.roots
            && nif.blocks.iter().enumerate().all(|(i,b)| current[i]==self.parsed[i] || matches!(b,NiObject::Unknown(raw) if raw.type_name==original.blocks[i].type_name() && raw.raw_data.len()==self.blocks[i].len())) {
            let mut bytes=self.bytes.clone();
            for (i,block) in nif.blocks.iter().enumerate() {
                if current[i]!=self.parsed[i] { if let NiObject::Unknown(raw)=block { bytes[self.blocks[i].clone()].copy_from_slice(&raw.raw_data); } }
            }
            let check=NifFile::load_bytes(&bytes).map_err(anyhow::Error::msg)?;
            for (i,block) in check.blocks.iter().enumerate() {
                // Some legacy library extra-data readers overrun their block into
                // adjacent bytes. Unedited blocks are preserved physically above;
                // only the deliberately changed block needs semantic verification.
                if current[i]!=self.parsed[i] {
                    anyhow::ensure!(serde_json::to_value(block)?==current[i],"Edited NIF block {i} did not reopen faithfully");
                }
            }
            return Ok(bytes);
        }
        let mut output = NifFile::load_bytes(&self.bytes).map_err(|e| anyhow::anyhow!(e))?;
        output.roots = nif.roots.clone();
        output.string_table = nif.string_table.clone();
        for i in 0..nif.blocks.len() {
            let type_name = original.block_types[original.block_type_indices[i] as usize].clone();
            if current[i] == self.parsed[i] {
                output.blocks[i] = NiObject::Unknown(RawBlockData {
                    type_name,
                    raw_data: self.bytes[self.blocks[i].clone()].to_vec(),
                });
            } else if let (
                NiObject::NiWiiUTextureData(before),
                NiObject::NiWiiUTextureData(after),
            ) = (&original.blocks[i], &nif.blocks[i])
            {
                let mut metadata = after.clone();
                metadata.raw_pixels = before.raw_pixels.clone();
                anyhow::ensure!(
                    serde_json::to_value(metadata)? == serde_json::to_value(before)?,
                    "Texture dimensions and format must remain unchanged"
                );
                anyhow::ensure!(
                    before.raw_pixels.len() == after.raw_pixels.len(),
                    "Texture payload size must be preserved"
                );
                let mut raw = self.bytes[self.blocks[i].clone()].to_vec();
                let start = raw
                    .len()
                    .checked_sub(before.raw_pixels.len())
                    .context("Invalid texture block")?;
                anyhow::ensure!(
                    raw[start..] == before.raw_pixels,
                    "Texture payload offset is not verified"
                );
                raw[start..].copy_from_slice(&after.raw_pixels);
                output.blocks[i] = NiObject::Unknown(RawBlockData {
                    type_name,
                    raw_data: raw,
                });
            } else {
                output.blocks[i] = nif.blocks[i].clone();
                if !matches!(nif.blocks[i], NiObject::Unknown(_)) {
                    add_strings(&current[i], &mut output.string_table);
                }
            }
        }
        let mut bytes = output.to_bytes().map_err(|e| anyhow::anyhow!(e))?;
        if original.version == 0x14060500 {
            let serialized_ranges = block_ranges(&bytes)?;
            let mut repaired = false;
            for i in 0..nif.blocks.len() {
                if current[i] == self.parsed[i] {
                    continue;
                }
                if let (NiObject::NiMesh(before), NiObject::NiMesh(after)) =
                    (&original.blocks[i], &nif.blocks[i])
                {
                    let count = 8
                        + if original.user_version > 14 { 4 } else { 0 }
                        + if original.user_version > 3 { 12 } else { 0 };
                    let offset = self.blocks[i].start + mesh_prefix_size(before);
                    let extension = self
                        .bytes
                        .get(offset..offset + count)
                        .context("Truncated MouseTrap mesh fields")?;
                    let mut raw = bytes[serialized_ranges[i].clone()].to_vec();
                    let at = mesh_prefix_size(after);
                    anyhow::ensure!(at <= raw.len(), "Invalid generated mesh header");
                    raw.splice(at..at, extension.iter().copied());
                    output.blocks[i] = NiObject::Unknown(RawBlockData {
                        type_name: "NiMesh".into(),
                        raw_data: raw,
                    });
                    repaired = true;
                }
            }
            if repaired {
                bytes = output.to_bytes().map_err(|e| anyhow::anyhow!(e))?;
            }
        }
        let check = NifFile::load_bytes(&bytes).map_err(|e| anyhow::anyhow!(e))?;
        anyhow::ensure!(
            check.blocks.len() == nif.blocks.len(),
            "NIF block count changed on save"
        );
        for (i, block) in check.blocks.iter().enumerate() {
            if current[i] != self.parsed[i] {
                anyhow::ensure!(serde_json::to_value(block)?==current[i],"Block {i} cannot be written faithfully by the current serializer; original archive is unchanged");
            }
        }
        let ranges = block_ranges(&bytes)?;
        for i in 0..ranges.len() {
            if current[i] == self.parsed[i] {
                anyhow::ensure!(
                    bytes[ranges[i].clone()] == self.bytes[self.blocks[i].clone()],
                    "Untouched NIF block changed"
                );
            }
        }
        Ok(bytes)
    }

    pub fn integer_value(&self, nif: &NifFile, index: usize) -> Option<i32> {
        let range = self.blocks.get(index)?;
        let kind = nif
            .block_types
            .get(*nif.block_type_indices.get(index)? as usize)?;
        if kind != "NiIntegerExtraData" || range.len() != 8 {
            return None;
        }
        let bytes = self
            .bytes
            .get(range.start + 4..range.end)?
            .try_into()
            .ok()?;
        Some(if nif.source_is_little_endian {
            i32::from_le_bytes(bytes)
        } else {
            i32::from_be_bytes(bytes)
        })
    }
    pub fn patch_integer(
        &self,
        nif: &NifFile,
        index: usize,
        value: i32,
    ) -> Result<(Vec<u8>, NifFile)> {
        anyhow::ensure!(
            self.integer_value(nif, index).is_some(),
            "Unsupported integer extra-data layout"
        );
        let mut bytes = self.save(nif)?;
        let ranges = block_ranges(&bytes)?;
        let range = &ranges[index];
        let encoded = if nif.source_is_little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[range.start + 4..range.end].copy_from_slice(&encoded);
        let parsed = NifFile::load_bytes(&bytes).map_err(|e| anyhow::anyhow!(e))?;
        Ok((bytes, parsed))
    }
}

pub fn mesh_prefix_size(mesh: &epic_mickey_lib_rs::nif_file::NiMeshData) -> usize {
    87 + 4 * mesh.base.base.base.extra_data_links.len()
        + 4 * mesh.base.base.property_links.len()
        + 8 * mesh.base.materials.len()
}

fn add_strings(value: &serde_json::Value, strings: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => {
            if !strings.contains(s) {
                strings.push(s.clone());
            }
        }
        serde_json::Value::Array(a) => {
            for v in a {
                add_strings(v, strings)
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values() {
                add_strings(v, strings)
            }
        }
        _ => {}
    }
}

pub fn block_ranges(data: &[u8]) -> Result<Vec<std::ops::Range<usize>>> {
    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
        le: bool,
    }
    impl Reader<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8]> {
            let end = self.pos.checked_add(n).context("NIF offset overflow")?;
            let b = self.data.get(self.pos..end).context("Truncated NIF")?;
            self.pos = end;
            Ok(b)
        }
        fn u32(&mut self) -> Result<u32> {
            let le = self.le;
            let b = self.take(4)?.try_into()?;
            Ok(if le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        }
        fn u16(&mut self) -> Result<u16> {
            let le = self.le;
            let b = self.take(2)?.try_into()?;
            Ok(if le {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            })
        }
        fn string(&mut self) -> Result<()> {
            let n = self.u32()? as usize;
            self.take(n)?;
            Ok(())
        }
    }
    let pos = data
        .iter()
        .position(|b| *b == b'\n')
        .context("No NIF header")?
        + 1;
    let mut r = Reader {
        data,
        pos,
        le: true,
    };
    let version = r.u32()?;
    anyhow::ensure!(
        version >= 0x14020005,
        "Lossless editing requires NIF block size metadata"
    );
    let le = r.take(1)?[0] != 0;
    r.u32()?;
    let count = r.u32()? as usize;
    anyhow::ensure!(count < 100000, "Too many NIF blocks");
    r.le = le;
    let types = r.u16()?;
    for _ in 0..types {
        r.string()?;
    }
    r.take(count * 2)?;
    let sizes: Vec<_> = (0..count).map(|_| r.u32()).collect::<Result<_>>()?;
    let strings = r.u32()?;
    r.u32()?;
    anyhow::ensure!(strings < 100000, "Too many NIF strings");
    for _ in 0..strings {
        r.string()?;
    }
    let groups = r.u32()?;
    for _ in 1..groups {
        r.u32()?;
    }
    let mut ranges = Vec::new();
    for size in sizes {
        let start = r.pos;
        r.take(size as usize)?;
        ranges.push(start..r.pos);
    }
    Ok(ranges)
}
