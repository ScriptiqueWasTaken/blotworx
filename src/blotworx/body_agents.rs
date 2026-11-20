//! Body filter refresh's existing-agent pass, DOL802db0f0..802db1f4.
//! Agent/island/collision-input records must be supplied by the world backend.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub agent: usize,
    pub other_collidable: usize,
}
pub trait Backend {
    fn type_1a(&mut self, body: usize) -> Result<i8>;
    fn count(&mut self, body: usize) -> Result<usize>;
    fn entry(&mut self, body: usize, index: usize) -> Result<Entry>;
    fn collision(&mut self, body: usize, other: usize) -> Result<u32>;
    fn other_type_1a(&mut self, collidable: usize) -> Result<i8>;
    fn dispatcher_byte(&mut self, types: [i8; 2]) -> Result<i8>;
    fn is_fixed_motion_5(&mut self, body: usize) -> Result<bool>;
    fn other_owner(&mut self, collidable: usize) -> Result<usize>;
    fn island(&mut self, owner: usize) -> Result<usize>;
    /// Bind actual world collision input+78 and agent update8024f420.
    fn update_collection(&mut self, agent: usize, island: usize) -> Result<()>;
    fn remove(&mut self, agent: usize) -> Result<()>;
    /// Reread body's current island after removal, then update byte+25.
    fn mark_removed_island(&mut self, body: usize) -> Result<()>;
}
/// rlwimi with r24=1,shift6,mask24..25 clears bit7 and sets bit6.
pub fn removal_flags(previous: u8) -> u8 {
    (previous & !0xc0) | 0x40
}
pub fn refresh(body: usize, collections: u32, backend: &mut impl Backend) -> Result<()> {
    let first_type = backend.type_1a(body)?;
    let mut index = 0;
    loop {
        let count = backend.count(body)?;
        ensure!(
            count <= i32::MAX as usize,
            "Native agent count outside signed32 range"
        );
        if index >= count {
            return Ok(());
        }
        let entry = backend.entry(body, index)?;
        ensure!(entry.agent != 0, "Null collision agent during body refresh");
        let admitted = backend.collision(body, entry.other_collidable)? >> 24 != 0;
        // Native rereads the slot's other collidable after the filter callback.
        let other = if admitted {
            backend.entry(body, index)?.other_collidable
        } else {
            entry.other_collidable
        };
        let enabled = if admitted {
            let second = backend.other_type_1a(other)?;
            backend.dispatcher_byte([first_type, second])? != 0
        } else {
            false
        };
        if !enabled {
            let agent = backend_entry_agent(body, index, backend)?;
            backend.remove(agent)?;
            backend.mark_removed_island(body)?;
            // Native decrements index before loop increment: revisit swapped slot.
            continue;
        }
        if collections == 1 {
            let owner = if backend.is_fixed_motion_5(body)? {
                backend.other_owner(other)?
            } else {
                body
            };
            let island = backend.island(owner)?;
            let agent = backend_entry_agent(body, index, backend)?;
            backend.update_collection(agent, island)?;
        }
        index += 1;
    }
}
fn backend_entry_agent(body: usize, index: usize, backend: &mut impl Backend) -> Result<usize> {
    let agent = backend.entry(body, index)?.agent;
    ensure!(agent != 0, "Null collision agent after refresh callback");
    Ok(agent)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        entries: Vec<Entry>,
        calls: Vec<String>,
        fixed: bool,
        first_type: i8,
        flags: u8,
        replace_on_filter: bool,
    }
    impl Backend for Trace {
        fn type_1a(&mut self, _: usize) -> Result<i8> {
            self.calls.push("type".into());
            Ok(self.first_type)
        }
        fn count(&mut self, _: usize) -> Result<usize> {
            Ok(self.entries.len())
        }
        fn entry(&mut self, _: usize, i: usize) -> Result<Entry> {
            Ok(self.entries[i])
        }
        fn collision(&mut self, _: usize, other: usize) -> Result<u32> {
            self.calls.push(format!("filter {other}"));
            if self.replace_on_filter && other == 130 {
                self.entries[0] = Entry {
                    agent: 40,
                    other_collidable: 140,
                };
            }
            Ok(if other == 110 { 1 } else { 0x01000000 })
        }
        fn other_type_1a(&mut self, c: usize) -> Result<i8> {
            Ok(if c == 120 { 0 } else { 3 })
        }
        fn dispatcher_byte(&mut self, t: [i8; 2]) -> Result<i8> {
            assert_eq!(t[0], 2);
            Ok(if t[1] == 0 { 0 } else { -1 })
        }
        fn is_fixed_motion_5(&mut self, _: usize) -> Result<bool> {
            Ok(self.fixed)
        }
        fn other_owner(&mut self, other: usize) -> Result<usize> {
            self.calls.push(format!("other owner {other}"));
            Ok(9)
        }
        fn island(&mut self, o: usize) -> Result<usize> {
            self.calls.push(format!("island {o}"));
            Ok(o + 1000)
        }
        fn update_collection(&mut self, a: usize, i: usize) -> Result<()> {
            self.calls.push(format!("update {a} {i}"));
            Ok(())
        }
        fn remove(&mut self, a: usize) -> Result<()> {
            self.calls.push(format!("remove {a}"));
            let i = self.entries.iter().position(|e| e.agent == a).unwrap();
            self.entries.swap_remove(i);
            self.first_type = 9;
            Ok(())
        }
        fn mark_removed_island(&mut self, _: usize) -> Result<()> {
            self.calls.push("mark".into());
            self.flags = removal_flags(self.flags);
            Ok(())
        }
    }
    #[test]
    fn native_agent_refresh_revisits_removed_slot_and_selects_correct_island() -> Result<()> {
        let entries = [10, 20, 30]
            .map(|agent| Entry {
                agent,
                other_collidable: agent + 100,
            })
            .to_vec();
        let mut trace = Trace {
            entries,
            calls: vec![],
            fixed: false,
            first_type: 2,
            flags: 0xff,
            replace_on_filter: false,
        };
        refresh(8, 1, &mut trace)?;
        assert_eq!(
            trace.calls,
            vec![
                "type",
                "filter 110",
                "remove 10",
                "mark",
                "filter 130",
                "island 8",
                "update 30 1008",
                "filter 120",
                "remove 20",
                "mark"
            ]
        );
        assert_eq!(trace.entries.len(), 1);
        assert_eq!(trace.entries[0].agent, 30);
        assert_eq!(trace.flags, 0x7f);
        trace.calls.clear();
        trace.first_type = 2;
        trace.fixed = true;
        refresh(8, 1, &mut trace)?;
        assert_eq!(
            trace.calls,
            vec![
                "type",
                "filter 130",
                "other owner 130",
                "island 9",
                "update 30 1009"
            ]
        );
        trace.calls.clear();
        refresh(8, 2, &mut trace)?;
        assert_eq!(trace.calls, vec!["type", "filter 130"]);
        assert_eq!(removal_flags(0x80), 0x40);
        Ok(())
    }
    #[test]
    fn agent_entry_is_reread_after_filter_before_collection_update() -> Result<()> {
        let mut trace = Trace {
            entries: vec![Entry {
                agent: 30,
                other_collidable: 130,
            }],
            calls: vec![],
            fixed: true,
            first_type: 2,
            flags: 0,
            replace_on_filter: true,
        };
        refresh(8, 1, &mut trace)?;
        assert_eq!(
            trace.calls,
            vec![
                "type",
                "filter 130",
                "other owner 140",
                "island 9",
                "update 40 1009"
            ]
        );
        Ok(())
    }
}
