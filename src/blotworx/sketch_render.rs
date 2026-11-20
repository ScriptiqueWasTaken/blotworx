//! InkThinnerFrontSurface color kernel, REL3d398..3d5fc and3d7d0..3d928.
//! This binds the recovered TEV/color calculation to the current front-surface
//! preview. Native back-surface pass scheduling and object opacity are pending.
use crate::render_support::material_preview;
use anyhow::{Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Default)]
pub struct Shaders {
    programs: HashMap<String, Arc<material_preview::Program>>,
}
// GX color: stage0 K0 -> Reg0; stage1 lerp(Reg0,K1,Reg0.a).
// GX alpha: stage0 1-Tex0.a -> Reg0; stage1 K1.a -> Prev.
const STAGES: &str = r#"<Stages Applyable="true">
<Stage Applyable="true"><Order RasOrder="ColorNull" TexMapOrder="0" TexCoordOrder="0"/><Swap RasSwap="0" TexMapSwap="0"/>
<ColorStage A="Konst" B="Zero" C="Zero" D="Zero" KSel="K0" Op="Add" Bias="Zero" Scale="Scale1" Clamp="true" OutReg="Reg0"/>
<AlphaStage A="Konst" B="Zero" C="TexA" D="Zero" KSel="C8_8" Op="Add" Bias="Zero" Scale="Scale1" Clamp="true" OutReg="Reg0"/>
<IndirectStage IndStage="0" Format="Itf8" BiasSel="ItbNone" MatrixSel="ItmOff" WrapS="ItwOff" WrapT="ItwOff" AddPrev="false" UtcLod="false" AlphaSel="ItbaOff"/></Stage>
<Stage Applyable="true"><Order RasOrder="ColorNull" TexMapOrder="-1" TexCoordOrder="-1"/><Swap RasSwap="0" TexMapSwap="0"/>
<ColorStage A="C0" B="Konst" C="A0" D="Zero" KSel="K1" Op="Add" Bias="Zero" Scale="Scale1" Clamp="true" OutReg="Prev"/>
<AlphaStage A="Konst" B="Zero" C="Zero" D="Zero" KSel="K1A" Op="Add" Bias="Zero" Scale="Scale1" Clamp="true" OutReg="Prev"/>
<IndirectStage IndStage="0" Format="Itf8" BiasSel="ItbNone" MatrixSel="ItmOff" WrapS="ItwOff" WrapT="ItwOff" AddPrev="false" UtcLod="false" AlphaSel="ItbaOff"/></Stage></Stages>"#;
pub fn colors(settings: &BTreeMap<String, String>) -> Result<crate::tev::Registers> {
    let byte = |name: &str, default: i32| -> Result<u8> {
        Ok(settings
            .get(name)
            .map(|v| v.parse::<i32>().with_context(|| format!("Invalid {name}")))
            .transpose()?
            .unwrap_or(default) as u8)
    };
    // Constructor explicitly forces white alpha255; stb preserves low bytes
    // of integer settings. Native absent-key defaults are shared with back.
    Ok(crate::tev::Registers {
        colors: [None; 3],
        constants: [
            Some([
                byte("CsgFrontThinnedWhiteR", 255)?,
                byte("CsgFrontThinnedWhiteG", 255)?,
                byte("CsgFrontThinnedWhiteB", 255)?,
                255,
            ]),
            Some([
                byte("CsgFrontThinnedBlueR", 6)?,
                byte("CsgFrontThinnedBlueG", 73)?,
                byte("CsgFrontThinnedBlueB", 250)?,
                byte("CsgFrontThinnedA", 12)?,
            ]),
            None,
            None,
        ],
    })
}
impl Shaders {
    pub fn load(files: &[VirtualFile], settings: &BTreeMap<String, String>) -> Result<Self> {
        let registers = colors(settings)?;
        let mut programs = HashMap::new();
        for name in ["GEN_sketch", "GEN_sketchOpaque", "GEN_sketch_characters"] {
            let suffix = format!("/{name}.r3mt").to_ascii_lowercase();
            let file = files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(&suffix))
                .context("Missing original sketch shader")?;
            let mut text = std::str::from_utf8(&file.data)?
                .trim_start_matches('\u{feff}')
                .to_owned();
            let start = text
                .find("<Stages ")
                .context("Missing original sketch stages")?;
            let end = start
                + text[start..]
                    .find("</Stages>")
                    .context("Missing sketch stage end")?
                + 9;
            text.replace_range(start..end, STAGES);
            // Native callback 3d5e4..3d5f8 explicitly sets both comparisons
            // to Always. Blend state is also overridden by this draw callback.
            let start = text
                .find("<ACompare ")
                .context("Missing sketch alpha comparison")?;
            let end = start
                + text[start..]
                    .find("/>")
                    .context("Missing sketch alpha comparison end")?
                + 2;
            text.replace_range(
                start..end,
                r#"<ACompare Comp0="Always" Ref0="0" Op="And" Comp1="Always" Ref1="0"/>"#,
            );
            let mut program =
                material_preview::compile_uniform_registers(&text, false, &registers)?;
            // REL3d5d0..3d5e0 calls GXSetBlendMode(1,4,5,0): source
            // alpha / inverse source alpha, regardless of the opaque template.
            program.blend = Some((glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::FUNC_ADD));
            // Front callback3d478..3d49c explicitly writes GX K0/K1. Preserve
            // this pass state after render-object counter/rim overrides; they
            // must not turn a thinned front surface into an opaque white fill.
            for slot in 0..2 {
                program.pass_constants[slot] =
                    registers.constants[slot].map(|color| color.map(|v| f32::from(v) / 255.));
            }
            programs.insert(name.into(), Arc::new(program));
        }
        Ok(Self { programs })
    }
    pub fn program(&self, name: &str) -> Result<Arc<material_preview::Program>> {
        self.programs
            .get(name)
            .cloned()
            .context("Unknown sketch surface shader")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_front_callback_uses_source_alpha_blending() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let stored = crate::pack_io::read_filtered(
            std::fs::read(root.join("packfiles/r3mt.pak"))?,
            |_, path| {
                [
                    "gen_sketch.r3mt",
                    "gen_sketchopaque.r3mt",
                    "gen_sketch_characters.r3mt",
                ]
                .iter()
                .any(|name| path.to_ascii_lowercase().ends_with(name))
            },
        )?;
        let opaque = stored
            .pak
            .files
            .iter()
            .find(|file| {
                file.path
                    .to_ascii_lowercase()
                    .ends_with("gen_sketchopaque.r3mt")
            })
            .context("Missing original opaque sketch")?;
        let source = std::str::from_utf8(&opaque.data)?.trim_start_matches('\u{feff}');
        assert_eq!(
            material_preview::compile(source, false)?.blend,
            Some((glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::FUNC_ADD))
        );
        let shaders = Shaders::load(&stored.pak.files, &BTreeMap::new())?;
        for name in ["GEN_sketch", "GEN_sketchOpaque", "GEN_sketch_characters"] {
            let program = shaders.program(name)?;
            assert_eq!(
                program.blend,
                Some((glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::FUNC_ADD))
            );
            assert_eq!(program.pass_constants[1].unwrap()[3], 12. / 255.);
        }
        Ok(())
    }
    #[test]
    fn native_defaults_and_low_byte_stores_are_preserved() -> Result<()> {
        let mut settings = BTreeMap::new();
        let defaults = colors(&settings)?;
        assert_eq!(defaults.constants[0], Some([255; 4]));
        assert_eq!(defaults.constants[1], Some([6, 73, 250, 12]));
        settings.insert("CsgFrontThinnedA".into(), "300".into());
        settings.insert("CsgFrontThinnedBlueR".into(), "-1".into());
        assert_eq!(colors(&settings)?.constants[1], Some([255, 73, 250, 44]));
        Ok(())
    }
}
