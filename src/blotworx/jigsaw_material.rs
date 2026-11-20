//! Native material selection, REL 0x15eb88..0x15ec24 and 0x16d81c..0x16d998.
//! Material selection alone does not implement the sketch passes or transitions.
pub fn sketch_shader(character_flag: bool, alpha_flags: u16) -> &'static str {
    if character_flag {
        "GEN_sketch_characters"
    } else if alpha_flags & 0x200 != 0 && (alpha_flags >> 10) & 7 != 0 {
        "GEN_sketch"
    } else {
        "GEN_sketchOpaque"
    }
}

/// Recovered transition-template selection (REL 0x176440..0x1764c0,
/// 0x176a74..0x176ba0). The renderer combines this with the mesh's material;
/// it must not blindly replace the original surface shader with this template.
#[derive(Debug, PartialEq)]
pub struct TransitionMaterial {
    pub shader: &'static str,
    pub progress: f32,
    pub curve_time: f32,
}
pub fn transition_material(
    alpha_flags: u16,
    scalar: Option<f32>,
) -> anyhow::Result<TransitionMaterial> {
    anyhow::ensure!(
        scalar.is_none_or(f32::is_finite),
        "Nonfinite jigsaw transition scalar"
    );
    // Missing extra-data value takes the painting branch with progress zero.
    let thinning = scalar.is_some_and(|v| v <= 1.);
    let progress = scalar.map_or(0., |v| if thinning { v } else { v - 2. });
    let alpha_test = alpha_flags & 0x200 != 0 && (alpha_flags >> 10) & 7 != 0;
    let shader = match (thinning, alpha_test) {
        (true, true) => "GEN_paintToSketch",
        (true, false) => "GEN_paintToSketchOpaque",
        (false, true) => "GEN_sketchToPaint",
        (false, false) => "GEN_sketchToPaintOpaque",
    };
    // Literal at REL 0x4e51c8; no clamp before animation sampling.
    Ok(TransitionMaterial {
        shader,
        progress,
        curve_time: progress * 1.5,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_selection_preserves_native_boundaries_and_curve_time() -> anyhow::Result<()> {
        for (scalar, shader, progress) in [
            (Some(0.), "GEN_paintToSketchOpaque", 0.),
            (Some(1.), "GEN_paintToSketchOpaque", 1.),
            (Some(1.5), "GEN_sketchToPaintOpaque", -0.5),
            (Some(2.), "GEN_sketchToPaintOpaque", 0.),
            (Some(3.), "GEN_sketchToPaintOpaque", 1.),
            (None, "GEN_sketchToPaintOpaque", 0.),
        ] {
            assert_eq!(
                transition_material(1, scalar)?,
                TransitionMaterial {
                    shader,
                    progress,
                    curve_time: progress * 1.5
                }
            );
        }
        assert_eq!(
            transition_material(0x600, Some(0.5))?.shader,
            "GEN_paintToSketch"
        );
        assert_eq!(
            transition_material(0x600, Some(2.5))?.shader,
            "GEN_sketchToPaint"
        );
        assert!(transition_material(0, Some(f32::NAN)).is_err());
        Ok(())
    }
    #[test]
    fn sketch_selection_uses_alpha_test_not_alpha_blending() {
        assert_eq!(sketch_shader(false, 1), "GEN_sketchOpaque");
        assert_eq!(sketch_shader(false, 0x200), "GEN_sketchOpaque");
        assert_eq!(sketch_shader(false, 0x400), "GEN_sketchOpaque");
        assert_eq!(sketch_shader(false, 0x600), "GEN_sketch");
        assert_eq!(sketch_shader(true, 0x600), "GEN_sketch_characters");
    }
    #[test]
    #[ignore = "Requires BLOTWORX_TEST_DATA pointing to original game files"]
    fn original_sketch_shaders_compile_without_unneeded_lighting() -> anyhow::Result<()> {
        use anyhow::Context;
        use epic_mickey_lib_rs::packfile::Packfile;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = Packfile::from_binary(std::fs::read(root.join("packfiles/r3mt.pak"))?);
        for name in [
            "GEN_paintToSketch",
            "GEN_paintToSketchOpaque",
            "GEN_sketchToPaint",
            "GEN_sketchToPaintOpaque",
        ] {
            let suffix = format!("{name}.r3mt").to_ascii_lowercase();
            let f = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(&suffix))
                .context("Missing transition shader")?;
            let program = crate::render_support::material_preview::compile(
                std::str::from_utf8(&f.data)?.trim_start_matches('\u{feff}'),
                false,
            )?;
            assert_eq!(program.texture_slots, vec![0, 1]);
            println!("{name}: compiled default projection, two original texture inputs");
        }
        for name in ["GEN_sketch", "GEN_sketchOpaque", "GEN_sketch_characters"] {
            let suffix = format!("{name}.r3mt").to_ascii_lowercase();
            let f = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(&suffix))
                .context("Missing original sketch shader")?;
            let source = std::str::from_utf8(&f.data)?.trim_start_matches('\u{feff}');
            let program = crate::render_support::material_preview::compile(source, false)?;
            assert!(!program.depth_write);
            assert!(program.depth_test);
            assert!(program.blend.is_some());
            println!(
                "{name}: original TEV compiled; {} texture slots",
                program.texture_slots.len()
            );
        }
        Ok(())
    }
}
