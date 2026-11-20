//! Lazy neutral-color mesh copies used only by inspection lighting.
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct Cache {
    buffers: HashMap<usize, (std::sync::Weak<Vec<f32>>, Arc<Vec<f32>>)>,
    misses_since_collection: usize,
}

impl Cache {
    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }

    pub(crate) fn neutralized(&mut self, source: &Arc<Vec<f32>>) -> Arc<Vec<f32>> {
        let key = Arc::as_ptr(source) as usize;
        if let Some((owner, cached)) = self.buffers.get(&key) {
            if owner
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, source))
            {
                return cached.clone();
            }
        }

        let mut vertices = source.as_ref().clone();
        for vertex in vertices.chunks_exact_mut(12) {
            vertex[8..11].fill(1.0);
        }
        let cached = Arc::new(vertices);
        self.buffers
            .insert(key, (Arc::downgrade(source), cached.clone()));
        self.misses_since_collection += 1;
        if self.misses_since_collection == 64 {
            self.collect_dead();
            self.misses_since_collection = 0;
        }
        cached
    }

    pub(crate) fn collect_dead(&mut self) {
        self.buffers
            .retain(|_, (owner, _)| owner.strong_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use std::sync::Arc;

    #[test]
    fn creates_neutral_buffers_on_demand_and_shares_by_source() {
        let source = Arc::new(vec![1., 2., 3., 4., 5., 6., 7., 8., 0.2, 0.4, 0.6, 0.3]);
        let mut cache = Cache::default();
        assert_eq!(cache.len(), 0);

        let first = cache.neutralized(&source);
        assert_eq!(cache.len(), 1);
        assert_eq!(&first[..8], &source[..8]);
        assert_eq!(&first[8..11], &[1., 1., 1.]);
        assert_eq!(first[11], source[11]);
        assert_eq!(&source[8..11], &[0.2, 0.4, 0.6]);

        let second = cache.neutralized(&source);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn discards_a_stale_entry_before_reusing_cache_space() {
        let mut cache = Cache::default();
        let first_source = Arc::new(vec![0.; 12]);
        let first = cache.neutralized(&first_source);
        drop(first_source);
        cache.collect_dead();
        assert_eq!(cache.len(), 0);

        let second_source = Arc::new(vec![2.; 12]);
        let second = cache.neutralized(&second_source);
        assert_eq!(&second[..8], &[2.; 8]);
        assert_eq!(&second[8..11], &[1.; 3]);
        assert_eq!(cache.len(), 1);
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn releases_buffers_when_source_meshes_are_retired() {
        let mut cache = Cache::default();
        let source = Arc::new(vec![0.; 12]);
        cache.neutralized(&source);
        assert_eq!(cache.len(), 1);
        drop(source);
        cache.collect_dead();
        assert_eq!(cache.len(), 0);
    }
}
