//! Candidate selection portion of retail REL19916c (1992e4..199844).
//! The enclosing clock/random timestamp, reentrant selection scheduling and
//! concrete behavior/controller callbacks remain separate required work.
use anyhow::{ensure, Result};

/// Enclosing selection admission in REL19916c/199844. The mandatory phase
/// executes owner-byte propagation and concrete selection. This gate is
/// separate from the global AI manager's budgeted-work enable byte.
#[derive(Default)]
pub struct Lifecycle {
    pub blocked: bool,
    pub interval: f32,
    running: bool,
    pending: bool,
}
impl Lifecycle {
    /// Uses the SAME C rand stream as winner selection. Entry consumes a draw
    /// even without candidates. Blocked/reentrant entry consumes none.
    /// Constants: REL4e5c40=.3, REL4bcf88=.1, REL4bd024=32767.
    pub fn execute(
        &mut self,
        random: impl FnMut()->Result<i32>,
        phase: impl FnMut(&mut Self)->Result<()>,
    )->Result<()> {
        execute_live(&mut BorrowedLifecycle{state:self,random,phase})
    }
}

pub trait LifecycleHost {
    fn lifecycle(&self)->Result<&Lifecycle>;
    fn lifecycle_mut(&mut self)->Result<&mut Lifecycle>;
    fn lifecycle_random(&mut self)->Result<i32>;
    fn selection_phase(&mut self)->Result<()>;
    /// Native selector+10 is shared with the common current-behavior update.
    /// Standalone callers retain their own state; World hosts bind this to
    /// Selector.updating so synchronous selection cannot enter during a tick.
    fn selection_running(&self)->Result<bool>{Ok(self.lifecycle()?.running)}
    fn set_selection_running(&mut self,value:bool)->Result<()>{self.lifecycle_mut()?.running=value;Ok(())}
}
struct BorrowedLifecycle<'a,R,F>{state:&'a mut Lifecycle,random:R,phase:F}
impl<R:FnMut()->Result<i32>,F:FnMut(&mut Lifecycle)->Result<()>> LifecycleHost for BorrowedLifecycle<'_,R,F>{
    fn lifecycle(&self)->Result<&Lifecycle>{Ok(self.state)}
    fn lifecycle_mut(&mut self)->Result<&mut Lifecycle>{Ok(self.state)}
    fn lifecycle_random(&mut self)->Result<i32>{(self.random)()}
    fn selection_phase(&mut self)->Result<()>{(self.phase)(self.state)}
}
pub fn execute_live(h:&mut impl LifecycleHost)->Result<()> {
        if h.lifecycle()?.blocked {
            return Ok(());
        }
        if h.selection_running()? {
            h.lifecycle_mut()?.pending = true;
            return Ok(());
        }
        loop {
            if h.lifecycle()?.blocked {
                return Ok(());
            }
            h.set_selection_running(true)?;
            let result = (|| {
                let draw = h.lifecycle_random()?;
                ensure!(
                    (0..=32767).contains(&draw),
                    "AI selection requires the original C rand domain"
                );
                h.lifecycle_mut()?.interval = 0.1_f32.mul_add(draw as f32 / 32767., 0.3);
                h.selection_phase()
            })();
            h.set_selection_running(false)?;
            result?;
            if !h.lifecycle()?.pending {
                return Ok(());
            }
            h.lifecycle_mut()?.pending = false;
        }
    }

/// Virtual slots retain their offsets until their complete contracts are known.
pub trait Host {
    fn score_20(&mut self, behavior: usize) -> Result<i32>;
    fn current_gate_0c(&mut self, behavior: usize) -> Result<bool>;
    fn candidate_gate_24(&mut self, behavior: usize) -> Result<bool>;
    fn candidate_gate_28(&mut self, behavior: usize) -> Result<bool>;
    fn current_gate_2c(&mut self, behavior: usize) -> Result<bool>;
    /// Clear/destroy the nested receiver, invoke slot50, and emit the original
    /// transition notification in that order. Host must implement every step.
    fn leave_and_notify(&mut self, behavior: usize) -> Result<()>;
    /// Reset behavior +10/+c and invoke slot4c with the same-pointer flag.
    fn enter_4c(&mut self, behavior: usize, same_as_previous: bool) -> Result<()>;
    /// The original rand() draw, including draws when there is just one winner.
    fn random(&mut self) -> Result<i32>;
}

#[derive(Default)]
pub struct Selector {
    /// Selection permanently sorts this list. Equal scores retain the order
    /// from the previous pass, not necessarily the initial serialization order.
    pub ordered: Vec<usize>,
    pub current: Option<usize>,
    /// REL selector+10, true only inside the common current-behavior update.
    pub updating: bool,
    scores: Vec<i32>,
    candidates: Vec<usize>,
}
impl Selector {
    pub fn new(ordered: Vec<usize>) -> Self {
        Self {
            ordered,
            ..Self::default()
        }
    }
    /// Selection phase only. Caller runs native reentrancy/clock handling and
    /// must not invoke this with guessed callbacks for unsupported behaviors.
    pub fn choose(&mut self, host: &mut impl Host) -> Result<Option<usize>> {
        choose_live(&mut BorrowedSelection { state:self, host })
    }
}

/// Canonical receiver access releases each field borrow before invoking a
/// callback. World events therefore observe the real selector during entry,
/// exit and nested requests; no temporarily removed or copied selector exists.
pub trait LiveHost:Host {
    fn selector(&self)->Result<&Selector>;
    fn selector_mut(&mut self)->Result<&mut Selector>;
}
struct BorrowedSelection<'a,H>{state:&'a mut Selector,host:&'a mut H}
impl<H:Host> LiveHost for BorrowedSelection<'_,H>{
    fn selector(&self)->Result<&Selector>{Ok(self.state)}
    fn selector_mut(&mut self)->Result<&mut Selector>{Ok(self.state)}
}
impl<H:Host> Host for BorrowedSelection<'_,H>{
    fn score_20(&mut self,b:usize)->Result<i32>{self.host.score_20(b)}
    fn current_gate_0c(&mut self,b:usize)->Result<bool>{self.host.current_gate_0c(b)}
    fn candidate_gate_24(&mut self,b:usize)->Result<bool>{self.host.candidate_gate_24(b)}
    fn candidate_gate_28(&mut self,b:usize)->Result<bool>{self.host.candidate_gate_28(b)}
    fn current_gate_2c(&mut self,b:usize)->Result<bool>{self.host.current_gate_2c(b)}
    fn leave_and_notify(&mut self,b:usize)->Result<()>{self.host.leave_and_notify(b)}
    fn enter_4c(&mut self,b:usize,same:bool)->Result<()>{self.host.enter_4c(b,same)}
    fn random(&mut self)->Result<i32>{self.host.random()}
}
pub fn choose_live(h:&mut impl LiveHost)->Result<Option<usize>> {
        h.selector_mut()?.scores.clear();
        h.selector_mut()?.candidates.clear();
        let previous = h.selector()?.current;
        let mut current_score = -1;
        let count = h.selector()?.ordered.len();
        for at in 0..count {
            let behavior = h.selector()?.ordered[at];
            let score = h.score_20(behavior)?;
            if h.selector_mut()?.current == Some(behavior) {
                current_score = score;
            }
            h.selector_mut()?.scores.push(score);
        }
        // REL199354..1993d0: adjacent swaps only on strictly greater signed
        // scores. Preserve this stable, permanent permutation and score pairing.
        for start in 0..h.selector()?.ordered.len() {
            let mut unchanged = true;
            for left in (start..h.selector()?.ordered.len().saturating_sub(1)).rev() {
                if h.selector()?.scores[left + 1] > h.selector()?.scores[left] {
                    h.selector_mut()?.scores.swap(left, left + 1);
                    h.selector_mut()?.ordered.swap(left, left + 1);
                    unchanged = false;
                }
            }
            if unchanged {
                break;
            }
        }
        let mut best = -1;
        for at in 0..h.selector()?.ordered.len() {
            let behavior = h.selector()?.ordered[at];
            let score = h.selector()?.scores[at];
            if score < current_score || score < best {
                break;
            }
            let admitted = if score == current_score {
                if h.selector_mut()?.current == Some(behavior) {
                    if h.current_gate_0c(behavior)? {
                        h.selector_mut()?.current = None;
                        h.leave_and_notify(behavior)?;
                        current_score = -1;
                        true
                    } else {
                        false
                    }
                } else {
                    let can_leave = match h.selector()?.current {
                        Some(current) => h.current_gate_2c(current)?,
                        None => true,
                    };
                    can_leave
                        && h.candidate_gate_28(behavior)?
                        && h.candidate_gate_24(behavior)?
                }
            } else {
                h.candidate_gate_24(behavior)?
            };
            if admitted {
                best = score;
                h.selector_mut()?.candidates.push(behavior);
            }
        }
        if best >= 0 {
            ensure!(
                !h.selector()?.candidates.is_empty(),
                "AI candidate score has no receiver"
            );
            let random = h.random()?;
            ensure!(
                random >= 0,
                "AI native random provider returned a negative value"
            );
            let selected = h.selector()?.candidates[random as usize % h.selector()?.candidates.len()];
            if let Some(old) = h.selector_mut()?.current.take() {
                h.leave_and_notify(old)?;
            }
            h.selector_mut()?.current = Some(selected);
            h.enter_4c(selected, previous == Some(selected))?;
        }
        Ok(h.selector()?.current)
    }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn canonical_entry_observes_selection_and_coalesces_synchronous_requests()->Result<()> {
        struct WorldReceiver {selector:Selector,lifecycle:Lifecycle,draws:usize,phases:usize,entries:usize}
        impl LiveHost for WorldReceiver {
            fn selector(&self)->Result<&Selector>{Ok(&self.selector)}
            fn selector_mut(&mut self)->Result<&mut Selector>{Ok(&mut self.selector)}
        }
        impl LifecycleHost for WorldReceiver {
            fn lifecycle(&self)->Result<&Lifecycle>{Ok(&self.lifecycle)}
            fn lifecycle_mut(&mut self)->Result<&mut Lifecycle>{Ok(&mut self.lifecycle)}
            fn lifecycle_random(&mut self)->Result<i32>{Host::random(self)}
            fn selection_phase(&mut self)->Result<()>{self.phases+=1;choose_live(self)?;Ok(())}
            fn selection_running(&self)->Result<bool>{Ok(self.selector.updating)}
            fn set_selection_running(&mut self,value:bool)->Result<()>{self.selector.updating=value;Ok(())}
        }
        impl Host for WorldReceiver {
            fn score_20(&mut self,_:usize)->Result<i32>{Ok(13)}
            fn current_gate_0c(&mut self,_:usize)->Result<bool>{Ok(false)}
            fn candidate_gate_24(&mut self,_:usize)->Result<bool>{Ok(true)}
            fn candidate_gate_28(&mut self,_:usize)->Result<bool>{Ok(false)}
            fn current_gate_2c(&mut self,_:usize)->Result<bool>{Ok(false)}
            fn leave_and_notify(&mut self,_:usize)->Result<()>{anyhow::bail!("Equal-priority callback should retain the actual current receiver")}
            fn enter_4c(&mut self,b:usize,same:bool)->Result<()>{
                assert_eq!(self.selector.current,Some(b));assert!(!same);assert!(self.selector.updating);
                assert_eq!(self.selector.ordered,[7]);self.entries+=1;
                execute_live(self)?;execute_live(self)?;
                assert_eq!(self.phases,1,"Synchronous requests must wait for the current callback");
                Ok(())
            }
            fn random(&mut self)->Result<i32>{self.draws+=1;Ok(0)}
        }
        let mut world=WorldReceiver{selector:Selector::new(vec![7]),lifecycle:Lifecycle::default(),draws:0,phases:0,entries:0};
        execute_live(&mut world)?;
        assert_eq!((world.draws,world.phases,world.entries),(3,2,1));
        assert_eq!(world.selector.current,Some(7));assert!(!world.lifecycle.running && !world.lifecycle.pending);
        assert!(!world.selector.updating);
        // The very same bit is held by198e18 while common task code runs.
        // A synchronous event may request selection there, but cannot enter.
        world.selector.updating=true;
        let before=(world.draws,world.phases,world.entries);
        execute_live(&mut world)?;
        assert_eq!((world.draws,world.phases,world.entries),before);
        assert!(world.lifecycle.pending);assert!(world.selector.updating);
        world.selector.updating=false;
        execute_live(&mut world)?;
        assert_eq!((world.draws,world.phases,world.entries),(5,4,1));
        assert!(!world.lifecycle.pending && !world.selector.updating);
        Ok(())
    }
    #[test]
    fn native_entry_draws_even_without_winners_and_coalesces_reentrant_requests() -> Result<()> {
        let mut lifecycle = Lifecycle::default();
        let mut draws = 0;
        let mut phases = 0;
        lifecycle.execute(
            || {
                draws += 1;
                Ok(if draws == 1 { 0 } else { 32767 })
            },
            |state| {
                phases += 1;
                if phases == 1 {
                    assert_eq!(state.interval.to_bits(), 0.3_f32.to_bits());
                    for _ in 0..2 {
                        state.execute(
                            || anyhow::bail!("Reentry consumed randomness"),
                            |_| anyhow::bail!("Reentry ran before the old phase finished"),
                        )?;
                    }
                }
                Ok(())
            },
        )?;
        assert_eq!((draws, phases), (2, 2));
        assert_eq!(lifecycle.interval.to_bits(), 0.4_f32.to_bits());
        Ok(())
    }
    #[test]
    fn blocked_selection_does_not_touch_random_or_phase_and_failure_releases_entry() -> Result<()> {
        let mut lifecycle = Lifecycle {
            blocked: true,
            ..Default::default()
        };
        lifecycle.execute(
            || anyhow::bail!("Blocked entry drew randomness"),
            |_| anyhow::bail!("Blocked entry ran a phase"),
        )?;
        lifecycle.blocked = false;
        assert!(lifecycle
            .execute(
                || Ok(0),
                |_| anyhow::bail!("Missing real behavior callback")
            )
            .is_err());
        let mut phases = 0;
        lifecycle.execute(
            || Ok(0),
            |_| {
                phases += 1;
                Ok(())
            },
        )?;
        assert_eq!(phases, 1);
        Ok(())
    }
    struct Fixture {
        scores: BTreeMap<usize, i32>,
        calls: Vec<String>,
        eligible: Vec<usize>,
        leave_equal: bool,
        restart: bool,
        random: i32,
    }
    impl Host for Fixture {
        fn score_20(&mut self, id: usize) -> Result<i32> {
            self.calls.push(format!("score{id}"));
            Ok(self.scores[&id])
        }
        fn current_gate_0c(&mut self, id: usize) -> Result<bool> {
            self.calls.push(format!("restart{id}"));
            Ok(self.restart)
        }
        fn candidate_gate_24(&mut self, id: usize) -> Result<bool> {
            self.calls.push(format!("eligible{id}"));
            Ok(self.eligible.contains(&id))
        }
        fn candidate_gate_28(&mut self, id: usize) -> Result<bool> {
            self.calls.push(format!("equal{id}"));
            Ok(true)
        }
        fn current_gate_2c(&mut self, id: usize) -> Result<bool> {
            self.calls.push(format!("yield{id}"));
            Ok(self.leave_equal)
        }
        fn leave_and_notify(&mut self, id: usize) -> Result<()> {
            self.calls.push(format!("leave{id}"));
            Ok(())
        }
        fn enter_4c(&mut self, id: usize, same: bool) -> Result<()> {
            self.calls.push(format!("enter{id}:{same}"));
            Ok(())
        }
        fn random(&mut self) -> Result<i32> {
            self.calls.push("random".into());
            Ok(self.random)
        }
    }
    fn fixture(scores: &[(usize, i32)]) -> Fixture {
        Fixture {
            scores: scores.iter().copied().collect(),
            calls: vec![],
            eligible: scores.iter().map(|p| p.0).collect(),
            leave_equal: false,
            restart: false,
            random: 0,
        }
    }
    #[test]
    fn sorting_is_signed_stable_persistent_and_randomizes_ties() -> Result<()> {
        let mut selector = Selector::new(vec![1, 2, 3, 4]);
        let mut host = fixture(&[(1, 10), (2, 40), (3, 40), (4, -2)]);
        host.random = 1;
        assert_eq!(selector.choose(&mut host)?, Some(3));
        assert_eq!(selector.ordered, [2, 3, 1, 4]);
        assert_eq!(
            host.calls,
            [
                "score1",
                "score2",
                "score3",
                "score4",
                "eligible2",
                "eligible3",
                "random",
                "enter3:false"
            ]
        );
        // Equal scores retain the already sorted order on a later pass.
        selector.current = None;
        host.calls.clear();
        host.scores.values_mut().for_each(|score| *score = 10);
        host.random = 2;
        assert_eq!(selector.choose(&mut host)?, Some(1));
        assert_eq!(selector.ordered, [2, 3, 1, 4]);
        Ok(())
    }
    #[test]
    fn equal_current_gate_blocks_replacement_and_lower_scores_without_random() -> Result<()> {
        let mut selector = Selector::new(vec![1, 2, 3]);
        selector.current = Some(1);
        let mut host = fixture(&[(1, 40), (2, 40), (3, 20)]);
        assert_eq!(selector.choose(&mut host)?, Some(1));
        assert_eq!(
            host.calls,
            ["score1", "score2", "score3", "restart1", "yield1"]
        );
        Ok(())
    }
    #[test]
    fn equal_replacement_calls_yield_then_equal_then_eligibility() -> Result<()> {
        let mut selector = Selector::new(vec![1, 2]);
        selector.current = Some(1);
        let mut host = fixture(&[(1, 40), (2, 40)]);
        host.leave_equal = true;
        assert_eq!(selector.choose(&mut host)?, Some(2));
        assert_eq!(
            host.calls,
            [
                "score1",
                "score2",
                "restart1",
                "yield1",
                "equal2",
                "eligible2",
                "random",
                "leave1",
                "enter2:false"
            ]
        );
        Ok(())
    }
    #[test]
    fn forced_current_restart_leaves_before_random_and_enters_with_same_flag() -> Result<()> {
        let mut selector = Selector::new(vec![1]);
        selector.current = Some(1);
        let mut host = fixture(&[(1, 40)]);
        host.restart = true;
        assert_eq!(selector.choose(&mut host)?, Some(1));
        assert_eq!(
            host.calls,
            ["score1", "restart1", "leave1", "random", "enter1:true"]
        );
        Ok(())
    }
    #[test]
    fn negative_candidates_do_not_select_or_consume_a_random_draw() -> Result<()> {
        let mut selector = Selector::new(vec![1, 2]);
        let mut host = fixture(&[(1, -1), (2, -2)]);
        assert_eq!(selector.choose(&mut host)?, None);
        assert_eq!(host.calls, ["score1", "score2", "equal1", "eligible1"]);
        Ok(())
    }
}
