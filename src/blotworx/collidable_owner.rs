//! Entity ownership helpers REL242ebc/242ee8, including nested collidables.
use anyhow::{ensure, Result};
use std::collections::BTreeSet;

pub struct Collidable {
    pub has_shape: bool,
    pub owner_offset: i8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserData {
    Missing,
    Entity(usize),
    Embedded,
}
/// Known activation vectors at userdata+50/+60. Raw words preserve W,
/// signed zero and payload bits; absent means not initialized, not zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationCache {
    pub linear: [u32; 4],
    pub angular: [u32; 4],
}
/// Loader userdata+40/+70. Point comes from the entity's virtual2b0 call;
/// matrix is captured from motion BEFORE entity placement (382c58..cd0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicBinding {
    pub point: [u32; 4],
    pub local_transform: [u32; 16],
}
impl DynamicBinding {
    pub fn new(point: [f32; 3], mut transform: [f32; 16]) -> Result<Self> {
        ensure!(
            point
                .iter()
                .chain(&transform)
                .all(|value| value.is_finite()),
            "Nonfinite dynamic userdata binding"
        );
        //23bc74 zeroes rotation padding only; translation W is retained.
        for index in [3, 7, 11] {
            transform[index] = 0.;
        }
        Ok(Self {
            point: [
                point[0].to_bits(),
                point[1].to_bits(),
                point[2].to_bits(),
                0,
            ],
            local_transform: transform.map(f32::to_bits),
        })
    }
}
/// Resolver-visible prefix of the176-byte native physics userdata allocation.
/// Activation vectors are retained below; other contact/transform fields remain pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityData {
    pub entity: Option<usize>,
    pub word_04: u32,
    pub byte_08: u8,
    pub byte_09: u8,
    pub word_0c: u32,
    pub word_10: u32,
    pub activation_cache: Option<ActivationCache>,
    pub dynamic_binding: Option<DynamicBinding>,
}
impl EntityData {
    /// Dynamic loader REL382eb4..382f64. Prefix flags use r27=1,
    /// r23=0;3734f4 copies four positive-zero lanes into +50/+60.
    /// Other userdata vectors/transform belong to subsequent binding stages.
    pub fn dynamic(entity: Option<usize>) -> Self {
        Self {
            entity,
            word_04: 0,
            byte_08: 1,
            byte_09: 1,
            word_0c: 0,
            word_10: 0,
            activation_cache: Some(ActivationCache {
                linear: [0; 4],
                angular: [0; 4],
            }),
            dynamic_binding: None,
        }
    }
    /// REL377cf8..377d68 common constructor and phantom owner assignment.
    pub fn phantom(entity: Option<usize>) -> Self {
        Self {
            entity,
            word_04: 0,
            byte_08: 0,
            byte_09: 1,
            word_0c: 0,
            word_10: 0,
            activation_cache: None,
            dynamic_binding: None,
        }
    }
    /// REL379cc4..379d4c reserves shared group r31, packs it at379dc0 and
    /// retains the same full word at userdata+c (37a0cc). Reservation occurs
    /// earlier than userdata allocation, so the caller must supply that ID.
    pub fn character(entity: Option<usize>, group: u32) -> Self {
        Self {
            entity,
            word_04: 0,
            byte_08: 1,
            byte_09: 0,
            word_0c: group,
            word_10: 1,
            activation_cache: None,
            dynamic_binding: None,
        }
    }
    pub fn owner(&self) -> UserData {
        match self.entity {
            Some(entity) if entity != 0 => UserData::Entity(entity),
            _ => UserData::Embedded,
        }
    }
    /// Group assignment297ddc/restore298088 use only these native words.
    pub fn group_word(&self, offset: usize) -> Result<u32> {
        match offset {
            0xc => Ok(self.word_0c),
            0x10 => Ok(self.word_10),
            _ => anyhow::bail!("Unsupported physics userdata group word {offset:x}"),
        }
    }
    pub fn store_group_word(&mut self, offset: usize, value: u32) -> Result<()> {
        match offset {
            0xc => self.word_0c = value,
            0x10 => self.word_10 = value,
            _ => anyhow::bail!("Unsupported physics userdata group word {offset:x}"),
        }
        Ok(())
    }
}
/// The world supplies reconstructed records, not emulated Wii memory.
pub trait Records {
    fn collidable(&self, address: usize) -> Result<Collidable>;
    /// Owner+0xc userdata: absent pointer, nonzero first entity pointer,
    /// or present userdata whose first pointer is zero (embedded fallback).
    fn user_data(&self, owner: usize) -> Result<UserData>;
}
pub fn from_collidable(address: usize, records: &impl Records) -> Result<Option<usize>> {
    let Some(owner) = owner_address(address, records)? else {
        return Ok(None);
    };
    from_owner(owner, records)
}
fn owner_address(address: usize, records: &impl Records) -> Result<Option<usize>> {
    if address == 0 {
        return Ok(None);
    }
    let c = records.collidable(address)?;
    if !c.has_shape {
        return Ok(None);
    }
    Ok(Some(
        address
            .checked_add_signed(c.owner_offset as isize)
            .ok_or_else(|| anyhow::anyhow!("Collidable owner offset overflows"))?,
    ))
}
pub fn from_owner(mut owner: usize, records: &impl Records) -> Result<Option<usize>> {
    let mut visited = BTreeSet::new();
    loop {
        if owner == 0 {
            return Ok(None);
        }
        ensure!(
            visited.insert(owner),
            "Cyclic embedded collidable ownership"
        );
        match records.user_data(owner)? {
            UserData::Missing => return Ok(None),
            UserData::Entity(entity) => {
                ensure!(
                    entity != 0,
                    "Zero entity pointer must use embedded ownership fallback"
                );
                return Ok(Some(entity));
            }
            UserData::Embedded => {
                let address = owner
                    .checked_add(0x10)
                    .ok_or_else(|| anyhow::anyhow!("Embedded collidable address overflows"))?;
                let Some(next) = owner_address(address, records)? else {
                    return Ok(None);
                };
                owner = next;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn userdata_prefix_preserves_phantom_character_defaults_and_null_first_pointer() {
        let phantom = EntityData::phantom(Some(71));
        let character = EntityData::character(Some(81), 0xabcdef12);
        assert_eq!(
            (
                phantom.word_04,
                phantom.byte_08,
                phantom.byte_09,
                phantom.word_0c,
                phantom.word_10
            ),
            (0, 0, 1, 0, 0)
        );
        assert_eq!(
            (
                character.word_04,
                character.byte_08,
                character.byte_09,
                character.word_0c,
                character.word_10
            ),
            (0, 1, 0, 0xabcdef12, 1)
        );
        assert_eq!(phantom.owner(), UserData::Entity(71));
        assert_eq!(character.owner(), UserData::Entity(81));
        assert_eq!(EntityData::phantom(None).owner(), UserData::Embedded);
        assert_eq!(
            EntityData::character(Some(0), 0).owner(),
            UserData::Embedded
        );
    }
    struct Fixture;
    impl Records for Fixture {
        fn collidable(&self, address: usize) -> Result<Collidable> {
            match address {
                108 => Ok(Collidable {
                    has_shape: true,
                    owner_offset: -8,
                }),
                116 => Ok(Collidable {
                    has_shape: true,
                    owner_offset: 84,
                }),
                316 => Ok(Collidable {
                    has_shape: true,
                    owner_offset: -16,
                }),
                400 => Ok(Collidable {
                    has_shape: false,
                    owner_offset: 0,
                }),
                _ => anyhow::bail!("Missing collidable record {address}"),
            }
        }
        fn user_data(&self, owner: usize) -> Result<UserData> {
            match owner {
                100 | 300 => Ok(UserData::Embedded),
                200 => Ok(UserData::Entity(7)),
                500 => Ok(UserData::Missing),
                _ => anyhow::bail!("Missing owner record"),
            }
        }
    }
    #[test]
    fn signed_offsets_nested_fallback_and_null_guards() -> Result<()> {
        assert_eq!(from_collidable(108, &Fixture)?, Some(7));
        assert_eq!(from_owner(200, &Fixture)?, Some(7));
        assert_eq!(from_owner(500, &Fixture)?, None);
        assert_eq!(from_collidable(0, &Fixture)?, None);
        assert_eq!(from_owner(0, &Fixture)?, None);
        assert_eq!(from_collidable(400, &Fixture)?, None);
        Ok(())
    }
    #[test]
    fn malformed_records_never_invent_entities() {
        assert!(from_owner(300, &Fixture)
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
        assert!(from_collidable(900, &Fixture).is_err());
    }
}
