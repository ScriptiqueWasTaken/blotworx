//! Original shared Paint/Thinner maxed-glow timers, REL344148..3442bc.
use super::{game_clock::Clock, locomotion_actions::ClockStamp};
use anyhow::{ensure, Result};

/// Own once in the runtime, not separately for each InventoryComponent.
#[derive(Default)]
pub struct GlowTimers {
    timers: [ClockStamp; 2],
}
impl GlowTimers {
    /// Channel0 Paint, channel1 Thinner. Duration comes from original variable
    /// UI_InventoryMaxedGlow_DisplayTime (default string0.5), not host time.
    pub fn show(
        &mut self,
        channel: usize,
        duration: f32,
        clock: &Clock,
        mut dispatch: impl FnMut(&str, &str, f32) -> Result<()>,
    ) -> Result<bool> {
        ensure!(channel < 2, "Invalid maxed-glow channel {channel}");
        let timer = &mut self.timers[channel];
        if timer.is_running() {
            let elapsed = clock.rate.seconds(timer.elapsed_ticks(clock.scaled_ticks));
            // Native ble skips on equality and unordered; do not invert <=.
            if !(elapsed > 0.5 * duration) {
                return Ok(false);
            }
        }
        dispatch(
            if channel == 0 {
                "UpdatePaintGlow"
            } else {
                "UpdateThinnerGlow"
            },
            "Show",
            duration,
        )?;
        timer.reset(clock.scaled_ticks);
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_glow_timers_preserve_strict_boundary_and_channel_independence() -> Result<()> {
        let rate = super::super::game_clock::Rate::from_bus_clock(400)?;
        let mut clock = Clock::new(rate, 100);
        let mut timers = GlowTimers::default();
        let mut calls = vec![];
        assert!(timers.show(0, 2., &clock, |name, arg, d| {
            calls.push((name.to_owned(), arg.to_owned(), d));
            Ok(())
        })?);
        clock.scaled_ticks = 200;
        assert!(!timers.show(0, 2., &clock, |_, _, _| panic!("Equality must not show"))?);
        assert!(timers.show(1, 2., &clock, |name, _, _| {
            assert_eq!(name, "UpdateThinnerGlow");
            Ok(())
        })?);
        clock.scaled_ticks = 201;
        assert!(timers.show(0, 2., &clock, |_, _, _| Ok(()))?);
        assert!(!timers.show(0, f32::NAN, &clock, |_, _, _| panic!(
            "Unordered must not show"
        ))?);
        assert_eq!(calls, vec![("UpdatePaintGlow".into(), "Show".into(), 2.)]);
        Ok(())
    }
    #[test]
    fn failed_dispatch_does_not_reset_timer() -> Result<()> {
        let clock = Clock::new(super::super::game_clock::Rate::from_bus_clock(400)?, 0);
        let mut timers = GlowTimers::default();
        assert!(timers
            .show(0, 0.5, &clock, |_, _, _| anyhow::bail!("UI unavailable"))
            .is_err());
        assert!(!timers.timers[0].is_running());
        assert!(timers.show(0, 0.5, &clock, |_, _, _| Ok(()))?);
        assert!(!timers.show(0, 0.5, &clock, |_, _, _| panic!(
            "Zero timestamp reset sentinel"
        ))?);
        Ok(())
    }
}
