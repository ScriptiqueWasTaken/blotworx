//! KynogonRuntimeMesh v2 loading42add4, ID normalization42b8cc and
//! neighbor binding42b9c8/42bab4. References are portable arena addresses.
use super::ai_graph_link_passes::Memory;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

const UNRESOLVED: u32 = 0xf000baa4;
fn add(base: u32, offset: u32) -> Result<u32> {
    base.checked_add(offset)
        .context("Runtime mesh address overflow")
}
fn word(m: &Memory, base: u32, offset: u32) -> Result<u32> {
    m.word(add(base, offset)?)
}
fn swap(m: &mut Memory, cursor: &mut u32) -> Result<u32> {
    let v = m.word(*cursor)?.swap_bytes();
    m.write(*cursor, v)?;
    *cursor = add(*cursor, 4)?;
    Ok(v)
}
fn skip(m: &mut Memory, cursor: &mut u32) -> Result<()> {
    m.write(*cursor, UNRESOLVED)?;
    *cursor = add(*cursor, 4)?;
    Ok(())
}
fn words(m: &mut Memory, cursor: &mut u32, count: u32) -> Result<()> {
    for _ in 0..count {
        swap(m, cursor)?;
    }
    Ok(())
}
fn fixup(m: &mut Memory, cursor: &mut u32, pending: &mut Vec<(u32, u32)>) -> Result<()> {
    pending.push((*cursor, m.word(*cursor)?.swap_bytes()));
    skip(m, cursor)
}
fn records(
    m: &mut Memory,
    cursor: &mut u32,
    count: u32,
    unbound: bool,
    base: u32,
    mapping: &mut BTreeMap<u32, u32>,
    pending: &mut Vec<(u32, u32)>,
) -> Result<()> {
    for _ in 0..count {
        if unbound {
            mapping.entry(*cursor - base).or_insert(*cursor);
        }
        words(m, cursor, 6)?;
        fixup(m, cursor, pending)?;
        swap(m, cursor)?;
        if unbound {
            words(m, cursor, 3)?;
        }
    }
    Ok(())
}
fn point_lists(m: &mut Memory, cursor: &mut u32, count: u32, patches: bool) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    let table = *cursor;
    for _ in 0..=count {
        swap(m, cursor)?;
        if patches {
            words(m, cursor, 3)?;
        }
    }
    let stride = if patches { 16 } else { 4 };
    let first = m.word(table)?;
    let end = word(
        m,
        table,
        count
            .checked_mul(stride)
            .context("Mesh point table overflow")?,
    )?;
    // Native unsigned division by12 (mulhwu/shift), followed by XYZ reads.
    let points = end.wrapping_sub(first) / 12;
    for _ in 0..points {
        words(m, cursor, 3)?;
    }
    Ok(())
}
fn floors(m: &Memory, sector: u32) -> Result<Vec<u32>> {
    let rows = word(m, sector, 0x24)?;
    let cols = word(m, sector, 0x28)?;
    let count = rows
        .checked_mul(cols)
        .context("Runtime mesh cell count overflow")?;
    let table = word(m, sector, 0x30)?;
    let mut result = Vec::new();
    for i in 0..count {
        let cell = word(m, table, i.checked_mul(4).context("Cell table overflow")?)?;
        for layer in 0..m.word(cell)? {
            result.push(word(
                m,
                cell,
                4u32.checked_add(layer.checked_mul(12).context("Floor table overflow")?)
                    .context("Floor table overflow")?,
            )?);
        }
    }
    Ok(result)
}
/// Original loader mutates its owned buffer, leaving source assets untouched.
/// All supported v2 source/destination cursors remain four-byte aligned and
/// advance together; skipped serialized offsets are replaced with pointers.
pub(super) fn load(m: &mut Memory, base: u32) -> Result<u32> {
    for (i, byte) in b"KynogonRuntimeMesh\0".iter().enumerate() {
        if m.byte(add(base, i as u32)?)? != *byte {
            return Ok(0);
        }
    }
    let sector = add(base, 20)?;
    match word(m, sector, 4)? {
        0 => {}
        old if old == base => {
            clear_neighbors(m, sector, None)?;
            return Ok(sector);
        }
        _ => return Ok(0),
    }
    m.swap_halfword(sector)?;
    let version = u16::from_be_bytes([m.byte(sector)?, m.byte(add(sector, 1)?)?]);
    if version != 2 {
        m.write_byte(sector, 0)?;
        m.write_byte(add(sector, 1)?, 0)?;
        return Ok(0);
    }
    m.swap_halfword(add(sector, 2)?)?;
    let flags_low = add(sector, 3)?;
    m.write_byte(flags_low, m.byte(flags_low)? & !2)?;
    m.write(add(sector, 4)?, base)?;
    let mut cursor = add(sector, 8)?;
    words(m, &mut cursor, 10)?;
    cursor = add(cursor, 4)?; // serialized grid offset is skipped by42a84c
    let neighbors = swap(m, &mut cursor)?;
    cursor = add(cursor, 4)?;
    m.write(add(sector, 0x30)?, cursor)?;
    let count = word(m, sector, 0x24)?
        .checked_mul(word(m, sector, 0x28)?)
        .context("Runtime mesh cell count overflow")?;
    let grid = cursor;
    for _ in 0..=count {
        skip(m, &mut cursor)?;
    }
    let mut mapping = BTreeMap::new();
    let mut pending = Vec::new();
    for cell_index in 0..count {
        m.write(
            add(
                grid,
                cell_index.checked_mul(4).context("Cell table overflow")?,
            )?,
            cursor,
        )?;
        let layers = swap(m, &mut cursor)?;
        if layers == 0 {
            continue;
        }
        let table = cursor;
        for _ in 0..layers {
            skip(m, &mut cursor)?;
            words(m, &mut cursor, 2)?;
        }
        skip(m, &mut cursor)?;
        for layer in 0..layers {
            let floor = cursor;
            m.write(
                add(
                    table,
                    layer.checked_mul(12).context("Floor table overflow")?,
                )?,
                floor,
            )?;
            mapping.entry(floor - base).or_insert(floor);
            words(m, &mut cursor, 20)?;
            for (offset, unbound) in [(0x20, false), (0x28, false), (0x30, true)] {
                let count = word(m, floor, offset)?;
                records(
                    m,
                    &mut cursor,
                    count,
                    unbound,
                    base,
                    &mut mapping,
                    &mut pending,
                )?;
            }
            for (offset, patches) in [(0x38, false), (0x40, false), (0x48, true)] {
                let count = word(m, floor, offset)?;
                point_lists(m, &mut cursor, count, patches)?;
            }
        }
        m.write(
            add(
                table,
                layers.checked_mul(12).context("Floor sentinel overflow")?,
            )?,
            cursor,
        )?;
    }
    m.write(
        add(
            grid,
            count.checked_mul(4).context("Grid sentinel overflow")?,
        )?,
        cursor,
    )?;
    swap(m, &mut cursor)?; // neighbor table's serialized position word
    m.write(add(sector, 0x38)?, if neighbors == 0 { 0 } else { cursor })?;
    let table = cursor;
    for _ in 0..neighbors {
        words(m, &mut cursor, 2)?;
        skip(m, &mut cursor)?;
    }
    for i in 0..neighbors {
        let header = add(table, i.checked_mul(12).context("Neighbor table overflow")?)?;
        let groups = word(m, header, 4)?;
        m.write(add(header, 8)?, cursor)?;
        let pointers = cursor;
        for _ in 0..groups {
            skip(m, &mut cursor)?;
        }
        for j in 0..groups {
            m.write(
                add(
                    pointers,
                    j.checked_mul(4).context("Neighbor groups overflow")?,
                )?,
                cursor,
            )?;
            words(m, &mut cursor, 3)?;
            let links = swap(m, &mut cursor)?;
            for _ in 0..links {
                fixup(m, &mut cursor, &mut pending)?;
            }
        }
    }
    mapping.entry(u32::MAX).or_insert(0);
    for (destination, offset) in pending {
        if let Some(&pointer) = mapping.get(&offset) {
            m.write(destination, pointer)?;
        }
    }
    let floors = floors(m, sector)?;
    if let Some(&first) = floors.first() {
        let delta = word(m, sector, 8)?.wrapping_sub(m.word(first)?);
        if delta != 0 {
            for floor in floors {
                m.write(floor, m.word(floor)?.wrapping_add(delta))?;
            }
            for i in 0..neighbors {
                let at = add(table, i.checked_mul(12).context("Neighbor table overflow")?)?;
                m.write(at, m.word(at)?.wrapping_add(delta))?;
            }
        }
    }
    Ok(sector)
}
fn neighbor_groups(m: &Memory, sector: u32, target: Option<u32>) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    let table = word(m, sector, 0x38)?;
    for i in 0..word(m, sector, 0x34)? {
        let header = add(table, i.checked_mul(12).context("Neighbor table overflow")?)?;
        let id = m.word(header)?;
        if target.is_some_and(|target| target != id) {
            continue;
        }
        let pointers = word(m, header, 8)?;
        for j in 0..word(m, header, 4)? {
            result.push(word(
                m,
                pointers,
                j.checked_mul(4).context("Neighbor groups overflow")?,
            )?);
        }
    }
    Ok(result)
}
pub(super) fn clear_neighbors(m: &mut Memory, sector: u32, target: Option<u32>) -> Result<()> {
    for group in neighbor_groups(m, sector, target)? {
        for i in 0..word(m, group, 12)? {
            let link = word(
                m,
                group,
                16u32
                    .checked_add(i.checked_mul(4).context("Neighbor link table overflow")?)
                    .context("Neighbor link table overflow")?,
            )?;
            m.write(add(link, 0x18)?, 0)?;
            m.write(add(link, 0x1c)?, 0)?;
        }
    }
    Ok(())
}
pub(super) fn bind_neighbor(m: &mut Memory, sector: u32, target: u32) -> Result<()> {
    let id = word(m, target, 8)?;
    for group in neighbor_groups(m, sector, Some(id))? {
        let x = m.word(group)?;
        let z = word(m, group, 4)?;
        let layer = word(m, group, 8)?;
        let index = x
            .checked_mul(word(m, target, 0x28)?)
            .and_then(|v| v.checked_add(z))
            .context("Neighbor cell overflow")?;
        let cell = word(
            m,
            word(m, target, 0x30)?,
            index.checked_mul(4).context("Neighbor cell overflow")?,
        )?;
        let offset = 4u32
            .checked_add(layer.checked_mul(12).context("Neighbor layer overflow")?)
            .context("Neighbor layer overflow")?;
        let floor = word(m, cell, offset)?;
        let size = word(
            m,
            cell,
            offset
                .checked_add(12)
                .context("Neighbor floor end overflow")?,
        )?
        .wrapping_sub(floor);
        for i in 0..word(m, group, 12)? {
            let link = word(
                m,
                group,
                16u32
                    .checked_add(i.checked_mul(4).context("Neighbor link table overflow")?)
                    .context("Neighbor link table overflow")?,
            )?;
            m.write(add(link, 0x18)?, floor)?;
            m.write(add(link, 0x1c)?, size)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct Floor {
    pub sector: u32,
    pub cell: [u32; 2],
    pub layer: u32,
    pub address: u32,
}
/// A database owns all its relocated sectors, so cross-sector links address
/// the same canonical bytes as floor search and later traversal queries.
pub(super) struct Store {
    memory: Memory,
    sectors: Vec<(u32, u32)>,
    next: u32,
    pub floor_count: usize,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            memory: Memory {
                regions: BTreeMap::from([(0x1000, vec![0; 16])]),
                resources: BTreeMap::new(),
                stride: 0,
                journal: BTreeMap::new(),
            },
            sectors: Vec::new(),
            next: 0x10000,
            floor_count: 0,
        }
    }
}
impl Store {
    pub fn insert(&mut self, source: &crate::ai_nav::Sector) -> Result<()> {
        ensure!(
            !self.sectors.iter().any(|(id, _)| *id == source.id),
            "Duplicate runtime sector identity"
        );
        let bytes = source.runtime_mesh_bytes();
        let size = u32::try_from(bytes.len()).context("Runtime mesh exceeds address domain")?;
        let base = self.next;
        let next = base
            .checked_add(size)
            .and_then(|end| end.checked_add(15))
            .map(|end| end & !15)
            .context("Runtime mesh arena exhausted its32-bit data references")?;
        self.memory.regions.insert(base, bytes.to_vec());
        let result = (|| -> Result<(u32, usize)> {
            let sector = load(&mut self.memory, base)?;
            ensure!(sector != 0, "Runtime mesh loader rejected resource");
            ensure!(
                word(&self.memory, sector, 8)? == source.id,
                "Runtime mesh sector identity mismatch"
            );
            Ok((sector, floors(&self.memory, sector)?.len()))
        })();
        match result {
            Ok((sector, count)) => {
                self.sectors.push((source.id, sector));
                self.floor_count += count;
                self.next = next;
                Ok(())
            }
            Err(error) => {
                self.memory.regions.remove(&base);
                Err(error)
            }
        }
    }
    pub fn bind_all(&mut self) -> Result<()> {
        let lookup = self.sectors.iter().copied().collect::<BTreeMap<_, _>>();
        for &(_, sector) in &self.sectors {
            let table = word(&self.memory, sector, 0x38)?;
            for i in 0..word(&self.memory, sector, 0x34)? {
                let id = word(
                    &self.memory,
                    table,
                    i.checked_mul(12).context("Neighbor table overflow")?,
                )?;
                if let Some(&target) = lookup.get(&id) {
                    bind_neighbor(&mut self.memory, sector, target)?;
                }
            }
        }
        Ok(())
    }
    pub fn find_floor(&mut self, position: [f32; 3]) -> Result<Option<Floor>> {
        for (i, value) in position.into_iter().enumerate() {
            self.memory.write(0x1000 + i as u32 * 4, value.to_bits())?;
        }
        for &(_, sector) in &self.sectors {
            let floor = super::ai_cell_search::find_cell(&self.memory, sector, 0x1000)?;
            if floor != 0 {
                return Ok(Some(Floor {
                    sector: self.memory.word(floor)?,
                    cell: [word(&self.memory, floor, 4)?, word(&self.memory, floor, 8)?],
                    layer: word(&self.memory, floor, 12)?,
                    address: floor,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(s: &str) -> Result<Vec<u8>> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
            .collect()
    }
    fn memory(base: u32, bytes: Vec<u8>) -> Memory {
        Memory {
            regions: BTreeMap::from([(base, bytes)]),
            resources: BTreeMap::new(),
            stride: 0,
            journal: BTreeMap::new(),
        }
    }
    fn hash(bytes: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    #[test]
    fn original_loader_matches_relocated_buffers() -> Result<()> {
        let f: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_runtime_mesh.json"
        ))?;
        let base = f["base"].as_u64().unwrap() as u32;
        for case in f["cases"].as_array().unwrap() {
            let mut m = memory(base, decode(case["input"].as_str().unwrap())?);
            assert_eq!(load(&mut m, base)?, case["result"].as_u64().unwrap() as u32);
            match case["operation"].as_str() {
                Some("reload") => {
                    assert_eq!(load(&mut m, base)?, base + 20);
                }
                Some("bind" | "unbind") => {
                    let target = base + 0x100000;
                    m.regions
                        .insert(target, decode(case["target"].as_str().unwrap())?);
                    assert_eq!(load(&mut m, target)?, target + 20);
                    bind_neighbor(&mut m, base + 20, target + 20)?;
                    if case["operation"] == "unbind" {
                        clear_neighbors(&mut m, base + 20, Some(78))?;
                    }
                }
                _ => {}
            }
            assert_eq!(m.regions[&base], decode(case["output"].as_str().unwrap())?);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_meshes_match_original_loader_and_find_authored_nodes() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let f: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_runtime_mesh.json"
        ))?;
        let base = f["base"].as_u64().unwrap() as u32;
        let mut store = Store::default();
        let mut found = 0;
        for c in f["production"].as_array().unwrap() {
            let source =
                std::fs::read(std::path::Path::new(&root).join(c["path"].as_str().unwrap()))?;
            let source = crate::ai_nav::Sector::load(&source)?;
            let bytes = source.runtime_mesh_bytes();
            assert_eq!(hash(bytes), c["input_sha256"].as_str().unwrap());
            let mut m = memory(base, bytes.to_vec());
            assert_eq!(load(&mut m, base)?, c["result"].as_u64().unwrap() as u32);
            assert_eq!(
                hash(&m.regions[&base]),
                c["output_sha256"].as_str().unwrap()
            );
            m.regions.insert(0x66000000, vec![0; 16]);
            for query in c["queries"].as_array().unwrap() {
                for i in 0..3 {
                    m.write(
                        0x66000000 + i * 4,
                        query["point"][i as usize].as_u64().unwrap() as u32,
                    )?;
                }
                let floor = super::super::ai_cell_search::find_cell(&m, base + 20, 0x66000000)?;
                assert_eq!(floor, query["result"].as_u64().unwrap() as u32);
                found += usize::from(floor != 0);
            }
            store.insert(&source)?;
        }
        store.bind_all()?;
        ensure!(
            store.floor_count > 0,
            "Original GV meshes produced no runtime floors"
        );
        ensure!(found > 0, "Original graph-node floor queries all failed");
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn all_imported_prelinked_sectors_construct_and_bind_runtime_meshes() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut failures = Vec::new();
        let mut sectors = 0;
        let mut floor_count = 0;
        for directory in std::fs::read_dir(std::path::Path::new(&root).join("sectors"))? {
            let directory = directory?;
            if !directory.file_type()?.is_dir() {
                continue;
            }
            let mut store = Store::default();
            for file in std::fs::read_dir(directory.path())? {
                let file = file?;
                let path = file.path();
                if !file.file_type()?.is_file()
                    || !file
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".prelnk.obj")
                {
                    continue;
                }
                sectors += 1;
                let result = (|| -> Result<()> {
                    let source = crate::ai_nav::Sector::load(&std::fs::read(&path)?)?;
                    store.insert(&source)
                })();
                if let Err(error) = result {
                    failures.push(format!("{}: {error:#}", path.display()));
                }
            }
            if let Err(error) = store.bind_all() {
                failures.push(format!(
                    "{} neighbors: {error:#}",
                    directory.path().display()
                ));
            }
            floor_count += store.floor_count;
        }
        ensure!(
            sectors > 0 && floor_count > 0,
            "No original navigation corpus was tested"
        );
        std::fs::write(
            "target/ai-runtime-mesh-corpus.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "sectors":sectors,"floors":floor_count,"failures":failures,
            "scope":"Entire imported prelinked-sector corpus constructed and neighbor-bound; original instruction equality is separately checked for the two GV reference meshes. Not NPC behavior or complete navigation validation."}))?,
        )?;
        ensure!(
            failures.is_empty(),
            "Runtime mesh corpus failures:\n{}",
            failures.join("\n")
        );
        Ok(())
    }
}
