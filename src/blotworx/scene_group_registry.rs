//! Scene manager's actual NiTStringMap at+54: insertion180ee4 and
//! assignment29f548. The navigation collector traverses this same memory.
//! Heap allocation/free and refcount critical/destruction services are required.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use super::{ai_graph_link_passes::Memory, ai_navigation_refresh_sources::{self, Lifecycle},
    entity_component_lookup::Storage, entity_handle_runtime::{Allocator, Registry}};

pub trait Host<A: Allocator>: Lifecycle<A> {
    /// Native allocation must map exactly the returned storage in `memory`.
    fn allocate(&mut self, memory: &mut Memory, bytes: u32) -> Result<u32>;
    /// Distinguishes805005c4 key buffers from804ffc94 sized node deletion.
    fn free(&mut self, memory: &mut Memory, address: u32, bytes: Option<u32>) -> Result<()>;
}
fn read(m: &Memory, p: u32, o: u32) -> Result<u32> { m.word(p.wrapping_add(o)) }
fn write(m: &mut Memory, p: u32, o: u32, v: u32) -> Result<()> { m.write(p.wrapping_add(o), v) }
fn name(m: &Memory, mut p: u32) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    loop { let b = m.byte(p)?; if b == 0 { return Ok(result) }
        result.try_reserve(1)?; result.push(b); p = p.checked_add(1).context("Scene key address overflow")?; }
}
fn hash(s: &[u8]) -> u32 { s.iter().fold(0u32, |h, &b| h.wrapping_mul(33).wrapping_add((b as i8 as i32) as u32)) }

/// Native17f5d0 creates37 buckets and enables key copies. The enclosing
///manager must already own mapped storage; no actors or handles are invented.
pub fn initialize<A: Allocator>(m: &mut Memory, manager: u32, copy_keys: bool, h: &mut impl Host<A>) -> Result<()> {
    let table = manager.wrapping_add(0x54);
    write(m, table, 4, 37)?; write(m, table, 0, 0x4ec91c)?; write(m, table, 12, 0)?;
    let buckets = h.allocate(m, 37 * 4)?;
    write(m, table, 8, buckets)?; m.fill(buckets, 37 * 4, 0)?;
    m.write_byte(table.wrapping_add(16), u8::from(copy_keys))?;
    // Constructor's intermediate string-map vtable, then derived final table.
    write(m, table, 0, 0x4ec8e4)?; write(m, table, 0, 0x4ec8c8)
}

fn assign<A: Allocator>(m: &mut Memory, node: u32, holder: u32,
    handles: &mut Registry<A>, components: &mut Storage, h: &mut impl Host<A>) -> Result<()> {
    let old = read(m, node, 8)?;
    if old == m.word(holder)? { return Ok(()) }
    if old != 0 { ai_navigation_refresh_sources::release(m, old, handles, components, h)? }
    // Destruction may change the source holder. Do not snapshot the replacement.
    let new = m.word(holder)?; write(m, node, 8, new)?;
    if new != 0 { ai_navigation_refresh_sources::retain::<A>(m, new, h)? }
    Ok(())
}

pub fn insert<A: Allocator>(m: &mut Memory, manager: u32, key: u32, holder: u32,
    handles: &mut Registry<A>, components: &mut Storage, h: &mut impl Host<A>) -> Result<()> {
    let table = manager.wrapping_add(0x54);
    let count = read(m, table, 4)?; ensure!(count != 0, "Scene group registry has zero buckets");
    let offset = (hash(&name(m, key)?) % count).wrapping_mul(4);
    let mut node = read(m, read(m, table, 8)?, offset)?;
    let mut seen = BTreeSet::new();
    while node != 0 {
        ensure!(seen.insert(node), "Cyclic scene group registry");
        if name(m, key)? == name(m, read(m, node, 4)?)? {
            return assign(m, node, holder, handles, components, h)
        }
        node = read(m, node, 0)?;
    }
    //29f164 initializes all three words after allocation, before reading holder.
    let node = h.allocate(m, 12)?;
    write(m, node, 8, 0)?; write(m, node, 0, 0)?; write(m, node, 4, 0)?;
    let resource = m.word(holder)?;
    if resource != 0 { ai_navigation_refresh_sources::retain::<A>(m, resource, h)? }
    //29f548 copies the key if requested. Both strlen reads surround allocation.
    if m.byte(table.wrapping_add(16))? != 0 {
        let size = u32::try_from(name(m, key)?.len())?.checked_add(1).context("Scene key length overflow")?;
        let copy = h.allocate(m, size)?; write(m, node, 4, copy)?;
        let current = name(m, key)?;
        let length = current.len().saturating_add(1).min(size as usize);
        for i in 0..length { m.write_byte(copy.wrapping_add(i as u32), current.get(i).copied().unwrap_or(0))? }
        m.write_byte(copy.wrapping_add(size - 1), 0)?;
    } else { write(m, node, 4, key)? }
    // The temporary retained holder is independent of later caller mutations.
    let old = read(m, node, 8)?;
    if old != resource {
        if old != 0 { ai_navigation_refresh_sources::release(m, old, handles, components, h)? }
        write(m, node, 8, resource)?;
        if resource != 0 { ai_navigation_refresh_sources::retain::<A>(m, resource, h)? }
    }
    if resource != 0 { ai_navigation_refresh_sources::release(m, resource, handles, components, h)? }
    let buckets = read(m, table, 8)?; let head = read(m, buckets, offset)?;
    write(m, node, 0, head)?; write(m, buckets, offset, node)?;
    let size = read(m, table, 12)?; write(m, table, 12, size.wrapping_add(1))
}

/// Map-erasure body1802f8..180428 after scene-name derivation. Unlinks before
/// callbacks, frees an owned key before the group, then decrements live size.
/// The surrounding actor/scene shutdown is a separate required lifecycle step.
pub fn remove<A: Allocator>(m: &mut Memory, manager: u32, key: u32,
    handles: &mut Registry<A>, components: &mut Storage, h: &mut impl Host<A>) -> Result<bool> {
    let table = manager.wrapping_add(0x54);
    let count = read(m, table, 4)?; ensure!(count != 0, "Scene group registry has zero buckets");
    let offset = (hash(&name(m, key)?) % count).wrapping_mul(4);
    let buckets = read(m, table, 8)?;
    let mut node = read(m, buckets, offset)?;
    let mut previous = None;
    let mut seen = BTreeSet::new();
    while node != 0 {
        ensure!(seen.insert(node), "Cyclic scene group registry");
        if name(m, key)? == name(m, read(m, node, 4)?)? {
            let next = read(m, node, 0)?;
            if let Some(previous) = previous { write(m, previous, 0, next)? }
            else { write(m, buckets, offset, next)? }
            if m.byte(table.wrapping_add(16))? != 0 {
                h.free(m, read(m, node, 4)?, None)?;
            }
            //29f0a0 reads the payload after key deletion, not before it.
            let group = read(m, node, 8)?;
            if group != 0 {
                ai_navigation_refresh_sources::release(m, group, handles, components, h)?;
                write(m, node, 8, 0)?;
            }
            write(m, node, 4, 0)?;
            h.free(m, node, Some(12))?;
            let size = read(m, table, 12)?;
            write(m, table, 12, size.wrapping_sub(1))?;
            return Ok(true)
        }
        previous = Some(node); node = read(m, node, 0)?;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Nodes(usize);
    impl Allocator for Nodes {
        fn node(&mut self, _: super::super::entity_handle_runtime::Direction,
            _: &mut super::super::entity_handle_lookup::Table, _: &mut u32) -> Result<usize> {
            self.0 += 1; Ok(self.0)
        }
    }
    struct Hooks { cursor: u32, enter: u32, mutate: bool, trace: Vec<Value> }
    impl Host<Nodes> for Hooks {
        fn allocate(&mut self, m: &mut Memory, n: u32) -> Result<u32> {
            let p = self.cursor; self.cursor += n + 0x100;
            m.regions.insert(p, vec![0xa5; n as usize]);
            self.trace.push(json!(["allocate", n, p])); Ok(p)
        }
        fn free(&mut self, _: &mut Memory, p: u32, n: Option<u32>) -> Result<()> {
            self.trace.push(json!(["free", p, n])); Ok(())
        }
    }
    impl Lifecycle<Nodes> for Hooks {
        fn enter(&mut self, _: &mut Memory) -> Result<u32> {
            self.trace.push(json!(["enter", self.enter])); Ok(self.enter)
        }
        fn leave(&mut self, _: &mut Memory, enabled: bool) -> Result<()> {
            self.trace.push(json!(["leave", u8::from(enabled)])); Ok(())
        }
        fn destroy_group(&mut self, m: &mut Memory, group: u32, _: &mut Registry<Nodes>, _: &mut Storage) -> Result<()> {
            self.trace.push(json!(["destroy", group]));
            if self.mutate { m.write(0x72001000, 0x74000300)? }
            Ok(())
        }
    }
    fn bytes(s: &str) -> Vec<u8> {
        s.as_bytes().chunks_exact(2).map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap()).collect()
    }
    #[test]
    fn original_scene_registration_replacement_and_navigation_collection_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!("../../tools/blotworx/fixtures/scene_group_registry.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let c = &row["input"]; let mut m = Memory::default();
            for span in row["initial"].as_array().unwrap() {
                m.regions.insert(span[0].as_u64().unwrap() as u32, bytes(span[1].as_str().unwrap()));
            }
            let mut handles = Registry::initialized(0, Nodes(0))?;
            for actor in row["actors"].as_array().unwrap() { handles.register(actor.as_u64().unwrap() as u32)? }
            let mut components = Storage { entities: Default::default(), metadata: Default::default() };
            for d in row["components"].as_array().unwrap() {
                components.entities.insert(d["base"].as_u64().unwrap() as u32, super::super::entity_component_lookup::Entity {
                    metadata: 0, slots: Some(d["slots"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect()),
                    interfaces: d["interfaces"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect(),
                });
            }
            let mut host = Hooks { cursor: 0x76000000, enter: c["enter"].as_u64().unwrap() as u32,
                mutate: c["mutate"].as_bool().unwrap_or(false), trace: vec![] };
            initialize(&mut m, 0x72000000, c["copy"] == 1, &mut host)?;
            for op in row["operations"].as_array().unwrap() {
                m.write(0x72001000, op[1].as_u64().unwrap() as u32)?;
                insert(&mut m, 0x72000000, op[0].as_u64().unwrap() as u32, 0x72001000,
                    &mut handles, &mut components, &mut host)?;
            }
            for key in row["removals"].as_array().unwrap() {
                remove(&mut m, 0x72000000, key.as_u64().unwrap() as u32,
                    &mut handles, &mut components, &mut host)?;
            }
            let output = ai_navigation_refresh_sources::collect(&mut m, 0x72000000,
                super::super::ai_navigation_refresh::Pass::Actors, &mut handles, &mut components, &mut host)?;
            assert_eq!(json!(output), row["output"], "navigation order {c}");
            assert_eq!(json!(host.trace), row["trace"], "callback order {c}");
            for span in row["final"].as_array().unwrap() {
                let p = span[0].as_u64().unwrap() as u32;
                assert_eq!(m.regions[&p], bytes(span[1].as_str().unwrap()), "{c} at {p:x}");
            }
        }
        Ok(())
    }
}
