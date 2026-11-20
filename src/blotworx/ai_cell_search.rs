//! Native grid candidates42f98c, floor boundary42fd48/42fed8 and search42f8a8.
//! Runtime data references are checked; no replacement polygons are invented.
use super::ai_graph_link_passes::Memory;
use anyhow::{ensure, Result};
fn word(m: &Memory, b: u32, o: u32) -> Result<u32> {
    m.word(b.wrapping_add(o))
}
fn scalar(m: &Memory, b: u32, o: u32) -> Result<f32> {
    Ok(f32::from_bits(word(m, b, o)?))
}
#[derive(Clone, Copy)]
enum Point {
    Address(u32),
    Position([f32; 3]),
}
impl Point {
    fn scalar(self, m: &Memory, offset: u32) -> Result<f32> {
        match self {
            Self::Address(p) => scalar(m, p, offset),
            Self::Position(p) => Ok(p[(offset / 4) as usize]),
        }
    }
}
fn sel(test: f32, positive: f32, negative: f32) -> f32 {
    if test >= 0. {
        positive
    } else {
        negative
    }
}
fn integer(value: f32) -> Result<i32> {
    ensure!(
        value.is_finite() && (-2147483648.0..2147483648.0).contains(&(value as f64)),
        "Unverified navigation grid fctiwz conversion for {value}"
    );
    Ok(value as i32)
}
pub fn grid(m: &Memory, sector: u32, point: u32, output: &mut [[i32; 2]; 4]) -> Result<u32> {
    grid_point(m, sector, Point::Address(point), output)
}
fn grid_point(m: &Memory, sector: u32, point: Point, output: &mut [[i32; 2]; 4]) -> Result<u32> {
    let x = point.scalar(m, 0)?;
    let margin = scalar(m, sector, 0x10)?;
    if x < scalar(m, sector, 0x14)? - margin {
        return Ok(0);
    }
    if x >= scalar(m, sector, 0x18)? + margin {
        return Ok(0);
    }
    let z = point.scalar(m, 8)?;
    if z < scalar(m, sector, 0x1c)? - margin {
        return Ok(0);
    }
    if z >= scalar(m, sector, 0x20)? + margin {
        return Ok(0);
    }
    let inverse = scalar(m, sector, 12)?;
    let row = inverse * (point.scalar(m, 0)? - scalar(m, sector, 0x14)?);
    let col = inverse * (point.scalar(m, 8)? - scalar(m, sector, 0x1c)?);
    let radius = margin * inverse;
    let row_max = (word(m, sector, 0x24)? as i32).wrapping_sub(1);
    let col_max = (word(m, sector, 0x28)? as i32).wrapping_sub(1);
    let base_row = integer(row)?.max(0).min(row_max);
    let base_col = integer(col)?.max(0).min(col_max);
    let row_low = integer(row - radius)?.max(0);
    let row_high = integer(row + radius)?.min(row_max);
    let col_low = integer(col - radius)?.max(0);
    let col_high = integer(col + radius)?.min(col_max);
    output[0] = [base_row, base_col];
    let mut count = 1;
    let mut r = row_low;
    while r <= row_high {
        let mut c = col_low;
        let mut remaining = (col_high as u32)
            .wrapping_add(1)
            .wrapping_sub(col_low as u32);
        if col_low <= col_high {
            while remaining != 0 {
                if r != base_row || c != base_col {
                    output[count as usize] = [r, c];
                    count += 1;
                    // Native42fb88 stops at four, even for large margins.
                    if count == 4 {
                        return Ok(count);
                    }
                }
                c = c.wrapping_add(1);
                remaining = remaining.wrapping_sub(1);
            }
        }
        r = r.wrapping_add(1);
    }
    Ok(count)
}
fn interpolate(m: &Memory, p: Point, a: u32, b: u32, offset: u32, epsilon: f32) -> Result<u32> {
    let start = scalar(m, a, offset)?;
    let delta = p.scalar(m, offset)? - start;
    let ay = scalar(m, a, 4)?;
    let dy = scalar(m, b, 4)? - ay;
    let numerator = delta * dy;
    let denominator = scalar(m, b, offset)? - start;
    let projected = ay + numerator / denominator;
    Ok(u32::from((projected - p.scalar(m, 4)?).abs() < epsilon))
}
pub fn edge(m: &Memory, p: u32, a: u32, b: u32, epsilon: f32, parity: &mut u8) -> Result<u32> {
    edge_point(m, Point::Address(p), a, b, epsilon, parity)
}
fn edge_point(m: &Memory, p: Point, a: u32, b: u32, epsilon: f32, parity: &mut u8) -> Result<u32> {
    let az = scalar(m, a, 8)?;
    let bz = scalar(m, b, 8)?;
    let min_z = sel(az - bz, bz, az);
    if az != bz {
        let ax = scalar(m, a, 0)?;
        let bx = scalar(m, b, 0)?;
        if ax == bx {
            if p.scalar(m, 0)? == ax {
                return interpolate(m, p, a, b, 8, epsilon);
            }
            if p.scalar(m, 8)? > min_z {
                *parity = u8::from(*parity == 0);
            }
            return Ok(0);
        }
        let dz = p.scalar(m, 8)? - az;
        let dx = bx - ax;
        let numerator = dz * dx;
        let denominator = bz - az;
        let hit_x = ax + numerator / denominator;
        let px = p.scalar(m, 0)?;
        if !(px <= hit_x) {
            return Ok(0);
        }
        if px == hit_x {
            return interpolate(m, p, a, b, 8, epsilon);
        }
        if p.scalar(m, 8)? > min_z {
            *parity = u8::from(*parity == 0);
        }
        return Ok(0);
    }
    let ax = scalar(m, a, 0)?;
    let bx = scalar(m, b, 0)?;
    let min_x = sel(ax - bx, bx, ax);
    if !(p.scalar(m, 0)? >= min_x) {
        return Ok(0);
    }
    interpolate(m, p, a, b, 0, epsilon)
}
fn segment(
    m: &Memory,
    p: Point,
    px: f32,
    pz: f32,
    a: u32,
    b: u32,
    epsilon: f32,
    parity: &mut u8,
) -> Result<u32> {
    let az = scalar(m, a, 8)?;
    let bz = scalar(m, b, 8)?;
    let dz = az - bz;
    let min_z = sel(dz, bz, az);
    let max_z = sel(dz, az, bz);
    let ax = scalar(m, a, 0)?;
    let bx = scalar(m, b, 0)?;
    let max_x = sel(ax - bx, ax, bx);
    let one = f32::from_bits(0x3f8147ae);
    let upper = sel(max_z - pz, one, 0.);
    let lower = sel(pz - min_z, one, 0.);
    let right = sel(max_x - px, one, 0.);
    let sum = right + (lower + upper);
    if sum >= 3. {
        edge_point(m, p, a, b, epsilon, parity)
    } else {
        Ok(0)
    }
}
pub fn contains(m: &Memory, floor: u32, p: u32, epsilon: f32) -> Result<u32> {
    contains_point(m, floor, Point::Address(p), epsilon)
}
pub fn contains_position(m: &Memory, floor: u32, p: [f32; 3], epsilon: f32) -> Result<u32> {
    contains_point(m, floor, Point::Position(p), epsilon)
}
fn contains_point(m: &Memory, floor: u32, p: Point, epsilon: f32) -> Result<u32> {
    let px = p.scalar(m, 0)?;
    let pz = p.scalar(m, 8)?;
    let mut parity = 0u8;
    if word(m, floor, 0x40)? != 0 {
        let mut offsets = floor.wrapping_add(word(m, floor, 0x44)?);
        let mut current = floor.wrapping_add(m.word(offsets)?);
        let mut index = 0;
        while index < word(m, floor, 0x40)? {
            let end = floor.wrapping_add(word(m, offsets, 4)?);
            let mut next = current.wrapping_add(12);
            while next != end {
                if segment(m, p, px, pz, current, next, epsilon, &mut parity)? != 0 {
                    return Ok(1);
                }
                current = current.wrapping_add(12);
                next = next.wrapping_add(12);
            }
            current = end;
            offsets = offsets.wrapping_add(4);
            index += 1;
        }
    }
    for (count_offset, data_offset, stride) in [(0x28, 0x2c, 0x20u32), (0x30, 0x34, 0x2c)] {
        let count = word(m, floor, count_offset)?;
        if count == 0 {
            continue;
        }
        let mut at = floor.wrapping_add(word(m, floor, data_offset)?);
        for _ in 0..count {
            if segment(m, p, px, pz, at, at.wrapping_add(12), epsilon, &mut parity)? != 0 {
                return Ok(1);
            }
            at = at.wrapping_add(stride);
        }
    }
    Ok(parity as u32)
}
pub fn find_cell(m: &Memory, sector: u32, point: u32) -> Result<u32> {
    find_cell_point(m, sector, Point::Address(point))
}
pub fn find_cell_position(m: &Memory, sector: u32, point: [f32; 3]) -> Result<u32> {
    find_cell_point(m, sector, Point::Position(point))
}
fn find_cell_point(m: &Memory, sector: u32, point: Point) -> Result<u32> {
    let mut candidates = [[0; 2]; 4];
    let count = grid_point(m, sector, point, &mut candidates)?;
    for i in 0..count as usize {
        let table = word(m, sector, 0x30)?;
        let width = word(m, sector, 0x28)?;
        let offset = (candidates[i][0] as u32)
            .wrapping_mul(width)
            .wrapping_add(candidates[i][1] as u32)
            .wrapping_mul(4);
        let list = m.word(table.wrapping_add(offset))?;
        let mut at = list.wrapping_add(4);
        let mut index = 0;
        while index < word(m, list, 0)? {
            let y = point.scalar(m, 4)?;
            if !(y < scalar(m, at, 4)?) && !(y > scalar(m, at, 8)?) {
                let floor = word(m, at, 0)?;
                if contains_point(m, floor, point, scalar(m, floor, 0x14)?)? as u8 != 0 {
                    return Ok(floor);
                }
            }
            index += 1;
            at = at.wrapping_add(12);
        }
    }
    Ok(0)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::ai_spatial_walk as walk;
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const PROVIDER: u32 = 0x60000000;
    const SLOTS: u32 = 0x61000000;
    const SECTOR: u32 = 0x62000000;
    const GRID: u32 = 0x63000000;
    const LISTS: u32 = 0x64000000;
    const FLOORS: u32 = 0x65000000;
    const POINT: u32 = 0x66000000;
    const A: u32 = POINT + 0x100;
    const B: u32 = POINT + 0x200;
    const CACHE: u32 = 0x67000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn add(m: &mut Memory, p: u32, n: usize) {
        m.regions.insert(p, vec![0; n]);
    }
    fn put_vector(m: &mut Memory, p: u32, v: &Value) -> Result<()> {
        for i in 0..3 {
            m.write(p + i * 4, bits(&v[i as usize]))?;
        }
        Ok(())
    }
    fn prepare_floor(m: &mut Memory, p: u32, f: &Value, layer: u32) -> Result<()> {
        add(m, p, 0x3000);
        m.write(p, 501)?;
        m.write(p + 12, layer)?;
        m.write(p + 0x14, bits(&f["epsilon"]))?;
        m.write(p + 0x18, bits(&f["range"][1]))?;
        m.write(p + 0x1c, bits(&f["range"][0]))?;
        let paths = f["paths"].as_array().unwrap();
        m.write(p + 0x40, paths.len() as u32)?;
        m.write(p + 0x44, 0x50)?;
        let mut cursor = 0x100;
        for (i, path) in paths.iter().enumerate() {
            m.write(p + 0x50 + i as u32 * 4, cursor)?;
            for v in path.as_array().unwrap() {
                put_vector(m, p + cursor, v)?;
                cursor += 12;
            }
        }
        m.write(p + 0x50 + paths.len() as u32 * 4, cursor)?;
        for (name, count_off, data_off, start, stride) in [
            ("segments", 0x28, 0x2c, 0x1000, 0x20),
            ("links", 0x30, 0x34, 0x2000, 0x2c),
        ] {
            let pairs = f[name].as_array().unwrap();
            m.write(p + count_off, pairs.len() as u32)?;
            m.write(p + data_off, start)?;
            for (i, pair) in pairs.iter().enumerate() {
                for k in 0..2 {
                    put_vector(m, p + start + i as u32 * stride + k * 12, &pair[k as usize])?;
                }
            }
        }
        Ok(())
    }
    pub(in crate::blotworx) fn memory(c: &Value) -> Result<Memory> {
        let mut m = Memory {
            regions: BTreeMap::new(),
            resources: BTreeMap::new(),
            stride: 32,
            journal: BTreeMap::new(),
        };
        add(&mut m, POINT, 12);
        put_vector(&mut m, POINT, &c["point"])?;
        if c["op"] == "edge" {
            add(&mut m, A, 12);
            add(&mut m, B, 12);
            put_vector(&mut m, A, &c["a"])?;
            put_vector(&mut m, B, &c["b"])?;
            return Ok(m);
        }
        if c["op"] == "contains" {
            prepare_floor(&mut m, FLOORS, &c["floor"], 0)?;
            return Ok(m);
        }
        let s = &c["sector"];
        add(&mut m, SECTOR, 0x34);
        for (off, v) in [
            (8, 501),
            (12, bits(&s["inverse"])),
            (16, bits(&s["margin"])),
            (20, bits(&s["min_x"])),
            (24, bits(&s["max_x"])),
            (28, bits(&s["min_z"])),
            (32, bits(&s["max_z"])),
            (36, bits(&s["rows"])),
            (40, bits(&s["cols"])),
            (48, GRID),
        ] {
            m.write(SECTOR + off, v)?;
        }
        if c["op"] == "grid" {
            return Ok(m);
        }
        let floors = c["floors"].as_array().unwrap();
        for (i, f) in floors.iter().enumerate() {
            prepare_floor(&mut m, FLOORS + i as u32 * 0x10000, f, i as u32)?;
        }
        let cells = c["cells"].as_array().unwrap();
        add(&mut m, GRID, cells.len() * 4);
        for (i, indices) in cells.iter().enumerate() {
            let indices = indices.as_array().unwrap();
            let ptr = LISTS + i as u32 * 0x100;
            add(&mut m, ptr, 4 + indices.len() * 12);
            m.write(GRID + i as u32 * 4, ptr)?;
            m.write(ptr, indices.len() as u32)?;
            for (j, index) in indices.iter().enumerate() {
                let index = bits(index);
                let f = &floors[index as usize];
                m.write(ptr + 4 + j as u32 * 12, FLOORS + index * 0x10000)?;
                m.write(ptr + 8 + j as u32 * 12, bits(&f["range"][0]))?;
                m.write(ptr + 12 + j as u32 * 12, bits(&f["range"][1]))?;
            }
        }
        add(&mut m, PROVIDER, 16);
        add(&mut m, SLOTS, 8);
        add(&mut m, CACHE, 28);
        m.write(PROVIDER + 8, SLOTS)?;
        m.write(PROVIDER + 12, 1)?;
        m.write_byte(SLOTS, 1)?;
        m.write(SLOTS + 4, SECTOR)?;
        for (i, v) in [u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0, u32::MAX, 0]
            .into_iter()
            .enumerate()
        {
            m.write(CACHE + i as u32 * 4, v)?;
        }
        Ok(m)
    }
    struct NativeSearch;
    impl walk::Host for NativeSearch {
        // The default finder must execute the complete native cell search.
        fn trace42e46c(
            &mut self,
            _: &mut Memory,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: &mut Option<walk::Filter>,
            _: u32,
        ) -> Result<u32> {
            anyhow::bail!("Cell search unexpectedly attempted a segment trace")
        }
    }
    #[test]
    fn original_cell_grid_boundaries_ray_parity_floor_search_and_cache_composition_match(
    ) -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_cell_search.json"
        ))?;
        for expected in data["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut m = memory(c)?;
            let mut parity = c["parity"].as_u64().unwrap_or(0) as u8;
            let mut candidates = [[77; 2]; 4];
            let mut native = NativeSearch;
            let result = match c["op"].as_str().unwrap() {
                "edge" => edge(
                    &m,
                    POINT,
                    A,
                    B,
                    f32::from_bits(bits(&c["epsilon"])),
                    &mut parity,
                )?,
                "grid" => grid(&m, SECTOR, POINT, &mut candidates)?,
                "contains" => contains(&m, FLOORS, POINT, f32::from_bits(bits(&c["epsilon"])))?,
                "find" => walk::Host::find_cell42f8a8(&mut native, &mut m, SECTOR, POINT)?,
                "select" => {
                    walk::select(&mut m, PROVIDER, POINT, CACHE, &mut native)?;
                    0
                }
                _ => unreachable!(),
            };
            let candidate_bits = candidates.map(|p| p.map(|v| v as u32));
            let cache = if c["op"] == "select" {
                Some(
                    (0..7)
                        .map(|i| m.word(CACHE + i * 4))
                        .collect::<Result<Vec<_>>>()?,
                )
            } else {
                None
            };
            let actual = json!({"result":result,"parity":if c["op"]=="edge"{Some(parity)}else{None},"candidates":if c["op"]=="grid"{Some(candidate_bits)}else{None},"cache":cache});
            for key in actual.as_object().unwrap().keys() {
                assert_eq!(actual[key], expected[key], "{key}: {c}");
            }
        }
        Ok(())
    }
    #[test]
    fn malformed_search_data_and_unverified_grid_conversions_are_explicit_errors() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_cell_search.json"
        ))?;
        let c = &data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| {
                v["input"]["op"] == "find"
                    && v["input"]["point"] == json!([1f32.to_bits(), 0, 1f32.to_bits()])
            })
            .unwrap()["input"];
        let mut m = memory(c)?;
        assert_eq!(
            find_cell(&m, SECTOR, POINT)?,
            FLOORS,
            "Corruption checks need a verified in-sector baseline"
        );
        m.regions.remove(&GRID);
        assert!(find_cell(&m, SECTOR, POINT).is_err());
        let mut m = memory(c)?;
        m.write(POINT, 0x7fc12345)?;
        assert!(grid(&m, SECTOR, POINT, &mut [[77; 2]; 4]).is_err());
        let mut m = memory(c)?;
        m.write(POINT, 4f32.to_bits())?;
        m.write(POINT + 8, 4f32.to_bits())?;
        m.write(FLOORS + 0x50, 0x100)?;
        m.write(FLOORS + 0x54, 0x100)?;
        assert!(contains(&m, FLOORS, POINT, 0.1).is_err());
        Ok(())
    }
    #[test]
    fn typed_query_positions_and_borrowed_cache_match_original_without_mapping_points() -> Result<()>
    {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_cell_search.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            let mut m = memory(c)?;
            let point = std::array::from_fn(|i| f32::from_bits(bits(&c["point"][i])));
            m.regions.remove(&POINT);
            let mut parity = c["parity"].as_u64().unwrap_or(0) as u8;
            let mut candidates = [[77; 2]; 4];
            let mut cache = [u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0, u32::MAX, 0];
            let result = match c["op"].as_str().unwrap() {
                "edge" => edge_point(
                    &m,
                    Point::Position(point),
                    A,
                    B,
                    f32::from_bits(bits(&c["epsilon"])),
                    &mut parity,
                )?,
                "grid" => grid_point(&m, SECTOR, Point::Position(point), &mut candidates)?,
                "contains" => {
                    contains_position(&m, FLOORS, point, f32::from_bits(bits(&c["epsilon"])))?
                }
                "find" => find_cell_position(&m, SECTOR, point)?,
                "select" => {
                    m.regions.remove(&CACHE);
                    walk::find_position(&mut m, PROVIDER, point, &mut cache)?;
                    0
                }
                _ => unreachable!(),
            };
            assert_eq!(json!(result), case["result"], "typed result {index}");
            if c["op"] == "edge" {
                assert_eq!(json!(parity), case["parity"], "typed edge {index}");
            }
            if c["op"] == "grid" {
                assert_eq!(
                    json!(candidates.map(|p| p.map(|v| v as u32))),
                    case["candidates"],
                    "typed grid {index}"
                );
            }
            if c["op"] == "select" {
                assert_eq!(json!(cache), case["cache"], "borrowed cache {index}");
            }
        }
        Ok(())
    }
}
