//! Retail playlist selection kernels,8032ce90 and8032d420. The caller owns
//! global/per-object history, shared C RNG and continuous-loop lifetime.
//! This is selection, not voice admission, resource lookup or audio rendering.
use super::container::PlaylistItem;
use anyhow::{ensure, Context, Result};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Loop {
    pub remaining: i16,
    pub flags: u8,
}
impl Loop {
    fn advance(value: &mut Option<Self>) -> bool {
        let Some(value) = value else { return true };
        if value.flags & 0x80 == 0 {
            return false;
        }
        if value.flags & 0x40 != 0 {
            return true;
        }
        value.remaining = value.remaining.wrapping_sub(1);
        value.remaining != 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Sequence {
    pub forward: bool,
    pub index: i16,
}
impl Default for Sequence {
    fn default() -> Self {
        Self {
            forward: true,
            index: -1,
        }
    }
}
impl Sequence {
    /// Direct kernel entry: the container bypasses this routine for zero/one
    /// item playlists. Keep its signed halfword index and mutation on loop end.
    pub fn next(
        &mut self,
        count: u16,
        restart_backward: bool,
        loops: &mut Option<Loop>,
    ) -> Option<u16> {
        if self.forward {
            if (self.index as i32 + 1) as u32 == count as u32 {
                if restart_backward {
                    self.index = self.index.wrapping_sub(1);
                    self.forward = false;
                } else {
                    self.index = 0;
                    if !Loop::advance(loops) {
                        return None;
                    }
                }
            } else {
                self.index = self.index.wrapping_add(1);
            }
        } else if self.index == 0 {
            self.index = 1;
            self.forward = true;
            if !Loop::advance(loops) {
                return None;
            }
        } else {
            self.index = self.index.wrapping_sub(1);
        }
        Some(self.index as u16)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Random {
    pub total_weight: u32,
    pub eligible_weight: u32,
    pub eligible_count: u16,
    pub remaining: u16,
    pub played: Vec<bool>,
    pub blocked: Vec<bool>,
    pub history: std::collections::VecDeque<u16>,
}
impl Random {
    pub fn new(playlist: &[PlaylistItem], weighted: bool) -> Result<Self> {
        let count =
            u16::try_from(playlist.len()).context("Playlist exceeds the native halfword count")?;
        let weight = if weighted {
            playlist.iter().map(|item| item.weight as u32).sum()
        } else {
            count as u32 * 50
        };
        Ok(Self {
            total_weight: weight,
            eligible_weight: weight,
            eligible_count: count,
            remaining: count,
            played: vec![false; playlist.len()],
            blocked: vec![false; playlist.len()],
            history: Default::default(),
        })
    }
    fn eligible(&self, index: usize, mode: u8, avoid: u16) -> bool {
        if mode == 0 {
            avoid == 0 || !self.blocked[index]
        } else {
            !self.played[index] && !self.blocked[index]
        }
    }
    /// D030 normal-mode repeat window. VecDeque preserves the native FIFO
    /// ordering without the original array's memmove after each expiration.
    fn avoid(&mut self, playlist: &[PlaylistItem], index: u16, avoid: u16) -> Result<()> {
        if avoid == 0 {
            return Ok(());
        }
        self.eligible_count = self.eligible_count.wrapping_sub(1);
        self.history.push_back(index);
        self.blocked[index as usize] = true;
        self.eligible_weight = self
            .eligible_weight
            .wrapping_sub(playlist[index as usize].weight as u32);
        if self.history.len() > usize::from(avoid.min((playlist.len() - 1) as u16)) {
            let old = self
                .history
                .pop_front()
                .context("Missing native repeat history")? as usize;
            self.blocked[old] = false;
            self.eligible_count = self.eligible_count.wrapping_add(1);
            self.eligible_weight = self
                .eligible_weight
                .wrapping_add(playlist[old].weight as u32);
        }
        Ok(())
    }
    pub fn next(
        &mut self,
        playlist: &[PlaylistItem],
        mode: u8,
        avoid: u16,
        weighted: bool,
        loops: &mut Option<Loop>,
        mut random: impl FnMut() -> u32,
    ) -> Result<Option<u16>> {
        ensure!(
            playlist.len() > 1
                && playlist.len() == self.played.len()
                && playlist.len() == self.blocked.len(),
            "Random kernel requires a stable multi-item playlist"
        );
        if self.remaining == 0 {
            if !Loop::advance(loops) {
                return Ok(None);
            }
            self.remaining = playlist.len() as u16;
            self.played.fill(false);
            self.eligible_count = playlist.len() as u16;
            if mode == 1 {
                self.eligible_weight = self.total_weight;
                for &index in &self.history {
                    self.eligible_weight = self
                        .eligible_weight
                        .wrapping_sub(playlist[index as usize].weight as u32);
                }
            }
            self.eligible_count = self.eligible_count.wrapping_sub(self.history.len() as u16);
        }
        let draw = random();
        ensure!(
            draw <= 0x7fff,
            "Playlist requires the original fifteen-bit C RNG output"
        );
        let divisor = if weighted {
            self.eligible_weight
        } else {
            self.eligible_count as u32
        };
        ensure!(
            divisor != 0,
            "Native playlist selection has zero eligible weight/count"
        );
        let target = (draw % divisor) as u16;
        let mut accumulated = -1i32;
        let mut selected = None;
        for (index, item) in playlist.iter().enumerate() {
            if self.eligible(index, mode, avoid) {
                accumulated += if weighted { item.weight as i32 } else { 1 };
            }
            if accumulated >= target as i32 {
                selected = Some(index as u16);
                break;
            }
        }
        let index = selected
            .context("Native playlist eligibility counters exceed its available entries")?;
        if mode == 0 {
            if !self.played[index as usize] {
                self.played[index as usize] = true;
                self.remaining = self.remaining.wrapping_sub(1);
            }
            self.avoid(playlist, index, avoid)?;
        } else {
            self.eligible_count = self.eligible_count.wrapping_sub(1);
            self.remaining = self.remaining.wrapping_sub(1);
            self.eligible_weight = self
                .eligible_weight
                .wrapping_sub(playlist[index as usize].weight as u32);
            self.played[index as usize] = true;
            self.history.push_back(index);
            self.blocked[index as usize] = true;
            let maximum = avoid.max(1).min((playlist.len() - 1) as u16);
            if self.history.len() > maximum as usize {
                let old = self
                    .history
                    .pop_front()
                    .context("Missing native shuffle history")? as usize;
                self.blocked[old] = false;
                if !self.played[old] {
                    self.eligible_count = self.eligible_count.wrapping_add(1);
                    self.eligible_weight = self
                        .eligible_weight
                        .wrapping_add(playlist[old].weight as u32);
                }
            }
        }
        Ok(Some(index))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum State {
    Sequence(Sequence),
    Random(Random),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Scope {
    Global,
    Object(Option<usize>),
}
struct Entry {
    settings: super::container::Settings,
    playlist: Vec<PlaylistItem>,
    state: State,
}
#[derive(Default)]
pub struct Runtime {
    entries: std::collections::BTreeMap<(u32, Scope), Entry>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Outcome {
    pub selected: Option<u32>,
    pub index: u16,
    /// Native output ID retains the last attempted child even if lookup fails.
    pub last_candidate: u32,
}
impl Runtime {
    fn key(id: u32, global: bool, owner: Option<usize>) -> (u32, Scope) {
        (
            id,
            if global {
                Scope::Global
            } else {
                Scope::Object(owner)
            },
        )
    }
    pub fn state(&self, id: u32, global: bool, owner: Option<usize>) -> Option<&State> {
        self.entries
            .get(&Self::key(id, global, owner))
            .map(|entry| &entry.state)
    }
    ///8032bd10 removes this object's registered history; the independent
    /// global history at+10c remains alive. SIS cleanup belongs to its owner.
    pub fn retire_object(&mut self, id: u32, owner: Option<usize>) -> bool {
        self.entries.remove(&(id, Scope::Object(owner))).is_some()
    }
    pub fn retire_owner(&mut self,owner:usize) {
        self.entries.retain(|(_,scope),_|*scope!=Scope::Object(Some(owner)));
    }
    /// Original8032c2a0 non-continuous selection. Resource lookup is supplied;
    /// this does not admit voices or choose an arbitrary substitute sound.
    pub fn select(
        &mut self,
        id: u32,
        settings: &super::container::Settings,
        playlist: &[PlaylistItem],
        owner: Option<usize>,
        mut loaded: impl FnMut(u32) -> bool,
        mut random: impl FnMut() -> u32,
    ) -> Result<Outcome> {
        ensure!(
            settings.continuous == 0,
            "Continuous container requires8032c610 scheduling, not single selection"
        );
        ensure!(
            settings.mode <= 1 && settings.random_mode <= 1,
            "Unverified container selection mode"
        );
        let count =
            u16::try_from(playlist.len()).context("Playlist exceeds the native halfword count")?;
        let mut outcome = Outcome {
            selected: None,
            index: 0,
            last_candidate: 0,
        };
        if count == 0 {
            return Ok(outcome);
        }
        if count == 1 {
            outcome.last_candidate = playlist[0].sound;
            if loaded(outcome.last_candidate) {
                outcome.selected = Some(outcome.last_candidate);
            }
            return Ok(outcome);
        }
        let weighted = playlist.iter().any(|item| item.weight != 50);
        let key = Self::key(id, settings.global != 0, owner);
        if !self.entries.contains_key(&key) {
            let state = if settings.mode == 1 {
                State::Sequence(Sequence::default())
            } else {
                State::Random(Random::new(playlist, weighted)?)
            };
            self.entries.insert(
                key.clone(),
                Entry {
                    settings: settings.clone(),
                    playlist: playlist.to_vec(),
                    state,
                },
            );
        }
        let entry = self.entries.get_mut(&key).unwrap();
        ensure!(
            entry.settings == *settings && entry.playlist == playlist,
            "Changing a live playlist requires native history invalidation"
        );
        let mut fallback = false;
        let mut index = 0u16;
        // This bound is the original playlist-length retry, not an engine cap.
        for _ in 0..count {
            let candidate = if fallback {
                index = index.wrapping_add(1);
                if index >= count {
                    index = 0;
                }
                let State::Random(state) = &mut entry.state else {
                    unreachable!()
                };
                if state.eligible(index as usize, settings.random_mode, settings.avoid_repeat) {
                    state.avoid(playlist, index, settings.avoid_repeat)?;
                    Some(index)
                } else {
                    None
                }
            } else {
                match &mut entry.state {
                    State::Sequence(state) => {
                        state.next(count, settings.restart_backward != 0, &mut None)
                    }
                    State::Random(state) => state.next(
                        playlist,
                        settings.random_mode,
                        settings.avoid_repeat,
                        weighted,
                        &mut None,
                        &mut random,
                    )?,
                }
            };
            if let Some(candidate) = candidate {
                index = candidate;
                let item = playlist
                    .get(index as usize)
                    .context("Native selection index exceeds playlist")?;
                outcome.last_candidate = item.sound;
                if loaded(item.sound) {
                    outcome.selected = Some(item.sound);
                    outcome.index = index;
                    return Ok(outcome);
                }
            } else if !fallback {
                break;
            }
            if !fallback && settings.mode == 0 && settings.random_mode != 1 {
                fallback = true;
            }
        }
        Ok(outcome)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_dispatch_scope_missing_children_fast_paths_and_rng_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_dispatch.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let playlist = case["weights"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, value)| PlaylistItem {
                    sound: 100 + i as u32,
                    weight: value.as_u64().unwrap() as u8,
                })
                .collect::<Vec<_>>();
            let global = case["global_scope"].as_bool().unwrap();
            let mode = case["mode"].as_u64().unwrap() as u8;
            let settings = super::super::container::Settings {
                loops: 1,
                transition: [0.; 3],
                avoid_repeat: case["avoid"].as_u64().unwrap() as u16,
                transition_mode: 0,
                random_mode: case["random_mode"].as_u64().unwrap() as u8,
                mode,
                unused_weight: 0,
                reset_playlist: 0,
                restart_backward: u8::from(case["reverse"].as_bool().unwrap()),
                continuous: 0,
                global: u8::from(global),
            };
            let mut runtime = Runtime::default();
            let mut rng = super::super::super::game_random::State::default();
            for (step, expected) in case["steps"].as_array().unwrap().iter().enumerate() {
                let owner = expected["owner"].as_u64().unwrap() as usize;
                let mut lookups = Vec::new();
                let outcome = runtime.select(
                    777,
                    &settings,
                    &playlist,
                    Some(owner),
                    |sound| {
                        lookups.push(sound);
                        expected["loaded"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|id| id.as_u64() == Some(sound as u64))
                    },
                    || rng.draw(),
                )?;
                let context = format!(
                    "mode={mode} random={} global={global} pattern={} step={step}",
                    settings.random_mode, case["pattern"]
                );
                assert_eq!(
                    expected["selected"],
                    serde_json::to_value(outcome.selected)?,
                    "{context}"
                );
                assert_eq!(expected["index"], outcome.index, "{context}");
                assert_eq!(
                    expected["last_candidate"], outcome.last_candidate,
                    "{context}"
                );
                assert_eq!(expected["lookups"], serde_json::json!(lookups), "{context}");
                assert_eq!(expected["seed"], rng.seed, "{context}");
                assert_eq!(expected["draws"], rng.draws, "{context}");
                for expected in expected["states"].as_array().unwrap() {
                    let owner = expected["owner"].as_u64().map(|owner| owner as usize);
                    let initial = if mode == 1 {
                        State::Sequence(Sequence::default())
                    } else {
                        State::Random(Random::new(
                            &playlist,
                            playlist.iter().any(|item| item.weight != 50),
                        )?)
                    };
                    let state = runtime.state(777, global, owner).unwrap_or(&initial);
                    let actual = match state {
                        State::Sequence(state) => serde_json::to_value(state)?,
                        State::Random(state) => serde_json::to_value(state)?,
                    };
                    assert_eq!(
                        expected["state"], actual,
                        "{context} history owner={owner:?}"
                    );
                }
                if playlist.len() <= 1 {
                    assert!(
                        runtime.entries.is_empty(),
                        "Fast path created unused history"
                    );
                }
            }
            let before = runtime.state(777, global, Some(7)).cloned();
            let removed = runtime.retire_object(777, Some(7));
            if playlist.len() > 1 {
                assert_eq!(removed, !global);
                if global {
                    assert_eq!(runtime.state(777, true, Some(7)), before.as_ref());
                } else {
                    assert!(runtime.state(777, false, Some(7)).is_none());
                    for owner in case["retirement"]["remaining_owners"].as_array().unwrap() {
                        assert!(runtime
                            .state(777, false, Some(owner.as_u64().unwrap() as usize))
                            .is_some());
                    }
                    assert_eq!(
                        runtime.entries.len(),
                        case["retirement"]["registry_count"].as_u64().unwrap() as usize
                    );
                }
            } else {
                assert!(!removed);
            }
            assert_eq!(case["retirement"]["global_history_preserved"], true);
        }
        Ok(())
    }
    fn loops(value: &serde_json::Value) -> Option<Loop> {
        (!value.is_null()).then(|| Loop {
            remaining: value["remaining"].as_i64().unwrap() as i16,
            flags: value["flags"].as_u64().unwrap() as u8,
        })
    }
    #[test]
    fn original_sequence_indices_direction_and_loop_end_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_selection.json"
        ))?;
        for case in fixture["sequences"].as_array().unwrap() {
            let mut state = Sequence::default();
            let mut looping = loops(&case["loop"]);
            for expected in case["steps"].as_array().unwrap() {
                let selected = state.next(
                    case["count"].as_u64().unwrap() as u16,
                    case["reverse"].as_bool().unwrap(),
                    &mut looping,
                );
                assert_eq!(expected["selected"], selected.unwrap_or(0), "{case}");
                assert_eq!(expected["valid"], selected.is_some());
                assert_eq!(expected["forward"], state.forward);
                assert_eq!(expected["index"], state.index);
                assert_eq!(expected["loop"], serde_json::to_value(&looping)?);
            }
        }
        Ok(())
    }
    #[test]
    fn original_random_choices_rng_bits_counters_and_repeat_history_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_selection.json"
        ))?;
        for case in fixture["randoms"].as_array().unwrap() {
            let playlist = case["weights"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, weight)| PlaylistItem {
                    sound: 100 + i as u32,
                    weight: weight.as_u64().unwrap() as u8,
                })
                .collect::<Vec<_>>();
            let weighted = case["weighted"].as_bool().unwrap();
            let mode = case["mode"].as_u64().unwrap() as u8;
            let avoid = case["avoid"].as_u64().unwrap() as u16;
            let mut state = Random::new(&playlist, weighted)?;
            let mut looping = loops(&case["loop"]);
            let mut rng = super::super::super::game_random::State {
                seed: case["seed"].as_u64().unwrap() as u32,
                draws: 0,
            };
            for (step, expected) in case["steps"].as_array().unwrap().iter().enumerate() {
                let selected =
                    state.next(&playlist, mode, avoid, weighted, &mut looping, || {
                        rng.draw()
                    })?;
                let actual = serde_json::to_value(&state)?;
                assert_eq!(
                    expected["selected"],
                    selected.unwrap_or(0),
                    "weights={:?} mode={mode} avoid={avoid} step={step}",
                    case["weights"]
                );
                assert_eq!(expected["valid"], selected.is_some());
                for field in [
                    "eligible_weight",
                    "eligible_count",
                    "remaining",
                    "played",
                    "blocked",
                    "history",
                ] {
                    assert_eq!(
                        expected[field], actual[field],
                        "{field} weights={:?} mode={mode} avoid={avoid} step={step}",
                        case["weights"]
                    );
                }
                assert_eq!(expected["seed"], rng.seed);
                assert_eq!(expected["draws"], rng.draws);
                assert_eq!(expected["loop"], serde_json::to_value(&looping)?);
            }
            if !case["undefined_division"].is_null() {
                let error = state
                    .next(&playlist, mode, avoid, weighted, &mut looping, || {
                        rng.draw()
                    })
                    .err()
                    .context("Native zero-divisor case was silently substituted")?;
                assert!(error.to_string().contains("zero eligible"));
                assert_eq!(case["undefined_division"]["seed"], rng.seed);
                assert_eq!(case["undefined_division"]["draws"], rng.draws);
            }
        }
        Ok(())
    }
}
