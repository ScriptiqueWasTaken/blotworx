//! Shared frame clock14b37c/14b440, published before this demo's tick callbacks.
//! Native complete frame/component ordering and clock controls remain open.
use super::game_clock::{Clock, Frame, Rate, Step};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Monotonic,
    DeterministicCapture,
    SuppliedSamples,
}
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub clock: Clock,
    pub frame: Frame,
    origin: Instant,
    counter_base: u64,
    last_sample: u64,
    rebase: bool,
    published: bool,
    source: Source,
    rtc_origin: Option<u32>,
    rtc_source: Option<&'static str>,
}
impl Default for Context {
    fn default() -> Self {
        // Wii low memory+f8 is0x0e7be2c0 (243MHz bus), timebase=bus/4.
        // Corroborated by Dolphin Boot_BS2Emu.cpp's Wii memory initialization.
        Self::new(Rate::from_bus_clock(0x0e7be2c0).unwrap(), 0)
    }
}
impl Context {
    pub fn new(rate: Rate, initial_sample: u64) -> Self {
        Self {
            clock: Clock::new(rate, initial_sample),
            frame: Frame {
                unscaled: 0.,
                scaled: 0.,
                hardware: 0.,
            },
            origin: Instant::now(),
            counter_base: initial_sample,
            last_sample: initial_sample,
            rebase: true,
            published: false,
            source: Source::Monotonic,
            rtc_origin: None,
            rtc_source: None,
        }
    }
    /// Boot hardware TB from whole RTC seconds since2000, as corroborated by
    /// the reference platform's SystemTimers initialization. An explicit value
    /// makes captures reproducible without changing any gameplay clock delta.
    pub fn boot(rtc: Option<u32>) -> Self {
        let (seconds, source) = match rtc {
            Some(seconds) => (seconds, "explicit Wii RTC seconds since2000"),
            None => (
                host_rtc_seconds(),
                if cfg!(windows) {
                    "host local RTC"
                } else {
                    "host UTC RTC; local timezone integration pending"
                },
            ),
        };
        let rate = Rate::from_bus_clock(0x0e7be2c0).unwrap();
        let mut context = Self::new(
            rate,
            u64::from(seconds) * u64::from(rate.ticks_per_second.get()),
        );
        context.rtc_origin = Some(seconds);
        context.rtc_source = Some(source);
        context
    }
    fn monotonic_ticks(&self) -> u64 {
        let elapsed = self.origin.elapsed();
        let frequency = u64::from(self.clock.rate.ticks_per_second.get());
        // Integer conversion preserves the counter's precision and u64 wrap.
        self.counter_base
            .wrapping_add(elapsed.as_secs().wrapping_mul(frequency))
            .wrapping_add(u64::from(elapsed.subsec_nanos()) * frequency / 1_000_000_000)
    }
    /// Loading suspends the session. The first incoming frame refreshes only
    /// its hardware baseline; the suspended interval is not gameplay time.
    pub fn suspend_for_loading(&mut self) {
        self.rebase = true;
    }
    fn apply_sample(&mut self, active: bool, sample: u64) {
        let step = if active && !self.rebase {
            Step::Realtime
        } else {
            Step::Frozen
        };
        self.frame = self.clock.sample(step, sample, 1.);
        self.last_sample = sample;
        self.rebase = false;
        self.published = true;
    }
    pub fn publish_sample(&mut self, active: bool, sample: u64) {
        self.source = Source::SuppliedSamples;
        self.apply_sample(active, sample);
    }
    pub fn publish(&mut self, active: bool, deterministic_capture: bool) {
        let sample = if deterministic_capture {
            self.source = Source::DeterministicCapture;
            if self.rebase {
                // The capture driver supplies a full simulated frame on its
                // first tick. Rebase at the retained sample before advancing.
                self.clock.sample(Step::Frozen, self.last_sample, 1.);
                self.rebase = false;
            }
            // An explicit replay driver, not an estimate from rendered FPS.
            self.last_sample
                .wrapping_add(u64::from(self.clock.rate.ticks_per_second.get() / 30))
        } else {
            self.source = Source::Monotonic;
            self.monotonic_ticks()
        };
        self.apply_sample(active, sample);
    }
    pub fn hardware_sample(&self) -> u64 {
        if self.source == Source::Monotonic {
            self.monotonic_ticks()
        } else {
            self.last_sample
        }
    }
    pub fn is_published(&self) -> bool {
        self.published
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"published":self.is_published(),"source":match self.source {
            Source::Monotonic=>"host monotonic timebase",Source::DeterministicCapture=>"explicit 30Hz capture samples",Source::SuppliedSamples=>"supplied frame samples"},
            "ticks_per_second":self.clock.rate.ticks_per_second.get(),"frames":self.clock.frames,
            "unscaled_seconds":self.clock.unscaled_seconds,"scaled_seconds":self.clock.scaled_seconds,
            "hardware_seconds":self.clock.hardware_seconds,"unscaled_ticks_hex":format!("0x{:016x}",self.clock.unscaled.ticks),
            "hardware_tick_sample_hex":format!("0x{:016x}",self.last_sample),"frame_unscaled":self.frame.unscaled,
            "frame_hardware":self.frame.hardware,"native_complete_frame_order_verified":false,
            "boot_rtc_origin_implemented":self.rtc_origin.is_some(),"rtc_seconds_since2000":self.rtc_origin,"rtc_source":self.rtc_source,
            "native_clock_controls_connected":false,"other_component_clocks_migrated":false})
    }
}
/// Civil Gregorian date to the Wii RTC's unsigned seconds since2000. Weekday
/// and milliseconds do not participate in the platform's whole-second seed.
#[cfg(any(windows, test))]
fn civil_rtc(year: i32, month: u16, day: u16, hour: u16, minute: u16, second: u16) -> u32 {
    fn days(year: i32, month: i32, day: i32) -> i64 {
        let year = year - i32::from(month <= 2);
        let era = year.div_euclid(400);
        let y = year - era * 400;
        let m = month + if month > 2 { -3 } else { 9 };
        i64::from(era) * 146097 + i64::from(y * 365 + y / 4 - y / 100 + (153 * m + 2) / 5 + day - 1)
    }
    ((days(year, i32::from(month), i32::from(day)) - days(2000, 1, 1)) * 86400
        + i64::from(hour) * 3600
        + i64::from(minute) * 60
        + i64::from(second)) as u32
}
#[cfg(windows)]
fn host_rtc_seconds() -> u32 {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millisecond: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime::default();
    // Windows writes the fixed16-byte SYSTEMTIME structure supplied here.
    unsafe {
        GetLocalTime(&mut time);
    }
    civil_rtc(
        i32::from(time.year),
        time.month,
        time.day,
        time.hour,
        time.minute,
        time.second,
    )
}
#[cfg(not(windows))]
fn host_rtc_seconds() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .wrapping_sub(946684800) as u32
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Context {
        Context::new(Rate::from_bus_clock(4000).unwrap(), 100)
    }
    #[test]
    fn pause_and_load_refresh_baselines_without_charging_suspended_time() {
        let mut clock = context();
        clock.publish_sample(true, 10000);
        assert_eq!(clock.clock.unscaled.ticks, 100);
        clock.publish_sample(true, 11000);
        assert_eq!(clock.clock.unscaled.ticks, 1100);
        clock.publish_sample(false, 91000);
        assert_eq!(clock.clock.unscaled.ticks, 1100);
        clock.publish_sample(true, 92000);
        assert_eq!(clock.clock.unscaled.ticks, 2100);
        clock.suspend_for_loading();
        let mut incoming = clock;
        incoming.publish_sample(true, 192000);
        assert_eq!(incoming.clock.unscaled.ticks, 2100);
        incoming.publish_sample(true, 193000);
        assert_eq!(incoming.clock.unscaled.ticks, 3100);
        assert_eq!(
            clock.clock.unscaled.ticks, 2100,
            "Replacing a world must not mutate retained rollback state"
        );
        clock.publish_sample(true, 192000);
        assert_eq!(
            clock.clock.unscaled.ticks, 2100,
            "Failed-load rollback must rebase too"
        );
        clock.publish_sample(true, 193000);
        assert_eq!(clock.clock.unscaled.ticks, 3100);
    }
    #[test]
    fn replay_uses_exact_platform_ticks_and_preserves_counter_precision() {
        let mut clock = Context::default();
        for _ in 0..90 {
            clock.publish(true, true);
        }
        assert_eq!(clock.clock.unscaled.ticks, 182250000);
        assert_eq!(clock.clock.frames, 91);
        let mut large = Context::new(Rate::from_bus_clock(4000).unwrap(), (1 << 54) + 17);
        large.publish_sample(true, (1 << 54) + 17);
        large.publish_sample(true, (1 << 54) + 18);
        assert_eq!(large.clock.unscaled.ticks, (1 << 54) + 18);
    }
    #[test]
    fn rtc_boot_preserves_tick_precision_without_charging_playtime() {
        assert_eq!(civil_rtc(2000, 1, 1, 0, 0, 0), 0);
        assert_eq!(civil_rtc(2000, 3, 1, 0, 0, 0), 60 * 86400);
        assert_eq!(civil_rtc(2001, 1, 1, 0, 0, 0), 366 * 86400);
        assert_eq!(
            civil_rtc(2100, 3, 1, 0, 0, 0) - civil_rtc(2100, 2, 28, 0, 0, 0),
            86400
        );
        let mut clock = Context::boot(Some(600_000_000));
        let seed = 600_000_000u64 * 60_750_000;
        assert_eq!(clock.counter_base, seed);
        for _ in 0..90 {
            clock.publish(true, true);
        }
        assert_eq!(clock.clock.unscaled.ticks, seed + 182250000);
        assert_eq!(
            clock.clock.unscaled_seconds,
            f64::from(clock.clock.rate.seconds(2_025_000)) * 90.
        );
    }
}
