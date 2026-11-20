//! PC PlayerManager97ab10/97abb0, game overridef28950 and AbilityContainer
//! a3b920/a3b990. Retains status event9f producers for the unfinished event owner.
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, VecDeque};
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct AbilityInfo {
    pub unlocked: [bool; 2],
    pub allowed: [bool; 2],
    pub native_flag4: bool,
    pub native_word8: u32,
}
impl Default for AbilityInfo {
    fn default() -> Self {
        Self {
            unlocked: [false; 2],
            allowed: [true; 2],
            native_flag4: false,
            native_word8: 0,
        }
    }
}
#[derive(Debug, serde::Serialize)]
struct Container {
    owner: usize,
    kind: &'static str,
    status: BTreeMap<String, u32>,
    extensions_verified: bool,
}
#[derive(Debug, serde::Serialize)]
pub(super) struct StatusChange {
    event: u16,
    pub owner: usize,
    pub ability: String,
    container: &'static str,
    pub before: u32,
    pub after: u32,
}
#[derive(Debug,serde::Serialize)]
struct PendingDisable {owner:usize,ability:String,before:u32,cancel_pending:bool}
#[derive(Default, serde::Serialize)]
pub(super) struct State {
    slots: [BTreeMap<String, AbilityInfo>; 2],
    containers: Vec<Container>,
    pending_status_changes: VecDeque<StatusChange>,
    pending_disable: Option<PendingDisable>,
}
impl State {
    /// Basic registration98b2d0 adds Use, Jump, Bounce, each constructed locked.
    /// The human and AI subclass extensions are separate registrations: unknown
    /// names must not silently be treated as absent from an unfinished registry.
    pub fn register(
        &mut self,
        owner: usize,
        human: bool,
        ai: bool,
        basic_only: bool,
    ) -> Result<()> {
        ensure!(
            !self.containers.iter().any(|c| c.owner == owner),
            "Ability owner registered twice"
        );
        for (present, kind) in [(human, "basic"), (ai, "ai")] {
            if present {
                self.containers.push(Container {
                    owner,
                    kind,
                    status: ["Use", "Jump", "Bounce"]
                        .into_iter()
                        .map(|n| (n.into(), 1))
                        .collect(),
                    extensions_verified: basic_only && kind == "basic",
                });
            }
        }
        Ok(())
    }
    ///97ab10 updates the two preservation channels independently, retaining the
    /// allowed flags and remaining native fields. Mutation precedes component propagation.
    pub fn set_slot(
        &mut self,
        index: usize,
        name: &str,
        unlocked: bool,
        channels: [bool; 2],
    ) -> Result<()> {
        let slot = self
            .slots
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("Invalid PC player ability slot {index}"))?;
        let info = slot.entry(name.into()).or_default();
        for channel in 0..2 {
            if channels[channel] {
                info.unlocked[channel] = unlocked;
            }
        }
        Ok(())
    }
    pub fn set_lock(
        &mut self,
        index: usize,
        owner: usize,
        name: &str,
        unlocked: bool,
        channels: [bool; 2],
    ) -> Result<()> {
        self.set_slot(index, name, unlocked, channels)?;
        for container in self.containers.iter_mut().filter(|c| c.owner == owner) {
            // Base97abb0 only propagates to the basic controller when flag1 is
            // true. Game overridef28950 always updates its AI container.
            if container.kind == "basic" && !channels[0] {
                continue;
            }
            let Some(status) = container.status.get_mut(name) else {
                ensure!(
                    container.extensions_verified,
                    "PC {} ability registration for {name} is not implemented; slot state retained",
                    container.kind
                );
                continue;
            };
            let before = *status;
            *status = if unlocked { *status & !1 } else { *status | 1 };
            if before != *status {
                self.pending_status_changes.push_back(StatusChange {
                    event: 0x9f,
                    owner,
                    ability: name.into(),
                    container: container.kind,
                    before,
                    after: *status,
                });
            }
        }
        Ok(())
    }
    pub fn basic_unlocked(&self,owner:usize,name:&str)->Result<bool> {
        let container=self.containers.iter().find(|c|c.owner==owner&&c.kind=="basic")
            .ok_or_else(||anyhow::anyhow!("Player basic ability container is absent"))?;
        let status=container.status.get(name).ok_or_else(||anyhow::anyhow!("Basic ability {name} is absent"))?;
        Ok(status&1==0)
    }
    pub fn basic_enabled(&self,owner:usize,name:&str)->Result<bool> {
        let container=self.containers.iter().find(|c|c.owner==owner&&c.kind=="basic")
            .ok_or_else(||anyhow::anyhow!("Player basic ability container is absent"))?;
        let status=container.status.get(name).ok_or_else(||anyhow::anyhow!("Basic ability {name} is absent"))?;
        Ok(status&15==0)
    }
    /// PC a3ba00/a3ba50 affect the basic component's packed counter only.
    /// Reason strings are diagnostic in this retail implementation; they do
    /// not replace the source three-bit saturating count with per-name locks.
    pub fn set_basic_disabled(&mut self,owner:usize,name:&str,increment:bool,cancel:impl FnMut(usize,&str,&mut u32)->Result<()>)->Result<()> {
        ensure!(self.pending_disable.is_none(),"An ability cancellation is still pending");
        let container=self.containers.iter_mut().find(|c|c.owner==owner&&c.kind=="basic")
            .ok_or_else(||anyhow::anyhow!("Player basic ability container is absent"))?;
        let Some(status)=container.status.get_mut(name) else {
            ensure!(container.extensions_verified,"PC basic ability registration for {name} is incomplete");
            return Ok(());
        };
        let change=super::em2_ability_disable::Change::new(*status,increment);
        *status=change.after;
        self.pending_disable=Some(PendingDisable {owner,ability:name.into(),before:change.before,cancel_pending:change.cancel_active});
        self.drain_basic_disable(cancel)
    }
    /// Counter mutation precedes virtual Stop. The status event compares the
    /// saved original word against the *post-callback* live word, not against
    /// the intermediate counter result. A missing cancellation keeps its
    /// receipt, so resuming never increments the counter a second time.
    pub fn drain_basic_disable(&mut self,mut cancel:impl FnMut(usize,&str,&mut u32)->Result<()>)->Result<()> {
        let Some(pending)=self.pending_disable.as_mut() else{return Ok(())};
        let container=self.containers.iter_mut().find(|c|c.owner==pending.owner&&c.kind=="basic")
            .ok_or_else(||anyhow::anyhow!("Pending ability cancellation owner disappeared"))?;
        let status=container.status.get_mut(&pending.ability).ok_or_else(||anyhow::anyhow!("Pending ability disappeared"))?;
        if pending.cancel_pending {
            cancel(pending.owner,&pending.ability,status)?;
            pending.cancel_pending=false;
        }
        if pending.before!=*status {
            self.pending_status_changes.push_back(StatusChange {event:0x9f,owner:pending.owner,ability:pending.ability.clone(),container:container.kind,
                before:pending.before,after:*status});
        }
        self.pending_disable=None;Ok(())
    }
    pub fn retire(&mut self, owner: usize) {
        self.containers.retain(|c| c.owner != owner);
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"state":self,"status_event_consumer_connected":false,
            "scope":"verified PC slot metadata, basic Use/Jump/Bounce lock and packed disable-counter producers; cancellation/status event consumers, subclass registration, actions and save restoration remain incomplete"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_disable_producers_match_original_pc_control_flow()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/em2_ability_disable_reference.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut state=State::default();state.register(7,true,false,true)?;
            let before=case["before"].as_u64().unwrap()as u32;
            if case["present"]==true {state.containers[0].status.insert("Use".into(),before);}
            else {state.containers[0].status.remove("Use");}
            let mut effects=Vec::new();
            state.set_basic_disabled(7,"Use",case["increment"].as_bool().unwrap(),|_,_,status|{
                effects.push(serde_json::json!({"kind":"cancel"}));
                if case["cancel_mutates"]==true {*status &= !16;}
                Ok(())
            })?;
            for change in &state.pending_status_changes {
                effects.push(serde_json::json!({"kind":"status","before":change.before,"after":change.after}));
            }
            assert_eq!(serde_json::json!(effects),case["effects"],"{case}");
            if case["present"]==true {
                assert_eq!(state.containers[0].status["Use"],case["after"].as_u64().unwrap()as u32);
                assert_eq!(state.basic_enabled(7,"Use")?,state.containers[0].status["Use"]&15==0);
            }
        }Ok(())
    }
    #[test]
    fn cancellation_failure_resumes_without_reapplying_counter_and_reads_final_status()->Result<()> {
        let mut state=State::default();state.register(7,true,false,true)?;
        state.containers[0].status.insert("Use".into(),0x80000010);
        assert!(state.set_basic_disabled(7,"Use",true,|_,_,_|anyhow::bail!("cancel receiver missing")).is_err());
        assert_eq!(state.containers[0].status["Use"],0x80000012);
        assert!(state.pending_status_changes.is_empty());assert!(state.pending_disable.is_some());
        state.drain_basic_disable(|_,_,status|{assert_eq!(*status,0x80000012);*status &= !16;Ok(())})?;
        assert_eq!(state.containers[0].status["Use"],0x80000002);
        assert_eq!(state.pending_status_changes.len(),1);
        assert_eq!(state.pending_status_changes[0].before,0x80000010);
        assert_eq!(state.pending_status_changes[0].after,0x80000002);
        assert!(state.pending_disable.is_none());Ok(())
    }
    #[test]
    fn setters_match_original_pc_instruction_fixtures() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/em2_ability_reference.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s = State::default();
            let name = "Use";
            let unlocked = case["unlocked"].as_bool().unwrap();
            let expected = &case["reference"];
            if case["kind"] == "slot" {
                let index = case["index"].as_u64().unwrap() as usize;
                if let Some(initial) = case.get("initial") {
                    let flags = initial["flags"].as_array().unwrap();
                    s.slots[index].insert(
                        name.into(),
                        AbilityInfo {
                            unlocked: [
                                flags[0].as_u64().unwrap() != 0,
                                flags[1].as_u64().unwrap() != 0,
                            ],
                            allowed: [
                                flags[2].as_u64().unwrap() != 0,
                                flags[3].as_u64().unwrap() != 0,
                            ],
                            native_flag4: flags[4].as_u64().unwrap() != 0,
                            native_word8: initial["word8"].as_u64().unwrap() as u32,
                        },
                    );
                }
                let channels = std::array::from_fn(|i| case["channels"][i].as_bool().unwrap());
                s.set_slot(index, name, unlocked, channels)?;
                let info = &s.slots[index][name];
                assert_eq!(
                    serde_json::json!([
                        u8::from(info.unlocked[0]),
                        u8::from(info.unlocked[1]),
                        u8::from(info.allowed[0]),
                        u8::from(info.allowed[1]),
                        u8::from(info.native_flag4)
                    ]),
                    expected["flags"],
                    "{}",
                    case["name"]
                );
                assert_eq!(
                    u64::from(info.native_word8),
                    expected["word8"].as_u64().unwrap()
                );
            } else {
                s.register(4, true, false, true)?;
                let present = case
                    .get("present")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let status = case["status"].as_u64().unwrap() as u32;
                if present {
                    s.containers[0].status.insert(name.into(), status);
                } else {
                    s.containers[0].status.remove(name);
                }
                s.set_lock(0, 4, name, unlocked, [true; 2])?;
                if present {
                    assert_eq!(
                        u64::from(s.containers[0].status[name]),
                        expected["status"].as_u64().unwrap()
                    );
                }
                let events = s
                    .pending_status_changes
                    .iter()
                    .map(|e| serde_json::json!({"before":e.before,"after":e.after}))
                    .collect::<Vec<_>>();
                assert_eq!(
                    serde_json::json!(events),
                    expected["events"],
                    "{}",
                    case["name"]
                );
            }
        }
        Ok(())
    }
    #[test]
    fn independent_slot_flags_keep_native_constructor_defaults() -> Result<()> {
        let mut s = State::default();
        s.set_slot(0, "Use", true, [true, false])?;
        let v = &s.slots[0]["Use"];
        assert_eq!(v.unlocked, [true, false]);
        assert_eq!(v.allowed, [true; 2]);
        assert!(!v.native_flag4);
        assert_eq!(v.native_word8, 0);
        s.set_slot(0, "Use", false, [false, true])?;
        assert_eq!(s.slots[0]["Use"].unlocked, [true, false]);
        assert!(s.set_slot(2, "Use", true, [true; 2]).is_err());
        Ok(())
    }
    #[test]
    fn lock_edges_preserve_other_status_bits_and_producer_order() -> Result<()> {
        let mut s = State::default();
        s.register(8, true, true, false)?;
        s.containers[0].status.insert("Use".into(), 0x1f);
        s.set_lock(0, 8, "Use", true, [true; 2])?;
        assert_eq!(s.containers[0].status["Use"], 0x1e);
        assert_eq!(s.pending_status_changes.len(), 2);
        assert_eq!(s.pending_status_changes[0].container, "basic");
        assert_eq!(s.pending_status_changes[1].container, "ai");
        s.set_lock(0, 8, "Use", true, [true; 2])?;
        assert_eq!(s.pending_status_changes.len(), 2);
        s.set_lock(0, 8, "Use", false, [true; 2])?;
        assert_eq!(s.pending_status_changes.len(), 4);
        Ok(())
    }
    #[test]
    fn game_override_updates_ai_even_when_basic_channel_is_false() -> Result<()> {
        let mut s = State::default();
        s.register(1, true, true, false)?;
        s.set_lock(1, 1, "Jump", true, [false, true])?;
        assert_eq!(s.containers[0].status["Jump"], 1);
        assert_eq!(s.containers[1].status["Jump"], 0);
        Ok(())
    }
    #[test]
    fn unverified_extensions_are_retained_failures_not_absent_abilities() -> Result<()> {
        let mut s = State::default();
        s.register(2, true, false, false)?;
        assert!(s.set_lock(0, 2, "Brush", true, [true; 2]).is_err());
        assert_eq!(s.slots[0]["Brush"].unlocked, [true; 2]);
        assert!(s.pending_status_changes.is_empty());
        let mut plain = State::default();
        plain.register(3, true, false, true)?;
        plain.set_lock(0, 3, "not_registered", true, [true; 2])?;
        assert!(plain.pending_status_changes.is_empty());
        Ok(())
    }
}
