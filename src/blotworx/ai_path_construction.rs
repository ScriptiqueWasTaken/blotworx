//! Polygon-route construction4425c0 and cache/filter publication4444bc.
//! Query1c/60/70/74 and other spatial bindings remain mandatory services.
use super::{
    ai_actor_queries::{Filter, Runtime},
    ai_astar_graph_services::GraphContext,
    ai_edge_crossing::{self, Acceptance, Globals},
    ai_graph_link_passes::Memory,
    ai_region_geometry::{self, vertex, Children},
    ai_segment_query::{Context, WalkPoint},
    ai_traversal_instances::Instance,
};
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
fn word(m: &Memory, b: u32, o: u32) -> Result<u32> {
    m.word(b.wrapping_add(o))
}
fn set(m: &mut Memory, b: u32, o: u32, v: u32) -> Result<()> {
    m.write(b.wrapping_add(o), v)
}
fn xyz(m: &Memory, b: u32, o: u32) -> Result<[u32; 3]> {
    Ok([word(m, b, o)?, word(m, b, o + 4)?, word(m, b, o + 8)?])
}
fn write_xyz(m: &mut Memory, b: u32, o: u32, p: [u32; 3]) -> Result<()> {
    for i in 0..3 {
        set(m, b, o + i as u32 * 4, p[i])?;
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub struct Work {
    pub globals: Globals,
    pub path_visits: u32,
}
pub trait Queries {
    ///1c may change the point buffer and output count. Its return is low-byte.
    fn filter(
        &mut self,
        c: &mut Context<'_, '_>,
        query: u32,
        points: u32,
        count: u32,
        output: &mut u32,
    ) -> Result<u8>;
    fn cost70(
        &mut self,
        c: &mut Context<'_, '_>,
        query: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        cost: &mut u32,
    ) -> Result<u8>;
    fn cost74(
        &mut self,
        c: &mut Context<'_, '_>,
        query: u32,
        start: &mut WalkPoint,
        end: &mut WalkPoint,
        cost: &mut u32,
    ) -> Result<()>;
    fn direct60(
        &mut self,
        c: &mut Context<'_, '_>,
        query: u32,
        start: [u32; 3],
        end: [u32; 3],
        cost: &mut u32,
    ) -> Result<u8>;
}
#[derive(Clone, Copy)]
struct Layout {
    previous: u32,
    next: u32,
    active: u32,
}
const CACHE: Layout = Layout {
    previous: 8,
    next: 12,
    active: 16,
};
const POINT: Layout = Layout {
    previous: 16,
    next: 20,
    active: 24,
};
fn clear(m: &mut Memory, pool: u32, l: Layout) -> Result<()> {
    let mut visited = BTreeSet::new();
    loop {
        let node = word(m, pool, 8)?;
        if node == 0 {
            break;
        }
        ensure!(visited.insert(node), "Cyclic native route pool");
        m.write_byte(node.wrapping_add(l.active), 0)?;
        let head = word(m, pool, 8)?;
        let next = word(m, head, l.next)?;
        let free = word(m, pool, 16)?;
        set(m, head, l.next, free)?;
        let head = word(m, pool, 8)?;
        set(m, pool, 16, head)?;
        set(m, pool, 8, next)?;
    }
    set(m, pool, 12, 0)?;
    set(m, pool, 20, 0)
}
fn append(
    m: &mut Memory,
    pool: u32,
    l: Layout,
    value: impl FnOnce(&mut Memory, u32) -> Result<()>,
) -> Result<u32> {
    let node = word(m, pool, 16)?;
    if node == 0 {
        return Ok(0);
    }
    let count = word(m, pool, 20)?.wrapping_add(1);
    set(m, pool, 20, count)?;
    let free = word(m, node, l.next)?;
    set(m, pool, 16, free)?;
    value(m, node)?;
    m.write_byte(node.wrapping_add(l.active), 1)?;
    let tail = word(m, pool, 12)?;
    if tail == 0 {
        set(m, pool, 8, node)?;
        set(m, pool, 12, node)?;
        set(m, node, l.next, 0)?;
        let head = word(m, pool, 8)?;
        set(m, head, l.previous, 0)?;
        let tail = word(m, pool, 12)?;
        set(m, tail, l.previous, 0)?;
        let tail = word(m, pool, 12)?;
        set(m, tail, l.next, 0)?;
    } else {
        set(m, node, l.previous, tail)?;
        set(m, node, l.next, 0)?;
        let tail = word(m, pool, 12)?;
        set(m, tail, l.next, node)?;
        set(m, pool, 12, node)?;
    }
    Ok(node)
}
fn cache_append(m: &mut Memory, pool: u32, property: u32) -> Result<u32> {
    append(m, pool, CACHE, |m, n| set(m, n, 4, property))
}
fn point_append(m: &mut Memory, pool: u32, p: [u32; 3]) -> Result<bool> {
    Ok(append(m, pool, POINT, |m, n| write_xyz(m, n, 4, p))? != 0)
}
///440898: valid timestamp comparison, not allocation or an unconditional state.
pub fn cached_state(m: &Memory, property: u32, config: u32) -> Result<u32> {
    if f32::from_bits(word(m, property, 8)?) >= f32::from_bits(word(m, config, 0xd4)?) {
        word(m, property, 4)
    } else {
        Ok(0)
    }
}
fn mark(c: &mut Context<'_, '_>, g: Globals, list_node: u32, value: u32) -> Result<()> {
    let property = word(c.graph.tables, list_node, 4)?;
    set(c.graph.tables, property, 4, value)?;
    let clock = g.clock(c.graph.tables)?;
    set(c.graph.tables, property, 8, clock)
}
///4444bc. Workspace/header reads after virtual1c remain fresh.
pub fn filter(
    c: &mut Context<'_, '_>,
    work: Work,
    region: u32,
    query: u32,
    prefix: u32,
    suffix: u32,
    host: &mut impl Queries,
) -> Result<u8> {
    let config = word(c.graph.tables, region, 4)?;
    let workspace = word(c.graph.tables, config, 4)?;
    let points = word(c.graph.tables, workspace, 0xa4)?;
    let pool = word(c.graph.tables, workspace, 0xa8)?;
    let count = word(c.graph.tables, workspace, 0xa0)?;
    if count < 2 {
        return Ok(1);
    }
    let visits = c.graph.tables.word(work.path_visits)?.wrapping_add(count);
    c.graph.tables.write(work.path_visits, visits)?;
    let count = word(c.graph.tables, workspace, 0xa0)?;
    let index = word(c.graph.tables, config, 0xac)? as i32;
    work.globals.charge(c.graph.tables, count, index)?;
    // The original reads the last XYZ before1c, even though that stack copy
    // is not subsequently consumed. Preserve required data-access diagnostics.
    let count = word(c.graph.tables, workspace, 0xa0)?;
    let _ = xyz(
        c.graph.tables,
        points,
        count.wrapping_sub(1).wrapping_mul(12),
    )?;
    let mut output = count;
    let result = host.filter(c, query, points, count, &mut output)?;
    let mut node = word(c.graph.tables, pool, 8)?;
    if node == 0 {
        return Ok(result);
    }
    if output == 0 {
        mark(c, work.globals, node, 2)?;
        return Ok(0);
    }
    if prefix != 0 {
        mark(c, work.globals, node, 1)?;
    }
    node = word(c.graph.tables, node, 12)?;
    let mut index = 1u32;
    let mut visited = BTreeSet::new();
    while node != 0 && index < output {
        ensure!(visited.insert(node), "Cyclic route cache marks");
        mark(c, work.globals, node, 1)?;
        node = word(c.graph.tables, node, 12)?;
        index = index.wrapping_add(1);
    }
    if node != 0 {
        if output == word(c.graph.tables, workspace, 0xa0)? {
            if suffix != 0 {
                mark(c, work.globals, node, 1)?;
            }
        } else {
            mark(c, work.globals, node, 2)?;
        }
    }
    Ok(result)
}
fn scratch_push(m: &mut Memory, workspace: u32, points: u32, p: [u32; 3]) -> Result<()> {
    let count = word(m, workspace, 0xa0)?;
    set(m, workspace, 0xa0, count.wrapping_add(1))?;
    write_xyz(m, points, count.wrapping_mul(12), p)
}
fn scratch_reset(
    m: &mut Memory,
    workspace: u32,
    points: u32,
    p: [u32; 3],
    cache: u32,
) -> Result<()> {
    set(m, workspace, 0xa0, 1)?;
    write_xyz(m, points, 0, p)?;
    clear(m, cache, CACHE)
}
fn outline(m: &Memory, polygon: u32) -> Result<u32> {
    word(m, polygon, 4)
}
fn next_forward(m: &Memory, polygon: u32, current: u32) -> Result<(u32, u32)> {
    let outline = outline(m, polygon)?;
    let previous = if current == word(m, outline, 12)? {
        word(m, outline, 8)?
    } else {
        current
    };
    Ok((previous, word(m, previous, 0x30)?))
}
fn next_backward(m: &Memory, polygon: u32, current: u32, last: u32) -> Result<Option<u32>> {
    let outline = outline(m, polygon)?;
    let current = if current == word(m, outline, 8)? {
        word(m, outline, 12)?
    } else {
        current
    };
    if current == last {
        Ok(None)
    } else {
        Ok(Some(word(m, current, 0x2c)?))
    }
}
#[derive(Clone, Copy)]
struct Scratch {
    workspace: u32,
    points: u32,
    pool: u32,
}
fn prepare_direction(
    c: &mut Context<'_, '_>,
    work: Work,
    region: u32,
    query: u32,
    polygon: u32,
    first: u32,
    last: u32,
    start: [u32; 3],
    end: [u32; 3],
    s: Scratch,
    backward: bool,
    host: &mut impl Queries,
) -> Result<bool> {
    set(c.graph.tables, s.workspace, 0xa0, 1)?;
    write_xyz(c.graph.tables, s.points, 0, start)?;
    let mut prefix = 0;
    let mut previous = word(c.graph.tables, first, 0x2c)?;
    clear(c.graph.tables, s.pool, CACHE)?;
    let mut current = first;
    let mut valid = true;
    let mut visited = BTreeSet::new();
    while current != 0 {
        if backward {
            match next_backward(c.graph.tables, polygon, current, last)? {
                None => break,
                Some(v) => {
                    current = v;
                    if v == 0 {
                        valid = false;
                        break;
                    }
                    previous = current;
                }
            }
        }
        ensure!(visited.insert(current), "Cyclic native route outline");
        let property = if backward {
            current.wrapping_add(0x20)
        } else {
            previous.wrapping_add(0x14)
        };
        let config = word(c.graph.tables, region, 4)?;
        match cached_state(c.graph.tables, property, config)? {
            0 => {
                let p = vertex(c.graph.tables, current)?.map(f32::to_bits);
                scratch_push(c.graph.tables, s.workspace, s.points, p)?;
                cache_append(c.graph.tables, s.pool, property)?;
            }
            1 => {
                if filter(c, work, region, query, prefix, 1, host)? == 0 {
                    valid = false
                } else {
                    let p = vertex(c.graph.tables, current)?.map(f32::to_bits);
                    scratch_reset(c.graph.tables, s.workspace, s.points, p, s.pool)?;
                    prefix = 1;
                }
            }
            _ => valid = false,
        }
        if !valid {
            break;
        }
        if word(c.graph.tables, s.workspace, 0xa0)? == word(c.graph.tables, s.workspace, 0x9c)? {
            let accepted = filter(c, work, region, query, prefix, 1, host)? != 0;
            if !accepted {
                valid = false;
                if backward {
                    break;
                }
            }
            let p = vertex(c.graph.tables, current)?.map(f32::to_bits);
            scratch_reset(c.graph.tables, s.workspace, s.points, p, s.pool)?;
            if backward {
                cache_append(c.graph.tables, s.pool, property)?;
            } else {
                //4429a0 sets the forward prefix after a capacity flush.
                //442e24..442f50 retains the backward prefix instead.
                prefix = 1;
            }
        }
        if !backward {
            let (p, n) = next_forward(c.graph.tables, polygon, current)?;
            previous = p;
            current = n;
            if current == 0 {
                valid = false;
                break;
            }
            if current == last {
                break;
            }
        }
    }
    if valid {
        let property = previous.wrapping_add(if backward { 0x20 } else { 0x14 });
        let config = word(c.graph.tables, region, 4)?;
        let mut append_end = true;
        match cached_state(c.graph.tables, property, config)? {
            0 | 2 => {}
            1 => {
                if filter(c, work, region, query, prefix, 1, host)? == 0 {
                    valid = false
                }
                append_end = false;
            }
            _ => {
                valid = false;
                append_end = false;
            }
        }
        if append_end {
            scratch_push(c.graph.tables, s.workspace, s.points, end)?;
            if filter(c, work, region, query, prefix, 0, host)? == 0 {
                valid = false
            }
        }
    }
    Ok(valid)
}
fn cost_direction(
    c: &mut Context<'_, '_>,
    query: u32,
    polygon: u32,
    first: u32,
    last: u32,
    start: [u32; 3],
    end: [u32; 3],
    backward: bool,
    from: &mut WalkPoint,
    to: &mut WalkPoint,
    host: &mut impl Queries,
) -> Result<(bool, u32)> {
    from.position = start;
    let mut current = first;
    let mut total = 0f32;
    let mut visited = BTreeSet::new();
    while current != 0 {
        if backward {
            match next_backward(c.graph.tables, polygon, current, last)? {
                None => break,
                Some(v) => current = v,
            }
        }
        ensure!(visited.insert(current), "Cyclic route cost outline");
        to.position = vertex(c.graph.tables, current)?.map(f32::to_bits);
        let mut cost = 0;
        if host.cost70(c, query, from, to, &mut cost)? == 0 {
            return Ok((false, total.to_bits()));
        }
        total += f32::from_bits(cost);
        cost = 0;
        host.cost74(c, query, from, to, &mut cost)?;
        total += f32::from_bits(cost);
        *from = to.clone();
        if !backward {
            current = next_forward(c.graph.tables, polygon, current)?.1;
            if current == last {
                break;
            }
        }
    }
    *to = WalkPoint {
        position: end,
        auxiliary: [0; 16],
    };
    let mut cost = 0;
    let valid = host.cost70(c, query, from, to, &mut cost)? != 0;
    total += f32::from_bits(cost);
    cost = 0;
    host.cost74(c, query, from, to, &mut cost)?;
    total += f32::from_bits(cost);
    Ok((valid, total.to_bits()))
}
fn publish(
    m: &mut Memory,
    pool: u32,
    polygon: u32,
    first: u32,
    last: u32,
    start: [u32; 3],
    end: [u32; 3],
    backward: Option<bool>,
) -> Result<u32> {
    if !point_append(m, pool, start)? {
        return Ok(0);
    }
    if let Some(backward) = backward {
        let mut current = first;
        let mut visited = BTreeSet::new();
        while current != 0 {
            if backward {
                match next_backward(m, polygon, current, last)? {
                    None => break,
                    Some(v) => current = v,
                }
            }
            ensure!(visited.insert(current), "Cyclic published route outline");
            let p = vertex(m, current)?.map(f32::to_bits);
            if !point_append(m, pool, p)? {
                return Ok(0);
            }
            if !backward {
                current = next_forward(m, polygon, current)?.1;
                if current == last {
                    break;
                }
            }
        }
    }
    Ok(if point_append(m, pool, end)? { 2 } else { 0 })
}
pub fn construct(
    c: &mut Context<'_, '_>,
    work: Work,
    region: u32,
    polygon: u32,
    first: u32,
    last: u32,
    start: [u32; 3],
    end: [u32; 3],
    query: u32,
    pool: u32,
    scalar: &mut u32,
    direct: &mut u8,
    host: &mut impl Queries,
) -> Result<u32> {
    *direct = 0;
    let outline = outline(c.graph.tables, polygon)?;
    if word(c.graph.tables, outline, 16)? < 2 {
        return Ok(0);
    }
    let config = word(c.graph.tables, region, 4)?;
    let workspace = word(c.graph.tables, config, 4)?;
    let points = word(c.graph.tables, workspace, 0xa4)?;
    let cache = word(c.graph.tables, workspace, 0xa8)?;
    let scratch = Scratch {
        workspace,
        points,
        pool: cache,
    };
    let mut forward = prepare_direction(
        c, work, region, query, polygon, first, last, start, end, scratch, false, host,
    )?;
    let mut backward = prepare_direction(
        c, work, region, query, polygon, first, last, start, end, scratch, true, host,
    )?;
    let mut from = WalkPoint {
        position: [0; 3],
        auxiliary: [0; 16],
    };
    let mut to = from.clone();
    let mut fc = 0;
    let mut bc = 0;
    if forward {
        (forward, fc) = cost_direction(
            c, query, polygon, first, last, start, end, false, &mut from, &mut to, host,
        )?;
    }
    if backward {
        (backward, bc) = cost_direction(
            c, query, polygon, first, last, start, end, true, &mut from, &mut to, host,
        )?;
    }
    if forward && backward {
        if f32::from_bits(fc) > f32::from_bits(bc) {
            forward = false
        } else {
            backward = false
        }
    }
    let mut direct_cost = 0;
    let available = host.direct60(c, query, start, end, &mut direct_cost)? != 0;
    if available
        && ((!forward && !backward)
            || f32::from_bits(direct_cost) < f32::from_bits(if forward { fc } else { bc }))
    {
        *scalar = direct_cost;
        *direct = 1;
        return publish(c.graph.tables, pool, polygon, first, last, start, end, None);
    }
    if forward {
        *scalar = fc;
        publish(
            c.graph.tables,
            pool,
            polygon,
            first,
            last,
            start,
            end,
            Some(false),
        )
    } else if backward {
        *scalar = bc;
        publish(
            c.graph.tables,
            pool,
            polygon,
            first,
            last,
            start,
            end,
            Some(true),
        )
    } else {
        Ok(1)
    }
}
pub trait Bindings: Children + Acceptance + Queries {
    fn invalidate(
        &mut self,
        r: &mut Runtime,
        g: &mut GraphContext<'_, Memory>,
        i: &mut Instance,
        f: Filter,
    ) -> Result<u8>;
    fn walk(
        &mut self,
        c: &mut Context<'_, '_>,
        query: u32,
        p: &mut WalkPoint,
        end: [u32; 3],
        iterator: &mut u32,
        output: &mut [u32; 2],
    ) -> Result<u8>;
}
pub struct Native<H> {
    pub work: Work,
    pub spatial: H,
}
impl<H: Bindings> Children for Native<H> {
    fn contains(&mut self, m: &Memory, child: u32, p: [u32; 3], flag: u32) -> Result<u8> {
        self.spatial.contains(m, child, p, flag)
    }
}
impl<H: Bindings> Acceptance for Native<H> {
    fn accept(&mut self, c: &mut Context<'_, '_>, q: u32, s: [u32; 3], p: [u32; 3]) -> Result<u8> {
        self.spatial.accept(c, q, s, p)
    }
}
impl<H: Bindings> ai_edge_crossing::Services for Native<H> {
    fn path(
        &mut self,
        c: &mut Context<'_, '_>,
        r: u32,
        p: &mut u32,
        first: &mut u32,
        last: &mut u32,
        s: [u32; 3],
        e: [u32; 3],
        q: u32,
        pool: u32,
        scalar: &mut u32,
    ) -> Result<u32> {
        let mut direct = 0;
        construct(
            c,
            self.work,
            r,
            *p,
            *first,
            *last,
            s,
            e,
            q,
            pool,
            scalar,
            &mut direct,
            &mut self.spatial,
        )
    }
    fn invalidate(
        &mut self,
        r: &mut Runtime,
        g: &mut GraphContext<'_, Memory>,
        i: &mut Instance,
        f: Filter,
    ) -> Result<u8> {
        self.spatial.invalidate(r, g, i, f)
    }
    fn walk(
        &mut self,
        c: &mut Context<'_, '_>,
        q: u32,
        p: &mut WalkPoint,
        e: [u32; 3],
        i: &mut u32,
        o: &mut [u32; 2],
    ) -> Result<u8> {
        self.spatial.walk(c, q, p, e, i, o)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ai_goto_traversal, ai_graph_handles, ai_graph_queries::Metadata};
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const CFG: u32 = 0x60000000;
    const REGION: u32 = 0x61000000;
    const NODES: u32 = 0x64000000;
    const VERT: u32 = 0x63000000;
    const WORKSPACE: u32 = 0x5e000000;
    const POINTS: u32 = 0x5e001000;
    const CACHE_POOL: u32 = 0x5e002000;
    const CNODES: u32 = 0x5e003000;
    const QUERY: u32 = 0x6b000000;
    const POOL: u32 = 0x6d005000;
    const PNODES: u32 = 0x6d006000;
    const FRAME: u32 = 0x6f000000;
    const VISITS: u32 = 0x5f003000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn vector(v: &Value) -> [u32; 3] {
        std::array::from_fn(|i| bits(&v[i]))
    }
    fn memory(c: &Value) -> Result<(Memory, Work)> {
        let mut m = ai_region_geometry::fixture_memory(&c["globals"]["geometry"])?;
        let globals = ai_edge_crossing::fixture_globals(&mut m, &c["globals"])?;
        for (at, size) in [
            (WORKSPACE, 0xb0),
            (POINTS, 192),
            (CACHE_POOL, 0x18),
            (POOL, 0x18),
            (VISITS, 4),
        ] {
            m.regions
                .insert(at, vec![if at == POINTS { 0xa5 } else { 0 }; size]);
        }
        for (at, v) in [
            (CFG + 4, WORKSPACE),
            (CFG + 0xac, c["budget_index"].as_i64().unwrap() as u32),
            (CFG + 0xd4, bits(&c["snapshot"])),
            (WORKSPACE + 0x9c, bits(&c["capacity"])),
            (WORKSPACE + 0xa0, bits(&c["initial_count"])),
            (WORKSPACE + 0xa4, POINTS),
            (WORKSPACE + 0xa8, CACHE_POOL),
        ] {
            m.write(at, v)?;
        }
        for i in 0..5 {
            for (off, v) in [
                (0x18, bits(&c["forward"][i])),
                (0x24, bits(&c["backward"][i])),
                (0x1c, bits(&c["cache_time"])),
                (0x28, bits(&c["cache_time"])),
            ] {
                m.write(VERT + i as u32 * 0x40 + off, v)?;
            }
        }
        for (pool, base, size, active, count, l) in [
            (
                CACHE_POOL,
                CNODES,
                0x14,
                bits(&c["cache_active"]),
                bits(&c["cache_capacity"]),
                CACHE,
            ),
            (
                POOL,
                PNODES,
                0x20,
                bits(&c["output_active"]),
                bits(&c["output_capacity"]),
                POINT,
            ),
        ] {
            m.regions.insert(base, vec![0xa5; (count * size) as usize]);
            for (off, v) in [
                (8, if active > 0 { base } else { 0 }),
                (
                    12,
                    if active > 0 {
                        base + (active - 1) * size
                    } else {
                        0
                    },
                ),
                (
                    16,
                    if active < count {
                        base + active * size
                    } else {
                        0
                    },
                ),
                (20, active),
            ] {
                set(&mut m, pool, off, v)?;
            }
            for i in 0..count {
                let at = base + i * size;
                set(
                    &mut m,
                    at,
                    l.next,
                    if if i < active {
                        i + 1 < active
                    } else {
                        i + 1 < count
                    } {
                        base + (i + 1) * size
                    } else {
                        0
                    },
                )?;
                set(
                    &mut m,
                    at,
                    l.previous,
                    if i > 0 { base + (i - 1) * size } else { 0 },
                )?;
                m.write_byte(at + l.active, u8::from(i < active))?;
                if size == 0x14 {
                    set(&mut m, at, 4, VERT + (i % 5) * 0x40 + 0x14)?;
                }
            }
        }
        Ok((
            m,
            Work {
                globals,
                path_visits: VISITS,
            },
        ))
    }
    fn graph() -> ai_graph_handles::State {
        ai_graph_handles::State {
            phase: 0,
            serial: 0,
            pool: Vec::new(),
            nodes: BTreeMap::new(),
            resources: BTreeMap::new(),
            current: None,
            tracked_resource: None,
            tracked158: 0,
            tracked160: 0,
            deferred_enabled: 0,
        }
    }
    fn actors() -> Runtime {
        Runtime {
            pathfinder_class: 0,
            sentinel: [0; 3],
            constraints: BTreeMap::new(),
            owners: BTreeMap::new(),
            services: BTreeMap::new(),
            classes: BTreeMap::new(),
            queries: BTreeMap::new(),
            pools: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }
    struct Fixture<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        filters: i64,
        costs70: i64,
        costs74: i64,
    }
    impl Queries for Fixture<'_> {
        fn filter(
            &mut self,
            c: &mut Context<'_, '_>,
            q: u32,
            points: u32,
            count: u32,
            output: &mut u32,
        ) -> Result<u8> {
            let values = (0..count)
                .map(|i| xyz(c.graph.tables, points, i * 12))
                .collect::<Result<Vec<_>>>()?;
            self.trace
                .push(json!(["filter", q, count, *output, values]));
            *output = match self.case["filter_rule"].as_str().unwrap() {
                "same" => count,
                "zero" => 0,
                "one" => 1,
                "minus1" => count.saturating_sub(1),
                "beyond" => count + 1,
                _ => unreachable!(),
            };
            let index = self.filters;
            self.filters += 1;
            Ok(if index == self.case["filter_fail_at"].as_i64().unwrap() {
                0
            } else {
                bits(&self.case["filter_result"]) as u8
            })
        }
        fn cost70(
            &mut self,
            _: &mut Context<'_, '_>,
            q: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            cost: &mut u32,
        ) -> Result<u8> {
            let i = self.costs70;
            self.costs70 += 1;
            self.trace.push(json!([
                "70",
                q,
                [s.position, s.auxiliary],
                [e.position, e.auxiliary],
                *cost
            ]));
            *cost = bits(&self.case["cost70"]);
            if self.case["mutate_aux"] == true {
                s.auxiliary[1] = 300 + i as u32;
                e.auxiliary[0] = 100 + i as u32;
            }
            Ok(u8::from(i != self.case["fail70_at"].as_i64().unwrap()))
        }
        fn cost74(
            &mut self,
            _: &mut Context<'_, '_>,
            q: u32,
            s: &mut WalkPoint,
            e: &mut WalkPoint,
            cost: &mut u32,
        ) -> Result<()> {
            let i = self.costs74;
            self.costs74 += 1;
            self.trace.push(json!([
                "74",
                q,
                [s.position, s.auxiliary],
                [e.position, e.auxiliary],
                *cost
            ]));
            *cost = bits(&self.case["cost74"]);
            if self.case["mutate_aux"] == true {
                s.auxiliary[1] = 300 + i as u32;
                e.auxiliary[0] = 200 + i as u32;
            }
            Ok(())
        }
        fn direct60(
            &mut self,
            _: &mut Context<'_, '_>,
            q: u32,
            s: [u32; 3],
            e: [u32; 3],
            cost: &mut u32,
        ) -> Result<u8> {
            self.trace.push(json!(["direct", q, s, e, *cost]));
            *cost = bits(&self.case["direct_cost"]);
            Ok(bits(&self.case["direct_result"]) as u8)
        }
    }
    impl Children for Fixture<'_> {
        fn contains(&mut self, _: &Memory, _: u32, _: [u32; 3], _: u32) -> Result<u8> {
            anyhow::bail!("Child geometry not part of path-construction fixture")
        }
    }
    impl Acceptance for Fixture<'_> {
        fn accept(
            &mut self,
            _: &mut Context<'_, '_>,
            _: u32,
            _: [u32; 3],
            _: [u32; 3],
        ) -> Result<u8> {
            anyhow::bail!("Spatial acceptance not part of path-construction fixture")
        }
    }
    impl Bindings for Fixture<'_> {
        fn invalidate(
            &mut self,
            _: &mut Runtime,
            _: &mut GraphContext<'_, Memory>,
            _: &mut Instance,
            _: Filter,
        ) -> Result<u8> {
            anyhow::bail!("Cache invalidation not part of path fixture")
        }
        fn walk(
            &mut self,
            _: &mut Context<'_, '_>,
            _: u32,
            _: &mut WalkPoint,
            _: [u32; 3],
            _: &mut u32,
            _: &mut [u32; 2],
        ) -> Result<u8> {
            anyhow::bail!("Outer spatial walk not part of path fixture")
        }
    }
    fn bytes(m: &Memory, base: u32, count: u32) -> Result<Vec<u8>> {
        (0..count).map(|i| m.byte(base + i)).collect()
    }
    #[test]
    fn original_path_builder_filter_caches_and_callback_state_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_path_construction.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let (mut memory, work) = memory(c)?;
            let mut graph = graph();
            let mut metadata = Metadata::default();
            let mut actors = actors();
            let mut instance = ai_goto_traversal::construct(1);
            let mut gc = GraphContext {
                graph: &mut graph,
                tables: &mut memory,
                metadata: &mut metadata,
            };
            let mut context = Context {
                actors: &mut actors,
                graph: &mut gc,
                instance: &mut instance,
            };
            let mut host = Fixture {
                case: c,
                trace: Vec::new(),
                filters: 0,
                costs70: 0,
                costs74: 0,
            };
            let mut scalar = 9f32.to_bits();
            let mut direct = 255;
            let result = if c["op"] == "filter" {
                filter(
                    &mut context,
                    work,
                    REGION,
                    QUERY,
                    bits(&c["prefix"]),
                    bits(&c["suffix"]),
                    &mut host,
                )? as u32
            } else {
                construct(
                    &mut context,
                    work,
                    REGION,
                    NODES,
                    VERT + bits(&c["first"]) * 0x40,
                    VERT + bits(&c["last"]) * 0x40,
                    vector(&c["start"]),
                    vector(&c["end"]),
                    QUERY,
                    POOL,
                    &mut scalar,
                    &mut direct,
                    &mut host,
                )?
            };
            assert_eq!(json!(result), expected["result"], "{c}");
            assert_eq!(json!(scalar), expected["scalar"], "{c}");
            assert_eq!(json!(direct), expected["direct"], "{c}");
            assert_eq!(host.trace, *expected["trace"].as_array().unwrap(), "{c}");
            assert_eq!(
                json!(word(&memory, WORKSPACE, 0xa0)?),
                expected["scratch_count"],
                "{c}"
            );
            assert_eq!(
                json!(bytes(&memory, POINTS, 192)?),
                expected["points"],
                "{c}"
            );
            for (pool, base, count, key, nodes) in [
                (
                    CACHE_POOL,
                    CNODES,
                    bits(&c["cache_capacity"]) * 0x14,
                    "cache",
                    "cache_nodes",
                ),
                (
                    POOL,
                    PNODES,
                    bits(&c["output_capacity"]) * 0x20,
                    "pool",
                    "pool_nodes",
                ),
            ] {
                assert_eq!(
                    json!([
                        word(&memory, pool, 8)?,
                        word(&memory, pool, 12)?,
                        word(&memory, pool, 16)?,
                        word(&memory, pool, 20)?
                    ]),
                    expected[key],
                    "{c}"
                );
                assert_eq!(json!(bytes(&memory, base, count)?), expected[nodes], "{c}");
            }
            let states = (0..5)
                .map(|i| {
                    Ok([
                        word(&memory, VERT + i * 0x40, 0x18)?,
                        word(&memory, VERT + i * 0x40, 0x1c)?,
                        word(&memory, VERT + i * 0x40, 0x24)?,
                        word(&memory, VERT + i * 0x40, 0x28)?,
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(json!(states), expected["states"], "{c}");
            assert_eq!(json!(memory.word(VISITS)?), expected["visits"], "{c}");
            assert_eq!(
                json!(word(&memory, FRAME, 0x7c)?),
                expected["budget"],
                "{c}"
            );
        }
        Ok(())
    }

    #[test]
    fn region_query_executes_native_crossings_and_complete_path_construction() -> Result<()> {
        use super::super::{ai_astar_memory::Node, ai_region_segment};
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_constructed_region.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let pc = &c["path_case"];
            let (mut memory, mut graph) = ai_region_segment::fixture(c)?;
            let (mut path_memory, work) = self::memory(pc)?;
            // The caller owns its output pool. Import only the shared query
            // workspace, geometry and context from the path fixture builder.
            path_memory.regions.remove(&POOL);
            path_memory.regions.remove(&PNODES);
            memory.regions.extend(path_memory.regions);
            let mut metadata = Metadata::default();
            let mut actors = actors();
            let mut instance = ai_goto_traversal::construct(1);
            let mut gc = GraphContext {
                graph: &mut graph,
                tables: &mut memory,
                metadata: &mut metadata,
            };
            let mut context = Context {
                actors: &mut actors,
                graph: &mut gc,
                instance: &mut instance,
            };
            let mut native = ai_region_segment::Native {
                bindings: ai_edge_crossing::Native {
                    globals: work.globals,
                    services: Native {
                        work,
                        spatial: Fixture {
                            case: pc,
                            trace: Vec::new(),
                            filters: 0,
                            costs70: 0,
                            costs74: 0,
                        },
                    },
                },
            };
            let mut scalar = bits(&c["scalar"]);
            let mut seed: Node = serde_json::from_value(c["seed"].clone())?;
            let result = super::super::ai_segment_query::Services::segment(
                &mut native,
                &mut context,
                QUERY,
                vector(&c["start"]),
                vector(&c["end"]),
                REGION,
                0x6d000000,
                &mut scalar,
                &mut seed,
            )?;
            assert_eq!(json!(result), expected["result"], "{c}");
            assert_eq!(json!(scalar), expected["scalar"], "{c}");
            assert_eq!(serde_json::to_value(seed)?, expected["seed"], "{c}");
            assert_eq!(
                native.bindings.services.spatial.trace,
                *expected["trace"].as_array().unwrap(),
                "{c}"
            );
            assert_eq!(
                json!([
                    word(&memory, 0x6d000000, 8)?,
                    word(&memory, 0x6d000000, 12)?,
                    word(&memory, 0x6d000000, 16)?,
                    word(&memory, 0x6d000000, 20)?
                ]),
                expected["pool"],
                "{c}"
            );
            assert_eq!(
                json!(bytes(
                    &memory,
                    0x75000000,
                    (bits(&c["pool_count"]) + 1) * 0x20
                )?),
                expected["nodes"],
                "{c}"
            );
            assert_eq!(
                json!(word(&memory, WORKSPACE, 0xa0)?),
                expected["scratch_count"],
                "{c}"
            );
            assert_eq!(
                json!(bytes(&memory, POINTS, 192)?),
                expected["points"],
                "{c}"
            );
            assert_eq!(
                json!([
                    word(&memory, CACHE_POOL, 8)?,
                    word(&memory, CACHE_POOL, 12)?,
                    word(&memory, CACHE_POOL, 16)?,
                    word(&memory, CACHE_POOL, 20)?
                ]),
                expected["cache"]
            );
            assert_eq!(
                json!(bytes(&memory, CNODES, bits(&pc["cache_capacity"]) * 0x14)?),
                expected["cache_nodes"]
            );
            let states = (0..5)
                .map(|i| {
                    Ok([
                        word(&memory, VERT + i * 0x40, 0x18)?,
                        word(&memory, VERT + i * 0x40, 0x1c)?,
                        word(&memory, VERT + i * 0x40, 0x24)?,
                        word(&memory, VERT + i * 0x40, 0x28)?,
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(json!(states), expected["states"], "{c}");
            for (at, key) in [
                (VISITS, "path_visits"),
                (work.globals.polygon_visits, "visits"),
                (work.globals.intersections, "counter"),
                (work.globals.intersections + 16, "pending"),
                (FRAME + 0x7c, "budget"),
            ] {
                assert_eq!(json!(memory.word(at)?), expected[key], "{key}: {c}");
            }
        }
        Ok(())
    }
}
