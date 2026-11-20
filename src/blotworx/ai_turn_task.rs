//! Concrete turning task, REL197a60/197acc/197b28 and197b98/197c94.
//! Controller callbacks are explicit: an unimplemented agent is not absence.
use anyhow::{Context as _, Result};

pub trait Host {
    fn resolve_position(&mut self, entity: usize) -> Result<Option<[f32; 4]>>;
    fn owner_position(&mut self, owner: usize) -> Result<[f32; 4]>;
    /// Actual transformation forward axis (native297110 selector2), not intent.
    fn owner_facing(&mut self, owner: usize) -> Result<[f32; 3]>;
    fn has_agent(&mut self, owner: usize) -> Result<bool>;
    fn agent_heading_degrees(&mut self, owner: usize, heading: f32) -> Result<()>;
    fn has_basic_controller(&mut self, owner: usize) -> Result<bool>;
    fn basic_facing(&mut self, owner: usize, facing: [f32; 3]) -> Result<()>;
    fn stop_basic_controller(&mut self, owner: usize) -> Result<()>;
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Turn {
    pub reference: Option<usize>,
    pub point: Option<[f32; 4]>,
    pub angle: f32,
    pub kind: u32,
}
impl Turn {
    pub fn toward_point(point: [f32; 4]) -> Self {
        Self {
            reference: None,
            point: Some(point),
            angle: f32::MAX,
            kind: 0,
        }
    }
    pub fn toward_entity(reference: Option<usize>) -> Self {
        //197acc does not initialize point storage. Preserve that distinction.
        Self {
            reference,
            point: None,
            angle: f32::MAX,
            kind: 0,
        }
    }
    pub fn toward_angle(angle: f32) -> Self {
        Self {
            reference: None,
            point: None,
            angle,
            kind: 0,
        }
    }
    fn submit(&self, owner: usize, host: &mut impl Host) -> Result<Option<f32>> {
        let target = if let Some(reference) = self.reference {
            let Some(point) = host.resolve_position(reference)? else {
                return Ok(None);
            };
            Some(point)
        } else if self.angle != f32::MAX {
            None
        } else {
            Some(
                self.point
                    .context("Native turn point storage was never initialized")?,
            )
        };
        let angle = if let Some(point) = target {
            let origin = host.owner_position(owner)?;
            let x = point[0] - origin[0];
            let z = point[2] - origin[2];
            super::native_angle::atan2(x as f64, z as f64) as f32
        } else {
            self.angle
        };
        if host.has_agent(owner)? {
            host.agent_heading_degrees(owner, f32::from_bits(0x42652ee0) * angle)?;
        } else if host.has_basic_controller(owner)? {
            let (sin, cos) = super::native_angle::sin_cos_medium(angle as f64)?;
            host.basic_facing(owner, [sin as f32, 0., cos as f32])?;
        }
        Ok(Some(angle))
    }
    pub fn activate(&self, owner: usize, host: &mut impl Host) -> Result<()> {
        self.submit(owner, host).map(|_| ())
    }
    pub fn stop(&self, owner: usize, host: &mut impl Host) -> Result<()> {
        if host.has_basic_controller(owner)? {
            host.stop_basic_controller(owner)?;
        }
        Ok(())
    }
    /// Query completion without taking the owned task out of its brain while
    /// controller callbacks execute. None preserves its existing kind word.
    pub fn completion(&self, owner: usize, host: &mut impl Host) -> Result<Option<u32>> {
        let Some(desired) = self.submit(owner, host)? else {
            return Ok(Some(2));
        };
        let facing = host.owner_facing(owner)?;
        let current = super::native_angle::atan2(facing[0] as f64, facing[2] as f64) as f32;
        //297cd0 adds4pi before double fmod and rounds back to single. Do not
        // replace it with rem_euclid: negative/out-of-domain inputs differ.
        let offset = (current - desired) + 12.566370964050293_f32;
        let mut difference = ((offset as f64) % (6.2831854820251465_f32 as f64)) as f32;
        if difference > std::f32::consts::PI {
            difference -= std::f32::consts::TAU;
        }
        if difference.abs() < 0.08_f32 {
            Ok(Some(1))
        } else {
            Ok(None)
        }
    }
    pub fn update(&mut self, owner: usize, host: &mut impl Host) -> Result<u32> {
        if let Some(kind) = self.completion(owner, host)? {
            self.kind = kind;
            Ok(1)
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture<'a> {
        input: &'a serde_json::Value,
        trace: Vec<serde_json::Value>,
    }
    impl Fixture<'_> {
        fn vector<const N: usize>(&self, field: &str, default: [f32; N]) -> [f32; N] {
            self.input[field]
                .as_array()
                .map(|values| std::array::from_fn(|i| values[i].as_f64().unwrap() as f32))
                .unwrap_or(default)
        }
    }
    impl Host for Fixture<'_> {
        fn resolve_position(&mut self, entity: usize) -> Result<Option<[f32; 4]>> {
            assert_eq!(entity, 42);
            self.trace.push(serde_json::json!(["resolve", 42]));
            Ok(self.input["found"].as_bool().unwrap_or(true).then(|| {
                self.trace.push(serde_json::json!(["position", "target"]));
                self.vector("point", [2., 2., 3., 0.])
            }))
        }
        fn owner_position(&mut self, owner: usize) -> Result<[f32; 4]> {
            assert_eq!(owner, 7);
            self.trace.push(serde_json::json!(["position", "owner"]));
            Ok([1., 2., 3., 0.])
        }
        fn owner_facing(&mut self, owner: usize) -> Result<[f32; 3]> {
            assert_eq!(owner, 7);
            self.trace.push(serde_json::json!(["facing"]));
            Ok(self.vector("facing", [0., 0., 1.]))
        }
        fn has_agent(&mut self, owner: usize) -> Result<bool> {
            assert_eq!(owner, 7);
            let present = self.input["agent"].as_bool().unwrap();
            self.trace.push(serde_json::json!(["agent", present]));
            Ok(present)
        }
        fn agent_heading_degrees(&mut self, owner: usize, heading: f32) -> Result<()> {
            assert_eq!(owner, 7);
            self.trace
                .push(serde_json::json!(["heading", heading.to_bits()]));
            Ok(())
        }
        fn has_basic_controller(&mut self, owner: usize) -> Result<bool> {
            assert_eq!(owner, 7);
            let present = self.input["basic"].as_bool().unwrap();
            self.trace.push(serde_json::json!(["basic", present]));
            Ok(present)
        }
        fn basic_facing(&mut self, owner: usize, facing: [f32; 3]) -> Result<()> {
            assert_eq!(owner, 7);
            self.trace
                .push(serde_json::json!(["direction", facing.map(f32::to_bits)]));
            Ok(())
        }
        fn stop_basic_controller(&mut self, owner: usize) -> Result<()> {
            assert_eq!(owner, 7);
            self.trace.push(serde_json::json!(["stop"]));
            Ok(())
        }
    }
    #[test]
    fn concrete_turn_task_matches_original_dispatch_order_angle_words_and_completion() -> Result<()>
    {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_turn_task.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            let mut host = Fixture {
                input,
                trace: vec![],
            };
            let mut task = match input["target"].as_str().unwrap() {
                "entity" => Turn::toward_entity(Some(42)),
                "angle" => {
                    Turn::toward_angle(f32::from_bits(input["angle"].as_u64().unwrap() as u32))
                }
                _ => Turn::toward_point(host.vector("point", [2., 2., 3., 0.])),
            };
            let result = match input["op"].as_str().unwrap() {
                "update" => Some(task.update(7, &mut host)?),
                "activate" => {
                    task.activate(7, &mut host)?;
                    None
                }
                _ => {
                    task.stop(7, &mut host)?;
                    None
                }
            };
            assert_eq!(serde_json::to_value(host.trace)?, case["trace"], "{input}");
            assert_eq!(task.kind, case["kind"].as_u64().unwrap() as u32, "{input}");
            assert_eq!(result, case["result"].as_u64().map(|v| v as u32), "{input}");
        }
        Ok(())
    }
}
