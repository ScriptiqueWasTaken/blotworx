//! PC HUD fade owner f0b320..f0b68b/f0ca80 and GameState UI producers.
//! Flash UI consumption, live camera fade color and completion receivers are
//! distinct owners. Missing receivers remain explicit, retained effects.
use anyhow::{ensure,Result};
use std::collections::{BTreeMap,VecDeque};
#[derive(Debug,Clone,Copy,serde::Serialize)]
pub(super) enum Effect {Unmute,FadeOutComplete}
#[derive(Debug,serde::Serialize)]
struct PropertyChange {property:&'static str,value:bool}
#[derive(Debug,Clone,Copy,serde::Serialize)]
pub(super) enum CameraUiChange {
    /// GameState 97cee0 writes numeric GFx value to the UI manager's d4 binding.
    Hud2D(f64),
    /// GameState 97c610 resolves the cursor and calls its a17730 visibility setter.
    CursorVisible(bool),
}
#[derive(Debug,serde::Serialize)]
pub(super) struct State {
    pub out:bool,pub forced:bool,pub entering:bool,pub elapsed:f32,pub duration:f32,
    pub guard:f32,pub deferred:i32,pub held:bool,pub suppress_hud:bool,
    pub opacity:f32,pub ui_opacity:f32,pub clear_depth:bool,
    pub hud_visible:bool,pub desired_hud:bool,pub cursor_visible:bool,pub desired_cursor:bool,
    pub cursor_only:bool,pub faded:bool,pub mode:u32,pub property_lock:bool,
    pub hud2d:bool,
    pub unmute_threshold:f32,pub unmuted:bool,pub watchdog_real:bool,
    pub loading_throbber:bool,pub pending_effect:Option<Effect>,
    properties:VecDeque<PropertyChange>,default_out:f32,
    camera_ui:VecDeque<CameraUiChange>,
}
impl State {
    pub fn new(settings:&BTreeMap<String,String>)->Result<Self> {
        // f0b550: negative explicit duration resolves key74e190ec, default1.0.
        let mut default_out=1.;
        for (name,value) in settings {
            if config_key(name)==0x74e190ec {default_out=super::config_value::decimal_float(value).map_err(anyhow::Error::msg)?;}
        }
        Ok(Self {out:false,forced:false,entering:false,elapsed:-1.,duration:0.,guard:0.,deferred:0,held:false,suppress_hud:false,
            opacity:0.,ui_opacity:0.,clear_depth:false,hud_visible:true,desired_hud:true,cursor_visible:false,desired_cursor:true,
            cursor_only:false,faded:false,mode:0xa017,property_lock:false,unmute_threshold:1.,unmuted:false,watchdog_real:false,
            hud2d:false,loading_throbber:false,pending_effect:None,properties:VecDeque::new(),default_out,camera_ui:VecDeque::new()})
    }
    pub fn set_desired_cursor(&mut self,visible:bool) { self.desired_cursor=visible; }
    pub fn set_cursor(&mut self,visible:bool) {
        if !self.property_lock { self.cursor_visible=visible; }
        // The locked path still forwards the retained visibility to the cursor.
        self.camera_ui.push_back(CameraUiChange::CursorVisible(self.cursor_visible));
    }
    pub fn set_hud2d(&mut self,active:bool) {
        self.hud2d=active;
        self.camera_ui.push_back(CameraUiChange::Hud2D(if active {1.}else{0.}));
    }
    pub fn drain_camera_ui(&mut self,mut receive:impl FnMut(CameraUiChange)->Result<()>)->Result<()> {
        while let Some(&change)=self.camera_ui.front() {
            receive(change)?;
            self.camera_ui.pop_front();
        }
        Ok(())
    }
    pub fn start_in(&mut self,duration:f32,no_hud:bool)->Result<()> {
        ensure!(duration.is_finite(),"Nonfinite PC fade duration");
        // These two owners run even if the held/forced fade rejects reversal.
        self.ui_opacity=0.;self.loading_throbber=false;
        if self.out && self.forced {return Ok(());}
        if self.deferred==0 {self.deferred=1;}
        self.suppress_hud=no_hud;
        if self.out {
            self.entering=true;self.out=false;self.forced=false;
            if duration>=0. {self.duration=duration;}
            self.elapsed=if self.elapsed>=0. {(self.duration-self.elapsed).max(0.)}else{0.};
        }Ok(())
    }
    pub fn start_out(&mut self,duration:f32)->Result<()> {
        ensure!(duration.is_finite(),"Nonfinite PC fade duration");
        if self.out && self.forced {return Ok(());}
        self.faded=true;
        if self.out {return Ok(());}
        self.clear_depth=true;self.duration=if duration>=0. {duration}else{self.default_out};
        self.out=true;self.entering=false;
        self.elapsed=if self.elapsed>=0. {(self.duration-self.elapsed).max(0.)}else{0.};Ok(())
    }
    fn visibility(&mut self,value:bool) {
        self.hud_visible=value;
        if !self.property_lock {self.properties.push_back(PropertyChange {property:"HudVisible",value});}
    }
    fn reset(&mut self) {
        self.out=false;self.forced=false;self.entering=false;self.elapsed=-1.;self.guard=0.;
        self.held=false;self.deferred=0;self.unmuted=false;self.opacity=0.;
    }
    fn effect(&mut self,effect:Effect,backend:&mut impl FnMut(Effect)->Result<()>)->Result<()> {
        self.pending_effect=Some(effect);backend(effect)?;self.pending_effect=None;Ok(())
    }
    fn complete_in(&mut self,backend:&mut impl FnMut(Effect)->Result<()>)->Result<()> {
        self.reset();self.clear_depth=false;
        if !self.suppress_hud && self.mode==0xa017 {
            self.visibility(self.desired_hud);
            if !self.property_lock {self.set_cursor(self.desired_cursor);}
        }
        self.faded=false;self.effect(Effect::Unmute,backend)
    }
    /// PC TimeManager1439468 is real delta;1439464 is game delta. This owner
    /// updates once per presentation frame, not each physics catch-up tick.
    pub fn advance(&mut self,real:f32,game:f32,transition_pending:bool,camera_alpha:f32,mut backend:impl FnMut(Effect)->Result<()>)->Result<()> {
        ensure!([real,game,camera_alpha].iter().all(|v|v.is_finite()) && real>=0. && game>=0.,"Invalid PC fade clock/color");
        ensure!(self.pending_effect.is_none(),"PC fade completion receiver is still pending");
        if self.held {return Ok(());}
        if self.deferred>0 {self.deferred-=1;return Ok(());}
        if self.entering && transition_pending {return Ok(());}
        let mut opacity=0.;
        if self.out && !self.entering {
            self.guard+=if self.watchdog_real {real}else{game};
            if self.guard>10. {
                self.ui_opacity=0.;self.guard=0.;self.entering=true;self.out=false;self.deferred=1;
                self.elapsed=-1.;self.loading_throbber=false;
            } else {opacity=1.;}
        } else {self.guard=0.;}
        if self.elapsed>=0. {
            self.elapsed+=real;
            if self.out && !self.entering {
                if self.elapsed>=self.duration {
                    self.elapsed=-1.;self.opacity=1.;self.visibility(false);
                    if self.mode!=10 && !self.property_lock {self.set_cursor(false);}
                    self.forced=false;self.effect(Effect::FadeOutComplete,&mut backend)?;
                }else{opacity=self.elapsed/self.duration;}
            }else if self.entering {
                if self.elapsed/self.duration>=self.unmute_threshold && !self.unmuted {
                    self.effect(Effect::Unmute,&mut backend)?;self.unmuted=true;
                }
                if self.elapsed>=self.duration {self.complete_in(&mut backend)?;}else{opacity=1.-self.elapsed/self.duration;}
            }
        }else if self.entering {
            if self.deferred>0 {return Ok(());}
            self.complete_in(&mut backend)?;
        }
        self.opacity=(opacity+camera_alpha).min(1.);Ok(())
    }
    pub fn call(&mut self,lua:&mlua::Lua,name:&str,args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        match name {
            "StartFadeIn"|"StartFadeInNoHUD"=>{
                let duration=match args.back() {None=>-1.,Some(value)=>lua.coerce_number(value.clone())?.unwrap_or(0.) as f32};
                self.start_in(duration,name=="StartFadeInNoHUD").map_err(mlua::Error::external)?;
            },
            "StartFadeOut"=>{
                let duration=lua.coerce_number(args.back().cloned().unwrap_or(mlua::Value::Nil))?.unwrap_or(0.) as f32;
                self.start_out(duration).map_err(mlua::Error::external)?;
            },
            "HideHud"|"ShowHud"=>{let visible=name=="ShowHud";self.desired_hud=visible;self.visibility(visible);},
            "SetCursorOnly"=>{
                self.cursor_only=pc_boolean(lua,args.back().cloned().unwrap_or(mlua::Value::Nil))?;
                self.properties.push_back(PropertyChange {property:"CursorOnly",value:self.cursor_only});
            },
            _=>return Err(mlua::Error::external("Unknown PC HUD operation")),
        }Ok(mlua::MultiValue::new())
    }
    pub fn draw(&self,ui:&egui::Ui) {
        for opacity in [self.opacity,self.ui_opacity] {
            if opacity>0. {ui.painter().with_clip_rect(ui.max_rect()).rect_filled(ui.max_rect(),0.,egui::Color32::from_black_alpha((opacity.min(1.)*255.).round() as u8));}
        }
    }
    pub fn summary(&self)->serde_json::Value {
        serde_json::json!({"fade":self,"ui_property_consumer_connected":false,"live_camera_fade_color_connected":false,
            "scope":"verified PC fade/visibility producers and black-quad submission; Flash HUD, camera color, render-click depth policy and completion receivers remain incomplete"})
    }
}
pub(super) fn pc_boolean(lua:&mlua::Lua,value:mlua::Value)->mlua::Result<bool> {
    if let Some(number)=lua.coerce_number(value.clone())? {return Ok(number as f32!=0.);}
    if let mlua::Value::String(string)=&value {
        let bytes=string.as_bytes();let bytes=bytes.split(|b|*b==0).next().unwrap_or_default();
        if bytes.eq_ignore_ascii_case(b"true"){return Ok(true);}if bytes.eq_ignore_ascii_case(b"false"){return Ok(false);}
    }Ok(!matches!(value,mlua::Value::Nil|mlua::Value::Boolean(false)))
}
fn config_key(name:&str)->u32 {
    let bytes=name.as_bytes();let mut crc=!(bytes.len() as u32);
    for byte in bytes {crc^=byte.to_ascii_lowercase() as u32;for _ in 0..8 {crc=(crc>>1)^(0u32.wrapping_sub(crc&1)&0xedb88320);}}!crc
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_hud_and_cursor_setters_match_original_pc_calls()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/em2_camera_hud_reference.json"))?;
        for c in fixture["cases"].as_array().unwrap() {
            let flag=|key:&str|c[key].as_bool().unwrap();
            let mut s=State::new(&BTreeMap::new())?;
            s.property_lock=flag("locked");s.cursor_visible=flag("initial");
            s.set_desired_cursor(flag("desired"));s.set_hud2d(flag("hud2d"));s.set_cursor(flag("request"));
            assert_eq!(s.cursor_visible,flag("cursor_visible"));assert_eq!(s.desired_cursor,flag("desired_cursor"));assert_eq!(s.hud2d,flag("hud2d_state"));
            let mut calls=Vec::new();s.drain_camera_ui(|change|{calls.push(serde_json::to_value(change)?);Ok(())})?;
            assert_eq!(serde_json::json!(calls),c["calls"]);assert!(s.camera_ui.is_empty());
        }Ok(())
    }
    #[test]
    fn camera_ui_retry_keeps_only_the_unconsumed_suffix()->Result<()> {
        let mut s=State::new(&BTreeMap::new())?;s.set_hud2d(false);s.set_cursor(true);
        let mut count=0;
        assert!(s.drain_camera_ui(|_|{count+=1;ensure!(count!=2,"Cursor not connected");Ok(())}).is_err());
        assert_eq!(s.camera_ui.len(),1);
        let mut calls=Vec::new();s.drain_camera_ui(|change|{calls.push(change);Ok(())})?;
        assert!(matches!(calls.as_slice(),[CameraUiChange::CursorVisible(true)]));Ok(())
    }
    #[test]
    fn fade_in_setter_matches_original_pc_instructions_bit_for_bit()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/em2_fade_in_reference.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s=State::new(&BTreeMap::new())?;
            s.out=case["out"]==1;s.forced=case["forced"]==1;s.entering=case["entering"]==1;s.suppress_hud=case["no_hud"]==1;
            s.elapsed=case["elapsed"].as_f64().unwrap() as f32;s.duration=case["duration"].as_f64().unwrap() as f32;
            s.deferred=case["deferred"].as_i64().unwrap_or(0) as i32;s.ui_opacity=0.7;s.loading_throbber=true;
            s.start_in(case["request_duration"].as_f64().unwrap() as f32,case["request_no_hud"]==1)?;
            let expected=&case["reference"];
            assert_eq!(serde_json::json!({"out":s.out,"forced":s.forced,"entering":s.entering,"no_hud":s.suppress_hud,"deferred":s.deferred,
                "elapsed_bits":s.elapsed.to_bits(),"duration_bits":s.duration.to_bits()}),serde_json::json!({"out":expected["out"],"forced":expected["forced"],"entering":expected["entering"],"no_hud":expected["no_hud"],"deferred":expected["deferred"],"elapsed_bits":expected["elapsed_bits"],"duration_bits":expected["duration_bits"]}),"{}",case["name"]);
            assert_eq!(s.ui_opacity,0.);assert!(!s.loading_throbber);
        }Ok(())
    }
    #[test]
    fn pc_real_clock_interpolation_delay_and_duplicate_unmute_order()->Result<()> {
        let mut s=State::new(&BTreeMap::new())?;s.start_out(1.)?;
        s.advance(0.25,0.,false,0.,|_|Ok(()))?;assert_eq!(s.opacity,0.25);assert_eq!(s.guard,0.);
        s.advance(0.25,0.,false,0.,|_|Ok(()))?;assert_eq!(s.opacity,0.5);
        s.start_in(0.5,false)?;assert_eq!(s.elapsed,0.);assert_eq!(s.deferred,1);
        s.advance(0.25,0.,false,0.,|_|Ok(()))?;assert_eq!(s.opacity,0.5);assert_eq!(s.elapsed,0.);
        s.advance(0.25,0.,true,0.,|_|Ok(()))?;assert_eq!(s.elapsed,0.);
        s.advance(0.25,0.,false,0.,|_|Ok(()))?;assert_eq!(s.opacity,0.5);
        let mut effects=Vec::new();s.advance(0.25,0.,false,0.,|e|{effects.push(e);Ok(())})?;
        assert_eq!(effects.len(),2);assert!(effects.iter().all(|e|matches!(e,Effect::Unmute)));
        assert_eq!(s.opacity,0.);assert!(!s.clear_depth);assert!(s.hud_visible && s.cursor_visible);assert!(!s.faded);Ok(())
    }
    #[test]
    fn missing_receiver_retains_effect_and_preceding_native_side_effects()->Result<()> {
        let mut s=State::new(&BTreeMap::new())?;s.start_out(0.)?;
        assert!(s.advance(0.1,0.1,false,0.,|_|anyhow::bail!("Receiver missing")).is_err());
        assert!(matches!(s.pending_effect,Some(Effect::FadeOutComplete)));assert_eq!(s.opacity,1.);
        assert!(!s.hud_visible && !s.cursor_visible);assert_eq!(s.elapsed,-1.);Ok(())
    }
    #[test]
    fn pc_boolean_uses_single_precision_and_visibility_keeps_producer_order()->Result<()> {
        let lua=mlua::Lua::new();let mut s=State::new(&BTreeMap::new())?;
        for (value,expected) in [(mlua::Value::Number(1e-100),false),(mlua::Value::Number(0.5),true),(mlua::Value::String(lua.create_string("false")?),false)] {
            s.call(&lua,"SetCursorOnly",mlua::MultiValue::from_vec(vec![value]))?;assert_eq!(s.cursor_only,expected);
        }
        s.call(&lua,"HideHud",mlua::MultiValue::new())?;assert!(!s.hud_visible && !s.desired_hud);
        assert_eq!(s.properties.len(),4);assert_eq!(s.properties.back().unwrap().property,"HudVisible");Ok(())
    }
}
