//! Deferred entity cleanup container, EM1 REL181fb0 and1818b0.
//! This models ordering only; component destruction and resource release must
//! be performed by the lifecycle callback, never replaced by vector deletion.
use anyhow::Result;

/// Base entity REL1edecc invokes each component's preparation slot+184.
pub fn prepare_components(
    count: usize,
    mut prepare: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    for index in 0..count {
        prepare(index)?;
    }
    Ok(())
}
/// REL1ede30 snapshots component count, passes dt to slot+180, and stops at
/// the first false result. No later readiness callback runs in that case.
pub fn components_ready(
    count: usize,
    dt: f32,
    mut ready: impl FnMut(usize, f32) -> Result<bool>,
) -> Result<bool> {
    for index in 0..count {
        if !ready(index, dt)? {
            return Ok(false);
        };
    }
    Ok(true)
}
/// REL1edf44 queues event0x44 before DOL804282c0 walks component slot+188 in
/// reverse. Queueing is mandatory and fallible until its receiver is ported.
pub fn teardown_components(
    count: usize,
    mut queue_event: impl FnMut(u16) -> Result<()>,
    mut teardown: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    queue_event(0x44)?;
    for index in (0..count).rev() {
        teardown(index)?;
    }
    Ok(())
}

#[derive(Default, Debug)]
pub struct Queue {
    entries: Vec<usize>,
}
impl Queue {
    // Split phase access permits preparation/readiness callbacks to enqueue
    // more owners without borrowing the queue across a World callback.
    pub fn contains(&self,entity:usize)->bool {self.entries.contains(&entity)}
    pub fn len(&self)->usize {self.entries.len()}
    pub fn at(&self,index:usize)->Option<usize> {self.entries.get(index).copied()}
    pub fn append_prepared(&mut self,entity:usize)->bool {
        if self.contains(entity) {return false;}
        self.entries.push(entity);true
    }
    pub fn remove_at(&mut self,index:usize)->usize {self.entries.swap_remove(index)}

    /// Native enqueue checks scene membership and duplicates before invoking
    /// virtual+184 and appending. Failure must not publish a queued entry.
    pub fn enqueue(
        &mut self,
        entity: usize,
        has_scene: bool,
        mut prepare: impl FnMut(usize) -> Result<()>,
    ) -> Result<bool> {
        if !has_scene || self.entries.contains(&entity) {
            return Ok(false);
        };
        prepare(entity)?;
        self.entries.push(entity);
        Ok(true)
    }
    /// REL181d34 swaps the last entry into the removed slot; that slot is
    /// visited again. A rejected cleanup advances the cursor, not the queue.
    /// Callback must include native readiness and all lifecycle operations.
    pub fn process(&mut self, mut cleanup: impl FnMut(usize) -> Result<bool>) -> Result<()> {
        let mut cursor = 0;
        while cursor < self.entries.len() {
            if cleanup(self.entries[cursor])? {
                self.entries.swap_remove(cursor);
            } else {
                cursor += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_lifecycle_orders_and_readiness_short_circuit_match_original() -> Result<()> {
        let mut preparation = Vec::new();
        prepare_components(3, |i| {
            preparation.push(i);
            Ok(())
        })?;
        assert_eq!(preparation, [0, 1, 2]);
        let mut checked = Vec::new();
        assert!(!components_ready(3, 0.125, |i, dt| {
            checked.push((i, dt.to_bits()));
            Ok(i != 1)
        })?);
        assert_eq!(checked, [(0, 0.125f32.to_bits()), (1, 0.125f32.to_bits())]);
        let trace = std::cell::RefCell::new(Vec::new());
        teardown_components(
            3,
            |event| {
                trace.borrow_mut().push(format!("event:{event:x}"));
                Ok(())
            },
            |i| {
                trace.borrow_mut().push(format!("component:{i}"));
                Ok(())
            },
        )?;
        assert_eq!(
            *trace.borrow(),
            ["event:44", "component:2", "component:1", "component:0"]
        );
        assert!(components_ready(0, 0., |_, _| panic!("Empty entity"))?);
        assert!(teardown_components(
            3,
            |_| anyhow::bail!("Missing event manager"),
            |_| panic!("Must not teardown after failed event queue")
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn native_queue_suppresses_duplicates_and_processes_swapped_tail() -> Result<()> {
        let mut queue = Queue::default();
        let mut prepared = Vec::new();
        for entity in [1, 2, 3, 4, 2] {
            queue.enqueue(entity, true, |e| {
                prepared.push(e);
                Ok(())
            })?;
        }
        assert_eq!(prepared, [1, 2, 3, 4]);
        assert!(!queue.enqueue(7, false, |_| panic!("Scene-less enqueue"))?);
        let mut visited = Vec::new();
        queue.process(|entity| {
            visited.push(entity);
            Ok(entity != 2)
        })?;
        assert_eq!(visited, [1, 4, 3, 2]);
        assert_eq!(queue.entries, [2]);
        queue.process(|_| Ok(true))?;
        assert!(queue.entries.is_empty());
        Ok(())
    }
    #[test]
    fn explicit_lifecycle_errors_keep_unprocessed_entries() -> Result<()> {
        let mut queue = Queue::default();
        assert!(queue
            .enqueue(1, true, |_| anyhow::bail!("Missing preparation"))
            .is_err());
        assert!(queue.entries.is_empty());
        for e in [1, 2, 3] {
            queue.enqueue(e, true, |_| Ok(()))?;
        }
        assert!(queue
            .process(|e| {
                if e == 3 {
                    anyhow::bail!("Missing destruction")
                };
                Ok(true)
            })
            .is_err());
        assert_eq!(queue.entries, [3, 2]);
        Ok(())
    }
}
