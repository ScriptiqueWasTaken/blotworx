//! Direction cones4a6f14/4a7178 and their native cache append semantics.
//! Original sqrt arithmetic is shared; torso orientation is a retained actor
//! attribute, distinct from its forward vector and the world up vector.
use super::ai_goto_gap::{Cache24, Pool, State};
use anyhow::{Context as _, Result};
pub trait Host {
    fn torso(&mut self, state: &mut State, actor: usize) -> Result<Option<[f32; 3]>>;
    fn forward(&mut self, state: &mut State, actor: usize) -> Result<[f32; 3]>;
    fn position(&mut self, state: &mut State, actor: usize) -> Result<[f32; 3]>;
    fn up(&mut self, state: &mut State) -> Result<[f32; 3]>;
    fn zero(&mut self, state: &mut State) -> Result<[f32; 3]>;
}
fn scale(s: &State, index: usize) -> Result<f32> {
    s.scales_74_78[index].context("Goto body scale is unwritten")
}
fn gap(s: &State) -> Result<f32> {
    s.scalars_10_20[4].context("Goto extra gap is unwritten")
}
fn current(s: &State) -> Result<[f32; 3]> {
    let v = s.vectors_5c_80_8c_9c_a8_b4[0];
    Ok([
        v[0].context("Goto position is unwritten")?,
        v[1].context("Goto position is unwritten")?,
        v[2].context("Goto position is unwritten")?,
    ])
}
fn norm_xz(v: [f32; 3], s: &mut State, h: &mut impl Host) -> Result<[f32; 3]> {
    let v = [v[0], 0., v[2]];
    let n = super::cursor::native_sqrt(v[2] * v[2] + (v[0] * v[0] + v[1] * v[1]));
    Ok(if n == 0. {
        h.zero(s)?
    } else {
        v.map(|x| x / n)
    })
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: [f32; 3], v: f32) -> [f32; 3] {
    a.map(|a| v * a)
}
pub fn append(pool: &mut Pool<Cache24>, direction: [f32; 3], threshold: f32) -> Result<()> {
    let Some(index) = pool.free_head else {
        return Ok(());
    };
    let node = pool
        .nodes
        .get_mut(index)
        .context("Goto cone free link is out of bounds")?;
    pool.count = pool.count.wrapping_add(1);
    pool.free_head = node.next;
    for (i, v) in direction.into_iter().enumerate() {
        node.payload.words_00_14[2 + i] = Some(v.to_bits());
    }
    node.payload.words_00_14[5] = Some(threshold.to_bits());
    node.active = 1;
    node.next = None;
    node.previous = pool.tail;
    if let Some(tail) = pool.tail {
        pool.nodes
            .get_mut(tail)
            .context("Goto cone tail is out of bounds")?
            .next = Some(index);
        pool.tail = Some(index);
    } else {
        pool.active_head = Some(index);
        pool.tail = Some(index);
        pool.nodes[index].previous = None;
    }
    Ok(())
}
pub fn sphere(s: &mut State, relative: [f32; 3], diameter: f32) -> Result<()> {
    let Some(pool) = s.pool_118.as_ref() else {
        return Ok(());
    };
    if pool.count == u32::try_from(pool.nodes.len())? {
        return Ok(());
    }
    let squared = relative[0] * relative[0] + relative[2] * relative[2];
    let radius = gap(s)? + 0.5 * (scale(s, 0)? + diameter);
    let range = radius + 0.5 * scale(s, 1)?;
    if squared > range * range || squared == 0. {
        return Ok(());
    }
    let length = super::cursor::native_sqrt(squared);
    let direction = relative.map(|v| v / length);
    let threshold = if squared <= radius * radius {
        0.
    } else {
        super::cursor::native_sqrt(1. - (radius * radius) / squared)
    };
    append(
        s.pool_118.as_mut().context("Goto cone pool disappeared")?,
        direction,
        threshold,
    )
}
pub fn rectangle(
    s: &mut State,
    actor: usize,
    relative: [f32; 3],
    diameter: f32,
    length: f32,
    h: &mut impl Host,
) -> Result<()> {
    if diameter <= 0. || length <= 0. {
        return Ok(());
    }
    let forward = if let Some(v) = h.torso(s, actor)? {
        v
    } else {
        let forward = h.forward(s, actor)?;
        if forward[0] * forward[0] + forward[2] * forward[2] == 0. {
            return Ok(());
        }
        norm_xz(forward, s, h)?
    };
    let up = h.up(s)?;
    let side = [
        forward[1] * up[2] - forward[2] * up[1],
        forward[2] * up[0] - forward[0] * up[2],
        forward[0] * up[1] - forward[1] * up[0],
    ];
    let half_length = 0.5 * length;
    let half_width = 0.5 * diameter;
    let own = scale(s, 1)?;
    let width = gap(s)? + 0.5 * (own + diameter);
    let width_range = width + 0.5 * own;
    let extent = gap(s)? + 0.5 * (own + length);
    let length_range = extent + 0.5 * own;
    let front = relative[2] * forward[2] + relative[0] * forward[0];
    let lateral = relative[2] * side[2] + relative[0] * side[0];
    if front.abs() > length_range || lateral.abs() > width_range {
        return Ok(());
    }
    let (direction, threshold) = if front.abs() <= half_length && lateral.abs() <= half_width {
        if relative[0] * relative[0] + relative[2] * relative[2] == 0. {
            return Ok(());
        }
        (norm_xz(relative, s, h)?, 0.)
    } else if front.abs() <= extent && lateral.abs() <= width {
        let direction = if lateral.abs() >= half_width && front.abs() >= half_length {
            let actor_position = h.position(s, actor)?;
            let f = mul(forward, half_length);
            let w = mul(side, half_width);
            let corner = if lateral > 0. {
                if front > 0. {
                    add(add(actor_position, f), w)
                } else {
                    add(sub(actor_position, f), w)
                }
            } else if front > 0. {
                sub(add(actor_position, f), w)
            } else {
                sub(sub(actor_position, f), w)
            };
            let delta = sub(corner, current(s)?);
            norm_xz(delta, s, h)?
        } else if lateral.abs() >= half_width {
            let v = norm_xz(side, s, h)?;
            if lateral > 0. {
                v
            } else {
                v.map(|x| -x)
            }
        } else {
            let v = norm_xz(forward, s, h)?;
            if front > 0. {
                v
            } else {
                v.map(|x| -x)
            }
        };
        (direction, 0.)
    } else {
        let f = mul(forward, extent);
        let w = mul(side, width);
        let actor = h.position(s, actor)?;
        let plus = add(actor, f);
        let minus = sub(actor, f);
        let corners = [add(plus, w), add(minus, w), sub(minus, w), sub(plus, w)];
        let origin = current(s)?;
        let perpendicular = [relative[2], 0., -relative[0]];
        let mut left_score = -f32::MAX;
        let mut right_score = -f32::MAX;
        let mut left = 0usize;
        let mut right = 0usize;
        let mut left_squared = 0.;
        let mut right_squared = 0.;
        for (i, corner) in corners.iter().enumerate() {
            let delta = sub(*corner, origin);
            let squared = delta[2] * delta[2] + delta[0] * delta[0];
            let dot = delta[2] * perpendicular[2] + delta[0] * perpendicular[0];
            if dot < 0. {
                if dot * dot > left_score * squared {
                    left_score = (dot * dot) / squared;
                    left = i;
                    left_squared = squared;
                }
            } else if dot * dot > right_score * squared {
                right_score = (dot * dot) / squared;
                right = i;
                right_squared = squared;
            }
        }
        let r = sub(corners[right], origin);
        let l = sub(corners[left], origin);
        let ratio = super::cursor::native_sqrt(left_squared / right_squared);
        let sum = add(l, mul(r, ratio));
        let midpoint = add(origin, mul(sum, 0.5));
        let direction = norm_xz(sub(midpoint, current(s)?), s, h)?;
        let l = sub(corners[left], current(s)?);
        let projected = l[2] * direction[2] + l[0] * direction[0];
        let radius = super::cursor::native_sqrt(left_squared);
        let raw = projected / radius;
        // fsel(0-raw,0,raw), including its unordered selection semantics.
        let threshold = if 0. - raw >= 0. { 0. } else { raw };
        (direction, threshold)
    };
    append(
        s.pool_118
            .as_mut()
            .context("Rectangle cone requires its initialized pool")?,
        direction,
        threshold,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Construction;
    impl super::super::ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(Some(1.))
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            Ok(1)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            unreachable!()
        }
    }
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn vector(v: &Value) -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(bits(&v[i])))
    }
    struct Replay<'a>(&'a Value);
    impl Host for Replay<'_> {
        fn torso(&mut self, _: &mut State, _: usize) -> Result<Option<[f32; 3]>> {
            Ok((!self.0["torso"].is_null()).then(|| vector(&self.0["torso"])))
        }
        fn forward(&mut self, _: &mut State, _: usize) -> Result<[f32; 3]> {
            Ok(vector(&self.0["forward"]))
        }
        fn position(&mut self, _: &mut State, _: usize) -> Result<[f32; 3]> {
            Ok(vector(&self.0["actor"]))
        }
        fn up(&mut self, _: &mut State) -> Result<[f32; 3]> {
            Ok(vector(&self.0["up"]))
        }
        fn zero(&mut self, _: &mut State) -> Result<[f32; 3]> {
            Ok(vector(&self.0["zero"]))
        }
    }
    #[test]
    fn original_cone_geometry_and_pool_links_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_cones.json"
        ))?;
        for (index, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &c["input"];
            let mut s = State::construct(1, 2, b"cone", &mut Construction)?;
            s.scales_74_78 = [
                Some(f32::from_bits(bits(&input["own_diameter"]))),
                Some(f32::from_bits(bits(&input["own_length"]))),
            ];
            s.scalars_10_20[4] = Some(f32::from_bits(bits(&input["gap"])));
            s.vectors_5c_80_8c_9c_a8_b4[0] = vector(&input["position"]).map(Some);
            let capacity = bits(&input["capacity"]);
            let mut pool = Pool::new(capacity, Cache24::node)?;
            for (i, node) in pool.nodes.iter_mut().enumerate() {
                for field in 0..4 {
                    node.payload.words_00_14[2 + field] = Some(((i + field + 1) as f32).to_bits());
                }
                node.previous = None;
                node.next = None;
            }
            let active: Vec<usize> = input["active"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| bits(v) as usize)
                .collect();
            let free: Vec<usize> = if input["free"].is_null() {
                (0..capacity as usize)
                    .filter(|i| !active.contains(i))
                    .collect()
            } else {
                input["free"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| bits(v) as usize)
                    .collect()
            };
            pool.active_head = active.first().copied();
            pool.tail = active.last().copied();
            pool.free_head = free.first().copied();
            pool.count = if input["count"].is_null() {
                active.len() as u32
            } else {
                bits(&input["count"])
            };
            for (chain, on) in [(&active, 1), (&free, 0)] {
                for (j, &i) in chain.iter().enumerate() {
                    pool.nodes[i].previous = if on == 1 {
                        j.checked_sub(1).map(|j| chain[j])
                    } else {
                        None
                    };
                    pool.nodes[i].next = chain.get(j + 1).copied();
                    pool.nodes[i].active = on;
                }
            }
            s.pool_118 = input["pool"].as_bool().unwrap().then_some(pool);
            if input["op"] == "sphere" {
                sphere(
                    &mut s,
                    vector(&input["relative"]),
                    f32::from_bits(bits(&input["diameter"])),
                )?;
            } else {
                rectangle(
                    &mut s,
                    3,
                    vector(&input["relative"]),
                    f32::from_bits(bits(&input["diameter"])),
                    f32::from_bits(bits(&input["length"])),
                    &mut Replay(input),
                )?;
            }
            if let Some(pool) = &s.pool_118 {
                let image = json!({"head":pool.active_head,"tail":pool.tail,"free":pool.free_head,"count":pool.count,"nodes":pool.nodes.iter().map(|n|json!({"words":n.payload.words_00_14.map(|v|v.unwrap()),"previous":n.previous,"next":n.next,"active":n.active})).collect::<Vec<_>>()});
                assert_eq!(image, c["pool"], "cone case {index} input {input}");
            }
        }
        Ok(())
    }
}
