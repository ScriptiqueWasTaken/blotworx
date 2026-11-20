//! RobotArmGeneral spotlight transition/publication, retail REL5f7d8..5fa54.
//! Resource lookup, transforms, audio and render objects remain host-owned.
use anyhow::Result;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct State {
    #[serde(rename = "spotlight_status")]
    pub status: i32,
    #[serde(rename = "spotlight_disabled")]
    pub disabled: bool,
}
impl State {
    /// Commit requested state even if the effective state is unchanged or its
    /// referenced entity is absent. Enabling restores the retained status.
    pub fn transition(&mut self, status: i32, disabled: bool) -> Option<i32> {
        let before = if self.disabled { 0 } else { self.status };
        let after = if disabled { 0 } else { status };
        self.status = status;
        self.disabled = disabled;
        (before != after).then_some(after)
    }
}
pub trait Host {
    fn resolve(&mut self) -> Result<Option<usize>>;
    fn parent(&mut self, entity: usize) -> Result<Option<usize>>;
    fn rotator_enabled(&mut self, entity: Option<usize>, enabled: bool) -> Result<()>;
    fn hide_resolved(&mut self, hidden: bool) -> Result<()>;
    fn audio(&mut self, entity: usize, event: &str) -> Result<()>;
    fn color(&mut self, entity: usize, slot: i32, color: [f32; 4]) -> Result<()>;
}
/// Called only after transition commits. The second entity resolution for
/// visibility is intentional; color/audio retain the initially resolved owner.
pub fn publish(effective: i32, host: &mut impl Host) -> Result<()> {
    let Some(entity) = host.resolve()? else { return Ok(()) };
    let parent = host.parent(entity)?;
    let grandparent = match parent { Some(parent) => host.parent(parent)?, None => None };
    if matches!(effective, 0..=2) {
        host.rotator_enabled(parent, effective != 2)?;
        host.rotator_enabled(grandparent, effective != 2)?;
        host.hide_resolved(effective == 0)?;
        host.audio(entity, match effective {
            0 => "Play_sfx_RobotArm_Light_Off",
            1 => "Play_sfx_RobotArm_Light_White",
            _ => "Play_sfx_RobotArm_Light_Red",
        })?;
        if effective == 0 { return Ok(()) }
    }
    host.color(entity, 1, if effective == 2 { [1., 0., 0., 1.] } else { [0.5; 4] })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_spotlight_transition_and_publication_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/robot_arm_spotlight.json"))?;
        struct Target { exists: bool, model: bool, parents: usize, trace: Vec<serde_json::Value> }
        impl Host for Target {
            fn resolve(&mut self) -> Result<Option<usize>> {self.trace.push(serde_json::json!(["resolve"])); Ok(self.exists.then_some(1))}
            fn parent(&mut self, entity: usize) -> Result<Option<usize>> {self.trace.push(serde_json::json!(["parent",entity])); Ok((entity <= self.parents).then_some(entity+1))}
            fn rotator_enabled(&mut self, entity: Option<usize>, enabled: bool) -> Result<()> {self.trace.push(serde_json::json!(["rotator",entity,enabled])); Ok(())}
            fn hide_resolved(&mut self, hidden: bool) -> Result<()> {let entity=self.resolve()?;self.trace.push(serde_json::json!(["hide",entity,hidden])); Ok(())}
            fn audio(&mut self, entity: usize, event: &str) -> Result<()> {self.trace.push(serde_json::json!(["audio",entity,event])); Ok(())}
            fn color(&mut self, entity: usize, slot: i32, color: [f32;4]) -> Result<()> {self.trace.push(serde_json::json!(["model",entity]));if self.model {self.trace.push(serde_json::json!(["color",entity,slot,color]));} Ok(())}
        }
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let mut state=State {status:input["before_status"].as_i64().unwrap() as i32,disabled:input["before_disabled"].as_bool().unwrap()};
            let mut target=Target {exists:input["exists"].as_bool().unwrap(),model:input["model"].as_bool().unwrap(),parents:input["parents"].as_u64().unwrap() as usize,trace:vec![]};
            if let Some(effective)=state.transition(input["status"].as_i64().unwrap() as i32,input["disabled"].as_bool().unwrap()) {
                publish(effective,&mut target)?;
            }
            assert_eq!(serde_json::to_value(&target.trace)?,case["trace"],"{case}");
            assert_eq!(state.status,input["status"].as_i64().unwrap() as i32);
            assert_eq!(state.disabled,input["disabled"].as_bool().unwrap());
        }
        Ok(())
    }
}
