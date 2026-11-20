//! Game-state playthrough word+e0, GAME header+2c0, Lua1c844/1c944.
//! Score tracking, new-game reset coordination and complete save restoration
//! are separate from this field's live ownership and native callback access.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct Playthrough {
    pub value: i32,
}
/// Seed existing singleton fields before a new world's script initialization;
/// applying them afterwards could erase incoming callback changes.
#[derive(Clone, Debug, Default)]
pub struct InitialState {
    pub session: Option<super::level_session::Session>,
    pub restore_checkpoint: bool,
    pub playthrough: Playthrough,
    pub playstyle_points: super::playstyle_points::State,
    pub playtime: super::checkpoint_time::Playtime,
    pub frame_clock: Option<super::frame_clock::Context>,
    pub checkpoint: super::checkpoint::State,
}
impl Playthrough {
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated playthrough header"
        );
        Ok(Self {
            value: i32::from_be_bytes(header[0x2c0..0x2c4].try_into()?),
        })
    }
    ///216f0..21700 writes this word only; do not invent the distinct2c4 flag.
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated playthrough header"
        );
        header[0x2c0..0x2c4].copy_from_slice(&self.value.to_be_bytes());
        Ok(())
    }
}
///2786b0: numbers/numeric strings first, then bool, then one global-name
/// lookup for a nonnumeric string. A nonnumeric global is NOT followed again.
/// Invalid nonnumeric values return0. Wii invalid fctiwz cases remain errors.
pub fn integer_argument(lua: &mlua::Lua, value: mlua::Value) -> mlua::Result<i32> {
    let numeric = if let Some(number) = lua.coerce_number(value.clone())? {
        Some(number)
    } else {
        match value {
            mlua::Value::Boolean(value) => return Ok(i32::from(value)),
            mlua::Value::String(name) => {
                let bytes = name.as_bytes();
                let end = bytes
                    .iter()
                    .position(|&byte| byte == 0)
                    .unwrap_or(bytes.len());
                let key = lua.create_string(&bytes[..end])?;
                let value = lua.globals().get::<mlua::Value>(key)?;
                lua.coerce_number(value)?
            }
            _ => None,
        }
    };
    let Some(number) = numeric else { return Ok(0) };
    let truncated = number.trunc();
    if !truncated.is_finite() || truncated < f64::from(i32::MIN) || truncated > f64::from(i32::MAX)
    {
        return Err(mlua::Error::external(
            "Unverified Wii playthrough fctiwz conversion",
        ));
    }
    Ok(truncated as i32)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_integer_argument_uses_one_global_lookup_and_boolean_conversion() -> Result<()> {
        let lua = mlua::Lua::new();
        lua.globals().set("named", -19.75)?;
        lua.globals().set("alias", "named")?;
        lua.globals().set("truth", true)?;
        for (source, expected) in [
            ("1.9", 1),
            ("-2.9", -2),
            ("true", 1),
            ("false", 0),
            ("'17.75'", 17),
            ("'named'", -19),
            ("'named\\000ignored'", -19),
            ("'alias'", 0),
            ("'truth'", 0),
            ("'missing'", 0),
            ("nil", 0),
            ("{}", 0),
        ] {
            let value = lua.load(format!("return {source}")).eval::<mlua::Value>()?;
            assert_eq!(integer_argument(&lua, value)?, expected, "{source}");
        }
        assert!(integer_argument(&lua, mlua::Value::Number(f64::NAN)).is_err());
        assert!(integer_argument(&lua, mlua::Value::Number(2147483648.)).is_err());
        Ok(())
    }
    #[test]
    fn playthrough_header_preserves_raw_signed_words_and_other_fields() -> Result<()> {
        for value in [0, -1, 1, i32::MIN, i32::MAX] {
            let mut header = vec![0xa5; 0x400];
            let before = header.clone();
            let state = Playthrough { value };
            state.write_header(&mut header)?;
            assert_eq!(Playthrough::from_header(&header)?, state);
            for index in 0..header.len() {
                if !(0x2c0..0x2c4).contains(&index) {
                    assert_eq!(header[index], before[index]);
                }
            }
            assert!(state.write_header(&mut header[..0x31f]).is_err());
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_world_lua_playthrough_reaches_live_header_synchronously() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture("assert(GetPlaythrough()==0); assert(select('#',GetPlaythrough())==1); assert(select('#',SetPlaythrough(-7.75))==0); assert(GetPlaythrough()==-7); BwxPlaythrough=13.5; SetPlaythrough('BwxPlaythrough'); assert(GetPlaythrough()==13); SetPlaythrough(true); assert(GetPlaythrough()==1); assert(not pcall(GetPlaythrough,1)); assert(not pcall(SetPlaythrough)); assert(GetPlaythrough()==1)")?;
        let mut header = vec![0xa5; super::super::save_game::HEADER_SIZE];
        super::super::save_game::initialize_header(&mut header)?;
        header[0x2c4] = 0xa5;
        world.write_save_header_owners(&mut header)?;
        assert_eq!(Playthrough::from_header(&header)?, Playthrough { value: 1 });
        assert_eq!(
            header[0x2c4], 0xa5,
            "Playthrough setter must not alter the separate new-playthrough marker"
        );
        let ctx = egui::Context::default();
        let mut next = super::super::assets::load_with_score_state(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &ctx,
            None,
            None,
            Default::default(),
            Default::default(),
            InitialState {
                playthrough: Playthrough { value: 27 },
                playstyle_points: world.playstyle_points,
                ..Default::default()
            },
        )?;
        next.run_callback_fixture(
            "assert(GetPlaythrough()==27); SetPlaythrough(28); assert(GetPlaythrough()==28)",
        )?;
        assert_eq!(next.playthrough.value, 28);
        Ok(())
    }
    #[test]
    #[ignore = "requires original native save in BLOTWORX_TEST_SAVE"]
    fn native_playthrough_header_roundtrip_leaves_save_unchanged() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_SAVE"))?;
        let original = std::fs::read(&path)?;
        let state = Playthrough::from_header(&original)?;
        let mut rewritten = original.clone();
        state.write_header(&mut rewritten)?;
        assert_eq!(rewritten, original);
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }
}
