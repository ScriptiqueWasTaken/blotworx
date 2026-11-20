//! NintendoWare TEV color-combiner lowering. Sampling and lighting supply stage inputs.
use anyhow::{bail, ensure, Context, Result};
use roxmltree::Node;
/// Generated-function extension point. Composed base surfaces rename this
/// along with bt_combiner, so runtime effects attach only to the final pass.
pub const EFFECT_HOOK: &str = "vec4 bt_combiner_effect(vec4 previous,vec4 result){return result;}";
fn attr<'a>(n: Node<'a, 'a>, name: &str) -> Result<&'a str> {
    n.attribute(name)
        .with_context(|| format!("{} needs {name}", n.tag_name().name()))
}
fn child<'a>(n: Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>> {
    n.children()
        .find(|c| c.has_tag_name(name))
        .with_context(|| format!("{} needs {name}", n.tag_name().name()))
}
fn register(s: &str) -> Result<usize> {
    Ok(match s {
        "Prev" => 0,
        "Reg0" => 1,
        "Reg1" => 2,
        "Reg2" => 3,
        _ => bail!("Unknown output register {s}"),
    })
}
fn konst(s: &str, alpha: bool) -> Result<String> {
    if let Some(f) = s.strip_prefix('C').and_then(|s| s.strip_suffix("_8")) {
        let n: u8 = f.parse()?;
        ensure!((1..=8).contains(&n), "Invalid constant fraction");
        return Ok(format!(
            "{}.0 / 255.0",
            [32, 64, 96, 128, 159, 191, 223, 255][n as usize - 1]
        ));
    }
    let b = s.as_bytes();
    ensure!(
        (b.len() == 2 || b.len() == 3) && b[0] == b'K' && (b'0'..=b'3').contains(&b[1]),
        "Invalid constant selector {s}"
    );
    let i = b[1] - b'0';
    let channel = if b.len() == 2 {
        ensure!(!alpha, "Alpha needs a constant channel");
        "rgb"
    } else {
        match b[2] {
            b'R' => "r",
            b'G' => "g",
            b'B' => "b",
            b'A' => "a",
            _ => bail!("Invalid constant channel {s}"),
        }
    };
    Ok(format!("k[{i}].{channel}"))
}
fn operand(s: &str, alpha: bool, k: &str) -> Result<String> {
    let scalar = match s {
        "Zero" => Some("0.0"),
        "One" if !alpha => Some("1.0"),
        "Half" if !alpha => Some("128.0 / 255.0"),
        "APrev" => Some("r[0].a"),
        "A0" => Some("r[1].a"),
        "A1" => Some("r[2].a"),
        "A2" => Some("r[3].a"),
        "TexA" => Some("tex.a"),
        "RasA" => Some("ras.a"),
        _ => None,
    };
    let value = if let Some(v) = scalar {
        v.to_string()
    } else {
        match s {
            "Konst" => k.into(),
            "CPrev" if !alpha => "r[0].rgb".into(),
            "C0" if !alpha => "r[1].rgb".into(),
            "C1" if !alpha => "r[2].rgb".into(),
            "C2" if !alpha => "r[3].rgb".into(),
            "TexC" if !alpha => "tex.rgb".into(),
            "RasC" if !alpha => "ras.rgb".into(),
            _ => bail!(
                "Invalid {} operand {s}",
                if alpha { "alpha" } else { "color" }
            ),
        }
    };
    Ok(if alpha {
        value
    } else {
        format!("vec3({value})")
    })
}
fn expression(n: Node, alpha: bool) -> Result<(usize, String)> {
    let k = konst(attr(n, "KSel")?, alpha)?;
    let values = ["A", "B", "C", "D"].map(|a| operand(attr(n, a)?, alpha, &k));
    let [a, b, c, d] = values;
    let (a, b, c, d) = (a?, b?, c?, d?);
    let op = attr(n, "Op")?;
    let expr = match op {
        "Add" => format!("({d} + mix({a}, {b}, {c}))"),
        "Sub" => format!("({d} - mix({a}, {b}, {c}))"),
        "CompA8Gt" if alpha => format!("({d} + (({a} > {b}) ? {c} : 0.0))"),
        "CompA8Eq" if alpha => format!("({d} + (({a} == {b}) ? {c} : 0.0))"),
        "CompRgb8Gt" if !alpha => format!("({d} + mix(vec3(0.0), {c}, greaterThan({a}, {b})))"),
        "CompRgb8Eq" if !alpha => format!("({d} + mix(vec3(0.0), {c}, equal({a}, {b})))"),
        _ => bail!("Unsupported combiner operation {op}"),
    };
    let bias = match attr(n, "Bias")? {
        "Zero" => "0.0",
        "AddHalf" => "0.5",
        "SubHalf" => "-0.5",
        v => bail!("Invalid bias {v}"),
    };
    let scale = match attr(n, "Scale")? {
        "Scale1" => "1.0",
        "Scale2" => "2.0",
        "Scale4" => "4.0",
        "Divide2" => "0.5",
        v => bail!("Invalid scale {v}"),
    };
    let clamped = match attr(n, "Clamp")? {
        "true" => true,
        "false" => false,
        v => bail!("Invalid clamp flag {v}"),
    };
    // Comparison operations ignore bias/scale; hardware integer rounding remains separate.
    let expr = if matches!(op, "Add" | "Sub") {
        format!("(({expr} + {bias}) * {scale})")
    } else {
        expr
    };
    let (min, max) = if clamped {
        ("0.0", "1.0")
    } else {
        ("-1024.0 / 255.0", "1023.0 / 255.0")
    };
    Ok((
        register(attr(n, "OutReg")?)?,
        format!("clamp({expr}, {min}, {max})"),
    ))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterSource {
    Zero,
    Color0,
    Color1,
    AlphaBump,
    NormalizedAlphaBump,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageInput {
    pub texture: Option<u8>,
    pub coordinate: Option<u8>,
    pub raster: RasterSource,
    pub indirect: bool,
}
fn stage_input(n: Node) -> Result<StageInput> {
    let order = child(n, "Order")?;
    let index = |name| -> Result<Option<u8>> {
        let v: i32 = attr(order, name)?.parse()?;
        ensure!(
            (-1..=7).contains(&v),
            "{name} must be -1 or a texture slot from 0 to 7"
        );
        Ok((v >= 0).then_some(v as u8))
    };
    let raster = match attr(order, "RasOrder")? {
        "ColorNull" => RasterSource::Zero,
        "Color0A0" => RasterSource::Color0,
        "Color1A1" => RasterSource::Color1,
        "AlphaBump" => RasterSource::AlphaBump,
        "AlphaBumpN" => RasterSource::NormalizedAlphaBump,
        v => bail!("Unknown raster source {v}"),
    };
    let ind = child(n, "IndirectStage")?;
    let indirect = attr(ind, "MatrixSel")? != "ItmOff"
        || attr(ind, "WrapS")? != "ItwOff"
        || attr(ind, "WrapT")? != "ItwOff"
        || attr(ind, "AddPrev")? == "true"
        || matches!(
            raster,
            RasterSource::AlphaBump | RasterSource::NormalizedAlphaBump
        );
    Ok(StageInput {
        texture: index("TexMapOrder")?,
        coordinate: index("TexCoordOrder")?,
        raster,
        indirect,
    })
}
#[derive(Debug)]
pub struct Combiner {
    pub register_values: [[f32; 4]; 7],
    pub glsl: String,
    pub stages: usize,
    pub inputs: Vec<StageInput>,
}
/// Lower native Gamebryo GX stages through the same arithmetic convention as
/// R3MT. Texture samples, raster colors and konst selections are supplied by
/// the caller; this does not infer channel lighting or texture ownership.
/// Like the existing R3MT lowering, exact GX integer rounding remains separate.
pub fn compile_wii_default_stages(stages: &[crate::render_support::wii_default::Stage]) -> Result<String> {
    ensure!(
        (1..=16).contains(&stages.len()),
        "Expected1..16 native GX stages"
    );
    let mut glsl=format!("{EFFECT_HOOK}\nvec4 bt_default_combiner(vec4 stage_tex[16],vec4 stage_ras[16],vec4 stage_k[16],vec4 initial[4]){{\nvec4 r[4];for(int i=0;i<4;i++)r[i]=initial[i];\n");
    for (i, stage) in stages.iter().enumerate() {
        let color = |value: u32| -> Result<String> {
            Ok(match value {
                0 => "r[0].rgb",
                1 => "vec3(r[0].a)",
                2 => "r[1].rgb",
                3 => "vec3(r[1].a)",
                4 => "r[2].rgb",
                5 => "vec3(r[2].a)",
                6 => "r[3].rgb",
                7 => "vec3(r[3].a)",
                8 => "tex.rgb",
                9 => "vec3(tex.a)",
                10 => "ras.rgb",
                11 => "vec3(ras.a)",
                12 => "vec3(1.0)",
                13 => "vec3(128.0/255.0)",
                14 => "k.rgb",
                15 => "vec3(0.0)",
                _ => bail!("Invalid GX color operand {value}"),
            }
            .into())
        };
        let alpha = |value: u32| -> Result<String> {
            Ok(match value {
                0 => "r[0].a",
                1 => "r[1].a",
                2 => "r[2].a",
                3 => "r[3].a",
                4 => "tex.a",
                5 => "ras.a",
                6 => "k.a",
                7 => "0.0",
                _ => bail!("Invalid GX alpha operand {value}"),
            }
            .into())
        };
        let [a, b, c, d] = stage.color.map(color);
        let (a, b, c, d) = (a?, b?, c?, d?);
        let [aa, ab, ac, ad] = stage.alpha.map(alpha);
        let (aa, ab, ac, ad) = (aa?, ab?, ac?, ad?);
        let scale = |value| -> Result<&str> {
            Ok(match value {
                0 => "1.0",
                1 => "2.0",
                2 => "4.0",
                3 => "0.5",
                _ => bail!("Invalid GX scale {value}"),
            })
        };
        ensure!(stage.output < 4, "Invalid GX destination");
        // Read both channels before either write: alpha may sample RGB's old
        // register alpha, and RGB may sample alpha's destination register.
        glsl+=&format!("{{vec4 tex=stage_tex[{i}];vec4 ras=stage_ras[{i}];vec4 k=stage_k[{i}];\nvec3 next_rgb=clamp(({d}+mix({a},{b},{c}))*{},0.0,1.0);\nfloat next_alpha=clamp(({ad}+mix({aa},{ab},{ac}))*{},0.0,1.0);\nr[{}].rgb=next_rgb;r[{}].a=next_alpha;}}\n",scale(stage.color_scale)?,scale(stage.alpha_scale)?,stage.output,stage.output);
    }
    glsl += &format!(
        "return bt_combiner_effect(r[0],r[{}]);\n}}\n",
        stages.last().unwrap().output
    );
    Ok(glsl)
}
/// Generates arithmetic only. Each stage receives its correctly sampled texture and lit raster color.
/// This does not claim to implement indirect sampling, animation or exact GX integer rounding.
pub fn compile(text: &str) -> Result<Combiner> {
    compile_with_registers(text, &Registers::default())
}
#[derive(Default)]
pub struct Registers {
    pub colors: [Option<[u8; 4]>; 3],
    pub constants: [Option<[u8; 4]>; 4],
}
pub fn compile_with_registers(text: &str, overrides: &Registers) -> Result<Combiner> {
    compile_register_mode(text, overrides, false)
}
pub fn compile_uniform_registers(text: &str, overrides: &Registers) -> Result<Combiner> {
    compile_register_mode(text, overrides, true)
}
fn compile_register_mode(text: &str, overrides: &Registers, dynamic: bool) -> Result<Combiner> {
    let doc = roxmltree::Document::parse(text)?;
    let material = child(doc.root_element(), "Material")?;
    let tev = child(material, "Tev")?;
    let colors = child(tev, "ColorRegisters")?
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    ensure!(colors.len() == 3, "Expected three color registers");
    let constants = child(tev, "ConstantRegisters")?
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    ensure!(constants.len() == 4, "Expected four constant registers");
    let swaps = child(tev, "SwapTables")?
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    ensure!(swaps.len() == 4, "Expected four swap tables");
    let stages = child(tev, "Stages")?
        .children()
        .filter(|n| n.has_tag_name("Stage"))
        .collect::<Vec<_>>();
    ensure!((1..=16).contains(&stages.len()), "Expected 1–16 stages");
    let mut glsl=format!("{EFFECT_HOOK}\nvec4 bt_combiner(vec4 stage_tex[16], vec4 stage_ras[16], vec4 initial_prev) {{\nvec4 r[4];\nvec4 k[4];\nr[0] = initial_prev;\n");
    if dynamic {
        glsl = format!("uniform vec4 bt_colors[3];\nuniform vec4 bt_constants[4];\n{glsl}");
    }
    let mut register_values = [[0.; 4]; 7];
    for (array, nodes, start) in [("r", colors, 1), ("k", constants, 0)] {
        for (i, n) in nodes.iter().enumerate() {
            let mut values = vec![];
            for (channel, a) in ["R", "G", "B", "A"].into_iter().enumerate() {
                let override_value = if array == "k" {
                    overrides.constants[i]
                } else {
                    overrides.colors[i]
                };
                let v: i32 = if let Some(value) = override_value {
                    value[channel] as i32
                } else {
                    attr(*n, a)?.parse()?
                };
                ensure!(
                    (if array == "k" { 0..=255 } else { -1024..=1023 }).contains(&v),
                    "Register color is out of range"
                );
                register_values[if array == "k" { i + 3 } else { i }][channel] = v as f32 / 255.;
                values.push(format!("{v}.0 / 255.0"));
            }
            if dynamic {
                let uniform = if array == "k" {
                    "bt_constants"
                } else {
                    "bt_colors"
                };
                glsl += &format!("{array}[{}] = {uniform}[{i}];\n", i + start);
            } else {
                glsl += &format!("{array}[{}] = vec4({});\n", i + start, values.join(", "));
            }
        }
    }
    let mut output = (0, 0);
    let mut inputs = Vec::with_capacity(stages.len());
    for (i, n) in stages.iter().enumerate() {
        inputs.push(stage_input(*n).with_context(|| format!("Stage {} inputs", i + 1))?);
        glsl += "{\n";
        let swap = child(*n, "Swap")?;
        for (input, var, selector) in [
            ("stage_tex", "tex", "TexMapSwap"),
            ("stage_ras", "ras", "RasSwap"),
        ] {
            let idx: usize = attr(swap, selector)?.parse()?;
            let table = swaps.get(idx).context("Swap table index out of range")?;
            let mut swizzle = String::new();
            for a in ["R", "G", "B", "A"] {
                swizzle.push(match attr(*table, a)? {
                    "R" => 'r',
                    "G" => 'g',
                    "B" => 'b',
                    "A" => 'a',
                    v => bail!("Invalid swap channel {v}"),
                });
            }
            glsl += &format!("vec4 {var} = {input}[{i}].{swizzle};\n");
        }
        let (rgb, expr) = expression(child(*n, "ColorStage")?, false)
            .with_context(|| format!("Stage {} color", i + 1))?;
        glsl += &format!("vec3 next_rgb = {expr};\n");
        let (alpha, expr) = expression(child(*n, "AlphaStage")?, true)
            .with_context(|| format!("Stage {} alpha", i + 1))?;
        output = (rgb, alpha);
        glsl+=&format!("float next_alpha = {expr};\nr[{rgb}].rgb = next_rgb;\nr[{alpha}].a = next_alpha;\n}}\n");
    }
    glsl += &format!(
        "return bt_combiner_effect(r[0],vec4(r[{}].rgb, r[{}].a));\n}}\n",
        output.0, output.1
    );
    Ok(Combiner {
        register_values,
        glsl,
        stages: stages.len(),
        inputs,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiler_preserves_simultaneous_color_alpha_reads() {
        let source = crate::render_support::shader::new_shader();
        let c = compile(&source).unwrap();
        assert_eq!(c.stages, 1);
        assert!(
            c.glsl.find("float next_alpha").unwrap() < c.glsl.find("r[0].rgb = next_rgb").unwrap()
        );
        assert!(compile(&source.replace("Op=\"Add\"", "Op=\"Broken\"")).is_err());
    }
    #[test]
    fn final_output_uses_the_last_stage_destinations() {
        let text =
            crate::render_support::shader::new_shader().replace("OutReg=\"Prev\"", "OutReg=\"Reg2\"");
        assert!(compile(&text)
            .unwrap()
            .glsl
            .contains("return bt_combiner_effect(r[0],vec4(r[3].rgb, r[3].a))"));
        assert!(konst("C1.5_8", true).is_err());
        assert!(konst("K4", false).is_err());
    }
    #[test]
    #[ignore = "Requires EM1_SHADER_FIXTURES"]
    fn compile_retail_combiners() -> Result<()> {
        let mut count = 0;
        let root = std::env::var("EM1_SHADER_FIXTURES")?;
        std::fs::create_dir_all("target/tev-glsl")?;
        for file in std::fs::read_dir(root)? {
            let p = file?.path();
            if p.extension().is_some_and(|e| e == "r3mt") {
                let source = std::fs::read_to_string(&p)?;
                let compiled = compile(&source).with_context(|| p.display().to_string())?;
                ensure!(!compiled.glsl.is_empty(), "Empty shader");
                let shader=format!("#version 330 core\nuniform vec4 inputs_tex[16];\nuniform vec4 inputs_ras[16];\nout vec4 color;\n{}\nvoid main(){{color=bt_combiner(inputs_tex,inputs_ras,vec4(0.0));}}\n",compiled.glsl);
                std::fs::write(
                    std::path::Path::new("target/tev-glsl")
                        .join(p.file_name().unwrap())
                        .with_extension("frag"),
                    shader,
                )?;
                count += 1;
            }
        }
        ensure!(count == 168, "Expected all 168 retail shaders");
        Ok(())
    }
}
