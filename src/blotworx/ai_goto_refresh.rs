//! Original4a5564 obstacle refresh. Actor list links, profile iterators and
//! attributes are read at their original query points, never cached as copies.
use super::{ai_goto_cones, ai_goto_gap::State};
use anyhow::{Context as _, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribute {
    Height,
    Diameter,
    Length,
}
pub trait Host: ai_goto_cones::Host {
    fn time(&mut self, state: &mut State) -> Result<f32>;
    fn force_refresh(&mut self, state: &mut State, pathfinder: usize) -> Result<u8>;
    fn unit_scale(&mut self, state: &mut State) -> Result<Option<f32>>;
    fn scalar(&mut self, state: &mut State, attribute: usize) -> Result<f32>;
    fn attribute(
        &mut self,
        state: &mut State,
        actor: usize,
        kind: Attribute,
    ) -> Result<Option<usize>>;
    fn predicate(
        &mut self,
        state: &mut State,
        callback: usize,
        own: usize,
        other: usize,
    ) -> Result<u32>;
    fn profile_first(
        &mut self,
        state: &mut State,
        manager: usize,
        position: [f32; 3],
        radius: f32,
    ) -> Result<Option<usize>>;
    fn profile_next(&mut self, state: &mut State, manager: usize) -> Result<Option<usize>>;
    fn actors_first(&mut self, state: &mut State) -> Result<Option<usize>>;
    fn actor_at(&mut self, state: &mut State, node: usize) -> Result<usize>;
    fn actors_next(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn sphere(
        &mut self,
        state: &mut State,
        _actor: usize,
        relative: [f32; 3],
        diameter: f32,
    ) -> Result<()> {
        ai_goto_cones::sphere(state, relative, diameter)
    }
    fn rectangle(
        &mut self,
        state: &mut State,
        actor: usize,
        relative: [f32; 3],
        diameter: f32,
        length: f32,
    ) -> Result<()>
    where
        Self: Sized,
    {
        ai_goto_cones::rectangle(state, actor, relative, diameter, length, self)
    }
}
fn parameter(s: &State, index: usize) -> Result<f32> {
    s.scalars_10_20[index].context("Goto refresh parameter is unwritten")
}
fn vector(s: &State, index: usize) -> Result<[f32; 3]> {
    let v = s.vectors_5c_80_8c_9c_a8_b4[index];
    Ok([
        v[0].context("Goto refresh vector is unwritten")?,
        v[1].context("Goto refresh vector is unwritten")?,
        v[2].context("Goto refresh vector is unwritten")?,
    ])
}
fn own(s: &State) -> Result<usize> {
    s.entity_58.context("Goto refresh owner is unwritten")
}
fn retained(s: &mut State, h: &mut impl Host, index: usize) -> Result<Option<f32>> {
    match s.attributes_68_6c_70[index].context("Goto refresh attribute reference is unwritten")? {
        Some(a) => Ok(Some(h.scalar(s, a)?)),
        None => Ok(None),
    }
}
fn attribute(
    s: &mut State,
    h: &mut impl Host,
    actor: usize,
    kind: Attribute,
) -> Result<Option<f32>> {
    match h.attribute(s, actor, kind)? {
        Some(a) => Ok(Some(h.scalar(s, a)?)),
        None => Ok(None),
    }
}
fn obstacle(s: &mut State, h: &mut impl Host, actor: usize, range_check: bool) -> Result<()> {
    let entity = own(s)?;
    if actor == entity {
        return Ok(());
    }
    if let Some(callback) = s
        .reference_54
        .context("Goto refresh predicate reference is unwritten")?
    {
        if h.predicate(s, callback, entity, actor)? & 255 != 1 {
            return Ok(());
        }
    }
    if range_check {
        let position = h.position(s, actor)?;
        let source = vector(s, 0)?;
        let delta = std::array::from_fn::<_, 3, _>(|i| source[i] - position[i]);
        let squared = delta[1] * delta[1] + (delta[2] * delta[2] + delta[0] * delta[0]);
        let range = parameter(s, 0)?;
        if !(squared < range * range) {
            return Ok(());
        }
    }
    let half_height = match attribute(s, h, actor, Attribute::Height)? {
        Some(v) => 0.5 * v,
        None => h.unit_scale(s)?.unwrap_or(1.),
    };
    let entity = own(s)?;
    let other = h.position(s, actor)?;
    let own = h.position(s, entity)?;
    let relative = std::array::from_fn(|i| other[i] - own[i]);
    let plane = vector(s, 4)?;
    let dot = relative[2] * plane[2] + (relative[0] * plane[0] + relative[1] * plane[1]);
    let bound = s
        .scalar_c4
        .context("Goto refresh height cache is unwritten")?
        + half_height;
    if dot < -bound || dot > bound {
        return Ok(());
    }
    let diameter = match attribute(s, h, actor, Attribute::Diameter)? {
        Some(v) => v,
        None => h.unit_scale(s)?.unwrap_or(1.),
    };
    match attribute(s, h, actor, Attribute::Length)? {
        Some(length) => h.rectangle(s, actor, relative, diameter, length),
        None => h.sphere(s, actor, relative, diameter),
    }
}
pub fn refresh(s: &mut State, h: &mut impl Host) -> Result<()> {
    let time = h.time(s)?;
    let deadline = s.scalars_cc_d0[1].context("Goto refresh deadline is unwritten")?;
    if time < deadline {
        return Ok(());
    }
    if time < deadline + 1.5 * parameter(s, 1)? {
        let parent = s.pathfinder;
        if h.force_refresh(s, parent)? == 0 {
            return Ok(());
        }
    }
    s.scalars_cc_d0[1] = Some(time + parameter(s, 1)?);
    s.pool_118
        .as_mut()
        .context("Goto refresh requires its initialized cone pool")?
        .clear()?;
    let entity = own(s)?;
    s.vectors_5c_80_8c_9c_a8_b4[0] = h.position(s, entity)?.map(Some);
    let diameter = match retained(s, h, 1)? {
        Some(v) => v,
        None => h.unit_scale(s)?.unwrap_or(1.),
    };
    s.scales_74_78[0] = Some(diameter);
    let (length, rect) = match retained(s, h, 2)? {
        Some(v) if !(v <= diameter) => (v + diameter, 1),
        _ => (diameter, 0),
    };
    s.scales_74_78[1] = Some(length);
    s.flag_7c = Some(rect);
    let gap = parameter(s, 4)?;
    s.scalar_c4 = Some(gap);
    let height = match retained(s, h, 0)? {
        Some(v) => 0.5 * v,
        None => h.unit_scale(s)?.unwrap_or(1.),
    };
    s.scalar_c4 = Some(gap + height);
    if let Some(manager) =
        s.managers_4c_50[0].context("Goto refresh profile manager reference is unwritten")?
    {
        let position = vector(s, 0)?;
        let radius = parameter(s, 0)?;
        let mut actor = h.profile_first(s, manager, position, radius)?;
        while let Some(current) = actor {
            obstacle(s, h, current, false)?;
            let manager = s.managers_4c_50[0]
                .flatten()
                .context("Goto profile manager disappeared during iteration")?;
            actor = h.profile_next(s, manager)?;
        }
    } else {
        let mut node = h.actors_first(s)?;
        let mut visited = std::collections::BTreeSet::new();
        while let Some(current) = node {
            anyhow::ensure!(visited.insert(current), "Cycle in Goto actor list");
            let actor = h.actor_at(s, current)?;
            obstacle(s, h, actor, true)?;
            node = h.actors_next(s, current)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ai_goto_gap::{Cache24, Pool};
    use super::*;
    use serde_json::{json, Value};
    const ACTORS: usize = 0x61000000;
    const ATTRS: usize = 0x62000000;
    const LINKS: usize = 0x63000000;
    const PROFILE: usize = 0x64000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn vector(v: &Value) -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(bits(&v[i])))
    }
    struct Replay<'a> {
        input: &'a Value,
        trace: Vec<Value>,
        next: usize,
        spatial: std::collections::BTreeMap<u32, u32>,
    }
    impl Replay<'_> {
        fn actor(&self, actor: usize) -> &Value {
            &self.input["actors"][(actor - ACTORS) / 0x100 - 1]
        }
        fn attr(&mut self, actor: usize, index: usize) -> Option<usize> {
            let metadata = [0x36094, 0x361b4, 0x360bc, 0x36164][index];
            self.trace.push(json!(["attribute", actor, metadata]));
            let value = if actor == ACTORS {
                &self.input["own"][index]
            } else {
                &self.actor(actor)["attrs"][index]
            };
            (!value.is_null()).then_some(ATTRS + (actor - ACTORS) + index * 0x20)
        }
        fn sequence(&self, index: usize) -> Option<usize> {
            self.input["sequence"]
                .get(index)
                .map(|v| ACTORS + bits(v) as usize * 0x100)
        }
    }
    impl super::super::ai_entity_profile_query::Host for Replay<'_> {
        fn read(&mut self, p: u32) -> Result<u32> {
            let actor_count = self.input["actors"].as_array().unwrap().len() + 1;
            if p >= ACTORS as u32 && p < ACTORS as u32 + actor_count as u32 * 0x100 {
                let actor = ((p - ACTORS as u32) / 0x100) as usize;
                let off = (p - ACTORS as u32) % 0x100;
                if [0x34, 0x38, 0x3c].contains(&off) {
                    let v = if actor == 0 {
                        &self.input["position"]
                    } else {
                        &self.input["actors"][actor - 1]["position"]
                    };
                    return Ok(bits(&v[((off - 0x34) / 4) as usize]));
                }
            }
            self.spatial
                .get(&p)
                .copied()
                .with_context(|| format!("Unmapped refresh profile word {p:#x}"))
        }
        fn write(&mut self, p: u32, v: u32) -> Result<()> {
            anyhow::ensure!(
                self.spatial.contains_key(&p),
                "Unmapped refresh profile write {p:#x}"
            );
            self.spatial.insert(p, v);
            Ok(())
        }
        fn time(&mut self) -> Result<f32> {
            self.trace.push(json!(["time"]));
            Ok(f32::from_bits(bits(&self.input["time"])))
        }
    }
    impl ai_goto_cones::Host for Replay<'_> {
        fn torso(&mut self, s: &mut State, actor: usize) -> Result<Option<[f32; 3]>> {
            Ok(self
                .attr(actor, 3)
                .map(|_| vector(&self.actor(actor)["attrs"][3])))
        }
        fn forward(&mut self, _: &mut State, actor: usize) -> Result<[f32; 3]> {
            Ok(vector(&self.actor(actor)["forward"]))
        }
        fn position(&mut self, _: &mut State, actor: usize) -> Result<[f32; 3]> {
            Ok(vector(if actor == ACTORS {
                &self.input["position"]
            } else {
                &self.actor(actor)["position"]
            }))
        }
        fn up(&mut self, _: &mut State) -> Result<[f32; 3]> {
            Ok([0., 1., 0.])
        }
        fn zero(&mut self, _: &mut State) -> Result<[f32; 3]> {
            Ok([0.; 3])
        }
    }
    impl Host for Replay<'_> {
        fn time(&mut self, _: &mut State) -> Result<f32> {
            self.trace.push(json!(["time"]));
            Ok(f32::from_bits(bits(&self.input["time"])))
        }
        fn force_refresh(&mut self, _: &mut State, _: usize) -> Result<u8> {
            Ok(bits(&self.input["force"]) as u8)
        }
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(
                (!self.input["scale"].is_null())
                    .then(|| f32::from_bits(bits(&self.input["scale"]))),
            )
        }
        fn scalar(&mut self, _: &mut State, a: usize) -> Result<f32> {
            let offset = a - ATTRS;
            let actor = offset / 0x100;
            let index = (offset % 0x100) / 0x20;
            Ok(f32::from_bits(bits(if actor == 0 {
                &self.input["own"][index]
            } else {
                &self.input["actors"][actor - 1]["attrs"][index]
            })))
        }
        fn attribute(
            &mut self,
            _: &mut State,
            actor: usize,
            kind: Attribute,
        ) -> Result<Option<usize>> {
            Ok(self.attr(
                actor,
                match kind {
                    Attribute::Height => 0,
                    Attribute::Diameter => 1,
                    Attribute::Length => 2,
                },
            ))
        }
        fn predicate(&mut self, _: &mut State, _: usize, own: usize, other: usize) -> Result<u32> {
            self.trace.push(json!(["predicate", own, other]));
            Ok(if self.input["predicate"] == "actor" {
                bits(&self.actor(other)["result"])
            } else {
                bits(&self.input["predicate"])
            })
        }
        fn profile_first(
            &mut self,
            _: &mut State,
            manager: usize,
            position: [f32; 3],
            radius: f32,
        ) -> Result<Option<usize>> {
            self.trace.push(json!([
                "first",
                manager,
                position.map(f32::to_bits),
                radius.to_bits()
            ]));
            self.next = 1;
            Ok(self.sequence(0))
        }
        fn profile_next(&mut self, _: &mut State, manager: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["next", manager]));
            let result = self.sequence(self.next);
            self.next += 1;
            Ok(result)
        }
        fn actors_first(&mut self, _: &mut State) -> Result<Option<usize>> {
            Ok((!self.input["sequence"].as_array().unwrap().is_empty()).then_some(LINKS))
        }
        fn actor_at(&mut self, _: &mut State, node: usize) -> Result<usize> {
            self.sequence((node - LINKS) / 16)
                .context("invalid actor link")
        }
        fn actors_next(&mut self, _: &mut State, node: usize) -> Result<Option<usize>> {
            let next = (node - LINKS) / 16 + 1;
            Ok(self.sequence(next).map(|_| LINKS + next * 16))
        }
        fn sphere(
            &mut self,
            s: &mut State,
            actor: usize,
            relative: [f32; 3],
            diameter: f32,
        ) -> Result<()> {
            self.trace.push(json!([
                "sphere",
                actor,
                relative.map(f32::to_bits),
                diameter.to_bits()
            ]));
            ai_goto_cones::sphere(s, relative, diameter)
        }
        fn rectangle(
            &mut self,
            s: &mut State,
            actor: usize,
            relative: [f32; 3],
            diameter: f32,
            length: f32,
        ) -> Result<()> {
            self.trace.push(json!([
                "box",
                actor,
                relative.map(f32::to_bits),
                diameter.to_bits(),
                length.to_bits()
            ]));
            ai_goto_cones::rectangle(s, actor, relative, diameter, length, self)
        }
    }
    struct Construction;
    impl super::super::ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(Some(1.))
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            Ok(ACTORS)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            unreachable!()
        }
    }
    #[test]
    fn original_refresh_composes_actual_cones_and_lists() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_refresh.json"
        ))?;
        replay_refresh_fixture(fixture, false)
    }
    #[test]
    fn original_refresh_uses_native_profile_query_and_cones() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_goto_refresh_profiles.json"
        ))?;
        replay_refresh_fixture(fixture, true)
    }
    fn replay_refresh_fixture(fixture: Value, native_profile: bool) -> Result<()> {
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let mut s = State::construct(0x60000000, 0x60001000, b"refresh", &mut Construction)?;
            for (i, key) in [(0, "radius"), (1, "period"), (4, "gap")] {
                s.scalars_10_20[i] = Some(f32::from_bits(bits(&input[key])));
            }
            s.scalars_cc_d0[1] = Some(f32::from_bits(bits(&input["deadline"])));
            s.vectors_5c_80_8c_9c_a8_b4[4] = vector(&input["plane"]).map(Some);
            s.vectors_5c_80_8c_9c_a8_b4[0] = [Some(-17.); 3];
            s.scales_74_78 = [Some(-17.); 2];
            s.scalar_c4 = Some(-17.);
            s.flag_7c = Some(7);
            s.managers_4c_50[0] = Some(input["profile"].as_bool().unwrap().then_some(PROFILE));
            s.reference_54 = Some((!input["predicate"].is_null()).then_some(0x7f000054));
            for i in 0..3 {
                s.attributes_68_6c_70[i] =
                    Some((!input["own"][i].is_null()).then_some(ATTRS + i * 0x20));
            }
            let mut pool = Pool::new(8, Cache24::node)?;
            let active: Vec<usize> = input["active"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| bits(v) as usize)
                .collect();
            let free: Vec<usize> = (0..8).filter(|i| !active.contains(i)).collect();
            pool.active_head = active.first().copied();
            pool.tail = active.last().copied();
            pool.free_head = free.first().copied();
            pool.count = active.len() as u32;
            for (i, n) in pool.nodes.iter_mut().enumerate() {
                for j in 0..4 {
                    n.payload.words_00_14[j + 2] = Some(((i + j) as f32).to_bits());
                }
            }
            for (chain, on) in [(&active, 1), (&free, 0)] {
                for (j, &i) in chain.iter().enumerate() {
                    pool.nodes[i].previous = j.checked_sub(1).map(|j| chain[j]);
                    pool.nodes[i].next = chain.get(j + 1).copied();
                    pool.nodes[i].active = on;
                }
            }
            s.pool_118 = Some(pool);
            let mut h = Replay {
                input,
                trace: vec![],
                next: 0,
                spatial: Default::default(),
            };
            if native_profile {
                const WORLD: u32 = 0x60002000;
                const LIST: u32 = 0x63001000;
                const ROWS: u32 = 0x65000000;
                for (base, size) in [
                    (WORLD, 0x20),
                    (LIST, 0x20),
                    (PROFILE as u32, 0x80),
                    (ROWS, 8 * 12),
                    (LINKS as u32, 8 * 20),
                    (0x5a108, 4),
                ] {
                    for offset in (0..size).step_by(4) {
                        h.spatial.insert(base + offset, 0);
                    }
                }
                for (p, v) in [
                    (0x5a108, WORLD),
                    (WORLD + 0x14, 8),
                    (WORLD + 0x18, LIST),
                    (LIST + 4, LINKS as u32),
                    (
                        LIST + 8,
                        if h.sequence(0).is_some() {
                            LINKS as u32
                        } else {
                            0
                        },
                    ),
                    (PROFILE as u32 + 4, 2f32.to_bits()),
                    (PROFILE as u32 + 8, 0.5f32.to_bits()),
                    (PROFILE as u32 + 12, ROWS),
                    (PROFILE as u32 + 0x10, (-1f32).to_bits()),
                ] {
                    h.spatial.insert(p, v);
                }
                for i in 0..input["sequence"].as_array().unwrap().len() {
                    let actor = h.sequence(i).unwrap() as u32;
                    let node = LINKS as u32 + i as u32 * 20;
                    h.spatial.insert(node + 4, actor);
                    h.spatial.insert(
                        node + 12,
                        if h.sequence(i + 1).is_some() {
                            node + 20
                        } else {
                            0
                        },
                    );
                    h.spatial.insert(actor + 0x14, node);
                }
                let mut profiles = super::super::ai_goto_refresh_profiles::NativeProfiles {
                    host: &mut h,
                    context: super::super::ai_entity_profile_query::Context { world: 0x5a108 },
                };
                refresh(&mut s, &mut profiles)?;
                let words: Vec<u32> = [0x10]
                    .into_iter()
                    .chain((0x18..0x50).step_by(4))
                    .map(|off| h.spatial[&(PROFILE as u32 + off)])
                    .collect();
                assert_eq!(json!(words), case["profile_words"], "profile words {index}");
                let rows: Vec<u32> = (0..24).map(|i| h.spatial[&(ROWS + i * 4)]).collect();
                assert_eq!(json!(rows), case["profile_rows"], "profile rows {index}");
            } else {
                refresh(&mut s, &mut h)?;
            }
            let state = json!({"deadline":s.scalars_cc_d0[1].unwrap().to_bits(),"position":s.vectors_5c_80_8c_9c_a8_b4[0].map(|v|v.unwrap().to_bits()),"diameter":s.scales_74_78[0].unwrap().to_bits(),"length":s.scales_74_78[1].unwrap().to_bits(),"height":s.scalar_c4.unwrap().to_bits(),"rect":s.flag_7c.unwrap()});
            assert_eq!(state, case["state"], "refresh state {index}");
            let pool = s.pool_118.as_ref().unwrap();
            let image = json!({"head":pool.active_head,"tail":pool.tail,"free":pool.free_head,"count":pool.count,"nodes":pool.nodes.iter().map(|n|json!({"words":n.payload.words_00_14.map(|v|v.unwrap()),"previous":n.previous,"next":n.next,"active":n.active})).collect::<Vec<_>>()});
            assert_eq!(image, case["pool"], "refresh pool {index}");
            assert_eq!(json!(h.trace), case["trace"], "refresh trace {index}");
        }
        Ok(())
    }
}
