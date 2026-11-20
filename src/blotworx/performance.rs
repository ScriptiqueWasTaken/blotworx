//! Bounded timing windows. Instrumentation never advances game state.
use std::{collections::VecDeque, time::Duration};

const WINDOW: usize = 240;
#[derive(Default)]
pub struct Timing {
    pub samples: u64,
    pub total_ms: f64,
    pub max_ms: f64,
    recent: VecDeque<f64>,
}
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct Recent {
    pub samples: usize,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}
impl serde::Serialize for Timing {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut output = serializer.serialize_struct("Timing", 5)?;
        output.serialize_field("samples", &self.samples)?;
        output.serialize_field("total_ms", &self.total_ms)?;
        output.serialize_field("max_ms", &self.max_ms)?;
        output.serialize_field("recent", &self.recent())?;
        output.serialize_field("window_limit", &WINDOW)?;
        output.end()
    }
}
impl Timing {
    pub fn record(&mut self, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.;
        self.samples += 1;
        self.total_ms += ms;
        self.max_ms = self.max_ms.max(ms);
        if self.recent.len() == WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(ms);
    }
    pub fn recent(&self) -> Option<Recent> {
        if self.recent.is_empty() {
            return None;
        }
        let mut values: Vec<_> = self.recent.iter().copied().collect();
        values.sort_by(f64::total_cmp);
        let samples = values.len();
        Some(Recent {
            samples,
            mean_ms: values.iter().sum::<f64>() / samples as f64,
            p95_ms: values[((samples - 1) as f64 * 0.95).ceil() as usize],
            max_ms: values[samples - 1],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_slowdown_is_not_hidden_by_startup_history() {
        let mut timing = Timing::default();
        for _ in 0..10000 {
            timing.record(Duration::from_millis(1));
        }
        for _ in 0..WINDOW {
            timing.record(Duration::from_millis(40));
        }
        assert_eq!(
            timing.recent(),
            Some(Recent {
                samples: WINDOW,
                mean_ms: 40.,
                p95_ms: 40.,
                max_ms: 40.
            })
        );
        assert_eq!(timing.samples, 10240);
        assert_eq!(timing.total_ms, 19600.);
        assert_eq!(timing.recent.len(), WINDOW);
    }
    #[test]
    fn old_stall_expires_without_losing_lifetime_maximum() {
        let mut timing = Timing::default();
        timing.record(Duration::from_secs(10));
        for _ in 0..WINDOW {
            timing.record(Duration::from_millis(2));
        }
        assert_eq!(timing.recent().unwrap().max_ms, 2.);
        assert_eq!(timing.max_ms, 10000.);
    }
    #[test]
    fn empty_and_single_sample_have_no_fabricated_frames() {
        let mut timing = Timing::default();
        assert_eq!(timing.recent(), None);
        timing.record(Duration::ZERO);
        assert_eq!(
            timing.recent(),
            Some(Recent {
                samples: 1,
                mean_ms: 0.,
                p95_ms: 0.,
                max_ms: 0.
            })
        );
    }
}

pub fn append_line(lines: &mut Vec<String>, name: &str, timing: &Timing) {
    if let Some(recent) = timing.recent() {
        lines.push(format!(
            "{name}: {:.2} / {:.2} ms",
            recent.mean_ms, recent.p95_ms
        ));
    }
}
