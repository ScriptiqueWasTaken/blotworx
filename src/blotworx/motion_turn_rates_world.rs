//! Real World property/cache providers and native turn override callbacks.
//! This does not implement the remaining BasicMotion physics controller.
use super::{
    assets::World,
    motion_turn_rates::{self, Properties},
};
use anyhow::{ensure, Context, Result};
impl World {
    fn turn_rate_motion_component(&self, owner: usize) -> Result<Option<usize>> {
        let entity = self.entities.get(owner).context("Motion owner is absent")?;
        ensure!(
            !entity
                .components
                .iter()
                .any(|c| c.class_name == "MickeyPlayerMotionControllerComponent"),
            "Mickey's derived motion-controller cache binding is not connected"
        );
        let mut slots = entity
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| c.class_name == "BasicMotionControllerComponent")
            .map(|(index, _)| index);
        let first = slots.next();
        ensure!(
            slots.next().is_none(),
            "Turn rate requires native motion-component slot arbitration"
        );
        Ok(first)
    }
    pub(super) fn initialize_npc_motion_turn_rates(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Result<()> {
        for owner in range {
            if !self.entities[owner]
                .components
                .iter()
                .any(|c| c.class_name == "BasicMotionControllerComponent")
                || self.entities[owner]
                    .components
                    .iter()
                    .any(|c| c.class_name == "MickeyPlayerMotionControllerComponent")
            {
                continue;
            }
            if self
                .npc_relocations
                .get(&owner)
                .is_some_and(|s| s.turn_rates.initialization_applied)
            {
                continue;
            }
            //305290..3052ec uses the same getters and conversion as30c868.
            self.reset_npc_motion_turn_rates(owner)?;
            self.npc_relocations
                .get_mut(&owner)
                .unwrap()
                .turn_rates
                .initialization_applied = true;
        }
        Ok(())
    }
    pub(super) fn reset_npc_motion_turn_rates(&mut self, owner: usize) -> Result<()> {
        let slot = self
            .turn_rate_motion_component(owner)?
            .context("Turn cache reset requires its motion controller")?;
        struct Binding<'a>(&'a epic_mickey_lib_rs::scene_file::Component);
        impl Properties for Binding<'_> {
            fn read(&mut self, name: &str, out: &mut Option<u32>) -> Result<()> {
                let value = self
                    .0
                    .properties
                    .iter()
                    .find(|p| p.name == name)
                    .with_context(|| {
                        format!("Motion property provider is missing resolved {name}")
                    })?
                    .value
                    .as_f64()
                    .with_context(|| format!("Invalid motion property {name}"))?
                    as f32;
                *out = Some(value.to_bits());
                Ok(())
            }
        }
        let component = &self.entities[owner].components[slot];
        self.npc_relocations
            .entry(owner)
            .or_default()
            .turn_rates
            .reset(&mut Binding(component))
    }
    pub(super) fn set_npc_moving_turn_rate(&mut self, owner: usize, bits: u32) -> Result<()> {
        self.turn_rate_motion_component(owner)?
            .context("Moving turn cache requires its motion controller")?;
        self.npc_relocations
            .entry(owner)
            .or_default()
            .turn_rates
            .set_moving(bits);
        Ok(())
    }
    pub(super) fn read_npc_ground_stop_acceleration(&self, owner: usize) -> Result<u32> {
        let slot = self
            .turn_rate_motion_component(owner)?
            .context("Ground stop acceleration requires its motion controller")?;
        let value = self.entities[owner].components[slot]
            .properties
            .iter()
            .find(|p| p.name == motion_turn_rates::GROUND_CONTROL_ACCEL_STOP)
            .context("Resolved GroundControlAccelStop is missing")?
            .value
            .as_f64()
            .context("Invalid GroundControlAccelStop")? as f32;
        Ok(value.to_bits())
    }
    pub(super) fn write_npc_ground_stop_acceleration(
        &mut self,
        owner: usize,
        bits: u32,
    ) -> Result<()> {
        let slot = self
            .turn_rate_motion_component(owner)?
            .context("Ground stop acceleration requires its motion controller")?;
        let value = serde_json::Number::from_f64(f32::from_bits(bits) as f64).context(
            "Nonfinite reflected motion scalars require a bit-preserving property backend",
        )?;
        let property = self.entities[owner].components[slot]
            .properties
            .iter_mut()
            .find(|p| p.name == motion_turn_rates::GROUND_CONTROL_ACCEL_STOP)
            .context("Resolved GroundControlAccelStop is missing")?;
        property.value = serde_json::Value::Number(value);
        Ok(())
    }
    pub(super) fn motion_turn_override_lua(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        mut args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        if !matches!(name, "SetOverrideTurnRate" | "ResetOverrideTurnRate") {
            return Err(mlua::Error::external("Unknown turn-rate command"));
        }
        let set = name == "SetOverrideTurnRate";
        let expected = if set { 2 } else { 1 };
        if args.len() != expected {
            self.report.warnings.push(format!("{name}: native argument count diagnostic ({} arguments); continued with source stack order",args.len()));
        }
        let value = if set {
            Some(
                (lua.coerce_number(args.pop_back().unwrap_or(mlua::Value::Nil))?
                    .unwrap_or(0.) as f32)
                    .to_bits(),
            )
        } else {
            None
        };
        let owner = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|owner| !self.removed_entities.contains(owner));
        let Some(owner) = owner else {
            return Ok(mlua::MultiValue::new());
        };
        if self
            .turn_rate_motion_component(owner)
            .map_err(mlua::Error::external)?
            .is_none()
        {
            return Ok(mlua::MultiValue::new());
        }
        if let Some(value) = value {
            self.npc_relocations
                .entry(owner)
                .or_default()
                .turn_rates
                .set_both(value);
        } else {
            self.reset_npc_motion_turn_rates(owner)
                .map_err(mlua::Error::external)?;
        }
        Ok(mlua::MultiValue::new())
    }
}
