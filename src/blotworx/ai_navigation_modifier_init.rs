//!49b274 base modifier definitions and4a9588 steering initialization order.
//! Completing the definition pass alone does not publish a ready modifier.
use super::{ai_goto_gap::State, ai_traversal_registry::names_equal};
use anyhow::{Context, Result};
pub trait Host<S> {
    fn child_count(&mut self, state: &mut S, root: usize) -> Result<u32>;
    fn child(&mut self, state: &mut S, root: usize, index: u32) -> Result<Option<usize>>;
    fn name(&mut self, state: &mut S, node: usize) -> Result<Option<Vec<u8>>>;
    fn initialize_attribute(&mut self, state: &mut S, node: usize) -> Result<bool>;
}
pub fn initialize<S>(state: &mut S, root: Option<usize>, host: &mut impl Host<S>) -> Result<bool> {
    let Some(root) = root else { return Ok(true) };
    let mut index = 0u32;
    while index < host.child_count(state, root)? {
        let child = host
            .child(state, root, index)?
            .context("Modifier definition child is null")?;
        let name = host
            .name(state, child)?
            .context("Modifier definition child has no name")?;
        // The original retains this child across name comparison, in contrast
        // to465d74's second lookup for service definitions.
        if !names_equal(&name, b"Class") && !host.initialize_attribute(state, child)? {
            return Ok(false);
        }
        index = index.wrapping_add(1);
    }
    Ok(true)
}
pub trait GapHost: Host<State> {
    /// Actual4a88e4 setup is required; successful definition parsing is not setup.
    fn setup(&mut self, state: &mut State) -> Result<bool>;
}
impl State {
    pub fn initialize_definition(
        &mut self,
        root: Option<usize>,
        host: &mut impl GapHost,
    ) -> Result<bool> {
        if !initialize(self, root, host)? {
            return Ok(false);
        }
        host.setup(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Construction;
    impl super::super::ai_goto_gap::Host for Construction {
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            Ok(None)
        }
        fn owner_entity(&mut self, _: &mut State) -> Result<usize> {
            Ok(20)
        }
        fn profiling_enabled(&mut self, _: &mut State) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut State, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected profiler")
        }
    }
    struct Binding<'a> {
        case: &'a Value,
        rows: Vec<Value>,
        children: Vec<usize>,
        swapped: bool,
        trace: Vec<Value>,
    }
    impl<S> Host<S> for Binding<'_> {
        fn child_count(&mut self, _: &mut S, _: usize) -> Result<u32> {
            self.trace.push(json!(["count"]));
            Ok(self.children.len() as u32)
        }
        fn child(&mut self, _: &mut S, _: usize, index: u32) -> Result<Option<usize>> {
            self.trace.push(json!(["child", index]));
            Ok(self.children.get(index as usize).copied())
        }
        fn name(&mut self, _: &mut S, node: usize) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["name", node]));
            let name = self.rows[node]["name"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec();
            if self.case["swap"] == true && !self.swapped {
                self.children[0] = self.rows.len();
                self.rows
                    .push(json!({"name":"Replacement","accepted":false}));
                self.swapped = true;
            }
            Ok(Some(name))
        }
        fn initialize_attribute(&mut self, _: &mut S, node: usize) -> Result<bool> {
            self.trace.push(json!(["attribute", node]));
            let row = self.rows[node].clone();
            if row["append"] == true {
                self.children.push(self.rows.len());
                self.rows.push(json!({"name":"Appended"}));
            }
            if row["shrink"] == true {
                self.children.truncate(1);
            }
            Ok(row["accepted"] != false)
        }
    }
    impl GapHost for Binding<'_> {
        fn setup(&mut self, _: &mut State) -> Result<bool> {
            self.trace.push(json!(["setup"]));
            Ok(self.case["setup"] != false)
        }
    }
    #[test]
    fn navigation_modifier_definition_matches_original_retained_children_and_required_setup(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_modifier_init.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let rows = c["rows"].as_array().unwrap().clone();
            let mut host = Binding {
                case: c,
                children: (0..rows.len()).collect(),
                rows,
                swapped: false,
                trace: vec![],
            };
            let root = (c["root"] != false).then_some(1);
            let result = if c["mode"] == "base" {
                initialize(&mut (), root, &mut host)?
            } else {
                State::construct(60, 50, b"IGoto", &mut Construction)?
                    .initialize_definition(root, &mut host)?
            };
            assert_eq!(
                json!({"result":result,"trace":host.trace}),
                json!({"result":expected["result"],"trace":expected["trace"]}),
                "{c}"
            );
        }
        Ok(())
    }
}
