//! Spatial entity cache463204, queries463354/463460 and cell quantizers463730/77c.
//! All fields use the caller's canonical storage. No parallel actor/manager copy.
use anyhow::{ensure, Result};
pub trait Host {
    fn read(&mut self, address: u32) -> Result<u32>;
    fn write(&mut self, address: u32, value: u32) -> Result<()>;
    fn time(&mut self) -> Result<f32>;
    fn byte(&mut self, address: u32) -> Result<u8> {
        Ok((self.read(address)? >> 24) as u8)
    }
    fn write_byte(&mut self, address: u32, value: u8) -> Result<()> {
        let old = self.read(address)?;
        self.write(address, (old & 0xffffff) | ((value as u32) << 24))
    }
}
#[derive(Clone, Copy)]
pub struct Context {
    pub world: u32,
}
fn scalar(h: &mut impl Host, address: u32) -> Result<f32> {
    Ok(f32::from_bits(h.read(address)?))
}
fn put(h: &mut impl Host, address: u32, value: f32) -> Result<()> {
    h.write(address, value.to_bits())
}
fn capacity(h: &mut impl Host, c: Context) -> Result<u32> {
    let world = h.read(c.world)?;
    h.read(world.wrapping_add(0x14))
}
fn row(h: &mut impl Host, m: u32, index: u32) -> Result<u32> {
    Ok(h.read(m.wrapping_add(12))?
        .wrapping_add(index.wrapping_mul(12)))
}
/// Native uses truncation then subtracts one for every negative input, including
/// exact cell boundaries. This intentionally differs from mathematical floor.
pub fn quantize(value: f32, size: f32) -> Result<i32> {
    let ratio = value / size;
    ensure!(
        ratio.is_finite() && (-2147483648.0..2147483648.0).contains(&(ratio as f64)),
        "Unverified entity cell fctiwz conversion for {ratio}"
    );
    let integer = ratio as i32;
    Ok(if value >= 0. {
        integer
    } else {
        integer.wrapping_sub(1)
    })
}
fn compare(h: &mut impl Host, a: u32, key: [i32; 2]) -> Result<i32> {
    let x = h.read(a.wrapping_add(4))? as i32;
    if x < key[0] {
        return Ok(-1);
    }
    if x > key[0] {
        return Ok(1);
    }
    let z = h.read(a.wrapping_add(8))? as i32;
    Ok(if z < key[1] {
        -1
    } else {
        i32::from(z > key[1])
    })
}
fn row_compare(h: &mut impl Host, m: u32, a: u32, b: u32) -> Result<i32> {
    let b = row(h, m, b)?;
    let key = [
        h.read(b.wrapping_add(4))? as i32,
        h.read(b.wrapping_add(8))? as i32,
    ];
    let a = row(h, m, a)?;
    compare(h, a, key)
}
fn swap(h: &mut impl Host, m: u32, a: u32, b: u32) -> Result<()> {
    let a = row(h, m, a)?;
    let b = row(h, m, b)?;
    for i in [0, 4, 8] {
        let av = h.read(a.wrapping_add(i))?;
        let bv = h.read(b.wrapping_add(i))?;
        h.write(a.wrapping_add(i), bv)?;
        h.write(b.wrapping_add(i), av)?;
    }
    Ok(())
}
/// Original DOL800177d0 heap order, including equal-cell actor ordering.
fn sort(h: &mut impl Host, m: u32, count: u32) -> Result<()> {
    if count < 2 {
        return Ok(());
    }
    let mut left = count / 2 + 1;
    let mut right = count;
    loop {
        if left > 1 {
            left -= 1;
        } else {
            swap(h, m, right - 1, 0)?;
            right -= 1;
            if right == 1 {
                break;
            }
        }
        let mut current = left;
        while current * 2 <= right {
            let mut child = current * 2;
            if child < right && row_compare(h, m, child - 1, child)? < 0 {
                child += 1;
            }
            if row_compare(h, m, current - 1, child - 1)? >= 0 {
                break;
            }
            swap(h, m, current - 1, child - 1)?;
            current = child;
        }
    }
    Ok(())
}
fn actor_index(h: &mut impl Host, c: Context, actor: u32) -> Result<u32> {
    let node = h.read(actor.wrapping_add(0x14))?;
    if node == 0 {
        return Ok(u32::MAX);
    }
    let world = h.read(c.world)?;
    let list = h.read(world.wrapping_add(0x18))?;
    let first = h.read(list.wrapping_add(4))?;
    let difference = node.wrapping_sub(first);
    Ok((((difference as u64) * 0xcccccccd) >> 32) as u32 >> 4)
}
pub fn rebuild(h: &mut impl Host, c: Context, m: u32) -> Result<()> {
    let time = h.time()?;
    if !(scalar(h, m.wrapping_add(0x10))? <= time) {
        return Ok(());
    }
    let period = scalar(h, m.wrapping_add(8))?;
    put(h, m.wrapping_add(0x10), time + period)?;
    let mut i = 0u32;
    while i < capacity(h, c)? {
        let p = row(h, m, i)?;
        h.write(p, 0)?;
        h.write(p.wrapping_add(4), u32::MAX)?;
        h.write(p.wrapping_add(8), u32::MAX)?;
        i = i.wrapping_add(1);
    }
    let world = h.read(c.world)?;
    let list = h.read(world.wrapping_add(0x18))?;
    let mut node = h.read(list.wrapping_add(8))?;
    let mut visited = std::collections::BTreeSet::new();
    while node != 0 {
        ensure!(visited.insert(node), "Cycle in entity profile actor list");
        let actor = h.read(node.wrapping_add(4))?;
        let i = actor_index(h, c, actor)?;
        let p = row(h, m, i)?;
        h.write(p, actor)?;
        let size = scalar(h, m.wrapping_add(4))?;
        let x = quantize(scalar(h, actor.wrapping_add(0x34))?, size)?;
        let p = row(h, m, i)?;
        h.write(p.wrapping_add(4), x as u32)?;
        let size = scalar(h, m.wrapping_add(4))?;
        let z = quantize(scalar(h, actor.wrapping_add(0x3c))?, size)?;
        let p = row(h, m, i)?;
        h.write(p.wrapping_add(8), z as u32)?;
        node = h.read(node.wrapping_add(12))?;
    }
    let count = capacity(h, c)?;
    sort(h, m, count)
}
pub fn first(
    h: &mut impl Host,
    c: Context,
    m: u32,
    position: [f32; 3],
    radius: f32,
) -> Result<Option<u32>> {
    rebuild(h, c, m)?;
    let minimum = [position[0] - radius, position[1], position[2] - radius];
    let maximum = [position[0] + radius, position[1], position[2] + radius];
    for (offset, v, axis, increment) in [
        (0x18, minimum, 0, 0),
        (0x1c, maximum, 0, 1),
        (0x20, minimum, 2, 0),
        (0x24, maximum, 2, 1),
    ] {
        let size = scalar(h, m.wrapping_add(4))?;
        h.write(
            m.wrapping_add(offset),
            (quantize(v[axis], size)? as u32).wrapping_add(increment),
        )?;
    }
    let x = h.read(m.wrapping_add(0x18))?;
    h.write(m.wrapping_add(0x28), x)?;
    let z = h.read(m.wrapping_add(0x20))?;
    h.write(m.wrapping_add(0x2c), z)?;
    for (i, v) in position.into_iter().enumerate() {
        put(h, m.wrapping_add(0x40 + i as u32 * 4), v)?;
    }
    put(h, m.wrapping_add(0x4c), radius * radius)?;
    h.write(m.wrapping_add(0x30), u32::MAX)?;
    h.write(m.wrapping_add(0x34), u32::MAX)?;
    next(h, c, m)
}
fn matches(h: &mut impl Host, m: u32, p: u32) -> Result<bool> {
    Ok(h.read(m.wrapping_add(0x28))? == h.read(p.wrapping_add(4))?
        && h.read(m.wrapping_add(0x2c))? == h.read(p.wrapping_add(8))?)
}
fn within(h: &mut impl Host, m: u32, p: u32) -> Result<Option<u32>> {
    let actor = h.read(p)?;
    if actor == 0 {
        return Ok(None);
    }
    let dz = scalar(h, m.wrapping_add(0x48))? - scalar(h, actor.wrapping_add(0x3c))?;
    let dx = scalar(h, m.wrapping_add(0x40))? - scalar(h, actor.wrapping_add(0x34))?;
    let dy = scalar(h, m.wrapping_add(0x44))? - scalar(h, actor.wrapping_add(0x38))?;
    let squared = dy * dy + (dz * dz + dx * dx);
    Ok((squared < scalar(h, m.wrapping_add(0x4c))?).then_some(actor))
}
pub fn next(h: &mut impl Host, c: Context, m: u32) -> Result<Option<u32>> {
    while (h.read(m.wrapping_add(0x28))? as i32) < h.read(m.wrapping_add(0x1c))? as i32 {
        while (h.read(m.wrapping_add(0x2c))? as i32) < h.read(m.wrapping_add(0x24))? as i32 {
            let key = [
                h.read(m.wrapping_add(0x28))? as i32,
                h.read(m.wrapping_add(0x2c))? as i32,
            ];
            if h.read(m.wrapping_add(0x30))? == u32::MAX {
                let mut low = 0u32;
                let mut high = capacity(h, c)?.wrapping_sub(1);
                ensure!(
                    high != u32::MAX,
                    "Entity profile capacity is zero during binary query"
                );
                while high.wrapping_sub(low) > 1 {
                    let mid = low.wrapping_add(high) >> 1;
                    let p = row(h, m, mid)?;
                    if compare(h, p, key)? < 0 {
                        low = mid
                    } else {
                        high = mid
                    }
                }
                let p = row(h, m, low)?;
                let start = if compare(h, p, key)? <= 0 { low } else { high };
                h.write(m.wrapping_add(0x38), start)?;
                h.write_byte(m.wrapping_add(0x3c), 1)?;
                h.write(m.wrapping_add(0x30), low)?;
                h.write(m.wrapping_add(0x34), high)?;
            }
            if h.byte(m.wrapping_add(0x3c))? != 0 {
                loop {
                    let index = h.read(m.wrapping_add(0x38))?;
                    let p = row(h, m, index)?;
                    if !matches(h, m, p)? {
                        break;
                    }
                    if let Some(actor) = within(h, m, p)? {
                        if index == 0 {
                            let low = h.read(m.wrapping_add(0x30))?;
                            h.write(m.wrapping_add(0x38), low.wrapping_add(1))?;
                            h.write_byte(m.wrapping_add(0x3c), 0)?;
                        } else {
                            h.write(m.wrapping_add(0x38), index - 1)?;
                        }
                        return Ok(Some(actor));
                    }
                    if index == 0 {
                        break;
                    }
                    h.write(m.wrapping_add(0x38), index - 1)?;
                }
                let low = h.read(m.wrapping_add(0x30))?;
                h.write(m.wrapping_add(0x38), low.wrapping_add(1))?;
                h.write_byte(m.wrapping_add(0x3c), 0)?;
            }
            loop {
                let index = h.read(m.wrapping_add(0x38))?;
                if index >= capacity(h, c)? {
                    break;
                }
                let p = row(h, m, index)?;
                if !matches(h, m, p)? {
                    break;
                }
                if let Some(actor) = within(h, m, p)? {
                    h.write(m.wrapping_add(0x38), index.wrapping_add(1))?;
                    return Ok(Some(actor));
                }
                h.write(m.wrapping_add(0x38), index.wrapping_add(1))?;
            }
            h.write(m.wrapping_add(0x30), u32::MAX)?;
            let z = h.read(m.wrapping_add(0x2c))?;
            h.write(m.wrapping_add(0x2c), z.wrapping_add(1))?;
        }
        let x = h.read(m.wrapping_add(0x28))?;
        h.write(m.wrapping_add(0x28), x.wrapping_add(1))?;
        let z = h.read(m.wrapping_add(0x20))?;
        h.write(m.wrapping_add(0x2c), z)?;
        h.write(m.wrapping_add(0x30), u32::MAX)?;
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const M: u32 = 0x60000000;
    const WORLD: u32 = 0x61000000;
    const LIST: u32 = 0x62000000;
    const LINKS: u32 = 0x62001000;
    const ACTORS: u32 = 0x63000000;
    const ROWS: u32 = 0x64000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    struct Replay {
        words: BTreeMap<u32, u32>,
        time: f32,
        trace: Vec<&'static str>,
    }
    impl Host for Replay {
        fn read(&mut self, address: u32) -> Result<u32> {
            self.words
                .get(&address)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Unmapped profile word {address:#x}"))
        }
        fn write(&mut self, address: u32, value: u32) -> Result<()> {
            ensure!(
                self.words.contains_key(&address),
                "Unmapped profile write {address:#x}"
            );
            self.words.insert(address, value);
            Ok(())
        }
        fn time(&mut self) -> Result<f32> {
            self.trace.push("time");
            Ok(self.time)
        }
    }
    fn state(h: &mut Replay) -> Result<Vec<u32>> {
        [0x10]
            .into_iter()
            .chain((0x18..0x50).step_by(4))
            .map(|off| h.read(M + off))
            .collect()
    }
    #[test]
    fn original_profile_rebuild_quantization_sort_and_iteration_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_entity_profile_query.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let capacity = bits(&input["capacity"]);
            let actors = input["actors"].as_array().unwrap();
            let actor = |i: usize| ACTORS + i as u32 * 0x80;
            let link = |i: usize| LINKS + bits(&actors[i]["slot"]) * 20;
            let mut h = Replay {
                words: BTreeMap::new(),
                time: f32::from_bits(bits(&input["time"])),
                trace: vec![],
            };
            for (base, size) in [
                (M, 0x50),
                (WORLD, 0x20),
                (LIST, 0x20),
                (LINKS, capacity * 20),
                (ACTORS, actors.len().max(1) as u32 * 0x80),
                (ROWS, capacity * 12),
                (0x5a108, 4),
            ] {
                for offset in (0..size).step_by(4) {
                    h.words.insert(base + offset, 0);
                }
            }
            for (p, v) in [
                (0x5a108, WORLD),
                (WORLD + 0x14, capacity),
                (WORLD + 0x18, LIST),
                (LIST + 4, LINKS),
                (LIST + 8, if actors.is_empty() { 0 } else { link(0) }),
                (M + 4, bits(&input["cell"])),
                (M + 8, bits(&input["period"])),
                (M + 12, ROWS),
                (M + 0x10, bits(&input["deadline"])),
                (M + 0x3c, 0x07aabbcc),
            ] {
                h.write(p, v)?;
            }
            let mut rows = vec![[0, u32::MAX, u32::MAX]; capacity as usize];
            for (i, a) in actors.iter().enumerate() {
                h.write(actor(i) + 0x14, link(i))?;
                h.write(link(i) + 4, actor(i))?;
                h.write(
                    link(i) + 12,
                    if i + 1 < actors.len() { link(i + 1) } else { 0 },
                )?;
                for j in 0..3 {
                    h.write(actor(i) + 0x34 + j as u32 * 4, bits(&a["position"][j]))?;
                }
                if input["cached"] == true {
                    let size = f32::from_bits(bits(&input["cell"]));
                    let coordinate = |axis: usize| {
                        let v = f32::from_bits(bits(&a["position"][axis]));
                        ((v / size) as i32).wrapping_sub(i32::from(v < 0.)) as u32
                    };
                    rows[bits(&a["slot"]) as usize] = [actor(i), coordinate(0), coordinate(2)];
                }
            }
            if input["cached"] == true {
                rows.sort_by_key(|r| (r[1] as i32, r[2] as i32));
            }
            for (i, r) in rows.iter().enumerate() {
                for j in 0..3 {
                    h.write(ROWS + i as u32 * 12 + j as u32 * 4, r[j])?;
                }
            }
            let c = Context { world: 0x5a108 };
            let mut outputs = vec![];
            let mut states = vec![];
            if input["op"] == "rebuild" {
                rebuild(&mut h, c, M)?;
                states.push(state(&mut h)?);
            } else {
                let position = std::array::from_fn(|i| f32::from_bits(bits(&input["position"][i])));
                outputs.push(
                    first(
                        &mut h,
                        c,
                        M,
                        position,
                        f32::from_bits(bits(&input["radius"])),
                    )?
                    .unwrap_or(0),
                );
                states.push(state(&mut h)?);
                while outputs.last() != Some(&0) {
                    outputs.push(next(&mut h, c, M)?.unwrap_or(0));
                    states.push(state(&mut h)?);
                }
                outputs.push(next(&mut h, c, M)?.unwrap_or(0));
                states.push(state(&mut h)?);
            }
            assert_eq!(json!(outputs), case["outputs"], "profile outputs {index}");
            assert_eq!(json!(states), case["states"], "profile state {index}");
            let rows: Vec<u32> = (0..capacity * 3)
                .map(|i| h.read(ROWS + i * 4))
                .collect::<Result<_>>()?;
            assert_eq!(json!(rows), case["rows"], "profile rows {index}");
            assert_eq!(json!(h.trace), case["trace"], "profile trace {index}");
        }
        Ok(())
    }
}
