//! Gamebryo 20.1+ render properties, edited without rewriting their object links.
//! Layout/masks: Gamebryo 2.6 NiObjectNET and Ni{VertexColor,ZBuffer,Specular,Shade}Property.
use epic_mickey_lib_rs::nif_file::RawBlockData;

pub fn friendly_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "NiVertexColorProperty" => "Vertex colors & lighting",
        "NiZBufferProperty" => "Depth testing",
        "NiSpecularProperty" => "Specular highlights",
        "NiShadeProperty" => "Surface shading",
        _ => return None,
    })
}
#[derive(Debug)]
pub struct RenderProperty {
    pub flags: u16,
    offset: usize,
}
impl RenderProperty {
    pub fn read(raw: &RawBlockData, little: bool, version: u32) -> Option<Self> {
        friendly_name(&raw.type_name)?;
        if version < 0x14010002 {
            return None;
        }
        let bytes: [u8; 4] = raw.raw_data.get(4..8)?.try_into().ok()?;
        let count = if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        } as usize;
        let offset = count.checked_mul(4)?.checked_add(12)?;
        if offset.checked_add(2)? != raw.raw_data.len() {
            return None;
        }
        let bytes = raw.raw_data[offset..].try_into().ok()?;
        Some(Self {
            flags: if little {
                u16::from_le_bytes(bytes)
            } else {
                u16::from_be_bytes(bytes)
            },
            offset,
        })
    }
    pub fn write(&self, raw: &mut RawBlockData, little: bool) {
        let bytes = if little {
            self.flags.to_le_bytes()
        } else {
            self.flags.to_be_bytes()
        };
        raw.raw_data[self.offset..self.offset + 2].copy_from_slice(&bytes);
    }
    fn toggle(&mut self, ui: &mut egui::Ui, label: &str, mask: u16) {
        let mut enabled = self.flags & mask != 0;
        if ui.checkbox(&mut enabled, label).changed() {
            self.flags = (self.flags & !mask) | if enabled { mask } else { 0 };
        }
    }
    fn choice(&mut self, ui: &mut egui::Ui, label: &str, mask: u16, shift: u16, options: &[&str]) {
        let mut value = (self.flags & mask) >> shift;
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(
                options
                    .get(value as usize)
                    .copied()
                    .unwrap_or("Unrecognized (preserved)"),
            )
            .show_ui(ui, |ui| {
                for (index, text) in options.iter().enumerate() {
                    if ui
                        .selectable_value(&mut value, index as u16, *text)
                        .changed()
                    {
                        self.flags = (self.flags & !mask) | ((value << shift) & mask);
                    }
                }
            });
    }
}
pub fn show(ui: &mut egui::Ui, raw: &mut RawBlockData, little: bool, version: u32) -> bool {
    let Some(mut property) = RenderProperty::read(raw, little, version) else {
        return false;
    };
    ui.strong(friendly_name(&raw.type_name).unwrap());
    match raw.type_name.as_str() {
        "NiVertexColorProperty" => {
            property.choice(
                ui,
                "Vertex color contribution",
                0x30,
                4,
                &[
                    "Use material colors",
                    "Emissive color",
                    "Ambient and diffuse color",
                ],
            );
            property.choice(
                ui,
                "Lighting",
                0x08,
                3,
                &["Emissive only", "Emissive, ambient and diffuse"],
            );
            ui.weak("Controls how mesh vertex colors contribute to lighting. The assigned shader must support this setting.");
        }
        "NiZBufferProperty" => {
            property.toggle(ui, "Test against scene depth", 1);
            property.toggle(ui, "Write to scene depth", 2);
            property.choice(
                ui,
                "Depth comparison",
                0x3c,
                2,
                &[
                    "Always",
                    "Closer",
                    "Equal",
                    "Closer or equal",
                    "Farther",
                    "Not equal",
                    "Farther or equal",
                    "Never",
                ],
            );
            ui.weak("Depth writes determine whether this surface hides geometry behind it.");
        }
        "NiSpecularProperty" => {
            property.toggle(ui, "Enable specular highlights", 1);
            ui.weak("Highlight color and glossiness are controlled by the material.");
        }
        "NiShadeProperty" => {
            property.toggle(ui, "Smooth shading", 1);
        }
        _ => unreachable!(),
    }
    property.write(raw, little);
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_flags_preserve_names_links_and_reserved_bits_in_both_byte_orders() {
        for little in [false, true] {
            let mut data = vec![0xab; 22];
            data[4..8].copy_from_slice(&if little {
                2u32.to_le_bytes()
            } else {
                2u32.to_be_bytes()
            });
            data[20..].copy_from_slice(&if little {
                0xfacbu16.to_le_bytes()
            } else {
                0xfacbu16.to_be_bytes()
            });
            let mut raw = RawBlockData {
                type_name: "NiZBufferProperty".into(),
                raw_data: data.clone(),
            };
            let mut property = RenderProperty::read(&raw, little, 0x14060500).unwrap();
            assert_eq!(property.flags, 0xfacb);
            property.flags ^= 2;
            property.write(&mut raw, little);
            assert_eq!(&raw.raw_data[..20], &data[..20]);
            assert_eq!(
                RenderProperty::read(&raw, little, 0x14060500)
                    .unwrap()
                    .flags,
                0xfac9
            );
            assert!(RenderProperty::read(&raw, little, 0x14000005).is_none());
            raw.raw_data.push(0);
            assert!(RenderProperty::read(&raw, little, 0x14060500).is_none());
        }
    }
    #[test]
    #[ignore = "Requires local retail packfiles; set EM1_PACKFILES"]
    fn retail_render_properties_save_and_reopen_without_changing_other_bytes() {
        use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
        let root = std::env::var("EM1_PACKFILES").expect("Set EM1_PACKFILES");
        let mut archives = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pak"))
            .collect::<Vec<_>>();
        archives.sort();
        let mut seen = std::collections::BTreeSet::new();
        for archive in archives {
            let pack = crate::pack_io::read_filtered(std::fs::read(archive).unwrap(), |_, p| {
                p.to_lowercase().ends_with(".nif_wii")
            })
            .unwrap();
            for file in pack.pak.files {
                if file.data.is_empty() || !file.path.to_lowercase().ends_with(".nif_wii") {
                    continue;
                }
                let mut nif = NifFile::load_bytes(&file.data).unwrap();
                let original = crate::nif_save::OriginalNif::capture(&file.data, &nif).unwrap();
                for index in 0..nif.blocks.len() {
                    let NiObject::Unknown(raw) = &mut nif.blocks[index] else {
                        continue;
                    };
                    if seen.contains(&raw.type_name) {
                        continue;
                    }
                    let Some(mut property) =
                        RenderProperty::read(raw, nif.source_is_little_endian, nif.version)
                    else {
                        continue;
                    };
                    let kind = raw.type_name.clone();
                    let old = raw.raw_data.clone();
                    property.flags ^= if kind == "NiVertexColorProperty" {
                        8
                    } else {
                        1
                    };
                    property.write(raw, nif.source_is_little_endian);
                    let changed = raw.raw_data.clone();
                    let bytes = original.save(&nif).unwrap();
                    let loaded = NifFile::load_bytes(&bytes).unwrap();
                    let NiObject::Unknown(raw) = &loaded.blocks[index] else {
                        panic!("Unexpected type");
                    };
                    assert_eq!(raw.raw_data, changed);
                    assert_eq!(bytes.len(), file.data.len());
                    assert_eq!(
                        bytes.iter().zip(&file.data).filter(|(a, b)| a != b).count(),
                        1
                    );
                    let NiObject::Unknown(raw) = &mut nif.blocks[index] else {
                        unreachable!()
                    };
                    raw.raw_data = old;
                    seen.insert(kind);
                }
                if seen.len() == 4 {
                    return;
                }
            }
        }
        panic!("Missing retail property samples: {seen:?}");
    }
}
