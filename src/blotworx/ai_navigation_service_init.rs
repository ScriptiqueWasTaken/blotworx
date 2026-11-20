//! Service definition dispatch465d74. Attribute handlers remain mandatory;
//! completing this pass alone does not make a pathfinder ready for use.
use super::ai_traversal_registry::names_equal;
use anyhow::{Context, Result};

pub trait Host<S> {
    fn child_count(&mut self, service: &mut S, root: usize) -> Result<u32>;
    fn child(&mut self, service: &mut S, root: usize, index: u32) -> Result<Option<usize>>;
    fn name(&mut self, service: &mut S, child: usize) -> Result<Option<Vec<u8>>>;
    fn initialize_attribute(&mut self, service: &mut S, child: usize) -> Result<bool>;
}

pub fn initialize<S>(service: &mut S, root: usize, host: &mut impl Host<S>) -> Result<bool> {
    let mut index = 0u32;
    while index < host.child_count(service, root)? {
        let child = host
            .child(service, root, index)?
            .context("Navigation service child is null")?;
        let name = host
            .name(service, child)?
            .context("Navigation service child has no name")?;
        if !names_equal(&name, b"Class") {
            // The native code retrieves the child again after comparing its tag.
            let child = host
                .child(service, root, index)?
                .context("Navigation service child disappeared")?;
            if !host.initialize_attribute(service, child)? {
                return Ok(false);
            }
        }
        index = index.wrapping_add(1);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding {
        rows: Vec<Value>,
        trace: Vec<Value>,
    }
    impl Host<()> for Binding {
        fn child_count(&mut self, _: &mut (), _: usize) -> Result<u32> {
            self.trace.push(json!(["count"]));
            Ok(self.rows.len() as u32)
        }
        fn child(&mut self, _: &mut (), _: usize, index: u32) -> Result<Option<usize>> {
            self.trace.push(json!(["child", index]));
            Ok(Some(index as usize))
        }
        fn name(&mut self, _: &mut (), child: usize) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["name", child]));
            Ok(Some(
                self.rows[child]["name"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
            ))
        }
        fn initialize_attribute(&mut self, _: &mut (), child: usize) -> Result<bool> {
            self.trace.push(json!(["attribute", child]));
            let row = self.rows[child].clone();
            if row["append"] == true {
                self.rows.push(json!({"name":"Appended"}));
            }
            if row["shrink"] == true {
                self.rows.truncate(child + 1);
            }
            Ok(row["accepted"] != false)
        }
    }
    #[test]
    fn navigation_service_definition_matches_original_live_dispatch() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_service_init.json"
        ))
        .unwrap();
        for case in data["cases"].as_array().unwrap() {
            let mut host = Binding {
                rows: case["input"]["rows"].as_array().unwrap().clone(),
                trace: vec![],
            };
            let result = initialize(&mut (), 1, &mut host).unwrap();
            assert_eq!(
                json!({"result":result,"trace":host.trace}),
                json!({"result":case["result"],"trace":case["trace"]}),
                "{}",
                case["input"]
            );
        }
        Ok(())
    }
}
