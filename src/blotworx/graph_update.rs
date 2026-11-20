//! Retail active-node update pass, 0x801ef714..0x801ef888.
//! Topology changes, event processing and sync metadata rebuilding follow this
//! pass in the graph owner. This is not a complete behavior graph executor.
use super::animation_sync;
use anyhow::{ensure, Result};

pub struct Node {
    pub timing: animation_sync::Node,
    /// Active-node byte +0x70 selects a temporary, discarded event queue.
    pub discard_events: bool,
    /// Modifier runtime activation flag, active-node byte +0x74.
    pub modifier_active: bool,
    /// Active child count at +0x5c: activation changes topology when positive.
    pub active_children: i32,
}

/// Implementations resolve node/template identity and own variable storage,
/// event queues and generators. No default no-op implementations are supplied.
pub trait Executor {
    fn select_owner_graph(&mut self, node: usize) -> Result<()>;
    fn bindings(&mut self, node: usize, reverse: bool) -> Result<()>;
    fn begin_events(&mut self, node: usize, discard: bool) -> Result<()>;
    fn end_events(&mut self, node: usize) -> Result<()>;
    fn modifier_enabled(&mut self, node: usize) -> Result<bool>;
    fn activate(&mut self, node: usize) -> Result<()>;
    fn deactivate(&mut self, node: usize) -> Result<()>;
    fn topology_changed(&mut self) -> Result<()>;
    fn update(&mut self, node: usize, dt: f32) -> Result<()>;
}

/// Consume already-computed synchronization timesteps, preserving the original
/// ascending active-node order. Binding writes can change a modifier's enable
/// state, so enable must be queried after the forward binding pass.
pub fn run(nodes: &mut [Node], timesteps: &[f32], executor: &mut impl Executor) -> Result<()> {
    ensure!(
        nodes.len() == timesteps.len() && timesteps.iter().all(|t| t.is_finite()),
        "Invalid graph update timestep array"
    );
    for (index, (node, &dt)) in nodes.iter_mut().zip(timesteps).enumerate() {
        executor.select_owner_graph(index)?;
        executor.bindings(index, false)?;
        executor.begin_events(index, node.discard_events)?;
        let update: Result<()> = (|| {
            if node.timing.flag_71 {
                executor.update(index, dt)?;
            } else if executor.modifier_enabled(index)? {
                if !node.modifier_active {
                    executor.activate(index)?;
                    node.modifier_active = true;
                    if node.active_children > 0 {
                        executor.topology_changed()?;
                    }
                }
                executor.update(index, dt)?;
            } else if node.modifier_active {
                executor.deactivate(index)?;
                node.modifier_active = false;
                if node.active_children > 0 {
                    executor.topology_changed()?;
                }
            }
            Ok(())
        })();
        // Restore event scope even if a native implementation reports an error.
        let restore = executor.end_events(index);
        update?;
        restore?;
        executor.bindings(index, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Probe {
        calls: Vec<String>,
        enabled: bool,
        enable_on_bind: bool,
        fail_update: bool,
    }
    impl Executor for Probe {
        fn select_owner_graph(&mut self, n: usize) -> Result<()> {
            self.calls.push(format!("graph{n}"));
            Ok(())
        }
        fn bindings(&mut self, n: usize, reverse: bool) -> Result<()> {
            self.calls.push(format!("bind{n}:{reverse}"));
            if !reverse && self.enable_on_bind {
                self.enabled = true;
            }
            Ok(())
        }
        fn begin_events(&mut self, n: usize, discard: bool) -> Result<()> {
            self.calls.push(format!("events{n}:{discard}"));
            Ok(())
        }
        fn end_events(&mut self, n: usize) -> Result<()> {
            self.calls.push(format!("restore{n}"));
            Ok(())
        }
        fn modifier_enabled(&mut self, _: usize) -> Result<bool> {
            Ok(self.enabled)
        }
        fn activate(&mut self, n: usize) -> Result<()> {
            self.calls.push(format!("activate{n}"));
            Ok(())
        }
        fn deactivate(&mut self, n: usize) -> Result<()> {
            self.calls.push(format!("deactivate{n}"));
            Ok(())
        }
        fn topology_changed(&mut self) -> Result<()> {
            self.calls.push("topology".into());
            Ok(())
        }
        fn update(&mut self, n: usize, dt: f32) -> Result<()> {
            self.calls.push(format!("update{n}:{dt}"));
            ensure!(!self.fail_update, "test failure");
            Ok(())
        }
    }
    fn node(generator: bool) -> Node {
        Node {
            timing: animation_sync::Node {
                sync: animation_sync::Sync {
                    points: vec![],
                    frequency: 0.,
                    time: 0.,
                    field_48: 0.,
                    mapped_events: false,
                },
                parent: 0,
                flag_71: generator,
                synchronize: false,
                flag_73: false,
            },
            discard_events: false,
            modifier_active: false,
            active_children: 0,
        }
    }
    #[test]
    fn bindings_precede_modifier_activation_and_zero_dt_still_updates() -> Result<()> {
        let mut nodes = vec![node(true), node(false)];
        nodes[1].discard_events = true;
        nodes[1].active_children = 1;
        let mut p = Probe {
            enable_on_bind: true,
            ..Default::default()
        };
        run(&mut nodes, &[0.25, 0.], &mut p)?;
        assert_eq!(
            p.calls,
            vec![
                "graph0",
                "bind0:false",
                "events0:false",
                "update0:0.25",
                "restore0",
                "bind0:true",
                "graph1",
                "bind1:false",
                "events1:true",
                "activate1",
                "topology",
                "update1:0",
                "restore1",
                "bind1:true"
            ]
        );
        assert!(nodes[1].modifier_active);
        p.calls.clear();
        p.enable_on_bind = false;
        p.enabled = false;
        run(&mut nodes, &[0., 0.], &mut p)?;
        assert!(!nodes[1].modifier_active);
        assert!(p.calls.contains(&"deactivate1".into()));
        assert!(!p.calls.iter().any(|s| s.starts_with("update1:")));
        Ok(())
    }
    #[test]
    fn failed_node_restores_queue_and_stops_pass() {
        let mut nodes = vec![node(true), node(true)];
        let mut p = Probe {
            fail_update: true,
            ..Default::default()
        };
        assert!(run(&mut nodes, &[0., 0.], &mut p).is_err());
        assert_eq!(p.calls.last().unwrap(), "restore0");
        assert!(!p.calls.contains(&"graph1".into()));
    }
}
