//! BasicMotion turn caches30c868/30c910/30c918/30c920. Unknown construction
//! words remain unknown until an actual getter or setter supplies them.
use anyhow::{Context, Result};
pub const GROUND_CONTROL_ACCEL_STOP: &str = "GroundControlAccelStop";
pub const STATIONARY_TURN_RATE: &str = "Stationary Turn Rate";
pub const MOVING_TURN_RATE: &str = "Moving Turn Rate";
#[derive(Default, Debug, serde::Serialize)]
pub struct Rates {
    pub stationary: Option<u32>,
    pub moving: Option<u32>,
    /// World initialization bookkeeping, not an invented native cache value.
    pub initialization_applied: bool,
}
pub trait Properties {
    /// A declined native getter leaves the destination unchanged. An unbound
    /// property provider is an error, not a declined getter or default zero.
    fn read(&mut self, name: &str, destination: &mut Option<u32>) -> Result<()>;
}
impl Rates {
    pub fn reset(&mut self, properties: &mut impl Properties) -> Result<()> {
        properties.read(STATIONARY_TURN_RATE, &mut self.stationary)?;
        let stationary = self
            .stationary
            .context("Stationary turn cache remains uninitialized")?;
        self.stationary = Some((f32::from_bits(stationary) * f32::from_bits(0x40c90fdb)).to_bits());
        properties.read(MOVING_TURN_RATE, &mut self.moving)?;
        let moving = self
            .moving
            .context("Moving turn cache remains uninitialized")?;
        self.moving = Some((f32::from_bits(moving) * f32::from_bits(0x40c90fdb)).to_bits());
        Ok(())
    }
    pub fn set_stationary(&mut self, value: u32) {
        self.stationary = Some(value);
    }
    pub fn set_moving(&mut self, value: u32) {
        self.moving = Some(value);
    }
    pub fn set_both(&mut self, value: u32) {
        self.set_stationary(value);
        self.set_moving(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn motion_turn_caches_match_original_getter_retention_conversion_and_setters() -> Result<()> {
        let catalog: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/motion_property_catalog.json"
        ))?;
        for (slot, name) in [
            (0x28, GROUND_CONTROL_ACCEL_STOP),
            (0x54, STATIONARY_TURN_RATE),
            (0x58, MOVING_TURN_RATE),
        ] {
            assert_eq!(catalog["properties"][slot / 4]["name"], name);
        }
        struct Binding<'a> {
            case: &'a Value,
            trace: Vec<String>,
        }
        impl Properties for Binding<'_> {
            fn read(&mut self, name: &str, out: &mut Option<u32>) -> Result<()> {
                self.trace.push(name.into());
                let index = if name == STATIONARY_TURN_RATE { 0 } else { 1 };
                if let Some(value) = self.case["read"][index].as_u64() {
                    *out = Some(value as u32);
                }
                Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/motion_turn_rates.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let mut rates = Rates {
                stationary: Some(c["initial"][0].as_u64().unwrap() as u32),
                moving: Some(c["initial"][1].as_u64().unwrap() as u32),
                ..Default::default()
            };
            let mut host = Binding {
                case: c,
                trace: Vec::new(),
            };
            match c["mode"].as_str().unwrap() {
                "reset" | "initialize" => {
                    for _ in 0..c["repeats"].as_u64().unwrap() {
                        rates.reset(&mut host)?;
                    }
                }
                "stationary" => rates.set_stationary(c["value"].as_u64().unwrap() as u32),
                "moving" => rates.set_moving(c["value"].as_u64().unwrap() as u32),
                "both" => rates.set_both(c["value"].as_u64().unwrap() as u32),
                _ => unreachable!(),
            }
            assert_eq!(
                json!({"values":[rates.stationary,rates.moving],"trace":host.trace}),
                case["output"],
                "{c}"
            );
        }
        let c = json!({"read":[null,null]});
        let mut host = Binding {
            case: &c,
            trace: Vec::new(),
        };
        assert!(
            Rates::default().reset(&mut host).is_err(),
            "An unknown cache must not become zero on a declined getter"
        );
        Ok(())
    }
}
