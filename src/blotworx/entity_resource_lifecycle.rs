//! Resource publication tail REL1e7ff4..1e807c and retirement1e8098.
//! Resource lookup/type validation precede this entry and remain the caller's
//! responsibility. Uses the same paired registry and retain/release operations
//! as navigation collection; this is not yet normal scene-loader integration.
use anyhow::{Context, Result};
use super::{ai_graph_link_passes::Memory, ai_navigation_refresh_sources::{self, Lifecycle},
    entity_handle_runtime::{Allocator, Registry}, entity_component_lookup::Storage};

pub trait Host<A: Allocator>: Lifecycle<A> {
    fn update_80443a10(&mut self, m: &mut Memory, resource: u32, time: f32, flag: bool) -> Result<()>;
    fn resource_virtual_4c(&mut self, m: &mut Memory, resource: u32) -> Result<()>;
    fn postprocess_80443df8(&mut self, m: &mut Memory, resource: u32) -> Result<()>;
    fn postprocess_804445a8(&mut self, m: &mut Memory, resource: u32) -> Result<()>;
}

/// `resource` is the validated object, not a newly allocated scene or actor.
/// Native failure clears the output. Publication is deliberately last, and
/// errors do not roll back side effects already performed by source operations.
pub fn publish<A: Allocator>(m: &mut Memory, output: u32, resource: Option<u32>,
    handles: &mut Registry<A>, host: &mut impl Host<A>) -> Result<()> {
    let Some(resource) = resource.filter(|&p| p != 0) else { return m.write(output, 0) };
    ai_navigation_refresh_sources::retain::<A>(m, resource, host)?;
    host.update_80443a10(m, resource, 0.0, true)?;
    host.resource_virtual_4c(m, resource)?;
    host.postprocess_80443df8(m, resource)?;
    host.postprocess_804445a8(m, resource)?;
    handles.register(resource)?;
    let handle = handles.handle(resource)?;
    m.write(output, handle)
}

/// Source tests the holder, resolves it, unpublishes the current reverse
/// registration, then releases the resource. It does not clear the holder.
pub fn retire<A: Allocator>(m: &mut Memory, holder: u32, handles: &mut Registry<A>,
    components: &mut Storage, host: &mut impl Lifecycle<A>) -> Result<()> {
    let handle = m.word(holder)?;
    if handle == 0 { return Ok(()) }
    let resource = handles.entity(handle)?.filter(|&p| p != 0)
        .context("Resource retirement references a stale native handle")?;
    let resource = u32::try_from(resource)?;
    handles.remove(resource)?;
    ai_navigation_refresh_sources::release(m, resource, handles, components, host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    const R: u32 = 0x72000000;
    const O: u32 = 0x72000100;
    struct Nodes(usize);
    impl Allocator for Nodes {
        fn node(&mut self, _: super::super::entity_handle_runtime::Direction,
            _: &mut super::super::entity_handle_lookup::Table, _: &mut u32) -> Result<usize> {
            self.0 += 1; Ok(self.0)
        }
    }
    struct Hooks { enter: u32, trace: Vec<Value> }
    impl Lifecycle<Nodes> for Hooks {
        fn enter(&mut self, _: &mut Memory) -> Result<u32> {
            self.trace.push(json!(["enter", self.enter])); Ok(self.enter)
        }
        fn leave(&mut self, _: &mut Memory, enabled: bool) -> Result<()> {
            self.trace.push(json!(["leave", u8::from(enabled)])); Ok(())
        }
        fn destroy_group(&mut self, _: &mut Memory, r: u32, handles: &mut Registry<Nodes>,
            _: &mut Storage) -> Result<()> {
            // Unpublication must already have happened when deletion executes.
            assert_eq!(handles.entity(1)?, None);
            assert_eq!(handles.handle(r)?, 0);
            self.trace.push(json!(["0x7f000010", r])); Ok(())
        }
    }
    impl Host<Nodes> for Hooks {
        fn update_80443a10(&mut self, _: &mut Memory, r: u32, t: f32, b: bool) -> Result<()> {
            assert_eq!(t.to_bits(), 0); assert!(b);
            self.trace.push(json!(["0x80443a10", r])); Ok(())
        }
        fn resource_virtual_4c(&mut self, _: &mut Memory, r: u32) -> Result<()> {
            self.trace.push(json!(["0x7f000014", r])); Ok(())
        }
        fn postprocess_80443df8(&mut self, _: &mut Memory, r: u32) -> Result<()> {
            self.trace.push(json!(["0x80443df8", r])); Ok(())
        }
        fn postprocess_804445a8(&mut self, _: &mut Memory, r: u32) -> Result<()> {
            self.trace.push(json!(["0x804445a8", r])); Ok(())
        }
    }
    fn bytes(s: &str) -> Vec<u8> {
        s.as_bytes().chunks_exact(2).map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap()).collect()
    }
    #[test]
    fn resource_publication_and_retirement_match_original_order_and_storage() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!("../../tools/blotworx/fixtures/entity_resource_lifecycle.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let c = &row["input"]; let mut m = Memory::default();
            m.regions.insert(R, bytes(row["initial"][0].as_str().unwrap()));
            m.regions.insert(O, bytes(row["initial"][1].as_str().unwrap()));
            let mut handles = Registry::initialized(0, Nodes(0))?;
            let mut components = Storage { entities: Default::default(), metadata: Default::default() };
            let mut host = Hooks { enter: c["enter"].as_u64().unwrap() as u32, trace: vec![] };
            let present = c["present"].as_bool().unwrap();
            if c["mode"] == "publish" {
                publish(&mut m, O, present.then_some(R), &mut handles, &mut host)?;
            } else {
                if present { handles.register(R)? }
                retire(&mut m, O, &mut handles, &mut components, &mut host)?;
            }
            assert_eq!(json!(host.trace), row["trace"], "{c}");
            for (i, p) in [R, O].into_iter().enumerate() {
                assert_eq!(m.regions[&p], bytes(row["final"][i].as_str().unwrap()), "{c} at {p:x}");
            }
            assert_eq!(handles.entity(1)?.is_some(), row["registered"].as_bool().unwrap(), "{c}");
            assert_eq!(handles.handle(R)?, if row["registered"] == true { 1 } else { 0 }, "{c}");
        }
        Ok(())
    }
}
