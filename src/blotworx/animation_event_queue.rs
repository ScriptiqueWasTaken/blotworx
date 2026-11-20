//! Event normalization before controller export, DOL80225538..802259ec.
//! Deduplicate after mapping, by ID and payload POINTER identity, preserving
//! first occurrence. An authored reference ID represents one cloned payload
//! object within its owning character; payload string content is not identity.
use super::state_machine::Event;
use anyhow::Result;
use std::collections::BTreeSet;
pub fn normalize(
    events: impl IntoIterator<Item = Event>,
    mut map: impl FnMut(i32) -> Result<i32>,
) -> Result<Vec<Event>> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for mut event in events {
        event.id = map(event.id)?;
        if event.id < 0 {
            continue;
        }
        let identity = (
            event.id,
            event.payload.as_ref().map(|reference| reference.id.clone()),
        );
        if seen.insert(identity) {
            output.push(event);
        }
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicates_use_mapped_id_and_payload_identity_preserving_original_order() -> Result<()> {
        let event = |id, payload: Option<&str>| Event {
            id,
            payload: payload.map(|id| super::super::state_machine::Reference { id: id.into() }),
        };
        let result = normalize(
            [
                event(1, None),
                event(2, None),
                event(1, Some("a")),
                event(1, Some("b")),
                event(1, Some("a")),
                event(1, None),
                event(-1, None),
                event(9, None),
            ],
            |id| {
                Ok(match id {
                    2 => 1,
                    9 => -1,
                    _ => id,
                })
            },
        )?;
        assert_eq!(
            result
                .iter()
                .map(|event| (event.id, event.payload.as_ref().map(|p| p.id.as_str())))
                .collect::<Vec<_>>(),
            [(1, None), (1, Some("a")), (1, Some("b"))]
        );
        Ok(())
    }
}
