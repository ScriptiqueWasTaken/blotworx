//!47fea8 adjacency selection,47f314/47f50c,47fde8 owner ranges,47db50
//! linked-point filtering and47e238 traversal policy lookup. Pure native port.
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
pub trait Host {
    fn read(&mut self, p: u32) -> Result<u32>;
    fn write(&mut self, p: u32, v: u32) -> Result<()>;
    fn byte(&mut self, p: u32) -> Result<u8> {
        let base = p & !3;
        Ok((self.read(base)? >> (24 - (p & 3) * 8)) as u8)
    }
    fn handle(&mut self, h: u32, offset: u32) -> Result<u32> {
        self.read(h.wrapping_add(offset))
    }
    fn set_handle(&mut self, h: u32, offset: u32, value: u32) -> Result<()> {
        self.write(h.wrapping_add(offset), value)
    }
    fn kind(&mut self, resource: u32) -> Result<u32>;
    fn resource_state(&mut self, r: u32) -> Result<u32> {
        self.read(r.wrapping_add(0x10))
    }
    fn resource_range(&mut self, r: u32) -> Result<(u32, u32)> {
        Ok((
            self.read(r.wrapping_add(0x14))?,
            self.read(r.wrapping_add(0x18))?,
        ))
    }
    fn points(&mut self, r: u32) -> Result<u32> {
        self.read(r.wrapping_add(0x2c))
    }
    fn edges(&mut self, r: u32) -> Result<u32> {
        self.read(r.wrapping_add(0x30))
    }
    fn point_stride(&mut self, r: u32) -> Result<u32> {
        let cfg = self.read(r.wrapping_add(4))?;
        self.read(cfg.wrapping_add(0x7c))
    }
    fn edge_stride(&mut self, r: u32) -> Result<u32> {
        let cfg = self.read(r.wrapping_add(4))?;
        self.read(cfg.wrapping_add(0x80))
    }
    fn edge_count(&mut self, r: u32) -> Result<u32> {
        let header = self.read(r.wrapping_add(0x28))?;
        self.read(header.wrapping_add(4))
    }
}
#[derive(Clone, Copy)]
pub struct Context {
    pub count: u32,
    pub ids: u32,
}
fn word(h: &mut impl Host, p: u32, o: u32) -> Result<u32> {
    h.read(p.wrapping_add(o))
}
/// Owner lookup includes the end address, matching the original unsigned range.
pub fn owner(h: &mut impl Host, mut root: u32, p: u32) -> Result<u32> {
    let mut seen = BTreeSet::new();
    loop {
        ensure!(seen.insert(root), "Cycle in graph parent handles");
        let parent = h.handle(root, 0x3c)?;
        if parent == 0 {
            break;
        }
        root = parent;
    }
    let mut node = root;
    let mut seen = BTreeSet::new();
    while node != 0 {
        ensure!(seen.insert(node), "Cycle in graph owner handles");
        let r = h.handle(node, 0x14)?;
        if h.resource_state(r)? == 5 {
            let (base, size) = h.resource_range(r)?;
            if p >= base && p <= base.wrapping_add(size) {
                return Ok(node);
            }
        }
        node = h.handle(node, 0x40)?;
    }
    Ok(0)
}
fn blocked(h: &mut impl Host, graph: u32, p: u32) -> Result<bool> {
    if h.byte(p.wrapping_add(0x22))? != 1 {
        return Ok(false);
    }
    let mut next = word(h, p, 0x14)?;
    let mut has_owner = false;
    let found = owner(h, graph, next)?;
    if found != 0 {
        has_owner = h.handle(found, 0x14)? != 0;
    }
    let mut seen = BTreeSet::new();
    while next != 0 && next != p {
        ensure!(seen.insert(next), "Cycle in linked graph-point filter");
        if has_owner && h.byte(next.wrapping_add(0x22))? == 0 {
            return Ok(false);
        }
        next = word(h, next, 0x14)?;
        if next != 0 && next != p {
            let found = owner(h, graph, next)?;
            if found != 0 {
                has_owner = h.handle(found, 0x14)? != 0;
            }
        }
    }
    Ok(true)
}
fn search(
    h: &mut impl Host,
    c: Context,
    graph: u32,
    mut p: u32,
    mode: u32,
    target: u32,
    by_index: u32,
    dynamic: bool,
) -> Result<u32> {
    let mut first = 0;
    let mut seen = BTreeSet::new();
    while p != 0 && p != first {
        ensure!(seen.insert(p), "Cycle in graph-point adjacency chain");
        let found = owner(h, graph, p)?;
        if found != 0 {
            let resource = h.handle(found, 0x14)?;
            let start = word(h, p, if dynamic { 0x18 } else { 0x1c })?;
            if start != 0 {
                let mut index = start - 1;
                let stride = if dynamic {
                    h.edge_stride(resource)?
                } else {
                    16
                };
                let edges = h.edges(resource)?;
                let mut edge = edges.wrapping_add(stride.wrapping_mul(index));
                loop {
                    let point_stride = if dynamic {
                        h.point_stride(resource)?
                    } else {
                        32
                    };
                    let points = h.points(resource)?;
                    let source = points.wrapping_add(point_stride.wrapping_mul(word(h, edge, 4)?));
                    if source != p {
                        break;
                    }
                    let points = h.points(resource)?;
                    let destination =
                        points.wrapping_add(point_stride.wrapping_mul(word(h, edge, 8)?));
                    let id = h.read(destination)? & 0x7fffffff;
                    let mut allow = if dynamic {
                        !blocked(h, graph, destination)?
                    } else {
                        true
                    };
                    let count = h.read(c.count)?;
                    let mut i = 0;
                    while i < count && allow {
                        if h.read(c.ids.wrapping_add(i * 4))? == id {
                            allow = false;
                        }
                        i += 1;
                    }
                    if allow {
                        if mode == 0 {
                            let selected = if by_index != 0 {
                                count == target
                            } else {
                                h.read(target)? & 0x7fffffff == id
                            };
                            if selected {
                                h.set_handle(graph, 0, edge)?;
                                h.set_handle(graph, 0xc, resource)?;
                                return Ok(0);
                            }
                        }
                        if mode != 0 || by_index != 0 {
                            let count = h.read(c.count)?;
                            h.write(c.ids.wrapping_add(count.wrapping_mul(4)), id)?;
                            h.write(c.count, count.wrapping_add(1))?;
                        }
                    }
                    index = index.wrapping_add(1);
                    if index >= h.edge_count(resource)? {
                        break;
                    }
                    let stride = if dynamic {
                        h.edge_stride(resource)?
                    } else {
                        16
                    };
                    edge = edge.wrapping_add(stride);
                }
            }
        }
        if first == 0 {
            first = p;
        }
        p = word(h, p, if dynamic { 0x14 } else { 0x18 })?;
    }
    Ok(if mode != 0 {
        h.read(c.count)?
    } else {
        u32::MAX
    })
}
pub fn connection_index(
    h: &mut impl Host,
    c: Context,
    graph: u32,
    mode: u32,
    target: u32,
    by_index: u32,
) -> Result<u32> {
    let p = h.handle(graph, 4)?;
    if p == 0 {
        return Ok(u32::MAX);
    }
    let mut resource = h.handle(graph, 8)?;
    if resource == 0 {
        let found = owner(h, graph, p)?;
        if found == 0 {
            return Ok(u32::MAX);
        }
        resource = h.handle(found, 0x14)?;
        h.set_handle(graph, 8, resource)?;
        if resource == 0 {
            return Ok(u32::MAX);
        }
    }
    h.write(c.count, 0)?;
    let dynamic = h.kind(resource)? == 1;
    let p = h.handle(graph, 4)?;
    search(h, c, graph, p, mode, target, by_index, dynamic)
}
pub fn traversal(h: &mut impl Host, graph: u32) -> Result<Option<u32>> {
    let edge = h.handle(graph, 0)?;
    if edge == 0 {
        return Ok(None);
    }
    let mut resource = h.handle(graph, 0xc)?;
    if resource == 0 {
        let found = owner(h, graph, edge)?;
        if found == 0 {
            return Ok(None);
        }
        resource = h.handle(found, 0x14)?;
        h.set_handle(graph, 0xc, resource)?;
        if resource == 0 {
            return Ok(None);
        }
    }
    let _kind = h.kind(resource)?;
    let edge = h.handle(graph, 0)?;
    let flags = h.read(edge)?;
    if flags & 0x80000000 != 0 {
        return Ok(None);
    }
    let manager = h.handle(graph, 0x1c)?;
    let service = word(h, manager, 0x8534)?;
    if word(h, service, 0x1c)? == 0 {
        return Ok(None);
    }
    let array = word(h, manager, 0x2cc)?;
    let index = (flags >> 16) & 0x7fff;
    let value = h.read(array.wrapping_add(index * 4))?;
    Ok((value != 0).then_some(value))
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const G: u32 = 0x60000000;
    const PARENT: u32 = G + 0x100;
    const RES: u32 = 0x62000000;
    const CFG: u32 = 0x63000000;
    const DATA: u32 = 0x64000000;
    const PTS: u32 = DATA + 0x1000;
    const EDGES: u32 = DATA + 0x2000;
    const HEADER: u32 = DATA + 0x80;
    const MAN: u32 = 0x65000000;
    const SERVICE: u32 = 0x66000000;
    const POLICIES: u32 = 0x67000000;
    const COUNT: u32 = 0x62e64;
    const IDS: u32 = 0x62e68;
    fn number(v: &Value) -> u32 {
        v.as_u64().unwrap_or_else(|| v.as_i64().unwrap() as u64) as u32
    }
    struct Replay {
        words: BTreeMap<u32, u32>,
        trace: Vec<Value>,
    }
    impl Host for Replay {
        fn read(&mut self, p: u32) -> Result<u32> {
            self.words
                .get(&p)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Unmapped graph connection word {p:#x}"))
        }
        fn write(&mut self, p: u32, v: u32) -> Result<()> {
            ensure!(
                self.words.contains_key(&p),
                "Unmapped graph connection write {p:#x}"
            );
            self.words.insert(p, v);
            Ok(())
        }
        fn kind(&mut self, r: u32) -> Result<u32> {
            self.trace.push(json!(["kind", r]));
            let vt = self.read(r)?;
            match vt {
                0x51ea08 => Ok(0),
                0x51ea58 => Ok(1),
                _ => anyhow::bail!("Unknown resource kind"),
            }
        }
        fn edge_count(&mut self, r: u32) -> Result<u32> {
            self.trace.push(json!(["count", r]));
            let header = self.read(r + 0x28)?;
            self.read(header + 4)
        }
    }
    fn prepare(c: &Value) -> Result<Replay> {
        let dynamic = c["kind"] == 1;
        let stride = if dynamic {
            number(&c["point_stride"])
        } else {
            32
        };
        let edge_stride = if dynamic {
            number(&c["edge_stride"])
        } else {
            16
        };
        let point = |i: u32| if i == u32::MAX { 0 } else { PTS + i * stride };
        let edge = |i: u32| {
            if i == u32::MAX {
                0
            } else {
                EDGES + i * edge_stride
            }
        };
        let mut h = Replay {
            words: BTreeMap::new(),
            trace: vec![],
        };
        for (base, size) in [
            (G, 0x200),
            (RES, 0x40),
            (CFG, 0x84),
            (DATA, 0x4004),
            (MAN, 0x9000),
            (SERVICE, 0x20),
            (POLICIES, 64),
            (COUNT, 4 + 32 * 4),
        ] {
            for off in (0..size).step_by(4) {
                h.words.insert(base + off, 0);
            }
        }
        for (p, v) in [
            (RES, if dynamic { 0x51ea58 } else { 0x51ea08 }),
            (RES + 4, CFG),
            (RES + 0x10, if c["ready"] == true { 5 } else { 4 }),
            (RES + 0x14, DATA),
            (RES + 0x18, 0x4000),
            (RES + 0x28, HEADER),
            (RES + 0x2c, PTS),
            (RES + 0x30, EDGES),
            (CFG + 0x7c, stride),
            (CFG + 0x80, edge_stride),
            (HEADER, c["ids"].as_array().unwrap().len() as u32),
            (HEADER + 4, c["edges"].as_array().unwrap().len() as u32),
        ] {
            h.write(p, v)?;
        }
        for handle in [G, PARENT] {
            h.write(handle + 0x14, RES)?;
            h.write(handle + 0x1c, MAN)?;
        }
        for (p, v) in [
            (G + 4, point(number(&c["point"]))),
            (G + 8, if c["cached"] == true { RES } else { 0 }),
            (G, edge(number(&c["connection"]))),
            (
                G + 12,
                if c["traversal_cached"] == true {
                    RES
                } else {
                    0
                },
            ),
            (G + 0x3c, if c["parent"] == true { PARENT } else { 0 }),
            (PARENT + 0x40, G),
        ] {
            h.write(p, v)?;
        }
        for (i, id) in c["ids"].as_array().unwrap().iter().enumerate() {
            let p = point(i as u32);
            h.write(p, number(id))?;
            h.write(
                p + if dynamic { 0x14 } else { 0x18 },
                point(number(&c["links"][i])),
            )?;
            let start = c["edges"]
                .as_array()
                .unwrap()
                .iter()
                .position(|e| number(&e[1]) == i as u32)
                .map_or(0, |i| i as u32 + 1);
            h.write(p + if dynamic { 0x18 } else { 0x1c }, start)?;
            if dynamic {
                let old = h.read(p + 0x20)?;
                h.write(
                    p + 0x20,
                    (old & 0xffff00ff) | (number(&c["disabled"][i]) << 8),
                )?;
            }
        }
        for (i, row) in c["edges"].as_array().unwrap().iter().enumerate() {
            for j in 0..3 {
                h.write(edge(i as u32) + j as u32 * 4, number(&row[j]))?;
            }
        }
        h.write(MAN + 0x8534, SERVICE)?;
        h.write(SERVICE + 0x1c, u32::from(c["policy_enabled"] == true))?;
        h.write(MAN + 0x2cc, POLICIES)?;
        for i in 0..16 {
            h.write(POLICIES + i * 4, 0x68000000 + i * 0x20)?;
        }
        h.write(COUNT, 9)?;
        for i in 0..32 {
            h.write(IDS + i * 4, 0xdead0000 + i)?;
        }
        Ok(h)
    }
    pub(in crate::blotworx) fn prepared_words(c: &Value) -> Result<BTreeMap<u32, u32>> {
        Ok(prepare(c)?.words)
    }
    #[test]
    fn original_graph_connection_owner_adjacency_filter_and_traversal_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_graph_connection.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let c = &case["input"];
            let mut h = prepare(c)?;
            let result = match c["op"].as_str().unwrap() {
                "index" => {
                    let stride = if c["kind"] == 1 {
                        number(&c["point_stride"])
                    } else {
                        32
                    };
                    let target = if c["by_index"] != 0 {
                        number(&c["target"])
                    } else {
                        PTS + number(&c["target"]) * stride
                    };
                    connection_index(
                        &mut h,
                        Context {
                            count: COUNT,
                            ids: IDS,
                        },
                        G,
                        number(&c["mode"]),
                        target,
                        number(&c["by_index"]),
                    )?
                }
                "owner" => owner(&mut h, G, DATA.wrapping_add(number(&c["owner_offset"])))?,
                "traversal" => traversal(&mut h, G)?.unwrap_or(0),
                _ => unreachable!(),
            };
            assert_eq!(json!(result), case["result"], "connection result {index}");
            let graph: Vec<u32> = (0..4).map(|i| h.read(G + i * 4)).collect::<Result<_>>()?;
            assert_eq!(json!(graph), case["graph"], "connection graph {index}");
            assert_eq!(
                json!(h.read(COUNT)?),
                case["count"],
                "connection count {index}"
            );
            let scratch: Vec<u32> = (0..32)
                .map(|i| h.read(IDS + i * 4))
                .collect::<Result<_>>()?;
            assert_eq!(
                json!(scratch),
                case["scratch"],
                "connection scratch {index}"
            );
            assert_eq!(json!(h.trace), case["trace"], "connection trace {index}");
        }
        Ok(())
    }
}
