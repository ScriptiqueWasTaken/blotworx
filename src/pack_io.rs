//! Bounded EM1 archive I/O. Reuse compressed bytes of unchanged entries when
//! rebuilding; compress edited/new assets once, instead of once per size field.
use anyhow::{Context, Result, ensure};
use epic_mickey_lib_rs::packfile::{EndianDependentString, Packfile, VirtualFile};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    ops::Range,
    path::Path,
};
pub struct StoredPack {
    pub pak: Packfile,
    bytes: Vec<u8>,
    ranges: Vec<Range<usize>>,
    little_endian: bool,
}
/// Header-only resource identity. Payloads are read only when requested; a
/// running level does not retain a second uncompressed copy of its archives.
#[derive(Clone, Debug)]
pub struct Member {
    pub path: String,
    pub tag: String,
    pub size: usize,
    pub packed_size: usize,
    pub offset: u64,
}
pub fn index(path: &Path) -> Result<Vec<Member>> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let mut prefix = [0; 32];
    file.read_exact(&mut prefix)?;
    let little = archive_endian(&prefix)?;
    let u32_at = |data: &[u8], at| number_at(data, at, little);
    let header = u32_at(&prefix, 12)? as usize;
    ensure!(header == 32, "Unsupported packfile header layout");
    let data_offset = header
        .checked_add(u32_at(&prefix, 16)? as usize)
        .context("Packfile header overflow")?;
    ensure!(
        data_offset as u64 <= length && data_offset >= header + 4,
        "Invalid packfile table range"
    );
    let mut table = vec![0; data_offset - header];
    file.seek(SeekFrom::Start(header as u64))?;
    file.read_exact(&mut table)?;
    let count = u32_at(&table, 0)? as usize;
    ensure!(count <= 100000, "Packfile entry limit exceeded");
    let strings = 4usize
        .checked_add(count.checked_mul(24).context("Packfile table overflow")?)
        .context("Packfile table overflow")?;
    ensure!(strings <= table.len(), "Invalid packfile string table");
    let string = |offset: u32| -> Result<String> {
        let at = strings
            .checked_add(offset as usize)
            .context("Packfile string overflow")?;
        let bytes = table.get(at..).context("Invalid packfile string offset")?;
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .context("Unterminated packfile string")?;
        Ok(std::str::from_utf8(&bytes[..end])?.into())
    };
    let mut offset = data_offset as u64;
    let mut members = Vec::with_capacity(count);
    for i in 0..count {
        let at = 4 + i * 24;
        let size = u32_at(&table, at)? as usize;
        let packed_size = u32_at(&table, at + 4)? as usize;
        let aligned = u32_at(&table, at + 8)? as u64;
        ensure!(
            size <= 512 * 1024 * 1024 && packed_size as u64 <= aligned && aligned % 32 == 0,
            "Invalid packfile entry sizes"
        );
        let folder = string(u32_at(&table, at + 12)?)?;
        let name = string(u32_at(&table, at + 20)?)?;
        let tag = archive_tag(&table[at + 16..at + 20], little)?;
        let path = if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        };
        let next = offset
            .checked_add(aligned)
            .context("Packfile alignment overflow")?;
        ensure!(next <= length, "Truncated packfile payload or padding");
        members.push(Member {
            path,
            tag,
            size,
            packed_size,
            offset,
        });
        offset = next;
    }
    Ok(members)
}
impl Member {
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let mut file = std::fs::File::open(path)?;
        self.read_from(&mut file)
    }

    /// Read one indexed payload from an already-open archive. This lets the
    /// runtime stream selected members without retaining both the compressed
    /// archive and every inflated file at once.
    pub fn read_from(&self, file: &mut std::fs::File) -> Result<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(self.offset))?;
        if self.size == self.packed_size {
            let mut bytes = vec![0; self.size];
            file.read_exact(&mut bytes)?;
            return Ok(bytes);
        }
        let mut bytes = Vec::new();
        flate2::read::ZlibDecoder::new(file.take(self.packed_size as u64))
            .take(self.size as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() == self.size,
            "Packfile decompressed size mismatch"
        );
        Ok(bytes)
    }
}
fn archive_endian(bytes: &[u8]) -> Result<bool> {
    match bytes.get(..4) {
        Some(b" KAP") => Ok(false),
        Some(b"PAK ") => Ok(true),
        _ => anyhow::bail!("Expected a Wii or PC packfile"),
    }
}
fn number_at(data: &[u8], at: usize, little: bool) -> Result<u32> {
    let bytes = data.get(at..at + 4).context("Truncated packfile header")?.try_into()?;
    Ok(if little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) })
}
fn archive_tag(bytes: &[u8], little: bool) -> Result<String> {
    let mut tag: [u8; 4] = bytes.try_into()?;
    if little { tag.reverse(); }
    Ok(std::str::from_utf8(&tag)?.trim_matches('\0').to_owned())
}
pub fn read(bytes: Vec<u8>) -> Result<StoredPack> {
    read_filtered(bytes, |_, _| true)
}
pub fn read_member(path: &Path, index: usize) -> Result<VirtualFile> {
    let mut stored = read_filtered(std::fs::read(path)?, |i, _| i == index)?;
    ensure!(
        index < stored.pak.files.len(),
        "Archive member index is out of range"
    );
    Ok(stored.pak.files.swap_remove(index))
}
/// Excluded members retain their headers but have empty data; for indexing only.
pub fn read_filtered(bytes: Vec<u8>, include: impl Fn(usize, &str) -> bool) -> Result<StoredPack> {
    let little = archive_endian(&bytes)?;
    let u32_at = |data: &[u8], at| number_at(data, at, little);
    let version = u32_at(&bytes, 4)?;
    let header = u32_at(&bytes, 12)? as usize;
    let mut pos = header
        .checked_add(u32_at(&bytes, 16)? as usize)
        .context("Packfile data offset overflow")?;
    let count = u32_at(&bytes, header)? as usize;
    ensure!(count <= 100000, "Packfile entry limit exceeded");
    let strings = header + 4 + count * 24;
    ensure!(
        strings <= pos && pos <= bytes.len(),
        "Invalid packfile table range"
    );
    ensure!(
        header == 32 && bytes.len() >= 32,
        "Unsupported packfile header layout"
    );
    let strings_end = pos;
    let string = |offset: u32| -> Result<String> {
        let start = strings
            .checked_add(offset as usize)
            .context("String offset overflow")?;
        let remaining = bytes
            .get(start..strings_end)
            .context("Invalid packfile string offset")?;
        let end = remaining
            .iter()
            .position(|b| *b == 0)
            .context("Unterminated packfile string")?;
        Ok(std::str::from_utf8(&remaining[..end])?.into())
    };
    let mut files = vec![];
    let mut ranges = vec![];
    for i in 0..count {
        let at = header + 4 + i * 24;
        let real = u32_at(&bytes, at)? as usize;
        let packed = u32_at(&bytes, at + 4)? as usize;
        let aligned = u32_at(&bytes, at + 8)? as usize;
        ensure!(
            real <= 512 * 1024 * 1024 && packed <= aligned && aligned % 32 == 0,
            "Invalid packfile entry sizes"
        );
        let folder = string(u32_at(&bytes, at + 12)?)?;
        let name = string(u32_at(&bytes, at + 20)?)?;
        let path = if folder.is_empty() {
            name
        } else {
            format!("{folder}/{name}")
        };
        let tag = archive_tag(&bytes[at + 16..at + 20], little)?;
        let end = pos.checked_add(packed).context("Packfile entry overflow")?;
        let compressed = bytes.get(pos..end).context("Truncated packfile payload")?;
        let data = if !include(i, &path) {
            vec![]
        } else if real == packed {
            compressed.to_vec()
        } else {
            let mut data = vec![];
            flate2::read::ZlibDecoder::new(compressed)
                .take(real as u64 + 1)
                .read_to_end(&mut data)?;
            ensure!(data.len() == real, "Packfile decompressed size mismatch");
            data
        };
        ranges.push(pos..end);
        files.push(VirtualFile::new(
            EndianDependentString::new(tag),
            real != packed,
            6,
            path,
            data,
        ));
        pos = pos
            .checked_add(aligned)
            .context("Packfile alignment overflow")?;
        ensure!(pos <= bytes.len(), "Truncated packfile padding");
    }
    Ok(StoredPack {
        pak: Packfile::new(EndianDependentString::new(" KAP".into()), version, files),
        bytes,
        ranges,
        little_endian: little,
    })
}
pub fn write(pak: &Packfile, original: Option<&StoredPack>, path: &Path) -> Result<()> {
    ensure!(!original.is_some_and(|p| p.little_endian), "Writing PC archives is not supported; original archive was left unchanged");
    let mut folders = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut strings = vec![];
    fn intern(s: &str, map: &mut BTreeMap<String, u32>, strings: &mut Vec<u8>) -> u32 {
        if let Some(i) = map.get(s) {
            return *i;
        }
        let i = strings.len() as u32;
        strings.extend(s.as_bytes());
        strings.push(0);
        map.insert(s.into(), i);
        i
    }
    let originals: BTreeMap<_, _> = original
        .into_iter()
        .flat_map(|p| {
            p.pak
                .files
                .iter()
                .enumerate()
                .map(|(i, f)| (crate::mod_project::key(&f.path), i))
        })
        .collect();
    let mut payloads = vec![];
    let mut table = vec![];
    table.extend((pak.files.len() as u32).to_be_bytes());
    for f in &pak.files {
        let normalized = f.path.replace('\\', "/");
        let (folder, name) = normalized.rsplit_once('/').unwrap_or(("", &normalized));
        let folder = intern(folder, &mut folders, &mut strings);
        let name = intern(name, &mut names, &mut strings);
        let reuse = original
            .and_then(|p| {
                originals
                    .get(&crate::mod_project::key(&f.path))
                    .map(|i| (p, *i))
            })
            .filter(|(p, i)| {
                p.pak.files[*i].data == f.data && p.pak.files[*i].compress == f.compress
            });
        let payload = if let Some((p, i)) = reuse {
            p.bytes[p.ranges[i].clone()].to_vec()
        } else if f.compress {
            let mut encoder = flate2::write::ZlibEncoder::new(
                vec![],
                flate2::Compression::new(f.compression_level),
            );
            encoder.write_all(&f.data)?;
            encoder.finish()?
        } else {
            f.data.clone()
        };
        ensure!(
            !f.compress || payload.len() != f.data.len(),
            "Compressed size aliases raw size for {}; disable compression for this entry",
            f.path
        );
        let aligned = (payload.len() + 31) & !31;
        for n in [f.data.len(), payload.len(), aligned, folder as usize] {
            table.extend(u32::try_from(n)?.to_be_bytes());
        }
        let mut tag = f.type_.text.as_bytes().to_vec();
        ensure!(tag.len() <= 4, "Invalid packfile type tag");
        tag.resize(4, 0);
        table.extend(tag);
        table.extend(name.to_be_bytes());
        payloads.push((payload, aligned));
    }
    let data_offset = (32 + table.len() + strings.len() + 31) & !31;
    let mut out = original
        .map(|p| p.bytes[..32].to_vec())
        .unwrap_or_else(|| vec![0u8; 32]);
    out[..4].copy_from_slice(b" KAP");
    out[4..8].copy_from_slice(&pak.version.to_be_bytes());
    out[12..16].copy_from_slice(&32u32.to_be_bytes());
    out[16..20].copy_from_slice(&u32::try_from(data_offset - 32)?.to_be_bytes());
    out.extend(table);
    out.extend(strings);
    out.resize(data_offset, 0);
    for (data, size) in payloads {
        let start = out.len();
        out.extend(data);
        out.resize(start + size, 0);
    }
    let check = read(out.clone())?;
    ensure!(
        check.pak.files.len() == pak.files.len(),
        "Packfile verification count mismatch"
    );
    for (a, b) in pak.files.iter().zip(&check.pak.files) {
        ensure!(
            a.path == b.path
                && a.data == b.data
                && a.type_.text == b.type_.text
                && a.compress == b.compress,
            "Packfile verification failed for {}",
            a.path
        );
    }
    crate::mod_project::atomic_save(path, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_archive_reads_both_payload_encodings_and_rejects_writes() -> Result<()> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("blotworx-pc-{}-{stamp}.pak", std::process::id()));
        let forbidden = path.with_extension("rewrite.pak");
        let files = vec![
            VirtualFile::new(EndianDependentString::new("BIN".into()), true, 6, "scene/main.bin".into(), vec![7; 4096]),
            VirtualFile::new(EndianDependentString::new("NIF".into()), false, 6, "models/main.nif".into(), vec![1,2,3,4]),
        ];
        let pak = Packfile::new(EndianDependentString::new(" KAP".into()), 2, files);
        let result = (|| -> Result<()> {
            write(&pak, None, &path)?;
            let mut bytes = std::fs::read(&path)?;
            // PC stores every numeric table/header word and type tag reversed.
            for at in (0..32).step_by(4) { bytes[at..at+4].reverse(); }
            bytes[32..36].reverse();
            for at in (36..36 + 24 * pak.files.len()).step_by(4) { bytes[at..at+4].reverse(); }
            std::fs::write(&path, &bytes)?;
            let decoded = read(bytes)?;
            let members = index(&path)?;
            for ((expected, actual), member) in pak.files.iter().zip(&decoded.pak.files).zip(members) {
                ensure!(expected.path == actual.path && expected.data == actual.data && expected.type_.text == actual.type_.text);
                ensure!(member.tag == expected.type_.text && member.read(&path)? == expected.data);
            }
            ensure!(write(&decoded.pak, Some(&decoded), &forbidden).is_err());
            ensure!(!forbidden.exists());
            Ok(())
        })();
        let _ = std::fs::remove_file(&path);
        result
    }

    #[test]
    fn indexed_member_reads_match_full_archive_decode() -> Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "blotworx-pack-read-from-{}-{stamp}.pak",
            std::process::id()
        ));
        let files = vec![
            VirtualFile::new(
                EndianDependentString::new("BIN".into()),
                true,
                6,
                "scene/compressed.bin".into(),
                vec![0x5a; 32 * 1024],
            ),
            VirtualFile::new(
                EndianDependentString::new("NIF".into()),
                false,
                6,
                "models/raw.nif_wii".into(),
                (0..=255).cycle().take(4096).collect(),
            ),
        ];
        let pack = Packfile::new(EndianDependentString::new(" KAP".into()), 1, files);
        let result = (|| -> Result<()> {
            write(&pack, None, &path)?;
            let full = read(std::fs::read(&path)?)?;
            let indexed = index(&path)?;
            ensure!(indexed.len() == full.pak.files.len());
            let mut archive = std::fs::File::open(&path)?;
            for (member, expected) in indexed.iter().zip(&full.pak.files) {
                ensure!(member.path == expected.path);
                ensure!(member.read_from(&mut archive)? == expected.data);
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(&path);
        result
    }
}

pub fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| anyhow::anyhow!("The format parser rejected this data"))?
}

