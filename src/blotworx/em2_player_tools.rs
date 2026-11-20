//! PC979270/979330/979630: slot flag, child visibility and HasTool write.
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, VecDeque};
#[derive(Clone, serde::Serialize)]
pub(super) struct AnimationWrite {
    pub owner: usize,
    pub id: u32,
    pub value: bool,
}
#[derive(Clone,serde::Serialize)]
pub(super) struct Visibility {
    pub owner: usize,
    pub template: String,
    pub hidden: bool,
}
#[derive(serde::Serialize)]
pub(super) struct State {
    hidden: [bool; 2],
    names: [String; 2],
    pub pending_animation: VecDeque<AnimationWrite>,
    pub pending_visibility: VecDeque<Visibility>,
    applied_animation: u64,
}
impl State {
    pub fn new(settings: &BTreeMap<String, String>) -> Self {
        let names = std::array::from_fn(|i| {
            let (key, default) = if i == 0 {
                ("MickeyAbilities_Tool_Name", "MagicBrush")
            } else {
                ("OswaldAbilities_Tool_Name", "oswald_remote")
            };
            settings
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.split('\0').next().unwrap_or_default())
                .unwrap_or(default)
                .to_owned()
        });
        Self {
            hidden: [false; 2],
            names,
            pending_animation: VecDeque::new(),
            pending_visibility: VecDeque::new(),
            applied_animation: 0,
        }
    }
    pub fn set_hidden(
        &mut self,
        index: usize,
        hidden: bool,
        owner: Option<usize>,
        has_animation: bool,
    ) -> Result<()> {
        ensure!(index < 2, "Invalid PC tool player slot {index}");
        self.hidden[index] = hidden;
        if let Some(owner) = owner {
            self.pending_visibility.push_back(Visibility {
                owner,
                template: self.names[index].clone(),
                hidden,
            });
            if has_animation {
                self.pending_animation.push_back(AnimationWrite {
                    owner,
                    id: 13,
                    value: !hidden,
                });
            }
        }
        Ok(())
    }
    pub fn animation_applied(&mut self) {
        self.pending_animation.pop_front();
        self.applied_animation += 1;
    }
    pub fn retire(&mut self, owner: usize) {
        self.pending_animation.retain(|w| w.owner != owner);
        self.pending_visibility.retain(|w| w.owner != owner);
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"state":self,"tool_child_lookup_connected":true,
            "scope":"PC tool visibility slot producer and live HasTool graph writes; complete child lifecycle and component hide hooks remain incomplete"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slot_and_graph_write_match_native_order_even_for_repeat_assignments() -> Result<()> {
        let mut s = State::new(&BTreeMap::from([(
            "MickeyAbilities_Tool_Name".into(),
            "CustomBrush".into(),
        )]));
        s.set_hidden(0, true, Some(7), true)?;
        s.set_hidden(0, true, Some(7), true)?;
        assert!(s.hidden[0]);
        assert_eq!(s.pending_animation.len(), 2);
        assert_eq!(s.pending_visibility[0].template, "CustomBrush");
        assert_eq!(s.pending_animation[0].id, 13);
        assert!(!s.pending_animation[0].value);
        s.animation_applied();
        assert_eq!(s.pending_animation.len(), 1);
        s.set_hidden(0, false, Some(7), true)?;
        assert!(s.pending_animation.back().unwrap().value);
        assert!(s.set_hidden(2, false, None, false).is_err());
        Ok(())
    }
    #[test]
    fn empty_slot_keeps_flag_without_fabricating_a_tool_or_animation() -> Result<()> {
        let mut s = State::new(&BTreeMap::new());
        s.set_hidden(1, true, None, true)?;
        assert!(s.hidden[1]);
        assert!(s.pending_animation.is_empty() && s.pending_visibility.is_empty());
        s.set_hidden(1, false, Some(9), false)?;
        assert!(s.pending_animation.is_empty());
        assert_eq!(s.pending_visibility.len(), 1);
        s.retire(9);
        assert!(s.pending_visibility.is_empty());
        Ok(())
    }
}
