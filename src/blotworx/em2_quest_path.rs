//! PC QuestManager critical-path leaves, b56cd0/b56d40 and b57820/b57930.
//! These are string assignments, not quest progression or UI notifications.
//! Wii U sets a dirty byte here; the verified PC setter does not.
#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub selection: Option<[String; 2]>,
}
impl State {
    pub fn call(&mut self, lua: &mlua::Lua, name: &str, args: mlua::MultiValue) -> mlua::Result<mlua::MultiValue> {
        use mlua::{FromLua, Value};
        if name == "Quest_SetCriticalPath" {
            // Native pops from the top: final argument is stage, preceding is quest.
            let mut args=args.into_vec();
            let stage=String::from_lua(args.pop().unwrap_or(Value::Nil),lua)?;
            let quest=String::from_lua(args.pop().unwrap_or(Value::Nil),lua)?;
            self.selection=Some([quest.split('\0').next().unwrap_or_default().into(),stage.split('\0').next().unwrap_or_default().into()]);
            Ok(mlua::MultiValue::new())
        } else {
            let values=self.selection.as_ref().ok_or_else(||mlua::Error::external("PC quest critical-path constructor/save restoration is not implemented"))?;
            Ok(mlua::MultiValue::from_vec(vec![Value::String(lua.create_string(&values[0])?),Value::String(lua.create_string(&values[1])?)]))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mlua::FromLua;
    #[test]
    fn setter_uses_last_two_arguments_and_getter_preserves_case_and_order() -> mlua::Result<()> {
        let lua=mlua::Lua::new();
        let mut state=State::default();
        assert!(state.call(&lua,"Quest_GetCriticalPath",mlua::MultiValue::new()).is_err());
        for (quest,stage) in [("Critical_Yensid","Critical_Yensid_6"),("", "Unregistered"),("CaseSensitive", "Stage\0ignored")] {
            let args=mlua::MultiValue::from_vec(vec![mlua::Value::Integer(99),mlua::Value::String(lua.create_string(quest)?),mlua::Value::String(lua.create_string(stage)?)]);
            assert!(state.call(&lua,"Quest_SetCriticalPath",args)?.is_empty());
            let result=state.call(&lua,"Quest_GetCriticalPath",mlua::MultiValue::new())?;
            assert_eq!(result.len(),2);
            assert_eq!(String::from_lua(result[0].clone(),&lua)?,quest);
            assert_eq!(String::from_lua(result[1].clone(),&lua)?,stage.split('\0').next().unwrap());
        }
        Ok(())
    }
}
