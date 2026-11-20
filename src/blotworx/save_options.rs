//! OPTN1 stores its state in GAME header2b4..2c0, not its section payload.
//! REL21728 ->1f7614 / restore1f75f0 / config initialization1f6990..1f6a40.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct State {
    // Restore copies raw bytes; normalizing here would change native save data.
    pub left_handed: u8,
    pub rumble: u8,
    pub sfx_volume: i32,
    pub music_volume: i32,
}
impl State {
    pub fn from_settings(settings: &BTreeMap<String, String>) -> Result<Self> {
        let integer = |name: &str, default: i32| -> Result<i32> {
            match settings.get(name).map(|value| value.trim()) {
                None => Ok(default),
                Some(value) if value.eq_ignore_ascii_case("true") => Ok(1),
                Some(value) if value.eq_ignore_ascii_case("false") => Ok(0),
                Some(value) => value
                    .parse()
                    .with_context(|| format!("Invalid integer option {name}: {value}")),
            }
        };
        Ok(Self {
            left_handed: u8::from(integer("PlayerLeftHanded", 0)? != 0),
            rumble: u8::from(integer("PlayerRumble", 0)? != 0),
            sfx_volume: integer("AudioSFXVolume", 100)?,
            music_volume: integer("AudioMusicVolume", 100)?,
        })
    }
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated options save header"
        );
        Ok(Self {
            left_handed: header[0x2b4],
            rumble: header[0x2b5],
            sfx_volume: i32::from_be_bytes(header[0x2b8..0x2bc].try_into()?),
            music_volume: i32::from_be_bytes(header[0x2bc..0x2c0].try_into()?),
        })
    }
    ///1f7668..1f7684 leaves header2b6/2b7 and every other byte untouched.
    /// Volumes are signed integer words; native serialization does not clamp.
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated options save header"
        );
        header[0x2b4] = self.left_handed;
        header[0x2b5] = self.rumble;
        header[0x2b8..0x2bc].copy_from_slice(&self.sfx_volume.to_be_bytes());
        header[0x2bc..0x2c0].copy_from_slice(&self.music_volume.to_be_bytes());
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"state":self,"section":"OPTN","version":1,"payload_bytes":0,
            "header_offset":0x2b4,"runtime_application_implemented":false,"checkpoint_integrated":false})
    }
}

/// SCPT1's actual registered adapter2d4618 ->27653c tail-calls25e0cc.
/// It only queries read mode;25fd10 ignores the return and it never touches
/// Lua state or section/header bytes. This is verified code, not an empty-save
/// inference. Version/tag276534/276528; registration26ea34..26ea4c.
pub const SCRIPT_SECTION: ([u8; 4], u32, &[u8]) = (*b"SCPT", 1, &[]);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_options_defaults_and_integer_flags_are_not_slider_clamped() -> Result<()> {
        let mut settings = BTreeMap::new();
        assert_eq!(
            State::from_settings(&settings)?,
            State {
                left_handed: 0,
                rumble: 0,
                sfx_volume: 100,
                music_volume: 100
            }
        );
        for (name, value) in [
            ("PlayerLeftHanded", "-1"),
            ("PlayerRumble", "true"),
            ("AudioSFXVolume", "-25"),
            ("AudioMusicVolume", "125"),
        ] {
            settings.insert(name.into(), value.into());
        }
        assert_eq!(
            State::from_settings(&settings)?,
            State {
                left_handed: 1,
                rumble: 1,
                sfx_volume: -25,
                music_volume: 125
            }
        );
        settings.insert("AudioSFXVolume".into(), "invalid".into());
        assert!(State::from_settings(&settings).is_err());
        Ok(())
    }
    #[test]
    fn native_header_write_preserves_padding_tail_and_raw_restored_flag_bytes() -> Result<()> {
        let mut header = (0..0x400).map(|i| (i * 37 + 11) as u8).collect::<Vec<_>>();
        let before = header.clone();
        let state = State {
            left_handed: 128,
            rumble: 255,
            sfx_volume: i32::MIN,
            music_volume: i32::MAX,
        };
        state.write_header(&mut header)?;
        assert_eq!(State::from_header(&header)?, state);
        for i in 0..header.len() {
            if !matches!(i, 0x2b4 | 0x2b5 | 0x2b8..=0x2bf) {
                assert_eq!(header[i], before[i], "Changed {i:#x}");
            }
        }
        assert_eq!(header[0x2b4], 128);
        assert_eq!(header[0x2b5], 255);
        let before = header.clone();
        assert!(state.write_header(&mut header[..0x31f]).is_err());
        assert_eq!(header, before);
        assert!(State::from_header(&header[..0x31f]).is_err());
        Ok(())
    }
    #[test]
    fn native_options_and_script_owners_have_empty_sections_without_erasing_header_state(
    ) -> Result<()> {
        let mut storage = vec![0xa5; 65536];
        super::super::save_game::initialize_header(&mut storage)?;
        let state = State {
            left_handed: 1,
            rumble: 1,
            sfx_volume: 60,
            music_volume: 20,
        };
        state.write_header(&mut storage)?;
        let mut writer = super::super::save_game::MemoryWriter::from_storage(storage)?;
        writer.append_section(SCRIPT_SECTION.0, SCRIPT_SECTION.1, SCRIPT_SECTION.2)?;
        writer.append_section(*b"OPTN", 1, &[])?;
        let built = writer.finish()?;
        let save = super::super::save_game::Archive::parse(&built.bytes)?;
        assert_eq!(State::from_header(&built.bytes)?, state);
        assert_eq!(
            save.sections
                .iter()
                .map(|section| (section.tag, section.version, section.payload.len()))
                .collect::<Vec<_>>(),
            [(*b"SCPT", 1, 0), (*b"OPTN", 1, 0), (*b"LAST", 1, 0)]
        );
        assert_eq!(
            built.logical_size,
            super::super::save_game::HEADER_SIZE + 48
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original populated EM1 save in BLOTWORX_TEST_SAVE"]
    fn original_native_options_rewrite_only_owned_header_bytes() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let save = super::super::save_game::Archive::parse(&bytes)?;
        for tag in [*b"SCPT", *b"OPTN"] {
            let section = save.section(tag)?;
            assert_eq!(section.version, 1);
            assert!(section.payload.is_empty());
        }
        let state = State::from_header(&bytes)?;
        let mut copy = bytes.clone();
        state.write_header(&mut copy)?;
        assert_eq!(copy, bytes);
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
}
