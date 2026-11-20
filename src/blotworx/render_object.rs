//! NiRenderObject runtime byteD0, distinct from serialized NiAVObject flags18.
//! Constructor8048b53c clearsD0; generated decals25b134..144 set bit20.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct Flags(u8);
impl Flags {
    pub fn from_native_byte(byte: u8) -> Self {
        Self(byte)
    }
    /// Mickey motion initialization88998..889ac visits model render objects;
    /// callback886a8 ORs bit10. Renderer15c1f0 records the last admitted target.
    pub fn mark_player_depth_target(&mut self) {
        self.0 |= 0x10;
    }
    pub fn is_player_depth_target(self) -> bool {
        self.0 & 0x10 != 0
    }
    /// Generated ShadowComponent volumes3b5304..310 set bit4, independently.
    pub fn is_shadow_volume(self) -> bool {
        self.0 & 4 != 0
    }
    pub fn is_silhouette(self) -> bool {
        self.0 & 0x40 != 0
    }
    pub fn mark_silhouette(&mut self) {
        self.0 |= 0x40;
    }
    pub fn ignore_color_override(self) -> bool {
        self.0 & 0x20 != 0
    }
    pub fn set_ignore_color_override(&mut self, ignore: bool) {
        if ignore {
            self.0 |= 0x20;
        } else {
            self.0 &= !0x20;
        }
    }
}
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct State {
    pub flags: Flags,
    pub constant_colors: [Option<[u8; 4]>; 4],
    pub regular_colors: [Option<[u8; 4]>; 3],
}
impl State {
    pub fn generated_decal() -> Self {
        let mut state = Self::default();
        state.flags.set_ignore_color_override(true);
        state
    }
    pub fn publish(&mut self, slot: u8, color: [u8; 4]) -> Result<bool> {
        ensure!(slot < 4, "Invalid native constant-color slot");
        if self.flags.ignore_color_override() {
            return Ok(false);
        }
        self.constant_colors[slot as usize] = Some(color);
        Ok(true)
    }
    /// Native16e914 uses flag bit8+slot and storage70+4*slot, separate
    /// from16e978's constant bank (flag11+slot, storage7c+4*slot).
    pub fn publish_regular(&mut self, slot: u8, color: [u8; 4]) -> Result<bool> {
        ensure!(slot < 3, "Invalid native regular-color slot");
        if self.flags.ignore_color_override() {
            return Ok(false);
        }
        self.regular_colors[slot as usize] = Some(color);
        Ok(true)
    }
    pub fn apply(&self, draw: &mut crate::render_support::viewport3d::GpuDrawCommand) -> Result<()> {
        // An unsupported material has no executable shader to receive these
        // values. Preserve its existing diagnostic and retained object state;
        // retry naturally when lighting/material preparation supplies a program.
        if matches!(draw.preview, Some(Err(_))) {
            return Ok(());
        }
        for (slot, color) in self.regular_colors.iter().enumerate() {
            if let Some(color) = color {
                let program = draw
                    .preview
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Regular-color drawable has no material program")
                    })?
                    .as_ref()
                    .map_err(|error| anyhow::anyhow!(error.clone()))?;
                let defaults = program.register_defaults.ok_or_else(|| {
                    anyhow::anyhow!("Regular-color drawable requires a uniform material program")
                })?;
                draw.material_registers.get_or_insert(defaults)[slot] =
                    color.map(|v| f32::from(v) / 255.);
            }
        }
        for (slot, color) in self.constant_colors.iter().enumerate() {
            if let Some(color) = color {
                super::paint_counter::MaterialUpdate {
                    filtered: 0.,
                    constant_color: slot as u8,
                    rgba: *color,
                }
                .apply_to_draw(draw)?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_depth_callback_matches_all_retail_byte_cases() -> Result<()> {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/player_render_flags.json"
        ))?;
        for case in corpus["cases"].as_array().unwrap() {
            let before = case["before"].as_u64().unwrap() as u8;
            let mut flags = Flags::from_native_byte(before);
            flags.mark_player_depth_target();
            assert_eq!(
                flags,
                Flags::from_native_byte(case["after"].as_u64().unwrap() as u8)
            );
            assert!(flags.is_player_depth_target());
            assert_eq!(flags.is_shadow_volume(), before & 4 != 0);
            assert_eq!(flags.is_silhouette(), before & 0x40 != 0);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_mickey_model_initializes_depth_target_without_marking_other_models() -> Result<()> {
        use anyhow::Context;
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let world = super::super::assets::load(
            &root,
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let mut player_meshes = 0;
        let mut other_meshes = 0;
        for (&(owner, _), object) in &world.render_objects {
            let motion = world.entities[owner]
                .components
                .iter()
                .any(|c| c.class_name == "MickeyPlayerMotionControllerComponent");
            assert_eq!(
                object.flags.is_player_depth_target(),
                motion,
                "{}",
                world.entities[owner].name
            );
            if motion {
                player_meshes += 1;
            } else {
                other_meshes += 1;
            }
        }
        assert!(player_meshes > 0 && other_meshes > 0);
        Ok(())
    }
    #[test]
    fn runtime_flags_preserve_other_bits_and_decal_updates_decline() -> Result<()> {
        let mut imported = State::default();
        assert!(imported.publish(1, [55, 139, 213, 234])?);
        let mut decal = State::generated_decal();
        assert!(!decal.publish(1, [1, 2, 3, 4])?);
        assert!(decal.constant_colors.iter().all(Option::is_none));
        let mut flags = Flags::from_native_byte(0xd3);
        flags.set_ignore_color_override(true);
        assert_eq!(flags, Flags::from_native_byte(0xf3));
        flags.set_ignore_color_override(false);
        assert_eq!(flags, Flags::from_native_byte(0xd3));
        assert!(imported.publish(4, [0; 4]).is_err());
        Ok(())
    }
    #[test]
    fn regular_and_constant_banks_remain_independent() -> Result<()> {
        let mut state = State::default();
        state.publish(1, [1, 2, 3, 4])?;
        state.publish_regular(1, [5, 6, 7, 8])?;
        assert_eq!(state.constant_colors[1], Some([1, 2, 3, 4]));
        assert_eq!(state.regular_colors[1], Some([5, 6, 7, 8]));
        state.flags.set_ignore_color_override(true);
        assert!(!state.publish_regular(1, [0; 4])?);
        assert_eq!(state.regular_colors[1], Some([5, 6, 7, 8]));
        assert!(state.publish_regular(3, [0; 4]).is_err());
        Ok(())
    }
}
