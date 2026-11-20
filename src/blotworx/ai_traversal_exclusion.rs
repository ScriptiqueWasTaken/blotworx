//! Traversal exclusion484c80/4860ec, actor lookup486520, bit test458b50,
//! and concrete world-service lookup46c91c. Only class20 is a virtual boundary.
use super::ai_graph_link_passes::Memory;
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
fn word(m: &Memory, b: u32, o: u32) -> Result<u32> {
    m.word(b.wrapping_add(o))
}
#[derive(Clone, Copy)]
pub struct World {
    pub root_slot: u32,
    pub service_class: u32,
}
pub trait Services {
    fn class20(&mut self, m: &mut Memory, service: u32) -> Result<u32>;
}
pub fn find_service(
    m: &mut Memory,
    root: u32,
    target: u32,
    host: &mut impl Services,
) -> Result<u32> {
    let mut index = 0u32;
    while index < word(m, root, 0x38)? {
        let table = word(m, root, 0x34)?;
        let service = word(m, table, index.wrapping_mul(4))?;
        if service != 0 {
            let mut class = host.class20(m, service)?;
            let mut visited = BTreeSet::new();
            loop {
                if class == target {
                    return word(m, word(m, root, 0x34)?, index.wrapping_mul(4));
                }
                ensure!(
                    visited.insert(class),
                    "Cyclic traversal world-service ancestry"
                );
                class = word(m, class, 0x14)?;
                if class == 0 {
                    break;
                }
            }
        }
        index = index.wrapping_add(1);
    }
    Ok(0)
}
pub fn actor_index(m: &Memory, root_slot: u32, manager: u32, actor: u32) -> Result<u32> {
    let root = m.word(root_slot)?;
    let count = word(m, root, 0x14)?;
    for index in 0..count {
        let table = word(m, manager, 0x14)?;
        if word(m, table, index.wrapping_mul(4))? == actor {
            return Ok(index);
        }
    }
    Ok(u32::MAX)
}
pub fn bit(m: &Memory, bitset: u32, index: u32) -> Result<u32> {
    let data = m.word(bitset)?;
    let byte = m.byte(data.wrapping_add(index >> 3))?;
    Ok(((byte >> (7 - (index & 7))) & 1) as u32)
}
///458b04, with the same MSB-first bit numbering as458b50.
pub fn set_bit(m:&mut Memory,bitset:u32,index:u32,enabled:bool)->Result<()> {
    let data=m.word(bitset)?;let address=data.wrapping_add(index>>3);
    let mask=1u8<<(7-(index&7));let old=m.byte(address)?;
    m.write_byte(address,if enabled{old|mask}else{old&!mask})
}
pub fn query(m: &Memory, root_slot: u32, manager: u32, interface: u32, actor: u32) -> Result<u32> {
    let header = word(m, manager, 0x18)?;
    if header == 0 {
        return Ok(0);
    }
    let count = word(m, header, 8)?;
    for graph_index in 0..count {
        let graph = if graph_index < word(m, header, 8)? {
            word(m, header, 4)?.wrapping_add(graph_index.wrapping_mul(0x8630))
        } else {
            0
        };
        if graph == 0 || word(m, graph, 0x2d4)? != 6 {
            continue;
        }
        let slots = word(m, graph, 0x2c0)?
            .wrapping_add(word(m, manager, 4)?)
            .wrapping_sub(0x4000);
        for index in 0..slots {
            let table = word(m, graph, 0x2cc)?;
            if word(m, table, index.wrapping_mul(4))? != interface {
                continue;
            }
            let metadata = word(m, graph, 0x2c4)?.wrapping_add(index.wrapping_mul(8));
            let flags = u16::from_be_bytes([
                m.byte(metadata.wrapping_add(4))?,
                m.byte(metadata.wrapping_add(5))?,
            ]);
            let state = word(m, graph, 0x2c8)?.wrapping_add(index.wrapping_mul(12));
            if flags & 6 == 0 {
                return Ok(u32::from(m.byte(state.wrapping_add(8))? == 0));
            }
            if m.word(state)? == 0 {
                return Ok(1);
            }
            let actor_index = actor_index(m, root_slot, manager, actor)?;
            if actor_index == u32::MAX {
                return Ok(1);
            }
            let state = word(m, graph, 0x2c8)?.wrapping_add(index.wrapping_mul(12));
            return Ok(u32::from(bit(m, m.word(state)?, actor_index)? == 0));
        }
    }
    Ok(1)
}
pub fn cached(
    m: &mut Memory,
    world: World,
    interface: u32,
    actor: u32,
    host: &mut impl Services,
) -> Result<u32> {
    if word(m, interface, 0x90)? == 0 {
        let root = m.word(world.root_slot)?;
        let manager = find_service(m, root, world.service_class, host)?;
        m.write(interface.wrapping_add(0x90), manager)?;
    }
    query(
        m,
        world.root_slot,
        word(m, interface, 0x90)?,
        interface,
        actor,
    )
}

#[cfg(test)]
mod tests {
    use super::super::{ai_segment_query::WalkPoint, ai_traversal_cost as cost};
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    const ROOT: u32 = 0x69000000;
    const INTERFACE: u32 = 0x73000000;
    const MANAGER: u32 = 0x74000000;
    const OTHER: u32 = 0x74100000;
    const HEADER: u32 = 0x75000000;
    const GRAPHS: u32 = 0x76000000;
    const META: u32 = 0x77000000;
    const CLASS: u32 = 0x77f00000;
    const STATES: u32 = 0x78000000;
    const REFS: u32 = 0x79000000;
    const ACTORS: u32 = 0x7a000000;
    const BITS: u32 = 0x7b000000;
    const BITSETS: u32 = 0x7c000000;
    const SERVICES: u32 = 0x7d000000;
    const ALT: u32 = SERVICES + 0x1000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn world() -> World {
        World {
            root_slot: 0x5a108,
            service_class: 0x6af44,
        }
    }
    struct Spy<'a> {
        case: &'a Value,
        trace: Vec<Value>,
    }
    impl Services for Spy<'_> {
        fn class20(&mut self, m: &mut Memory, service: u32) -> Result<u32> {
            self.trace.push(json!(["class", service]));
            if service == OTHER {
                if self.case["mutation"] == "count" {
                    m.write(ROOT + 0x38, 0)?;
                }
                Ok(CLASS)
            } else {
                if self.case["mutation"] == "slot" {
                    m.write(SERVICES + 8, OTHER)?;
                }
                if self.case["mutation"] == "array" {
                    m.write(ROOT + 0x34, ALT)?;
                }
                Ok(match bits(&self.case["class_kind"]) {
                    0 => 0x6af44,
                    1 => CLASS + 0x100,
                    _ => CLASS,
                })
            }
        }
    }
    // Exercise the production default exclusion hook, not a replacement for it.
    impl cost::Host for Spy<'_> {
        fn exclusion_world(&self) -> World {
            world()
        }
        fn class18(&mut self, _: &mut Memory, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected cost class query")
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
            anyhow::bail!("Unexpected cost spatial query")
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
            anyhow::bail!("Unexpected cost contribution")
        }
    }
    fn memory(c: &Value) -> Result<Memory> {
        let mut m = Memory {
            regions: BTreeMap::new(),
            resources: BTreeMap::new(),
            stride: 32,
            journal: BTreeMap::new(),
        };
        for (at, size) in [
            (ROOT, 0x40),
            (0x5a108, 4),
            (INTERFACE, 0x94),
            (MANAGER, 0x1c),
            (OTHER, 0x1c),
            (HEADER, 12),
            (CLASS, 0x18),
            (CLASS + 0x100, 0x18),
            (SERVICES, 12),
            (ALT, 12),
        ] {
            m.regions.insert(at, vec![0; size]);
        }
        let graphs = c["graphs"].as_array().unwrap();
        let actors = c["actors"].as_array().unwrap();
        m.regions
            .insert(GRAPHS, vec![0; graphs.len().max(1) * 0x8630]);
        m.regions.insert(ACTORS, vec![0; actors.len().max(1) * 4]);
        for (at, v) in [
            (0x5a108, if c["root"] == true { ROOT } else { 0 }),
            (
                ROOT + 0x14,
                c["actor_count"].as_u64().unwrap_or(actors.len() as u64) as u32,
            ),
            (ROOT + 0x34, SERVICES),
            (ROOT + 0x38, 3),
            (CLASS + 0x100 + 0x14, 0x6af44),
            (
                INTERFACE + 0x90,
                if c["cached"] == true { MANAGER } else { 0 },
            ),
            (HEADER + 4, GRAPHS),
            (
                HEADER + 8,
                c["graph_count"].as_u64().unwrap_or(graphs.len() as u64) as u32,
            ),
        ] {
            m.write(at, v)?;
        }
        for (i, v) in [0, OTHER, MANAGER].into_iter().enumerate() {
            m.write(SERVICES + i as u32 * 4, v)?;
            m.write(ALT + i as u32 * 4, if i == 0 { 0 } else { OTHER })?;
        }
        for manager in [MANAGER, OTHER] {
            for (at, v) in [
                (manager, 0x65003000),
                (
                    manager + 4,
                    (0x4000i64 + c["bias"].as_i64().unwrap()) as u32,
                ),
                (manager + 0x14, ACTORS),
                (manager + 0x18, if c["header"] == true { HEADER } else { 0 }),
            ] {
                m.write(at, v)?;
            }
        }
        for (i, v) in actors.iter().enumerate() {
            m.write(ACTORS + i as u32 * 4, bits(v))?;
        }
        for (g, graph) in graphs.iter().enumerate() {
            let at = GRAPHS + g as u32 * 0x8630;
            let off = g as u32 * 0x1000;
            let entries = graph["entries"].as_array().unwrap();
            for (base, size) in [
                (REFS + off, (entries.len() + 1) * 4),
                (META + off, entries.len().max(1) * 8),
                (STATES + off, entries.len().max(1) * 12),
            ] {
                m.regions.insert(base, vec![0; size]);
            }
            for (p, v) in [
                (at + 0x2d4, bits(&graph["phase"])),
                (
                    at + 0x2c0,
                    graph["declared"].as_u64().unwrap_or(entries.len() as u64) as u32,
                ),
                (at + 0x2c4, META + off),
                (at + 0x2c8, STATES + off),
                (at + 0x2cc, REFS + off),
            ] {
                m.write(p, v)?;
            }
            for (i, e) in entries.iter().enumerate() {
                let i = i as u32;
                let mask = e["mask"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| bits(v) as u8)
                    .collect();
                m.regions.insert(BITS + off + i * 0x10, mask);
                m.regions.insert(BITSETS + off + i * 0x10, vec![0; 4]);
                m.write(REFS + off + i * 4, bits(&e["interface"]))?;
                let flags = (bits(&e["flags"]) as u16).to_be_bytes();
                for j in 0..2 {
                    m.write_byte(META + off + i * 8 + 4 + j as u32, flags[j])?;
                }
                m.write(
                    STATES + off + i * 12,
                    if e["bitset"] == true {
                        BITSETS + off + i * 0x10
                    } else {
                        0
                    },
                )?;
                m.write_byte(STATES + off + i * 12 + 8, bits(&e["enabled"]) as u8)?;
                m.write(BITSETS + off + i * 0x10, BITS + off + i * 0x10)?;
            }
        }
        if c["op"] == "bit" {
            m.regions.insert(
                BITS,
                c["mask"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| bits(v) as u8)
                    .collect(),
            );
            m.write(BITSETS, BITS)?;
        }
        Ok(m)
    }
    #[test]
    fn original_exclusion_cache_services_graph_permissions_actor_indices_and_bits_match(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_traversal_exclusion.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let mut m = memory(c)?;
            let mut host = Spy {
                case: c,
                trace: vec![],
            };
            let actor = bits(&c["actor"]);
            let result = match c["op"].as_str().unwrap() {
                "cached" => cost::Host::excluded484c80(&mut host, &mut m, INTERFACE, actor)?,
                "query" => query(&m, 0x5a108, MANAGER, INTERFACE, actor)?,
                "actor" => actor_index(&m, 0x5a108, MANAGER, actor)?,
                "bit" => bit(&m, BITSETS, bits(&c["bit_index"]))?,
                "find" => find_service(&mut m, ROOT, 0x6af44, &mut host)?,
                _ => unreachable!(),
            };
            let slots = (0..3)
                .map(|i| m.word(SERVICES + i * 4))
                .collect::<Result<Vec<_>>>()?;
            let actual = json!({"result":result,"trace":host.trace,"cache":m.word(INTERFACE+0x90)?,"service_count":m.word(ROOT+0x38)?,"service_array":m.word(ROOT+0x34)?,"slots":slots});
            for key in actual.as_object().unwrap().keys() {
                assert_eq!(actual[key], expected[key], "{key}: {c}");
            }
        }
        Ok(())
    }
    #[test]
    fn corrupt_exclusion_storage_and_missing_required_manager_report_errors() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_traversal_exclusion.json"
        ))?;
        let mut c = fixture["cases"][0]["input"].clone();
        c["class_kind"] = json!(2);
        let mut m = memory(&c)?;
        let mut host = Spy {
            case: &c,
            trace: vec![],
        };
        assert!(cost::Host::excluded484c80(&mut host, &mut m, INTERFACE, 900).is_err());
        let mut m = memory(&c)?;
        m.write(CLASS + 0x14, CLASS)?;
        assert!(find_service(&mut m, ROOT, 0x6af44, &mut host).is_err());
        let mut m = memory(&c)?;
        m.regions.remove(&GRAPHS);
        assert!(query(&m, 0x5a108, MANAGER, INTERFACE, 900).is_err());
        let m = memory(&c)?;
        assert!(bit(&m, BITSETS, 64).is_err());
        Ok(())
    }
}
