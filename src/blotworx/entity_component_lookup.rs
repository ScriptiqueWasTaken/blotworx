//! Actor component lookup REL 0xe14/0xe34 and inherited slot lookup 0xebc.
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

/// BSS27c48/27c50 descriptor, named at4b448a by141fac. IsCharacter
/// queries this typed component, not physics shape or actor/AI membership.
pub const CHARACTER_COMPONENT: &str = "MickeyPlayerMotionControllerComponent";
pub fn has_character_component<'a>(types: impl IntoIterator<Item=&'a str>) -> bool {
    types.into_iter().any(|name|name==CHARACTER_COMPONENT)
}


/// Runtime records, not serialized scene component order. Interfaces refer
/// to native component +8; the getter returns the owning component token.
pub struct Entity {
    pub metadata: usize,
    pub slots: Option<Vec<u8>>,
    pub interfaces: Vec<u32>,
}
pub struct Metadata {
    pub inherited: usize,
    pub slots: Option<Vec<u8>>,
}
pub struct Storage {
    pub entities: BTreeMap<u32, Entity>,
    pub metadata: BTreeMap<usize, Metadata>,
}
impl Storage {
    fn slot(&self, mut metadata: usize, type_index: u8) -> Result<u8> {
        let mut visited = BTreeSet::new();
        loop {
            anyhow::ensure!(
                visited.insert(metadata),
                "Cycle in inherited component metadata"
            );
            let record = self
                .metadata
                .get(&metadata)
                .with_context(|| format!("Missing component metadata {metadata}"))?;
            if let Some(slots) = &record.slots {
                return slots
                    .get(type_index as usize)
                    .copied()
                    .context("Component type outside metadata slot table");
            }
            metadata = record.inherited;
        }
    }
    /// `type_index` is the runtime descriptor byte (actor uses REL 0x26fe0),
    /// never an inferred serialized component-array position.
    pub fn component(&self, entity: u32, type_index: u8) -> Result<Option<u32>> {
        if entity == 0 {
            return Ok(None);
        }
        let entity = self
            .entities
            .get(&entity)
            .context("Missing runtime entity component table")?;
        let slot = if let Some(slots) = &entity.slots {
            *slots
                .get(type_index as usize)
                .context("Component type outside entity slot table")?
        } else {
            self.slot(entity.metadata, type_index)?
        };
        if slot == 0xff {
            return Ok(None);
        }
        let interface = *entity
            .interfaces
            .get(slot as usize)
            .context("Missing runtime component interface slot")?;
        Ok((interface != 0).then(|| interface.wrapping_sub(8)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_is_character_slot_lookup_matches() -> Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/character_query.json"))?;
        assert_eq!(fixture["component_name"],CHARACTER_COMPONENT);
        let mut count=0;
        for row in fixture["cases"].as_array().unwrap() {
            let c=&row["input"];
            let index=c["type_index"].as_u64().unwrap() as usize;
            let mut slots=vec![0xff;256];slots[index]=c["slot"].as_u64().unwrap() as u8;
            let depth=c["metadata_depth"].as_u64().map(|v|v as usize);
            let mut metadata=BTreeMap::new();
            if let Some(depth)=depth {
                for level in 0..=depth {
                    metadata.insert(level+1,Metadata{inherited:level+2,slots:(level==depth).then(||slots.clone())});
                }
            }
            let storage=Storage { entities:BTreeMap::from([(1,Entity {
                metadata:1,slots:depth.is_none().then_some(slots),
                interfaces:vec![if c["interface_present"].as_bool().unwrap(){1008}else{0};3],
            })]),metadata};
            let entity=if c["exists"].as_bool().unwrap(){1}else{0};
            assert_eq!(storage.component(entity,index as u8)?.is_some(),row["result"].as_bool().unwrap(),"{c}");
            count+=1;
        }
        assert!(has_character_component(["MickeyPlayerMotionControllerComponent"]));
        assert!(!has_character_component(["BasicMotionControllerComponent","PhysicsRigidBodyCharacterComponent","AIBrainComponent"]));
        assert!(!has_character_component(std::iter::empty()));
        assert_eq!(count,180);
        println!("Matched {count} original IsCharacter slot/inheritance cases");
        Ok(())
    }

    #[test]
    fn entity_component_lookup_uses_inherited_byte_slots_and_nulls() -> Result<()> {
        let mut storage = Storage {
            entities: BTreeMap::from([(
                70,
                Entity {
                    metadata: 1,
                    slots: None,
                    interfaces: vec![0, 1008, 2008],
                },
            )]),
            metadata: BTreeMap::from([
                (
                    1,
                    Metadata {
                        inherited: 2,
                        slots: None,
                    },
                ),
                (
                    2,
                    Metadata {
                        inherited: 3,
                        slots: None,
                    },
                ),
                (
                    3,
                    Metadata {
                        inherited: 0,
                        slots: Some(vec![2, 0xff, 0, 1]),
                    },
                ),
            ]),
        };
        assert_eq!(storage.component(0, 255)?, None);
        assert_eq!(storage.component(70, 0)?, Some(2000));
        assert_eq!(storage.component(70, 1)?, None);
        assert_eq!(storage.component(70, 2)?, None);
        assert_eq!(storage.component(70, 3)?, Some(1000));
        storage.entities.get_mut(&70).unwrap().slots = Some(vec![1, 0xff]);
        assert_eq!(storage.component(70, 0)?, Some(1000));
        assert_eq!(storage.component(70, 1)?, None);
        storage.entities.get_mut(&70).unwrap().slots = None;
        storage.metadata.get_mut(&1).unwrap().slots = Some(vec![0xff]);
        assert_eq!(storage.component(70, 0)?, None); // no fallback for sentinel
        assert!(storage.component(70, 1).is_err());
        storage.metadata.get_mut(&1).unwrap().slots = None;
        storage.metadata.get_mut(&3).unwrap().slots = None;
        storage.metadata.get_mut(&3).unwrap().inherited = 1;
        assert!(storage.component(70, 0).is_err());
        Ok(())
    }
}
