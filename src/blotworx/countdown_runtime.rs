use super::{assets::World, countdown::State};
use anyhow::{Context, Result};
use mlua::{Lua, MultiValue, Value};
fn configured(world: &World, key: &str, default: f32) -> Result<f32> {
    let value = world
        .runtime_settings
        .get(key)
        .map(|value| value.parse::<f32>())
        .transpose()
        .with_context(|| format!("Invalid {key}"))?
        .unwrap_or(default);
    anyhow::ensure!(value.is_finite(), "Nonfinite {key}");
    Ok(value)
}
fn scalar(lua: &Lua, value: Value) -> mlua::Result<f32> {
    let value = lua.coerce_number(value)?.unwrap_or(0.) as f32;
    if !value.is_finite() || value < i32::MIN as f32 || value >= 2147483648. {
        return Err(mlua::Error::external(
            "Timer value exceeds verified finite display conversion range",
        ));
    }
    Ok(value)
}
impl World {
    pub(super) fn initialize_countdown(&mut self) -> Result<()> {
        self.countdown.mid = configured(self, "TimerMidSFX", 10.)?;
        self.countdown.late = configured(self, "TimerLateSFX", 5.)?;
        Ok(())
    }
    fn countdown_audio(&mut self, events: Vec<&'static str>) {
        for event in events {
            self.post_audio_from_game(self.player_entity, event);
        }
    }
    pub(super) fn advance_countdown(&mut self, delta: f32) {
        let was_visible = self.countdown.visible;
        let mut events = self.countdown.advance(delta);
        if was_visible && !self.countdown.visible {
            events.extend(self.countdown.closed());
        }
        self.countdown_audio(events);
    }
    pub(super) fn countdown_lua(
        &mut self,
        lua: &Lua,
        name: &str,
        args: MultiValue,
    ) -> mlua::Result<MultiValue> {
        let required = match name {
            "TimerStart" => 1,
            "TimerSetTime" | "TimerAddTime" => 1,
            _ => 0,
        };
        if (name == "TimerStart" && args.len() < 1)
            || (name != "TimerStart" && args.len() != required)
        {
            return Err(mlua::Error::external(format!(
                "{name} has invalid argument count"
            )));
        }
        let mut events = Vec::new();
        match name {
            "TimerGetTime" => {
                return Ok(MultiValue::from_vec(vec![Value::Number(
                    self.countdown.remaining as f64,
                )]))
            }
            "TimerGetState" => {
                return Ok(MultiValue::from_vec(vec![Value::String(
                    lua.create_string(if self.countdown.ticking() {
                        "Ticking"
                    } else {
                        "Expired"
                    })?,
                )]))
            }
            "TimerSetTime" => self.countdown.remaining = scalar(lua, args[0].clone())?,
            "TimerAddTime" => {
                let value = self.countdown.remaining + scalar(lua, args[0].clone())?;
                if !value.is_finite() || value < i32::MIN as f32 || value >= 2147483648. {
                    return Err(mlua::Error::external(
                        "Timer sum exceeds verified conversion range",
                    ));
                }
                self.countdown.remaining = value;
            }
            "TimerPause" => events = self.countdown.pause(),
            "TimerUnpause" => events = self.countdown.unpause(),
            "TimerStop" => {
                self.countdown.stop();
                events = self.countdown.closed();
            }
            "TimerStart" => {
                //211b70 consumes the last four stack values if extras exist.
                let values = args
                    .iter()
                    .skip(args.len().saturating_sub(4))
                    .collect::<Vec<_>>();
                let time = scalar(lua, values[0].clone())?;
                let mid = values
                    .get(1)
                    .map(|value| scalar(lua, (*value).clone()))
                    .transpose()?
                    .unwrap_or(-1.);
                let late = values
                    .get(2)
                    .map(|value| scalar(lua, (*value).clone()))
                    .transpose()?
                    .unwrap_or(-1.);
                let sound = values
                    .get(3)
                    .map(|value| super::actor_scripts::native_boolean(lua, (*value).clone()))
                    .transpose()?
                    .unwrap_or(true);
                //211448 only assigns a threshold on its negative/default
                // branch. Explicit nonnegative threshold arguments leave it.
                let mid = if mid < 0. {
                    configured(self, "TimerMidSFX", 10.).map_err(mlua::Error::external)?
                } else {
                    self.countdown.mid
                };
                let late = if late < 0. {
                    configured(self, "TimerLateSFX", 5.).map_err(mlua::Error::external)?
                } else {
                    self.countdown.late
                };
                self.countdown.start(time, mid, late, sound);
                // The functional HUD acknowledges the native Menu/Open request.
                self.countdown.shown();
            }
            _ => return Err(mlua::Error::external("Unknown countdown API")),
        }
        self.countdown_audio(events);
        Ok(MultiValue::new())
    }
    pub(super) fn countdown_snapshot(&self) -> serde_json::Value {
        let s: &State = &self.countdown;
        serde_json::json!({"remaining":s.remaining,"running":s.running,"shown":s.shown,"visible":s.visible,
            "mid":s.mid,"late":s.late,"sound":s.sound,"phase":s.phase,"display":s.display,
            "original_flash_presentation_parity":false})
    }
}
