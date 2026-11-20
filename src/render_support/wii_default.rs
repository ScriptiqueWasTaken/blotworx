//! EM1's default Wii shader texture-combine workers, DOL805211ac/8051fd94.
//! These are native GX stage descriptions, not substitute R3MT assets. Texture
//! allocation, channel lighting and PE state belong to the surrounding renderer.
use anyhow::{ensure, Context, Result};

// Original lookup tables806d76f0/806d7738/806d7760. Indices are the default
// shader's internal selectors; their values are GX enums (not R3MT enums).
const COLOR: [u32; 9] = [2, 4, 6, 0, 8, 10, 15, 12, 14];
const COLOR_ALPHA: [u32; 9] = [3, 5, 7, 1, 9, 11, 15, 12, 14];
const ALPHA: [u32; 9] = [1, 2, 3, 0, 4, 5, 7, 6, 6];
const OUTPUT: [u32; 4] = [1, 2, 3, 0];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Stage {
    pub color: [u32; 4],
    pub alpha: [u32; 4],
    pub color_scale: u32,
    pub alpha_scale: u32,
    pub output: u32,
}

fn selectors(input: [u32; 3], output: u32) -> Result<([u32; 3], [u32; 3], u32)> {
    ensure!(
        input.iter().all(|&v| v < 9) && output < 4,
        "Invalid native default-shader selector"
    );
    Ok((
        input.map(|v| COLOR[v as usize]),
        input.map(|v| ALPHA[v as usize]),
        OUTPUT[output as usize],
    ))
}

///80521290..80521490: all six native map-combine cases. Color and alpha
/// deliberately use different operands for several modes. In particular mode3
/// doubles BOTH results, while8051fd94 doubles only color.
pub fn texture_stage(mode: u32, input: [u32; 3], output: u32) -> Result<Stage> {
    let ([x, y, z], [a, b, c], output) = selectors(input, output)?;
    let (color, alpha, scale) = match mode {
        0 => ([x, 15, 15, z], [a, 7, 7, c], 0),
        1 => ([y, x, COLOR_ALPHA[input[0] as usize], z], [7, 7, 7, b], 0),
        2 => ([15, x, y, z], [a, 7, 7, 7], 0),
        3 => ([15, x, y, z], [b, 7, 7, c], 1),
        4 => ([x, 15, 15, z], [7, 7, 7, 0], 0),
        5 => ([x, y, z, 15], [7, 7, 7, a], 0),
        _ => anyhow::bail!("Unknown native default-shader combine mode {mode}"),
    };
    Ok(Stage {
        color,
        alpha,
        color_scale: scale,
        alpha_scale: scale,
        output,
    })
}

///8051fd94: untextured multiply/add stage. Its boolean scale input applies
/// only to RGB; alpha always uses scale1, irrespective of that input.
pub fn multiply_stage(input: [u32; 3], output: u32, double_color: bool) -> Result<Stage> {
    let ([x, y, z], [a, b, _], output) = selectors(input, output)?;
    Ok(Stage {
        color: [15, x, y, z],
        alpha: [7, a, b, 7],
        color_scale: u32::from(double_color),
        alpha_scale: 0,
        output,
    })
}

/// Native admission order matters: stage budget, texture budget, then texture
/// presence. Rejected stages leave both counters unchanged. The limits here
/// are GX hardware limits implemented by the original worker, not engine caps.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub stages: u32,
    pub textures: u32,
}
impl Counts {
    pub fn texture(
        &mut self,
        present: bool,
        mode: u32,
        input: [u32; 3],
        output: u32,
    ) -> Result<(u32, Option<Stage>)> {
        if self.stages >= 16 {
            return Ok((1, None));
        }
        if self.textures >= 8 {
            return Ok((2, None));
        }
        if !present {
            return Ok((3, None));
        }
        let stage = texture_stage(mode, input, output)?;
        self.stages += 1;
        self.textures += 1;
        Ok((0, Some(stage)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub raster: bool,
    pub vertex: bool,
    pub material: Option<[u8; 4]>,
}
///8050c020..8050c218, unlit path. REPLACE bypasses channel setup even if a
/// vertex-color property requests lighting. GQR2=40004 is set at8000b91c.
pub fn unlit_channel(
    apply: u16,
    flags: u16,
    vertex: bool,
    emissive: [f32; 3],
    alpha: f32,
) -> Result<Channel> {
    if apply == 0 {
        return Ok(Channel {
            raster: false,
            vertex: false,
            material: None,
        });
    }
    ensure!(
        flags & 8 == 0,
        "Default Wii shader lighting channels are not implemented"
    );
    let vertex = vertex && (flags >> 4) & 3 == 1;
    let material = if vertex {
        None
    } else {
        ensure!(
            emissive.iter().chain([alpha].iter()).all(|v| v.is_finite()),
            "Nonfinite default material color"
        );
        Some(
            [emissive[0], emissive[1], emissive[2], alpha]
                .map(|v| (255f32 * v).clamp(0., 255.) as u8),
        )
    };
    Ok(Channel {
        raster: true,
        vertex,
        material,
    })
}
///80520950..80521194, with no dynamic effects or secondary texture maps.
pub fn base_stages(raster: bool, base: bool) -> Result<Vec<Stage>> {
    let mut stages = Vec::new();
    if raster {
        stages.push(Stage {
            color: [15, 12, 10, 14],
            alpha: [5, 7, 7, 7],
            color_scale: 0,
            alpha_scale: 0,
            output: 0,
        });
    }
    let output = if raster { 2 } else { 3 };
    if base {
        stages.push(texture_stage(0, [4, 6, 6], output)?);
    }
    if raster {
        stages.push(multiply_stage(
            [3, if base { output } else { 7 }, 0],
            3,
            true,
        )?);
    }
    Ok(stages)
}

const COMPARE: [u32; 8] = [7, 1, 2, 3, 4, 5, 6, 0];
const BLEND: [u32; 11] = [1, 0, 2, 3, 2, 3, 4, 5, 6, 7, 2];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pe {
    pub blend: Option<[u32; 2]>,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: u32,
    pub alpha_compare: u32,
    pub alpha_reference: u8,
    pub early_depth: bool,
}
/// Cached depth worker80523308, blend80504688 and alpha80522474. Keep GX
/// enums until backend translation: NIF blend names alone are insufficient.
pub fn pe(alpha: u16, threshold: u8, depth: u16) -> Result<Pe> {
    let blend = if alpha & 1 != 0 {
        let src = ((alpha >> 1) & 15) as usize;
        let dst = ((alpha >> 5) & 15) as usize;
        Some([
            *BLEND
                .get(src)
                .context("Unsupported native source blend index")?,
            *BLEND
                .get(dst)
                .context("Unsupported native destination blend index")?,
        ])
    } else {
        None
    };
    let test = alpha & 512 != 0;
    Ok(Pe {
        blend,
        depth_test: depth & 1 != 0,
        depth_write: depth & 3 == 3,
        depth_compare: COMPARE[((depth >> 2) & 7) as usize],
        alpha_compare: if test {
            COMPARE[((alpha >> 10) & 7) as usize]
        } else {
            7
        },
        alpha_reference: if test { threshold } else { 0 },
        early_depth: !test && blend.is_none(),
    })
}
pub(super) fn comparison(gx: u32) -> u32 {
    [
        glow::NEVER,
        glow::LESS,
        glow::EQUAL,
        glow::LEQUAL,
        glow::GREATER,
        glow::NOTEQUAL,
        glow::GEQUAL,
        glow::ALWAYS,
    ][gx as usize]
}
pub(super) fn blend_factor(gx: u32, source: bool) -> Result<u32> {
    Ok(match gx {
        0 => glow::ZERO,
        1 => glow::ONE,
        2 => {
            if source {
                glow::DST_COLOR
            } else {
                glow::SRC_COLOR
            }
        }
        3 => {
            if source {
                glow::ONE_MINUS_DST_COLOR
            } else {
                glow::ONE_MINUS_SRC_COLOR
            }
        }
        4 => glow::SRC_ALPHA,
        5 => glow::ONE_MINUS_SRC_ALPHA,
        6 => glow::DST_ALPHA,
        7 => glow::ONE_MINUS_DST_ALPHA,
        _ => anyhow::bail!("Unsupported GX blend factor"),
    })
}

/// Default material for an attached, single-UV particle generator. This is
/// native stage lowering; it does not assign a made-up R3MT material. Lighting,
/// secondary maps and wireframe paths remain explicit unsupported operations.
/// GPU pixel parity, winding and native shader selection still require captures.
pub fn compile_particle(
    bindings: &super::nif::MaterialBindings,
) -> Result<super::material_preview::Program> {
    if bindings.legacy_texture_flags.is_none() {
        // Some authored gameplay FX intentionally carry vertex color and no
        // legacy NiTexturingProperty at all. Keep those emitters visible with
        // their sampled particle color instead of dropping the entire system.
        let vertex: std::sync::Arc<str> = std::sync::Arc::from(super::material_preview::VERTEX);
        return Ok(super::material_preview::Program {
            runtime_stages:0,
            linear_clip_source:Some(std::sync::Arc::downgrade(&vertex)),
            vertex,
            uv_sets:[0;8],
            pinned_projection:false,
            scene_color_slot:None,
            texture_names:vec![],
            template_samplers:vec![],
            register_defaults:None,
            pass_constants:[None;4],
            fragment:"#version 330 core\nin vec4 v_color;out vec4 frag_color;void main(){frag_color=clamp(v_color,0.0,1.0);}".into(),
            textured:false,
            texture_slots:vec![],
            uv_textures:vec![],
            depth_test:true,
            depth_write:false,
            depth_func:glow::LEQUAL,
            blend:Some((glow::SRC_ALPHA,glow::ONE_MINUS_SRC_ALPHA,glow::FUNC_ADD)),
            cull:None,
        });
    }
    compile_unlit_default(bindings)
}
/// Rigid meshes share the native stage/channel/PE workers, but do not have
/// the particle generator's implicit one-UV clamp. Admit UV0 only until the
/// complete mesh coordinate-descriptor selection has reference validation.
pub fn compile_unlit_mesh(
    bindings: &super::nif::MaterialBindings,
) -> Result<super::material_preview::Program> {
    ensure!(
        bindings.textures.iter().all(|texture| texture.uv_set == 0),
        "Default mesh secondary UV coordinates are not implemented"
    );
    // NiTexturingProperty is optional in Gamebryo. A number of EM1 rigid
    // meshes (including the stacked pizza pieces on GV Zone I's tower) use
    // NiMaterialProperty as their complete surface description. Treating
    // that valid state as a shader compile error used to draw huge white
    // fallback rectangles over the level.
    if bindings.legacy_texture_flags.is_none() && bindings.shader.is_none() {
        return compile_untextured_mesh(bindings);
    }
    compile_unlit_default(bindings)
}

fn compile_untextured_mesh(
    bindings: &super::nif::MaterialBindings,
) -> Result<super::material_preview::Program> {
    ensure!(
        bindings.textures.is_empty(),
        "Untextured default mesh has texture bindings"
    );
    let material = bindings
        .legacy_colors
        .as_ref()
        .context("Untextured mesh is missing NiMaterialProperty colors")?;
    let color = [
        material.diffuse[0],
        material.diffuse[1],
        material.diffuse[2],
        material.alpha,
    ];
    ensure!(
        color.iter().all(|value| value.is_finite()),
        "Nonfinite untextured material color"
    );

    // The default NIF depth state is enabled, writes depth, and uses LEQUAL.
    // Authored NiAlphaProperty still controls blending and alpha testing.
    let state = pe(
        bindings.legacy_alpha_flags.unwrap_or(0),
        bindings.legacy_alpha_threshold.unwrap_or(0),
        bindings
            .legacy_depth_flags
            .clone()
            .transpose()
            .map_err(anyhow::Error::msg)?
            .unwrap_or(15),
    )?;
    let wireframe = bindings
        .legacy_wireframe_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or(0);
    ensure!(
        wireframe & 1 == 0,
        "Native default wireframe drawing is not implemented"
    );
    let shade = bindings
        .legacy_shade_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?;
    ensure!(
        shade.is_none_or(|flags| flags & 1 != 0),
        "Native default flat shading is not implemented"
    );
    let stencil = bindings
        .legacy_stencil_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or(0x4180);
    let cull = match (stencil >> 10) & 3 {
        0 | 1 => Some(glow::BACK),
        2 => Some(glow::FRONT),
        _ => None,
    };
    let test = match state.alpha_compare {
        0 => "false".to_owned(),
        7 => "true".to_owned(),
        compare => format!(
            "alpha {} {:.8}",
            ["", "<", "==", "<=", ">", "!=", ">="][compare as usize],
            state.alpha_reference as f32 / 255.0
        ),
    };
    let fragment = format!(
        "#version 330 core\nin vec4 v_color;out vec4 frag_color;void main(){{vec4 base=vec4({:.8},{:.8},{:.8},{:.8});base*=v_color;float alpha=floor(clamp(base.a,0.0,1.0)*255.0+0.5);if(!({test}))discard;frag_color=clamp(base,0.0,1.0);}}",
        color[0], color[1], color[2], color[3]
    );
    let vertex: std::sync::Arc<str> = std::sync::Arc::from(super::material_preview::VERTEX);
    Ok(super::material_preview::Program {
        runtime_stages: 1,
        linear_clip_source: Some(std::sync::Arc::downgrade(&vertex)),
        vertex,
        uv_sets: [0; 8],
        pinned_projection: false,
        scene_color_slot: None,
        texture_names: vec![],
        template_samplers: vec![],
        register_defaults: None,
        pass_constants: [None; 4],
        fragment,
        textured: false,
        texture_slots: vec![],
        uv_textures: vec![],
        depth_test: state.depth_test,
        depth_write: state.depth_write,
        depth_func: comparison(state.depth_compare),
        blend: match state.blend {
            Some([source, destination]) => Some((
                blend_factor(source, true)?,
                blend_factor(destination, false)?,
                glow::FUNC_ADD,
            )),
            None => None,
        },
        cull,
    })
}
fn compile_unlit_default(
    bindings: &super::nif::MaterialBindings,
) -> Result<super::material_preview::Program> {
    ensure!(
        bindings.shader.is_none(),
        "Default material cannot replace an active R3MT material"
    );
    ensure!(
        bindings
            .textures
            .iter()
            .all(|t| t.slot == 0 && t.shader_id.is_none())
            && bindings.textures.len() <= 1,
        "Default Wii shader secondary/shader texture maps are not implemented"
    );
    let flags = |state: &Option<Result<u16, String>>, name: &str| -> Result<u16> {
        state
            .clone()
            .context(format!("Missing {name} state"))?
            .map_err(anyhow::Error::msg)
    };
    let texture_flags = flags(&bindings.legacy_texture_flags, "NiTexturingProperty")?;
    let color_flags = flags(&bindings.vertex_color_flags, "NiVertexColorProperty")?;
    let material = bindings
        .legacy_colors
        .as_ref()
        .context("Missing native material colors")?;
    let channel = unlit_channel(
        (texture_flags >> 1) & 7,
        color_flags,
        bindings.has_vertex_colors,
        material.emissive,
        material.alpha,
    )?;
    let wireframe = bindings
        .legacy_wireframe_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or(0);
    ensure!(
        wireframe & 1 == 0,
        "Native default wireframe drawing is not implemented"
    );
    let shade = bindings
        .legacy_shade_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?;
    ensure!(
        shade.is_none_or(|flags| flags & 1 != 0),
        "Native default flat shading is not implemented"
    );
    // Native NiStencilProperty construction804a99cc installs4180. Only draw
    // mode participates in Wii8051ee08; the Wii has no stencil buffer.
    let stencil = bindings
        .legacy_stencil_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or(0x4180);
    let cull = match (stencil >> 10) & 3 {
        0 | 1 => Some(glow::BACK),
        2 => Some(glow::FRONT),
        _ => None,
    };
    let alpha = bindings
        .legacy_alpha_flags
        .context("Missing NiAlphaProperty state")?;
    let state = pe(
        alpha,
        bindings
            .legacy_alpha_threshold
            .context("Missing alpha-test threshold")?,
        flags(&bindings.legacy_depth_flags, "NiZBufferProperty")?,
    )?;
    let base = !bindings.textures.is_empty();
    let stages = base_stages(channel.raster, base)?;
    ensure!(!stages.is_empty(),"Default material with neither raster channel nor base texture needs native zero-stage validation");
    let glsl = crate::tev::compile_wii_default_stages(&stages)?;
    let raster = if channel.vertex {
        "v_color".to_owned()
    } else if let Some(color) = channel.material {
        format!(
            "vec4({}.0,{}.0,{}.0,{}.0)/255.0",
            color[0], color[1], color[2], color[3]
        )
    } else {
        "vec4(0.0)".into()
    };
    let mut body = String::new();
    for i in 0..stages.len() {
        // Particle generators have one UV stream. Native805209e0 clamps an
        // authored index to the last available stream, hence always UV0 here.
        let sample = if base && i == usize::from(channel.raster) {
            "textureProj(u_tex[0],u_tex_uv[0]*vec3(v_uv,1.0))"
        } else {
            "vec4(1.0)"
        };
        body += &format!(
            "stage_tex[{i}]={sample};stage_ras[{i}]={raster};stage_k[{i}]=vec4(0.0,0.0,0.0,1.0);\n"
        );
    }
    let test = match state.alpha_compare {
        0 => "false".into(),
        7 => "true".into(),
        c => format!(
            "alpha {} {}.0",
            ["", "<", "==", "<=", ">", "!=", ">="][c as usize],
            state.alpha_reference
        ),
    };
    let fragment=format!("#version 330 core\nin vec2 v_uv;in vec4 v_color;out vec4 frag_color;uniform sampler2D u_tex[8];uniform mat3 u_tex_uv[8];\n{glsl}\nvoid main(){{vec4 stage_tex[16];vec4 stage_ras[16];vec4 stage_k[16];vec4 initial[4];for(int i=0;i<4;i++)initial[i]=vec4(0.0);{body}vec4 result=bt_default_combiner(stage_tex,stage_ras,stage_k,initial);float alpha=floor(clamp(result.a,0.0,1.0)*255.0+0.5);if(!({test}))discard;frag_color=clamp(result,0.0,1.0);}}");
    let vertex: std::sync::Arc<str> = std::sync::Arc::from(super::material_preview::VERTEX);
    Ok(super::material_preview::Program {
        runtime_stages: stages.len(),
        linear_clip_source: Some(std::sync::Arc::downgrade(&vertex)),
        vertex,
        uv_sets: [0; 8],
        pinned_projection: false,
        scene_color_slot: None,
        texture_names: vec![],
        template_samplers: vec![],
        register_defaults: None,
        pass_constants: [None; 4],
        fragment,
        textured: base,
        texture_slots: if base { vec![0] } else { vec![] },
        uv_textures: vec![],
        depth_test: state.depth_test,
        depth_write: state.depth_write,
        depth_func: comparison(state.depth_compare),
        blend: match state.blend {
            Some([s, d]) => Some((
                blend_factor(s, true)?,
                blend_factor(d, false)?,
                glow::FUNC_ADD,
            )),
            None => None,
        },
        cull,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_untextured_gamemesh_uses_its_authored_diffuse_color() -> Result<()> {
        let bindings = super::super::nif::MaterialBindings {
            legacy_colors: Some(super::super::nif::LegacyMaterialColors {
                ambient: [0.5; 3],
                diffuse: [0.4, 0.25, 0.1],
                specular: [0.0; 3],
                emissive: [0.0; 3],
                shininess: 0.0,
                alpha: 1.0,
            }),
            legacy_diffuse: Some([0.4, 0.25, 0.1, 1.0]),
            ..Default::default()
        };
        let program = compile_unlit_mesh(&bindings)?;
        assert!(!program.textured);
        assert!(program
            .fragment
            .contains("vec4(0.40000001,0.25000000,0.10000000,1.00000000)"));
        assert!(program.depth_test && program.depth_write);
        assert_eq!(program.depth_func, glow::LEQUAL);
        Ok(())
    }

    #[test]
    fn native_unlit_channels_pipeline_and_pe_match_original() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/wii_default_pipeline.json"
        ))?;
        for c in fixture["channels"].as_array().unwrap() {
            let emissive = std::array::from_fn(|i| c["emissive"][i].as_f64().unwrap() as f32);
            let state = unlit_channel(
                c["apply"].as_u64().unwrap() as u16,
                c["flags"].as_u64().unwrap() as u16,
                c["vertex"] == true,
                emissive,
                c["alpha"].as_f64().unwrap() as f32,
            )?;
            let mut calls = Vec::new();
            if let Some(color) = state.material {
                calls.push(serde_json::json!([
                    "material", 4, color[0], color[1], color[2], color[3]
                ]));
            }
            if state.raster {
                calls.push(serde_json::json!([
                    "control",
                    4,
                    0,
                    0,
                    u8::from(state.vertex),
                    0,
                    0,
                    2
                ]));
            }
            calls.push(serde_json::json!(["channels", u8::from(state.raster)]));
            assert_eq!(serde_json::json!(calls), c["calls"], "{c}");
            assert_eq!(u64::from(state.raster), c["channels"].as_u64().unwrap());
            assert_eq!(u64::from(state.raster), c["mode"].as_u64().unwrap());
        }
        for c in fixture["pipelines"].as_array().unwrap() {
            let stages = base_stages(c["raster"] == true, c["base"] == true)?;
            assert_eq!(serde_json::to_value(stages)?, c["stages"], "{c}");
            assert_eq!(c["colors"], serde_json::json!([[1, 0, 0, 0, 0]]));
            if c["raster"] == true {
                assert_eq!(c["konst"], serde_json::json!([[0, 0, 0, 0, 255]]));
            }
        }
        for c in fixture["pe"].as_array().unwrap() {
            let flags = c["flags"].as_u64().unwrap() as u16;
            let reference = c["threshold"].as_u64().unwrap() as u8;
            let kind = c["kind"].as_str().unwrap();
            let state = pe(
                if kind == "depth" { 0 } else { flags },
                reference,
                if kind == "depth" { flags } else { 0 },
            )?;
            let calls = match kind {
                "blend" => {
                    if let Some([s, d]) = state.blend {
                        serde_json::json!([["blend", 1, s, d, 5]])
                    } else {
                        serde_json::json!([["blend", 0, 1, 0, 5]])
                    }
                }
                "depth" => serde_json::json!([[
                    "depth",
                    u8::from(state.depth_test),
                    state.depth_compare,
                    u8::from(state.depth_write)
                ]]),
                "alpha" => serde_json::json!([
                    ["alpha", state.alpha_compare, state.alpha_reference, 0, 7, 0],
                    ["early_depth", u8::from(state.early_depth)]
                ]),
                _ => unreachable!(),
            };
            assert_eq!(calls, c["calls"], "{c}");
        }
        Ok(())
    }
    #[test]
    fn default_particle_material_uses_native_emissive_and_draw_state() -> Result<()> {
        use super::super::nif::{LegacyMaterialColors, MaterialBindings, MaterialTexture};
        let mut bindings = MaterialBindings {
            legacy_colors: Some(LegacyMaterialColors {
                ambient: [1.; 3],
                diffuse: [0.; 3],
                specular: [0.; 3],
                emissive: [0.4; 3],
                shininess: 4.,
                alpha: 1.,
            }),
            legacy_alpha_flags: Some(3073),
            legacy_alpha_threshold: Some(0),
            legacy_depth_flags: Some(Ok(12)),
            legacy_texture_flags: Some(Ok(4)),
            vertex_color_flags: Some(Ok(16)),
            ..Default::default()
        };
        bindings.textures.push(MaterialTexture {
            shader_id: None,
            slot: 0,
            path: "starburst_tex.nif_wii".into(),
            uv_set: 7,
            sampler: Default::default(),
            transform: None,
        });
        let program = compile_particle(&bindings)?;
        assert!(!program.depth_test && !program.depth_write);
        assert_eq!(program.blend, Some((glow::ONE, glow::ONE, glow::FUNC_ADD)));
        assert!(program
            .fragment
            .contains("vec4(102.0,102.0,102.0,255.0)/255.0"));
        assert!(program.fragment.contains("u_tex_uv[0]*vec3(v_uv,1.0)"));
        assert_eq!(program.texture_slots, vec![0]);
        assert!(program.uv_textures.is_empty());
        bindings.has_vertex_colors = true;
        assert!(!compile_particle(&bindings)?
            .fragment
            .contains("vec4(102.0,102.0,102.0,255.0)/255.0"));
        bindings.legacy_texture_flags = Some(Ok(0));
        bindings.vertex_color_flags = Some(Ok(24));
        assert!(!compile_particle(&bindings)?
            .fragment
            .contains("stage_ras[0]=v_color"));
        bindings.legacy_texture_flags = Some(Ok(4));
        assert!(compile_particle(&bindings).is_err());
        bindings.vertex_color_flags = Some(Ok(16));
        bindings.textures[0].slot = 1;
        assert!(compile_particle(&bindings).is_err());
        Ok(())
    }
    #[test]
    fn native_combine_workers_match_original_gx_calls() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/wii_default_stages.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = std::array::from_fn(|i| case["input"][i].as_u64().unwrap() as u32);
            let output = case["output"].as_u64().unwrap() as u32;
            let stage = if case["kind"] == "texture" {
                let mut counts = Counts {
                    stages: case["stages"].as_u64().unwrap() as u32,
                    textures: case["textures"].as_u64().unwrap() as u32,
                };
                let (status, stage) = counts.texture(
                    case["present"] == true,
                    case["mode"].as_u64().unwrap() as u32,
                    input,
                    output,
                )?;
                assert_eq!(status as u64, case["status"].as_u64().unwrap());
                assert_eq!(counts.stages as u64, case["final_stages"].as_u64().unwrap());
                assert_eq!(
                    counts.textures as u64,
                    case["final_textures"].as_u64().unwrap()
                );
                stage
            } else {
                Some(multiply_stage(input, output, case["double_color"] == true)?)
            };
            assert_eq!(serde_json::to_value(stage)?, case["stage"], "{case}");
        }
        Ok(())
    }
    #[test]
    fn native_stages_lower_without_losing_register_or_alpha_scale() -> Result<()> {
        let mut stages = Vec::new();
        for mode in 0..6 {
            stages.push(texture_stage(mode, [3, 4, 2], mode % 4)?);
        }
        stages.push(multiply_stage([3, 2, 0], 3, true)?);
        let glsl = crate::tev::compile_wii_default_stages(&stages)?;
        // Color doubles in the final worker, alpha does not. Both expressions
        // read the old registers before either destination channel is written.
        let last = glsl.rsplit("{vec4 tex=").next().unwrap();
        assert!(last.contains("*2.0,0.0,1.0)"));
        assert!(last.contains("*1.0,0.0,1.0)"));
        assert!(last.find("float next_alpha").unwrap() < last.find("r[0].rgb=").unwrap());
        assert!(glsl.ends_with("return bt_combiner_effect(r[0],r[0]);\n}\n"));
        assert!(crate::tev::compile_wii_default_stages(&[]).is_err());
        assert!(texture_stage(6, [0; 3], 0).is_err());
        assert!(texture_stage(0, [9, 0, 0], 0).is_err());
        Ok(())
    }
}
