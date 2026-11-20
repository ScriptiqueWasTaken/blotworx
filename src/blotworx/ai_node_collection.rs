//! Retail47cde0/47ce74 node collection, including indexed open bounds,
//! cross-resource seam deduplication, callback costs and native heap ordering.
//! All addresses are owned data references; filter keys select Rust dispatch.
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
#[derive(Clone, Copy)]
pub struct Context {
    pub scratch: u32,
    pub flags: u32,
    pub visited_total: u32,
    pub frame: u32,
    pub point_kind: u32,
    pub marker_kind: u32,
}
#[derive(Clone, Copy)]
pub struct Args {
    pub graph: u32,
    pub minimum: u32,
    pub maximum: u32,
    pub output: u32,
    pub capacity: u32,
    pub pointers: u32,
    pub sort: u32,
    pub filter: u32,
    pub filter_data: u32,
}
pub trait Host {
    fn read(&mut self, address: u32) -> Result<u32>;
    fn write(&mut self, address: u32, value: u32) -> Result<()>;
    fn byte(&mut self, address: u32) -> Result<u8> {
        let word = self.read(address & !3)?;
        Ok((word >> (24 - (address & 3) * 8)) as u8)
    }
    fn half(&mut self, address: u32) -> Result<u16> {
        ensure!(address & 1 == 0, "Unaligned graph index");
        let word = self.read(address & !3)?;
        Ok((word >> (16 - (address & 2) * 8)) as u16)
    }
    fn count(&mut self, resource: u32) -> Result<u32>;
    fn marker_index(&mut self) -> Result<u32>;
    fn filter(&mut self, key: u32, source: u32, target: u32, cost: u32, data: u32) -> Result<u32>;
    fn clock(&mut self) -> Result<u32>;
    fn coefficient(&mut self, clock: u32, index: u32) -> Result<f32>;
}
// Native47d57c/47d580 limits, not an editor/runtime heuristic.
const NATIVE_CAPACITY: u32 = 0x8000;
fn add(p: u32, o: u32) -> u32 {
    p.wrapping_add(o)
}
fn get(h: &mut impl Host, p: u32, o: u32) -> Result<u32> {
    h.read(add(p, o))
}
fn put(h: &mut impl Host, p: u32, o: u32, v: u32) -> Result<()> {
    h.write(add(p, o), v)
}
fn scalar(h: &mut impl Host, p: u32, o: u32) -> Result<f32> {
    Ok(f32::from_bits(get(h, p, o)?))
}
fn flag(h: &mut impl Host, c: Context, value: u32) -> Result<()> {
    let old = h.read(c.flags)?;
    h.write(c.flags, old | value)
}
fn point(h: &mut impl Host, resource: u32, index: u32) -> Result<u32> {
    let header = get(h, resource, 4)?;
    let stride = get(h, header, 0x7c)?;
    let points = get(h, resource, 0x2c)?;
    Ok(add(points, index.wrapping_mul(stride)))
}
fn sorted_point(
    h: &mut impl Host,
    resource: u32,
    rank: u32,
    offset: u32,
    axis: u32,
) -> Result<u32> {
    let p = point(h, resource, rank)?;
    let index = h.half(add(
        p,
        0x1c_u32
            .wrapping_add(offset)
            .wrapping_add(axis.wrapping_mul(2)),
    ))?;
    point(h, resource, u32::from(index))
}
fn coordinate(h: &mut impl Host, p: u32, axis: u32) -> Result<f32> {
    scalar(h, p, 4 + axis * 4)
}
fn sorted_coordinate(
    h: &mut impl Host,
    resource: u32,
    rank: u32,
    offset: u32,
    axis: u32,
) -> Result<f32> {
    let p = sorted_point(h, resource, rank, offset, axis)?;
    coordinate(h, p, axis)
}
struct Counts {
    accepted: u32,
    seams: u32,
    visited: u32,
}
fn resource(
    h: &mut impl Host,
    c: Context,
    a: Args,
    r: u32,
    offset: u32,
    s: &mut Counts,
) -> Result<bool> {
    let count = h.count(r)?;
    if count == 0 {
        return Ok(false);
    }
    let mut minimum = [0.; 3];
    let mut maximum = [0.; 3];
    for axis in 0..3 {
        minimum[axis] = scalar(h, a.minimum, axis as u32 * 4)?;
        maximum[axis] = scalar(h, a.maximum, axis as u32 * 4)?;
    }
    let last = count.wrapping_sub(1);
    // The retail broad phase uses ordered comparisons. The later per-point
    // comparisons reject unordered values through their inverse branch tests.
    if sorted_coordinate(h, r, 0, offset, 0)? >= maximum[0]
        || sorted_coordinate(h, r, 0, offset, 2)? >= maximum[2]
        || sorted_coordinate(h, r, last, offset, 0)? <= minimum[0]
        || sorted_coordinate(h, r, last, offset, 2)? <= minimum[2]
    {
        return Ok(false);
    }
    if sorted_coordinate(h, r, last, offset, 1)? <= minimum[1] {
        flag(h, c, 1)?;
        return Ok(false);
    }
    if sorted_coordinate(h, r, 0, offset, 1)? >= maximum[1] {
        flag(h, c, 2)?;
        return Ok(false);
    }
    let mut lower = [0; 3];
    let mut upper = [0; 3];
    for axis in 0..3 {
        let k = axis as u32;
        if minimum[axis] < sorted_coordinate(h, r, 0, offset, k)? {
            lower[axis] = 0;
        } else {
            let mut lo = 0u32;
            let mut hi = last;
            while hi.wrapping_sub(lo) > 1 {
                let mid = lo.wrapping_add(hi) >> 1;
                if sorted_coordinate(h, r, mid, offset, k)? > minimum[axis] {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            lower[axis] = hi;
        }
        if maximum[axis] > sorted_coordinate(h, r, last, offset, k)? {
            upper[axis] = last;
        } else {
            let mut lo = 0u32;
            let mut hi = last;
            while hi.wrapping_sub(lo) > 1 {
                let mid = lo.wrapping_add(hi) >> 1;
                if sorted_coordinate(h, r, mid, offset, k)? >= maximum[axis] {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            upper[axis] = lo;
        }
    }
    let sizes: [u32; 3] = std::array::from_fn(|i| upper[i].wrapping_add(1).wrapping_sub(lower[i]));
    let mut axis = 0usize;
    for i in 1..3 {
        if sizes[i] < sizes[axis] {
            axis = i;
        }
    }
    if sizes[axis] == 0 {
        return Ok(false);
    }
    // Only seams from earlier resources participate in this resource's duplicate
    // comparison. New seams are still appended, including duplicates in this list.
    let previous_seams = s.seams;
    for rank in 0..sizes[axis] {
        let p = sorted_point(h, r, lower[axis].wrapping_add(rank), offset, axis as u32)?;
        let mut xyz = [0.; 3];
        for i in 0..3 {
            xyz[i] = coordinate(h, p, i as u32)?;
        }
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        if !(xyz[first] > minimum[first]
            && xyz[first] < maximum[first]
            && xyz[second] > minimum[second]
            && xyz[second] < maximum[second])
        {
            continue;
        }
        if h.byte(add(p, 0x22))? != 0 {
            continue;
        }
        let id = h.read(p)?;
        let mut duplicate = false;
        if id & 0x80000000 != 0 {
            for i in 0..previous_seams {
                if h.read(add(c.scratch, i * 8))? == id & 0x7fffffff {
                    duplicate = true;
                    break;
                }
            }
            if s.seams < NATIVE_CAPACITY {
                put(h, c.scratch, s.seams * 8, id & 0x7fffffff)?;
                s.seams += 1;
            }
        }
        if duplicate {
            continue;
        }
        put(h, a.output, s.accepted.wrapping_mul(4), p)?;
        s.accepted = s.accepted.wrapping_add(1);
        if s.accepted >= a.capacity || s.accepted >= NATIVE_CAPACITY {
            break;
        }
    }
    Ok(s.accepted >= a.capacity || s.accepted >= NATIVE_CAPACITY)
}
fn compare(h: &mut impl Host, scratch: u32, left: u32, right: u32) -> Result<i32> {
    let a = scalar(h, scratch, left * 8 + 4)?;
    let b = scalar(h, scratch, right * 8 + 4)?;
    Ok(if a > b {
        1
    } else if a < b {
        -1
    } else {
        0
    })
}
fn swap(h: &mut impl Host, scratch: u32, left: u32, right: u32) -> Result<()> {
    for offset in [0, 4] {
        let a = add(scratch, left * 8 + offset);
        let b = add(scratch, right * 8 + offset);
        let av = h.read(a)?;
        let bv = h.read(b)?;
        h.write(a, bv)?;
        h.write(b, av)?;
    }
    Ok(())
}
/// DOL800177d0, fixed8-byte rows and actual47cdb8 comparator. Heap sorting is
/// deliberately retained: a stable host sort changes equal-cost node order.
fn sort(h: &mut impl Host, scratch: u32, count: u32) -> Result<()> {
    if count < 2 {
        return Ok(());
    }
    let mut left = count / 2 + 1;
    let mut right = count;
    loop {
        if left > 1 {
            left -= 1;
        } else {
            swap(h, scratch, right - 1, 0)?;
            right -= 1;
            if right == 1 {
                break;
            }
        }
        let mut current = left;
        while current * 2 <= right {
            let mut child = current * 2;
            if child < right && compare(h, scratch, child - 1, child)? < 0 {
                child += 1;
            }
            if compare(h, scratch, current - 1, child - 1)? >= 0 {
                break;
            }
            swap(h, scratch, current - 1, child - 1)?;
            current = child;
        }
    }
    Ok(())
}
fn make_point(h: &mut impl Host, c: Context, p: u32, xyz: [f32; 3]) -> Result<()> {
    h.write(p, c.point_kind)?;
    for (i, v) in xyz.into_iter().enumerate() {
        put(h, p, 4 + i as u32 * 4, v.to_bits())?;
    }
    for i in 0..16 {
        put(h, p, 16 + i * 4, 0)?;
    }
    Ok(())
}
fn collect(h: &mut impl Host, c: Context, a: Args, offset: u32) -> Result<u32> {
    h.write(c.flags, 0)?;
    let mut center = [0.; 3];
    for i in 0..3 {
        center[i] =
            (scalar(h, a.minimum, i as u32 * 4)? + scalar(h, a.maximum, i as u32 * 4)?) * 0.5;
    }
    let mut counts = Counts {
        accepted: 0,
        seams: 0,
        visited: 0,
    };
    let mut graph = a.graph;
    let mut seen = BTreeSet::new();
    while graph != 0 {
        ensure!(
            seen.insert(graph),
            "Cyclic graph chain during node collection"
        );
        let r = get(h, graph, 0x14)?;
        counts.visited = counts.visited.wrapping_add(1);
        if resource(h, c, a, r, offset, &mut counts)? {
            break;
        }
        graph = get(h, graph, 0x40)?;
    }
    if counts.accepted == 0 {
        flag(h, c, 4)?;
    } //47ffa0 is an actual literal blr.
    let mut rejected = 0u32;
    if a.sort & 255 == 1 {
        for i in 0..counts.accepted {
            let p = get(h, a.output, i * 4)?;
            put(h, c.scratch, i * 8, p)?;
            let cost = if a.filter == 0 {
                let dz = center[2] - coordinate(h, p, 2)?;
                let dx = center[0] - coordinate(h, p, 0)?;
                let dy = center[1] - coordinate(h, p, 1)?;
                ((dz * dz + dx * dx) + dy * dy).to_bits()
            } else {
                let cost = add(c.frame, 0x18);
                h.write(cost, 0f32.to_bits())?;
                let marker = add(c.frame, 0x20);
                h.write(marker, c.marker_kind)?;
                let id = h.read(p)?;
                put(h, marker, 4, id)?;
                let target = add(c.frame, 0xd0);
                let mut xyz = [0.; 3];
                for axis in 0..3 {
                    xyz[axis] = coordinate(h, p, axis as u32)?;
                }
                make_point(h, c, target, xyz)?;
                let index = h.marker_index()?;
                put(h, target, 16u32.wrapping_add(index.wrapping_mul(4)), marker)?;
                let source = add(c.frame, 0x80);
                make_point(h, c, source, center)?;
                if h.filter(a.filter, source, target, cost, a.filter_data)? & 255 != 0 {
                    h.read(cost)?
                } else {
                    rejected += 1;
                    f32::MAX.to_bits()
                }
            };
            put(h, c.scratch, i * 8 + 4, cost)?;
        }
        sort(h, c.scratch, counts.accepted)?;
        counts.accepted = counts.accepted.wrapping_sub(rejected);
        for i in 0..counts.accepted {
            let p = get(h, c.scratch, i * 8)?;
            put(h, a.output, i * 4, p)?;
        }
    }
    if a.pointers & 255 == 0 {
        for i in 0..counts.accepted {
            let p = get(h, a.output, i * 4)?;
            let id = h.read(p)? & 0x7fffffff;
            put(h, a.output, i * 4, id)?;
        }
    }
    let previous = h.read(c.visited_total)?;
    h.write(c.visited_total, previous.wrapping_add(counts.visited))?;
    let index = get(h, a.graph, 0x48)?;
    if (index as i32) >= 0 {
        let clock = h.clock()?;
        let coefficient = h.coefficient(clock, index)?;
        let amount = counts.visited as f32 * coefficient;
        let before = scalar(h, clock, 0x7c)?;
        put(h, clock, 0x7c, (before + amount).to_bits())?;
    }
    Ok(counts.accepted)
}
pub fn query(h: &mut impl Host, c: Context, a: Args) -> Result<u32> {
    for channel in 0..4 {
        let r = get(h, a.graph, 0x14)?;
        if r != 0 {
            let header = get(h, r, 4)?;
            if get(h, header, 0x1c + channel * 12)? == 2 {
                let offset = get(h, header, 0x20 + channel * 12)?;
                return collect(h, c, a, offset);
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use anyhow::Context as _;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const G: u32 = 0x60000000;
    const MIN: u32 = 0x61000000;
    const MAX: u32 = MIN + 0x10;
    const OUT: u32 = 0x62000000;
    const SCRATCH: u32 = 0x63000000;
    const CONFIG: u32 = 0x64000000;
    const RES: u32 = 0x65000000;
    const CLOCK: u32 = 0x67000000;
    pub(crate) struct Replay<'a> {
        pub(crate) input: &'a Value,
        pub(crate) bytes: BTreeMap<u32, u8>,
        pub(crate) trace: Vec<Value>,
    }
    impl Replay<'_> {
        fn region(&mut self, p: u32, n: u32) {
            for i in 0..n {
                self.bytes.insert(p + i, 0);
            }
        }
        fn write_half(&mut self, p: u32, value: u16) {
            for (i, v) in value.to_be_bytes().into_iter().enumerate() {
                self.bytes.insert(p + i as u32, v);
            }
        }
        fn point(&mut self, p: u32) -> Result<[u32; 3]> {
            Ok([self.read(p + 4)?, self.read(p + 8)?, self.read(p + 12)?])
        }
    }
    impl Host for Replay<'_> {
        fn read(&mut self, p: u32) -> Result<u32> {
            let mut b = [0; 4];
            for i in 0..4 {
                b[i] = *self
                    .bytes
                    .get(&(p + i as u32))
                    .with_context(|| format!("Unmapped collection read {p:08x}"))?;
            }
            Ok(u32::from_be_bytes(b))
        }
        fn write(&mut self, p: u32, v: u32) -> Result<()> {
            for (i, v) in v.to_be_bytes().into_iter().enumerate() {
                *self
                    .bytes
                    .get_mut(&(p + i as u32))
                    .with_context(|| format!("Unmapped collection write {p:08x}"))? = v;
            }
            Ok(())
        }
        fn count(&mut self, r: u32) -> Result<u32> {
            self.trace.push(json!(["count", r]));
            let header = self.read(r + 0x28)?;
            self.read(header)
        }
        fn marker_index(&mut self) -> Result<u32> {
            Ok(7)
        }
        fn filter(
            &mut self,
            key: u32,
            source: u32,
            target: u32,
            cost: u32,
            data: u32,
        ) -> Result<u32> {
            assert_eq!(key, 1);
            let marker = self.read(target + 16 + 7 * 4)?;
            let id = self.read(marker + 4)?;
            let source = self.point(source)?;
            let target = self.point(target)?;
            self.trace.push(json!(["filter", source, target, id, data]));
            let entry = self.input["filter"].get(id.to_string());
            let value = entry.map_or(1, |e| e[0].as_u64().unwrap() as u32);
            let bits = entry.map_or_else(
                || ((id & 0x7fffffff) as f32).to_bits(),
                |e| e[1].as_u64().unwrap() as u32,
            );
            self.write(cost, bits)?;
            if self.input["mutate"] == "flags" {
                let flags = self.read(0x62e60)?;
                self.write(0x62e60, flags | 0x100)?;
            }
            Ok(value)
        }
        fn clock(&mut self) -> Result<u32> {
            self.trace.push(json!(["clock"]));
            Ok(CLOCK)
        }
        fn coefficient(&mut self, clock: u32, index: u32) -> Result<f32> {
            self.trace.push(json!(["coefficient", clock, index]));
            Ok(f32::from_bits(
                self.input["coefficient"].as_u64().unwrap() as u32
            ))
        }
    }
    pub(crate) fn replay(input: &Value) -> Result<(Replay<'_>, u32)> {
        let rows = input["resources"].as_array().unwrap();
        let count: usize = rows.iter().map(|r| r.as_array().unwrap().len()).sum();
        let size = count.saturating_add(2).max(32) as u32;
        let mut h = Replay {
            input,
            bytes: BTreeMap::new(),
            trace: vec![],
        };
        for (p, n) in [
            (G, (rows.len() as u32 * 0x100).max(0x100)),
            (MIN, 0x20),
            (OUT, size * 4),
            (SCRATCH, size * 8),
            (CONFIG, 0x100),
            (CLOCK, 0x80),
            (0x61e50, 4),
            (0x62e60, 4),
            (0x70000000, 0x1a0),
        ] {
            h.region(p, n);
        }
        h.write(0x61e50, 7)?;
        h.write(0x62e60, 0xdead)?;
        h.write(CLOCK + 0x7c, 0.5f32.to_bits())?;
        h.write(G + 0x48, input["profile"].as_i64().unwrap() as u32)?;
        h.write(CONFIG + 0x7c, 0x40)?;
        let offsets: Vec<u32> = input["offsets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        for i in 0..4 {
            h.write(
                CONFIG + 0x1c + i * 12,
                input["modes"][i as usize].as_u64().unwrap() as u32,
            )?;
            h.write(CONFIG + 0x20 + i * 12, offsets[i as usize])?;
        }
        for i in 0..3 {
            h.write(
                MIN + i * 4,
                input["minimum"][i as usize].as_u64().unwrap() as u32,
            )?;
            h.write(
                MAX + i * 4,
                input["maximum"][i as usize].as_u64().unwrap() as u32,
            )?;
        }
        for (i, row) in rows.iter().enumerate() {
            let nodes = row.as_array().unwrap();
            let r = RES + i as u32 * 0x10000;
            let points = r + 0x1000;
            let header = r + 0x800;
            h.region(r, 0x40);
            h.region(header, 4);
            h.region(points, nodes.len().max(1) as u32 * 0x40);
            h.write(G + i as u32 * 0x100 + 0x14, r)?;
            h.write(
                G + i as u32 * 0x100 + 0x40,
                if i + 1 < rows.len() {
                    G + (i + 1) as u32 * 0x100
                } else {
                    0
                },
            )?;
            h.write(r, 0x51ea08)?;
            h.write(r + 4, CONFIG)?;
            h.write(r + 0x28, header)?;
            h.write(header, nodes.len() as u32)?;
            h.write(r + 0x2c, points)?;
            let indices: [Vec<usize>; 3] = std::array::from_fn(|axis| {
                let mut order: Vec<_> = (0..nodes.len()).collect();
                order.sort_by(|&a, &b| {
                    f32::from_bits(nodes[a]["xyz"][axis].as_u64().unwrap() as u32)
                        .partial_cmp(&f32::from_bits(
                            nodes[b]["xyz"][axis].as_u64().unwrap() as u32
                        ))
                        .unwrap()
                });
                order
            });
            for (j, node) in nodes.iter().enumerate() {
                let p = points + j as u32 * 0x40;
                h.write(p, node["id"].as_u64().unwrap() as u32)?;
                for axis in 0..3 {
                    h.write(
                        p + 4 + axis * 4,
                        node["xyz"][axis as usize].as_u64().unwrap() as u32,
                    )?;
                }
                for &offset in &offsets {
                    for axis in 0..3 {
                        h.write_half(p + 0x1c + offset + axis as u32 * 2, indices[axis][j] as u16);
                    }
                }
                h.bytes
                    .insert(p + 0x22, node["disabled"].as_u64().unwrap() as u8);
            }
        }
        Ok((h, size))
    }
    #[test]
    fn original_node_collection_bounds_seams_costs_sort_and_budget_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_node_collection.json"
        ))?;
        for (case_index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let (mut h, size) = replay(input)?;
            let result = query(
                &mut h,
                Context {
                    scratch: SCRATCH,
                    flags: 0x62e60,
                    visited_total: 0x61e50,
                    frame: 0x70000000,
                    point_kind: 0x4ec1b0,
                    marker_kind: 0x4ebe80,
                },
                Args {
                    graph: G,
                    minimum: MIN,
                    maximum: MAX,
                    output: OUT,
                    capacity: input["capacity"].as_u64().unwrap() as u32,
                    pointers: input["pointers"].as_u64().unwrap() as u32,
                    sort: input["sort"].as_u64().unwrap() as u32,
                    filter: if input["filter"].is_null() { 0 } else { 1 },
                    filter_data: 0x68000000,
                },
            )?;
            assert_eq!(json!(result), case["result"], "count {case_index}");
            let output: Vec<_> = (0..result).map(|i| h.read(OUT + i * 4).unwrap()).collect();
            assert_eq!(json!(output), case["output"], "output {case_index}");
            let scratch: Vec<_> = (0..size * 2)
                .map(|i| h.read(SCRATCH + i * 4).unwrap())
                .collect();
            assert_eq!(json!(scratch), case["scratch"], "scratch {case_index}");
            assert_eq!(json!(h.read(0x62e60)?), case["flags"], "flags {case_index}");
            assert_eq!(
                json!(h.read(0x61e50)?),
                case["visited"],
                "visited {case_index}"
            );
            assert_eq!(
                json!(h.read(CLOCK + 0x7c)?),
                case["clock"],
                "clock {case_index}"
            );
            assert_eq!(json!(h.trace), case["trace"], "trace {case_index}");
        }
        Ok(())
    }
}
