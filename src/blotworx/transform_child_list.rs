//! Owned logical child array, REL3536b8/35376c/3537b0. Native allocator
//! behavior is separate; logical order/count and retained tail are preserved.
use anyhow::{bail, Result};
#[derive(Default, Debug)]
pub struct Children {
    storage: Vec<u32>,
    count: u16,
}
impl Children {
    pub fn entries(&self) -> &[u32] {
        &self.storage[..usize::from(self.count)]
    }
    pub fn count(&self) -> u16 {
        self.count
    }
    pub fn handle(&self, index: usize) -> Result<u32> {
        self.entries().get(index).copied().ok_or_else(|| {
            anyhow::anyhow!("Child index {index} outside logical count {}", self.count)
        })
    }
    /// Callback implements the >100/nonzero registry lookup. It may alter the
    /// list/request holder; insertion uses the fresh handle/count afterward.
    pub fn register(
        &mut self,
        handle: u32,
        mut lookup: impl FnMut(&mut Self, u32) -> Result<u32>,
    ) -> Result<bool> {
        if self.entries().contains(&handle) {
            return Ok(false);
        }
        let value = if self.count > 100 && handle != 0 {
            lookup(self, handle)?
        } else {
            handle
        };
        let Some(next) = self.count.checked_add(1) else {
            bail!("Native child count overflow would wrap at 65536; invalid hierarchy requires diagnosis");
        };
        let at = usize::from(self.count);
        if at == self.storage.len() {
            self.storage.try_reserve(1)?;
            self.storage.push(value)
        } else {
            self.storage[at] = value
        }
        self.count = next;
        Ok(true)
    }
    pub fn remove(&mut self, handle: u32) -> bool {
        let Some(index) = self.entries().iter().position(|h| *h == handle) else {
            return false;
        };
        self.count -= 1;
        let last = usize::from(self.count);
        if index != last {
            self.storage[index] = self.storage[last];
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unique_registration_swap_removal_and_lookup_side_effects() -> Result<()> {
        let mut c = Children::default();
        for h in [0, 1, 2, 3] {
            assert!(c.register(h, |_, _| bail!("Unexpected lookup"))?)
        }
        assert!(!c.register(2, |_, _| bail!("Duplicate looked up"))?);
        assert!(c.remove(1));
        assert_eq!(c.entries(), [0, 3, 2]);
        assert_eq!(c.storage, [0, 3, 2, 3]); // Removed tail remains allocated.
        assert!(!c.remove(99));
        assert!(c.remove(2));
        assert_eq!(c.entries(), [0, 3]);
        for h in 4..103 {
            c.register(h, |_, _| bail!("Lookup before >100"))?;
        }
        assert_eq!(c.count(), 101);
        c.register(103, |list, handle| {
            assert_eq!(handle, 103);
            assert!(list.remove(4));
            Ok(104)
        })?;
        assert_eq!(c.count(), 101);
        assert_eq!(c.entries().last(), Some(&104));
        let before = c.entries().to_vec();
        assert!(c.register(105, |_, _| bail!("Registry failure")).is_err());
        assert_eq!(c.entries(), before);
        Ok(())
    }
}
