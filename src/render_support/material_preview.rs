//! Shared lowering of supported retail material state into viewport shaders.
//! Unsupported inputs return a reason; they must not silently look like an accurate material.
use anyhow::{bail, ensure, Context, Result};
use roxmltree::Node;
#[path = "material_indirect.rs"]
pub(crate) mod indirect;
#[path = "material_projection.rs"]
pub(crate) mod projection;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Program {
    pub(crate) runtime_stages: usize,
    pub vertex: Arc<str>,
    /// Provenance of a factory-generated u_mvp * vec4(a_pos,1) shader.
    /// Weak ownership also invalidates this proof on Arc::make_mut edits.
    pub(crate) linear_clip_source: Option<std::sync::Weak<str>>,
    pub uv_sets: [u8; 8],
    pub pinned_projection: bool,
    /// R3MT ImageName by map index (not texture-coordinate index). Native
    /// REL 0x176044 fills absent shader maps from these template resources.
    pub texture_names: Vec<String>,
    pub template_samplers: Vec<Option<super::viewport3d::TextureSampler>>,
    pub register_defaults: Option<[[f32; 4]; 7]>,
    /// Draw callbacks can set GX constants after object material overrides.
    /// These belong to this pass, not to shared/default render-object state.
    pub pass_constants: [Option<[f32; 4]>; 4],
    pub fragment: String,
    pub textured: bool,
    pub texture_slots: Vec<u8>,
    pub uv_textures: Vec<u8>,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_func: u32,
    pub blend: Option<(u32, u32, u32)>,
    pub cull: Option<u32>,
}
impl Program {
    /// Native dynamic-effect stages4782095/524943. The first color write
    /// targets Reg0 while alpha retains APrev; the next lerps CPrev/Reg0.
    /// Read CPrev explicitly, even when a base template's last output differs.
    pub(crate) fn projected_shadow(&self) -> Result<Self> {
        ensure!(
            self.runtime_stages + 2 <= 16,
            "Projected shadow exceeds native TEV stage budget"
        );
        ensure!(
            self.texture_slots.len() + 2 <= 8,
            "Projected shadow exceeds native texture budget"
        );
        ensure!(
            self.has_linear_clip_position(),
            "Projected shadow needs a verified mesh vertex program"
        );
        ensure!(
            self.fragment.matches(crate::tev::EFFECT_HOOK).count() == 1,
            "Projected shadow requires one final generated combiner hook"
        );
        ensure!(
            self.vertex.matches("void main(){").count() == 1,
            "Unknown mesh vertex entry"
        );
        let mut result = self.clone();
        result.fragment = self.fragment.replacen(
            crate::tev::EFFECT_HOOK,
            r#"in vec2 bw_shadow_uv;in vec2 bw_clip_uv;
uniform sampler2D bw_shadow_image;uniform sampler2D bw_clip_image;
vec4 bt_combiner_effect(vec4 previous,vec4 result){
ivec3 prev=ivec3(floor(previous.rgb*255.+0.5))&ivec3(255);
ivec3 mask=ivec3(floor(texture(bw_shadow_image,bw_shadow_uv).rgb*255.+0.5));
ivec3 clip=ivec3(floor(texture(bw_clip_image,bw_clip_uv).rgb*255.+0.5));
ivec3 reg0=clamp((mask*(prev+(prev>>7))+ivec3(128))>>8,ivec3(0),ivec3(255));
ivec3 out_rgb=clamp(((prev<<8)+(reg0-prev)*(clip+(clip>>7))+ivec3(128))>>8,ivec3(0),ivec3(255));
return vec4(vec3(out_rgb)/255.,clamp(previous.a,0.,1.));}"#,
            1,
        );
        result.vertex = Arc::from(self.vertex.replacen(
            "void main(){",
            r#"uniform mat4 bw_shadow_matrix;uniform mat4 bw_clip_matrix;
out vec2 bw_shadow_uv;out vec2 bw_clip_uv;
void main(){
bw_shadow_uv=(bw_shadow_matrix*vec4(a_pos,1.)).xy;
bw_clip_uv=(bw_clip_matrix*vec4(a_pos,1.)).xy;"#,
            1,
        ));
        result.linear_clip_source = Some(Arc::downgrade(&result.vertex));
        result.runtime_stages += 2;
        Ok(result)
    }
    pub(crate) fn has_linear_clip_position(&self) -> bool {
        self.linear_clip_source
            .as_ref()
            .and_then(|source| source.upgrade())
            .is_some_and(|source| Arc::ptr_eq(&source, &self.vertex))
    }

    pub fn presentation_registers(&self, object: Option<&[[f32; 4]; 7]>) -> Option<[[f32; 4]; 7]> {
        let defaults = self.register_defaults.as_ref()?;
        let mut registers = *object.unwrap_or(defaults);
        for (slot, color) in self.pass_constants.iter().enumerate() {
            if let Some(color) = color {
                registers[3 + slot] = *color;
            }
        }
        Some(registers)
    }
    /// Native attachment scans map-array order, including maps not sampled
    /// by the TEV program, and exits after the first matching resource.
    pub fn first_runtime_sh_slot(&self, bindings: &super::nif::MaterialBindings) -> Option<u32> {
        let slots: std::collections::BTreeSet<_> = (0..self.texture_names.len() as u32)
            .chain(bindings.textures.iter().map(|t| t.slot))
            .collect();
        slots.into_iter().find(|&slot| {
            let name = bindings
                .textures
                .iter()
                .rev()
                .find(|t| t.slot == slot)
                .map(|t| t.path.as_str())
                .or_else(|| self.texture_names.get(slot as usize).map(String::as_str));
            name.is_some_and(|name| {
                matches!(
                    name.replace('\\', "/").to_ascii_lowercase().as_str(),
                    "defaultsh_32" | "effects/r3mt/defaultsh_32_tex.nif"
                )
            })
        })
    }
    /// A supplied map wins over the template. REL 0x176094..0x1760f4 only
    /// creates defaults for null entries, never overwriting an authored map.
    pub fn image_name<'a>(
        &'a self,
        slot: u8,
        bindings: &'a super::nif::MaterialBindings,
    ) -> Option<&'a str> {
        bindings
            .textures
            .iter()
            .rev()
            .find(|t| t.slot == u32::from(slot))
            .map(|t| t.path.as_str())
            .or_else(|| {
                self.texture_names
                    .get(usize::from(slot))
                    .map(String::as_str)
            })
    }
    /// REL 0x24f52c compares the two interned resource names. An integer named
    /// SphericalHarmonicMap alone does not establish that a map is replaceable.
    pub fn is_runtime_sh(&self, slot: u8, bindings: &super::nif::MaterialBindings) -> bool {
        self.image_name(slot, bindings).is_some_and(|name| {
            matches!(
                name.replace('\\', "/").to_ascii_lowercase().as_str(),
                "defaultsh_32" | "effects/r3mt/defaultsh_32_tex.nif"
            )
        })
    }
}
pub const VERTEX: &str = "#version 330 core
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normal;
layout(location=2) in vec2 a_uv;
layout(location=3) in vec4 a_color;
layout(location=4) in vec2 a_uv1;layout(location=5) in vec2 a_uv2;
layout(location=6) in vec2 a_uv3;layout(location=7) in vec2 a_uv4;
layout(location=8) in vec2 a_uv5;layout(location=9) in vec2 a_uv6;layout(location=10) in vec2 a_uv7;
uniform mat4 u_mvp;uniform mat4 u_model_view;
uniform mat3 u_uv_transform;
out vec2 v_uv;out vec2 v_environment;out vec3 v_projection; out vec4 v_color;out vec2 v_toon;
out vec2 v_extra_uv[7];
void main(){
vec4 clip=u_mvp*vec4(a_pos,1);gl_Position=clip;v_uv=a_uv;v_color=a_color;
v_toon=(floor(clamp(v_color.rgb,0.0,1.0)*255.0+0.5)/255.0).rg;
v_extra_uv[0]=a_uv1;v_extra_uv[1]=a_uv2;v_extra_uv[2]=a_uv3;v_extra_uv[3]=a_uv4;
v_extra_uv[4]=a_uv5;v_extra_uv[5]=a_uv6;v_extra_uv[6]=a_uv7;
vec3 n=transpose(inverse(mat3(u_model_view)))*a_normal;
float magnitude=length(n);n=magnitude>0.000001?n/magnitude:vec3(0.0);
v_environment=vec2(0.5+0.5*n.x,0.5-0.5*n.y);
v_projection=vec3(0.5*(clip.x+clip.w),0.5*(clip.w-clip.y),clip.w);
}";

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>> {
    n.children()
        .find(|n| n.has_tag_name(name))
        .with_context(|| format!("Missing {name}"))
}
fn attr<'a>(n: Node<'a, 'a>, name: &str) -> Result<&'a str> {
    n.attribute(name).with_context(|| format!("Missing {name}"))
}
fn comparison(name: &str) -> Result<u32> {
    Ok(match name {
        "Never" => glow::NEVER,
        "Less" => glow::LESS,
        "Equal" => glow::EQUAL,
        "Lequal" => glow::LEQUAL,
        "Greater" => glow::GREATER,
        "Nequal" => glow::NOTEQUAL,
        "Gequal" => glow::GEQUAL,
        "Always" => glow::ALWAYS,
        _ => bail!("Unknown comparison {name}"),
    })
}
fn alpha_test(name: &str, reference: &str) -> Result<String> {
    let reference: u8 = reference.parse()?;
    let op = match comparison(name)? {
        glow::NEVER => return Ok("false".into()),
        glow::ALWAYS => return Ok("true".into()),
        glow::LESS => "<",
        glow::EQUAL => "==",
        glow::LEQUAL => "<=",
        glow::GREATER => ">",
        glow::NOTEQUAL => "!=",
        _ => ">=",
    };
    Ok(format!("(alpha {op} {reference}.0)"))
}
fn factor(name: &str) -> Result<u32> {
    Ok(match name {
        "Zero" => glow::ZERO,
        "One" => glow::ONE,
        "SrcColor" => glow::SRC_COLOR,
        "InvSrcColor" => glow::ONE_MINUS_SRC_COLOR,
        "SrcAlpha" => glow::SRC_ALPHA,
        "InvSrcAlpha" => glow::ONE_MINUS_SRC_ALPHA,
        "DstAlpha" => glow::DST_ALPHA,
        "InvDstAlpha" => glow::ONE_MINUS_DST_ALPHA,
        _ => bail!("Unknown blend factor {name}"),
    })
}

/// XML enum decoder 0x16858c and map constructor 0x16ac38. The native
/// min/mag conversion is intentionally not the obvious direct GL mapping.
fn template_sampler(node: Node<'_, '_>) -> Result<super::viewport3d::TextureSampler> {
    let wrap = |name: &str| -> Result<u32> {
        Ok(match name {
            "Clamp" => glow::CLAMP_TO_EDGE,
            "Repeat" => glow::REPEAT,
            "Mirror" => glow::MIRRORED_REPEAT,
            _ => bail!("Unknown template wrap {name}"),
        })
    };
    let filter = |name: &str| -> Result<usize> {
        Ok(match name {
            "Near" => 0,
            "NearMipNear" => 1,
            "NearMipLinear" => 2,
            "Linear" => 3,
            "LinearMipNear" => 4,
            "LinearMipLinear" => 5,
            _ => bail!("Unknown template filter {name}"),
        })
    };
    let min = filter(attr(node, "MinFilter")?)?;
    let mag = filter(attr(node, "MagFilter")?)?;
    let native = if min <= 2 {
        [0, 3, 4, 4, 4, 4][mag]
    } else {
        [5, 5, 5, 1, 5, 1][mag]
    };
    let mut sampler = super::viewport3d::TextureSampler::from_nif_flags(native << 8);
    sampler.wrap_u = wrap(attr(node, "WarpS")?)?;
    sampler.wrap_v = wrap(attr(node, "WarpT")?)?;
    Ok(sampler)
}
/// EM1 REL 0x1775f8..0x1776a8, UV generator before the NIF texture matrix.
/// Rotation is in degrees. Native translation is scaled and S is negated;
/// replacing this with a conventional translate/rotate/scale changes the art.
pub(crate) fn native_uv_matrix(
    scale: [f32; 2],
    degrees: f32,
    translation: [f32; 2],
) -> Result<[f32; 9]> {
    ensure!(
        scale
            .into_iter()
            .chain(translation)
            .chain([degrees])
            .all(f32::is_finite),
        "Nonfinite shader texture transform"
    );
    let radians = degrees * f32::from_bits(0x3c8efa35);
    // Native calls double-precision sin/cos, then frsp narrows each result.
    let (sin, cos) = (radians as f64).sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let half_cos = 0.5 * cos;
    let s_offset = scale[0] * ((0.5 + (-0.5f32).mul_add(sin, -half_cos)) - translation[0]);
    let t_offset = scale[1].mul_add(translation[1] + (0.5f32.mul_add(sin, -half_cos) - 0.5), 1.);
    // Column-major GLSL mat3. Native 3x4 uses the translation column for UVs.
    let matrix = [
        scale[0] * cos,
        -scale[1] * sin,
        0.,
        scale[0] * sin,
        scale[1] * cos,
        0.,
        s_offset,
        t_offset,
        1.,
    ];
    ensure!(
        matrix.into_iter().all(f32::is_finite),
        "Shader texture transform overflow"
    );
    Ok(matrix)
}

fn coordinate_expression(
    map: Node<'_, '_>,
    slot: u8,
    uv_textures: &mut std::collections::BTreeSet<u8>,
) -> Result<String> {
    let coord = child(map, "TexCoordGen")?;
    let generated = match attr(coord, "Method")? {
        "UV" => {
            ensure!(
                attr(coord, "UVNumber")?.parse::<u8>()? < 8,
                "UV channel exceeds GX texture-coordinate inputs"
            );
            uv_textures.insert(slot);
            return uv_coordinate_expression(map, slot, uv_textures);
        }
        "Toon" => {
            // Parser168dcc maps Toon to5. REL177fd4 selects GX_TG_SRTG,
            // GX_TG_COLOR0 and identity matrices, ignoring UV/transform fields.
            return Ok("vec3(v_toon,1.0)".into());
        }
        "Environ" => {
            // EM1 method 1 (REL 0x177df8..0x177fd0) obtains the active
            // camera through 0x21a98/0x21a08. r22 (CameraIndex) is not
            // consumed by this branch; authored 0/1 are not cameras
            // that we must resolve from the scene.
            attr(coord, "CameraIndex")?.parse::<i32>()?;
            "vec3(v_environment,1.0)"
        }
        "Projection" => {
            // Retail method 4 ignores CameraIndex (REL 0x177708).
            // This is the default active-camera path; object-specific
            // projection overrides still require runtime bindings.
            attr(coord, "CameraIndex")?.parse::<i32>()?;
            "v_projection"
        }
        _ => bail!("Light-based texture coordinates are not yet reproduced"),
    };
    let mut transform = [0.; 5];
    for (i, (key, expected)) in [
        ("ScaleS", 1.),
        ("ScaleT", 1.),
        ("Rotate", 0.),
        ("TransS", 0.),
        ("TransT", 0.),
    ]
    .into_iter()
    .enumerate()
    {
        transform[i] = attr(coord, key)?.parse::<f32>()?;
        ensure!(
            transform[i].is_finite(),
            "Nonfinite shader texture transform"
        );
        if attr(coord, "Method")? == "Environ" {
            ensure!(
                transform[i] == expected,
                "Non-UV shader texture-coordinate transforms are not yet reproduced"
            );
        }
    }
    let generated = if attr(coord, "Method")? == "Projection" {
        ensure!(
            transform[2] == 0.,
            "Rotated projection requires native material rotation-enable state"
        );
        let name = format!("PinScreenSpace_{slot}");
        let pin = map
            .document()
            .root()
            .descendants()
            .find(|node| {
                node.has_tag_name("StringUserData") && node.attribute("Name") == Some(name.as_str())
            })
            .map(|node| {
                child(node, "Value").and_then(|value| match value.text().unwrap_or("").trim() {
                    "true" | "1" => Ok(true),
                    "false" | "0" => Ok(false),
                    other => bail!("Invalid {name}: {other}"),
                })
            })
            .transpose()?
            .unwrap_or(false);
        // REL177970..177a80 and DOL805ae680: centered active-camera
        // coordinates, S translation added and T translation subtracted.
        // Pinning subtracts the projected NiAVObject world-bound center,
        // then scales by clamped camera-forward depth times 0.1.
        let anchor = if pin {
            "bw_projection_bound.xy"
        } else {
            "vec2(0.5)"
        };
        let depth = if pin { "bw_projection_bound.z" } else { "1.0" };
        format!("vec3((v_projection.xy-{anchor}*v_projection.z)*vec2({:?},{:?})*{depth}+vec2({:?},{:?})*v_projection.z,v_projection.z)",
            transform[0],transform[1],0.5+transform[3],0.5-transform[4])
    } else if attr(coord, "Method")? == "UV" {
        let matrix = native_uv_matrix(
            [transform[0], transform[1]],
            transform[2],
            [transform[3], transform[4]],
        )?;
        format!(
            "(mat3({})*{generated})",
            matrix
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        generated.to_owned()
    };
    Ok(format!("(u_tex_uv[{slot}]*{generated})"))
}
fn uv_coordinate_expression(
    map: Node<'_, '_>,
    slot: u8,
    uv: &mut std::collections::BTreeSet<u8>,
) -> Result<String> {
    let coord = child(map, "TexCoordGen")?;
    uv.insert(slot);
    let get = |name| -> Result<f32> {
        let v = attr(coord, name)?.parse::<f32>()?;
        ensure!(v.is_finite(), "Nonfinite UV transform");
        Ok(v)
    };
    let matrix = native_uv_matrix(
        [get("ScaleS")?, get("ScaleT")?],
        get("Rotate")?,
        [get("TransS")?, get("TransT")?],
    )?;
    // Keep identity text stable for transition material composition.
    let generated = if matrix == [1., 0., 0., 0., 1., 0., 0., 0., 1.] {
        format!("vec3(bw_uv({slot}),1.0)")
    } else {
        format!(
            "(mat3({})*vec3(bw_uv({slot}),1.0))",
            matrix
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    Ok(format!("(u_tex_uv[{slot}]*{generated})"))
}

pub fn compile(text: &str, baked: bool) -> Result<Program> {
    compile_with_registers(text, baked, &crate::tev::Registers::default())
}
pub fn compile_with_registers(
    text: &str,
    baked: bool,
    registers: &crate::tev::Registers,
) -> Result<Program> {
    compile_register_mode(text, baked, registers, false, None)
}
pub fn compile_uniform_registers(
    text: &str,
    baked: bool,
    registers: &crate::tev::Registers,
) -> Result<Program> {
    compile_register_mode(text, baked, registers, true, None)
}
fn compile_register_mode(
    text: &str,
    baked: bool,
    registers: &crate::tev::Registers,
    dynamic: bool,
    native_vertex: Option<bool>,
) -> Result<Program> {
    let mut combiner = if dynamic {
        crate::tev::compile_uniform_registers(text, registers)?
    } else {
        crate::tev::compile_with_registers(text, registers)?
    };
    let doc = roxmltree::Document::parse(text)?;
    let material = child(doc.root_element(), "Material")?;
    let channel = child(material, "Channel")?;
    let maps = child(material, "TextureMaps")?;
    // Texture orders may be authored even when neither TEV result samples them
    // (GEN_sketchOpaque). Do not require a model binding for unused samples.
    let needs_coordinate_history = material
        .descendants()
        .any(|n| n.has_tag_name("IndirectStage") && n.attribute("AddPrev") == Some("true"));
    for (input, stage) in combiner
        .inputs
        .iter_mut()
        .zip(material.descendants().filter(|n| n.has_tag_name("Stage")))
    {
        let sampled = stage
            .children()
            .filter(|n| n.has_tag_name("ColorStage") || n.has_tag_name("AlphaStage"))
            .any(|n| {
                ["A", "B", "C", "D"]
                    .iter()
                    .any(|&a| matches!(n.attribute(a), Some("TexC" | "TexA")))
            });
        if !sampled && !input.indirect && !needs_coordinate_history {
            input.texture = None;
        }
    }

    // Several native sketch shaders declare lighting but never read raster
    // color/alpha in TEV. Those channels cannot affect their output. Keep the
    // lighting restriction for any referenced raster input (conservative even
    // when a later stage might overwrite it).
    let uses_toon = combiner.inputs.iter().any(|input| {
        input.texture.is_some()
            && input
                .coordinate
                .and_then(|coordinate| {
                    maps.children()
                        .filter(|node| node.has_tag_name("TextureMap"))
                        .nth(coordinate as usize)
                })
                .and_then(|map| map.children().find(|node| node.has_tag_name("TexCoordGen")))
                .is_some_and(|generator| generator.attribute("Method") == Some("Toon"))
    });
    let uses_raster = uses_toon
        || material
            .descendants()
            .filter(|n| n.has_tag_name("ColorStage") || n.has_tag_name("AlphaStage"))
            .any(|n| {
                ["A", "B", "C", "D"].iter().any(|&a| {
                    // Add/Sub uses A*(1-C)+B*C: B cannot contribute when C is Zero.
                    let dead_b = a == "B"
                        && n.attribute("C") == Some("Zero")
                        && matches!(n.attribute("Op"), Some("Add" | "Sub"));
                    !dead_b && matches!(n.attribute(a), Some("RasC" | "RasA"))
                })
            });
    let (rgb, alpha) = if let Some(vertex) = native_vertex {
        // REL 0x1768c0..0x176914 calls DOL 0x8050d548, bypassing the
        // general Gamebryo light setup. Both GX channels have lighting off.
        // COLOR0A0 takes vertex color or register 0x000000ff (DOL 0x8069b344).
        // The XML Channel contents are ignored by REL 0x168114.
        if vertex {
            ("v_color.rgb".to_owned(), "v_color.a".to_owned())
        } else {
            ("vec3(0.0)".to_owned(), "1.0".to_owned())
        }
    } else {
        ensure!(
            !uses_raster
                || ((baked || attr(channel, "DiffuseAmbientLighting")? == "false")
                    && attr(channel, "AlphaLighting")? == "false"
                    && attr(channel, "SpecularLighting")? == "false"),
            "Game lighting channel is not yet reproduced"
        );
        let color = child(channel, "Mat0Color")?;
        let mut rgba = Vec::new();
        for c in ["R", "G", "B", "A"] {
            rgba.push(format!("{}.0/255.0", attr(color, c)?.parse::<u8>()?));
        }
        let rgb = match attr(channel, "DiffuseSrc")? {
            "Vertex" => "v_color.rgb".into(),
            "Register" => format!("vec3({})", rgba[..3].join(",")),
            v => bail!("Unsupported diffuse source {v}"),
        };
        let alpha = match attr(channel, "AlphaSrc")? {
            "Vertex" => "v_color.a".into(),
            "Register" => rgba[3].clone(),
            v => bail!("Unsupported alpha source {v}"),
        };
        (rgb, alpha)
    };
    let mut body = String::from("ivec2 bw_ind_coord=ivec2(0);\n");
    let stages = material
        .descendants()
        .filter(|n| n.has_tag_name("Stage"))
        .collect::<Vec<_>>();
    let mut texture_slots = std::collections::BTreeSet::new();
    let mut textured = false;
    let mut uv_textures = std::collections::BTreeSet::new();
    for (i, input) in combiner.inputs.iter().enumerate() {
        let mut indirect_code = None;
        let tex = if let Some(slot) = input.texture {
            let coordinate = input
                .coordinate
                .context("Texture has no coordinate generator")?;
            let map = maps
                .children()
                .filter(|n| n.has_tag_name("TextureMap"))
                .nth(coordinate as usize)
                .context("Missing texture coordinate generator")?;
            let generated = coordinate_expression(map, slot, &mut uv_textures)?;
            textured = true;
            texture_slots.insert(slot);
            if input.indirect {
                indirect_code = Some(
                    indirect::texture(
                        stages[i],
                        maps,
                        slot,
                        &generated,
                        &mut uv_textures,
                        &mut texture_slots,
                    )?
                    .replace("BW_STAGE", &i.to_string()),
                );
                "vec4(1.0)".to_owned()
            } else {
                body.push_str(&format!("{{vec3 bw_coord={generated};bw_ind_coord=ivec2(round((bw_coord.xy/bw_coord.z)*vec2(textureSize(u_tex[{slot}],0))*128.0));}}\n"));
                format!("textureProj(u_tex[{slot}],{generated})")
            }
        } else {
            ensure!(
                !input.indirect,
                "Indirect stage without a sampled texture requires native coordinate ownership"
            );
            "vec4(1.0)".to_owned()
        };
        let raster = match input.raster {
            crate::tev::RasterSource::Zero => "vec4(0.0)".to_owned(),
            crate::tev::RasterSource::Color0 => format!("vec4({rgb},{alpha})"),
            _ => bail!("Secondary raster channel is not yet reproduced"),
        };
        if let Some(code) = indirect_code {
            body.push_str(&code);
        } else {
            body.push_str(&format!("stage_tex[{i}]={tex};\n"));
        }
        body.push_str(&format!("stage_ras[{i}]={raster};\n"));
    }
    let pe = child(material, "PE")?;
    let ac = child(pe, "ACompare")?;
    let a = alpha_test(attr(ac, "Comp0")?, attr(ac, "Ref0")?)?;
    let b = alpha_test(attr(ac, "Comp1")?, attr(ac, "Ref1")?)?;
    let op = match attr(ac, "Op")? {
        "And" => "&&",
        "Or" => "||",
        "Xor" => "!=",
        "Xnor" => "==",
        v => bail!("Unknown alpha operation {v}"),
    };
    ensure!(
        attr(child(pe, "DstAlpha")?, "Enable")? == "false",
        "Destination alpha override is not yet reproduced"
    );
    let z = child(pe, "ZCompare")?;
    let blend = child(pe, "Blend")?;
    let blend = match attr(blend, "Type")? {
        "None" => None,
        "Blend" => Some((
            factor(attr(blend, "SrcFactor")?)?,
            factor(attr(blend, "DstFactor")?)?,
            glow::FUNC_ADD,
        )),
        "Subtract" => Some((glow::ONE, glow::ONE, glow::FUNC_REVERSE_SUBTRACT)),
        v => bail!("Blend mode {v} is not yet reproduced"),
    };
    let cull = match attr(child(material, "DisplayFace")?, "DisplayFace")? {
        "Front" => Some(glow::BACK),
        "Back" => Some(glow::FRONT),
        "Both" => None,
        v => bail!("Unknown display face {v}"),
    };
    static COMMON_VERTEX: std::sync::OnceLock<Arc<str>> = std::sync::OnceLock::new();
    let vertex: Arc<str> = if uses_toon && rgb != "v_color.rgb" {
        Arc::from(VERTEX.replace(
            "clamp(v_color.rgb,0.0,1.0)",
            &format!("clamp({rgb},0.0,1.0)"),
        ))
    } else {
        COMMON_VERTEX.get_or_init(|| Arc::from(VERTEX)).clone()
    };
    Ok(Program {
        runtime_stages:combiner.stages,
        linear_clip_source:Some(Arc::downgrade(&vertex)),vertex,
        uv_sets:{let mut sets=[0;8];for(i,map)in maps.children().filter(|n|n.has_tag_name("TextureMap")).enumerate(){
            ensure!(i<8,"Too many GX texture maps");let coord=child(map,"TexCoordGen")?;
            if attr(coord,"Method")?=="UV" {sets[i]=attr(coord,"UVNumber")?.parse()?;ensure!(sets[i]<8,"Invalid GX UV channel");}
        }sets},
        pinned_projection:body.contains("bw_projection_bound"),
        template_samplers:maps.children().filter(|n|n.has_tag_name("TextureMap"))
            .map(|n|n.children().find(|n|n.has_tag_name("ImageAttr")).map(template_sampler).transpose())
            .collect::<Result<Vec<_>>>()?,
        texture_names: maps.children().filter(|n|n.has_tag_name("TextureMap"))
            .map(|n|attr(n,"ImageName").map(str::to_owned)).collect::<Result<_>>()?,
        register_defaults:dynamic.then_some(combiner.register_values),
        pass_constants:[None;4],
        fragment:format!("#version 330 core\nin vec2 v_uv;in vec2 v_environment;in vec3 v_projection;in vec4 v_color;in vec2 v_toon;out vec4 frag_color;uniform sampler2D u_tex[8];uniform mat3 u_tex_uv[8];uniform bool bw_indirect_override;uniform ivec3 bw_indirect[6];uniform vec3 bw_projection_bound;uniform int u_tex_uv_set[8];in vec2 v_extra_uv[7];vec2 bw_uv(int slot){{int channel=u_tex_uv_set[slot];return channel==0?v_uv:v_extra_uv[channel-1];}}\n{}\nvoid main(){{vec4 stage_tex[16];vec4 stage_ras[16];{body}vec4 result=bt_combiner(stage_tex,stage_ras,vec4(0.0));float alpha=floor(clamp(result.a,0.0,1.0)*255.0+0.5);if(!(({a}) {op} ({b})))discard;frag_color=clamp(result,0.0,1.0);}}",combiner.glsl),
        uv_textures:uv_textures.into_iter().collect(),
        texture_slots:texture_slots.into_iter().collect(),
        textured,depth_test:attr(z,"Enable")?=="true",depth_write:attr(z,"Update")?=="true",depth_func:comparison(attr(z,"Func")?)?,blend,cull,
    })
}

pub struct Source {
    text: Result<Arc<str>, String>,
    variants: [std::sync::OnceLock<Result<Arc<Program>, String>>; 2],
    /// Deduplicate identical R3MT programs within one import. This cache is
    /// owned by its Sources collection so dropping a world releases programs
    /// and lets the GL cache retire their GPU objects.
    cache: Arc<std::sync::Mutex<std::collections::HashMap<(Arc<str>, bool, bool), Result<Arc<Program>, String>>>>,
    native: bool,
    name: String,
    /// None uses the generic authoring preview; Some uses EM1's runtime path.
    native_support: Option<Result<bool, String>>,
}
impl Source {
    fn variant(&self, index: usize) -> Result<Arc<Program>, String> {
        let slot = self
            .variants
            .get(index)
            .ok_or_else(|| format!("{}: invalid material variant {index}", self.name))?;
        slot.get_or_init(|| {
            let text = self.text.as_ref().map_err(Clone::clone)?;
            let baked = index == 1;
            let key = (text.clone(), baked, self.native);
            let mut cache = self.cache.lock().map_err(|_| "Material program cache was poisoned".to_owned())?;
            if let Some(result) = cache.get(&key) {
                return result.clone();
            }
            // Compile on first actual use. Most scene packs carry a large
            // shared material library, but a scene references only a small
            // fraction of its R3MT files. Eagerly compiling both variants
            // inflated load-time memory. The cache is import-scoped, rather
            // than stored in egui Context (whose temporary data persists
            // until explicitly removed), to avoid retaining every level's
            // shaders forever.
            let result = compile_register_mode(
                text,
                baked,
                &crate::tev::Registers::default(),
                self.native,
                self.native.then_some(index == 1),
            )
            .map(Arc::new)
            .map_err(|error| format!("{}: {error:#}", self.name));
            cache.insert(key, result.clone());
            result
        })
        .clone()
    }
}
pub type Sources = std::collections::HashMap<String, Source>;
/// Index once per viewport, not once per mesh. Cache compilation by complete
/// source content so project edits never reuse stale programs.
pub fn sources(
    ctx: &egui::Context,
    files: &[epic_mickey_lib_rs::packfile::VirtualFile],
) -> Sources {
    collect_sources(ctx, files, false)
}
pub fn native_sources(
    ctx: &egui::Context,
    files: &[epic_mickey_lib_rs::packfile::VirtualFile],
) -> Sources {
    collect_sources(ctx, files, true)
}
fn native_support(text: &str) -> Result<bool> {
    let doc = roxmltree::Document::parse(text)?;
    let material = child(doc.root_element(), "Material")?;
    let mut support = true; // REL 0x166784 constructor default.
    if let Some(list) = material.children().find(|n| n.has_tag_name("UserDataList")) {
        for field in list.children().filter(|n| {
            n.has_tag_name("StringUserData")
                && n.attribute("Name") == Some("SupportsHardwareLighting")
        }) {
            let Some(value) = field
                .children()
                .find(|n| n.has_tag_name("Value"))
                .and_then(|n| n.text())
            else {
                continue;
            };
            let bytes = value.as_bytes();
            ensure!(!bytes.is_empty(), "Empty SupportsHardwareLighting value");
            // REL 0x168cc8: two-or-more characters test the second byte
            // against 'a'; single-character values test against '0'.
            support = if bytes.len() > 1 {
                bytes[1] != b'a'
            } else {
                bytes[0] != b'0'
            };
        }
    }
    Ok(support)
}
/// Runtime particle meshes use the same EM1 vertex-property replacement as
/// ordinary meshes; their authored Channel XML does not select GX lighting.
pub fn compile_native_with_registers(
    text: &str,
    bindings: &super::nif::MaterialBindings,
    registers: &crate::tev::Registers,
) -> Result<Program> {
    let flags = bindings
        .vertex_color_flags
        .clone()
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let state = crate::blotworx::raster_lighting::material_vertex_state(
        native_support(text)?,
        bindings.integers.get("R3MTLightingFlags").copied(),
        flags,
    );
    compile_register_mode(
        text,
        false,
        registers,
        true,
        Some(state.mesh_flag && bindings.has_vertex_colors),
    )
}
fn collect_sources(
    _ctx: &egui::Context,
    files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    native: bool,
) -> Sources {
    let mut sources = Sources::new();
    let cache = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    for file in files {
        if !file.path.to_ascii_lowercase().ends_with(".r3mt") {
            continue;
        }
        let path = file.path.replace('\\', "/");
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .to_ascii_lowercase()
            .trim_end_matches(".r3mt")
            .to_owned();
        let variants = std::array::from_fn(|_| std::sync::OnceLock::new());
        let native_support = native.then(|| {
            std::str::from_utf8(&file.data)
                .map_err(|e| e.to_string())
                .and_then(|text| native_support(text).map_err(|e| format!("{e:#}")))
        });
        let text = std::str::from_utf8(&file.data)
            .map(Arc::<str>::from)
            .map_err(|e| e.to_string());
        sources.insert(
            name.clone(),
            Source {
                text,
                variants,
                cache: cache.clone(),
                native,
                name,
                native_support,
            },
        );
    }
    sources
}
pub fn prepare(
    bindings: &super::nif::MaterialBindings,
    sources: &Sources,
    baked: bool,
) -> Result<Arc<Program>, String> {
    let Some(name) = bindings.shader.as_ref() else {
        return super::wii_default::compile_unlit_mesh(bindings)
            .map(Arc::new)
            .map_err(|error| format!("Default Wii mesh shader: {error:#}"));
    };
    let source = sources
        .get(&name.to_ascii_lowercase())
        .ok_or_else(|| format!("{name}: material definition unavailable"))?;
    let variant = if let Some(support) = &source.native_support {
        let flags = bindings.vertex_color_flags.clone().transpose()?;
        let state = crate::blotworx::raster_lighting::material_vertex_state(
            support.clone()?,
            bindings.integers.get("R3MTLightingFlags").copied(),
            flags,
        );
        // Baked application updates an existing COLOR stream; it does not
        // authorize synthesizing a missing color stream.
        (state.mesh_flag && bindings.has_vertex_colors) as usize
    } else {
        baked as usize
    };
    source.variant(variant)
}
/// Dynamic object-property overrides use uniforms, while retaining the same
/// EM1 vertex-color/lighting selection as the ordinary material path.
pub fn prepare_uniform(
    bindings: &super::nif::MaterialBindings,
    sources: &Sources,
    baked: bool,
) -> Result<Arc<Program>, String> {
    let name = bindings.shader.as_ref().ok_or("No game shader assigned")?;
    let source = sources
        .get(&name.to_ascii_lowercase())
        .ok_or_else(|| format!("{name}: material definition unavailable"))?;
    let variant = if let Some(support) = &source.native_support {
        let flags = bindings.vertex_color_flags.clone().transpose()?;
        let state = crate::blotworx::raster_lighting::material_vertex_state(
            support.clone()?,
            bindings.integers.get("R3MTLightingFlags").copied(),
            flags,
        );
        state.mesh_flag && bindings.has_vertex_colors
    } else {
        baked
    };
    compile_register_mode(
        source.text.as_ref().map_err(Clone::clone)?,
        variant,
        &crate::tev::Registers::default(),
        true,
        source.native_support.as_ref().map(|_| variant),
    )
    .map(Arc::new)
    .map_err(|e| format!("{name}: {e:#}"))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires original EM1_SHADER_FIXTURES"]
    fn original_interior_projection_retains_scale_pinning_and_indirect_maps() -> anyhow::Result<()>
    {
        let root = std::path::PathBuf::from(
            std::env::var_os("EM1_SHADER_FIXTURES")
                .ok_or_else(|| anyhow::anyhow!("Set EM1_SHADER_FIXTURES"))?,
        );
        for name in ["GEN_insideSurface", "GEN_insideSurface_hack"] {
            let text = std::fs::read_to_string(root.join(format!("{name}.r3mt")))?;
            let program = super::compile_register_mode(
                &text,
                false,
                &crate::tev::Registers::default(),
                false,
                Some(true),
            )?;
            assert!(program.pinned_projection);
            assert!(program.fragment.contains("bw_projection_bound.xy"));
            assert!(program.fragment.contains("vec2(0.25,0.25)"));
            assert!(program.texture_slots.contains(&0) && program.texture_slots.contains(&1));
            assert!(program.fragment.contains(".abg"));
            let unpinned = text.replace("<Value>true</Value>", "<Value>false</Value>");
            assert!(
                !super::compile_register_mode(
                    &unpinned,
                    false,
                    &crate::tev::Registers::default(),
                    false,
                    Some(true)
                )?
                .pinned_projection
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1_SHADER_FIXTURES"]
    fn original_warp_materials_bind_indirect_maps_and_generate_integer_coordinates(
    ) -> anyhow::Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("EM1_SHADER_FIXTURES")
                .ok_or_else(|| anyhow::anyhow!("Set EM1_SHADER_FIXTURES"))?,
        );
        for (name, slot) in [
            ("CHAR_Spatter_Goopy", 3),
            ("thinner_stagnent_top", 1),
            ("thinner_stagnent_bottom", 1),
            ("Fog_GroundCover_TwoTone", 3),
        ] {
            let text = std::fs::read_to_string(root.join(format!("{name}.r3mt")))?;
            let program = super::compile_register_mode(
                &text,
                false,
                &crate::tev::Registers::default(),
                false,
                Some(true),
            )?;
            assert!(
                program.texture_slots.contains(&slot),
                "{name} omitted its indirect resource"
            );
            assert!(
                program.fragment.contains(".abg"),
                "{name} lost indirect channel order"
            );
            assert!(
                program
                    .fragment
                    .contains("bw_ind_coord=(bw_ind_coord<<8)>>8"),
                "{name} omitted coordinate overflow"
            );
            assert!(
                program.fragment.contains(">>3)<<1"),
                "{name} omitted original matrix/exponent stages"
            );
        }
        Ok(())
    }
    #[test]
    fn em1_runtime_ignores_channel_xml_and_requires_real_vertex_colors() {
        let source = super::super::shader::new_shader();
        let start = source.find("<Channel").unwrap();
        let end = source[start..].find("</Channel>").unwrap() + start + "</Channel>".len();
        let mut native = source.clone();
        native.replace_range(start..end, "<Channel/>");
        let registers = crate::tev::Registers::default();
        for vertex in [false, true] {
            let before =
                super::compile_register_mode(&source, false, &registers, false, Some(vertex))
                    .unwrap();
            let after =
                super::compile_register_mode(&native, false, &registers, false, Some(vertex))
                    .unwrap();
            assert_eq!(before.fragment, after.fragment);
        }
        let files = vec![epic_mickey_lib_rs::packfile::VirtualFile::new(
            epic_mickey_lib_rs::packfile::EndianDependentString::new("r3mt".into()),
            false,
            0,
            "test.r3mt".into(),
            native.into_bytes(),
        )];
        let sources = super::native_sources(&egui::Context::default(), &files);
        let source = &sources["test"];
        assert!(source
            .variants
            .iter()
            .all(|variant| variant.get().is_none()));
        let mut bindings = super::super::nif::MaterialBindings::default();
        bindings.shader = Some("test".into());
        bindings.integers.insert("R3MTLightingFlags".into(), 3);
        let black = super::prepare(&bindings, &sources, false).unwrap();
        assert!(source.variants[0].get().is_some());
        assert!(source.variants[1].get().is_none());
        assert!(black.fragment.contains("vec4(vec3(0.0),1.0)"));
        bindings.has_vertex_colors = true;
        let vertex = super::prepare(&bindings, &sources, false).unwrap();
        assert!(source.variants[1].get().is_some());
        assert!(vertex.fragment.contains("vec4(v_color.rgb,v_color.a)"));
        bindings.integers.insert("R3MTLightingFlags".into(), 0);
        assert!(std::sync::Arc::ptr_eq(
            &black,
            &super::prepare(&bindings, &sources, true).unwrap()
        ));
        bindings.vertex_color_flags = Some(Err("Malformed property".into()));
        assert!(super::prepare(&bindings, &sources, false).is_err());
        for (value, expected) in [("false", false), ("true", true), ("0", false), ("1", true)] {
            let xml=format!("<Root><Material><UserDataList><StringUserData Name=\"SupportsHardwareLighting\"><Value>{value}</Value></StringUserData></UserDataList></Material></Root>");
            assert_eq!(super::native_support(&xml).unwrap(), expected);
        }
    }
    #[test]
    fn shader_program_cache_is_shared_within_import_and_released_with_it() {
        let text = super::super::shader::new_shader().into_bytes();
        let make_file = |path: &str| {
            epic_mickey_lib_rs::packfile::VirtualFile::new(
                epic_mickey_lib_rs::packfile::EndianDependentString::new("r3mt".into()),
                false,
                0,
                path.into(),
                text.clone(),
            )
        };
        let files = vec![make_file("a.r3mt"), make_file("b.r3mt")];
        let context = egui::Context::default();
        let first_import = super::native_sources(&context, &files);
        let first = first_import["a"].variant(0).unwrap();
        let same_import = first_import["b"].variant(0).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &same_import));
        let weak = std::sync::Arc::downgrade(&first);
        drop(first);
        drop(same_import);
        drop(first_import);
        assert!(weak.upgrade().is_none(), "egui Context retained the compiled material");

        let second_import = super::native_sources(&context, &files);
        let reloaded = second_import["a"].variant(0).unwrap();
        assert!(weak.upgrade().is_none(), "egui Context retained the previous import's program");
        assert!(!reloaded.fragment.is_empty());
    }
    #[test]
    fn template_sampler_preserves_native_filter_conversion() {
        let xml =
            r#"<ImageAttr WarpS="Clamp" WarpT="Mirror" MinFilter="Linear" MagFilter="Near"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let sampler = super::template_sampler(doc.root_element()).unwrap();
        assert_eq!(sampler.wrap_u, glow::CLAMP_TO_EDGE);
        assert_eq!(sampler.wrap_v, glow::MIRRORED_REPEAT);
        assert_eq!(sampler.min_filter, glow::LINEAR_MIPMAP_NEAREST);
        assert_eq!(sampler.mag_filter, glow::LINEAR);
        let xml = xml.replace("MagFilter=\"Near\"", "MagFilter=\"Linear\"");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(
            super::template_sampler(doc.root_element())
                .unwrap()
                .min_filter,
            glow::LINEAR
        );
    }
    #[test]
    fn native_uv_transform_keeps_em1_translation_and_rotation_conventions() {
        use super::native_uv_matrix;
        let apply = |m: [f32; 9], p: [f32; 2]| {
            [
                m[0] * p[0] + m[3] * p[1] + m[6],
                m[1] * p[0] + m[4] * p[1] + m[7],
            ]
        };
        assert_eq!(
            native_uv_matrix([1.; 2], 0., [0.; 2]).unwrap(),
            [1., 0., 0., 0., 1., 0., 0., 0., 1.]
        );
        assert_eq!(
            apply(
                native_uv_matrix([2., 3.], 0., [0.25, 0.125]).unwrap(),
                [0.5, 0.5]
            ),
            [0.5, -0.125]
        );
        let rotated = apply(
            native_uv_matrix([1.; 2], 90., [0.; 2]).unwrap(),
            [0.25, 0.75],
        );
        assert!((rotated[0] - 0.75).abs() < 0.000001 && (rotated[1] - 0.75).abs() < 0.000001);
        assert!(native_uv_matrix([1.; 2], f32::NAN, [0.; 2]).is_err());
        assert!(native_uv_matrix([f32::MAX; 2], 0., [f32::MAX; 2]).is_err());
    }

    use super::*;
    #[test]
    fn missing_mesh_map_uses_template_resource_but_never_replaces_authored_map() {
        use super::super::{
            nif::{MaterialBindings, MaterialTexture},
            viewport3d::{DecodedImage, TextureSampler},
        };
        let mut program = compile(&super::super::shader::new_shader(), false).unwrap();
        program.texture_slots = vec![0];
        program.texture_names = vec!["GlintMap".into()];
        let mut sampler = TextureSampler::default();
        sampler.wrap_u = glow::MIRRORED_REPEAT;
        program.template_samplers = vec![Some(sampler)];
        let pixels = Arc::new(vec![1, 2, 3, 255]);
        let mut cache = std::collections::HashMap::from([(
            "effects/r3mt/glintmap_tex.nif_wii".into(),
            Some(DecodedImage {
                rgba: pixels.clone(),
                width: 1,
                height: 1,
            }),
        )]);
        let mut bindings = MaterialBindings::default();
        let resolved = images(&program, &bindings, &[], &mut cache).unwrap();
        assert!(Arc::ptr_eq(&resolved[0].rgba, &pixels));
        assert_eq!(resolved[0].sampler.wrap_u, glow::MIRRORED_REPEAT);
        bindings.textures.push(MaterialTexture {
            shader_id: None,
            slot: 0,
            path: "custom.nif".into(),
            uv_set: 0,
            sampler: Default::default(),
            transform: None,
        });
        // A broken explicit override must remain an error, not use the default.
        assert!(images(&program, &bindings, &[], &mut cache)
            .unwrap_err()
            .contains("custom.nif"));
        bindings.textures.clear();
        program.texture_names[0] = "defaultSH_32".into();
        assert!(images(&program, &bindings, &[], &mut cache)
            .unwrap_err()
            .contains("defaultsh_32_tex.nif_wii"));
        cache.insert(
            "effects/r3mt/defaultsh_32_tex.nif_wii".into(),
            Some(DecodedImage {
                rgba: pixels.clone(),
                width: 1,
                height: 1,
            }),
        );
        assert!(Arc::ptr_eq(
            &images(&program, &bindings, &[], &mut cache).unwrap()[0].rgba,
            &pixels
        ));
        let generated = MaterialImage {
            slot: 0,
            rgba: Arc::new(vec![9, 8, 7, 255]),
            width: 1,
            height: 1,
            sampler: Default::default(),
            matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        };
        let overridden =
            images_with_runtime_sh(&program, &bindings, &[], &mut cache, Some(&generated)).unwrap();
        assert!(Arc::ptr_eq(&overridden[0].rgba, &generated.rgba));
        program.texture_names[0] = "GlintMap".into();
        program.template_samplers[0] = None;
        assert!(images(&program, &bindings, &[], &mut cache)
            .unwrap_err()
            .contains("template sampler"));
    }
    #[test]
    fn animated_register_values_do_not_change_shader_source() {
        let source = super::super::shader::new_shader();
        let defaults = crate::tev::Registers::default();
        let first = compile_uniform_registers(&source, false, &defaults).unwrap();
        let mut changed = crate::tev::Registers::default();
        changed.colors[2] = Some([1, 2, 3, 4]);
        changed.constants[0] = Some([5, 6, 7, 8]);
        let second = compile_uniform_registers(&source, false, &changed).unwrap();
        assert_eq!(first.fragment, second.fragment);
        assert_eq!(
            second.register_defaults.unwrap()[2],
            [1., 2., 3., 4.].map(|v| v / 255.)
        );
        assert_eq!(
            second.register_defaults.unwrap()[3],
            [5., 6., 7., 8.].map(|v| v / 255.)
        );
        assert!(second.fragment.contains("r[3] = bt_colors[2]"));
        assert!(second.fragment.contains("k[0] = bt_constants[0]"));
        assert!(compile(&source, false).unwrap().register_defaults.is_none());
    }
    #[test]
    fn draw_pass_constants_win_without_mutating_object_or_shared_defaults() {
        let mut program = compile_uniform_registers(
            &super::super::shader::new_shader(),
            false,
            &crate::tev::Registers::default(),
        )
        .unwrap();
        let defaults = program.register_defaults.unwrap();
        let mut object = defaults;
        object[1] = [0.1, 0.2, 0.3, 0.4];
        object[4] = [0.5, 0.6, 0.7, 1.];
        object[6] = [0.8; 4];
        program.pass_constants[1] = Some([0.02, 0.3, 0.98, 32. / 255.]);
        let presented = program.presentation_registers(Some(&object)).unwrap();
        assert_eq!(presented[4], program.pass_constants[1].unwrap());
        assert_eq!(presented[1], object[1]);
        assert_eq!(presented[6], object[6]);
        assert_eq!(object[4], [0.5, 0.6, 0.7, 1.]);
        assert_eq!(program.register_defaults.unwrap(), defaults);
        program.pass_constants = [None; 4];
        assert_eq!(
            program.presentation_registers(Some(&object)).unwrap(),
            object
        );
        let fixed = compile(&super::super::shader::new_shader(), false).unwrap();
        assert!(fixed.presentation_registers(Some(&object)).is_none());
    }
    #[test]
    fn texture_order_can_select_a_secondary_input() {
        let map="<TextureMap ImageName=\"defaultSH_32\"><TexCoordGen Method=\"UV\" UVNumber=\"0\" ScaleS=\"1\" ScaleT=\"1\" Rotate=\"0\" TransS=\"0\" TransT=\"0\"/></TextureMap>";
        let source = super::super::shader::new_shader()
            .replace(
                "<TextureMaps Applyable=\"true\"/>",
                &format!("<TextureMaps>{map}{map}</TextureMaps>"),
            )
            .replace(
                "TexMapOrder=\"-1\" TexCoordOrder=\"-1\"",
                "TexMapOrder=\"1\" TexCoordOrder=\"1\"",
            )
            .replace("A=\"RasC\"", "A=\"TexC\"");
        let program = compile(&source, false).unwrap();
        assert_eq!(program.texture_slots, vec![1]);
        let mut bindings = super::super::nif::MaterialBindings::default();
        let generated = MaterialImage {
            slot: 1,
            rgba: Arc::new(vec![0; 16 * 16 * 4]),
            width: 16,
            height: 16,
            sampler: Default::default(),
            matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        };
        // Slot zero is unused by TEV but still wins the native attachment scan.
        assert_eq!(program.first_runtime_sh_slot(&bindings), Some(0));
        assert!(images_with_runtime_sh(
            &program,
            &bindings,
            &[],
            &mut Default::default(),
            Some(&generated)
        )
        .is_err());
        let mut single = program.clone();
        single.texture_names[0] = "other".into();
        let images = images_with_runtime_sh(
            &single,
            &bindings,
            &[],
            &mut Default::default(),
            Some(&generated),
        )
        .unwrap();
        assert_eq!(images.len(), 1);
        assert!(Arc::ptr_eq(&images[0].rgba, &generated.rgba));
        let mut malformed = generated.clone();
        malformed.width = 0;
        assert!(images_with_runtime_sh(
            &single,
            &bindings,
            &[],
            &mut Default::default(),
            Some(&malformed)
        )
        .is_err());
        assert!(program.is_runtime_sh(1, &bindings));
        assert_eq!(program.image_name(1, &bindings), Some("defaultSH_32"));
        bindings.textures.push(super::super::nif::MaterialTexture {
            shader_id: None,
            uv_set: 0,
            slot: 1,
            path: "custom_texture.nif".into(),
            sampler: Default::default(),
            transform: None,
        });
        bindings.integers.insert("SphericalHarmonicMap".into(), 1);
        assert!(!program.is_runtime_sh(1, &bindings));
        bindings.textures[0].path = "Effects\\R3MT\\DefaultSH_32_Tex.nif".into();
        assert!(program.is_runtime_sh(1, &bindings));
        assert!(!program.is_runtime_sh(2, &bindings));
        let environment = source.replace("Method=\"UV\"", "Method=\"Environ\" CameraIndex=\"-1\"");
        let env = compile(&environment, false).unwrap();
        assert!(env.uv_textures.is_empty());
        assert!(env.fragment.contains("vec3(v_environment,1.0)"));
        for index in ["0", "1"] {
            assert_eq!(
                compile(
                    &environment.replace("CameraIndex=\"-1\"", &format!("CameraIndex=\"{index}\"")),
                    false
                )
                .unwrap()
                .fragment,
                env.fragment
            );
        }
        let projected = environment.replace("Method=\"Environ\"", "Method=\"Projection\"");
        assert!(compile(&projected, false)
            .unwrap()
            .fragment
            .contains("v_projection"));

        for index in ["0", "1"] {
            assert_eq!(
                compile(
                    &projected.replace("CameraIndex=\"-1\"", &format!("CameraIndex=\"{index}\"")),
                    false
                )
                .unwrap()
                .fragment,
                compile(&projected, false).unwrap().fragment
            );
        }
        assert!(program.fragment.contains("textureProj(u_tex[1],"));
        assert!(compile(
            &source.replace("TexCoordOrder=\"1\"", "TexCoordOrder=\"2\""),
            false
        )
        .is_err());
    }
    #[test]
    fn standalone_material_uses_vertex_colour_and_authored_alpha_test() {
        let source = super::super::shader::new_shader();
        let material = compile(&source, false).unwrap();
        assert!(!material.textured);
        assert!(material.fragment.contains("v_color.rgb"));
        assert!(material.fragment.contains("bt_combiner(stage_tex"));
        assert!(material.depth_write);
        assert!(compile(
            &source.replace("Comp0=\"Always\"", "Comp0=\"Invalid\""),
            false
        )
        .is_err());
        assert_eq!(alpha_test("Gequal", "128").unwrap(), "(alpha >= 128.0)");
        assert!(alpha_test("Less", "256").is_err());
    }
    #[test]
    #[ignore = "Requires EM1_SHADER_FIXTURES retail material directory"]
    fn export_retail_gpu_materials() -> Result<()> {
        let root = std::env::var("EM1_SHADER_FIXTURES")?;
        let out = std::path::Path::new("target/material-glsl");
        std::fs::create_dir_all(out)?;
        std::fs::write(out.join("material.vert"), VERTEX)?;
        let mut supported = 0;
        let mut limitations = std::collections::BTreeMap::new();
        for file in std::fs::read_dir(root)? {
            let file = file?.path();
            if file.extension().is_none_or(|s| s != "r3mt") {
                continue;
            }
            let text = std::fs::read_to_string(&file)?;
            match compile(&text, true) {
                Ok(program) => {
                    std::fs::write(
                        out.join(file.file_stem().unwrap()).with_extension("frag"),
                        program.fragment,
                    )?;
                    supported += 1;
                }
                Err(e) => {
                    limitations.insert(
                        file.file_name().unwrap().to_string_lossy().into_owned(),
                        format!("{e:#}"),
                    );
                }
            }
        }
        std::fs::write(
            out.join("limitations.json"),
            serde_json::to_vec_pretty(&limitations)?,
        )?;
        println!("{supported} supported static/baked GPU material definitions; {} explicitly unsupported",limitations.len());
        assert!(supported > 0);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct MaterialImage {
    pub slot: u8,
    pub rgba: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub sampler: super::viewport3d::TextureSampler,
    pub matrix: [f32; 9],
}
pub fn images(
    program: &Program,
    bindings: &super::nif::MaterialBindings,
    files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    cache: &mut std::collections::HashMap<String, Option<super::viewport3d::DecodedImage>>,
) -> Result<Vec<MaterialImage>, String> {
    images_with_runtime_sh(program, bindings, files, cache, None)
}

/// Supply the resolved property's generated image without inserting pixels
/// into the immutable asset cache. The caller supplies template sampler state
/// when no model map exists; an authored model map retains its own state.
pub fn images_with_runtime_sh(
    program: &Program,
    bindings: &super::nif::MaterialBindings,
    files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    cache: &mut std::collections::HashMap<String, Option<super::viewport3d::DecodedImage>>,
    runtime: Option<&MaterialImage>,
) -> Result<Vec<MaterialImage>, String> {
    if let Some(image) = runtime {
        if program.first_runtime_sh_slot(bindings) != Some(u32::from(image.slot)) {
            return Err("Generated SH image must target the first matching shader map".into());
        }
        let expected = u64::from(image.width) * u64::from(image.height) * 4;
        if image.width == 0 || image.height == 0 || expected != image.rgba.len() as u64 {
            return Err("Invalid generated SH image dimensions".into());
        }
    }
    let mut images = Vec::new();
    for &slot in &program.texture_slots {
        if let Some(image) = runtime.filter(|image| image.slot == slot) {
            let mut image = image.clone();
            if let Some(binding) = bindings
                .textures
                .iter()
                .rev()
                .find(|t| t.slot == u32::from(slot))
            {
                if binding.uv_set >= 8 && program.uv_textures.contains(&slot) {
                    return Err(format!("Texture {slot} needs UV set {}", binding.uv_set));
                }
                image.sampler = binding.sampler;
                image.matrix = binding
                    .transform
                    .and_then(|t| t.matrix())
                    .unwrap_or([1., 0., 0., 0., 1., 0., 0., 0., 1.]);
            } else if let Some(Some(sampler)) = program.template_samplers.get(slot as usize) {
                image.sampler = *sampler;
            }
            images.push(image);
            continue;
        }
        let binding = bindings
            .textures
            .iter()
            .rev()
            .find(|t| t.slot == slot as u32);
        if binding.is_none() {
            // A defaultSH name permits a later generated-map attachment; it
            // does not remove the shader's original texture resource. Without
            // an attached property, use the same native provider lookup as
            // every other template image (REL16ae84/176094).
            // REL 0x16ae84 lowercases ImageName, prepends the resource
            // provider path (0x160688: effects/r3mt/), then appends _tex.nif_wii.
            // Only absent model maps use this path (0x176094..0x1760f4).
            let name = program
                .texture_names
                .get(slot as usize)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| format!("Shader texture {slot} has no model or template image"))?;
            let path = format!("effects/r3mt/{}_tex.nif_wii", name.to_ascii_lowercase());
            let sampler = program
                .template_samplers
                .get(slot as usize)
                .copied()
                .flatten()
                .ok_or_else(|| format!("Shader texture {slot} has no template sampler"))?;
            let image = cache
                .entry(path.clone())
                .or_insert_with(|| {
                    // Template resources use their complete provider path. A
                    // same-named texture elsewhere must not mask a missing asset.
                    files
                        .iter()
                        .rev()
                        .find(|file| file.path.replace('\\', "/").eq_ignore_ascii_case(&path))
                        .and_then(|file| super::nif::decode_image_bytes(&file.data))
                        .map(|(rgba, width, height)| super::viewport3d::DecodedImage {
                            rgba: Arc::new(rgba),
                            width,
                            height,
                        })
                })
                .as_ref()
                .ok_or_else(|| format!("Template texture unavailable: {path}"))?;
            images.push(MaterialImage {
                slot,
                rgba: image.rgba.clone(),
                width: image.width,
                height: image.height,
                sampler,
                matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            });
            continue;
        }
        let binding = binding.unwrap();
        if binding.uv_set >= 8 && program.uv_textures.contains(&slot) {
            return Err(format!("Texture {slot} needs UV set {}", binding.uv_set));
        }
        let image = cache
            .entry(binding.path.clone())
            .or_insert_with(|| {
                super::nif::find_pak_texture(files, &binding.path)
                    .and_then(|f| super::nif::decode_image_bytes(&f.data))
                    .map(|(rgba, width, height)| super::viewport3d::DecodedImage {
                        rgba: Arc::new(rgba),
                        width,
                        height,
                    })
            })
            .as_ref()
            .ok_or_else(|| format!("Texture unavailable: {}", binding.path))?;
        images.push(MaterialImage {
            slot,
            rgba: image.rgba.clone(),
            width: image.width,
            height: image.height,
            sampler: binding.sampler,
            matrix: binding
                .transform
                .and_then(|t| t.matrix())
                .unwrap_or([1., 0., 0., 0., 1., 0., 0., 0., 1.]),
        });
    }
    Ok(images)
}

#[cfg(test)]
mod clip_provenance_tests {
    #[test]
    fn editing_a_vertex_program_invalidates_linear_position_provenance() -> anyhow::Result<()> {
        let mut program = super::compile(&super::super::shader::new_shader(), false)?;
        assert!(program.has_linear_clip_position());
        let clone = program.clone();
        assert!(clone.has_linear_clip_position());
        std::sync::Arc::make_mut(&mut program.vertex).make_ascii_uppercase();
        assert!(!program.has_linear_clip_position());
        assert!(clone.has_linear_clip_position());
        Ok(())
    }
}
