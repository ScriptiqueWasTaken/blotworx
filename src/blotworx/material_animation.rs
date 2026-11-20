//! Authored NW4R texture-SRT playback. No shader recompilation per frame.
use crate::render_support::viewport3d::GpuDrawCommand;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::packfile::VirtualFile;
use std::{collections::BTreeMap, sync::Arc};

pub fn model_registers(
    nif: &epic_mickey_lib_rs::nif_file::NifFile,
) -> Result<BTreeMap<u32, [Option<[f32; 4]>; 7]>> {
    use epic_mickey_lib_rs::nif_file::NiObject;
    let mut result = BTreeMap::new();
    for (index, block) in nif.blocks.iter().enumerate() {
        let NiObject::NiMesh(mesh) = block else {
            continue;
        };
        let mut colors = [None; 7];
        for link in mesh.base.base.base.extra_data_links.iter().flatten() {
            let Some(NiObject::Unknown(raw)) = nif.blocks.get(*link as usize) else {
                continue;
            };
            if raw.type_name != "NiColorExtraData" {
                continue;
            }
            ensure!(
                raw.raw_data.len() == 20,
                "Invalid material color extra data"
            );
            let word = |at| {
                let b = raw.raw_data[at..at + 4].try_into().unwrap();
                if nif.source_is_little_endian {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                }
            };
            let name = nif
                .string_table
                .get(word(0) as usize)
                .context("Missing material color name")?;
            let slot = if let Some(slot) = name.strip_prefix("Color_Register_") {
                let s = slot.parse::<usize>()?;
                ensure!(s < 3, "Invalid regular register");
                Some(s)
            } else if let Some(slot) = name.strip_prefix("Constant_Color_") {
                let s = slot.parse::<usize>()?;
                ensure!(s < 4, "Invalid constant register");
                Some(3 + s)
            } else {
                None
            };
            if let Some(slot) = slot {
                let mut color = [0.; 4];
                for (i, value) in color.iter_mut().enumerate() {
                    let source = f32::from_bits(word(4 + i * 4)) * 255.;
                    ensure!(
                        source.is_finite()
                            && (-2147483648.0..2147483648.0).contains(&(source as f64)),
                        "Invalid material color channel"
                    );
                    *value = f32::from((source as i32) as u8) / 255.;
                }
                colors[slot] = Some(color);
            }
        }
        if colors.iter().any(Option::is_some) {
            result.insert(index as u32, colors);
        }
    }
    Ok(result)
}

#[derive(Clone)]
enum Curve {
    Constant(f32),
    Hermite(Vec<[f32; 3]>),
}
impl Curve {
    fn sample(&self, time: f32) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Hermite(keys) => {
                if time < keys[0][0] {
                    return keys[0][1];
                }
                let upper = keys.partition_point(|k| k[0] <= time);
                if upper == keys.len() {
                    return keys[keys.len() - 1][1];
                }
                let a = keys[upper - 1];
                let b = keys[upper];
                let span = b[0] - a[0];
                let t = (time - a[0]) / span;
                let t2 = t * t;
                let t3 = t2 * t;
                (2. * t3 - 3. * t2 + 1.) * a[1]
                    + (t3 - 2. * t2 + t) * span * a[2]
                    + (-2. * t3 + 3. * t2) * b[1]
                    + (t3 - t2) * span * b[2]
            }
        }
    }
}
struct Channel {
    indirect: bool,
    slot: u8,
    values: [Curve; 5],
    inverse_base: [f32; 9],
}
struct Animation {
    duration: f32,
    looping: bool,
    channels: Vec<Channel>,
    indirect_base: [[i32; 3]; 6],
}
pub struct Runtime {
    animations: BTreeMap<String, Animation>,
    pub errors: BTreeMap<String, String>,
}
fn stem(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}
fn multiply(a: [f32; 9], b: [f32; 9]) -> [f32; 9] {
    std::array::from_fn(|i| (0..3).map(|k| a[k * 3 + i % 3] * b[i / 3 * 3 + k]).sum())
}
fn inverse(m: [f32; 9]) -> Result<[f32; 9]> {
    let d = m[0] * m[4] - m[3] * m[1];
    ensure!(d.abs() > 1e-12, "Singular texture SRT base");
    let mut r = [m[4] / d, -m[1] / d, 0., -m[3] / d, m[0] / d, 0., 0., 0., 1.];
    r[6] = -(r[0] * m[6] + r[3] * m[7]);
    r[7] = -(r[1] * m[6] + r[4] * m[7]);
    Ok(r)
}
impl Animation {
    fn load(text: &str, material: &str) -> Result<Self> {
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}'))?;
        let info = doc
            .descendants()
            .find(|n| n.has_tag_name("tex_srt_anim_info"))
            .context("Missing SRT info")?;
        let duration = info
            .attribute("frame_size")
            .context("Missing SRT duration")?
            .parse::<f32>()?;
        ensure!(
            duration.is_finite() && duration > 0.,
            "Invalid SRT duration"
        );
        ensure!(
            info.attribute("tex_matrix_mode") == Some("maya"),
            "Unsupported SRT matrix convention"
        );
        let base = roxmltree::Document::parse(material.trim_start_matches('\u{feff}'))?;
        let maps = base
            .descendants()
            .filter(|n| n.has_tag_name("TexCoordGen"))
            .collect::<Vec<_>>();
        let indirect_maps = base
            .descendants()
            .filter(|n| n.has_tag_name("IndirectMap"))
            .collect::<Vec<_>>();
        let mut indirect_base = [[0; 3]; 6];
        for (i, map) in indirect_maps.iter().enumerate() {
            ensure!(i < 3, "Too many indirect matrices");
            let number = |name| -> Result<f32> {
                Ok(map
                    .attribute(name)
                    .context("Missing indirect SRT field")?
                    .parse()?)
            };
            let matrix = crate::render_support::material_preview::indirect::srt_matrix(
                [number("ScaleS")?, number("ScaleT")?],
                number("Rotate")?,
                [number("TransS")?, number("TransT")?],
            )?;
            indirect_base[i * 2..i * 2 + 2].copy_from_slice(&matrix);
        }
        let mut channels = Vec::new();
        for node in doc
            .descendants()
            .filter(|n| n.has_tag_name("tex_srt_anim") || n.has_tag_name("indirect_srt_anim"))
        {
            let indirect = node.has_tag_name("indirect_srt_anim");
            let slot = node
                .attribute(if indirect {
                    "indirect_matrix_index"
                } else {
                    "tex_matrix_index"
                })
                .context("Missing texture matrix index")?
                .parse::<u8>()?;
            let matrix = if indirect {
                ensure!(
                    (slot as usize) < indirect_maps.len(),
                    "Indirect SRT index outside material"
                );
                [1., 0., 0., 0., 1., 0., 0., 0., 1.]
            } else {
                let map = maps
                    .get(slot as usize)
                    .context("SRT matrix index outside material")?;
                ensure!(
                    map.attribute("Method") == Some("UV"),
                    "Animated non-UV material needs its camera-space update"
                );
                let number = |name| -> Result<f32> {
                    Ok(map
                        .attribute(name)
                        .context("Missing base SRT field")?
                        .parse()?)
                };
                crate::render_support::material_preview::native_uv_matrix(
                    [number("ScaleS")?, number("ScaleT")?],
                    number("Rotate")?,
                    [number("TransS")?, number("TransT")?],
                )?
            };
            let mut values = [
                Curve::Constant(1.),
                Curve::Constant(1.),
                Curve::Constant(0.),
                Curve::Constant(0.),
                Curve::Constant(0.),
            ];
            for target in node.children().filter(|n| {
                n.has_tag_name("tex_srt_anim_target") || n.has_tag_name("indirect_srt_anim_target")
            }) {
                let target_name = target.attribute("target").context("Missing SRT target")?;
                let index = match target_name
                    .strip_prefix("tex_")
                    .or_else(|| target_name.strip_prefix("indirect_"))
                    .unwrap_or(target_name)
                {
                    "scale_s" => 0,
                    "scale_t" => 1,
                    "rotate" => 2,
                    "translate_s" => 3,
                    "translate_t" => 4,
                    other => anyhow::bail!("Unsupported SRT channel {other}"),
                };
                if let Some(c) = target.children().find(|n| n.has_tag_name("anim_constant")) {
                    let v = c
                        .attribute("value")
                        .context("Missing SRT constant")?
                        .parse::<f32>()?;
                    ensure!(v.is_finite(), "Nonfinite SRT constant");
                    values[index] = Curve::Constant(v);
                } else if let Some(c) = target.children().find(|n| n.has_tag_name("anim_hermite")) {
                    let count = c
                        .attribute("size")
                        .context("Missing SRT key count")?
                        .parse::<usize>()?;
                    let numbers = c
                        .text()
                        .unwrap_or("")
                        .split_whitespace()
                        .map(str::parse::<f32>)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    ensure!(
                        count > 0
                            && numbers.len() == count * 3
                            && numbers.iter().all(|v| v.is_finite()),
                        "Invalid SRT keys"
                    );
                    let keys = numbers
                        .chunks_exact(3)
                        .map(|v| [v[0], v[1], v[2]])
                        .collect::<Vec<_>>();
                    ensure!(
                        keys.windows(2).all(|p| p[0][0] <= p[1][0]),
                        "Unordered SRT keys"
                    );
                    values[index] = Curve::Hermite(keys);
                } else {
                    anyhow::bail!("Unsupported SRT interpolation");
                }
            }
            channels.push(Channel {
                indirect,
                slot,
                values,
                inverse_base: inverse(matrix)?,
            });
        }
        Ok(Self {
            duration,
            looping: info.attribute("loop") == Some("on"),
            channels,
            indirect_base,
        })
    }
}
impl Runtime {
    pub fn load(files: &[VirtualFile]) -> Self {
        let mut result = Self {
            animations: BTreeMap::new(),
            errors: BTreeMap::new(),
        };
        for f in files
            .iter()
            .filter(|f| f.path.to_ascii_lowercase().ends_with(".rtsa"))
        {
            let name = stem(&f.path);
            let loaded = (|| -> Result<Animation> {
                let shader = files
                    .iter()
                    .find(|s| {
                        s.path.to_ascii_lowercase().ends_with(".r3mt") && stem(&s.path) == name
                    })
                    .context("SRT material unavailable")?;
                Animation::load(
                    std::str::from_utf8(&f.data)?,
                    std::str::from_utf8(&shader.data)?,
                )
            })();
            match loaded {
                Ok(v) => {
                    result.animations.insert(name, v);
                }
                Err(e) => {
                    result.errors.insert(name, format!("{e:#}"));
                }
            }
        }
        result
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"animated_materials":self.animations.keys().collect::<Vec<_>>(),"errors":self.errors,"original_frame_parity_validated":false})
    }
    pub fn apply(&self, seconds: f32, draws: &mut [GpuDrawCommand]) -> Result<()> {
        for draw in draws {
            let Some(name) = draw.material.as_ref().and_then(|m| m.shader.as_ref()) else {
                continue;
            };
            let Some(animation) = self.animations.get(&stem(name)) else {
                continue;
            };
            let frame = if animation.looping {
                (seconds * 60.).rem_euclid(animation.duration)
            } else {
                (seconds * 60.).min(animation.duration)
            };
            for channel in &animation.channels {
                let v = channel.values.each_ref().map(|v| v.sample(frame));
                if channel.indirect {
                    let matrices = draw
                        .material_indirect
                        .get_or_insert(animation.indirect_base);
                    let rows = crate::render_support::material_preview::indirect::srt_matrix(
                        [v[0], v[1]],
                        v[2],
                        [v[3], v[4]],
                    )?;
                    let start = channel.slot as usize * 2;
                    matrices[start..start + 2].copy_from_slice(&rows);
                    continue;
                }
                let matrix = crate::render_support::material_preview::native_uv_matrix(
                    [v[0], v[1]],
                    v[2],
                    [v[3], v[4]],
                )?;
                // The shader contains its authored base SRT. Replace that base
                // while preserving the independently authored NIF UV transform.
                let delta = multiply(matrix, channel.inverse_base);
                for image in Arc::make_mut(&mut draw.material_images)
                    .iter_mut()
                    .filter(|i| i.slot == channel.slot)
                {
                    image.matrix = multiply(image.matrix, delta);
                }
            }
        }
        Ok(())
    }
}
