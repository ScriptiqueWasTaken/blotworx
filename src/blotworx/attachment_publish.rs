//! Final attachment composition/publication REL 0x35572c..0x355954.
use super::native_transform_record::Transform;
use anyhow::Result;
pub trait Backend {
    fn store_cache_word(&mut self, index: usize, bits: u32) -> Result<()>;
    fn one(&mut self) -> Result<f32>;
}
/// `work` must already contain mode-specific rotation/offset changes.
/// Intermediate product scale is published before the source overwrites it.
pub fn publish(
    parent: Transform,
    work: Transform,
    prepared: Transform,
    attachment_scale: f32,
    backend: &mut impl Backend,
) -> Result<()> {
    let position = work[9..12].try_into().unwrap();
    let rotation = work[..9].try_into().unwrap();
    let (position, rotation) =
        super::native_transform_record::world_pose(&parent, position, &rotation);
    let result: Transform = std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            position[i - 9]
        } else {
            parent[12] * work[12]
        }
    });
    for (index, value) in result.into_iter().enumerate() {
        backend.store_cache_word(index, value.to_bits())?;
    }
    let scale = if attachment_scale > f32::EPSILON {
        prepared[12]
    } else {
        backend.one()?
    };
    backend.store_cache_word(12, scale.to_bits())
}

pub struct Owned<'a, 's> {
    pub storage: &'a mut super::transform_storage::Storage<'s>,
    pub one: f32,
}
impl Backend for Owned<'_, '_> {
    fn store_cache_word(&mut self, i: usize, b: u32) -> Result<()> {
        self.storage.store_cache_word(i, b)
    }
    fn one(&mut self) -> Result<f32> {
        Ok(self.one)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_publish_composes_then_overwrites_scale_in_order() -> Result<()> {
        let parent = [1., 0., 0., 0., 1., 0., 0., 0., 1., 10., 20., 30., 2.];
        let work = [1., 0., 0., 0., 1., 0., 0., 0., 1., 1., 2., 3., 7.];
        struct Trace {
            writes: Vec<(usize, f32)>,
            fail: bool,
        }
        impl Backend for Trace {
            fn store_cache_word(&mut self, i: usize, b: u32) -> Result<()> {
                self.writes.push((i, f32::from_bits(b)));
                Ok(())
            }
            fn one(&mut self) -> Result<f32> {
                if self.fail {
                    anyhow::bail!("Missing one")
                } else {
                    Ok(1.)
                }
            }
        }
        let mut trace = Trace {
            writes: vec![],
            fail: false,
        };
        publish(parent, work, work, 2., &mut trace)?;
        assert_eq!(
            &trace.writes[9..],
            &[(9, 12.), (10, 24.), (11, 36.), (12, 14.), (12, 7.)]
        );
        trace.writes.clear();
        trace.fail = true;
        assert!(publish(parent, work, work, 0., &mut trace).is_err());
        assert_eq!(trace.writes.len(), 13);
        assert_eq!(trace.writes[12], (12, 14.));
        let mut bytes = [0xa5; 0x140];
        let mut storage = super::super::transform_storage::Storage::new(&mut bytes)?;
        publish(
            parent,
            work,
            work,
            0.,
            &mut Owned {
                storage: &mut storage,
                one: 1.,
            },
        )?;
        let result = storage.cached_parent();
        assert_eq!(&result[9..], &[12., 24., 36., 1.]);
        assert_eq!(&bytes[..0xe8], &[0xa5; 0xe8]);
        assert_eq!(&bytes[0x11c..], &[0xa5; 0x24]);
        Ok(())
    }
}
