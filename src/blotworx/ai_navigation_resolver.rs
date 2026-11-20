//!4572d8 source-definition expansion before4577b0. Owns an independent
//! temporary tree; missing references drop it, leaving the source unchanged.
use super::{
    ai_navigation_definitions::{Node, Tree},
    ai_traversal_registry::names_equal,
};
use anyhow::{Context, Result};

pub struct Manager {
    pub identity: usize,
    pub allocator: Option<usize>,
    pub definition_root: Option<usize>,
}
pub struct Runtime {
    pub world: Option<usize>,
    pub allocator: usize,
    pub fallback_allocator: usize,
}
pub struct Resolved {
    pub tree: Tree,
    pub root: usize,
}
impl Runtime {
    pub fn resolve(
        &mut self,
        manager: Option<&Manager>,
        source: &Tree,
        name: Option<&[u8]>,
    ) -> Result<Option<Resolved>> {
        let Some(manager) = manager else {
            return Ok(None);
        };
        //45730c/457324/457338 publish globals before missing-root failure.
        self.world = Some(manager.identity);
        self.allocator = manager.allocator.unwrap_or(self.fallback_allocator);
        let Some(source_root) = manager.definition_root else {
            return Ok(None);
        };
        let mut resolved = Resolved {
            tree: Tree::default(),
            root: 0,
        };
        resolved.root = resolved.tree.append(None, Node::default())?;
        let Some(entities) = source.find_tag(source_root, b"Entities")? else {
            return Ok(None);
        };
        let Some(entity) = source.find_identifier(entities, b"Entity", name)? else {
            return Ok(None);
        };
        for &child in &source.node(entity)?.children {
            let tag = source
                .node(child)?
                .name
                .as_deref()
                .context("Navigation entity definition contains a nameless child")?;
            if !names_equal(tag, b"Brain") {
                resolved
                    .tree
                    .copy_subtree(source, child, Some(resolved.root))?;
            }
        }
        let Some(brain_name) = source.first_value(entity, b"Brain")? else {
            return Ok(Some(resolved));
        };
        let Some(brains) = source.find_tag(source_root, b"Brains")? else {
            return Ok(None);
        };
        let Some(brain) = source.find_identifier(brains, b"Brain", Some(brain_name))? else {
            return Ok(None);
        };
        let output_brain = resolved.tree.append(
            Some(resolved.root),
            Node {
                name: Some(b"Brain".to_vec()),
                ..Node::default()
            },
        )?;
        let agents = source.find_tag(source_root, b"Agents")?;
        let services = source.find_tag(source_root, b"Services")?;
        for &child in &source.node(brain)?.children {
            let node = source.node(child)?;
            let kind = node.name.as_deref().and_then(|name| {
                if names_equal(name, b"Agent") {
                    Some((agents, b"Agent".as_slice()))
                } else if names_equal(name, b"Service") {
                    Some((services, b"Service".as_slice()))
                } else {
                    None
                }
            });
            if let Some((collection, tag)) = kind {
                let Some(value) = node.value.as_deref() else {
                    return Ok(None);
                };
                let Some(collection) = collection else {
                    return Ok(None);
                };
                let Some(definition) = source.find_identifier(collection, tag, Some(value))? else {
                    return Ok(None);
                };
                let copied = resolved
                    .tree
                    .copy_subtree(source, definition, Some(output_brain))?;
                //45766c/457764 change the copied TAG, not its identifier.
                //457678/457770 set VALUE to the source definition identifier.
                let identifier = source
                    .node(definition)?
                    .identifier
                    .as_deref()
                    .context("Resolved navigation identifier disappeared")?;
                resolved.tree.nodes[copied].name = Some(tag.to_vec());
                resolved.tree.nodes[copied].value = Some(
                    identifier[..identifier
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(identifier.len())]
                        .to_vec(),
                );
            } else {
                resolved
                    .tree
                    .copy_subtree(source, child, Some(output_brain))?;
            }
        }
        Ok(Some(resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn snapshot(tree: &Tree, index: usize) -> Result<Value> {
        let node = tree.node(index)?;
        let children = node
            .children
            .iter()
            .map(|&i| snapshot(tree, i))
            .collect::<Result<Vec<_>>>()?;
        let mut value = serde_json::to_value(node)?;
        value["children"] = json!(children);
        Ok(value)
    }
    #[test]
    fn navigation_definition_resolver_matches_original_expansion_copy_failures_and_context(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_resolver.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let source = Tree {
                nodes: serde_json::from_value(c["nodes"].clone())?,
            };
            let before = serde_json::to_value(&source.nodes)?;
            let query = c["query"].as_array().map(|a| {
                a.iter()
                    .map(|v| v.as_u64().unwrap() as u8)
                    .collect::<Vec<_>>()
            });
            let manager = Manager {
                identity: 300,
                allocator: (c["override"] == true).then_some(401),
                definition_root: (c["root"] != false).then_some(0),
            };
            let mut runtime = Runtime {
                world: Some(310),
                allocator: 402,
                fallback_allocator: 400,
            };
            let result = runtime.resolve(
                (c["manager"] != false).then_some(&manager),
                &source,
                query.as_deref(),
            )?;
            let output = result
                .as_ref()
                .map(|r| snapshot(&r.tree, r.root))
                .transpose()?;
            assert_eq!(
                json!(output),
                expected["result"],
                "resolved tree: {:?}",
                c.get("variant")
            );
            assert_eq!(json!(runtime.world), expected["world"]);
            assert_eq!(json!(runtime.allocator), expected["allocator"]);
            assert_eq!(
                before,
                serde_json::to_value(&source.nodes)?,
                "Resolver modified source data"
            );
            assert_eq!(
                expected["deleted"].as_u64().unwrap(),
                u64::from(result.is_none() && c["manager"] != false && c["root"] != false)
            );
        }
        Ok(())
    }
}
