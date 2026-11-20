//! MickeyPlayerMotionController +454 gates, recovered from EM1 REL.
//! These flags are prerequisites, not a replacement for ledge casts/hanging.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct State {
    pub flags: u32,
}
impl State {
    /// Constructor958f4 clears the word; initialization968f4 resets it to7.
    /// Property refresh963e4 sets bit8 from DoesLedgeGrab (tableA90+38).
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "MickeyPlayerMotionControllerComponent",
            "Not Mickey's motion controller"
        );
        let mut properties = component
            .properties
            .iter()
            .filter(|property| property.name == "DoesLedgeGrab");
        let authored = properties
            .next()
            .map(|property| {
                property
                    .value
                    .as_bool()
                    .context("Invalid DoesLedgeGrab boolean")
            })
            .transpose()?
            .unwrap_or(false);
        ensure!(
            properties.next().is_none(),
            "Duplicate DoesLedgeGrab property"
        );
        let mut state = Self { flags: 7 };
        state.set_authored_gate(authored);
        Ok(state)
    }
    pub fn set_authored_gate(&mut self, enabled: bool) {
        if enabled {
            self.flags |= 8;
        } else {
            self.flags &= !8;
        }
    }
    ///9c858..870 changes only bit2, independently of the authored property.
    pub fn set_script_gate(&mut self, enabled: bool) {
        if enabled {
            self.flags |= 2;
        } else {
            self.flags &= !2;
        }
    }
    pub fn script_enabled(self) -> bool {
        self.flags & 2 != 0
    }
    ///9a6c4..d8 requires every low bit; other pointer/distance/query checks
    /// are mandatory later stages and are not represented by this result.
    pub fn flags_admit_query(self) -> bool {
        self.flags & 15 == 15
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_gate_preserves_other_prerequisites_and_high_flags() {
        let mut state = State { flags: 0xabc0000f };
        state.set_script_gate(false);
        assert_eq!(state.flags, 0xabc0000d);
        assert!(!state.flags_admit_query());
        state.set_script_gate(true);
        assert_eq!(state.flags, 0xabc0000f);
        assert!(state.flags_admit_query());
        for missing in [1, 2, 4, 8] {
            assert!(!State {
                flags: 15 & !missing
            }
            .flags_admit_query());
        }
    }
    #[test]
    fn authored_gate_is_independent_of_script_enable() -> Result<()> {
        let mut component = Component::new(
            "MickeyPlayerMotionControllerComponent".into(),
            String::new(),
            epic_mickey_lib_rs::scene_file::ID::new(0),
            epic_mickey_lib_rs::scene_file::ID::new(0),
            epic_mickey_lib_rs::scene_file::ID::new(0),
            vec![],
        );
        let mut state = State::from_component(&component)?;
        assert_eq!(state.flags, 7);
        state.set_script_gate(true);
        assert!(!state.flags_admit_query());
        state.set_authored_gate(true);
        assert!(state.flags_admit_query());
        state.set_script_gate(false);
        assert_eq!(state.flags, 13);
        component
            .properties
            .push(epic_mickey_lib_rs::scene_file::Property::new(
                "Boolean".into(),
                "DoesLedgeGrab".into(),
                false,
                false,
                false,
                serde_json::json!("invalid"),
            ));
        assert!(State::from_component(&component).is_err());
        Ok(())
    }
}
