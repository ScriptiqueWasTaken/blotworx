//! Body entity-listener slots, DOL802b5664..802b5844. Native allocator/capacity
//! flags, body callbacks and listener lifetimes remain world-backend work.
use anyhow::{Context, Result};
#[derive(Default, Debug)]
pub struct EntitySlots {
    storage: Vec<Option<usize>>,
    count: u16,
}
impl EntitySlots {
    pub fn slots(&self) -> &[Option<usize>] {
        &self.storage[..self.count as usize]
    }
    pub fn count(&self) -> u16 {
        self.count
    }
    fn register(&mut self, listener: Option<usize>) -> Result<usize> {
        let listener = listener.filter(|id| *id != 0);
        if let Some(index) = self.slots().iter().position(Option::is_none) {
            self.storage[index] = listener;
            return Ok(index);
        }
        let index = self.count as usize;
        if index == self.storage.len() {
            self.storage
                .try_reserve(1)
                .context("Body listener storage allocation failed")?;
            self.storage.push(listener);
        } else {
            self.storage[index] = listener;
        }
        self.count = self.count.wrapping_add(1);
        Ok(index)
    }
    fn remove(&mut self, listener: Option<usize>) -> Result<usize> {
        let listener = listener.filter(|id| *id != 0);
        let index = self
            .slots()
            .iter()
            .position(|slot| *slot == listener)
            .context(
                "Removing an unregistered body entity listener would corrupt native storage",
            )?;
        self.storage[index] = None;
        Ok(index)
    }
}
/// body+214 lazily owns a16-byte extended-listener container. Only entity slots
/// at+8 are operated here; the other array's behavior is still unimplemented.
#[derive(Default, Debug)]
pub struct Extended {
    pub entities: EntitySlots,
}
#[derive(Default, Debug)]
pub struct BodyListeners {
    pub extended: Option<Extended>,
}
impl BodyListeners {
    pub fn register(&mut self, listener: Option<usize>) -> Result<usize> {
        self.extended
            .get_or_insert_with(Extended::default)
            .entities
            .register(listener)
    }
    /// Native removal also allocates missing extended storage before searching.
    pub fn remove(&mut self, listener: Option<usize>) -> Result<usize> {
        self.extended
            .get_or_insert_with(Extended::default)
            .entities
            .remove(listener)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_listener_holes_duplicates_nulls_and_native_count_wrap() -> Result<()> {
        let mut listeners = BodyListeners::default();
        assert!(listeners.extended.is_none());
        assert!(listeners.remove(Some(7)).is_err());
        assert!(listeners.extended.is_some());
        assert_eq!(listeners.register(Some(7))?, 0);
        assert_eq!(listeners.register(Some(8))?, 1);
        assert_eq!(listeners.register(Some(7))?, 2);
        assert_eq!(listeners.remove(Some(7))?, 0);
        assert_eq!(
            listeners.extended.as_ref().unwrap().entities.slots(),
            [None, Some(8), Some(7)]
        );
        assert_eq!(listeners.register(Some(9))?, 0);
        assert_eq!(listeners.remove(Some(7))?, 2);
        assert_eq!(listeners.register(None)?, 2);
        assert_eq!(listeners.register(Some(0))?, 2);
        assert_eq!(listeners.register(Some(10))?, 2);
        assert_eq!(
            listeners.extended.as_ref().unwrap().entities.slots(),
            [Some(9), Some(8), Some(10)]
        );
        assert!(listeners.remove(Some(777)).is_err());
        let mut slots = EntitySlots {
            storage: vec![Some(1); u16::MAX as usize],
            count: u16::MAX,
        };
        assert_eq!(slots.register(Some(2))?, 65535);
        assert_eq!(slots.count(), 0);
        assert!(slots.slots().is_empty());
        assert_eq!(slots.register(Some(3))?, 0);
        assert_eq!(slots.slots(), [Some(3)]);
        Ok(())
    }
}
