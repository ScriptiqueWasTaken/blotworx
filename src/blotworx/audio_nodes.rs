//! Persistent global SIS mute state for admitted actor-mixer nodes.
//! Worker integration lives in audio_mute_mix/audio_output; this module owns
//! native SIS state and fractional notification values.
use super::{
    audio_bank::Bank,
    audio_control::ParameterNodeControl,
    audio_transition::{Mute, Tables},
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Default)]
struct Node {
    global: Option<Mute>,
    references: usize,
}
pub struct Nodes {
    tables: Tables,
    nodes: BTreeMap<u32, Node>,
    known: BTreeMap<u32, u32>,
    tick: u32,
    notifications: Vec<(u32,f32)>,
}
impl Nodes {
    pub fn new() -> Result<Self> {
        Ok(Self {
            tables: Tables::retail()?,
            nodes: BTreeMap::new(),
            known: BTreeMap::new(),
            tick: 0,
            notifications: Vec::new(),
        })
    }
    /// Only the actor-mixer target class has its global mute path recovered.
    /// Keep other bank identities so they produce an explicit unsupported
    /// error rather than masquerading as an absent native target.
    pub fn register_bank(&mut self, bank: &Bank<'_>) -> Result<()> {
        for (&id, object) in &bank.objects {
            ensure!(
                self.known.get(&id).is_none_or(|&kind| kind == object.kind),
                "Conflicting audio object class for {id:08x}"
            );
        }
        for (&id, object) in &bank.objects {
            self.known.insert(id, object.kind);
            if object.kind == 7 {
                self.nodes.entry(id).or_default();
            }
        }
        Ok(())
    }
    pub fn update(&mut self, tick: u32) -> Result<()> {
        self.tick = tick;
        for (&id,node) in &mut self.nodes {
            if let Some(state) = &mut node.global {
                let updating=state.transition.is_some();
                state.update(&self.tables, tick)?;
                if updating {self.notifications.push((id,state.callback_value));}
            }
        }
        Ok(())
    }
    pub fn register_mixers(&mut self,ids:impl IntoIterator<Item=u32>) {
        for id in ids {self.known.insert(id,7);self.nodes.entry(id).or_default();}
    }
    pub fn active(&self)->bool {self.nodes.values().any(|node|node.global.as_ref().is_some_and(|state|state.transition.is_some()))}
    pub fn notifications(&mut self)->Vec<(u32,f32)> {std::mem::take(&mut self.notifications)}
    pub fn byte(&self, id: u32) -> Option<u8> {
        self.nodes
            .get(&id)
            .map(|node| node.global.as_ref().map_or(255, |state| state.byte))
    }
    pub fn references(&self, id: u32) -> Option<usize> {
        self.nodes.get(&id).map(|node| node.references)
    }
}
impl ParameterNodeControl for Nodes {
    type Handle = u32;
    fn acquire_node(&mut self, id: u32) -> Result<Option<u32>> {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.references = node
                .references
                .checked_add(1)
                .context("Audio reference count overflow")?;
            Ok(Some(id))
        } else if let Some(kind) = self.known.get(&id) {
            anyhow::bail!("Audio parameter-node class {kind} for {id:08x} is not admitted to native mute execution")
        } else {
            Ok(None)
        }
    }
    fn mute_global(&mut self, id: u32, curve: u8, duration: i32) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .context("Mute target is not registered")?;
        let state=node.global.get_or_insert_with(Mute::default);
        let immediate=state.transition.is_none() && duration==0;
        state.request(
            &self.tables,
            true,
            curve,
            duration,
            self.tick,
        )?;
        if immediate {self.notifications.push((id,state.callback_value));}
        Ok(())
    }
    fn unmute_global(&mut self, id: u32, curve: u8, duration: i32) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .context("Unmute target is not registered")?;
        // DOL80334ff0 checks an existing global SIS byte against ff before
        // unmute. No SIS is allocated and no active fade is cancelled when
        // this predicate is false, even if a fade was just requested.
        if let Some(state) = &mut node.global {
            if state.byte != 255 {
                let immediate=state.transition.is_none() && duration==0;
                state.request(&self.tables, false, curve, duration, self.tick)?;
                if immediate {self.notifications.push((id,state.callback_value));}
            }
        }
        Ok(())
    }
    fn release_node(&mut self, id: u32) {
        let node = self
            .nodes
            .get_mut(&id)
            .expect("Released unknown audio node");
        node.references = node
            .references
            .checked_sub(1)
            .expect("Audio reference underflow");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardian_actions_drive_real_persistent_node_state() -> Result<()> {
        use super::super::audio_control::{execute_due, Delivery};
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let mut nodes = Nodes::new()?;
        nodes.register_bank(&bank)?;
        let ids = bank.named_event_actions("Mute_Guardians")?;
        let mut targets = Vec::new();
        for id in ids {
            let action = bank.objects[&id].action()?;
            targets.push(action.target);
            assert_eq!(
                execute_due(&action, &mut nodes, || panic!("Unexpected transition RNG"))?,
                Delivery::Applied
            );
        }
        for &target in &targets {
            assert_eq!(nodes.byte(target), Some(255));
            assert_eq!(nodes.references(target), Some(0));
        }
        // Immediate unmute before the first audio update must not cancel the
        // newly allocated fade, matching the native existing-SIS predicate.
        for id in bank.named_event_actions("Unmute_Guardians")? {
            execute_due(&bank.objects[&id].action()?, &mut nodes, || 0)?;
        }
        nodes.update(167)?;
        for &target in &targets {
            assert_eq!(nodes.byte(target), Some(0));
        }
        for id in bank.named_event_actions("Unmute_Guardians")? {
            // The caller admits the original action delay. This test executes
            // already-due commands; it does not invent scheduler time units.
            execute_due(&bank.objects[&id].action()?, &mut nodes, || 0)?;
        }
        nodes.update(334)?;
        for target in targets {
            assert_eq!(nodes.byte(target), Some(254));
            assert_eq!(nodes.references(target), Some(0));
        }
        let sound = *bank
            .objects
            .iter()
            .find(|(_, object)| object.kind == 2)
            .context("Expected source sound")?
            .0;
        assert!(
            nodes.acquire_node(sound).is_err(),
            "Unimplemented sound targets must not silently disappear"
        );
        assert_eq!(nodes.acquire_node(0)?, None);
        Ok(())
    }
}
