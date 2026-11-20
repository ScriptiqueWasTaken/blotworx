//! Generic45efcc and JPSBaseEntity190714 owned construction. The AI-derived
//! constructor, definition initialization and World admission remain separate.
use super::ai_navigation_factory::Record;
use anyhow::{ensure, Result};

#[derive(Debug)]
pub struct PoolNode {
    /// Native node+4 is unwritten while this node belongs to the free list.
    pub child: Option<usize>,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    pub flag: u8,
}
#[derive(Debug)]
pub struct Pool {
    pub nodes: Vec<PoolNode>,
    pub free: Option<usize>,
    pub head: Option<usize>,
    pub tail: Option<usize>,
    pub count: u32,
}
#[derive(Debug)]
pub struct Entity {
    pub identity: usize,
    pub name: Vec<u8>,
    pub common: Record,
    pub values_34_58: [f32; 10],
    pub word_5c: i32,
    pub attributes: Vec<Option<usize>>,
    pub profile_68: Option<usize>,
    pub profile_node_70: Option<usize>,
    pub pool: Pool,
    //190714 replaces the generic external data pointer with its owned pair
    //at7c/80. Store that pair directly; do not retain the caller's address.
    pub user_data: [usize; 2],
    pub radius: f32,
    pub height: f32,
}
impl Entity {
    pub fn new(
        identity: usize,
        name: Option<&[u8]>,
        user_data: [usize; 2],
        attribute_classes: usize,
    ) -> Result<Self> {
        ensure!(
            attribute_classes <= i32::MAX as usize,
            "Navigation entity attribute registry exceeds its native signed-word domain"
        );
        let name = name.unwrap_or(b"none");
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let mut owned_name = Vec::new();
        owned_name.try_reserve_exact(name.len())?;
        owned_name.extend_from_slice(name);
        let mut attributes = Vec::new();
        attributes.try_reserve_exact(attribute_classes)?;
        attributes.resize(attribute_classes, None);
        //Four is the original initial allocation, not an entity/attribute cap.
        //Pool growth and allocation operations must be supplied before use.
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(4)?;
        for index in 0usize..4 {
            nodes.push(PoolNode {
                child: None,
                previous: index.checked_sub(1),
                next: (index < 3).then_some(index + 1),
                flag: 0,
            });
        }
        Ok(Self {
            identity,
            name: owned_name,
            common: Record {
                pool_node: None,
                word_14: 0,
                brain: None,
                flags: 0,
                definition: None,
                word_28: 0,
                world: None,
            },
            values_34_58: [0.; 10],
            word_5c: -1,
            attributes,
            profile_68: None,
            profile_node_70: None,
            pool: Pool {
                nodes,
                free: Some(0),
                head: None,
                tail: None,
                count: 0,
            },
            user_data,
            radius: 0.,
            height: 0.,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn navigation_entity_base_owns_native_constructor_state() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_entity_base.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let name = c["name"].as_array().map(|a| {
                a.iter()
                    .map(|b| b.as_u64().unwrap() as u8)
                    .collect::<Vec<_>>()
            });
            let user_data = std::array::from_fn(|i| c["user"][i].as_u64().unwrap() as usize);
            let entity = Entity::new(
                10,
                name.as_deref(),
                user_data,
                c["attributes"].as_u64().unwrap() as usize,
            )?;
            let r = &entity.common;
            assert_eq!(
                json!({
                    "name":entity.name,
                    "words":[r.pool_node.unwrap_or(0),r.word_14,r.brain.unwrap_or(0),r.flags,
                        r.definition.unwrap_or(0),r.word_28,r.world.unwrap_or(0),entity.word_5c as u32,entity.profile_68.unwrap_or(0),entity.profile_node_70.unwrap_or(0)],
                    "values":entity.values_34_58.map(f32::to_bits),
                    "attributes":entity.attributes.iter().map(|a|a.unwrap_or(0)).collect::<Vec<_>>(),
                    "inline_data":true,"user":entity.user_data,
                    "dimensions":[entity.radius.to_bits(),entity.height.to_bits()],
                    "pool":{"count":entity.pool.nodes.len(),"words":[entity.pool.head.unwrap_or(0),entity.pool.tail.unwrap_or(0),entity.pool.count as usize],"free":entity.pool.free,
                        "nodes":entity.pool.nodes.iter().map(|n|json!({"previous":n.previous,"next":n.next,"flag":n.flag})).collect::<Vec<_>>()}
                }),
                expected["state"],
                "input: {c}"
            );
            assert_eq!(entity.identity, 10);
        }
        assert!(Entity::new(10, None, [0; 2], i32::MAX as usize + 1).is_err());
        Ok(())
    }
}
