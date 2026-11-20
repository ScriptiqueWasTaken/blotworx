//! Playable brush integration. Scheduling uses the preview's fixed 30 Hz ticks.
//! Native component ordering, save restoration and event receivers are pending;
//! callback counts expose that missing dispatch instead of claiming delivery.
use super::{ammo, assets::Report, brush, brush_preview, game_clock};
use anyhow::Result;

/// REL 0x2e47a8..0x2e47b8 supplies this UTF-16 name to the audio
/// wrapper at 0x31c464. Its source string starts at REL file offset 0x4f9e54.
pub const DRY_SOUND_EVENT: &str = "Play_sfx_ammo_dry";

pub struct Runtime {
    clock: game_clock::Clock,
    pub inventory_notifications: u64,
    pub dry_sound_requests: u64,
}
impl Default for Runtime {
    fn default() -> Self {
        // A virtual 30,000 Hz timebase makes each preview step exactly 1,000
        // ticks. This is not a claim about the original hardware timer rate.
        Self {
            clock: game_clock::Clock::new(game_clock::Rate::from_bus_clock(120_000).unwrap(), 0),
            inventory_notifications: 0,
            dry_sound_requests: 0,
        }
    }
}
impl Runtime {
    pub fn recharge(&mut self, ammo: &mut ammo::State) -> Result<()> {
        let frame = self.clock.sample(
            game_clock::Step::Realtime,
            self.clock.unscaled.ticks + 1000,
            1.,
        );
        let writes = &mut self.inventory_notifications;
        ammo.tick(&self.clock, frame.hardware, &mut |_, _, _| {
            *writes += 1;
            Ok(())
        })
    }
}
pub struct Effects<'a> {
    pub runtime: &'a mut Runtime,
    pub emitters: &'a mut [bool; 2],
    pub projectiles: &'a mut brush_preview::Projectiles,
    pub report: &'a Report,
    pub owner: u64,
    pub origin: [f32; 3],
    pub target: [f32; 3],
}
impl brush::FiringEffects for Effects<'_> {
    fn deactivate(&mut self, ink: brush::Ink, _: &mut ammo::State) -> Result<()> {
        self.emitters[usize::from(ink == brush::Ink::Thinner)] = false;
        Ok(())
    }
    fn activate(&mut self, ink: brush::Ink, _: &mut ammo::State) -> Result<()> {
        self.emitters[usize::from(ink == brush::Ink::Thinner)] = true;
        Ok(())
    }
    fn spawn(&mut self, ink: brush::Ink, _: &mut ammo::State) -> Result<()> {
        self.projectiles
            .spawn(self.report, ink, self.owner, self.origin, self.target)
    }
    fn insufficient_ammo(&mut self, _: &mut ammo::State) -> Result<()> {
        // Player-owned path: the original requests audio, not an animation or
        // hose shutdown. Playback is not implemented; expose every request.
        self.runtime.dry_sound_requests += 1;
        Ok(())
    }
    fn inventory_written(&mut self, _: usize, _: f32, _: f32) -> Result<()> {
        self.runtime.inventory_notifications += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires BLOTWORX_TEST_DATA imported EM1 files"]
    fn original_empty_tank_audio_event_name() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").expect("Set BLOTWORX_TEST_DATA");
        let bytes = std::fs::read(std::path::Path::new(&root).join("MickeyF213923.213924.rel"))?;
        let actual: Vec<u16> = bytes
            .get(0x4f9e54..)
            .expect("Retail REL string missing")
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .take_while(|c| *c != 0)
            .collect();
        assert_eq!(String::from_utf16(&actual)?, DRY_SOUND_EVENT);
        Ok(())
    }
    #[test]
    #[ignore = "requires BLOTWORX_TEST_DATA imported EM1 files"]
    fn original_neutral_playstyle_is_shared_by_brush_properties() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").expect("Set BLOTWORX_TEST_DATA");
        let bytes = std::fs::read(std::path::Path::new(&root).join("MickeyF213923.213924.rel"))?;
        assert_eq!(&bytes[0x16da0..0x16da4], &[0x38, 0x60, 0x00, 0x32]);
        let world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/BuildingStandards.level",
            &egui::Context::default(),
        )?;
        let settings = world.report.brush_settings.as_ref().unwrap();
        for ink in [brush::Ink::Paint, brush::Ink::Thinner] {
            let channel = if ink == brush::Ink::Paint {
                "Paint"
            } else {
                "Thinner"
            };
            for property in [
                "Speed",
                "Range",
                "Lifetime",
                "Size",
                "StimulusScale",
                "BallScale",
                "Cost",
            ] {
                let base = settings.channels[channel][property].levels[0]
                    .value
                    .unwrap_or(0.);
                assert_eq!(
                    settings.value(
                        ink,
                        property,
                        0,
                        crate::blotworx::ability_value::NEUTRAL_PLAYSTYLE
                    )?,
                    base,
                    "Neutral {channel} {property} must not apply a biased multiplier"
                );
            }
        }
        // Actual Lob data has non-unit endpoint multipliers; preserve those
        // while proving that neutral means 50 rather than the old preview 0.
        let text = std::fs::read_to_string(std::path::Path::new(&root).join("ConfigFiles.ini"))?;
        let values = crate::render_support::game_config::runtime_settings(&text, false);
        let property = crate::blotworx::ability_value::RegisteredProperty::load(
            "Lob",
            "Paint",
            "Cost",
            |key| {
                values
                    .get(key)
                    .map(|v| v.parse::<f32>().map_err(anyhow::Error::from))
                    .transpose()
            },
        )?
        .levels[0];
        let neutral = crate::blotworx::ability_value::evaluate(
            Some(property),
            0,
            crate::blotworx::ability_value::NEUTRAL_PLAYSTYLE,
            true,
            1.,
            1.,
        );
        assert_eq!(neutral, property.value.unwrap());
        assert_ne!(
            crate::blotworx::ability_value::evaluate(Some(property), 0, 0, true, 1., 1.),
            neutral
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires BLOTWORX_TEST_DATA imported EM1 files"]
    fn authored_tanks_recharge_fire_deplete_and_recover() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").expect("Set BLOTWORX_TEST_DATA");
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/BuildingStandards.level",
            &egui::Context::default(),
        )?;
        assert!(!world.ammo.policy.infinite && !world.ammo.policy.shared);
        assert_eq!(world.ammo.inventory.as_ref().unwrap().amounts, [0.; 2]);
        for _ in 0..120 {
            world.brush_runtime.recharge(&mut world.ammo)?;
        }
        assert!(world.ammo.available(0) > 6. && world.ammo.available(1) > 6.);
        for ink in [brush::Ink::Paint, brush::Ink::Thinner] {
            let channel = usize::from(ink == brush::Ink::Thinner);
            let other = world.ammo.available(1 - channel);
            let settings = world.report.brush_settings.as_ref().unwrap();
            let cost = settings.value(
                ink,
                "Cost",
                0,
                crate::blotworx::ability_value::NEUTRAL_PLAYSTYLE,
            )?;
            let (interval, burst) = (settings.projectile_interval, settings.burst);
            let mut phase = brush::Firing::begin(ink, Default::default());
            let mut emitters = [false; 2];
            let before = world.projectile_preview.emitted;
            for _ in 0..300 {
                let mut effects = Effects {
                    runtime: &mut world.brush_runtime,
                    emitters: &mut emitters,
                    projectiles: &mut world.projectile_preview,
                    report: &world.report,
                    owner: world.player_entity as u64,
                    origin: [0., 2., 0.],
                    target: [0., 2., 10.],
                };
                phase.update(
                    ink,
                    true,
                    1. / 30.,
                    interval,
                    burst,
                    cost,
                    &mut brush::AmmoFiringHost {
                        ammo: &mut world.ammo,
                        effects: &mut effects,
                    },
                )?;
                world.advance_brush_projectiles(1. / 30.)?;
            }
            assert!(
                world.projectile_preview.emitted > before,
                "No actual projectile spawned"
            );
            assert!(world.ammo.available(channel) < cost / 30.);
            assert_eq!(
                world.ammo.available(1 - channel),
                other,
                "Firing drained the other tank"
            );
            println!(
                "{ink:?}: {} projectiles, remaining {}",
                world.projectile_preview.emitted - before,
                world.ammo.available(channel)
            );
        }
        assert!(world.brush_runtime.dry_sound_requests > 0);
        for _ in 0..360 {
            world.brush_runtime.recharge(&mut world.ammo)?;
        }
        for channel in 0..2 {
            assert!(
                world.ammo.available(channel) >= world.ammo.settings.channels[channel].recharge_max
            );
            assert!(
                world.ammo.available(channel)
                    <= world.ammo.inventory.as_ref().unwrap().maxima[channel].unwrap()
            );
        }
        assert!(world.brush_runtime.inventory_notifications > 0);
        Ok(())
    }
}
