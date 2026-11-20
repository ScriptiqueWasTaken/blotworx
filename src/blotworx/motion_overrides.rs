//! Motion component +0x204 priority table. REL 0x3069a8/0x3069c8/0x306c14.
//! The value's consumer determines its meaning; this table does not infer
//! speed, multiply entries, or cache the fallback property's live value.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Overrides {
    values: BTreeMap<i32, f32>,
}
impl Overrides {
    pub fn highest(&self) -> Option<f32> {
        self.values.last_key_value().map(|(_, value)| *value)
    }
    pub fn set(&mut self, priority: i32, value: f32) -> Result<()> {
        // Original count is u16. Fail explicitly instead of silently wrapping.
        ensure!(
            self.values.contains_key(&priority) || self.values.len() < u16::MAX as usize,
            "Motion override table exceeds original u16 capacity"
        );
        self.values.insert(priority, value);
        Ok(())
    }
    pub fn remove(&mut self, priority: i32) {
        self.values.remove(&priority);
    }
    /// Resolve the original property only if no override wins. Property access
    /// may have observable work, so callers provide a lazy resolver.
    pub fn resolve(&self, fallback: impl FnOnce() -> Result<f32>) -> Result<f32> {
        match self.values.last_key_value() {
            Some((_, value)) => Ok(*value),
            None => fallback(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ending_brush_reveals_other_priority_without_resolving_default() -> Result<()> {
        let mut values = Overrides::default();
        values.set(-2, 0.4)?;
        values.set(1, 0.7)?; // Brush action 0xa97cc.
        assert_eq!(
            values.resolve(|| panic!("Unexpected property lookup"))?,
            0.7
        );
        values.set(3, 0.2)?;
        values.set(1, 0.8)?;
        assert_eq!(
            values.resolve(|| panic!("Unexpected property lookup"))?,
            0.2
        );
        values.remove(1); // Brush stop 0xa9b2c must retain priority 3.
        assert_eq!(
            values.resolve(|| panic!("Unexpected property lookup"))?,
            0.2
        );
        values.remove(3);
        assert_eq!(
            values.resolve(|| panic!("Unexpected property lookup"))?,
            0.4
        );
        values.remove(-2);
        values.remove(1);
        assert_eq!(values.resolve(|| Ok(1.25))?, 1.25);
        // Store the original float bits, with no invented range validation.
        let payload = f32::from_bits(0x7fc01234);
        values.set(1, payload)?;
        assert_eq!(
            values.resolve(|| unreachable!())?.to_bits(),
            payload.to_bits()
        );
        Ok(())
    }
}
