//!43f7d0 interval overlay, including original pool identity/order and flags.
//! This is the native gap computation, not World admission or steering scheduling.
use super::{
    ai_gap_manager::{search, Gap, Search, SearchOutput, State},
    ai_goto_gap::Pool,
};
use anyhow::{ensure, Context, Result};
fn field(pool: &Pool<Gap>, index: usize, component: usize) -> Result<f32> {
    pool.nodes
        .get(index)
        .context("Gap interval is outside owned nodes")?
        .payload
        .parameters_08_0c_10[component]
        .context("Gap interval coordinate is uninitialized")
}
fn payload(pool: &Pool<Gap>, index: usize) -> Result<Gap> {
    Ok(pool
        .nodes
        .get(index)
        .context("Gap interval is outside owned nodes")?
        .payload)
}
fn gap(lower: f32, upper: f32, depth: f32, limit: f32, flag: u8) -> Gap {
    Gap {
        parameters_08_0c_10: [Some(lower), Some(upper), Some(depth)],
        limit_14: Some(limit),
        flag_18: Some(flag),
    }
}
fn insert(pool: &mut Pool<Gap>, before: Option<usize>, value: Gap) -> Result<Option<usize>> {
    let Some(index) = pool.free_head else {
        return Ok(None);
    };
    let next = pool
        .nodes
        .get(index)
        .context("Gap free-list head is outside owned nodes")?
        .next;
    pool.count = pool.count.wrapping_add(1);
    pool.free_head = next;
    pool.nodes[index].payload = value;
    pool.nodes[index].active = 1;
    if let Some(before) = before {
        let previous = pool
            .nodes
            .get(before)
            .context("Gap insertion target is outside owned nodes")?
            .previous;
        pool.nodes[index].next = Some(before);
        pool.nodes[before].previous = Some(index);
        pool.nodes[index].previous = previous;
        if let Some(previous) = previous {
            pool.nodes
                .get_mut(previous)
                .context("Gap predecessor is outside owned nodes")?
                .next = Some(index);
        } else {
            pool.active_head = Some(index);
        }
    } else {
        let previous = pool.tail;
        pool.nodes[index].previous = previous;
        pool.nodes[index].next = None;
        if let Some(previous) = previous {
            pool.nodes
                .get_mut(previous)
                .context("Gap tail is outside owned nodes")?
                .next = Some(index);
        } else {
            pool.active_head = Some(index);
        }
        pool.tail = Some(index);
    }
    Ok(Some(index))
}
fn remove(pool: &mut Pool<Gap>, index: usize) -> Result<Option<usize>> {
    let node = pool
        .nodes
        .get(index)
        .context("Gap removal target is outside owned nodes")?;
    let (previous, next) = (node.previous, node.next);
    pool.nodes[index].active = 0;
    if pool.active_head == Some(index) {
        pool.active_head = next;
    }
    if pool.tail == Some(index) {
        pool.tail = previous;
    }
    if let Some(previous) = previous {
        pool.nodes
            .get_mut(previous)
            .context("Gap predecessor is outside owned nodes")?
            .next = next;
    }
    if let Some(next) = next {
        pool.nodes
            .get_mut(next)
            .context("Gap successor is outside owned nodes")?
            .previous = previous;
    }
    pool.nodes[index].next = pool.free_head;
    pool.free_head = Some(index);
    pool.count = pool.count.wrapping_sub(1);
    Ok(next)
}
fn remove_between(pool: &mut Pool<Gap>, mut current: Option<usize>, end: usize) -> Result<()> {
    let mut steps = 0usize;
    while let Some(index) = current {
        if index == end {
            break;
        }
        ensure!(
            index < pool.nodes.len(),
            "Gap removal target is outside owned nodes"
        );
        ensure!(steps < pool.nodes.len(), "Cycle in gap interval removal");
        steps += 1;
        current = remove(pool, index)?;
    }
    Ok(())
}
fn locate(
    pool: &Pool<Gap>,
    mut current: Option<usize>,
    upper: f32,
    inclusive: bool,
) -> Result<Option<usize>> {
    let mut steps = 0usize;
    while let Some(index) = current {
        let value = field(pool, index, 1)?;
        ensure!(steps < pool.nodes.len(), "Cycle in gap interval lookup");
        steps += 1;
        if if inclusive {
            value >= upper
        } else {
            upper < value
        } {
            return Ok(Some(index));
        }
        current = pool.nodes[index].next;
    }
    Ok(None)
}
fn overlay(
    pool: &mut Pool<Gap>,
    lower: f32,
    upper: f32,
    depth: f32,
    limit: f32,
    mut flag: u8,
) -> Result<bool> {
    let capacity =
        u32::try_from(pool.nodes.len()).context("Gap capacity exceeds native word domain")?;
    // Preserve the original unsigned subtraction, including capacities below3.
    if pool.count > capacity.wrapping_sub(3) {
        return Ok(false);
    }
    let Some(start) = locate(pool, pool.active_head, lower, false)? else {
        return Ok(false);
    };
    let Some(end) = locate(pool, Some(start), upper, true)? else {
        return Ok(false);
    };
    let mut current = Some(start);
    let mut steps = 0usize;
    while let Some(index) = current {
        ensure!(
            index < pool.nodes.len(),
            "Gap flag interval is outside owned nodes"
        );
        ensure!(steps < pool.nodes.len(), "Cycle in gap flag propagation");
        steps += 1;
        if flag != 0 {
            if field(pool, index, 2)? < depth {
                pool.nodes[index].payload.flag_18 = Some(1);
            }
        } else if payload(pool, index)?
            .flag_18
            .context("Gap flag is uninitialized")?
            != 0
        {
            flag = 1;
            break;
        }
        if index == end {
            break;
        }
        current = pool.nodes[index].next;
    }
    if start == end {
        if depth < field(pool, start, 2)? {
            let old = payload(pool, start)?;
            let old_lower =
                old.parameters_08_0c_10[0].context("Gap lower endpoint is uninitialized")?;
            let old_depth = old.parameters_08_0c_10[2].context("Gap depth is uninitialized")?;
            let old_limit = old.limit_14.context("Gap limit is uninitialized")?;
            if lower != old_lower {
                insert(
                    pool,
                    Some(end),
                    gap(old_lower, lower, old_depth, old_limit, 0),
                )?;
            }
            let old_upper = field(pool, end, 1)?;
            if upper != old_upper {
                insert(pool, Some(end), gap(lower, upper, depth, limit, flag))?;
                pool.nodes[end].payload.parameters_08_0c_10[0] = Some(upper);
            } else {
                pool.nodes[end].payload = gap(lower, old_upper, depth, limit, flag);
            }
        }
        return Ok(true);
    }
    let mut cursor = Some(start);
    let mut steps = 0usize;
    loop {
        ensure!(steps < pool.nodes.len(), "Cycle in gap overlay scan");
        steps += 1;
        let mut above = SearchOutput {
            value: 0.,
            interval: None,
        };
        if !search(
            pool,
            cursor,
            Some(end),
            [lower, upper],
            depth,
            Search::Above,
            &mut above,
        )? {
            break;
        }
        let a = above
            .interval
            .context("Successful gap search has no interval")?;
        let mut below = SearchOutput {
            value: 0.,
            interval: None,
        };
        if search(
            pool,
            Some(a),
            Some(end),
            [lower, upper],
            depth,
            Search::Below,
            &mut below,
        )? {
            let b = below
                .interval
                .context("Successful gap search has no interval")?;
            if a != end {
                if field(pool, a, 1)? > above.value {
                    pool.nodes[a].payload.parameters_08_0c_10[1] = Some(above.value);
                }
                if field(pool, b, 0)? < below.value {
                    pool.nodes[b].payload.parameters_08_0c_10[0] = Some(below.value);
                }
                let next = pool.nodes[a].next;
                remove_between(pool, next, b)?;
                insert(
                    pool,
                    Some(b),
                    gap(above.value, below.value, depth, limit, flag),
                )?;
                if b == end {
                    break;
                }
                cursor = pool.nodes[b].next;
            } else {
                let old = payload(pool, end)?;
                let old_lower = field(pool, end, 0)?;
                if above.value != old_lower {
                    let old_depth = field(pool, end, 2)?;
                    insert(
                        pool,
                        Some(end),
                        gap(
                            old_lower,
                            above.value,
                            old_depth,
                            old.limit_14.context("Gap limit is uninitialized")?,
                            0,
                        ),
                    )?;
                }
                insert(
                    pool,
                    Some(b),
                    gap(above.value, below.value, depth, limit, flag),
                )?;
                break;
            }
        } else {
            if a != end {
                let next = pool.nodes[a].next;
                remove_between(pool, next, end)?;
                if field(pool, a, 0)? < above.value {
                    pool.nodes[a].payload.parameters_08_0c_10[1] = Some(above.value);
                } else {
                    remove(pool, a)?;
                }
            } else if field(pool, end, 0)? < above.value {
                let old = payload(pool, end)?;
                let old_lower = field(pool, end, 0)?;
                let old_depth = field(pool, end, 2)?;
                insert(
                    pool,
                    Some(end),
                    gap(
                        old_lower,
                        above.value,
                        old_depth,
                        old.limit_14.context("Gap limit is uninitialized")?,
                        0,
                    ),
                )?;
            }
            if field(pool, end, 1)? > upper {
                pool.nodes[end].payload.parameters_08_0c_10[0] = Some(upper);
                insert(pool, Some(end), gap(above.value, upper, depth, limit, flag))?;
            } else {
                let old_upper = field(pool, end, 1)?;
                pool.nodes[end].payload = gap(above.value, old_upper, depth, limit, flag);
            }
            break;
        }
    }
    Ok(true)
}
pub fn compute(state: &mut State, input: &Gap) -> Result<bool> {
    let cutoff = state.parameters_08_0c_14[2].context("Gap manager cutoff is uninitialized")?;
    let depth = input.parameters_08_0c_10[2].context("Input gap depth is uninitialized")?;
    if depth > cutoff {
        return Ok(true);
    }
    let lower = input.parameters_08_0c_10[0].context("Input lower bound is uninitialized")?;
    let upper = input.parameters_08_0c_10[1].context("Input upper bound is uninitialized")?;
    let flag = input.flag_18.context("Input gap flag is uninitialized")?;
    let limit = input.limit_14.context("Input gap limit is uninitialized")?;
    let minimum =
        state.parameters_08_0c_14[0].context("Gap manager lower bound is uninitialized")?;
    let maximum =
        state.parameters_08_0c_14[1].context("Gap manager upper bound is uninitialized")?;
    if lower < minimum || lower >= upper || upper > maximum {
        return Ok(false);
    }
    let pool = state
        .pool
        .as_mut()
        .context("Gap manager has no allocated pool")?;
    if !overlay(pool, lower, upper, depth, limit, flag)? {
        return Ok(false);
    }
    let old = state
        .limit_10
        .context("Gap manager minimum depth is uninitialized")?;
    if depth < old {
        state.limit_10 = Some(depth);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn snapshot(state: &State) -> Result<Vec<u8>> {
        let pool = state.pool.as_ref().context("Test pool missing")?;
        let index = |i: Option<usize>| i.map_or(0, |i| i as u32 + 1);
        let mut image = vec![];
        for value in [
            0x519a38,
            1,
            state.parameters_08_0c_14[0].unwrap().to_bits(),
            state.parameters_08_0c_14[1].unwrap().to_bits(),
            state.limit_10.unwrap().to_bits(),
            state.parameters_08_0c_14[2].unwrap().to_bits(),
            index(pool.active_head),
            index(pool.tail),
            index(pool.free_head),
            pool.count,
        ] {
            image.extend(value.to_be_bytes());
        }
        for node in &pool.nodes {
            let mut bytes = vec![0xa5; 0x28];
            for (at, value) in [
                (0, Some(0x5199f0)),
                (4, Some(0x519ad8)),
                (8, node.payload.parameters_08_0c_10[0].map(f32::to_bits)),
                (0xc, node.payload.parameters_08_0c_10[1].map(f32::to_bits)),
                (0x10, node.payload.parameters_08_0c_10[2].map(f32::to_bits)),
                (0x14, node.payload.limit_14.map(f32::to_bits)),
                (0x1c, Some(index(node.previous))),
                (0x20, Some(index(node.next))),
            ] {
                if let Some(value) = value {
                    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
                }
            }
            if let Some(flag) = node.payload.flag_18 {
                bytes[0x18] = flag;
            }
            bytes[0x24] = node.active;
            image.extend(bytes);
        }
        Ok(image)
    }
    #[test]
    fn navigation_gap_compute_matches_original_complete_overlay_and_pool_identity() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_gap_compute.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let capacity = c["capacity"].as_u64().unwrap() as u32;
            let mut state = State::new(50);
            state.pool = Some(Pool::new(capacity, Gap::node)?);
            let default = [0xbf800000u32, 0x3f800000, 0x7f7fffff];
            let parameters = if let Some(values) = c["parameters"].as_array() {
                std::array::from_fn(|i| values[i].as_u64().unwrap() as u32)
            } else {
                default
            };
            state.configure(parameters.map(f32::from_bits))?;
            let mut results = vec![];
            for row in c["inputs"].as_array().unwrap() {
                let number = |key: &str| row[key].as_u64().map(|v| f32::from_bits(v as u32));
                let input = Gap {
                    parameters_08_0c_10: [number("lower"), number("upper"), number("depth")],
                    limit_14: number("limit"),
                    flag_18: row["flag"].as_u64().map(|v| v as u8),
                };
                results.push(
                    compute(&mut state, &input)
                        .with_context(|| format!("Overlay error for {c}"))?,
                );
            }
            let baseline = &fixture["baselines"][capacity.to_string()];
            let mut expected_bytes: Vec<u8> = serde_json::from_value(baseline.clone())?;
            for (offset, value) in
                serde_json::from_value::<Vec<(usize, u8)>>(expected["overrides"].clone())?
            {
                *expected_bytes
                    .get_mut(offset)
                    .context("Fixture override is outside snapshot")? = value;
            }
            let expected_results: Vec<bool> = serde_json::from_value(expected["results"].clone())?;
            ensure!(
                results == expected_results,
                "Overlay return mismatch for {c}: {results:?} != {expected_results:?}"
            );
            let actual = snapshot(&state)?;
            ensure!(
                actual.len() == expected_bytes.len(),
                "Overlay snapshot lengths differ"
            );
            if let Some(offset) = actual.iter().zip(&expected_bytes).position(|(a, b)| a != b) {
                anyhow::bail!(
                    "Overlay byte mismatch at{offset}: {} != {}; case{c}",
                    actual[offset],
                    expected_bytes[offset]
                );
            }
        }
        Ok(())
    }
}
