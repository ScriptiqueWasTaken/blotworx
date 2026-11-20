//! Version-bounded EM1 texture bindings; no speculative offsets or partial results.

use anyhow::{ensure, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]

pub struct Binding {
    pub source: u32,
    pub shader_id: Option<u32>,
    pub slot: usize,
    pub source_offset: usize,
}

impl Binding {
    pub fn label(&self) -> String {
        if self.shader_id.is_some() {
            format!("Shader texture {}", self.slot + 1)
        } else {
            [
                "Base texture",
                "Dark texture",
                "Detail texture",
                "Gloss texture",
                "Glow texture",
                "Bump texture",
                "Normal texture",
                "Parallax texture",
                "Decal 1",
                "Decal 2",
                "Decal 3",
                "Decal 4",
            ]
            .get(self.slot)
            .unwrap_or(&"Texture")
            .to_string()
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    little: bool,
    anisotropy: bool,
}

impl Reader<'_> {
    fn skip(&mut self, n: usize) -> Result<()> {
        self.pos = self.pos.checked_add(n).context("Texture offset overflow")?;
        ensure!(self.pos <= self.data.len(), "Truncated texture descriptor");
        Ok(())
    }

    fn u32(&mut self) -> Result<u32> {
        let at = self.pos;
        self.skip(4)?;
        let b = self.data[at..self.pos].try_into()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn flag(&mut self) -> Result<bool> {
        let at = self.pos;
        self.skip(1)?;
        ensure!(self.data[at] <= 1, "Invalid texture presence flag");
        Ok(self.data[at] != 0)
    }

    fn descriptor(&mut self, slot: usize) -> Result<Binding> {
        let source_offset = self.pos;
        let source = self.u32()?;
        self.skip(if self.anisotropy { 4 } else { 2 })?; // packed flags, then anisotropy in newer versions

        if self.flag()? {
            self.skip(32)?;
        } // translation, scale, rotation, method, center

        Ok(Binding {
            source,
            shader_id: None,
            slot,
            source_offset,
        })
    }
}

pub fn bindings(data: &[u8], little: bool, version: u32) -> Result<Vec<Binding>> {
    ensure!(
        matches!(version, 0x14060500 | 0x14030109),
        "Unsupported texture binding version"
    );

    let mut r = Reader {
        data,
        pos: 4,
        little,
        anisotropy: version >= 0x14050004,
    };
    let extras = r.u32()? as usize;
    ensure!(extras <= 1024, "Too many property extra-data links");
    r.skip(extras * 4 + 4 + 2)?;

    let count = r.u32()? as usize;
    ensure!(count <= 12, "Invalid standard texture count");
    let mut maps = vec![];

    for slot in 0..count {
        if r.flag()? {
            maps.push(r.descriptor(slot)?);
            if slot == 5 {
                r.skip(24)?;
            }
            if slot == 7 {
                r.skip(4)?;
            }
        }
    }

    let count = r.u32()? as usize;
    ensure!(count <= 64, "Invalid shader texture count");

    for slot in 0..count {
        if r.flag()? {
            let mut map = r.descriptor(slot)?;
            map.shader_id = Some(r.u32()?);
            maps.push(map);
        }
    }

    ensure!(
        r.pos == data.len(),
        "Unrecognized trailing texture-property bytes"
    );
    Ok(maps)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn gpu_uv_matrix_matches_cpu_transform() {
        for method in 0..=2 {
            let t = UvTransform {
                translation: [0.3, -0.7],
                scale: [2., -3.],
                rotation: 0.8,
                method,
                center: [0.5, 0.2],
            };
            let m = t.matrix().unwrap();
            for uv in [[0., 0.], [1., 0.], [0., 1.], [-2., 3.]] {
                let expected = t.apply(uv).unwrap();
                let actual = [
                    m[0] * uv[0] + m[3] * uv[1] + m[6],
                    m[1] * uv[0] + m[4] * uv[1] + m[7],
                ];
                assert!(
                    (actual[0] - expected[0]).abs() < 1e-5
                        && (actual[1] - expected[1]).abs() < 1e-5
                );
            }
        }
    }
    #[test]
    fn uv_preview_respects_transform_conventions() {
        let mut t = UvTransform::default();
        assert_eq!(t.apply([0.25, 0.75]), Some([0.25, 0.75]));
        t.translation = [0.5, 0.25];
        assert_eq!(t.apply([0.25, 0.75]), Some([0.75, 1.]));
        t.method = 1;
        assert_eq!(t.apply([0.25, 0.75]), Some([-0.25, 1.]));
        t.method = 2;
        t.translation = [0., 0.];
        assert_eq!(t.apply([0.25, 0.75]), Some([0.25, 0.25]));
        t.method = 0;
        t.rotation = std::f32::consts::FRAC_PI_2;
        let uv = t.apply([1., 0.]).unwrap();
        assert!(uv[0].abs() < 1e-6 && (uv[1] - 1.).abs() < 1e-6);
        t.rotation = f32::NAN;
        assert!(t.apply([1., 0.]).is_none());
    }
    #[test]
    fn uv_controls_fit_a_narrow_inspector() {
        let mut bytes = vec![];
        for value in [u32::MAX, 0, u32::MAX] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([0, 0]);
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.push(1);
        bytes.extend(1u32.to_be_bytes());
        bytes.extend([0; 4]);
        bytes.push(0);
        bytes.extend(0u32.to_be_bytes());
        let bytes =
            with_transform(&bytes, 0, Some(UvTransform::default()), false, 0x14060500).unwrap();
        let mut raw = epic_mickey_lib_rs::nif_file::RawBlockData {
            type_name: "NiTexturingProperty".into(),
            raw_data: bytes.clone(),
        };
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(330., 700.),
                )),
                ..Default::default()
            },
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    show(ui, &mut raw, false, 0x14060500);
                });
            },
        );
        // This layout-only test does not upload textures to a renderer.
        output.textures_delta.clear();
        assert_eq!(
            raw.raw_data, bytes,
            "Rendering controls must not change the asset"
        );
    }
    #[test]
    fn parses_multi_texture_transforms_and_bump_fields_in_both_endians() {
        for little in [false, true] {
            let mut data = vec![];
            fn push(data: &mut Vec<u8>, v: u32, l: bool) {
                data.extend(if l { v.to_le_bytes() } else { v.to_be_bytes() });
            }

            push(&mut data, u32::MAX, little);
            push(&mut data, 0, little);
            push(&mut data, u32::MAX, little);
            data.extend([0, 0]);
            push(&mut data, 8, little);

            for slot in 0..8 {
                let active = matches!(slot, 0 | 5 | 7);
                data.push(active as u8);
                if active {
                    push(&mut data, slot + 20, little);
                    data.extend([0; 4]);
                    data.push(1);
                    data.extend([0; 32]);
                    if slot == 5 {
                        data.extend([0; 24]);
                    }
                    if slot == 7 {
                        data.extend([0; 4]);
                    }
                }
            }

            push(&mut data, 3, little);
            for i in 0..3 {
                data.push(1);
                push(&mut data, 40 + i, little);
                data.extend([0; 4]);
                data.push(0);
                push(&mut data, i, little);
            }

            let maps = bindings(&data, little, 0x14060500).unwrap();
            assert_eq!(
                maps.iter().map(|m| m.source).collect::<Vec<_>>(),
                [20, 25, 27, 40, 41, 42]
            );
            assert_eq!(maps[5].shader_id, Some(2));

            for version in [0x14060500, 0x14030109] {
                let mut source = data.clone();
                if version == 0x14030109 {
                    for map in maps.iter().rev() {
                        source.drain(map.source_offset + 6..map.source_offset + 8);
                    }
                }
                let mut value = UvTransform::default();
                value.translation = [0.25, -0.5];
                value.scale = [2., 3.];
                value.rotation = 1.;
                value.center = [0.5, 0.5];
                value.method = 2;
                let added = with_transform(&source, 5, Some(value), little, version).unwrap();
                assert_eq!(added.len(), source.len() + 32);
                let added_maps = bindings(&added, little, version).unwrap();
                assert_eq!(
                    transform(&added, &added_maps[5], little, version).unwrap(),
                    Some(value)
                );
                assert_eq!(
                    with_transform(&added, 5, None, little, version).unwrap(),
                    source
                );
                let before = source.clone();
                value.rotation = f32::NAN;
                assert!(with_transform(&source, 5, Some(value), little, version).is_err());
                assert_eq!(source, before);
            }
            for len in 0..data.len() {
                assert!(bindings(&data[..len], little, 0x14060500).is_err());
            }

            data.push(0);
            assert!(bindings(&data, little, 0x14060500).is_err());
        }
    }
}

/// Edit packed sampler fields while retaining the unused high bits and all links.

pub fn show(
    ui: &mut egui::Ui,
    raw: &mut epic_mickey_lib_rs::nif_file::RawBlockData,
    little: bool,
    version: u32,
) -> bool {
    if raw.type_name != "NiTexturingProperty" {
        return false;
    }

    let maps = match bindings(&raw.raw_data, little, version) {
        Ok(m) => m,
        Err(e) => {
            ui.colored_label(egui::Color32::GOLD, e.to_string());
            return true;
        }
    };

    if maps.is_empty() {
        ui.label("No texture inputs assigned.");
    }

    let mut pending = None;

    for (slot, map) in maps.into_iter().enumerate() {
        ui.push_id(slot, |ui| {
            ui.strong(map.label());
            let at = map.source_offset + 4;
            let bytes = raw.raw_data[at..at + 2].try_into().unwrap();
            let mut flags = if little {
                u16::from_le_bytes(bytes)
            } else {
                u16::from_be_bytes(bytes)
            };

            let mut uv = flags & 255;
            ui.horizontal(|ui| {
                ui.label("UV channel");
                if ui
                    .add(egui::DragValue::new(&mut uv).range(0..=255))
                    .changed()
                {
                    flags = (flags & !255) | uv;
                }
            });

            for (label, mask, shift, options) in [
                (
                    "Tiling",
                    0x3000,
                    12,
                    &[
                        "Clamp U and V",
                        "Clamp U, repeat V",
                        "Repeat U, clamp V",
                        "Repeat U and V",
                    ][..],
                ),
                (
                    "Filtering",
                    0x0f00,
                    8,
                    &[
                        "Nearest",
                        "Bilinear",
                        "Trilinear",
                        "Nearest mip / nearest pixel",
                        "Linear mip / nearest pixel",
                        "Nearest mip / bilinear",
                        "Anisotropic",
                    ][..],
                ),
            ] {
                let mut v = (flags & mask) >> shift;
                ui.label(label);
                egui::ComboBox::from_id_salt(label)
                    .selected_text(
                        options
                            .get(v as usize)
                            .copied()
                            .unwrap_or("Unrecognized (preserved)"),
                    )
                    .show_ui(ui, |ui| {
                        for (i, name) in options.iter().enumerate() {
                            if ui.selectable_value(&mut v, i as u16, *name).changed() {
                                flags = (flags & !mask) | ((v << shift) & mask);
                            }
                        }
                    });
            }

            raw.raw_data[at..at + 2].copy_from_slice(&if little {
                flags.to_le_bytes()
            } else {
                flags.to_be_bytes()
            });

            match transform(&raw.raw_data, &map, little, version) {
                Ok(before) => {
                    let mut enabled = before.is_some();
                    let mut value = before.unwrap_or_default();
                    let mut changed = ui.checkbox(&mut enabled, "Transform UVs").changed();

                    if enabled {
                        egui::Grid::new("uv_transform")
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (name, pair) in [
                                    ("Offset", &mut value.translation),
                                    ("Scale", &mut value.scale),
                                    ("Pivot", &mut value.center),
                                ] {
                                    ui.label(name);
                                    ui.horizontal(|ui| {
                                        for v in pair {
                                            changed |= ui
                                                .add(egui::DragValue::new(v).speed(0.01))
                                                .changed();
                                        }
                                    });
                                    ui.end_row();
                                }

                                ui.label("Rotation");
                                let mut degrees = value.rotation.to_degrees();
                                if ui
                                    .add(egui::DragValue::new(&mut degrees).speed(1.).suffix("°"))
                                    .changed()
                                {
                                    value.rotation = degrees.to_radians();
                                    changed = true;
                                }
                                ui.end_row();

                                ui.label("Transform order");
                                let old = value.method;
                                egui::ComboBox::from_id_salt("order")
                                    .selected_text(
                                        ["Maya legacy", "3ds Max", "Maya"]
                                            .get(value.method as usize)
                                            .copied()
                                            .unwrap_or("Unknown (preserved)"),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (i, label) in
                                            ["Maya legacy", "3ds Max", "Maya"].iter().enumerate()
                                        {
                                            ui.selectable_value(
                                                &mut value.method,
                                                i as u32,
                                                *label,
                                            );
                                        }
                                    });
                                changed |= old != value.method;
                                ui.end_row();
                            });
                        if ui.button("Reset UV transform").clicked() {
                            value = UvTransform::default();
                            changed = true;
                        }
                    }

                    if changed {
                        pending = Some((slot, enabled.then_some(value)));
                    }
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::GOLD, e.to_string());
                }
            }
        });
    }

    if let Some((slot, value)) = pending {
        match with_transform(&raw.raw_data, slot, value, little, version) {
            Ok(next) => raw.raw_data = next,
            Err(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
            }
        }
    }

    ui.weak("The shader may override sampler settings. Texture assets are listed below.");
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]

pub struct UvTransform {
    pub translation: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub method: u32,
    pub center: [f32; 2],
}

impl Default for UvTransform {
    fn default() -> Self {
        Self {
            translation: [0.; 2],
            scale: [1.; 2],
            rotation: 0.,
            method: 0,
            center: [0.; 2],
        }
    }
}

fn transform_flag(map: &Binding, version: u32) -> usize {
    map.source_offset + if version >= 0x14050004 { 8 } else { 6 }
}

pub fn transform(
    data: &[u8],
    map: &Binding,
    little: bool,
    version: u32,
) -> Result<Option<UvTransform>> {
    let at = transform_flag(map, version);
    let present = *data.get(at).context("Missing UV transform flag")?;
    ensure!(present <= 1, "Invalid UV transform flag");
    if present == 0 {
        return Ok(None);
    }

    let bytes = data
        .get(at + 1..at + 33)
        .context("Truncated UV transform")?;

    let u = |i: usize| {
        let b = bytes[i * 4..i * 4 + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };

    let f = |i| f32::from_bits(u(i));
    Ok(Some(UvTransform {
        translation: [f(0), f(1)],
        scale: [f(2), f(3)],
        rotation: f(4),
        method: u(5),
        center: [f(6), f(7)],
    }))
}

/// Rebuild one optional transform; re-parse all descriptors before returning it.

pub fn with_transform(
    data: &[u8],
    slot: usize,
    value: Option<UvTransform>,
    little: bool,
    version: u32,
) -> Result<Vec<u8>> {
    let maps = bindings(data, little, version)?;
    let map = maps.get(slot).context("Texture input no longer exists")?;
    let at = transform_flag(map, version);
    let old = transform(data, map, little, version)?;

    let mut encoded = vec![u8::from(value.is_some())];

    if let Some(t) = value {
        ensure!(
            t.translation
                .into_iter()
                .chain(t.scale)
                .chain([t.rotation])
                .chain(t.center)
                .all(f32::is_finite),
            "UV transform values must be finite"
        );
        ensure!(t.method <= 2, "Unknown UV transform order");

        for v in [
            t.translation[0].to_bits(),
            t.translation[1].to_bits(),
            t.scale[0].to_bits(),
            t.scale[1].to_bits(),
            t.rotation.to_bits(),
            t.method,
            t.center[0].to_bits(),
            t.center[1].to_bits(),
        ] {
            encoded.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            });
        }
    }

    let mut next = data.to_vec();
    next.splice(at..at + 1 + if old.is_some() { 32 } else { 0 }, encoded);

    let after = bindings(&next, little, version)?;
    ensure!(
        after.len() == maps.len()
            && after.iter().zip(&maps).all(|(a, b)| a.source == b.source
                && a.shader_id == b.shader_id
                && a.slot == b.slot),
        "UV edit changed texture bindings"
    );
    Ok(next)
}

impl UvTransform {
    pub fn matrix(self) -> Option<[f32; 9]> {
        let [x, y] = self.apply([0., 0.])?;
        let (s, c) = self.rotation.sin_cos();
        let [sx, sy] = self.scale;
        let (a, b, d, e) = match self.method {
            0 => (c * sx, s * sx, -s * sy, c * sy),
            1 => (c * sx, -s * sy, s * sx, c * sy),
            2 => (c * sx, -s * sx, -s * sy, -c * sy),
            _ => return None,
        };
        let m = [a, b, 0., d, e, 0., x, y, 1.];
        m.iter().all(|v| v.is_finite()).then_some(m)
    }
    /// Gamebryo 2.6's three texture-transform conventions, applied to a UV point.
    pub fn apply(self, uv: [f32; 2]) -> Option<[f32; 2]> {
        let [u, v] = uv;
        let [sx, sy] = self.scale;
        let [tx, ty] = self.translation;
        let [cx, cy] = self.center;
        let (s, c) = self.rotation.sin_cos();
        let point = match self.method {
            0 => {
                let x = sx * u + tx - cx;
                let y = sy * v + ty - cy;
                [c * x - s * y + cx, s * x + c * y + cy]
            }
            1 => {
                let x = u - cx - tx;
                let y = v - cy + ty;
                [cx + sx * (c * x + s * y), cy + sy * (-s * x + c * y)]
            }
            2 => {
                let x = sx * u + tx - cx;
                let y = 1. - sy * v - ty - cy;
                [c * x + s * y + cx, -s * x + c * y + cy]
            }
            _ => return None,
        };
        point.into_iter().all(f32::is_finite).then_some(point)
    }
}
