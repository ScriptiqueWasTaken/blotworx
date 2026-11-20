//! Opaque Lua identities. World slots may be reused after loading, tokens may not.
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Registry {
    by_entity: BTreeMap<usize, usize>,
    by_token: BTreeMap<usize, usize>,
    next: usize,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            by_entity: BTreeMap::new(),
            by_token: BTreeMap::new(),
            next: 1,
        }
    }
}
impl Registry {
    /// Preserve the initial bridge's identities; subsequent worlds allocate
    /// from the retained high-water mark instead of encoding their slots.
    pub fn initial(entities: impl IntoIterator<Item = usize>) -> Result<Self, &'static str> {
        let mut result = Self::default();
        for entity in entities {
            let token = entity.checked_add(1).ok_or("Entity handle overflow")?;
            let next = token
                .checked_add(1)
                .ok_or("Entity handle space exhausted")?;
            if result.by_entity.insert(entity, token).is_some() {
                return Err("Duplicate entity handle");
            }
            result.by_token.insert(token, entity);
            result.next = result.next.max(next);
        }
        Ok(result)
    }
    pub fn register(&mut self, entity: usize) -> Result<usize, &'static str> {
        if self.by_entity.contains_key(&entity) {
            return Err("Entity handle already registered");
        }
        let token = self.next;
        self.next = token
            .checked_add(1)
            .ok_or("Entity handle space exhausted")?;
        self.by_entity.insert(entity, token);
        self.by_token.insert(token, entity);
        Ok(token)
    }
    pub fn token(&self, entity: usize) -> Option<usize> {
        self.by_entity.get(&entity).copied()
    }
    pub fn resolve(&self, token: usize) -> Option<usize> {
        self.by_token.get(&token).copied()
    }
    pub fn retire(&mut self, entity: usize) {
        // Context getters can still expose a retired identity. Resolution
        // rejects it, while the tiny issued-token record lives until reload.
        if let Some(token) = self.by_entity.get(&entity) {
            self.by_token.remove(token);
        }
    }
    /// Transactional replacement: only explicitly retained objects keep their
    /// identities. Reusing a numerical world slot never revives a stale token.
    pub fn replace_world(
        &mut self,
        entities: impl IntoIterator<Item = (usize, Option<usize>)>,
    ) -> Result<(), &'static str> {
        let mut next = Self {
            next: self.next,
            ..Self::default()
        };
        for (entity, retained) in entities {
            if next.by_entity.contains_key(&entity) {
                return Err("Duplicate replacement entity");
            }
            if let Some(old) = retained {
                let token = self.token(old).ok_or("Retained entity is not live")?;
                if self.resolve(token) != Some(old) {
                    return Err("Retained entity was retired");
                }
                if next.by_token.insert(token, entity).is_some() {
                    return Err("Retained handle assigned twice");
                }
                next.by_entity.insert(entity, token);
            } else {
                next.register(entity)?;
            }
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_world_invalidates_reused_slots_but_retains_explicit_identity() {
        let mut r = Registry::initial([0, 1, 7]).unwrap();
        let old = r.token(1).unwrap();
        let player = r.token(7).unwrap();
        r.replace_world([(0, None), (1, None), (3, Some(7))])
            .unwrap();
        assert_eq!(r.resolve(old), None);
        assert_eq!(r.resolve(player), Some(3));
        assert_ne!(r.token(1), Some(old));
        assert_eq!(r.resolve(0), None);
        r.retire(3);
        assert_eq!(
            r.token(3),
            Some(player),
            "Retired callback context retains its opaque identity"
        );
        assert_eq!(r.resolve(player), None);
        assert!(r.replace_world([(0, Some(3))]).is_err());
        r.replace_world([(3, None)]).unwrap();
        assert_ne!(r.token(3), Some(player));
    }
    #[test]
    fn failed_replacement_is_transactional_and_repeated_loads_do_not_revive_tokens() {
        let mut r = Registry::initial([0]).unwrap();
        let first = r.token(0).unwrap();
        assert!(r.replace_world([(1, Some(0)), (2, Some(0))]).is_err());
        assert_eq!(r.resolve(first), Some(0));
        for _ in 0..1000 {
            r.replace_world([(0, None)]).unwrap();
            assert_eq!(r.resolve(first), None);
        }
        assert_eq!(r.by_entity.len(), 1);
        assert_eq!(r.by_token.len(), 1);
    }
    #[test]
    fn sparse_slots_do_not_define_later_opaque_tokens() {
        let mut r = Registry::initial([0, 7]).unwrap();
        let token = r.register(100).unwrap();
        assert_ne!(token, 101);
        assert_eq!(r.resolve(token), Some(100));
        r.retire(100);
        assert_eq!(r.resolve(token), None);
        assert!(r.register(100).is_err());
    }
}
