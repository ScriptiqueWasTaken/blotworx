//! Profile-selection state writes, EM1 REL1e0f00..1e1004.
//! Interruption snapshots (1e3280), profile evaluation and camera ownership
//! must be provided by the native camera backend; indices are not entities.
use anyhow::Result;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub current: i32,    //+14
    pub previous: i32,   //+18
    pub weight: f32,     //+1c
    pub duration: f32,   //+20
    pub elapsed: f32,    //+24
    pub immediate: bool, //+28
}
impl Default for State {
    fn default() -> Self {
        Self {
            current: 0,
            previous: -1,
            weight: 0.,
            duration: 0.,
            elapsed: 0.,
            immediate: false,
        }
    }
}
pub trait Backend {
    fn find_profile(&mut self, name: &str) -> Result<i32>; //1e0eb8
    /// Capture the current/intermediate blended profile, preserving native
    /// update order: duration/elapsed writes are visible to this callback.
    fn snapshot_interrupted(&mut self, state: &State) -> Result<i32>; //1e3280
}
impl State {
    /// REL1e334c: the native clock eases its elapsed-time rate, rather than
    /// linearly advancing time and applying an output easing curve.
    pub fn advance(&mut self, delta: f32) {
        if !(self.duration > 0.) {
            return;
        }
        let ratio = self.elapsed / self.duration;
        let rate = (-f32::from_bits(0x3fb9999a)).mul_add(ratio, 1.5);
        self.elapsed = rate.mul_add(delta, self.elapsed);
        let ratio = self.elapsed / self.duration;
        self.weight = if !(1. < ratio) { ratio } else { 1. };
        if self.weight >= 1. {
            self.previous = -1;
            self.duration = -1.;
        }
    }
    pub fn select(
        &mut self,
        name: &str,
        mode: i32,
        duration: f32,
        backend: &mut impl Backend,
    ) -> Result<()> {
        let target = backend.find_profile(name)?;
        if mode == 1 {
            self.duration = duration;
            self.elapsed = 0.;
            self.previous = if self.previous == -1 && target != self.current {
                self.current
            } else {
                backend.snapshot_interrupted(self)?
            };
        } else if mode == 2 {
            // Native does not reset elapsed in mode2.
            self.duration = -1.;
            self.previous = if self.previous == -1 && target != self.current {
                self.current
            } else {
                backend.snapshot_interrupted(self)?
            };
        } else {
            self.duration = -1.;
            self.previous = -1;
        }
        self.current = target;
        self.immediate = mode == 0;
        self.weight = if self.immediate { 1. } else { 0. };
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        target: i32,
        snapshots: Vec<State>,
        fail: bool,
    }
    impl Backend for Trace {
        fn find_profile(&mut self, name: &str) -> Result<i32> {
            assert_eq!(name, "CustomDefault");
            Ok(self.target)
        }
        fn snapshot_interrupted(&mut self, state: &State) -> Result<i32> {
            self.snapshots.push(*state);
            if self.fail {
                anyhow::bail!("Native snapshot backend unavailable")
            };
            Ok(9)
        }
    }
    fn backend(target: i32) -> Trace {
        Trace {
            target,
            snapshots: vec![],
            fail: false,
        }
    }
    #[test]
    fn first_timed_selection_uses_outgoing_profile_and_resets_elapsed() -> Result<()> {
        let mut state = State {
            elapsed: 4.,
            ..State::default()
        };
        let mut b = backend(2);
        state.select("CustomDefault", 1, 0.5, &mut b)?;
        assert_eq!(
            state,
            State {
                current: 2,
                previous: 0,
                duration: 0.5,
                elapsed: 0.,
                weight: 0.,
                immediate: false
            }
        );
        assert!(b.snapshots.is_empty());
        Ok(())
    }
    #[test]
    fn interrupted_or_same_profile_selection_snapshots_after_timer_writes() -> Result<()> {
        for (current, previous) in [(2, -1), (0, 3)] {
            let mut state = State {
                current,
                previous,
                elapsed: 4.,
                ..State::default()
            };
            let mut b = backend(2);
            state.select("CustomDefault", 1, 0.5, &mut b)?;
            assert_eq!(b.snapshots.len(), 1);
            assert_eq!(b.snapshots[0].duration, 0.5);
            assert_eq!(b.snapshots[0].elapsed, 0.);
            assert_eq!(b.snapshots[0].current, current);
            assert_eq!(b.snapshots[0].previous, previous);
            assert_eq!((state.current, state.previous), (2, 9));
        }
        Ok(())
    }
    #[test]
    fn second_mode_preserves_elapsed_and_other_modes_do_not_snapshot() -> Result<()> {
        let mut state = State {
            elapsed: 4.,
            ..State::default()
        };
        let mut b = backend(2);
        state.select("CustomDefault", 2, 99., &mut b)?;
        assert_eq!(
            (state.duration, state.elapsed, state.previous),
            (-1., 4., 0)
        );
        for mode in [0, -1, 3] {
            state.select("CustomDefault", mode, 99., &mut b)?;
            assert_eq!(
                (state.previous, state.duration, state.elapsed),
                (-1, -1., 4.)
            );
            assert_eq!(state.immediate, mode == 0);
            assert_eq!(state.weight, if mode == 0 { 1. } else { 0. });
        }
        assert!(b.snapshots.is_empty());
        Ok(())
    }
    #[test]
    fn snapshot_failure_preserves_native_partial_state() {
        let mut state = State {
            current: 2,
            previous: 3,
            elapsed: 4.,
            weight: 0.75,
            immediate: true,
            duration: 7.,
        };
        let mut b = backend(2);
        b.fail = true;
        assert!(state.select("CustomDefault", 1, 0.5, &mut b).is_err());
        assert_eq!(
            state,
            State {
                duration: 0.5,
                elapsed: 0.,
                current: 2,
                previous: 3,
                weight: 0.75,
                immediate: true
            }
        );
    }
    #[test]
    fn native_clock_advances_elapsed_at_a_changing_rate_and_finishes() {
        let mut s = State {
            duration: 1.,
            previous: 0,
            current: 1,
            ..State::default()
        };
        s.advance(0.25);
        assert_eq!(s.elapsed, 0.375);
        assert_eq!(s.weight, 0.375);
        s.advance(2.);
        assert_eq!(s.weight, 1.);
        assert_eq!(s.previous, -1);
        assert_eq!(s.duration, -1.);
        let finished = s;
        s.advance(10.);
        assert_eq!(s, finished);
        s.duration = 0.;
        s.elapsed = 2.;
        let inactive = s;
        s.advance(10.);
        assert_eq!(s, inactive);
    }
}
