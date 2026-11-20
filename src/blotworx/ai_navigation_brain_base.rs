//! Base navigation-brain configuration459080..459428. This is not the
//! gameplay AIBrainComponent or a successfully constructed World factory.
use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Service,
    Agent,
}
#[derive(Debug, serde::Serialize)]
pub struct State {
    pub owner: Option<usize>,  //14: navigation entity
    pub action: Option<usize>, //18: constructed ActionClass instance
    pub services: Vec<usize>,  //c/10
    pub agents: Vec<usize>,    //4/8
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    fn label(kind: Kind) -> &'static str {
        match kind {
            Kind::Service => "Service",
            Kind::Agent => "Agent",
        }
    }
    struct Binding {
        rows: Vec<Value>,
        trace: Vec<Value>,
        active: usize,
        next: usize,
        instances: BTreeMap<usize, (Kind, usize)>,
    }
    impl Host for Binding {
        fn child_count(&mut self, _: &mut State, root: usize) -> Result<u32> {
            assert_eq!(root, 1);
            self.trace.push(json!(["count"]));
            Ok(self.rows.len() as u32)
        }
        fn child(&mut self, _: &mut State, root: usize, index: u32) -> Result<usize> {
            assert_eq!(root, 1);
            self.trace.push(json!(["child", index]));
            Ok(index as usize)
        }
        fn name(&mut self, _: &mut State, node: usize) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["name", node]));
            Ok(Some(
                self.rows[node]["name"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
            ))
        }
        fn class_name(&mut self, _: &mut State, node: usize) -> Result<Option<Vec<u8>>> {
            self.active = node;
            self.trace.push(json!(["class_name", node]));
            Ok(self.rows[node]["class"]
                .as_str()
                .map(|s| s.as_bytes().to_vec()))
        }
        fn class_metadata(
            &mut self,
            _: &mut State,
            kind: Kind,
            name: &[u8],
        ) -> Result<Option<usize>> {
            self.trace.push(json!([
                "metadata",
                label(kind),
                String::from_utf8_lossy(name)
            ]));
            Ok(self.rows[self.active]["known"]
                .as_bool()
                .unwrap_or(true)
                .then_some(50 + self.active))
        }
        fn class_factory(&mut self, _: &mut State, metadata: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["factory", metadata]));
            let row = &self.rows[metadata - 50];
            Ok(row["factory"].as_bool().unwrap_or(true).then_some(
                if super::super::ai_traversal_registry::names_equal(
                    row["name"].as_str().unwrap().as_bytes(),
                    b"Service",
                ) {
                    1000
                } else {
                    1001
                },
            ))
        }
        fn construct(&mut self, _: &mut State, kind: Kind, factory: usize) -> Result<usize> {
            self.trace
                .push(json!(["construct", label(kind), factory, 1]));
            let id = self.next;
            self.next += 1;
            self.instances.insert(id, (kind, self.active));
            Ok(id)
        }
        fn initialize(
            &mut self,
            state: &mut State,
            kind: Kind,
            instance: usize,
            node: usize,
        ) -> Result<bool> {
            assert_eq!(self.instances[&instance], (kind, node));
            self.trace
                .push(json!(["initialize", label(kind), instance, node]));
            let row = &self.rows[node];
            match row["mutation"].as_str() {
                Some("service") => state.services.push(777),
                Some("agent") => state.agents.push(777),
                _ => {}
            }
            let result = row["initialize"].as_bool().unwrap_or(true);
            if row["append"] == true {
                self.rows.push(json!({"name":"AddedAttribute"}));
            }
            Ok(result)
        }
        fn destroy(&mut self, _: &mut State, kind: Kind, instance: usize) -> Result<()> {
            assert_eq!(self.instances[&instance].0, kind);
            self.instances.remove(&instance);
            self.trace.push(json!(["destroy", label(kind), instance]));
            Ok(())
        }
        fn attribute(&mut self, _: &mut State, node: usize) -> Result<bool> {
            self.trace.push(json!(["attribute", node]));
            Ok(self.rows[node]["attribute"].as_bool().unwrap_or(true))
        }
    }
    #[test]
    fn base_navigation_brain_configuration_matches_original_service_agent_and_attribute_passes(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_brain_base.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let seeded = c["seeded"].as_bool().unwrap_or(false);
            let mut state = State {
                owner: Some(3),
                action: c["action"].as_bool().unwrap_or(true).then_some(12),
                services: if seeded { vec![5] } else { vec![] },
                agents: if seeded { vec![6] } else { vec![] },
            };
            let mut host = Binding {
                rows: c["nodes"].as_array().cloned().unwrap_or_default(),
                trace: vec![],
                active: 0,
                next: 100,
                instances: BTreeMap::new(),
            };
            let result =
                state.initialize(c["root"].as_bool().unwrap_or(true).then_some(1), &mut host)?;
            assert_eq!(json!(host.trace), expected["trace"], "trace: {c}");
            assert_eq!(json!(state.services), expected["services"], "services: {c}");
            assert_eq!(json!(state.agents), expected["agents"], "agents: {c}");
            assert_eq!(json!(result), expected["result"], "result: {c}");
        }
        Ok(())
    }
}
pub trait Host {
    fn child_count(&mut self, state: &mut State, root: usize) -> Result<u32>;
    fn child(&mut self, state: &mut State, root: usize, index: u32) -> Result<usize>;
    fn name(&mut self, state: &mut State, node: usize) -> Result<Option<Vec<u8>>>;
    fn class_name(&mut self, state: &mut State, node: usize) -> Result<Option<Vec<u8>>>;
    ///459898/459ab4 include the native registry's initialization/lookup.
    fn class_metadata(
        &mut self,
        state: &mut State,
        kind: Kind,
        name: &[u8],
    ) -> Result<Option<usize>>;
    fn class_factory(&mut self, state: &mut State, metadata: usize) -> Result<Option<usize>>;
    /// Constructors receive the owning navigation brain, not its entity.
    ///Allocation failure cannot become a successful placeholder instance.
    fn construct(&mut self, state: &mut State, kind: Kind, factory: usize) -> Result<usize>;
    fn initialize(
        &mut self,
        state: &mut State,
        kind: Kind,
        instance: usize,
        node: usize,
    ) -> Result<bool>;
    fn destroy(&mut self, state: &mut State, kind: Kind, instance: usize) -> Result<()>;
    fn attribute(&mut self, state: &mut State, node: usize) -> Result<bool>;
}
fn c_string(value: &[u8]) -> &[u8] {
    &value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())]
}
impl State {
    pub fn initialize(&mut self, root: Option<usize>, host: &mut impl Host) -> Result<bool> {
        //45909c precedes the null-configuration success gate.
        if self.action.is_none() {
            return Ok(false);
        }
        let Some(root) = root else { return Ok(true) };
        let mut index = 0;
        while index < host.child_count(self, root)? {
            let node = host.child(self, root, index)?;
            if host.name(self, node)?.is_none() {
                index += 1;
                continue;
            }
            let name = host
                .name(self, node)?
                .context("Navigation node name disappeared during configuration")?;
            let kind = if super::ai_traversal_registry::names_equal(&name, b"Service") {
                Some(Kind::Service)
            } else {
                //Retail queries the captured node's name again for Agent.
                let name = host
                    .name(self, node)?
                    .context("Navigation node name disappeared during configuration")?;
                super::ai_traversal_registry::names_equal(&name, b"Agent").then_some(Kind::Agent)
            };
            if let Some(kind) = kind {
                if let Some(class) = host.class_name(self, node)? {
                    if let Some(metadata) = host.class_metadata(self, kind, c_string(&class))? {
                        if let Some(factory) = host.class_factory(self, metadata)? {
                            let instance = host.construct(self, kind, factory)?;
                            if host.initialize(self, kind, instance, node)? {
                                match kind {
                                    Kind::Service => self.services.push(instance),
                                    Kind::Agent => self.agents.push(instance),
                                }
                            } else {
                                host.destroy(self, kind, instance)?;
                            }
                        }
                    }
                }
            }
            index += 1;
        }
        //Attributes run only after ALL service/agent rows. Re-query each child
        //and name as retail does; callbacks may have changed the tree.
        let mut index = 0;
        while index < host.child_count(self, root)? {
            let mut reserved = false;
            for tag in [
                b"Class".as_slice(),
                b"Service".as_slice(),
                b"Agent".as_slice(),
            ] {
                let node = host.child(self, root, index)?;
                let name = host
                    .name(self, node)?
                    .context("Navigation attribute has no name")?;
                if super::ai_traversal_registry::names_equal(&name, tag) {
                    reserved = true;
                    break;
                }
            }
            if !reserved {
                let node = host.child(self, root, index)?;
                if !host.attribute(self, node)? {
                    return Ok(false);
                }
            }
            index += 1;
        }
        Ok(true)
    }
}
