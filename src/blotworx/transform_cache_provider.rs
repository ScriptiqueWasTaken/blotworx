//! Owned cache dispatch/base/fallback binding. Skeleton branch implementation
//! is mandatory and receives existing records; unsupported modes don't succeed.
use super::{
    native_matrix::Matrix,
    native_transform_record::{self, Transform},
    transform_attachment, transform_parent_cache,
    transform_storage::{self, Storage},
};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
pub struct Environment<'a> {
    pub records: &'a mut BTreeMap<usize, Vec<u8>>,
    pub components: &'a BTreeMap<u32, usize>,
    pub parents: &'a BTreeMap<usize, Vec<u8>>,
}
pub trait Skeleton {
    fn refresh(
        &mut self,
        storage: &mut Storage<'_>,
        parent: u32,
        mode: i32,
        environment: Environment<'_>,
    ) -> Result<()>;
}
pub struct Constants {
    pub constructor: native_transform_record::Constants,
    pub one: f32,
    pub identity: Matrix,
}
pub struct Provider<'a, S: Skeleton> {
    pub records: &'a mut BTreeMap<usize, Vec<u8>>,
    pub components: &'a BTreeMap<u32, usize>,
    pub parents: &'a BTreeMap<usize, Vec<u8>>,
    pub constants: Constants,
    pub skeleton: S,
}
struct Binding<'a, 's, 'e, S: Skeleton> {
    storage: &'a mut Storage<'s>,
    provider: &'a mut Provider<'e, S>,
}
impl<S: Skeleton> super::transform_update::Cache for Provider<'_, S> {
    fn refresh(&mut self, storage: &mut Storage<'_>, _: u32) -> Result<()> {
        transform_parent_cache::refresh(&mut Binding {
            storage,
            provider: self,
        })
    }
}
impl<S: Skeleton> Binding<'_, '_, '_, S> {
    fn parent_word(&self, t: usize, i: usize) -> Result<u32> {
        transform_storage::world_word(
            self.provider
                .parents
                .get(&t)
                .with_context(|| format!("Missing cache parent Transform {t}"))?,
            i,
        )
    }
}
impl<S: Skeleton> transform_parent_cache::Backend for Binding<'_, '_, '_, S> {
    fn parent(&mut self) -> Result<u32> {
        Ok(self.storage.parent_entity())
    }
    fn cached_record_11c(&mut self) -> Result<usize> {
        Ok(self.storage.cache_record())
    }
    fn store_byte(&mut self, o: usize, v: u8) -> Result<()> {
        self.storage.store_byte(o, v)
    }
    fn record_word(&mut self, r: usize, o: usize) -> Result<u32> {
        let record = self
            .provider
            .records
            .get(&r)
            .with_context(|| format!("Missing parent cache record {r}"))?;
        let bytes = record
            .get(o..o + 4)
            .with_context(|| format!("Truncated parent cache record {r}"))?;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    }
    fn attached_354a14(&mut self, _: usize, p: u32) -> Result<()> {
        transform_attachment::refresh(p, self)
    }
    fn parent_transform(&mut self, p: u32) -> Result<usize> {
        self.provider
            .components
            .get(&p)
            .copied()
            .with_context(|| format!("Cache parent entity {p} lacks required Transform"))
    }
    fn one_4cd748(&mut self) -> Result<f32> {
        Ok(self.provider.constants.one)
    }
    fn parent_position(&mut self, t: usize) -> Result<[f32; 3]> {
        let mut v = [0.; 3];
        for i in 0..3 {
            v[i] = f32::from_bits(self.parent_word(t, 9 + i)?);
        }
        Ok(v)
    }
    fn parent_rotation(&mut self, t: usize) -> Result<Matrix> {
        let mut v = [0.; 9];
        for i in 0..9 {
            v[i] = f32::from_bits(self.parent_word(t, i)?);
        }
        Ok(v)
    }
    fn default_transform_804c026c(&mut self) -> Result<Transform> {
        let mut bytes = [0; 0x34];
        native_transform_record::reset(&mut bytes, self.provider.constants.constructor)?;
        Ok(std::array::from_fn(|i| {
            f32::from_bits(u32::from_be_bytes(
                bytes[i * 4..i * 4 + 4].try_into().unwrap(),
            ))
        }))
    }
    fn mode_353ae8(&mut self) -> Result<i32> {
        Ok(self.storage.attachment_mode())
    }
    fn identity_80767b64(&mut self) -> Result<Matrix> {
        Ok(self.provider.constants.identity)
    }
    fn store_cached_parent_e8(&mut self, v: Transform) -> Result<()> {
        self.storage.store_cached_parent(v);
        Ok(())
    }
}
impl<S: Skeleton> transform_attachment::Backend for Binding<'_, '_, '_, S> {
    fn store_translation_override_126(&mut self, v: u8) -> Result<()> {
        self.storage.store_byte(0x126, v)
    }
    fn mode_353ae8(&mut self) -> Result<i32> {
        Ok(self.storage.attachment_mode())
    }
    fn copy_parent_pose_296f98(&mut self, p: u32) -> Result<()> {
        super::entity_pose_copy::copy(
            p,
            &mut super::entity_pose_copy::Owned {
                destination: self.storage,
                components: self.provider.components,
                records: self.provider.parents,
            },
        )
    }
    fn one_4cd748(&mut self) -> Result<f32> {
        Ok(self.provider.constants.one)
    }
    fn store_cache_scale_118(&mut self, v: f32) -> Result<()> {
        self.storage.store_cache_word(12, v.to_bits())
    }
    fn skeletal_354a5c(&mut self, p: u32, m: i32) -> Result<()> {
        self.provider.skeleton.refresh(
            self.storage,
            p,
            m,
            Environment {
                records: &mut *self.provider.records,
                components: self.provider.components,
                parents: self.provider.parents,
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::super::{transform_runtime::Runtime, transform_storage::Space, transform_update};
    use super::*;
    struct MissingSkeleton;
    impl Skeleton for MissingSkeleton {
        fn refresh(
            &mut self,
            _: &mut Storage<'_>,
            _: u32,
            _: i32,
            _: Environment<'_>,
        ) -> Result<()> {
            anyhow::bail!("Skeletal branch unavailable")
        }
    }
    #[test]
    fn owned_cache_refresh_rebuilds_world_and_missing_skeleton_keeps_dirty() -> Result<()> {
        let id = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut parent = vec![0; 0x140];
        {
            let mut s = Storage::new(&mut parent)?;
            s.store_position(Space::World, [2., 4., 6.]);
            s.store_rotation(Space::World, id);
            s.store_world_scale(9.);
        }
        let components = BTreeMap::from([(70, 700)]);
        let parents = BTreeMap::from([(700, parent)]);
        let mut record = vec![0; 0x3c];
        record[0x38..0x3c].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut records = BTreeMap::from([(100, record)]);
        let mut bytes = [0; 0x140];
        bytes[0xb4..0xb8].copy_from_slice(&70u32.to_be_bytes());
        bytes[0x11c..0x120].copy_from_slice(&100u32.to_be_bytes());
        bytes[0x124] = 1;
        let mut storage = Storage::new(&mut bytes)?;
        storage.store_position(Space::Local, [1., 2., 3.]);
        storage.store_rotation(Space::Local, id);
        storage.store_world_scale(1.);
        let mut children = super::super::transform_child_list::Children::default();
        let mut resolver = super::super::entity_handle_lookup::Table::initialized(8)?;
        let child_components = BTreeMap::new();
        let mut child_records = BTreeMap::new();
        let mut related_records = BTreeMap::new();
        let hooks = super::super::transform_record_hooks::Hooks {
            children: &mut children,
            resolver: &mut resolver,
            components: &child_components,
            child_records: &mut child_records,
            related_records: &mut related_records,
        };
        let mut runtime = Runtime {
            storage,
            hooks,
            constants: super::super::transform_runtime::Constants {
                zero: 0.,
                one: 1.,
                identity: id,
            },
        };
        let provider = Provider {
            records: &mut records,
            components: &components,
            parents: &parents,
            constants: Constants {
                constructor: native_transform_record::Constants {
                    matrix_one: 0x3f800000,
                    matrix_zero: 0,
                    translation: [0; 3],
                    scale: 0x3f800000,
                },
                one: 1.,
                identity: id,
            },
            skeleton: MissingSkeleton,
        };
        let mut gate = transform_update::Owned {
            runtime: &mut runtime,
            cache: provider,
        };
        assert!(transform_update::update(&mut gate)?);
        assert_eq!(gate.runtime.storage.position(Space::World), [3., 6., 9.]);
        assert_eq!(gate.runtime.storage.cached_parent()[12], 1.);
        assert_eq!(
            (
                gate.runtime.storage.byte(0x124)?,
                gate.runtime.storage.byte(0x125)?
            ),
            (0, 1)
        );
        gate.cache.records.get_mut(&100).unwrap()[0x38..0x3c].copy_from_slice(&0u32.to_be_bytes());
        gate.runtime.storage.store_byte(0x124, 1)?;
        let before = gate.runtime.storage.cached_parent();
        assert!(transform_update::update(&mut gate).is_err());
        assert_eq!(gate.runtime.storage.cached_parent(), before);
        assert_eq!(gate.runtime.storage.byte(0x124)?, 1);
        assert_eq!(gate.runtime.storage.byte(0x126)?, 1);
        Ok(())
    }
}
