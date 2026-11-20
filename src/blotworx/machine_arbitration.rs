//! Active-machine dispatch arbitration, DOL 0x80218f1c..0x802192e4.
//! Pending readiness must be serviced before this dispatch phase.
use super::{state_machine::Transition, transition_select::IntervalState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Local table owner; canonical zero for the machine-wide wildcard table.
    pub state: i32,
    pub wildcard: bool,
    pub index: usize,
}
#[derive(Clone)]
pub struct Request {
    pub identity: Identity,
    pub transition: Transition,
    pub interval: IntervalState,
    pub reset_previous: bool,
}
pub enum Decision {
    Blocked,
    Start(Request),
    Queued,
    Empty,
}
#[derive(Default)]
pub struct Dispatcher {
    pub pending: Vec<Request>,
}
impl Dispatcher {
    /// 0x80219394: use live interval bytes, never the selection-time snapshot.
    /// A forced check is supplied by the machine lifecycle, not inferred here.
    pub fn take_ready(
        &mut self,
        force: bool,
        mut interval: impl FnMut(Identity) -> anyhow::Result<IntervalState>,
    ) -> anyhow::Result<Option<Request>> {
        for i in 0..self.pending.len() {
            if force || interval(self.pending[i].identity)?.0 & 2 != 0 {
                let selected = self.pending.remove(i);
                self.pending.clear();
                return Ok(Some(selected));
            }
        }
        Ok(None)
    }
    pub fn allows_selection(&self, running: Option<&Transition>) -> bool {
        running.is_none_or(|t| t.flags & 4 == 0)
            && self.pending.iter().all(|p| p.transition.flags & 8 == 0)
    }
    pub fn submit(
        &mut self,
        mut candidates: Vec<Request>,
        running: Option<&Transition>,
    ) -> Decision {
        let Some(first) = candidates.first() else {
            return Decision::Empty;
        };
        if let Some(previous) = self.pending.first() {
            if previous.transition.priority > first.transition.priority
                || self.pending.iter().any(|p| p.transition.flags & 8 != 0)
            {
                return Decision::Blocked;
            }
            // Retail removes duplicate transition pointers by swapping the
            // final candidate into the removed slot, then checking it again.
            let mut i = 0;
            while i < candidates.len() {
                if self
                    .pending
                    .iter()
                    .any(|p| p.identity == candidates[i].identity)
                {
                    candidates.swap_remove(i);
                } else {
                    i += 1;
                }
            }
        } else if let Some(running) = running {
            if running.flags & 4 != 0 || running.priority > first.transition.priority {
                return Decision::Blocked;
            }
        }
        self.pending.clear();
        if let Some(index) = candidates.iter().position(|c| {
            (c.transition.flags & 2 == 0 || c.interval.0 & 2 != 0)
                && c.transition.flags & 16384 == 0
        }) {
            return Decision::Start(candidates.remove(index));
        }
        self.pending = candidates;
        Decision::Queued
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(index: usize, priority: i16, flags: u16) -> Request {
        let interval = super::super::state_machine::Interval {
            enter_event_id: -1,
            exit_event_id: -1,
            enter_time: 0.,
            exit_time: 0.,
        };
        Request {
            identity: Identity {
                state: 0,
                wildcard: false,
                index,
            },
            reset_previous: false,
            interval: IntervalState(0),
            transition: Transition {
                trigger_interval: interval.clone(),
                initiate_interval: interval,
                transition: None,
                condition: None,
                event_id: 0,
                to_state_id: 1,
                from_nested_state_id: -1,
                to_nested_state_id: -1,
                priority,
                flags,
            },
        }
    }
    #[test]
    fn pending_priority_and_uninterruptible_flags_precede_running_effect_gate() {
        let mut d = Dispatcher {
            pending: vec![request(0, 3, 2)],
        };
        assert!(matches!(
            d.submit(vec![request(1, 2, 0)], None),
            Decision::Blocked
        ));
        assert_eq!(d.pending.len(), 1);
        d.pending[0].transition.flags |= 8;
        assert!(matches!(
            d.submit(vec![request(1, 4, 0)], None),
            Decision::Blocked
        ));
        d.pending[0].transition.flags = 2;
        assert!(matches!(
            d.submit(vec![request(1, 3, 0)], Some(&request(2, 9, 4).transition)),
            Decision::Start(_)
        ));
        assert!(d.pending.is_empty());
        assert!(matches!(
            d.submit(vec![request(1, 10, 0)], Some(&request(2, 9, 4).transition)),
            Decision::Blocked
        ));
    }
    #[test]
    fn duplicate_removal_preserves_retail_swap_order_and_abut_waits() {
        let mut d = Dispatcher {
            pending: vec![request(0, 0, 2)],
        };
        let result = d.submit(
            vec![request(0, 0, 0), request(1, 0, 0), request(2, 0, 0)],
            None,
        );
        match result {
            Decision::Start(r) => assert_eq!(r.identity.index, 2),
            _ => panic!("expected ready replacement"),
        }
        assert!(matches!(
            d.submit(vec![request(0, 0, 16384), request(1, 0, 2)], None),
            Decision::Queued
        ));
        assert_eq!(d.pending.len(), 2);
        assert!(matches!(d.submit(vec![], None), Decision::Empty));
        assert_eq!(d.pending.len(), 2);
    }
    #[test]
    fn readiness_reads_live_windows_and_force_does_not_require_a_window() -> anyhow::Result<()> {
        let mut d = Dispatcher {
            pending: vec![request(0, 0, 2), request(1, 0, 2)],
        };
        assert!(d.take_ready(false, |_| Ok(IntervalState(0)))?.is_none());
        assert_eq!(d.pending.len(), 2);
        let ready = d
            .take_ready(false, |identity| {
                Ok(IntervalState(if identity.index == 1 { 2 } else { 0 }))
            })?
            .unwrap();
        assert_eq!(ready.identity.index, 1);
        assert!(d.pending.is_empty());
        d.pending.push(request(0, 0, 16384));
        assert!(d
            .take_ready(true, |_| anyhow::bail!(
                "forced path must not read interval"
            ))?
            .is_some());
        assert!(d.pending.is_empty());
        Ok(())
    }
}
