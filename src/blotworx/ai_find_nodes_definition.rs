use super::{
    ai_find_nodes::State, ai_navigation_definitions::Tree,
    ai_navigation_modifier_init as definition,
};
use anyhow::{Context as _, Result};
impl State {
    pub fn initialize_definition(&mut self, tree: &mut Tree, root: Option<usize>) -> Result<bool> {
        definition::initialize(self, root, &mut Definitions { tree })
    }
}
struct Definitions<'a> {
    tree: &'a mut Tree,
}
impl definition::Host<State> for Definitions<'_> {
    fn child_count(&mut self, _: &mut State, r: usize) -> Result<u32> {
        Ok(u32::try_from(self.tree.node(r)?.children.len())?)
    }
    fn child(&mut self, _: &mut State, r: usize, i: u32) -> Result<Option<usize>> {
        Ok(self.tree.node(r)?.children.get(i as usize).copied())
    }
    fn name(&mut self, _: &mut State, n: usize) -> Result<Option<Vec<u8>>> {
        Ok(self.tree.node(n)?.name.clone())
    }
    fn initialize_attribute(&mut self, _: &mut State, n: usize) -> Result<bool> {
        // Actual2b7c10 re-reads the name and accepts metadata beginning with '_'.
        Ok(self
            .tree
            .node(n)?
            .name
            .as_ref()
            .context("Modifier name disappeared")?
            .first()
            == Some(&b'_'))
    }
}

#[cfg(test)]
mod tests {
    use super::super::ai_navigation_definitions::Node;
    use super::*;
    use serde_json::Value;
    #[test]
    fn original_nearest_reachable_definitions_accept_class_and_underscore_metadata() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_find_nodes.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            if input["op"] != "init" {
                continue;
            }
            let mut state = State::construct(1, 2, b"nearest");
            state.start_flags = 11;
            state.goal_flags = 22;
            let mut tree = Tree::default();
            let root = tree.append(None, Node::default())?;
            for name in input["attrs"].as_array().unwrap() {
                tree.append(
                    Some(root),
                    Node {
                        name: Some(name.as_str().unwrap().as_bytes().to_vec()),
                        ..Node::default()
                    },
                )?;
            }
            let result = state.initialize_definition(
                &mut tree,
                input["root"].as_bool().unwrap().then_some(root),
            )?;
            assert_eq!(
                u32::from(result),
                case["result"].as_u64().unwrap() as u32,
                "{input}"
            );
            state.reset();
            assert_eq!([state.start_flags, state.goal_flags], [11, 22]);
        }
        Ok(())
    }
}
