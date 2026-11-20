//! Production pickup initialization over owned phantom/component leases.
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};
/// One scene-owner reference initially owns this runtime component record.
pub struct Component {
    pub references: u32,
    pub phantom: usize,
}
pub struct Bindings<'a> {
    pub component: usize,
    pub components: &'a mut BTreeMap<usize, Component>,
    pub monitor: &'a Mutex<()>,
    pub held: Option<MutexGuard<'a, ()>>,
    pub position: [f32; 4],
}
impl super::hk_reference::Backend for Bindings<'_> {
    fn enter(&mut self) -> Result<()> {
        ensure!(self.held.is_none(), "Pickup reference monitor already held");
        self.held = Some(
            self.monitor
                .lock()
                .map_err(|_| anyhow::anyhow!("Pickup reference monitor poisoned"))?,
        );
        Ok(())
    }
    fn leave(&mut self) -> Result<()> {
        ensure!(
            self.held.take().is_some(),
            "Pickup reference monitor not held"
        );
        Ok(())
    }
    fn destroy(&mut self, _: &mut super::hk_reference::Header) -> Result<()> {
        anyhow::bail!(
            "Pickup initialization cannot destroy a referenced object; teardown backend required"
        )
    }
}
impl super::pickup_lifecycle::ComponentBindings for Bindings<'_> {
    fn physics_component(&mut self) -> Result<Option<usize>> {
        ensure!(
            self.components.contains_key(&self.component),
            "Missing pickup physics component lease"
        );
        Ok(Some(self.component))
    }
    fn retain_component(&mut self, c: usize) -> Result<()> {
        let _guard = self
            .monitor
            .lock()
            .map_err(|_| anyhow::anyhow!("Component reference monitor poisoned"))?;
        let record = self
            .components
            .get_mut(&c)
            .context("Unknown pickup physics component lease")?;
        record.references = record.references.wrapping_add(1);
        Ok(())
    }
    fn release_component(&mut self, c: usize) -> Result<()> {
        let _guard = self
            .monitor
            .lock()
            .map_err(|_| anyhow::anyhow!("Component reference monitor poisoned"))?;
        let record = self
            .components
            .get_mut(&c)
            .context("Unknown pickup physics component lease")?;
        record.references = record.references.wrapping_sub(1);
        ensure!(
            record.references != 0,
            "Physics component destruction requires teardown backend"
        );
        Ok(())
    }
    fn phantom(&mut self, c: usize) -> Result<Option<usize>> {
        Ok(Some(
            self.components
                .get(&c)
                .context("Missing pickup physics component lease")?
                .phantom,
        ))
    }
    fn position(&mut self) -> Result<[f32; 4]> {
        Ok(self.position)
    }
}
