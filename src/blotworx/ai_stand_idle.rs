//! StandIdle's concrete virtuals, retail REL1b1058..1b10b4.
//! Entry belongs to behavior selection, never to a forced cutscene reset.
use anyhow::Result;

pub trait Host {
    /// Resolve the brain's animation-component binding through the native
    /// component lookup. An absent controller is a valid result.
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn send_animation_event(&mut self, controller: usize, event: u32) -> Result<()>;
}

#[derive(Default, Debug)]
pub struct StandIdle;
impl StandIdle {
    pub fn score(&self) -> i32 {
        1
    }
    pub fn eligible(&self) -> bool {
        true
    }
    pub fn replace_at_equal_priority(&self) -> bool {
        false
    }
    pub fn yield_at_equal_priority(&self) -> bool {
        false
    }
    pub fn restart_current(&self) -> bool {
        false
    }
    pub fn update_result(&self) -> u32 {
        1
    }
    pub fn enter(&self, owner: usize, host: &mut impl Host) -> Result<()> {
        if let Some(controller) = host.animation_controller(owner)? {
            host.send_animation_event(controller, 0xc2)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stand_idle_virtuals_and_entry_match_original_controller_resolution() -> Result<()> {
        struct Binding {
            controller: Option<usize>,
            trace: Vec<serde_json::Value>,
        }
        impl Host for Binding {
            fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>> {
                self.trace.push(serde_json::json!(["resolve", owner]));
                Ok(self.controller)
            }
            fn send_animation_event(&mut self, controller: usize, event: u32) -> Result<()> {
                self.trace
                    .push(serde_json::json!(["animation", controller, event]));
                Ok(())
            }
        }
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_stand_idle.json"
        ))?;
        let idle = StandIdle;
        assert_eq!(
            serde_json::json!([
                idle.score(),
                i32::from(idle.eligible()),
                i32::from(idle.replace_at_equal_priority()),
                i32::from(idle.yield_at_equal_priority()),
                i32::from(idle.restart_current()),
                idle.update_result() as i32
            ]),
            fixture["virtuals"]
        );
        for case in fixture["cases"].as_array().unwrap() {
            let owner = case["owner"].as_u64().unwrap() as usize;
            let mut host = Binding {
                controller: case["controller"].as_u64().map(|v| v as usize),
                trace: Vec::new(),
            };
            idle.enter(owner, &mut host)?;
            assert_eq!(serde_json::json!(host.trace), case["trace"]);
        }
        // Lookup or dispatch errors must not turn into a successful idle entry.
        struct Broken;
        impl Host for Broken {
            fn animation_controller(&mut self, _: usize) -> Result<Option<usize>> {
                anyhow::bail!("unbound controller")
            }
            fn send_animation_event(&mut self, _: usize, _: u32) -> Result<()> {
                panic!("lookup failure must stop dispatch")
            }
        }
        assert!(idle.enter(0, &mut Broken).is_err());
        Ok(())
    }
}
