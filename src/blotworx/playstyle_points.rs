//! Game-state points at REL18b88/18b98/18ba4, not the ability stat percentage.
//! Scoring events, reconciliation1a734 and UI notifications remain unimplemented.
use super::save_game;
use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct State {
    committed: f32,
    last_notified: f32,
    pending: f32,
}
impl State {
    /// REL1a8a8: the selector is a zero/nonzero test, not a channel index.
    pub fn points(&self, include_pending: i32) -> f32 {
        if include_pending == 0 {
            self.committed
        } else {
            self.committed + self.pending
        }
    }
    /// REL1a870: replacement resets both the other balance and notification
    /// comparison value. It does not clamp, accumulate, or notify the UI.
    pub fn replace(&mut self, value: f32, pending: i32) {
        self.last_notified = 0.;
        if pending == 0 {
            self.committed = value;
            self.pending = 0.;
        } else {
            self.pending = value;
            self.committed = 0.;
        }
    }
    /// REL20c84 always selects the sum; never substitute neutral percentage50.
    pub fn append_record(&self, output: &mut Vec<u8>) -> Result<()> {
        save_game::append_record(
            output,
            "PlaystylePoints",
            &self.points(1).to_bits().to_be_bytes(),
            4,
        )
    }
    /// REL20b3c: current record takes priority over the legacy guardian/type
    /// representation. Unknown legacy types leave the owner unchanged.
    /// This updates only this owner; it is not full gameplay restoration.
    pub fn restore_record(&mut self, payload: &[u8]) -> Result<()> {
        let records = save_game::records(payload)?;
        let word = |name: &str| -> Result<Option<[u8; 4]>> {
            records
                .iter()
                .find(|record| record.name == name)
                .map(|record| {
                    Ok(record
                        .data
                        .get(..4)
                        .with_context(|| format!("Truncated {name}"))?
                        .try_into()?)
                })
                .transpose()
        };
        if let Some(value) = word("PLAYSTYLEPOINTS")? {
            self.replace(f32::from_bits(u32::from_be_bytes(value)), 1);
        } else {
            let guardians = word("NUMGUARDIANS")?.map(i32::from_be_bytes).unwrap_or(0);
            let kind = word("PLAYSTYLE")?.map(i32::from_be_bytes).unwrap_or(50);
            let value = match kind {
                49 => Some(guardians as f32 * -15.),
                51 => Some(guardians as f32 * 15.),
                50 => Some(0.),
                _ => None,
            };
            if let Some(value) = value {
                self.replace(value, 1);
            }
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"committed":self.committed,"pending":self.pending,"last_notified":self.last_notified,
            "saved_points":self.points(1),"saved_bits":self.points(1).to_bits(),
            "scoring_events_implemented":false,"reconciliation_implemented":false,
            "ui_notifications_implemented":false,"checkpoint_integrated":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn point_owner_matches_original_instruction_cases() -> Result<()> {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("playstyle_points_reference.json"))?;
        for case in reference["cases"].as_array().context("Missing cases")? {
            let bits = |key: &str| -> u32 { case[key].as_u64().unwrap() as u32 };
            let mut state = State {
                committed: f32::from_bits(bits("initial_committed")),
                pending: f32::from_bits(bits("initial_pending")),
                last_notified: f32::from_bits(bits("initial_notified")),
            };
            state.replace(
                f32::from_bits(bits("value")),
                case["selector"].as_i64().unwrap() as i32,
            );
            assert_eq!(state.committed.to_bits(), bits("committed"));
            assert_eq!(state.pending.to_bits(), bits("pending"));
            assert_eq!(state.last_notified.to_bits(), bits("notified"));
            assert_eq!(state.points(0).to_bits(), bits("query_committed"));
            assert_eq!(state.points(1).to_bits(), bits("query_total"));
            assert_eq!(state.points(-1).to_bits(), bits("query_total"));
        }
        Ok(())
    }
    #[test]
    fn legacy_restore_priority_and_unknown_type_follow_native_branching() -> Result<()> {
        let mut state = State::default();
        assert_eq!(state.points(1), 0.);
        for (kind, expected) in [(49i32, -45.), (51, 45.), (50, 0.)] {
            let mut payload = vec![];
            save_game::append_record(&mut payload, "NumGuardians", &3i32.to_be_bytes(), 4)?;
            save_game::append_record(&mut payload, "Playstyle", &kind.to_be_bytes(), 4)?;
            state.restore_record(&payload)?;
            assert_eq!(state.points(1), expected);
            save_game::append_record(
                &mut payload,
                "PlaystylePoints",
                &(-44.25f32).to_be_bytes(),
                4,
            )?;
            state.restore_record(&payload)?;
            assert_eq!(state.points(1), -44.25);
        }
        let mut invalid = vec![];
        save_game::append_record(&mut invalid, "Playstyle", &99i32.to_be_bytes(), 4)?;
        let before = state;
        state.restore_record(&invalid)?;
        assert_eq!(state, before);
        let mut truncated = vec![];
        save_game::append_record(&mut truncated, "PlaystylePoints", &[1, 2], 4)?;
        assert!(state.restore_record(&truncated).is_err());
        assert_eq!(state, before);
        state.restore_record(&[])?;
        assert_eq!(state.points(1), 0.);
        Ok(())
    }
    #[test]
    #[ignore = "requires native save in BLOTWORX_TEST_SAVE"]
    fn original_mick_point_record_rewrites_without_modifying_native_save() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let original = std::fs::read(&path)?;
        let archive = save_game::Archive::parse(&original)?;
        let section = archive.section(*b"MICK")?;
        let mut state = State::default();
        state.restore_record(&section.payload)?;
        let source = save_game::records(&section.payload)?
            .into_iter()
            .find(|record| record.name == "PLAYSTYLEPOINTS")
            .context("No native points")?;
        let mut encoded = vec![];
        state.append_record(&mut encoded)?;
        let records = save_game::records(&encoded)?;
        assert_eq!(records[0].name, source.name);
        assert_eq!(records[0].data, source.data);
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }
}
