//! GameState80/90 playtime,1f7554/1f75d4; GAME13c/140 metadata2bebc..2bed0.
//! Callers must supply the real shared game clock and a raw hardware sample.
//! World now receives a shared frame context; complete native frame ordering,
//! other component-clock migration and boot RTC origin remain incomplete.
use super::{game_clock::Clock, locomotion_actions::ClockStamp};
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Playtime {
    seconds: i32,
    timer: ClockStamp,
}
impl Playtime {
    ///1f75d4 sets the accumulator and invokes the unscaled timer reset.
    pub fn set(&mut self, seconds: i32, clock: &Clock) {
        self.seconds = seconds;
        self.timer.reset(clock.unscaled.ticks);
    }
    ///1f7554 truncates each queried elapsed interval, adds with GPR wrapping,
    /// and resets the timer even when the interval contributed zero seconds.
    /// It is a mutating checkpoint operation, not a per-frame display query.
    pub fn capture(&mut self, clock: &Clock, hardware_tick_sample: u64) -> Result<Timing> {
        let elapsed = if self.timer.is_running() {
            super::inventory_commands::checked_fctiwz(
                clock
                    .rate
                    .seconds(self.timer.elapsed_ticks(clock.unscaled.ticks)),
            )?
        } else {
            0
        };
        self.seconds = self.seconds.wrapping_add(elapsed);
        self.timer.reset(clock.unscaled.ticks);
        Ok(Timing {
            seconds: self.seconds,
            hardware_tick_sample,
        })
    }
    ///1fdea0 restores the saved accumulator through1f75d4. A prior process's
    /// raw hardware sample never becomes the new timer's timestamp.
    pub fn restore_header(&mut self, header: &[u8], clock: &Clock) -> Result<()> {
        self.set(Timing::from_header(header)?.seconds, clock);
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"accumulated_seconds":self.seconds,"timer_running":self.timer.is_running(),
            "requires_shared_clock_context":true,"checkpoint_integrated":false,"gameplay_restore_complete":false})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub seconds: i32,
    pub hardware_tick_sample: u64,
}
impl Timing {
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint timing header"
        );
        Ok(Self {
            seconds: i32::from_be_bytes(header[0x13c..0x140].try_into()?),
            hardware_tick_sample: u64::from_be_bytes(header[0x140..0x148].try_into()?),
        })
    }
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint timing header"
        );
        header[0x13c..0x140].copy_from_slice(&self.seconds.to_be_bytes());
        header[0x140..0x148].copy_from_slice(&self.hardware_tick_sample.to_be_bytes());
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"seconds":self.seconds,"hardware_tick_sample":self.hardware_tick_sample,
            "hardware_tick_sample_hex":format!("0x{:016x}",self.hardware_tick_sample),
            "timestamp_is_calendar_time":false,"gameplay_restore_complete":false})
    }
}
impl super::assets::World {
    pub fn capture_owned_checkpoint_timing(&mut self, header: &mut [u8]) -> Result<Timing> {
        // The clock constructor already supplies a valid timebase before
        // first-frame publication; OnMapLoad checkpoints may query it too.
        let clock = self.frame_clock.clock;
        let sample = self.frame_clock.hardware_sample();
        self.capture_checkpoint_timing(&clock, sample, header)
    }
    /// Called by a coordinator with its actual frame-clock context. Ordinary
    /// read-only header diagnostics must not restart the playtime timer.
    pub fn capture_checkpoint_timing(
        &mut self,
        clock: &Clock,
        hardware_tick_sample: u64,
        header: &mut [u8],
    ) -> Result<Timing> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint timing header"
        );
        let timing = self.playtime.capture(clock, hardware_tick_sample)?;
        timing.write_header(header)?;
        Ok(timing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_clock(now: u64) -> Clock {
        Clock::new(
            super::super::game_clock::Rate::from_bus_clock(4000).unwrap(),
            now,
        )
    }
    #[test]
    fn getter_matches_original_caller_instruction_cases() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/checkpoint_playtime.json"
        ))?;
        let start = reference["start_ticks"].as_u64().unwrap();
        for case in reference["cases"].as_array().unwrap() {
            let mut clock = make_clock(start);
            let mut state = Playtime {
                seconds: case["initial"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            if case["active"].as_bool().unwrap() {
                state.set(state.seconds, &clock);
            }
            clock.unscaled.ticks = start.wrapping_add_signed(case["delta_ticks"].as_i64().unwrap());
            let captured = state.capture(&clock, 77)?;
            assert_eq!(
                captured.seconds,
                i32::try_from(case["result"].as_i64().unwrap()).unwrap()
            );
            let mut expected_stamp = ClockStamp::default();
            expected_stamp.reset(case["timer_ticks"].as_u64().unwrap());
            assert_eq!(state.timer, expected_stamp);
        }
        Ok(())
    }
    #[test]
    fn query_truncates_each_interval_restarts_and_wraps_the_native_accumulator() -> Result<()> {
        let mut clock = make_clock(100);
        let mut owner = Playtime::default();
        assert_eq!(owner.capture(&clock, 0x1122334455667788)?.seconds, 0);
        clock.sample(super::super::game_clock::Step::Realtime, 1350, 1.);
        assert_eq!(owner.capture(&clock, 55)?.seconds, 1);
        clock.sample(super::super::game_clock::Step::Realtime, 2250, 1.);
        assert_eq!(owner.capture(&clock, 56)?.seconds, 1);
        clock.sample(super::super::game_clock::Step::Realtime, 2450, 1.);
        assert_eq!(
            owner.capture(&clock, 57)?.seconds,
            1,
            "Queried fractional intervals must not accumulate"
        );
        owner.set(i32::MAX, &clock);
        clock.sample(super::super::game_clock::Step::Realtime, 3450, 1.);
        assert_eq!(owner.capture(&clock, 58)?.seconds, i32::MIN);
        let mut zero = make_clock(0);
        owner.set(-7, &zero);
        assert_eq!(
            owner.capture(&zero, 99)?.seconds,
            -7,
            "Native zero stamp becomes1; the tiny negative interval truncates to0"
        );
        zero.sample(super::super::game_clock::Step::Realtime, 1001, 1.);
        assert_eq!(owner.capture(&zero, 100)?.seconds, -6);
        Ok(())
    }
    #[test]
    fn unscaled_time_and_freeze_are_distinct_from_hardware_and_scaled_time() -> Result<()> {
        let mut clock = make_clock(100);
        let mut owner = Playtime::default();
        owner.set(9, &clock);
        clock.sample(super::super::game_clock::Step::Realtime, 1100, 0.125);
        assert_eq!(owner.capture(&clock, 1100)?.seconds, 10);
        clock.sample(super::super::game_clock::Step::Frozen, 101100, 2.);
        let paused = owner.capture(&clock, 101100)?;
        assert_eq!(paused.seconds, 10);
        assert_eq!(paused.hardware_tick_sample, 101100);
        clock.sample(super::super::game_clock::Step::Realtime, 102100, 2.);
        assert_eq!(owner.capture(&clock, 102100)?.seconds, 11);
        Ok(())
    }
    #[test]
    fn timing_roundtrip_and_restore_preserve_other_fields_and_reset_to_current_clock() -> Result<()>
    {
        let value = Timing {
            seconds: -2493,
            hardware_tick_sample: 0xfedcba9876543210,
        };
        let mut header = vec![0xa5; 0x400];
        let before = header.clone();
        value.write_header(&mut header)?;
        assert_eq!(Timing::from_header(&header)?, value);
        for index in 0..header.len() {
            if !(0x13c..0x148).contains(&index) {
                assert_eq!(header[index], before[index]);
            }
        }
        let mut clock = make_clock(1000);
        let mut restored = Playtime::default();
        restored.restore_header(&header, &clock)?;
        clock.sample(super::super::game_clock::Step::Realtime, 2000, 1.);
        assert_eq!(restored.capture(&clock, 7)?.seconds, -2492);
        assert!(value.write_header(&mut header[..0x31f]).is_err());
        assert!(Timing::from_header(&header[..0x31f]).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires native save in BLOTWORX_TEST_SAVE"]
    fn original_timing_roundtrip_keeps_native_save_unchanged() -> Result<()> {
        use anyhow::Context;
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let archive = super::super::save_game::Archive::parse(&bytes)?;
        archive.section(*b"MICK")?;
        let timing = Timing::from_header(&bytes)?;
        let mut rewritten = bytes.clone();
        timing.write_header(&mut rewritten)?;
        assert_eq!(rewritten, bytes);
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
    #[test]
    #[ignore = "requires original files in BLOTWORX_TEST_DATA"]
    fn real_world_time_capture_is_explicit_read_only_diagnostics_do_not_restart_it() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut clock = make_clock(100);
        world.playtime.set(17, &clock);
        let before = world.playtime;
        let mut header = vec![0xa5; 0x400];
        super::super::save_game::initialize_header(&mut header)?;
        let untouched = header[0x13c..0x148].to_vec();
        world.write_save_header_owners(&mut header)?;
        assert_eq!(world.playtime, before);
        assert_eq!(&header[0x13c..0x148], &untouched);
        clock.sample(super::super::game_clock::Step::Realtime, 1350, 0.25);
        world.capture_checkpoint_timing(&clock, 0x1234567887654321, &mut header)?;
        assert_eq!(
            Timing::from_header(&header)?,
            Timing {
                seconds: 18,
                hardware_tick_sample: 0x1234567887654321
            }
        );
        let state = world.playtime;
        assert!(world
            .capture_checkpoint_timing(&clock, 8, &mut header[..0x31f])
            .is_err());
        assert_eq!(
            world.playtime, state,
            "Invalid destination must not consume elapsed time"
        );
        let ctx = egui::Context::default();
        let next = super::super::assets::load_with_score_state(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &ctx,
            None,
            None,
            Default::default(),
            Default::default(),
            super::super::score_state::InitialState {
                playtime: world.playtime,
                ..Default::default()
            },
        )?;
        assert_eq!(next.playtime, world.playtime);
        Ok(())
    }
    #[test]
    #[ignore = "requires original files in BLOTWORX_TEST_DATA"]
    fn real_world_owned_clock_capture_preserves_session_time_through_handoff() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut header = vec![0; super::super::save_game::HEADER_SIZE];
        assert_eq!(
            world.capture_owned_checkpoint_timing(&mut header)?.seconds,
            0
        );
        world.frame_clock = super::super::frame_clock::Context::default();
        world.playtime.set(17, &world.frame_clock.clock);
        world.frame_clock.publish(true, true);
        for _ in 0..90 {
            world.frame_clock.publish(true, true);
        }
        assert_eq!(
            world.capture_owned_checkpoint_timing(&mut header)?.seconds,
            20
        );
        let mut frame_clock = world.frame_clock;
        frame_clock.suspend_for_loading();
        let mut next = super::super::assets::load_with_score_state(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
            None,
            None,
            Default::default(),
            Default::default(),
            super::super::score_state::InitialState {
                playtime: world.playtime,
                frame_clock: Some(frame_clock),
                ..Default::default()
            },
        )?;
        // First incoming publication rebases, without resetting the session.
        next.frame_clock.publish(true, true);
        for _ in 0..30 {
            next.frame_clock.publish(true, true);
        }
        assert_eq!(
            next.capture_owned_checkpoint_timing(&mut header)?.seconds,
            21
        );
        Ok(())
    }
}
