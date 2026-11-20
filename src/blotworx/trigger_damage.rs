//! Damage-channel selection, EM1 REL3cdc80..3cde18. Overlap admission and
//! character action responses are separate compatibility work.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

pub struct Authored {
    rates: Vec<f32>,
    types: Vec<u32>,
    layers: Vec<u32>,
    per_layer: bool,
    pub entry_only: bool,
    pub alias: String,
}
/// REL3d05f4 expands the authored player/character layer selectors.
pub(super) fn layer_mask(layer: u32) -> u32 {
    let bit = if layer < 32 { 1u32 << layer } else { 0 };
    bit | match layer {
        3 => 1 << 18,
        4 => 1 << 20,
        _ => 0,
    }
}
impl Authored {
    pub fn load(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "TriggerDamageComponent",
            "Not a damage trigger"
        );
        let field = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("Damage trigger lacks {name}"))
        };
        let array = |name: &str| {
            field(name)?
                .as_array()
                .with_context(|| format!("Invalid damage trigger array {name}"))
        };
        let integers = |name: &str| -> Result<Vec<u32>> {
            array(name)?
                .iter()
                .map(|v| {
                    v.as_u64()
                        .context("Invalid damage-trigger integer")?
                        .try_into()
                        .map_err(Into::into)
                })
                .collect()
        };
        let rates = array("Damage per second")?
            .iter()
            .map(|v| {
                let rate = v.as_f64().context("Invalid damage-trigger rate")? as f32;
                ensure!(rate.is_finite(), "Nonfinite damage-trigger rate");
                Ok(rate)
            })
            .collect::<Result<Vec<_>>>()?;
        let types = integers("Damage Type Array")?;
        ensure!(
            types.iter().all(|&kind| kind < 15),
            "Unknown damage-trigger stimulus type"
        );
        Ok(Self {
            rates,
            types,
            layers: integers("Collidable Collision Layers")?,
            per_layer: field("Damage Type Per Collision Layer")?
                .as_bool()
                .context("Invalid per-layer damage flag")?,
            entry_only: field("Damage Only On Entry")?
                .as_bool()
                .context("Invalid entry damage flag")?,
            alias: field("Damage Alias")?
                .as_str()
                .context("Invalid damage alias")?
                .into(),
        })
    }
    pub fn admits_layer(&self, layer: u32) -> bool {
        self.layers
            .iter()
            .any(|&selector| layer_mask(selector) & layer_mask_bit(layer) != 0)
    }
    pub fn samples(&self, elapsed: f32, recipient_layer: Option<u32>) -> Result<Vec<(u32, f32)>> {
        ensure!(elapsed.is_finite(), "Nonfinite damage-trigger elapsed time");
        if self.rates.is_empty() {
            return Ok(Vec::new());
        }
        let count = if self.per_layer {
            self.types.len().min(self.layers.len())
        } else {
            self.types.len()
        };
        let mut result = Vec::new();
        for i in 0..count {
            if self.per_layer
                && !recipient_layer
                    .is_some_and(|layer| layer_mask(self.layers[i]) & layer_mask_bit(layer) != 0)
            {
                continue;
            }
            // The native getter repeats the final rate when the type array is
            // longer. Preserve channel order and duplicate types in the batch.
            let amount = elapsed * self.rates[i.min(self.rates.len() - 1)];
            ensure!(amount.is_finite(), "Damage-trigger amount overflow");
            if amount > 0. {
                result.push((self.types[i], amount));
            }
        }
        Ok(result)
    }
}
fn layer_mask_bit(layer: u32) -> u32 {
    if layer < 32 {
        1u32 << layer
    } else {
        0
    }
}

#[cfg(any(test, debug_assertions))]
pub(super) mod tests {
    use super::*;
    #[test]
    fn steam_delivers_the_positive_later_channel_without_integer_accumulation() -> Result<()> {
        let settings = Authored {
            rates: vec![0., 10.],
            types: vec![0, 3],
            layers: vec![3],
            per_layer: false,
            entry_only: false,
            alias: String::new(),
        };
        assert_eq!(
            settings.samples(1. / 30., Some(3))?,
            [(3, 10. * (1. / 30.))]
        );
        assert!(settings.admits_layer(18));
        assert!(!settings.admits_layer(4));
        Ok(())
    }
    #[test]
    fn thinner_pool_selects_recipient_layers_before_delivering_a_single_batch() -> Result<()> {
        let settings = Authored {
            rates: vec![100000., 1., 1., 1000., 1.],
            types: vec![0, 0, 6, 0, 1],
            layers: vec![4, 3, 3, 2, 7],
            per_layer: true,
            entry_only: false,
            alias: "ThinnerOcean".into(),
        };
        let player = settings.samples(1. / 30., Some(3))?;
        assert_eq!(player, [(0, 1. / 30.), (6, 1. / 30.)]);
        assert_eq!(settings.samples(1. / 30., Some(18))?, player);
        assert_eq!(super::super::health::stimulus_damage(&player, false)?, 2);
        assert!(settings.samples(1., None)?.is_empty());
        Ok(())
    }
    #[test]
    fn short_rate_arrays_repeat_the_last_value_and_entry_uses_unit_elapsed() -> Result<()> {
        let settings = Authored {
            rates: vec![1.],
            types: vec![0, 6],
            layers: vec![3, 3],
            per_layer: false,
            entry_only: true,
            alias: String::new(),
        };
        assert_eq!(settings.samples(1., Some(3))?, [(0, 1.), (6, 1.)]);
        assert_eq!(
            settings.samples(1. / 30., Some(3))?,
            [(0, 1. / 30.), (6, 1. / 30.)]
        );
        Ok(())
    }
    #[cfg_attr(test, test)]
    #[cfg_attr(test, ignore = "requires original EM1 files in BLOTWORX_TEST_DATA")]
    pub(in crate::blotworx) fn original_hazard_arrays_reach_the_live_player_on_the_first_overlap(
    ) -> Result<()> {
        use super::super::assets::World;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        world.advance_models(0.)?;
        world.attach_player_body()?;
        let mut metadata = Vec::new();
        for entity in &world.entities {
            for component in &entity.components {
                if component.class_name == "TriggerDamageComponent" {
                    let damage = Authored::load(component)?;
                    metadata.push(serde_json::json!({"entity":entity.name,"per_layer":damage.per_layer,"rates":damage.rates,"types":damage.types,"player_samples":damage.samples(1./30.,Some(3))?}));
                }
            }
        }
        std::fs::write(
            "target/demo-hazard-owned.json",
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        // Isolate one original volume at a time. These are receiver fixtures,
        // not continuous traversal; game files and channel values are intact.
        world.pickup_receivers.clear();
        fn isolate(world: &mut World, name: &str) -> Result<[f32; 3]> {
            let selected = world
                .scripts
                .resolve_name(name)
                .context("Original hazard missing")?;
            for (entity, e) in world.entities.iter().enumerate() {
                for (index, c) in e.components.iter().enumerate() {
                    if matches!(
                        c.class_name.as_str(),
                        "TriggerVolumeComponent"
                            | "TriggerPlaneComponent"
                            | "TriggerDamageComponent"
                    ) {
                        world
                            .component_control
                            .disabled
                            .entry(entity)
                            .or_default()
                            .insert(index, entity != selected);
                    }
                }
            }
            let bounds = if let Some(bounds) = world.phantom_bounds(selected)? {
                bounds
            } else {
                let b = world.resource_phantom_aabb(selected, 0.)?;
                super::super::phantom_bounds::Bounds {
                    min: b[0][..3].try_into()?,
                    max: b[1][..3].try_into()?,
                }
            };
            let mut position = std::array::from_fn(|i| (bounds.min[i] + bounds.max[i]) * 0.5);
            position[0] += (bounds.max[0] - bounds.min[0]) * 0.2;
            Ok(position)
        }
        let mut player = world.take_startup_player();
        player.position = isolate(&mut world, "gremlinvillage_zonea.SteamTrigger 09")?;
        world.entities[world.player_entity].set_controller_pose(player.position, player.yaw);
        let health = world.runtime_health[&world.player_entity].health;
        let position = player.position;
        world.with_live_player(&mut player, |world| {
            world.advance_demo_triggers(position, super::super::STEP)
        })?;
        assert_eq!(
            world.runtime_health[&world.player_entity].health,
            health - 1,
            "Fractional damage was delayed or the recipient's vulnerability was bypassed"
        );
        player.position = isolate(&mut world, "gremlinvillage_zonea.SteamTrigger 01")?;
        player.velocity = [0.; 3];
        world.entities[world.player_entity].set_controller_pose(player.position, player.yaw);
        let position = player.position;
        world.with_live_player(&mut player, |world| {
            world.advance_demo_triggers(position, super::super::STEP)
        })?;
        assert!(
            player.native_knockback
                && world
                    .player_knockback
                    .as_ref()
                    .is_some_and(|runtime| runtime.state.active),
            "The positive second steam channel did not enter native motion10"
        );
        // Native motion launches on phase1, not inside the stimulus callback.
        // This next independent volume fixture does not simulate that launch.
        assert_eq!(
            player.position, position,
            "Damage trigger warped its recipient"
        );
        player.position = isolate(
            &mut world,
            "gremlinvillage_zonea_static.triggers_thinnerpool 01.TriggerDamageVolume_ThinnerPool",
        )?;
        player.native_knockback = false;
        world.player_knockback = None;
        player.velocity = [0.; 3];
        player.grounded = true;
        let health = world.authored_health[&world.player_entity].maximum;
        let state = world.runtime_health.get_mut(&world.player_entity).unwrap();
        state.health = health;
        state.invulnerable_until = 0.;
        world.entities[world.player_entity].set_controller_pose(player.position, player.yaw);
        let position = player.position;
        world.with_live_player(&mut player, |world| {
            world.advance_demo_triggers(position, super::super::STEP)
        })?;
        assert!(
            world.runtime_health[&world.player_entity].health < health,
            "Thinner pool caused no damage"
        );
        assert!(
            player.velocity[1] > 0. && !player.grounded,
            "Thinner pool did not start its recovery leap"
        );
        assert_eq!(player.velocity[1], player.motion.damage_leap_launch_speed);
        assert!(player.damage_leaping);
        let expected_event = world.player_behavior.single("hkbBehaviorGraphStringData")?["eventNames"]
            .as_array().context("Missing Mickey graph events")?.iter()
            .position(|name| name.as_str() == Some("EVENT_Start_Damaged_Leap"))
            .context("Mickey graph lacks the damaged-leap event")?;
        assert!(world.player_script_animation_events.iter().any(|(event, _)| event.id as usize == expected_event));
        println!("PASS: original steam damage/knockback and thinner pool damage/recovery leap");
        Ok(())
    }
}
