//! Spatial polyline43168c/42e794, step42e3b4 and cached cell selection
//! 431930/431a28/431a80. Cell search42f8a8 and trace42e46c remain required.
use super::ai_graph_link_passes::Memory;
use anyhow::Result;
fn word(m: &Memory, b: u32, o: u32) -> Result<u32> {
    m.word(b.wrapping_add(o))
}
const EMPTY: [u32; 7] = [u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0, u32::MAX, 0];
/// Native stack words stay local; caller caches retain their actual data
/// references, so input/output aliasing is not replaced by snapshot copies.
enum Cache<'a> {
    Borrowed(&'a mut [u32; 7]),
    Local([u32; 7]),
    Address(u32),
}
impl Cache<'_> {
    fn get(&self, m: &Memory, i: usize) -> Result<u32> {
        match self {
            Self::Local(w) => Ok(w[i]),
            Self::Borrowed(w) => Ok(w[i]),
            Self::Address(p) => word(m, *p, i as u32 * 4),
        }
    }
    fn set(&mut self, m: &mut Memory, i: usize, v: u32) -> Result<()> {
        match self {
            Self::Local(w) => {
                w[i] = v;
                Ok(())
            }
            Self::Borrowed(w) => {
                w[i] = v;
                Ok(())
            }
            Self::Address(p) => m.write(p.wrapping_add(i as u32 * 4), v),
        }
    }
    fn clear(&mut self, m: &mut Memory) -> Result<()> {
        for (i, v) in EMPTY.into_iter().enumerate() {
            self.set(m, i, v)?;
        }
        Ok(())
    }
    fn bind(&mut self, m: &mut Memory, provider: u32, index: u32, cell: u32) -> Result<()> {
        let first = [
            word(m, cell, 0)?,
            word(m, cell, 4)?,
            word(m, cell, 8)?,
            word(m, cell, 12)?,
        ];
        for (i, v) in first.into_iter().enumerate() {
            self.set(m, i, v)?;
        }
        self.set(m, 4, provider)?;
        self.set(m, 5, index)?;
        self.set(m, 6, cell)
    }
}
/// Query1c supplies a single copied stack word; other native callers may
/// supply a data reference. Neither variant is an executable address.
#[derive(Clone, Copy)]
pub enum Filter {
    Value(u32),
    Address(u32),
}
impl Filter {
    pub fn word(self, m: &Memory) -> Result<u32> {
        match self {
            Self::Value(v) => Ok(v),
            Self::Address(p) => m.word(p),
        }
    }
    pub fn set_word(&mut self, m: &mut Memory, v: u32) -> Result<()> {
        match self {
            Self::Value(w) => {
                *w = v;
                Ok(())
            }
            Self::Address(p) => m.write(*p, v),
        }
    }
}
pub trait Host {
    fn find_cell42f8a8(&mut self, m: &mut Memory, sector: u32, point: u32) -> Result<u32> {
        super::ai_cell_search::find_cell(m, sector, point)
    }
    fn trace42e46c(
        &mut self,
        m: &mut Memory,
        cell: u32,
        start: u32,
        end: u32,
        radius: u32,
        filter: &mut Option<Filter>,
        flags: u32,
    ) -> Result<u32>;
}
pub fn sector_index(m: &Memory, provider: u32, id: u32) -> Result<u32> {
    let count = word(m, provider, 12)?;
    for index in 0..count {
        let slot = word(m, provider, 8)?.wrapping_add(index.wrapping_mul(8));
        if m.byte(slot)? == 1 && word(m, word(m, slot, 4)?, 8)? == id {
            return Ok(index);
        }
    }
    Ok(u32::MAX)
}
fn search(
    m: &mut Memory,
    provider: u32,
    point: u32,
    cache: &mut Cache,
    host: &mut impl Host,
) -> Result<u32> {
    search_with(m, provider, cache, &mut |m, sector| {
        host.find_cell42f8a8(m, sector, point)
    })
}
fn search_with(
    m: &mut Memory,
    provider: u32,
    cache: &mut Cache<'_>,
    find: &mut impl FnMut(&mut Memory, u32) -> Result<u32>,
) -> Result<u32> {
    let mut index = 0u32;
    while index < word(m, provider, 12)? {
        let slot = word(m, provider, 8)?.wrapping_add(index.wrapping_mul(8));
        if m.byte(slot)? != 0 {
            let sector = word(m, slot, 4)?;
            let cell = find(m, sector)?;
            if cell != 0 {
                cache.bind(m, provider, index, cell)?;
                return Ok(1);
            }
        }
        index = index.wrapping_add(1);
    }
    cache.clear(m)?;
    Ok(0)
}
fn resolve(
    m: &mut Memory,
    provider: u32,
    point: u32,
    cache: &mut Cache,
    host: &mut impl Host,
) -> Result<()> {
    if cache.get(m, 1)? == u32::MAX {
        search(m, provider, point, cache, host)?;
        return Ok(());
    }
    let slot = word(m, provider, 8)?.wrapping_add(cache.get(m, 5)?.wrapping_mul(8));
    if m.byte(slot)? == 1 && word(m, word(m, slot, 4)?, 8)? == cache.get(m, 0)? {
        return Ok(());
    }
    let index = sector_index(m, provider, cache.get(m, 0)?)?;
    if index == u32::MAX {
        search(m, provider, point, cache, host)?;
        return Ok(());
    }
    cache.set(m, 5, index)?;
    let slot = word(m, provider, 8)?.wrapping_add(index.wrapping_mul(8));
    let sector = word(m, slot, 4)?;
    let grid = word(m, sector, 0x30)?;
    let width = word(m, sector, 0x28)?;
    let offset = cache
        .get(m, 1)?
        .wrapping_mul(width)
        .wrapping_add(cache.get(m, 2)?)
        .wrapping_mul(4);
    let list = m.word(grid.wrapping_add(offset))?;
    let cell = word(m, list, cache.get(m, 3)?.wrapping_mul(12).wrapping_add(4))?;
    cache.set(m, 6, cell)
}
pub fn select(
    m: &mut Memory,
    provider: u32,
    point: u32,
    cache: u32,
    host: &mut impl Host,
) -> Result<()> {
    resolve(m, provider, point, &mut Cache::Address(cache), host)
}
pub fn find(
    m: &mut Memory,
    provider: u32,
    point: u32,
    cache: u32,
    host: &mut impl Host,
) -> Result<u32> {
    search(m, provider, point, &mut Cache::Address(cache), host)
}
///431a80 over the caller's actual inline cache. Position is transient query
///data; it is never mapped as a second mutable parent/point record.
pub fn find_position(
    m: &mut Memory,
    provider: u32,
    point: [f32; 3],
    cache: &mut [u32; 7],
) -> Result<u32> {
    search_with(
        m,
        provider,
        &mut Cache::Borrowed(cache),
        &mut |m, sector| super::ai_cell_search::find_cell_position(m, sector, point),
    )
}
pub fn step(
    m: &mut Memory,
    cell: u32,
    start: u32,
    end: u32,
    radius: u32,
    last: Option<&mut u32>,
    filter: &mut Option<Filter>,
    flags: u32,
    host: &mut impl Host,
) -> Result<u32> {
    let cell = host.trace42e46c(m, cell, start, end, radius, filter, flags)?;
    if cell == 0 {
        return Ok(0);
    }
    if let Some(last) = last.as_ref() {
        if **last != 0 {
            return Ok(u32::from(**last == cell));
        }
    }
    let y = f32::from_bits(word(m, end, 4)?);
    let inside =
        y <= f32::from_bits(word(m, cell, 0x18)?) && y >= f32::from_bits(word(m, cell, 0x1c)?);
    if inside {
        if let Some(last) = last {
            *last = cell;
        }
    }
    Ok(u32::from(inside))
}
pub fn in_cell(
    m: &mut Memory,
    mut cell: u32,
    points: u32,
    count: u32,
    radius: u32,
    last: Option<&mut u32>,
    mut output: Option<&mut u32>,
    filter: &mut Option<Filter>,
    flags: u32,
    host: &mut impl Host,
) -> Result<u32> {
    if count < 2 {
        return Ok(0);
    }
    let mut previous = points;
    let mut next = points.wrapping_add(12);
    let mut last_cell = 0;
    for index in 1..count {
        last_cell = 0;
        if step(
            m,
            cell,
            previous,
            next,
            radius,
            Some(&mut last_cell),
            filter,
            flags,
            host,
        )? as u8
            == 0
        {
            if let Some(output) = output.as_mut() {
                **output = index - 1;
            }
            return Ok(0);
        }
        cell = last_cell;
        previous = next;
        next = next.wrapping_add(12);
    }
    if let Some(last) = last {
        *last = last_cell;
    }
    Ok(1)
}
pub fn polyline(
    m: &mut Memory,
    provider: u32,
    points: u32,
    count: u32,
    radius: u32,
    input_cache: Option<u32>,
    output_cache: Option<u32>,
    output: Option<&mut u32>,
    filter: &mut Option<Filter>,
    flags: u32,
    host: &mut impl Host,
) -> Result<u32> {
    if count < 2 {
        return Ok(0);
    }
    let mut cache = input_cache
        .map(Cache::Address)
        .unwrap_or(Cache::Local(EMPTY));
    resolve(m, provider, points, &mut cache, host)?;
    let cell = cache.get(m, 6)?;
    if cell == 0 {
        if let Some(p) = output_cache {
            Cache::Address(p).clear(m)?;
        }
        return Ok(0);
    }
    let mut last = 0;
    if in_cell(
        m,
        cell,
        points,
        count,
        radius,
        Some(&mut last),
        output,
        filter,
        flags,
        host,
    )? as u8
        == 0
    {
        if let Some(p) = output_cache {
            Cache::Address(p).clear(m)?;
        }
        return Ok(0);
    }
    if let Some(p) = output_cache {
        if last != 0 {
            let index = sector_index(m, provider, word(m, last, 0)?)?;
            if index != u32::MAX {
                Cache::Address(p).bind(m, provider, index, last)?;
            } else {
                Cache::Address(p).clear(m)?;
            }
        } else {
            Cache::Address(p).clear(m)?;
        }
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ai_segment_query::WalkPoint, ai_spatial_queries as spatial, ai_traversal_cost as cost,
        ai_traversal_exclusion as exclusion,
    };
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const PROVIDER: u32 = 0x60000000;
    const SLOTS: u32 = 0x61000000;
    const SECTORS: u32 = 0x62000000;
    const GRID: u32 = 0x63000000;
    const LISTS: u32 = 0x64000000;
    const CELLS: u32 = 0x65000000;
    const POINTS: u32 = 0x66000000;
    const CACHE: u32 = 0x67000000;
    const OUTCACHE: u32 = CACHE + 0x100;
    const COUNT: u32 = CACHE + 0x200;
    const LAST: u32 = CACHE + 0x300;
    const FILT: u32 = CACHE + 0x400;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn cell(index: i64) -> u32 {
        if index < 0 {
            0
        } else {
            CELLS + index as u32 * 0x100
        }
    }
    fn vector(m: &Memory, p: u32) -> Result<[u32; 3]> {
        Ok([m.word(p)?, m.word(p + 4)?, m.word(p + 8)?])
    }
    struct Spy<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        calls: usize,
    }
    impl Host for Spy<'_> {
        fn find_cell42f8a8(&mut self, m: &mut Memory, sector: u32, point: u32) -> Result<u32> {
            let index = (sector - SECTORS) / 0x100;
            self.trace.push(json!(["find", sector, vector(m, point)?]));
            if self.case["find_mutation"] == "count" {
                m.write(PROVIDER + 12, 0)?;
            }
            Ok(cell(
                self.case["find_cells"][index as usize].as_i64().unwrap(),
            ))
        }
        fn trace42e46c(
            &mut self,
            m: &mut Memory,
            cell: u32,
            start: u32,
            end: u32,
            radius: u32,
            filter: &mut Option<Filter>,
            flags: u32,
        ) -> Result<u32> {
            let value = filter.as_ref().map(|f| f.word(m)).transpose()?;
            self.trace.push(json!([
                "trace",
                cell,
                vector(m, start)?,
                vector(m, end)?,
                radius,
                value,
                flags
            ]));
            if self.case["mutate_filter"] == true {
                if let Some(f) = filter.as_mut() {
                    let value = f.word(m)?.wrapping_add(1);
                    f.set_word(m, value)?;
                }
            }
            if self.case["mutate_point"] == true {
                m.write(end + 4, 1.5f32.to_bits())?;
            }
            let results = self.case["trace_results"].as_array().unwrap();
            let result =
                super::tests::cell(results[self.calls.min(results.len() - 1)].as_i64().unwrap());
            self.calls += 1;
            Ok(result)
        }
    }
    impl exclusion::Services for Spy<'_> {
        fn class20(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected world lookup")
        }
    }
    impl cost::Host for Spy<'_> {
        fn exclusion_world(&self) -> exclusion::World {
            exclusion::World {
                root_slot: 0,
                service_class: 0,
            }
        }
        fn class18(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected class lookup")
        }
        fn query40(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: &mut WalkPoint,
            _: &mut [u32; 3],
            _: u32,
            _: &mut u32,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected spatial cost query")
        }
        fn contribution2c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected contribution")
        }
    }
    // Keep polyline's production default; no alternate implementation here.
    impl spatial::Host for Spy<'_> {
        fn cost_classes(&self) -> cost::Classes {
            cost::Classes {
                spatial: 0,
                contribution: 0,
            }
        }
        fn owner_tag3c(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected owner tag")
        }
        fn sweep34(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected sweep")
        }
        fn direct1c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<u32> {
            anyhow::bail!("Unexpected direct query")
        }
        fn adjust20(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: &mut WalkPoint,
            _: &mut WalkPoint,
            _: u32,
            _: &mut u32,
        ) -> Result<()> {
            anyhow::bail!("Unexpected adjustment")
        }
    }
    fn memory(c: &Value) -> Result<Memory> {
        let mut m = Memory {
            regions: BTreeMap::new(),
            resources: BTreeMap::new(),
            stride: 32,
            journal: BTreeMap::new(),
        };
        let points = c["points"].as_array().unwrap();
        let flags = c["slot_flags"].as_array().unwrap();
        for (at, size) in [
            (PROVIDER, 16),
            (SLOTS, flags.len() * 8),
            (POINTS, points.len() * 12),
            (CACHE, 28),
            (OUTCACHE, 28),
            (COUNT, 4),
            (LAST, 4),
            (FILT, 4),
        ] {
            m.regions.insert(at, vec![0; size]);
        }
        m.write(PROVIDER + 8, SLOTS)?;
        m.write(PROVIDER + 12, bits(&c["slot_count"]))?;
        for (i, f) in flags.iter().enumerate() {
            let i = i as u32;
            for (at, size) in [
                (SECTORS + i * 0x100, 0x34),
                (GRID + i * 0x100, 16),
                (LISTS + i * 0x100, 20),
            ] {
                m.regions.insert(at, vec![0; size]);
            }
            m.write_byte(SLOTS + i * 8, bits(f) as u8)?;
            m.write(SLOTS + i * 8 + 4, SECTORS + i * 0x100)?;
            for (at, v) in [
                (SECTORS + i * 0x100 + 8, bits(&c["ids"][i as usize])),
                (SECTORS + i * 0x100 + 0x28, 2),
                (SECTORS + i * 0x100 + 0x30, GRID + i * 0x100),
                (LISTS + i * 0x100, 2),
                (LISTS + i * 0x100 + 4, cell(i as i64)),
                (LISTS + i * 0x100 + 16, cell(i as i64)),
            ] {
                m.write(at, v)?;
            }
            for j in 0..4 {
                m.write(GRID + i * 0x100 + j * 4, LISTS + i * 0x100)?;
            }
        }
        for (i, id) in c["cell_ids"].as_array().unwrap().iter().enumerate() {
            let p = cell(i as i64);
            m.regions.insert(p, vec![0; 0x20]);
            m.write(p, bits(id))?;
            m.write(p + 0x18, bits(&c["bounds"][i][0]))?;
            m.write(p + 0x1c, bits(&c["bounds"][i][1]))?;
        }
        for (i, p) in points.iter().enumerate() {
            for j in 0..3 {
                m.write(POINTS + i as u32 * 12 + j * 4, bits(&p[j as usize]))?;
            }
        }
        for i in 0..7 {
            m.write(CACHE + i * 4, bits(&c["cache"][i as usize]))?;
            m.write(OUTCACHE + i * 4, 77)?;
        }
        m.write(COUNT, bits(&c["initial_count"]))?;
        m.write(LAST, cell(c["last"].as_i64().unwrap()))?;
        m.write(FILT, c["filter"].as_u64().unwrap_or(0) as u32)?;
        Ok(m)
    }
    #[test]
    fn original_spatial_walk_cache_aliases_segment_counts_heights_and_default_backend_match(
    ) -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_spatial_walk.json"
        ))?;
        for expected in data["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut m = memory(c)?;
            let mut host = Spy {
                case: c,
                trace: vec![],
                calls: 0,
            };
            let mut count = m.word(COUNT)?;
            let mut last = m.word(LAST)?;
            let radius = bits(&c["radius"]);
            let flags = bits(&c["flags"]);
            let mut filter = if c["filter"].is_null() {
                None
            } else {
                Some(Filter::Address(FILT))
            };
            let output_cache = match c["output_cache"].as_str().unwrap() {
                "none" => None,
                "same" => Some(CACHE),
                _ => Some(OUTCACHE),
            };
            let result = match c["op"].as_str().unwrap() {
                "polyline" => polyline(
                    &mut m,
                    PROVIDER,
                    POINTS,
                    bits(&c["count"]),
                    radius,
                    if c["input_cache"] == true {
                        Some(CACHE)
                    } else {
                        None
                    },
                    output_cache,
                    if c["count_output"] == true {
                        Some(&mut count)
                    } else {
                        None
                    },
                    &mut filter,
                    flags,
                    &mut host,
                )?,
                "default" => spatial::Host::polyline(
                    &mut host,
                    &mut m,
                    PROVIDER,
                    POINTS,
                    bits(&c["count"]),
                    radius,
                    &mut count,
                    c["filter"].as_u64().map(|v| v as u32),
                )?,
                "in_cell" => in_cell(
                    &mut m,
                    cell(0),
                    POINTS,
                    bits(&c["count"]),
                    radius,
                    if c["last_output"] == true {
                        Some(&mut last)
                    } else {
                        None
                    },
                    if c["count_output"] == true {
                        Some(&mut count)
                    } else {
                        None
                    },
                    &mut filter,
                    flags,
                    &mut host,
                )?,
                "step" => step(
                    &mut m,
                    cell(0),
                    POINTS,
                    POINTS + 12,
                    radius,
                    if c["last_output"] == true {
                        Some(&mut last)
                    } else {
                        None
                    },
                    &mut filter,
                    flags,
                    &mut host,
                )?,
                "index" => sector_index(&m, PROVIDER, bits(&c["id"]))?,
                "find" => find(&mut m, PROVIDER, POINTS, CACHE, &mut host)?,
                "select" => {
                    select(&mut m, PROVIDER, POINTS, CACHE, &mut host)?;
                    0
                }
                _ => unreachable!(),
            };
            let cache = (0..7)
                .map(|i| m.word(CACHE + i * 4))
                .collect::<Result<Vec<_>>>()?;
            let out = (0..7)
                .map(|i| m.word(OUTCACHE + i * 4))
                .collect::<Result<Vec<_>>>()?;
            let points = (0..5)
                .map(|i| vector(&m, POINTS + i * 12))
                .collect::<Result<Vec<_>>>()?;
            let actual = json!({"result":result,"trace":host.trace,"cache":cache,"out_cache":out,"count":count,"last":last,"filter":m.word(FILT)?,"points":points,"slot_count":m.word(PROVIDER+12)?});
            for key in actual.as_object().unwrap().keys() {
                assert_eq!(actual[key], expected[key], "{key}: {c}");
            }
        }
        Ok(())
    }
    #[test]
    fn invalid_cache_and_cell_storage_fail_without_inventing_clear_space() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_spatial_walk.json"
        ))?;
        let c = &data["cases"][0]["input"];
        let mut m = memory(c)?;
        m.write(CACHE + 4, 0)?;
        let mut host = Spy {
            case: c,
            trace: vec![],
            calls: 0,
        };
        assert!(select(&mut m, PROVIDER, POINTS, CACHE, &mut host).is_err());
        let mut m = memory(c)?;
        m.regions.remove(&CELLS);
        assert!(find(&mut m, PROVIDER, POINTS, CACHE, &mut host).is_err());
        let mut last = CELLS;
        assert_eq!(
            step(
                &mut m,
                CELLS,
                POINTS,
                POINTS + 12,
                0,
                Some(&mut last),
                &mut None,
                0,
                &mut host
            )?,
            1
        );
        host.calls = 0;
        assert!(step(
            &mut m,
            CELLS,
            POINTS,
            POINTS + 12,
            0,
            None,
            &mut None,
            0,
            &mut host
        )
        .is_err());
        Ok(())
    }
}
