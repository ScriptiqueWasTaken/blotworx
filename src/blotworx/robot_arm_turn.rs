//! RobotArmGeneral fixed-angle turn / restore, REL5f078 and5f2cc.
//! Property callbacks are ordered and retain the rotator's current velocity.
use anyhow::Result;
pub trait Host {
    fn rotator(&mut self, entity: Option<usize>) -> Result<Option<usize>>;
    fn target(&mut self, rotator: usize, entity: Option<usize>) -> Result<()>;
    fn speed(&mut self, rotator: usize, degrees: f32) -> Result<()>;
    fn position_mode(&mut self, rotator: usize, enabled: bool) -> Result<()>;
    fn angle(&mut self, rotator: usize, radians: f32) -> Result<()>;
    fn player(&mut self) -> Result<Option<usize>>;
    fn tracking_speed(&mut self) -> Result<f32>;
}
pub fn start(entity: Option<usize>, degrees: f32, speed: f32, host: &mut impl Host) -> Result<()> {
    let Some(rotator) = host.rotator(entity)? else { return Ok(()) };
    host.target(rotator, None)?;
    host.speed(rotator, speed)?;
    host.position_mode(rotator, true)?;
    host.angle(rotator, f32::from_bits(0x3c8efa35) * degrees)
}
pub fn restore(entity: Option<usize>, host: &mut impl Host) -> Result<()> {
    let Some(rotator) = host.rotator(entity)? else { return Ok(()) };
    host.position_mode(rotator, false)?;
    let player = host.player()?;
    host.target(rotator, player)?;
    // Native reads +204, SpeedToRotateWhenTrackingPlayer, after assigning the
    // live player. The independently bound sweep-speed key is not used here.
    let speed = host.tracking_speed()?;
    host.speed(rotator, speed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_turn_and_restore_callback_order_and_float_bits_match() -> Result<()> {
        let fixture: serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/robot_arm_turn.json"))?;
        struct Target<'a> {input:&'a serde_json::Value,trace:Vec<serde_json::Value>}
        impl Host for Target<'_> {
            fn rotator(&mut self,entity:Option<usize>)->Result<Option<usize>> {self.trace.push(serde_json::json!(["rotator",entity]));Ok((entity.is_some() && self.input["rotator"].as_bool().unwrap()).then_some(7))}
            fn target(&mut self,rotator:usize,entity:Option<usize>)->Result<()> {assert_eq!(rotator,7);self.trace.push(serde_json::json!(["target",entity]));Ok(())}
            fn speed(&mut self,rotator:usize,degrees:f32)->Result<()> {assert_eq!(rotator,7);self.trace.push(serde_json::json!(["speed",degrees.to_bits()]));Ok(())}
            fn position_mode(&mut self,rotator:usize,enabled:bool)->Result<()> {assert_eq!(rotator,7);self.trace.push(serde_json::json!(["mode",enabled]));Ok(())}
            fn angle(&mut self,rotator:usize,radians:f32)->Result<()> {assert_eq!(rotator,7);self.trace.push(serde_json::json!(["angle",radians.to_bits()]));Ok(())}
            fn player(&mut self)->Result<Option<usize>> {self.trace.push(serde_json::json!(["player"]));Ok(self.input["player"].as_bool().unwrap().then_some(2))}
            fn tracking_speed(&mut self)->Result<f32> {self.trace.push(serde_json::json!(["tracking_speed"]));Ok(f32::from_bits(self.input["speed_bits"].as_u64().unwrap() as u32))}
        }
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let mut target=Target {input,trace:vec![]};
            let entity=input["entity"].as_bool().unwrap().then_some(1);
            if input["restore"].as_bool().unwrap() {restore(entity,&mut target)?;} else {
                start(entity,f32::from_bits(input["angle_bits"].as_u64().unwrap() as u32),f32::from_bits(input["speed_bits"].as_u64().unwrap() as u32),&mut target)?;
            }
            assert_eq!(serde_json::to_value(&target.trace)?,case["trace"],"{case}");
        }
        Ok(())
    }
}
