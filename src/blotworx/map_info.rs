//! GameState pause-map metadata, EM1 REL1f9618 /1f7c64 /1f7cb8.
//! This state does not request a level transition or fabricate a pause menu.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct State {
    /// GameState+58, consumed as _global.PAUSE_MENU_MAP and a map texture stem.
    pub map: String,
    /// GameState+5c, consumed as _global.MAPTYPE.
    pub map_type: String,
}
impl Default for State {
    fn default() -> Self {
        // REL1f6668..1f66c8 initializes both interned strings to UNKNOWN.
        Self {
            map: "UNKNOWN".into(),
            map_type: "UNKNOWN".into(),
        }
    }
}

/// EM2 PC MapManager pointer leaves (aeadb0/aeae40). Rendering the selected
/// MapVolume through the original HUD is separate from these assignments.
#[derive(Default, Debug, serde::Serialize)]
pub struct Volumes {
    pub sub_map: Option<usize>,
    pub full_map: Option<usize>,
    pub sub_map_assignments: u64,
    pub full_map_assignments: u64,
}
impl Volumes {
    /// PC writes even null. Wii U also sets a dirty byte; PC does not.
    pub fn set_sub_map(&mut self, owner: Option<usize>) {
        self.sub_map = owner;
        self.sub_map_assignments += 1;
    }
    /// The full-map native wrapper ignores an invalid/non-map argument.
    pub fn set_full_map(&mut self, owner: Option<usize>) {
        if let Some(owner) = owner {
            self.full_map = Some(owner);
            self.full_map_assignments += 1;
        }
    }
}
#[cfg(test)]
mod volume_tests {
    use super::*;
    #[test]
    fn native_null_submap_clears_but_null_fullmap_preserves_selection() {
        let mut maps = Volumes::default();
        maps.set_sub_map(Some(7));
        maps.set_full_map(Some(7));
        maps.set_sub_map(None);
        maps.set_full_map(None);
        assert_eq!(maps.sub_map, None);
        assert_eq!(maps.full_map, Some(7));
        assert_eq!(maps.sub_map_assignments, 2);
        assert_eq!(maps.full_map_assignments, 1);
    }
}
