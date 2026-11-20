//! RobotArmGeneral virtual54, retail5d148..5d738. Common child/timer update
//! must call this only after its own gates. All service callbacks are required.
use anyhow::Result;
pub trait Host {
    fn phase(&self)->Result<u32>;
    fn set_phase(&mut self,value:u32)->Result<()>;
    fn smash_type(&self)->Result<i32>;
    fn smash_angle(&self)->Result<f32>;
    fn clear_smash(&mut self)->Result<()>;
    fn electro_type(&self)->Result<i32>;
    fn clear_electro_type(&mut self)->Result<()>;
    fn hold_electrocution(&self)->Result<bool>;
    fn current_attack(&self)->Result<i32>;
    fn set_current_attack(&mut self,value:i32)->Result<()>;
    fn angle(&self,kind:i32)->Result<f32>;
    fn delay(&self,kind:i32)->Result<f32>;
    fn cooldown(&self,kind:i32)->Result<f32>;
    fn set_timer(&mut self,value:f32)->Result<()>;
    fn can_attack(&mut self)->Result<bool>;
    fn choose_attack(&mut self)->Result<i32>;
    fn root_enabled(&mut self,value:bool)->Result<()>;
    fn turn(&mut self,degrees:f32,speed:f32)->Result<()>;
    fn tracking_speed(&mut self)->Result<f32>;
    fn sweep_speed(&mut self)->Result<f32>;
    fn player_angle(&mut self)->Result<f32>;
    fn lock_eyes(&mut self)->Result<()>;
    fn reset_attack_clock(&mut self)->Result<()>;
    fn attack_elapsed(&mut self)->Result<f32>;
    fn spotlight(&mut self,value:i32)->Result<()>;
    fn has_animation(&mut self)->Result<bool>;
    fn attack_variable(&mut self,value:i32)->Result<()>;
    fn animation_wait(&mut self,start:u32,done:u32,timeout:f32,wait:bool)->Result<()>;
    fn scripted_audio(&mut self)->Result<()>;
    fn finish_smash(&mut self)->Result<()>;
    fn break_impact(&mut self)->Result<()>;
    fn has_child(&self)->Result<bool>;
    fn restore_turn(&mut self)->Result<()>;
}
pub fn update(host:&mut impl Host)->Result<u32> {
    match host.phase()? {
        0=>{
            if host.electro_type()? != -1 {host.set_phase(9)?;return Ok(1);}
            if host.smash_type()? != -1 {
                host.root_enabled(true)?;
                let angle=host.smash_angle()?;
                host.turn(angle,180.)?;
                host.set_timer(1.1)?;
                host.set_phase(4)?;
                host.lock_eyes()?;
                return Ok(1);
            }
            if !host.can_attack()? {return Ok(1);}
            let kind=host.choose_attack()?;
            host.set_current_attack(kind)?;
            if kind<0 {return Ok(1);}
            host.spotlight(2)?;
            let kind=host.current_attack()?;
            let angle=host.angle(kind)?;
            host.root_enabled(true)?;
            if angle<0. {
                let speed=host.tracking_speed()?;
                let angle=host.player_angle()?;
                host.turn(angle,speed)?;
            } else {
                let speed=host.sweep_speed()?;
                let kind=host.current_attack()?;
                let angle=host.angle(kind)?;
                host.turn(angle,speed)?;
            }
            host.set_phase(2)?;
            host.reset_attack_clock()?;
            let kind=host.current_attack()?;
            let delay=host.delay(kind)?;
            host.set_timer(delay)?;
        }
        1=>host.set_phase(3)?,
        2=>{
            if host.current_attack()? == -1 {host.set_phase(3)?;return Ok(1);}
            let elapsed=host.attack_elapsed()?;
            let kind=host.current_attack()?;
            if elapsed>=host.delay(kind)? {
                if host.has_animation()? {
                    let kind=host.current_attack()?;
                    host.attack_variable(kind)?;
                    host.animation_wait(0x66,0xc1,10.,true)?;
                    host.spotlight(0)?;
                    host.set_phase(1)?;
                } else {host.set_phase(3)?;}
            }
        }
        3=>{
            host.spotlight(1)?;
            host.restore_turn()?;
            host.root_enabled(true)?;
            if host.electro_type()? == -1 && host.smash_type()? == -1 {
                let kind=host.current_attack()?;
                let cooldown=if kind != -1 {host.cooldown(kind)?}else{1.};
                host.set_timer(cooldown)?;
            }
            host.set_phase(0)?;
        }
        4=>{
            if host.has_animation()? {
                let kind=host.smash_type()?;
                host.attack_variable(kind)?;
                host.animation_wait(0x74,0xc1,60.,false)?;
                host.scripted_audio()?;
                host.set_timer(3.)?;
                host.set_phase(5)?;
            } else {host.set_phase(8)?;}
        }
        5=>host.finish_smash()?,
        6=>{host.break_impact()?;host.set_phase(7)?;}
        7=>if !host.has_child()? {host.set_phase(8)?;},
        8=>{host.clear_smash()?;host.restore_turn()?;host.set_phase(0)?;}
        9=>{
            host.spotlight(0)?;
            if host.has_animation()? {
                let kind=host.electro_type()?;
                host.attack_variable(kind)?;
                host.clear_electro_type()?;
                host.animation_wait(0xab,0xc1,10.,true)?;
                if host.hold_electrocution()? {host.root_enabled(false)?;host.set_phase(11)?;}
                else {host.set_phase(10)?;}
            } else {host.set_phase(10)?;}
        }
        10=>{host.restore_turn()?;host.set_phase(0)?;}
        _=>{},
    }
    Ok(1)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Target<'a> {input:&'a Value,phase:u32,timer:f32,current:i32,smash:i32,electro:i32,child:bool,trace:Vec<Value>}
    impl Target<'_> {
        fn i(&self,key:&str)->i32 {self.input[key].as_i64().unwrap() as i32}
        fn b(&self,key:&str)->bool {self.input[key].as_bool().unwrap()}
        fn f(&self,key:&str)->f32 {self.input[key].as_f64().unwrap() as f32}
    }
    impl Host for Target<'_> {
        fn phase(&self)->Result<u32> {Ok(self.phase)}
        fn set_phase(&mut self,value:u32)->Result<()> {self.phase=value;Ok(())}
        fn smash_type(&self)->Result<i32> {Ok(self.smash)}
        fn smash_angle(&self)->Result<f32> {Ok(44.)}
        fn clear_smash(&mut self)->Result<()> {self.smash=-1;Ok(())}
        fn electro_type(&self)->Result<i32> {Ok(self.electro)}
        fn clear_electro_type(&mut self)->Result<()> {self.electro=-1;Ok(())}
        fn hold_electrocution(&self)->Result<bool> {Ok(self.b("hold"))}
        fn current_attack(&self)->Result<i32> {Ok(self.current)}
        fn set_current_attack(&mut self,value:i32)->Result<()> {self.current=value;Ok(())}
        fn angle(&self,_:i32)->Result<f32> {Ok(self.f("angle"))}
        fn delay(&self,_:i32)->Result<f32> {Ok(0.5)}
        fn cooldown(&self,_:i32)->Result<f32> {Ok(1.25)}
        fn set_timer(&mut self,value:f32)->Result<()> {self.timer=value;Ok(())}
        fn can_attack(&mut self)->Result<bool> {Ok(self.b("can"))}
        fn choose_attack(&mut self)->Result<i32> {Ok(self.i("chosen"))}
        fn root_enabled(&mut self,value:bool)->Result<()> {self.trace.push(json!(["enabled",value]));Ok(())}
        fn turn(&mut self,degrees:f32,speed:f32)->Result<()> {self.trace.push(json!(["turn",degrees.to_bits(),speed.to_bits()]));Ok(())}
        fn tracking_speed(&mut self)->Result<f32> {Ok(15.)}
        fn sweep_speed(&mut self)->Result<f32> {Ok(30.)}
        fn player_angle(&mut self)->Result<f32> {Ok(73.)}
        fn lock_eyes(&mut self)->Result<()> {self.trace.push(json!(["lock"]));Ok(())}
        fn reset_attack_clock(&mut self)->Result<()> {self.trace.push(json!(["reset_clock"]));Ok(())}
        fn attack_elapsed(&mut self)->Result<f32> {Ok(self.f("elapsed"))}
        fn spotlight(&mut self,value:i32)->Result<()> {self.trace.push(json!(["spotlight",value]));Ok(())}
        fn has_animation(&mut self)->Result<bool> {Ok(self.b("animation"))}
        fn attack_variable(&mut self,value:i32)->Result<()> {self.trace.push(json!(["variable",value]));Ok(())}
        fn animation_wait(&mut self,start:u32,done:u32,timeout:f32,wait:bool)->Result<()> {self.trace.push(json!(["wait",start,done,timeout.to_bits(),wait]));self.child=true;Ok(())}
        fn scripted_audio(&mut self)->Result<()> {self.trace.push(json!(["audio"]));Ok(())}
        fn finish_smash(&mut self)->Result<()> {self.trace.push(json!(["finish"]));self.phase=if self.b("destination") {6}else{7};self.timer=0.;Ok(())}
        fn break_impact(&mut self)->Result<()> {self.trace.push(json!(["shake",4f32.to_bits(),5f32.to_bits(),0.05f32.to_bits(),4f32.to_bits(),0.02f32.to_bits(),0.045f32.to_bits()]));self.trace.push(json!(["rumble",0,0.5f32.to_bits()]));Ok(())}
        fn has_child(&self)->Result<bool> {Ok(self.child)}
        fn restore_turn(&mut self)->Result<()> {self.trace.push(json!(["restore"]));Ok(())}
    }
    #[test]
    fn all_arm_phases_match_original_instructions() -> Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/robot_arm_behavior.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let i=|key:&str|input[key].as_i64().unwrap() as i32;
            let mut host=Target {input,phase:i("phase") as u32,timer:0.,current:i("current"),smash:i("smash"),electro:i("electro"),child:input["child"].as_bool().unwrap(),trace:vec![]};
            assert_eq!(update(&mut host)?,1,"{case}");
            assert_eq!(json!(host.trace),case["trace"],"{case}");
            for (key,value) in [("phase",json!(host.phase)),("timer_bits",json!(host.timer.to_bits())),("current",json!(host.current)),("smash",json!(host.smash)),("electro",json!(host.electro))] {assert_eq!(value,case[key],"{key}: {case}");}
        }
        Ok(())
    }
}
