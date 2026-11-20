//! Native Apprentice quest save owner, REL2451b8/245b70/245c88.
//! Owns constructor/restored state; quest evaluation, notifications and UI are
//! separate required systems. Do not infer progress from the opaque status byte.
use crate::em1_bsq::{Document, Node};
use anyhow::{ensure, Context, Result};

#[derive(Clone, Debug)]
pub struct Definition {
    pub name: String,
    stages: Vec<Stage>,
}

#[derive(Clone, Copy, Debug)]
struct Stage {
    id: u16,
    completion_type: u16,
}

impl Definition {
    fn stage(&self, state: u16) -> Option<&Stage> {
        self.stages.iter().find(|stage| stage.id == state)
    }

    #[cfg(test)]
    fn state_ids(&self) -> Vec<u16> {
        self.stages.iter().map(|stage| stage.id).collect()
    }
}
#[derive(Clone, Debug)]
pub struct Slot {
    pub definition: Option<Definition>,
    pub current: u16,
    pub status: u8,
}
impl Slot {
    fn empty() -> Self {
        Self {
            definition: None,
            current: u16::MAX,
            status: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct State {
    pub slots: Vec<Slot>,
}
#[derive(Debug, serde::Serialize, PartialEq)]
pub struct Restored {
    pub saved_slots: usize,
    pub visited_slots: usize,
    pub matched_states: usize,
}
/// A state write is not completion of the native transition. REL244f64 calls
///244de4 afterwards: cancel the prior listener, post UI event ae and retain its
///new listener. The conversation event must wait for that notification path.
#[must_use = "A quest state change still requires native notification/listener delivery"]
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Transition {
    pub quest: usize,
    pub previous: u16,
    pub state: u16,
}
fn items<'a>(record: &'a Node, name: &str) -> Result<&'a [Node]> {
    let Node::Record { fields, .. } = record else {
        anyhow::bail!("Quest metadata is not a record")
    };
    fields
        .iter()
        .find_map(|node| match node {
            Node::List {
                name: field, items, ..
            } if field == name => Some(items.as_slice()),
            _ => None,
        })
        .with_context(|| format!("Missing quest metadata list {name}"))
}
impl State {
    /// REL244f64..24500c: authored state IDs, not stage ordinals. No status
    /// byte changes. Returning Some does not claim that the UI event completed.
    pub fn begin_transition(&mut self, quest: i32, state: u16) -> Option<Transition> {
        let quest = usize::try_from(quest).ok()?;
        let slot = self.slots.get_mut(quest)?;
        if slot.current == state
            || !slot
                .definition
                .as_ref()
                .is_some_and(|definition| definition.stage(state).is_some())
        {
            return None;
        }
        let previous = slot.current;
        slot.current = state;
        Some(Transition {
            quest,
            previous,
            state,
        })
    }
    /// REL24506c..2450dc, called by event4 when its Silent flag is true.
    /// A valid slot gets status=1 even for an unknown state or an empty slot.
    /// This differs from normal transitions and from save restore normalization.
    pub fn set_silent(&mut self, quest: i32, state: u16) {
        let Ok(quest) = usize::try_from(quest) else {
            return;
        };
        let Some(slot) = self.slots.get_mut(quest) else {
            return;
        };
        if slot
            .definition
            .as_ref()
            .is_some_and(|definition| definition.stage(state).is_some())
        {
            slot.current = state;
        }
        slot.status = 1;
    }
    /// BSQ event 16 (REL27d980 -> 245010). Preserve the status of
    /// quests whose current stage has completion type 1 or 2.
    pub fn mark_status_if_incomplete(&mut self, quest: u16) {
        let Some(slot) = self.slots.get_mut(usize::from(quest)) else {
            return;
        };
        let completed = slot.current != u16::MAX
            && slot.definition.as_ref()
                .and_then(|definition| definition.stage(slot.current))
                .is_some_and(|stage| matches!(stage.completion_type, 1 | 2));
        if !completed {
            slot.status = 1;
        }
    }

    /// REL246168..2461dc: resets allocated holes as well as authored quests.
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.current = u16::MAX;
            slot.status = 0;
        }
    }
    pub fn load(path: &str, bytes: &[u8]) -> Result<Self> {
        Self::from_document(&Document::load(path, bytes)?)
    }
    fn from_document(document: &Document) -> Result<Self> {
        ensure!(
            document.database,
            "Quest asset is not an Apprentice database"
        );
        let count = document
            .root
            .value("_id_limit")
            .and_then(|value| value.as_u64())
            .context("Quest slot count missing")?;
        //245238..2452b4 uses this count directly, not global-state's limit+1.
        // Diagnose corrupt overflow rather than emulate an out-of-bounds load.
        let count = u16::try_from(count).context("Quest slot count exceeds native u16")? as usize;
        let mut state = Self {
            slots: (0..count).map(|_| Slot::empty()).collect(),
        };
        for quest in items(&document.root, "Quests")? {
            let id = quest
                .value("ID")
                .and_then(|value| value.as_u64())
                .context("Quest ID missing")?;
            let slot = state
                .slots
                .get_mut(usize::try_from(id)?)
                .context("Quest ID exceeds authored slot count")?;
            ensure!(
                slot.definition.is_none(),
                "Duplicate authored quest ID {id}"
            );
            let name = quest
                .value("Name")
                .and_then(|value| value.as_str())
                .context("Quest name missing")?
                .to_owned();
            let stages = items(quest, "Stages")?;
            ensure!(
                u16::try_from(stages.len()).is_ok(),
                "Quest state count exceeds native u16 field"
            );
            let stages = stages
                .iter()
                .map(|stage| {
                    //24569c loads this short at stage+0;244c70/245d54 compare it
                    // with current/save state IDs. Accept the old editor label in
                    // retained structured documents without changing binary layout.
                    let id = stage
                        .value("State ID")
                        .or_else(|| stage.value("Condition type"))
                        .and_then(|value| value.as_u64())
                        .context("Quest state ID missing")?;
                    let completion_type = stage
                        .value("Completion type")
                        .and_then(|value| value.as_u64())
                        .context("Quest stage completion type missing")?;
                    Ok(Stage {
                        id: u16::try_from(id).context("Quest state ID exceeds native u16")?,
                        completion_type: u16::try_from(completion_type)
                            .context("Quest completion type exceeds native u16")?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            slot.definition = Some(Definition { name, stages });
        }
        Ok(state)
    }
    ///245dac..245e40 writes every allocated slot, including unnamed holes.
    /// The caller's EndSection supplies final four-byte zero alignment.
    pub fn save_payload(&self) -> Result<Vec<u8>> {
        let count = u16::try_from(self.slots.len()).context("Quest count exceeds native u16")?;
        let mut output = Vec::new();
        super::save_game::append_record(
            &mut output,
            "QuestDataSize",
            &u32::from(count).to_be_bytes(),
            4,
        )?;
        let mut data = Vec::with_capacity(self.slots.len() * 3);
        for slot in &self.slots {
            data.extend(slot.current.to_be_bytes());
            data.push(slot.status);
        }
        super::save_game::append_record(&mut output, "QuestData", &data, 4)?;
        Ok(output)
    }
    ///245cc0..245d9c applies the shared prefix and normalizes status bytes.
    /// Unknown saved state IDs preserve current state, including savedffff when
    /// no authored state has that ID. Shorter snapshots leave later slots alone.
    pub fn restore_payload(&mut self, payload: &[u8]) -> Result<Restored> {
        let records = super::save_game::records(payload)?;
        // Native named lookup takes the first match. Missing size leaves its
        // caller-initialized zero count unchanged; no quest values are touched.
        let count = match records.iter().find(|record| record.name == "QUESTDATASIZE") {
            Some(record) => {
                ensure!(record.data.len() >= 4, "Truncated quest save count");
                u32::from_be_bytes(record.data[..4].try_into()?) as usize
            }
            None => 0,
        };
        if count == 0 {
            return Ok(Restored {
                saved_slots: 0,
                visited_slots: 0,
                matched_states: 0,
            });
        }
        let length = count
            .checked_mul(3)
            .context("Quest save array size overflow")?;
        let data = records
            .iter()
            .find(|record| record.name == "QUESTDATA")
            .context("Quest save array missing")?
            .data;
        // Native malformed/missing data can expose uninitialized temporary
        // memory. Refuse that path rather than inventing state/flag bytes.
        ensure!(data.len() >= length, "Truncated quest save array");
        let mut matched_states = 0;
        let visited_slots = count.min(self.slots.len());
        for (slot, record) in self.slots.iter_mut().zip(data[..length].chunks_exact(3)) {
            let saved = u16::from_be_bytes(record[..2].try_into()?);
            if slot
                .definition
                .as_ref()
                .is_some_and(|definition| definition.stage(saved).is_some())
            {
                slot.current = saved;
                matched_states += 1;
            }
            slot.status = u8::from(record[2] != 0);
        }
        Ok(Restored {
            saved_slots: count,
            visited_slots,
            matched_states,
        })
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"slots":self.slots.len(),"authored":self.slots.iter().filter(|slot|slot.definition.is_some()).count(),
            "current_states":self.slots.iter().filter(|slot|slot.current!=u16::MAX).count(),
            "status_nonzero":self.slots.iter().filter(|slot|slot.status!=0).count(),
            "runtime_evaluation_implemented":false,"checkpoint_integrated":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State {
            slots: vec![
                Slot {
                    definition: Some(Definition {
                        name: "one".into(),
                        stages: vec![
                            Stage { id: 10, completion_type: 0 },
                            Stage { id: 20, completion_type: 1 },
                            Stage { id: 20, completion_type: 0 },
                        ],
                    }),
                    current: u16::MAX,
                    status: 0,
                },
                Slot::empty(),
                Slot {
                    definition: Some(Definition {
                        name: "two".into(),
                        stages: vec![
                            Stage { id: 0, completion_type: 2 },
                            Stage { id: 65534, completion_type: 3 },
                        ],
                    }),
                    current: u16::MAX,
                    status: 0,
                },
            ],
        }
    }
    fn saved(values: &[(u16, u8)]) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        super::super::save_game::append_record(
            &mut bytes,
            "QuestDataSize",
            &(values.len() as u32).to_be_bytes(),
            4,
        )?;
        let data = values
            .iter()
            .flat_map(|&(stage, flag)| {
                let [a, b] = stage.to_be_bytes();
                [a, b, flag]
            })
            .collect::<Vec<_>>();
        super::super::save_game::append_record(&mut bytes, "QuestData", &data, 4)?;
        Ok(bytes)
    }
    #[test]
    fn native_transitions_use_authored_ids_and_leave_notification_completion_pending() {
        let mut state = state();
        state.slots[0].status = 0x80;
        for quest in [-1, 3, i32::MAX] {
            assert!(state.begin_transition(quest, 20).is_none());
        }
        assert!(state.begin_transition(1, 20).is_none());
        assert!(
            state.begin_transition(0, 1).is_none(),
            "An ordinal must not select an authored state"
        );
        assert_eq!(
            state.begin_transition(0, 20),
            Some(Transition {
                quest: 0,
                previous: 65535,
                state: 20
            })
        );
        assert_eq!(state.slots[0].status, 0x80);
        assert!(
            state.begin_transition(0, 20).is_none(),
            "Repeated state must not request another notification"
        );
        assert!(state.begin_transition(0, 65535).is_none());
        assert_eq!(state.slots[0].current, 20);
        assert_eq!(
            state.begin_transition(0, 10),
            Some(Transition {
                quest: 0,
                previous: 20,
                state: 10
            })
        );
    }
    #[test]
    fn quest_updates_match_original_instruction_reference() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/quest_transition_instruction_reference.json"
        ))?;
        for case in reference["cases"]
            .as_array()
            .context("Missing original instruction cases")?
        {
            let mut state = state();
            state.slots[0].current = case["initial_current"].as_u64().unwrap() as u16;
            state.slots[0].status = 128;
            let quest = case["quest"].as_i64().unwrap() as i32;
            let value = case["state"].as_u64().unwrap() as u16;
            if case["mode"] == "silent" {
                state.set_silent(quest, value);
            } else {
                let transition = state.begin_transition(quest, value);
                assert_eq!(transition.is_some(), case["result"] == 1, "{case}");
                let notifications = case["notifications"].as_array().unwrap();
                assert_eq!(
                    usize::from(transition.is_some()),
                    notifications.len(),
                    "{case}"
                );
                if let Some(transition) = transition {
                    assert_eq!(
                        transition.quest as u64,
                        notifications[0]["quest"].as_u64().unwrap()
                    );
                    assert_eq!(
                        u64::from(transition.state),
                        notifications[0]["state"].as_u64().unwrap()
                    );
                }
            }
            assert_eq!(
                serde_json::json!(state
                    .slots
                    .iter()
                    .map(|slot| [slot.current, u16::from(slot.status)])
                    .collect::<Vec<_>>()),
                case["slots"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn silent_updates_mark_even_unknown_states_and_empty_slots_then_reset_every_slot() {
        let mut state = state();
        state.set_silent(0, 20);
        assert_eq!((state.slots[0].current, state.slots[0].status), (20, 1));
        state.slots[0].status = 0;
        state.set_silent(0, 999);
        assert_eq!((state.slots[0].current, state.slots[0].status), (20, 1));
        state.set_silent(1, 10);
        assert_eq!((state.slots[1].current, state.slots[1].status), (65535, 1));
        state.set_silent(-1, 0);
        state.set_silent(3, 0);
        assert_eq!((state.slots[2].current, state.slots[2].status), (65535, 0));
        state.reset();
        assert!(state
            .slots
            .iter()
            .all(|slot| slot.current == 65535 && slot.status == 0));
        assert_eq!(
            state.slots[0].definition.as_ref().unwrap().state_ids(),
            [10, 20, 20]
        );
    }
    #[test]
    fn event_16_marks_existing_slots_except_completed_stages() {
        let mut state = state();
        for (quest, current, expected) in [
            (0, 10, 1), (0, 20, 0), (0, 99, 1), (0, u16::MAX, 1),
            (1, u16::MAX, 1), (2, 0, 0), (2, 65534, 1),
        ] {
            state.slots[quest].current = current;
            state.slots[quest].status = 0;
            state.mark_status_if_incomplete(quest as u16);
            assert_eq!((state.slots[quest].current, state.slots[quest].status), (current, expected));
        }
        state.mark_status_if_incomplete(u16::MAX);
    }

    #[test]
    fn database_constructor_preserves_sparse_ids_and_legacy_labels() -> Result<()> {
        use crate::em1_bsq::Kind;
        let field = |name: &str, kind: Kind, value: serde_json::Value| Node::Field {
            name: name.into(),
            kind,
            baseline: value.clone(),
            value,
            raw: vec![],
        };
        for label in ["State ID", "Condition type"] {
            let stage = Node::Record {
                name: "Stage".into(),
                fields: vec![
                    field(label, Kind::Short, serde_json::json!(40)),
                    field("Completion type", Kind::Short, serde_json::json!(2)),
                ],
            };
            let quest = Node::Record {
                name: "Quest".into(),
                fields: vec![
                    field("ID", Kind::Short, serde_json::json!(2)),
                    field("Name", Kind::String, serde_json::json!("two")),
                    Node::List {
                        name: "Stages".into(),
                        count: crate::em1_bsq::Count::U32,
                        items: vec![stage],
                    },
                ],
            };
            let document = Document {
                database: true,
                root: Node::Record {
                    name: "Database".into(),
                    fields: vec![
                        field("_id_limit", Kind::U32, serde_json::json!(3)),
                        Node::List {
                            name: "Quests".into(),
                            count: crate::em1_bsq::Count::UntilEnd,
                            items: vec![quest],
                        },
                    ],
                },
            };
            let state = State::from_document(&document)?;
            assert_eq!(state.slots.len(), 3);
            assert!(state.slots[0].definition.is_none());
            assert!(state.slots[1].definition.is_none());
            assert_eq!(state.slots[2].definition.as_ref().unwrap().state_ids(), [40]);
            assert!(state
                .slots
                .iter()
                .all(|slot| slot.current == 65535 && slot.status == 0));
        }
        Ok(())
    }
    #[test]
    fn native_restore_retains_unknown_states_normalizes_flags_and_handles_count_changes(
    ) -> Result<()> {
        let mut state = state();
        assert_eq!(
            state.restore_payload(&saved(&[(20, 255), (999, 2), (0, 0), (99, 1)])?)?,
            Restored {
                saved_slots: 4,
                visited_slots: 3,
                matched_states: 2
            }
        );
        assert_eq!(
            state
                .slots
                .iter()
                .map(|s| (s.current, s.status))
                .collect::<Vec<_>>(),
            [(20, 1), (65535, 1), (0, 0)]
        );
        state.restore_payload(&saved(&[(65535, 0), (10, 0)])?)?;
        assert_eq!(
            state
                .slots
                .iter()
                .map(|s| (s.current, s.status))
                .collect::<Vec<_>>(),
            [(20, 0), (65535, 0), (0, 0)]
        );
        assert_eq!(state.restore_payload(&[])?.visited_slots, 0);
        assert!(state.restore_payload(&saved(&[(10, 1)])?[..25]).is_err());
        assert_eq!(state.slots[0].current, 20);
        Ok(())
    }
    #[test]
    fn native_writer_keeps_slot_order_holes_and_raw_status_bytes() -> Result<()> {
        let mut state = state();
        state.slots[0].current = 20;
        state.slots[0].status = 0x80;
        let bytes = state.save_payload()?;
        let records = super::super::save_game::records(&bytes)?;
        assert_eq!(records[0].name, "QUESTDATASIZE");
        assert_eq!(records[0].data, &3u32.to_be_bytes());
        assert_eq!(records[1].data, &[0, 20, 0x80, 255, 255, 0, 255, 255, 0]);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA and populated save in BLOTWORX_TEST_SAVE"]
    fn original_quest_owner_matches_authored_ids_and_restored_section() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut state = super::super::assets::quest_owner_from_game(std::path::Path::new(&root))?;
        assert_eq!(state.slots.len(), 214);
        assert!(state
            .slots
            .iter()
            .all(|slot| slot.current == 65535 && slot.status == 0));
        assert_eq!(
            state
                .slots
                .iter()
                .filter(|slot| slot.definition.is_some())
                .count(),
            155
        );
        let authored = state.slots[85]
            .definition
            .as_ref()
            .context("Original quest85 missing")?;
        assert_eq!(authored.name, "Quest_85");
        assert_eq!(
            authored.state_ids(),
            [4, 2, 1],
            "State IDs were replaced by stage ordinals"
        );
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let archive = super::super::save_game::Archive::parse(&bytes)?;
        let section = archive.section(*b"QUES")?;
        assert_eq!(section.version, 4);
        assert_eq!(state.restore_payload(&section.payload)?.visited_slots, 214);
        let mut writer = super::super::save_game::MemoryWriter::from_storage(bytes.clone())?;
        writer.append_section(*b"QUES", 4, &state.save_payload()?)?;
        let rebuilt = writer.finish()?;
        let copy = super::super::save_game::Archive::parse(&rebuilt.bytes)?;
        assert_eq!(
            &*copy.section(*b"QUES")?.payload,
            &*section.payload,
            "Original quest state did not roundtrip"
        );
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
}
