//! Entity definition45f6d4, attribute45f7ac and profile binding45ff54.
//! Tree, profile-manager and attachment ownership are mandatory services.
use super::{ai_navigation_entity_base::Entity, ai_traversal_registry::names_equal};
use anyhow::{Context, Result};

pub trait Profiles {
    fn profile_manager(&mut self, entity: &mut Entity) -> Result<Option<usize>>;
    /// Value is a retained tree-value handle, not an early string snapshot.
    fn find_profile(
        &mut self,
        entity: &mut Entity,
        manager: usize,
        value: usize,
    ) -> Result<Option<usize>>;
    fn attach_profile(&mut self, entity: &mut Entity, profile: usize) -> Result<bool>;
    fn detach_profile(&mut self, entity: &mut Entity, profile: usize) -> Result<()>;
}
pub trait Host: Profiles {
    fn child_count(&mut self, entity: &mut Entity, root: usize) -> Result<u32>;
    fn child(&mut self, entity: &mut Entity, root: usize, index: u32) -> Result<Option<usize>>;
    fn name(&mut self, entity: &mut Entity, node: usize) -> Result<Option<Vec<u8>>>;
    fn value(&mut self, entity: &mut Entity, node: usize) -> Result<Option<usize>>;
}
impl Entity {
    pub fn initialize_definition(&mut self, root: usize, host: &mut impl Host) -> Result<bool> {
        let mut index = 0;
        while index < host.child_count(self, root)? {
            let child = host
                .child(self, root, index)?
                .context("Navigation definition child is null")?;
            let mut reserved = false;
            for tag in [
                b"Class".as_slice(),
                b"ActionClass".as_slice(),
                b"Brain".as_slice(),
            ] {
                let name = host
                    .name(self, child)?
                    .context("Navigation definition child has no name")?;
                if names_equal(&name, tag) {
                    reserved = true;
                    break;
                }
            }
            if !reserved && !self.initialize_attribute(Some(child), host)? {
                return Ok(false);
            }
            index += 1;
        }
        Ok(true)
    }
    pub fn initialize_attribute(
        &mut self,
        node: Option<usize>,
        host: &mut impl Host,
    ) -> Result<bool> {
        let Some(node) = node else { return Ok(true) };
        if host.name(self, node)?.is_some() {
            let name = host
                .name(self, node)?
                .context("Navigation attribute name disappeared before underscore query")?;
            if name.first() == Some(&b'_') {
                return Ok(true);
            }
        }
        if host.name(self, node)?.is_none() {
            return Ok(false);
        }
        let name = host
            .name(self, node)?
            .context("Navigation attribute name disappeared before Profile comparison")?;
        if !names_equal(&name, b"Profile") {
            return Ok(false);
        }
        if let Some(value) = host.value(self, node)? {
            if let Some(manager) = host.profile_manager(self)? {
                if let Some(profile) = host.find_profile(self, manager, value)? {
                    //45f880 returns true independently of45ff54's result.
                    self.set_navigation_profile(Some(profile), host)?;
                }
            }
        }
        Ok(true)
    }
    pub fn set_navigation_profile(
        &mut self,
        profile: Option<usize>,
        host: &mut impl Profiles,
    ) -> Result<bool> {
        let old = self.profile_68;
        if profile == old && self.profile_node_70.is_some() {
            return Ok(true);
        }
        if self.common.word_28 != 0 {
            if let Some(profile) = profile {
                return host.attach_profile(self, profile);
            }
            if let Some(old) = old {
                if self.profile_node_70.is_some() {
                    host.detach_profile(self, old)?;
                }
            }
        } else {
            self.profile_68 = profile;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        rows: Vec<Value>,
        names: Vec<usize>,
        count: usize,
        trace: Vec<Value>,
    }
    impl Profiles for Binding<'_> {
        fn profile_manager(&mut self, _: &mut Entity) -> Result<Option<usize>> {
            self.trace.push(json!(["manager"]));
            Ok((self.case["manager"] != false).then_some(50))
        }
        fn find_profile(
            &mut self,
            _: &mut Entity,
            manager: usize,
            value: usize,
        ) -> Result<Option<usize>> {
            let name = self.rows[value]["value"]
                .as_str()
                .unwrap()
                .split('\0')
                .next()
                .unwrap();
            self.trace.push(json!(["find", manager, name]));
            Ok(if self.case.get("found").is_none() {
                Some(20)
            } else {
                self.case["found"].as_u64().map(|id| id as usize)
            })
        }
        fn attach_profile(&mut self, e: &mut Entity, profile: usize) -> Result<bool> {
            self.trace.push(json!(["attach", profile]));
            if self.case["effect"] == true {
                e.profile_68 = Some(22);
                e.profile_node_70 = Some(81);
            }
            Ok(self.case["attach"] != false)
        }
        fn detach_profile(&mut self, e: &mut Entity, profile: usize) -> Result<()> {
            self.trace.push(json!(["detach", profile]));
            if self.case["effect"] == true {
                e.profile_node_70 = None;
            }
            Ok(())
        }
    }
    impl Host for Binding<'_> {
        fn child_count(&mut self, _: &mut Entity, root: usize) -> Result<u32> {
            assert_eq!(root, 1);
            self.trace.push(json!(["count"]));
            Ok(self.count as u32)
        }
        fn child(&mut self, _: &mut Entity, root: usize, index: u32) -> Result<Option<usize>> {
            assert_eq!(root, 1);
            self.trace.push(json!(["child", index]));
            Ok(Some(index as usize))
        }
        fn name(&mut self, _: &mut Entity, node: usize) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["name", node]));
            let row = &self.rows[node];
            let count = self.names[node];
            self.names[node] += 1;
            let name = row["names"]
                .as_array()
                .map_or(&row["name"], |a| &a[count.min(a.len() - 1)]);
            Ok(name.as_str().map(|s| s.as_bytes().to_vec()))
        }
        fn value(&mut self, _: &mut Entity, node: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["value", node]));
            let result = self.rows[node]["value"].as_str().map(|_| node);
            if self.rows[node]["append"] == true {
                self.rows.push(json!({"name":"Unexpected"}));
                self.names.push(0);
                self.count = self.rows.len();
            }
            if self.rows[node]["shrink"] == true {
                self.count = node + 1;
            }
            Ok(result)
        }
    }
    #[test]
    fn navigation_entity_definition_and_profile_binding_match_original_live_tree_and_returns(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_entity_init.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let rows = c["rows"].as_array().cloned().unwrap_or_default();
            let mut binding = Binding {
                case: c,
                names: vec![0; rows.len()],
                count: rows.len(),
                rows,
                trace: Vec::new(),
            };
            let mut entity = Entity::new(10, None, [0; 2], 0)?;
            entity.common.word_28 = c["active"].as_u64().unwrap_or(0) as u32;
            entity.profile_68 = c["old"].as_u64().map(|v| v as usize);
            entity.profile_node_70 = c["node"].as_u64().map(|v| v as usize);
            let result = match c["mode"].as_str().unwrap() {
                "definition" => entity.initialize_definition(1, &mut binding)?,
                "attribute" => entity
                    .initialize_attribute((!binding.rows.is_empty()).then_some(0), &mut binding)?,
                _ => entity.set_navigation_profile(
                    c["requested"].as_u64().map(|v| v as usize),
                    &mut binding,
                )?,
            };
            assert_eq!(json!(binding.trace), expected["trace"], "trace:{c}");
            assert_eq!(
                json!({"result":result,"profile":entity.profile_68,"node":entity.profile_node_70,"active":entity.common.word_28}),
                json!({"result":expected["result"],"profile":expected["profile"],"node":expected["node"],"active":expected["active"]}),
                "state:{c}"
            );
        }
        Ok(())
    }
}
