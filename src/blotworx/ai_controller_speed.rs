//! BasicAI scalar setters30037c/3003f8 and input recomputation300ccc.
//! Native behavior selection, path planning and physical locomotion remain
//! separate; this module publishes the existing controller input front end.
use super::assets::World;
use anyhow::{ensure, Context, Result};

pub(super) fn normalized(desired: f32, maximum: f32) -> f32 {
    let value = if maximum > 0. { desired / maximum } else { 0. };
    value * -1.
}
pub(super) trait Host {
    fn disabled(&mut self) -> Result<bool>;
    fn store(&mut self, vertical: bool, value: f32) -> Result<()>;
    fn maximum(&mut self) -> Result<f32>;
    fn desired(&mut self) -> Result<f32>;
    fn publish(&mut self, value: f32) -> Result<()>;
}
pub(super) fn recompute(host: &mut impl Host) -> Result<()> {
    let maximum = host.maximum()?;
    let desired = host.desired()?;
    host.publish(normalized(desired, maximum))
}
pub(super) fn set(host: &mut impl Host, vertical: bool, value: f32) -> Result<()> {
    if host.disabled()? {
        return Ok(());
    }
    host.store(vertical, value)?;
    recompute(host)
}
struct Binding<'a> {
    world: &'a mut World,
    entity: usize,
}
impl Binding<'_> {
    fn read(&self, class: &str, name: &str) -> Result<f32> {
        self.world
            .entities
            .get(self.entity)
            .context("AI speed owner absent")?
            .components
            .iter()
            .find(|c| c.class_name == class)
            .and_then(|c| c.properties.iter().find(|p| p.name == name))
            .map(|p| {
                p.value
                    .as_f64()
                    .map(|v| v as f32)
                    .with_context(|| format!("Invalid {class} scalar {name}"))
            })
            .transpose()
            .map(|v| v.unwrap_or(0.))
    }
}
impl Host for Binding<'_> {
    fn disabled(&mut self) -> Result<bool> {
        Ok(self
            .world
            .ai_component_disabled(self.entity, "BasicAIControllerComponent"))
    }
    fn store(&mut self, vertical: bool, value: f32) -> Result<()> {
        self.world.set_entity_float_property(
            self.entity,
            if vertical {
                "Desired Vertical Speed"
            } else {
                "Desired Move Speed"
            },
            value,
        )
    }
    fn maximum(&mut self) -> Result<f32> {
        self.read("BasicMotionControllerComponent", "Max Move Speed")
    }
    fn desired(&mut self) -> Result<f32> {
        self.read("BasicAIControllerComponent", "Desired Move Speed")
    }
    fn publish(&mut self, value: f32) -> Result<()> {
        let state = self.world.npc_relocations.entry(self.entity).or_default();
        state.desired_speed = value;
        let facing = state.facing;
        self.world.publish_npc_motion_input(self.entity, facing)
    }
}
impl World {
    pub(super) fn set_npc_ai_speed(
        &mut self,
        entity: usize,
        vertical: bool,
        value: f32,
    ) -> Result<()> {
        let target = self.entities.get(entity).context("AI speed owner absent")?;
        ensure!(
            target
                .components
                .iter()
                .filter(|c| c.class_name == "BasicAIControllerComponent")
                .count()
                == 1,
            "AI speed requires exactly one BasicAI controller"
        );
        //30037c's early gate precedes the property write;300ccc does not
        // subsequently re-read this gate after its reflected getter callbacks.
        set(
            &mut Binding {
                world: self,
                entity,
            },
            vertical,
            value,
        )
    }
    pub(super) fn recompute_npc_ai_speed(&mut self, entity: usize) -> Result<()> {
        recompute(&mut Binding {
            world: self,
            entity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_speed_recomputation_matches_input_words() -> Result<()> {
        let f: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_controller_speed.json"
        ))?;
        for c in f["cases"].as_array().unwrap() {
            let desired = f32::from_bits(c["desired"].as_u64().unwrap() as u32);
            let maximum = f32::from_bits(c["maximum"].as_u64().unwrap() as u32);
            assert_eq!(
                normalized(desired, maximum).to_bits(),
                c["normalized"].as_u64().unwrap() as u32,
                "{c}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_scalar_setter_gate_and_callback_order_match() -> Result<()> {
        use serde_json::{json, Value};
        struct Fixture<'a> {
            case: &'a Value,
            desired: f32,
            maximum: f32,
            disabled: bool,
            trace: Vec<Value>,
            published: Option<u32>,
        }
        impl Host for Fixture<'_> {
            fn disabled(&mut self) -> Result<bool> {
                self.trace.push(json!(["disabled", self.disabled]));
                Ok(self.disabled)
            }
            fn store(&mut self, vertical: bool, value: f32) -> Result<()> {
                self.trace.push(json!(["store", vertical, value.to_bits()]));
                if !vertical {
                    self.desired = value;
                }
                if self.case["disable_after_store"].as_bool().unwrap_or(false) {
                    self.disabled = true;
                }
                Ok(())
            }
            fn maximum(&mut self) -> Result<f32> {
                self.trace.push(json!(["maximum"]));
                Ok(self.maximum)
            }
            fn desired(&mut self) -> Result<f32> {
                self.trace.push(json!(["desired"]));
                Ok(self.desired)
            }
            fn publish(&mut self, value: f32) -> Result<()> {
                self.published = Some(value.to_bits());
                self.trace.push(json!(["input", 0, value.to_bits()]));
                Ok(())
            }
        }
        let f: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_controller_speed.json"
        ))?;
        for c in f["setters"].as_array().unwrap() {
            let value = f32::from_bits(c["value"].as_u64().unwrap() as u32);
            let mut host = Fixture {
                case: c,
                desired: f32::from_bits(c["desired"].as_u64().unwrap() as u32),
                maximum: f32::from_bits(c["maximum"].as_u64().unwrap() as u32),
                disabled: c["disabled"].as_bool().unwrap(),
                trace: vec![],
                published: None,
            };
            set(&mut host, c["vertical"].as_bool().unwrap(), value)?;
            assert_eq!(json!(host.trace), c["trace"], "{c}");
            assert_eq!(json!(host.published), c["published"], "{c}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_controller_speed_setter_reaches_shared_input_and_respects_disable() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let entity = world
            .scripts
            .resolve_name("gremlinvillage_zonea.SpatterMelee 01")
            .context("Original Spatter missing")?;
        let index = world.entities[entity]
            .components
            .iter()
            .position(|c| c.class_name == "BasicAIControllerComponent")
            .unwrap();
        world.apply_basic_ai_disabled(entity, index, false)?;
        let maximum = world.entities[entity]
            .components
            .iter()
            .find(|c| c.class_name == "BasicMotionControllerComponent")
            .and_then(|c| c.properties.iter().find(|p| p.name == "Max Move Speed"))
            .and_then(|p| p.value.as_f64())
            .context("Original maximum speed missing")? as f32;
        world.frame_clock.publish(true, true);
        world.set_npc_ai_speed(entity, false, maximum * 0.5)?;
        assert_eq!(world.npc_relocations[&entity].desired_speed, -0.5);
        assert_eq!(world.npc_relocations[&entity].stored_input[1], -0.5);
        // Direct reflected scalar writes remain ordinary storage; they do
        // not pretend to invoke the explicit native speed setter/recompute.
        world.set_entity_float_property(entity, "Desired Move Speed", maximum * 0.25)?;
        assert_eq!(world.npc_relocations[&entity].desired_speed, -0.5);
        world.set_npc_ai_speed(entity, true, 3.)?;
        assert_eq!(world.npc_relocations[&entity].desired_speed, -0.25);
        let before = world.npc_relocations[&entity].stored_input;
        world.apply_basic_ai_disabled(entity, index, true)?;
        world.set_npc_ai_speed(entity, false, maximum)?;
        assert_eq!(world.npc_relocations[&entity].stored_input, before);
        let value = world.entities[entity].components[index]
            .properties
            .iter()
            .find(|p| p.name == "Desired Move Speed")
            .unwrap()
            .value
            .as_f64()
            .unwrap() as f32;
        assert_eq!(value, maximum * 0.25);
        Ok(())
    }
}
