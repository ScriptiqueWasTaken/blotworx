//! Entity world pose copy296f98. Matrix words are copied in native paired
//! load/store order; translation precedes rotation and scale comes last.
use anyhow::{Context, Result};
use std::collections::BTreeMap;
pub trait Backend {
    fn transform_296b88(&mut self, entity: u32) -> Result<usize>;
    fn world_word(&mut self, transform: usize, index: usize) -> Result<u32>;
    fn store_word(&mut self, index: usize, bits: u32) -> Result<()>;
}
pub fn copy(entity: u32, backend: &mut impl Backend) -> Result<()> {
    let transform = backend.transform_296b88(entity)?;
    for index in 9..12 {
        let word = backend.world_word(transform, index)?;
        backend.store_word(index, word)?;
    }
    for index in (0..8).step_by(2) {
        let first = backend.world_word(transform, index)?;
        let second = backend.world_word(transform, index + 1)?;
        backend.store_word(index + 1, second)?;
        backend.store_word(index, first)?;
    }
    let last = backend.world_word(transform, 8)?;
    backend.store_word(8, last)?;
    let scale = backend.world_word(transform, 12)?;
    backend.store_word(12, scale)
}
pub struct Owned<'a, 's> {
    pub destination: &'a mut super::transform_storage::Storage<'s>,
    pub components: &'a BTreeMap<u32, usize>,
    pub records: &'a BTreeMap<usize, Vec<u8>>,
}
impl Backend for Owned<'_, '_> {
    fn transform_296b88(&mut self, e: u32) -> Result<usize> {
        self.components
            .get(&e)
            .copied()
            .with_context(|| format!("Parent entity {e} lacks required Transform"))
    }
    fn world_word(&mut self, t: usize, i: usize) -> Result<u32> {
        super::transform_storage::world_word(
            self.records
                .get(&t)
                .with_context(|| format!("Missing parent Transform {t}"))?,
            i,
        )
    }
    fn store_word(&mut self, i: usize, b: u32) -> Result<()> {
        self.destination.store_cache_word(i, b)
    }
}
#[cfg(test)]
mod tests {
    use super::super::transform_storage::{Space, Storage};
    use super::*;
    #[test]
    fn owned_parent_copy_retains_words_and_partial_matrix_publication() -> Result<()> {
        let mut parent = vec![0; 0x140];
        let rotation = [1., -0., 0., 0., 1., 0., 0., 0., 1.];
        {
            let mut s = Storage::new(&mut parent)?;
            s.store_position(Space::World, [2., 4., 6.]);
            s.store_rotation(Space::World, rotation);
            s.store_world_scale(9.);
        }
        let components = BTreeMap::from([(70, 700)]);
        let records = BTreeMap::from([(700, parent.clone())]);
        let mut destination = [0xa5; 0x140];
        {
            let mut s = Storage::new(&mut destination)?;
            copy(
                70,
                &mut Owned {
                    destination: &mut s,
                    components: &components,
                    records: &records,
                },
            )?;
            let value = s.cached_parent();
            assert_eq!(&value[9..], [2., 4., 6., 9.]);
            assert_eq!(value[1].to_bits(), 0x80000000);
            assert_eq!(&value[..9], &rotation);
        }
        assert_eq!(records[&700], parent);
        assert!(destination[..0xe8].iter().all(|b| *b == 0xa5));
        assert!(destination[0x11c..].iter().all(|b| *b == 0xa5));
        let records = BTreeMap::from([(700, parent[..0x90].to_vec())]);
        let mut destination = [0xa5; 0x140];
        let mut s = Storage::new(&mut destination)?;
        assert!(copy(
            70,
            &mut Owned {
                destination: &mut s,
                components: &components,
                records: &records
            }
        )
        .is_err());
        let value = s.cached_parent();
        assert_eq!(&value[9..12], [2., 4., 6.]);
        assert_eq!(value[0], 1.);
        assert_eq!(value[1].to_bits(), 0x80000000);
        assert_eq!(value[2].to_bits(), 0xa5a5a5a5);
        assert_eq!(value[12].to_bits(), 0xa5a5a5a5);
        Ok(())
    }
}
