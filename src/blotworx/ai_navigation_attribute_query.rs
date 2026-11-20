//!45f994 existing entity-attribute lookup. Exact mode does not construct,
//! attach or validate the stored object's class. Creation is separate45fa78.
use super::ai_navigation_entity_base::Entity;
use anyhow::{ensure, Result};
pub trait Host {
    fn class_index(&mut self, entity: &mut Entity, class: usize) -> Result<i32>;
    fn attribute_class(&mut self, entity: &mut Entity, attribute: usize) -> Result<usize>;
    fn parent_class(&mut self, entity: &mut Entity, class: usize) -> Result<Option<usize>>;
}
pub fn query(
    entity: &mut Entity,
    class: usize,
    exact: bool,
    host: &mut impl Host,
) -> Result<Option<usize>> {
    if exact {
        let index = host.class_index(entity, class)?;
        if index < 0 {
            return Ok(None);
        }
        ensure!(
            entity.attributes.len() <= i32::MAX as usize,
            "Entity attribute count exceeds native signed domain"
        );
        return Ok(entity.attributes.get(index as usize).copied().flatten());
    }
    let mut index = 0usize;
    loop {
        ensure!(
            entity.attributes.len() <= i32::MAX as usize,
            "Entity attribute count exceeds native signed domain"
        );
        if index >= entity.attributes.len() {
            return Ok(None);
        }
        if let Some(attribute) = entity.attributes[index] {
            // Native r29 retains the attribute even if its virtual Class query
            // changes/removes the original entity slot.
            let mut metadata = host.attribute_class(entity, attribute)?;
            let mut visited = std::collections::HashSet::new();
            loop {
                if metadata == class {
                    return Ok(Some(attribute));
                }
                ensure!(
                    visited.insert(metadata),
                    "Cycle in navigation attribute class ancestry"
                );
                let Some(parent) = host.parent_class(entity, metadata)? else {
                    break;
                };
                metadata = parent;
            }
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        mutated: bool,
        cyclic: bool,
    }
    impl Host for Binding<'_> {
        fn class_index(&mut self, entity: &mut Entity, class: usize) -> Result<i32> {
            self.trace.push(json!(["index", class]));
            if self.case["mutation"] == "index" && !self.mutated {
                entity.attributes = vec![Some(101)];
                self.mutated = true;
            }
            Ok(self.case["index"].as_i64().unwrap() as i32)
        }
        fn attribute_class(&mut self, entity: &mut Entity, attribute: usize) -> Result<usize> {
            self.trace.push(json!(["class", attribute]));
            if !self.mutated {
                if self.case["mutation"] == "replace" {
                    entity.attributes[0] = Some(104);
                    self.mutated = true;
                }
                if self.case["mutation"] == "append" {
                    entity.attributes.push(Some(102));
                    self.mutated = true;
                }
                if self.case["mutation"] == "shrink" {
                    entity.attributes.clear();
                    self.mutated = true;
                }
            }
            Ok(match attribute {
                101 => 11,
                102 | 104 => 20,
                103 => 12,
                _ => anyhow::bail!("Unknown attribute instance"),
            })
        }
        fn parent_class(&mut self, _: &mut Entity, class: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["parent", class]));
            Ok(match class {
                11 => Some(if self.cyclic { 11 } else { 10 }),
                12 => Some(11),
                10 | 20 | 30 => None,
                _ => anyhow::bail!("Unknown metadata class"),
            })
        }
    }
    #[test]
    fn navigation_attribute_query_matches_original_exact_slots_ancestry_and_live_mutations(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_attribute_query.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let slots: Vec<Option<usize>> = serde_json::from_value(c["slots"].clone())?;
            let mut entity = Entity::new(10, None, [0; 2], slots.len())?;
            entity.attributes = slots;
            let mut host = Binding {
                case: c,
                trace: vec![],
                mutated: false,
                cyclic: false,
            };
            let result = query(
                &mut entity,
                c["class"].as_u64().unwrap() as usize,
                c["flag"].as_u64().unwrap() != 0,
                &mut host,
            )?;
            assert_eq!(
                json!({"result":result,"trace":host.trace,"slots":entity.attributes}),
                json!({"result":expected["result"],"trace":expected["trace"],"slots":expected["slots"]}),
                "{c}"
            );
        }
        let c = json!({"index":0});
        let mut entity = Entity::new(10, None, [0; 2], 1)?;
        entity.attributes[0] = Some(101);
        let mut host = Binding {
            case: &c,
            trace: vec![],
            mutated: false,
            cyclic: true,
        };
        assert!(query(&mut entity, 30, false, &mut host)
            .unwrap_err()
            .to_string()
            .contains("Cycle"));
        Ok(())
    }
}
