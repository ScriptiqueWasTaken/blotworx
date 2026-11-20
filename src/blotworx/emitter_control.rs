//! REL20f234/20f3b0: controller-chain first, then NiNode children.
use super::scene_bounds::Hierarchy;
use anyhow::{ensure, Context, Result};

pub struct Plan {
    controllers: Vec<(u32, bool)>,
}
impl Plan {
    pub fn load(graph: &Hierarchy, root: u32) -> Result<Self> {
        let mut controllers = Vec::new();
        let mut pending = vec![root];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            ensure!(seen.insert(id), "Repeated emitter-walk node {id}");
            let node = graph.nodes.get(&id).context("Missing emitter-walk node")?;
            for c in &node.controllers {
                controllers.push((
                    c.block,
                    super::nif_types::is_a(&c.class_name, "NiPSEmitterCtlr")?,
                ));
            }
            if super::nif_types::is_a(&node.class_name, "NiNode")? {
                // Native walk bounds by used count, not effective array size.
                // A hole inside that prefix would dereference null. Reject
                // rather than quietly visit a different set of children.
                let used = node.children.iter().flatten().count();
                let prefix = &node.children[..used];
                ensure!(
                    prefix.iter().all(Option::is_some),
                    "Sparse native emitter-walk child prefix at node {id}"
                );
                pending.extend(prefix.iter().rev().flatten().copied());
            }
        }
        Ok(Self { controllers })
    }
    pub fn emitters(&self) -> impl Iterator<Item = u32> + '_ {
        self.controllers
            .iter()
            .filter_map(|(id, emitter)| emitter.then_some(*id))
    }
    /// Read the application's DOUBLE clock for every controller on START,
    /// then narrow to float before invoking emitter virtual Start. STOP
    /// doesn't access the clock. Repeated commands must still execute.
    pub fn execute(
        &self,
        enabled: bool,
        mut clock: impl FnMut() -> Result<f64>,
        mut dispatch: impl FnMut(u32, Option<f32>) -> Result<()>,
    ) -> Result<()> {
        for &(id, emitter) in &self.controllers {
            let time = if enabled { Some(clock()? as f32) } else { None };
            if emitter {
                dispatch(id, time)?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_reads_include_skipped_controllers_and_repeated_starts() -> Result<()> {
        let plan = Plan {
            controllers: vec![(4, false), (8, true), (9, false), (10, true)],
        };
        let mut reads = 0;
        let mut calls = Vec::new();
        for _ in 0..2 {
            plan.execute(
                true,
                || {
                    reads += 1;
                    Ok(reads as f64 + 0.125)
                },
                |id, time| {
                    calls.push((id, time));
                    Ok(())
                },
            )?;
        }
        assert_eq!(reads, 8);
        assert_eq!(
            calls,
            vec![
                (8, Some(2.125)),
                (10, Some(4.125)),
                (8, Some(6.125)),
                (10, Some(8.125))
            ]
        );
        plan.execute(
            false,
            || anyhow::bail!("STOP read clock"),
            |id, time| {
                calls.push((id, time));
                Ok(())
            },
        )?;
        assert_eq!(&calls[4..], [(8, None), (10, None)]);
        Ok(())
    }
}
