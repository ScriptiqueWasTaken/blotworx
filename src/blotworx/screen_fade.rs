//! EM1 fade timer/setters: REL733c/73a4 and6f58..7160.
//! Native UI entity opacity, input contexts and completion event41 remain open.
use anyhow::{ensure, Result};
#[derive(Debug)]
pub struct State {
    pub out: bool,
    pub entering: bool,
    pub elapsed: f32,
    pub duration: f32,
    pub opacity: f32,
    /// REL733c stores byte6c before checking whether a fade is active.
    /// REL71e4 uses it to gate restoration of the previous mapping contexts.
    pub suppress_mapping_restore: bool,
    pub hide_gameplay_hud: bool,
    pub held: bool,
    pub deferred_updates: i32,
    guard: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            out: false,
            entering: false,
            elapsed: -1.,
            duration: 0.,
            opacity: 0.,
            suppress_mapping_restore: false,
            hide_gameplay_hud: false,
            held: false,
            deferred_updates: 0,
            guard: 0.,
        }
    }
}
impl State {
    pub fn start_out(&mut self, duration: f32) -> Result<()> {
        ensure!(duration.is_finite(), "Invalid fade-out duration");
        if self.out {
            return Ok(());
        }
        self.duration = duration;
        self.out = true;
        self.entering = false;
        self.elapsed = if self.elapsed >= 0. {
            self.duration - self.elapsed
        } else {
            0.
        };
        Ok(())
    }
    pub fn start_in(&mut self, duration: Option<f32>) -> Result<()> {
        self.start_in_mode(duration, false)
    }
    pub fn start_in_mode(
        &mut self,
        duration: Option<f32>,
        suppress_mapping_restore: bool,
    ) -> Result<()> {
        self.suppress_mapping_restore = suppress_mapping_restore;
        self.hide_gameplay_hud = suppress_mapping_restore;
        if !self.out {
            return Ok(());
        }
        if let Some(duration) = duration {
            ensure!(duration.is_finite(), "Invalid fade-in duration");
            if duration >= 0. {
                self.duration = duration;
            }
        }
        self.entering = true;
        self.out = false;
        self.elapsed = if self.elapsed >= 0. {
            self.duration - self.elapsed
        } else {
            0.
        };
        Ok(())
    }
    ///7310 and72b0 query the running timer, not the fade direction alone.
    pub fn busy(&self) -> bool {
        self.elapsed >= 0.
    }
    pub fn restoring_hud(&self) -> bool {
        self.busy() && self.entering && !self.suppress_mapping_restore
    }
    ///7330 stores the update delay before applying the normal fade-in setter.
    pub fn start_in_deferred(
        &mut self,
        updates: i32,
        duration: Option<f32>,
        suppress_mapping_restore: bool,
    ) -> Result<()> {
        self.deferred_updates = updates;
        self.start_in_mode(duration, suppress_mapping_restore)
    }
    /// Returns the native fade-out completion request. The real event receiver
    /// and mapping-mode transitions must be implemented before claiming parity.
    pub fn advance(&mut self, delta: f32) -> Result<bool> {
        self.advance_clocks(delta, delta, 0.)
    }
    ///6f58 reads unscaled clock+44 for the watchdog and scaled clock+48
    ///for interpolation. Camera1da6e8 contributes its alpha after fade work.
    ///The World frame coordinator supplies its shared clock samples. Camera
    ///alpha remains outside the currently connected camera publication path.
    pub fn advance_clocks(
        &mut self,
        unscaled: f32,
        scaled: f32,
        camera_alpha: f32,
    ) -> Result<bool> {
        ensure!(
            unscaled.is_finite()
                && unscaled >= 0.
                && scaled.is_finite()
                && scaled >= 0.
                && camera_alpha.is_finite(),
            "Invalid fade clock or camera alpha"
        );
        if self.held {
            return Ok(false);
        }
        if self.deferred_updates > 0 {
            self.deferred_updates -= 1;
            return Ok(false);
        }
        let mut opacity = 0.;
        if self.out && !self.entering {
            self.guard += unscaled;
            if self.guard > 10. {
                self.entering = true;
                self.out = false;
                self.guard = 0.;
            } else {
                opacity = 1.;
            }
        } else {
            self.guard = 0.;
        }
        let mut completed = false;
        if self.elapsed >= 0. {
            self.elapsed += scaled;
            if self.out && !self.entering {
                if self.elapsed >= self.duration {
                    self.elapsed = -1.;
                    opacity = 1.;
                    completed = true;
                } else {
                    opacity = self.elapsed / self.duration;
                }
            } else if self.entering {
                if self.elapsed >= self.duration {
                    self.finish();
                } else {
                    opacity = 1. - self.elapsed / self.duration;
                }
            }
        } else if self.entering {
            self.finish();
        }
        //7178 compares 1 against the sum: upper clamp only, not max(1,sum).
        self.opacity = (opacity + camera_alpha).min(1.);
        Ok(completed)
    }
    fn finish(&mut self) {
        self.out = false;
        self.entering = false;
        self.elapsed = -1.;
        self.opacity = 0.;
        self.held = false;
        self.deferred_updates = 0;
    }
}
impl super::assets::World {
    pub(super) fn advance_screen_fade_frame(&mut self) -> Result<()> {
        ensure!(
            self.frame_clock.is_published(),
            "Screen fade requires published frame clocks"
        );
        let frame = self.frame_clock.frame;
        if self
            .screen_fade
            .advance_clocks(frame.unscaled, frame.scaled, 0.)?
        {
            let limitation="Fade timer uses shared frame clocks; original UI fade entity, mapping transitions, completion event41 and camera alpha remain incomplete.";
            if !self
                .report
                .unsupported
                .iter()
                .any(|message| message == limitation)
            {
                self.report.unsupported.push(limitation.into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timer_and_composition_match_original_instruction_corpus() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/screen_fade.json"
        ))?;
        for case in data["cases"].as_array().unwrap() {
            let i = &case["initial"];
            let mut s = State {
                out: i["out"].as_bool().unwrap(),
                entering: i["entering"].as_bool().unwrap(),
                elapsed: i["elapsed"].as_f64().unwrap() as f32,
                duration: i["duration"].as_f64().unwrap() as f32,
                opacity: i["opacity"].as_f64().unwrap() as f32,
                suppress_mapping_restore: i["suppress"].as_bool().unwrap(),
                hide_gameplay_hud: false,
                held: i["held"].as_bool().unwrap(),
                deferred_updates: i["deferred"].as_i64().unwrap() as i32,
                guard: i["guard"].as_f64().unwrap() as f32,
            };
            let completed = s.advance_clocks(
                case["unscaled"].as_f64().unwrap() as f32,
                case["scaled"].as_f64().unwrap() as f32,
                case["alpha"].as_f64().unwrap() as f32,
            )?;
            let o = &case["output"];
            assert_eq!(s.out, o["out"].as_bool().unwrap(), "{case}");
            assert_eq!(s.entering, o["entering"].as_bool().unwrap(), "{case}");
            assert_eq!(s.held, o["held"].as_bool().unwrap(), "{case}");
            assert_eq!(
                s.deferred_updates,
                i32::try_from(o["deferred"].as_i64().unwrap())?,
                "{case}"
            );
            for (bits, name) in [
                (s.elapsed.to_bits(), "elapsed_bits"),
                (s.guard.to_bits(), "guard_bits"),
                (s.opacity.to_bits(), "opacity_bits"),
            ] {
                assert_eq!(u64::from(bits), o[name].as_u64().unwrap(), "{name}: {case}");
            }
            assert_eq!(
                completed,
                case["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call == "completion_event41"),
                "{case}"
            );
            assert_eq!(s.suppress_mapping_restore, i["suppress"].as_bool().unwrap());
        }
        Ok(())
    }
    #[test]
    fn deferred_in_and_queries_follow_timer_and_preserve_suppression() -> Result<()> {
        let mut s = State::default();
        s.start_out(1.)?;
        s.advance(0.25)?;
        assert!(s.busy());
        assert!(!s.restoring_hud());
        s.start_in_deferred(2, Some(-1.), true)?;
        assert_eq!(s.duration, 1.);
        assert_eq!(s.elapsed, 0.75);
        assert!(!s.restoring_hud());
        s.held = true;
        s.advance(1.)?;
        assert_eq!(s.deferred_updates, 2);
        assert_eq!(s.elapsed, 0.75);
        s.held = false;
        for n in [1, 0] {
            s.advance(1.)?;
            assert_eq!(s.deferred_updates, n);
            assert_eq!(s.elapsed, 0.75);
        }
        s.advance(0.25)?;
        assert!(!s.busy());
        assert_eq!(s.opacity, 0.);
        s.start_out(1.)?;
        s.start_in(None)?;
        assert!(s.restoring_hud());
        Ok(())
    }
    #[test]
    fn reversal_zero_duration_and_native_guard_preserve_setter_rules() -> Result<()> {
        let mut state = State::default();
        state.start_in(Some(1.))?;
        assert_eq!(state.opacity, 0.);
        state.start_out(1.)?;
        assert!(!state.advance(0.25)?);
        assert_eq!(state.opacity, 0.25);
        state.start_in(None)?;
        assert_eq!(state.elapsed, 0.75);
        state.advance(0.)?;
        assert_eq!(state.opacity, 0.25);
        state.advance(0.25)?;
        assert_eq!(state.opacity, 0.);
        state.start_out(0.)?;
        assert!(state.advance(0.)?);
        assert_eq!(state.opacity, 1.);
        state.start_out(1.)?;
        assert_eq!(state.duration, 0.);
        state.start_in(Some(0.))?;
        state.advance(0.)?;
        assert_eq!(state.opacity, 0.);
        state.start_out(0.)?;
        state.advance(0.)?;
        state.advance(10.)?;
        assert!(state.out);
        state.advance(0.01)?;
        assert!(!state.out);
        assert_eq!(state.opacity, 0.);
        Ok(())
    }
    #[test]
    fn no_hud_flag_is_written_even_without_an_active_fade() -> Result<()> {
        let mut state = State::default();
        state.start_in_mode(Some(2.), true)?;
        assert!(state.suppress_mapping_restore);
        assert!(!state.entering);
        assert_eq!(state.duration, 0.);
        state.start_out(1.)?;
        state.advance(0.25)?;
        state.start_in_mode(None, true)?;
        assert_eq!(state.elapsed, 0.75);
        state.advance(0.25)?;
        assert!(state.suppress_mapping_restore);
        assert_eq!(state.opacity, 0.);
        state.start_in(None)?;
        assert!(!state.suppress_mapping_restore);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_projector_child_alias_resolves_its_prefab_entry_and_fade_callbacks_execute(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        fn call(world: &mut super::super::assets::World, source: &str) -> Result<()> {
            world.run_callback_fixture(source)
        }
        call(&mut world,"local entry=GetRelativePrefabEntity('GV_ZoneA_ProjectorStart','.ExitProjectorMarker'); assert(entry==GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorMarker')); assert(GetRelativePrefabEntity('GV_ZoneA_ProjectorStart','.BwxMissing')==0); assert(GetRelativePrefabEntity('BwxMissing','.ExitProjectorMarker')==0); assert(select('#',StartFadeIn(0))==0)")?;
        // Test the bound callbacks without advancing unrelated authored
        // entrance coroutines, which can issue their own fade on that tick.
        world.screen_fade.advance(0.)?;
        assert_eq!(world.screen_fade.opacity, 0.);
        call(&mut world, "assert(select('#',StartFadeOut(.5))==0)")?;
        assert!(world.screen_fade.out);
        assert_eq!(world.screen_fade.duration, 0.5);
        world.screen_fade.advance(0.05)?;
        assert_eq!(world.screen_fade.opacity, 0.1);
        call(&mut world, "assert(select('#',StartFadeInNoHUD(.5))==0)")?;
        assert!(world.screen_fade.suppress_mapping_restore);
        assert!(world.screen_fade.entering);
        assert!(!world.screen_fade.out);
        world.screen_fade.advance(0.)?;
        assert!((world.screen_fade.opacity - 0.1).abs() < 1e-6);
        call(&mut world, "StartFadeIn()")?;
        assert!(!world.screen_fade.suppress_mapping_restore);
        assert!(
            !world
                .report
                .unsupported
                .iter()
                .any(|message| message.contains("runtime animation console")),
            "{:?}",
            world.report.unsupported
        );
        //The live coordinator consumes published clocks once per frame;
        //advancing character/model catch-up ticks must not advance the fade.
        world.screen_fade = State::default();
        world.screen_fade.start_out(2.)?;
        assert!(world.advance_screen_fade_frame().is_err());
        world.frame_clock = super::super::frame_clock::Context::new(
            super::super::game_clock::Rate::from_bus_clock(4000)?,
            0,
        );
        world.frame_clock.publish_sample(false, 0);
        world.frame_clock.publish_sample(true, 100);
        world.advance_screen_fade_frame()?;
        assert_eq!(world.screen_fade.elapsed, 0.1);
        world.advance_models_with_delta(world.animation_time + 0.2, 0.2)?;
        assert_eq!(
            world.screen_fade.elapsed, 0.1,
            "Model catch-up duplicated the fade update"
        );
        world.frame_clock.frame =
            world
                .frame_clock
                .clock
                .sample(super::super::game_clock::Step::Realtime, 200, 0.5);
        world.advance_screen_fade_frame()?;
        assert_eq!(world.screen_fade.elapsed, 0.1f32 + 0.05);
        assert_eq!(world.screen_fade.guard, 0.2);
        Ok(())
    }
}
