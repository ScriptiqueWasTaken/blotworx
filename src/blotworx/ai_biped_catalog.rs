//!188214 source entities/brains/agents and1890e4/188f48 pathfinder definitions.
//! Input databases must be admitted by the owning runtime. This catalog does
//! not construct the global graph/mesh/LPF services or a ready AI manager.
use super::{
    ai_agent_profiles::{self, Definition},
    ai_navigation_definitions::{BuiltIn, Node, Tree},
};
use anyhow::Result;
pub struct Database {
    pub radius: i32,
    pub height: i32,
    pub path: Option<Vec<u8>>,
}
impl Database {
    ///189344 normalizes separators and takes LEFT(last slash), via original
    ///804b3e70/804b4c60. No slash retains the whole path; an empty prefix is null.
    pub fn identifier(&self) -> Option<Vec<u8>> {
        let path = self.path.as_deref()?;
        let path = &path[..path.iter().position(|&b| b == 0).unwrap_or(path.len())];
        let path = path
            .iter()
            .map(|&b| if b == b'\\' { b'/' } else { b })
            .collect::<Vec<_>>();
        let end = path.iter().rposition(|&b| b == b'/').unwrap_or(path.len());
        (end != 0).then(|| path[..end].to_vec())
    }
}
pub struct Catalog {
    pub tree: Tree,
    pub root: usize,
}
fn add(
    tree: &mut Tree,
    parent: usize,
    name: &str,
    identifier: Option<&str>,
    value: Option<&[u8]>,
) -> Result<usize> {
    let mut node = Node {
        name: Some(name.as_bytes().to_vec()),
        value: value.map(|v| v.to_vec()),
        ..Node::default()
    };
    if let Some(id) = identifier {
        node.set_identifier(Some(id.as_bytes()));
    }
    tree.append(Some(parent), node)
}
fn value(tree: &mut Tree, parent: usize, name: &str, text: &str) -> Result<()> {
    add(tree, parent, name, None, Some(text.as_bytes()))?;
    Ok(())
}
impl Catalog {
    pub fn new(databases: &[Database]) -> Result<Self> {
        let mut tree = Tree::default();
        let root = tree.append(None, Node::default())?;
        let entities = add(&mut tree, root, "Entities", None, None)?;
        let brains = add(&mut tree, root, "Brains", None, None)?;
        for definition in Definition::built_in_bipeds() {
            let source = BuiltIn::biped(definition)?;
            tree.copy_subtree(&source.tree, source.entity, Some(entities))?;
            tree.copy_subtree(&source.tree, source.brain, Some(brains))?;
        }
        for (name, class) in [
            ("Mickey", "JPSBipedEntity"),
            ("Obstacle", "JPSObstacleEntity"),
        ] {
            let entity = add(&mut tree, entities, "Entity", Some(name), None)?;
            value(&mut tree, entity, "Class", class)?;
        }
        let agents = add(&mut tree, root, "Agents", None, None)?;
        for (name, class) in [
            ("GotoAgent", "JPSGotoAgent"),
            ("WanderAgent", "JPSWanderAgent"),
            ("FleeAgent", "Mpd::JPSFleeAgent"),
        ] {
            let agent = add(&mut tree, agents, "Agent", Some(name), None)?;
            value(&mut tree, agent, "Class", class)?;
            if name == "FleeAgent" {
                value(&mut tree, agent, "TraversalConfig", "FleeTraversal")?;
                value(
                    &mut tree,
                    agent,
                    "CheckSafetyTask",
                    "Mpd::JPSFleeAgent::CheckSafety",
                )?;
            }
        }
        let services = add(&mut tree, root, "Services", None, None)?;
        for database in databases {
            let name = format!(
                "{}PathFinder",
                ai_agent_profiles::descriptor(database.radius, database.height, 1)
            );
            let service = add(&mut tree, services, "Service", Some(&name), None)?;
            value(&mut tree, service, "Class", "Mpd::CPathFinder")?;
            value(&mut tree, service, "AvoidCpuPeaks", "1")?;
            let module = add(&mut tree, service, "LpfModule", None, None)?;
            for (key, text) in [
                ("LpfContext", "LpfContext1"),
                ("LocalPathmaxLength", "128"),
                ("TabooEdgeMaxCount", "30"),
            ] {
                value(&mut tree, module, key, text)?;
            }
            let modifiers = add(
                &mut tree,
                service,
                "Modifiers_IGoto",
                Some("Modifiers_IGoto"),
                None,
            )?;
            let goto = add(&mut tree, modifiers, "IGoto", Some("IGoto"), None)?;
            value(
                &mut tree,
                goto,
                "Class",
                "Mpd::CGoto_GapDynamicAvoidance_v2",
            )?;
            value(&mut tree, goto, "ContinuousSpeed", "1")?;
            value(&mut tree, modifiers, "Default", "IGoto")?;
            let modifiers = add(
                &mut tree,
                service,
                "Modifiers_IRefineGoal",
                Some("Modifiers_IRefineGoal"),
                None,
            )?;
            let refine = add(
                &mut tree,
                modifiers,
                "IRefineGoal",
                Some("IRefineGoal"),
                None,
            )?;
            value(&mut tree, refine, "Class", "JPSRefineGoal")?;
            value(&mut tree, modifiers, "Default", "IRefineGoal")?;
            add(
                &mut tree,
                service,
                "DataBase",
                None,
                database.identifier().as_deref(),
            )?;
        }
        Ok(Self { tree, root })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    use serde_json::{json, Value};
    fn snapshot(tree: &Tree, index: usize) -> Result<Value> {
        let node = tree.node(index)?;
        let mut value = serde_json::to_value(node)?;
        value["children"] = json!(node
            .children
            .iter()
            .map(|&i| snapshot(tree, i))
            .collect::<Result<Vec<_>>>()?);
        Ok(value)
    }
    #[test]
    fn biped_navigation_catalog_matches_original_source_builders_and_database_identifiers(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_biped_catalog.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let databases = expected["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| Database {
                    radius: d["radius"].as_i64().unwrap_or(0) as i32,
                    height: d["height"].as_i64().unwrap_or(0) as i32,
                    path: d["path"]
                        .as_array()
                        .map(|a| a.iter().map(|b| b.as_u64().unwrap() as u8).collect()),
                })
                .collect::<Vec<_>>();
            let catalog = Catalog::new(&databases)?;
            assert_eq!(snapshot(&catalog.tree, catalog.root)?, expected["tree"]);
            let names = databases
                .iter()
                .flat_map(|d| [d.identifier(), d.identifier()])
                .collect::<Vec<_>>();
            assert_eq!(json!(names), expected["database_names"]);
            let mut runtime = super::super::ai_navigation_resolver::Runtime {
                world: None,
                allocator: 0,
                fallback_allocator: 400,
            };
            let manager = super::super::ai_navigation_resolver::Manager {
                identity: 300,
                allocator: None,
                definition_root: Some(catalog.root),
            };
            for d in &databases {
                let descriptor = ai_agent_profiles::descriptor(d.radius, d.height, 1);
                if Definition::built_in_biped(&descriptor).is_none() {
                    continue;
                }
                let resolved = runtime
                    .resolve(Some(&manager), &catalog.tree, Some(descriptor.as_bytes()))?
                    .context("Captured biped definition did not resolve")?;
                let brain = resolved
                    .tree
                    .find_tag(resolved.root, b"Brain")?
                    .context("Resolved brain missing")?;
                let service = resolved
                    .tree
                    .find_tag(brain, b"Service")?
                    .context("Resolved pathfinder missing")?;
                assert_eq!(
                    resolved.tree.first_value(service, b"Class")?,
                    Some(b"Mpd::CPathFinder".as_slice())
                );
                assert_eq!(
                    resolved
                        .tree
                        .node(brain)?
                        .children
                        .iter()
                        .filter(|&&i| resolved.tree.node(i).unwrap().name.as_deref()
                            == Some(b"Agent".as_slice()))
                        .count(),
                    3
                );
            }
        }
        Ok(())
    }
}
