//! Concrete BSQ Lua9 and delay12 effects. Other tags remain explicit errors;
//! the original resource bytes remain authoritative for future handlers.
use anyhow::{ensure, Context, Result};
#[derive(Debug, serde::Serialize)]
pub struct Delay {
    pub duration: f32,
    pub remaining: f32,
}
impl Delay {
    pub fn new(duration: f32) -> Result<Self> {
        ensure!(duration.is_finite(), "Nonfinite BSQ delay");
        Ok(Self {
            duration,
            remaining: duration,
        })
    }
    pub fn reset(&mut self) {
        self.remaining = self.duration;
    }
    pub fn update(&mut self, delta: f32) -> Result<()> {
        ensure!(
            delta.is_finite() && self.remaining.is_finite(),
            "Nonfinite BSQ delay sample"
        );
        //27c6f0: subtract only while positive; preserve overshoot, no clamp.
        if self.remaining > 0. {
            self.remaining -= delta;
        }
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.remaining <= 0.
    }
}
#[derive(Debug)]
pub enum Event {
    Lua(String),
    Delay(Delay),
    Unsupported(u16),
}
impl Event {
    pub fn load(n: &crate::em1_bsq::Node) -> Result<Self> {
        let tag = u16::try_from(
            n.value("_type")
                .and_then(|v| v.as_u64())
                .context("BSQ event type missing")?,
        )?;
        Ok(match tag {
            9 => Self::Lua(
                n.value("Lua")
                    .and_then(|v| v.as_str())
                    .context("BSQ Lua source missing")?
                    .into(),
            ),
            12 => Self::Delay(Delay::new(
                n.value("Seconds")
                    .and_then(|v| v.as_f64())
                    .context("BSQ delay duration missing")? as f32,
            )?),
            _ => Self::Unsupported(tag),
        })
    }
    pub fn kind(&self) -> u16 {
        match self {
            Self::Lua(_) => 9,
            Self::Delay(_) => 12,
            Self::Unsupported(tag) => *tag,
        }
    }
    pub fn reset(&mut self) -> Result<()> {
        match self {
            //Lua9 slot10 is the verified base27a308 empty function.
            Self::Lua(_) => Ok(()),
            Self::Delay(delay) => {
                delay.reset();
                Ok(())
            }
            Self::Unsupported(tag) => anyhow::bail!("BSQ event{tag} reset is not implemented"),
        }
    }
    pub fn waits(&self) -> Result<bool> {
        match self {
            Self::Lua(_) => Ok(false),
            Self::Delay(_) => Ok(true),
            Self::Unsupported(tag) => {
                anyhow::bail!("BSQ event{tag} completion policy is not implemented")
            }
        }
    }
    pub fn ready(&self) -> Result<bool> {
        match self {
            Self::Lua(_) => Ok(true),
            Self::Delay(delay) => Ok(delay.ready()),
            Self::Unsupported(tag) => {
                anyhow::bail!("BSQ event{tag} completion query is not implemented")
            }
        }
    }
}
impl super::assets::World {
    pub(super) fn update_known_story_event(
        &mut self,
        instance: &super::story_instance::Instance,
        event: &mut Event,
        delta: f32,
    ) -> Result<()> {
        match event {
            Event::Delay(delay) => delay.update(delta),
            Event::Lua(source) => {
                //27d680 checks the source and resolves owner+8 before compiling.
                let source = source.split('\0').next().unwrap_or("");
                if source.is_empty() {
                    return Ok(());
                }
                let Some(owner) = instance
                    .owner
                    .filter(|i| *i < self.entities.len() && !self.removed_entities.contains(i))
                else {
                    return Ok(());
                };
                //278ee8 compiles source and schedules it with this entity's
                //context. Lua9 does not wait for that coroutine to finish.
                if let Err(error) = self.scripts.schedule_story_source(source, owner) {
                    //The native compile failure is logged, not raised into the
                    //BSQ instance. Pending coroutine errors retain their own log.
                    self.scripts.report_story_source_error(error)?;
                }
                Ok(())
            }
            Event::Unsupported(tag) => {
                anyhow::bail!("BSQ event{tag} gameplay handler is not implemented")
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delay_preserves_native_overshoot_and_readiness_bits() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/story_instance.json"
        ))?;
        for case in data["delays"].as_array().unwrap() {
            let mut s = Delay::new(case["remaining"].as_f64().unwrap() as f32)?;
            s.update(case["delta"].as_f64().unwrap() as f32)?;
            assert_eq!(
                u64::from(s.remaining.to_bits()),
                case["output_bits"].as_u64().unwrap(),
                "{case}"
            );
            assert_eq!(s.ready(), case["ready"].as_bool().unwrap());
            s.reset();
            assert_eq!(s.remaining.to_bits(), s.duration.to_bits());
        }
        Ok(())
    }
    #[test]
    fn event_completion_policies_do_not_wait_for_lua_threads() -> Result<()> {
        let event = Event::Lua("Wait(100)".into());
        assert_eq!(event.kind(), 9);
        assert!(!event.waits()?);
        assert!(event.ready()?);
        let mut delay = Event::Delay(Delay::new(0.05)?);
        assert!(delay.waits()?);
        assert!(!delay.ready()?);
        let Event::Delay(s) = &mut delay else {
            unreachable!()
        };
        s.update(0.1)?;
        assert!(delay.ready()?);
        delay.reset()?;
        assert!(!delay.ready()?);
        assert!(Event::Unsupported(2).waits().is_err());
        assert!(Event::Unsupported(2).ready().is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_world_executes_story_source_with_owner_across_waits() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        //Settle ready startup tasks before measuring this source's first
        //visit. Native swap-retirement skips a replacement until next update.
        for _ in 0..6 {
            world.advance_models_with_delta(
                world.animation_time + super::super::STEP,
                super::super::STEP,
            )?;
        }
        let instance = super::super::story_instance::Instance {
            owner: Some(world.player_entity),
            ..Default::default()
        };
        let mut event=Event::Lua("assert(_self()==GetPlayer()); assert(_activator()==nil); SetGlobal('2dDirection', -17); Wait(.05); assert(_self()==GetPlayer()); SetGlobal('2dDirection', -18)".into());
        world.update_known_story_event(&instance, &mut event, super::super::STEP)?;
        assert_ne!(
            world.globals.get("2dDirection"),
            Some(-17),
            "Native source scheduling is not immediate inline execution"
        );
        world.advance_models_with_delta(
            world.animation_time + super::super::STEP,
            super::super::STEP,
        )?;
        assert_eq!(
            world.globals.get("2dDirection"),
            Some(-17),
            "{:?}",
            world.scripts.errors
        );
        for _ in 0..3 {
            world.advance_models_with_delta(
                world.animation_time + super::super::STEP,
                super::super::STEP,
            )?;
        }
        assert_eq!(world.globals.get("2dDirection"), Some(-18));
        let before = world.report.unsupported.len();
        let mut invalid = Event::Lua("not valid Lua !".into());
        world.update_known_story_event(&Default::default(), &mut invalid, super::super::STEP)?;
        assert_eq!(world.report.unsupported.len(), before);
        world.update_known_story_event(&instance, &mut invalid, super::super::STEP)?;
        world.advance_models_with_delta(
            world.animation_time + super::super::STEP,
            super::super::STEP,
        )?;
        assert_eq!(world.report.unsupported.len(), before + 1);
        assert!(world.report.unsupported.last().unwrap().contains("BSQ Lua"));
        Ok(())
    }
}
