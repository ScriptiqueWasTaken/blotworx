//! Global HUD countdown, EM1 REL211428..21159c /2112e4.
//! Flash/menu acknowledgement is supplied by the presentation adapter.
#[derive(Clone, Debug)]
pub struct State {
    pub remaining: f32,
    pub mid: f32,
    pub late: f32,
    pub running: bool,
    pub shown: bool,
    pub visible: bool,
    pub sound: bool,
    pub phase: u8,
    pub display: String,
    seconds: i32,
}
const PLAY: [&str; 4] = [
    "Play_sfx_counter_ticking_normal",
    "Play_sfx_counter_ticking_mid",
    "Play_sfx_counter_ticking_late",
    "Play_sfx_counter_timeout",
];
const STOP: [&str; 3] = [
    "Stop_sfx_counter_ticking_normal",
    "Stop_sfx_counter_ticking_mid",
    "Stop_sfx_counter_ticking_late",
];
impl Default for State {
    fn default() -> Self {
        Self {
            remaining: 0.,
            mid: 10.,
            late: 5.,
            running: false,
            shown: false,
            visible: false,
            sound: true,
            phase: 0,
            display: "0:00".into(),
            seconds: 0,
        }
    }
}
impl State {
    pub fn ticking(&self) -> bool {
        self.remaining > 0.
    }
    pub fn start(&mut self, time: f32, mid: f32, late: f32, sound: bool) {
        self.remaining = time;
        self.mid = mid;
        self.late = late;
        self.sound = sound;
        self.seconds = 0;
    }
    pub fn shown(&mut self) {
        self.shown = true;
        self.visible = true;
        self.running = true;
        self.update_text();
    }
    fn update_text(&mut self) {
        let value = self.remaining.trunc() as i32;
        let seconds = value % 60;
        if seconds != self.seconds {
            self.seconds = seconds;
            self.display = if seconds < 10 {
                format!("{}:0{}", value / 60, seconds)
            } else {
                format!("{}:{}", value / 60, seconds)
            };
        }
    }
    fn stop_sound(&self, events: &mut Vec<&'static str>) {
        if (1..=3).contains(&self.phase) {
            events.push(STOP[self.phase as usize - 1]);
        }
    }
    pub fn pause(&mut self) -> Vec<&'static str> {
        let mut events = Vec::new();
        if self.shown {
            self.running = false;
            self.stop_sound(&mut events);
        }
        events
    }
    pub fn unpause(&mut self) -> Vec<&'static str> {
        let mut events = Vec::new();
        if self.shown {
            self.running = true;
            if self.sound && (1..=4).contains(&self.phase) {
                events.push(PLAY[self.phase as usize - 1]);
            }
        }
        events
    }
    pub fn stop(&mut self) {
        self.remaining = 0.;
        self.running = false;
        self.visible = false;
    }
    /// Functional HUD acknowledgement of the close animation: retire its
    /// looping cue as well. Original Flash close/audio phase parity is open.
    pub fn closed(&mut self) -> Vec<&'static str> {
        let mut events = Vec::new();
        self.stop_sound(&mut events);
        self.shown = false;
        self.running = false;
        self.visible = false;
        events
    }
    pub fn advance(&mut self, delta: f32) -> Vec<&'static str> {
        let mut events = Vec::new();
        if !self.running || delta <= 0. {
            return events;
        }
        self.remaining -= delta;
        let next = if self.remaining > self.mid {
            1
        } else if self.remaining > self.late {
            2
        } else if self.remaining > 0. {
            3
        } else {
            4
        };
        if next != self.phase {
            self.stop_sound(&mut events);
            // Native2110f4 writes the phase ONLY when SFX is enabled.
            if self.sound {
                self.phase = next;
                events.push(PLAY[next as usize - 1]);
            }
            if next == 4 {
                self.stop();
            }
        }
        self.update_text();
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pause_deadline_and_native_state_predicate_are_separate() {
        let mut s = State::default();
        s.start(2., 10., 5., true);
        s.shown();
        s.advance(0.25);
        s.pause();
        s.advance(8.);
        assert_eq!(s.remaining, 1.75);
        assert!(s.ticking());
        s.unpause();
        s.advance(2.);
        assert_eq!(s.remaining, 0.);
        assert!(!s.ticking());
        assert!(!s.running);
        s.remaining = 5.;
        assert!(s.ticking());
        assert!(!s.running);
    }
    #[test]
    fn cue_transitions_use_strict_thresholds_and_native_order() {
        let mut s = State::default();
        s.start(11., 10., 5., true);
        s.shown();
        assert_eq!(s.advance(0.5), vec![PLAY[0]]);
        assert_eq!(s.advance(0.5), vec![STOP[0], PLAY[1]]);
        assert_eq!(s.advance(5.), vec![STOP[1], PLAY[2]]);
        assert_eq!(s.advance(5.), vec![STOP[2], PLAY[3]]);
    }
    #[test]
    fn phase_is_not_written_when_sound_is_disabled() {
        let mut s = State::default();
        s.phase = 1;
        s.start(6., 10., 5., false);
        s.shown();
        assert_eq!(s.advance(0.1), vec![STOP[0]]);
        assert_eq!(s.phase, 1);
        assert_eq!(s.advance(0.1), vec![STOP[0]]);
    }
    #[test]
    fn text_truncates_and_updates_only_when_seconds_change() {
        let mut s = State::default();
        s.start(10.9, 10., 5., true);
        s.shown();
        assert_eq!(s.display, "0:10");
        s.remaining = 60.;
        s.seconds = 0;
        s.update_text();
        assert_eq!(s.display, "0:10");
        s.advance(0.1);
        assert_eq!(s.display, "0:59");
    }
}
