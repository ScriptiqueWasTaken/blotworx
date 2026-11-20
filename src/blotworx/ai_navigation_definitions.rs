//! Owned ordered navigation definition tree. Identifier, tag and value are
//! separate fields (457f08 ->45abfc/45ac58/45acb4); duplicates are preserved.
use super::{
    ai_agent_profiles::Definition,
    ai_navigation_entity_base::Entity,
    ai_navigation_entity_init::{Host, Profiles},
};
use anyhow::{ensure, Context, Result};

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub word_04: u32,
    pub word_08: u32,
    pub attributes: Vec<(Vec<u8>, Vec<u8>)>,
    pub identifier: Option<Vec<u8>>,
    pub name: Option<Vec<u8>>,
    pub value: Option<Vec<u8>>,
    pub children: Vec<usize>,
}
fn string(value: &[u8]) -> Vec<u8> {
    value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())].to_vec()
}
impl Node {
    ///45b210/45b544 keep the first key spelling and replace its value.
    pub fn set_attribute(&mut self, key: &[u8], value: &[u8]) {
        let (key, value) = (string(key), string(value));
        if let Some((_, stored)) = self
            .attributes
            .iter_mut()
            .find(|(k, _)| super::ai_traversal_registry::names_equal(k, &key))
        {
            *stored = value.clone();
        } else {
            self.attributes.push((key.clone(), value.clone()));
        }
        if key.len() >= 4 && super::ai_traversal_registry::names_equal(&key[..4], b"Name") {
            self.identifier = Some(value);
        }
    }
    ///45abfc/45ad10 synchronizes the identifier and the Name attribute.
    pub fn set_identifier(&mut self, value: Option<&[u8]>) {
        self.identifier = None;
        if let Some(value) = value {
            self.set_attribute(b"Name", value);
        } else if let Some(index) = self
            .attributes
            .iter()
            .position(|(key, _)| super::ai_traversal_registry::names_equal(key, b"Name"))
        {
            self.attributes.remove(index);
        }
    }
}
#[derive(Default, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
}
impl Tree {
    pub fn append(&mut self, parent: Option<usize>, node: Node) -> Result<usize> {
        if let Some(parent) = parent {
            let owner = self
                .nodes
                .get(parent)
                .context("Navigation definition parent absent")?;
            ensure!(
                owner.children.len() < u32::MAX as usize,
                "Navigation definition child count exceeds native word domain"
            );
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        if let Some(parent) = parent {
            self.nodes[parent].children.push(index);
        }
        Ok(index)
    }
    pub(super) fn node(&self, index: usize) -> Result<&Node> {
        self.nodes
            .get(index)
            .context("Navigation definition node absent")
    }
    pub fn find_tag(&self, parent: usize, name: &[u8]) -> Result<Option<usize>> {
        for &child in &self.node(parent)?.children {
            if self
                .node(child)?
                .name
                .as_ref()
                .is_some_and(|tag| super::ai_traversal_registry::names_equal(tag, name))
            {
                return Ok(Some(child));
            }
        }
        Ok(None)
    }
    pub fn find_identifier(
        &self,
        parent: usize,
        name: &[u8],
        identifier: Option<&[u8]>,
    ) -> Result<Option<usize>> {
        let Some(identifier) = identifier else {
            return Ok(None);
        };
        for &child in &self.node(parent)?.children {
            let node = self.node(child)?;
            if node
                .name
                .as_ref()
                .is_some_and(|tag| super::ai_traversal_registry::names_equal(tag, name))
                && node
                    .identifier
                    .as_ref()
                    .is_some_and(|id| super::ai_traversal_registry::names_equal(id, identifier))
            {
                return Ok(Some(child));
            }
        }
        Ok(None)
    }
    ///45a938 returns the FIRST matching child's value, including its null.
    pub fn first_value(&self, parent: usize, name: &[u8]) -> Result<Option<&[u8]>> {
        match self.find_tag(parent, name)? {
            Some(child) => Ok(self.node(child)?.value.as_deref()),
            None => Ok(None),
        }
    }
    pub fn copy_subtree(
        &mut self,
        source: &Tree,
        node: usize,
        parent: Option<usize>,
    ) -> Result<usize> {
        fn shallow(node: &Node) -> Node {
            let mut copied = Node::default();
            for (key, value) in &node.attributes {
                copied.set_attribute(key, value);
            }
            copied.set_identifier(node.identifier.as_deref());
            copied.name = node.name.as_deref().map(string);
            copied.value = node.value.as_deref().map(string);
            copied.word_04 = node.word_04;
            copied.word_08 = node.word_08;
            copied
        }
        let root = self.append(parent, shallow(source.node(node)?))?;
        let mut ancestors = std::collections::BTreeSet::from([node]);
        let mut stack = vec![(node, root, 0usize)];
        while let Some((input, output, index)) = stack.last_mut() {
            let Some(&child) = source.node(*input)?.children.get(*index) else {
                ancestors.remove(input);
                stack.pop();
                continue;
            };
            *index += 1;
            ensure!(ancestors.insert(child), "Cyclic navigation definition tree");
            let copied = self.append(Some(*output), shallow(source.node(child)?))?;
            stack.push((child, copied, 0));
        }
        Ok(root)
    }
}
pub struct BuiltIn {
    pub tree: Tree,
    pub entity: usize,
    pub brain: usize,
}
impl BuiltIn {
    ///187ffc plus real470a20/470a34/470af8 helper semantics. The enclosing
    ///entity/brain collection nodes are supplied roots, not invented tags.
    pub fn biped(definition: &Definition) -> Result<Self> {
        let mut tree = Tree::default();
        let entities = tree.append(None, Node::default())?;
        let brains = tree.append(None, Node::default())?;
        let entity = tree.append(
            Some(entities),
            Node {
                identifier: Some(definition.name.as_bytes().to_vec()),
                attributes: vec![(b"Name".to_vec(), definition.name.as_bytes().to_vec())],
                name: Some(b"Entity".to_vec()),
                ..Node::default()
            },
        )?;
        fn value(tree: &mut Tree, parent: usize, name: &str, value: &str) -> Result<()> {
            tree.append(
                Some(parent),
                Node {
                    name: Some(name.as_bytes().to_vec()),
                    value: Some(value.as_bytes().to_vec()),
                    ..Node::default()
                },
            )?;
            Ok(())
        }
        value(&mut tree, entity, "Class", definition.entity_class)?;
        value(&mut tree, entity, "ActionClass", definition.action_class)?;
        value(&mut tree, entity, "Brain", definition.brain)?;
        let brain = tree.append(
            Some(brains),
            Node {
                identifier: Some(definition.name.as_bytes().to_vec()),
                attributes: vec![(b"Name".to_vec(), definition.name.as_bytes().to_vec())],
                name: Some(b"Brain".to_vec()),
                ..Node::default()
            },
        )?;
        value(&mut tree, brain, "Class", definition.brain_class)?;
        value(&mut tree, brain, "Service", definition.service)?;
        for agent in definition.agents {
            value(&mut tree, brain, "Agent", agent)?;
        }
        Ok(Self {
            tree,
            entity,
            brain,
        })
    }
}
pub trait ProfileServices {
    fn manager(&mut self, entity: &mut Entity) -> Result<Option<usize>>;
    fn find(&mut self, entity: &mut Entity, manager: usize, name: &[u8]) -> Result<Option<usize>>;
    fn attach(&mut self, entity: &mut Entity, profile: usize) -> Result<bool>;
    fn detach(&mut self, entity: &mut Entity, profile: usize) -> Result<()>;
}
pub struct Binding<'a, P> {
    pub tree: &'a mut Tree,
    pub profiles: &'a mut P,
}
impl<P: ProfileServices> Profiles for Binding<'_, P> {
    fn profile_manager(&mut self, e: &mut Entity) -> Result<Option<usize>> {
        self.profiles.manager(e)
    }
    fn find_profile(
        &mut self,
        e: &mut Entity,
        manager: usize,
        value: usize,
    ) -> Result<Option<usize>> {
        let value = self
            .tree
            .node(value)?
            .value
            .as_deref()
            .context("Retained navigation Profile value disappeared")?;
        let value = &value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())];
        self.profiles.find(e, manager, value)
    }
    fn attach_profile(&mut self, e: &mut Entity, profile: usize) -> Result<bool> {
        self.profiles.attach(e, profile)
    }
    fn detach_profile(&mut self, e: &mut Entity, profile: usize) -> Result<()> {
        self.profiles.detach(e, profile)
    }
}
impl<P: ProfileServices> Host for Binding<'_, P> {
    fn child_count(&mut self, _: &mut Entity, root: usize) -> Result<u32> {
        u32::try_from(self.tree.node(root)?.children.len())
            .context("Navigation definition child count exceeds native word domain")
    }
    fn child(&mut self, _: &mut Entity, root: usize, index: u32) -> Result<Option<usize>> {
        Ok(self.tree.node(root)?.children.get(index as usize).copied())
    }
    fn name(&mut self, _: &mut Entity, node: usize) -> Result<Option<Vec<u8>>> {
        Ok(self.tree.node(node)?.name.clone())
    }
    fn value(&mut self, _: &mut Entity, node: usize) -> Result<Option<usize>> {
        Ok(self.tree.node(node)?.value.as_ref().map(|_| node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoProfileQueries;
    #[test]
    fn navigation_subtree_copy_keeps_owned_data_and_rejects_cycles() -> Result<()> {
        let mut source = Tree::default();
        let mut parent = None;
        for index in 0..4096 {
            parent = Some(source.append(
                parent,
                Node {
                    word_04: index,
                    value: Some(b"original".to_vec()),
                    ..Node::default()
                },
            )?);
        }
        let mut copied = Tree::default();
        let root = copied.copy_subtree(&source, 0, None)?;
        assert_eq!(root, 0);
        assert_eq!(copied.nodes.len(), source.nodes.len());
        source.nodes.last_mut().unwrap().value = Some(b"changed".to_vec());
        assert_eq!(
            copied.nodes.last().unwrap().value.as_deref(),
            Some(b"original".as_slice())
        );
        source.nodes.last_mut().unwrap().children.push(0);
        assert!(Tree::default().copy_subtree(&source, 0, None).is_err());
        Ok(())
    }
    impl ProfileServices for NoProfileQueries {
        fn manager(&mut self, _: &mut Entity) -> Result<Option<usize>> {
            anyhow::bail!("Built-in entity unexpectedly queries Profile manager")
        }
        fn find(&mut self, _: &mut Entity, _: usize, _: &[u8]) -> Result<Option<usize>> {
            anyhow::bail!("Unexpected Profile lookup")
        }
        fn attach(&mut self, _: &mut Entity, _: usize) -> Result<bool> {
            anyhow::bail!("Unexpected Profile attachment")
        }
        fn detach(&mut self, _: &mut Entity, _: usize) -> Result<()> {
            anyhow::bail!("Unexpected Profile detachment")
        }
    }
    #[test]
    fn navigation_definition_trees_match_original_builders_and_initialize_real_entity_storage(
    ) -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_definitions.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let definition = Definition::built_in_biped(expected["name"].as_str().unwrap())
                .context("Original biped definition absent")?;
            let mut built = BuiltIn::biped(definition)?;
            assert_eq!(serde_json::to_value(&built.tree.nodes)?, expected["nodes"]);
            assert_eq!(built.entity, expected["entity"].as_u64().unwrap() as usize);
            assert_eq!(built.brain, expected["brain"].as_u64().unwrap() as usize);
            let mut entity = Entity::new(10, None, [0; 2], 0)?;
            entity.common.definition = Some(built.entity);
            assert!(entity.initialize_definition(
                built.entity,
                &mut Binding {
                    tree: &mut built.tree,
                    profiles: &mut NoProfileQueries
                }
            )?);
            assert_eq!(entity.profile_68, None);
            assert_eq!(entity.profile_node_70, None);
            assert_eq!(entity.common.definition, Some(built.entity));
            built.tree.append(
                Some(built.entity),
                Node {
                    name: Some(b"UnsupportedRequiredAttribute".to_vec()),
                    ..Node::default()
                },
            )?;
            assert!(!entity.initialize_definition(
                built.entity,
                &mut Binding {
                    tree: &mut built.tree,
                    profiles: &mut NoProfileQueries
                }
            )?);
        }
        Ok(())
    }
}
