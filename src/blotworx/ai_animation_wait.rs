//! Animation task1986c0/19874c/1988a8/1988d4. Controller/agent services are
//! mandatory boundaries. A missing portable agent is not native absence.
use anyhow::Result;
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Definition {
    pub start_event: u32,
    pub done_event: u32,
    pub timeout: f32,
    pub apply_heading: bool,
    pub heading: f32,
    pub agent_40: bool,
}
pub trait Host {
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn send_animation_event(&mut self, controller: usize, event: u32) -> Result<()>;
    fn subscribe(&mut self, owner: usize, kind: u16, any_source: bool) -> Result<()>;
    fn has_agent(&mut self, owner: usize) -> Result<bool>;
    fn agent(&mut self, owner: usize) -> Result<usize>;
    fn agent_heading(&mut self, agent: usize, degrees: f32) -> Result<()>;
    fn has_basic_controller(&mut self, owner: usize) -> Result<bool>;
    fn basic_facing(&mut self, owner: usize, facing: [f32; 3]) -> Result<()>;
    fn agent_40(&mut self, agent: usize) -> Result<()>;
}
impl Definition {
    /// Activation does not reset elapsed/completion, including reactivation.
    pub fn activate(self, owner: usize, host: &mut impl Host) -> Result<()> {
        if let Some(controller) = host.animation_controller(owner)? {
            host.send_animation_event(controller, self.start_event)?;
        }
        host.subscribe(owner, 9, false)?;
        if self.apply_heading {
            if host.has_agent(owner)? {
                let agent = host.agent(owner)?;
                host.agent_heading(agent, f32::from_bits(0x42652ee0) * self.heading)?;
            } else if host.has_basic_controller(owner)? {
                let (sin, cos) = super::native_angle::sin_cos_medium(self.heading as f64)?;
                host.basic_facing(owner, [sin as f32, 0., cos as f32])?;
            }
        }
        if self.agent_40 && host.has_agent(owner)? {
            let agent = host.agent(owner)?;
            host.agent_40(agent)?;
        }
        Ok(())
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Wait {
    pub definition: Definition,
    pub elapsed: f32,
    pub completed: bool,
    pub kind: u32,
}
impl Wait {
    pub fn new(definition: Definition) -> Self {
        Self {
            definition,
            elapsed: 0.,
            completed: false,
            kind: 0,
        }
    }
    /// Subscription/source admission is the base1979e4 wrapper's job.
    pub fn receive(&mut self, event_kind: u16, id: u32) {
        if event_kind == 9 && id == self.definition.done_event {
            self.finish(true);
        }
    }
    pub fn finish(&mut self, success: bool) {
        self.completed = true;
        self.kind = if success { 1 } else { 2 };
    }
    pub fn update(&mut self, delta: f32) -> u32 {
        self.elapsed += delta;
        // Timeout is strict and is a successful completion in retail.
        if self.elapsed > self.definition.timeout {
            self.finish(true);
        }
        u32::from(self.completed)
    }
    /// Native1988a4 stop is empty; subscription storage dies with the task.
    pub fn stop(&self) {}
}
#[derive(serde::Serialize)]
pub struct Owned {
    pub task: Wait,
    pub subscribed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Fixture<'a> {
        input: &'a Value,
        trace: Vec<Value>,
    }
    impl Host for Fixture<'_> {
        fn animation_controller(&mut self, o: usize) -> Result<Option<usize>> {
            assert_eq!(o, 7);
            let present = self.input["animation"].as_bool().unwrap();
            self.trace.push(json!(["animation", present]));
            Ok(present.then_some(100))
        }
        fn send_animation_event(&mut self, c: usize, id: u32) -> Result<()> {
            assert_eq!(c, 100);
            self.trace.push(json!(["send", id]));
            Ok(())
        }
        fn subscribe(&mut self, o: usize, k: u16, any: bool) -> Result<()> {
            assert_eq!(o, 7);
            self.trace.push(json!(["subscribe", k, any]));
            Ok(())
        }
        fn has_agent(&mut self, o: usize) -> Result<bool> {
            assert_eq!(o, 7);
            let present = self.input["agent"].as_bool().unwrap();
            self.trace.push(json!(["has_agent", present]));
            Ok(present)
        }
        fn agent(&mut self, o: usize) -> Result<usize> {
            assert_eq!(o, 7);
            self.trace.push(json!(["agent"]));
            Ok(200)
        }
        fn agent_heading(&mut self, a: usize, v: f32) -> Result<()> {
            assert_eq!(a, 200);
            self.trace.push(json!(["heading", v.to_bits()]));
            Ok(())
        }
        fn has_basic_controller(&mut self, o: usize) -> Result<bool> {
            assert_eq!(o, 7);
            let present = self.input["basic"].as_bool().unwrap();
            self.trace.push(json!(["basic", present]));
            Ok(present)
        }
        fn basic_facing(&mut self, o: usize, v: [f32; 3]) -> Result<()> {
            assert_eq!(o, 7);
            self.trace.push(json!(["facing", v.map(f32::to_bits)]));
            Ok(())
        }
        fn agent_40(&mut self, a: usize) -> Result<()> {
            assert_eq!(a, 200);
            self.trace.push(json!(["agent40"]));
            Ok(())
        }
    }
    #[test]
    fn task_state_and_activation_order_match_original_instructions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_animation_wait.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let bits = |name: &str| f32::from_bits(c[name].as_u64().unwrap() as u32);
            let flag = |name: &str| c[name].as_bool().unwrap();
            let definition = Definition {
                start_event: 0x71,
                done_event: 0x72,
                timeout: bits("timeout"),
                apply_heading: flag("apply_heading"),
                heading: bits("heading"),
                agent_40: flag("agent_40"),
            };
            let mut state = Wait::new(definition);
            let mut host = Fixture {
                input: c,
                trace: vec![],
            };
            let op = c["operation"].as_str().unwrap();
            if op != "construct" {
                state.elapsed = bits("elapsed");
                state.completed = flag("completed");
                state.kind = c["kind"].as_u64().unwrap() as u32;
            }
            let returned = match op {
                "construct" => None,
                "activate" => {
                    state.definition.activate(7, &mut host)?;
                    None
                }
                "update" => Some(state.update(bits("delta"))),
                "receive" => {
                    state.receive(
                        c["event_kind"].as_u64().unwrap() as u16,
                        c["event_id"].as_u64().unwrap() as u32,
                    );
                    None
                }
                "stop" => {
                    state.stop();
                    None
                }
                _ => unreachable!(),
            };
            assert_eq!(json!(host.trace), case["trace"], "{c}");
            assert_eq!(state.kind as u64, case["kind"].as_u64().unwrap(), "{c}");
            assert_eq!(
                state.elapsed.to_bits() as u64,
                case["elapsed"].as_u64().unwrap(),
                "{c}"
            );
            assert_eq!(json!(state.completed), case["completed"], "{c}");
            assert_eq!(json!(returned), case["returned"], "{c}");
        }
        Ok(())
    }
}
