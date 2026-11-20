//! MICK ability ownership writer20f1c and reader21098.
//! Ability registration order and derived Give side effects belong to the caller.
use super::{ability_ownership::Ownership, save_game};
use anyhow::{ensure, Result};

/// vtable+3c is raw ownership2108c; effective enabled8b578 is +40.
/// Temporary disabling must not erase ownership in a checkpoint.
pub fn mask(state: &Ownership) -> i32 {
    i32::from(state.owned[0]) | i32::from(state.owned[1]) << 1
}

pub fn append(output: &mut Vec<u8>, name: &str, state: &Ownership) -> Result<()> {
    // Native virtual+58 with selector0 reads the Paint upgrade for the key,
    // even when only Thinner is owned or the two upgrades differ.
    let key = format!("{name}:{}", state.upgrades[0]);
    save_game::append_record(output, &key, &mask(state).to_be_bytes(), 4)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grant {
    pub selector: &'static str,
    pub level: i32,
}
/// Reader probes suffixes0,1,2 in that order and stops at the first record
/// found, including masks0 and unsupported values. It does not revoke anything
/// for those masks. The grant level comes from the CURRENT ability's selector0
/// getter, not the suffix found in the save.
pub fn grant(
    records: &[save_game::Record<'_>],
    name: &str,
    current: &Ownership,
) -> Result<Option<Grant>> {
    for suffix in 0..3 {
        let key = format!("{name}:{suffix}").to_ascii_uppercase();
        // Native named-record lookup selects the first matching record.
        let Some(record) = records.iter().find(|record| record.name == key) else {
            continue;
        };
        ensure!(
            record.data.len() == 4,
            "Invalid ability ownership record width: {key}"
        );
        let value = i32::from_be_bytes(record.data.try_into()?);
        let selector = match value {
            1 => "Paint",
            2 => "Thinner",
            3 => "All",
            _ => return Ok(None),
        };
        return Ok(Some(Grant {
            selector,
            level: current.upgrades[0],
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_uses_raw_ownership_and_paint_upgrade_even_when_disabled() -> Result<()> {
        let state = Ownership {
            owned: [false, true],
            upgrades: [-2, 7],
            disabled: true,
        };
        let mut bytes = Vec::new();
        append(&mut bytes, "Brush", &state)?;
        let records = save_game::records(&bytes)?;
        assert_eq!(records[0].name, "BRUSH:-2");
        assert_eq!(records[0].data, &2_i32.to_be_bytes());
        assert_eq!(mask(&state), 2);
        Ok(())
    }
    #[test]
    fn restore_stops_at_first_suffix_and_zero_never_revokes() -> Result<()> {
        let current = Ownership {
            owned: [true, true],
            upgrades: [7, -3],
            disabled: true,
        };
        let mut bytes = Vec::new();
        save_game::append_record(&mut bytes, "Brush:2", &3_i32.to_be_bytes(), 4)?;
        save_game::append_record(&mut bytes, "Brush:0", &0_i32.to_be_bytes(), 4)?;
        let records = save_game::records(&bytes)?;
        assert_eq!(grant(&records, "Brush", &current)?, None);
        let records = &records[..1];
        assert_eq!(
            grant(records, "Brush", &current)?,
            Some(Grant {
                selector: "All",
                level: 7
            })
        );
        assert!(current.disabled);
        assert_eq!(current.owned, [true, true]);
        Ok(())
    }
    #[test]
    fn restore_ignores_unprobed_levels_and_preserves_channel_selection() -> Result<()> {
        let current = Ownership::default();
        for (value, selector) in [
            (1, Some("Paint")),
            (2, Some("Thinner")),
            (3, Some("All")),
            (-1, None),
            (4, None),
        ] {
            let mut bytes = Vec::new();
            save_game::append_record(&mut bytes, "Blast:1", &i32::to_be_bytes(value), 4)?;
            assert_eq!(
                grant(&save_game::records(&bytes)?, "Blast", &current)?.map(|grant| grant.selector),
                selector
            );
        }
        let mut bytes = Vec::new();
        save_game::append_record(&mut bytes, "Blast:3", &3_i32.to_be_bytes(), 4)?;
        assert_eq!(
            grant(&save_game::records(&bytes)?, "Blast", &current)?,
            None
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 save at BLOTWORX_TEST_SAVE"]
    fn original_ability_records_decode_through_native_probe_order() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_SAVE"))?;
        let bytes = std::fs::read(path)?;
        let save = save_game::Archive::parse(&bytes)?;
        let section = save.section(*b"MICK")?;
        ensure!(section.version == 3, "Unsupported MICK version");
        let records = save_game::records(&section.payload)?;
        for name in [
            "Blast",
            "Invisible",
            "Indelible",
            "Brush",
            "Slide",
            "Spin",
            "Slash",
            "Sketch_TV",
            "Sketch_Anvil",
            "Sketch_Watch",
        ] {
            let current = Ownership::default();
            // A well-formed zero mask is still a found native record; it need
            // not produce a grant. Validate the stored field separately.
            let prefix = format!("{}:", name.to_ascii_uppercase());
            let record = records
                .iter()
                .find(|record| record.name.starts_with(&prefix))
                .ok_or_else(|| anyhow::anyhow!("Missing original ability {name}"))?;
            assert_eq!(record.data.len(), 4);
            let value = i32::from_be_bytes(record.data.try_into()?);
            assert!((0..=3).contains(&value));
            let restored = grant(&records, name, &current)?;
            assert_eq!(
                restored.map(|entry| entry.selector),
                match value {
                    1 => Some("Paint"),
                    2 => Some("Thinner"),
                    3 => Some("All"),
                    _ => None,
                }
            );
        }
        Ok(())
    }
}
