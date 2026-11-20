//! Ordered runtime references used by the asset cooker.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{file_manipulator::EndianType, scene_file::SceneFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Asset,
    Template,
    Candidate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub value: String,
    pub kind: ReferenceKind,
    pub source: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssetInfo {
    pub references: Vec<Reference>,
    pub definitions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loose_references: Vec<String>,
    /// Object indices absent from SceneFile.scene. None identifies an older cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolbox: Option<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefabs: Option<Vec<crate::mod_project::prefabs::Definition>>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub prefab_index_version: u32,
    pub placed: usize,
    pub classes: Vec<String>,
}
fn is_zero(v: &u32) -> bool {
    *v == 0
}
pub fn scene_toolbox(scene: &SceneFile) -> Vec<usize> {
    let placed: BTreeSet<_> = scene.scene.iter().map(|id| id.to_u32()).collect();
    scene
        .objects
        .iter()
        .enumerate()
        .filter(|(_, e)| !placed.contains(&e.link_id.to_u32()))
        .map(|(i, _)| i)
        .collect()
}
fn strings(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => output.push(s.clone()),
        serde_json::Value::Array(a) => {
            for v in a {
                strings(v, output);
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values() {
                strings(v, output);
            }
        }
        _ => {}
    }
}
fn pathlike(s: &str) -> bool {
    matches!(
        s.rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "nif"
                | "nif_wii"
                | "hkx"
                | "hkx_wii"
                | "gsa"
                | "pal"
                | "bin"
                | "lua"
                | "kfm"
                | "kfm_wii"
                | "kf"
                | "kf_wii"
                | "hkw"
                | "hkb"
                | "part"
                | "level"
                | "bsq"
                | "clb"
                | "bnk"
                | "wem"
                | "r3mt"
                | "rtsa"
                | "rcla"
                | "dds"
                | "hpd"
                | "lit"
                | "lit_cooked"
                | "gfx"
                | "dct"
                | "sub"
        )
    )
}
fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|_| anyhow::anyhow!("Malformed asset during reference parsing"))?
}
pub const VERSION: u32 = 3;

/// Preserve declaration order: palette DFS and runtime dependency DFS have
/// distinct load phases. A sorted set loses the author's reference sequence.
pub fn inspect(path: &str, data: &[u8]) -> Result<AssetInfo> {
    let path = path.to_lowercase();
    let mut info = AssetInfo::default();
    let mut add = |value: String, kind: ReferenceKind, source: String| {
        let value = value.trim().replace('\\', "/");
        if !value.is_empty() && !value.contains(['\n', '\r', '\0']) {
            info.references.push(Reference {
                value,
                kind,
                source,
            });
        }
    };
    if path.ends_with(".bin") {
        // `.bin` is shared by native scenes and unrelated game binaries. A
        // malformed scene header is therefore a valid opaque-binary case;
        // leave it with no inferred references so it remains editable through
        // the raw binary editor.
        let scene = match guarded(|| Ok(SceneFile::from_binary(data, EndianType::BIG))) {
            Ok(scene) => scene,
            Err(_) => return Ok(info),
        };
        info.placed = scene.scene.len();
        info.toolbox = Some(scene_toolbox(&scene));
        info.prefabs = Some(crate::mod_project::prefabs::index(&scene));
        info.prefab_index_version = crate::mod_project::prefabs::INDEX_VERSION;
        for entity in &scene.objects {
            info.definitions.push(entity.name.clone());
            // Master definitions are serialized runtime data too. Retail cooking
            // includes their dependencies even when an instance overrides them.
            for component in &entity.components {
                // Both streams are engine-created by Mickey's controller.
                // All 191 retail scene packs with Player include these palettes;
                // the global palette already defines them inline.
                if !path.starts_with("palettes/")
                    && component.class_name == "MickeyPlayerControllerComponent"
                {
                    for stream in ["paintstream", "thinnerstream"] {
                        add(
                            format!("Palettes/_Dynamic/PlayerTools/{stream}.bin"),
                            ReferenceKind::Asset,
                            "Mickey player controller".into(),
                        );
                    }
                }
                for property in &component.properties {
                    let kind = if property.asset || property.palette {
                        ReferenceKind::Asset
                    } else if property.template {
                        ReferenceKind::Template
                    } else {
                        continue;
                    };
                    let mut values = vec![];
                    strings(&property.value, &mut values);
                    for value in values {
                        add(
                            value,
                            kind.clone(),
                            format!(
                                "{} / {} / {}",
                                entity.name, component.class_name, property.name
                            ),
                        );
                    }
                }
            }
        }
    } else if path.ends_with(".nif_wii") || path.ends_with(".nif") {
        for value in nif_strings(data)? {
            if pathlike(&value) {
                add(value, ReferenceKind::Asset, "NIF external asset".into());
            }
        }
    } else if path.ends_with(".kfm") || path.ends_with(".kfm_wii") {
        let kfm = guarded(|| {
            epic_mickey_lib_rs::kfm_file::KfmFile::load_bytes(data).map_err(anyhow::Error::msg)
        })?;
        add(kfm.model_path, ReferenceKind::Asset, "KFM model".into());
        for seq in kfm.sequences {
            add(
                seq.filename,
                ReferenceKind::Asset,
                format!("KFM sequence {}", seq.sequence_id),
            );
        }
    } else if path.ends_with(".lua") {
        let compiled;
        let bytes = if data.starts_with(b"\x1bLua") {
            data
        } else {
            compiled = crate::lua_cook::compile(std::str::from_utf8(data)?)?;
            &compiled
        };
        info.definitions = crate::lua_cook::globals(bytes)?;
        for value in crate::lua_cook::strings(bytes)? {
            let kind = if pathlike(&value) {
                ReferenceKind::Asset
            } else {
                ReferenceKind::Candidate
            };
            add(value, kind, "Lua constant".into());
        }
    } else if path.ends_with(".hpd") {
        let database = crate::ai_nav::Database::load(data)?;
        info.loose_references = database
            .sectors
            .iter()
            .filter_map(|s| crate::ai_nav::Database::sector_path(&path, s.id))
            .collect();
    } else if path.ends_with(".bsq") {
        let doc = crate::em1_bsq::Document::load(&path, data)?;
        if !doc.database {
            if let crate::em1_bsq::Node::Record { fields, .. } = &doc.root {
                if let Some(name) = fields
                    .get(1)
                    .and_then(|n| n.value("Name"))
                    .and_then(|v| v.as_str())
                {
                    info.definitions.push(name.into());
                }
            }
        }
        let mut fields = vec![];
        doc.root.strings(&mut fields);
        for (field, value) in fields {
            if field == "Sequence" {
                add(value, ReferenceKind::Asset, "BSQ sequence".into());
            } else if pathlike(&value) {
                add(value, ReferenceKind::Asset, format!("BSQ {field}"));
            } else if field == "Lua" {
                // Parse embedded Lua without executing it, retaining constant references.
                if let Ok(bytes) = crate::lua_cook::compile(&value) {
                    for value in crate::lua_cook::strings(&bytes)? {
                        if pathlike(&value) {
                            add(value, ReferenceKind::Asset, "BSQ Lua asset".into());
                        }
                    }
                }
            }
        }
    } else if path.ends_with(".clb") {
        // JPS strings have two byte lengths and four-byte alignment. Reading
        // raw printable runs incorrectly prefixes paths with those length bytes.
        for offset in (0..data.len().saturating_sub(2)).step_by(4) {
            let tail = &data[offset + 2..];
            let Some(end) = tail.iter().take(65536).position(|b| *b == 0) else {
                continue;
            };
            let length = if end == 0 { 0 } else { end + 1 };
            let padded = (length + 2).div_ceil(4) * 4;
            if data[offset] != padded as u8 || data[offset + 1] != length as u8 {
                continue;
            }
            if let Ok(value) = std::str::from_utf8(&tail[..end]) {
                if pathlike(value) {
                    add(
                        value.into(),
                        ReferenceKind::Asset,
                        "JPS string field".into(),
                    );
                } else if value.contains(['\'', '"']) {
                    for token in value.split(['\'', '"']) {
                        if pathlike(token) {
                            add(
                                token.into(),
                                ReferenceKind::Asset,
                                "JPS script field".into(),
                            );
                        }
                    }
                }
            }
        }
    } else if path.ends_with(".hkw") || path.ends_with(".level") || path.ends_with(".part") {
        for line in String::from_utf8_lossy(data).lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            let value = line
                .split_once('=')
                .map(|(_, v)| v)
                .unwrap_or(line)
                .trim()
                .trim_matches(['"', '\'']);
            if pathlike(value) {
                add(value.into(), ReferenceKind::Asset, "Declared file".into());
            }
        }
    } else if path.ends_with(".gfx") {
        for value in gfx_references(data)? {
            add(value, ReferenceKind::Asset, "GFx external resource".into());
        }
    } else if path.ends_with(".hkx") || path.ends_with(".hkx_wii") {
        // Read references only through serialized pointers, avoiding printable
        // names/provenance embedded in unrelated binary structures.
        let hkx = guarded(|| {
            epic_mickey_lib_rs::hkx_file::HkxFile::load_bytes(data).map_err(anyhow::Error::msg)
        })?;
        info.classes = hkx.class_names.iter().map(|c| c.name.clone()).collect();
        for (section, fixups) in hkx.local_fixups.iter().enumerate() {
            let bytes = &hkx.section_data[section];
            for fixup in fixups {
                if let Some(rest) = bytes.get(fixup.dst_offset as usize..) {
                    if let Some(n) = rest.iter().take(2048).position(|b| *b == 0) {
                        if let Ok(value) = std::str::from_utf8(&rest[..n]) {
                            if pathlike(value) {
                                add(
                                    value.into(),
                                    ReferenceKind::Asset,
                                    format!("Havok pointer {}:{}", section, fixup.src_offset),
                                );
                            }
                        }
                    }
                }
            }
        }
    } else {
        for run in data.split(|b| !b.is_ascii_graphic() && *b != b' ') {
            if run.len() < 2048 {
                if let Ok(s) = std::str::from_utf8(run) {
                    for value in std::iter::once(s).chain(s.split(['\"', '\'', '=', ',', ';'])) {
                        let value = value.trim();
                        if pathlike(value) {
                            add(value.into(), ReferenceKind::Asset, "Serialized path".into());
                        }
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    info.references
        .retain(|r| seen.insert((format!("{:?}", r.kind), r.value.to_lowercase())));
    Ok(info)
}

pub(crate) fn nif_strings(data: &[u8]) -> Result<Vec<String>> {
    struct Reader<'a> {
        data: &'a [u8],
        pos: usize,
        le: bool,
    }
    impl Reader<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8]> {
            let end = self.pos.checked_add(n).context("NIF offset overflow")?;
            let s = self
                .data
                .get(self.pos..end)
                .context("Truncated NIF header")?;
            self.pos = end;
            Ok(s)
        }
        fn u32(&mut self) -> Result<u32> {
            let le = self.le;
            let b = self.take(4)?.try_into()?;
            Ok(if le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        }
        fn string(&mut self) -> Result<String> {
            let n = self.u32()? as usize;
            Ok(std::str::from_utf8(self.take(n)?)?.to_owned())
        }
    }
    let pos = data
        .iter()
        .position(|b| *b == b'\n')
        .context("Invalid NIF header")?
        + 1;
    let mut r = Reader {
        data,
        pos,
        le: true,
    };
    let version = r.u32()?;
    ensure!(
        version >= 0x14020005,
        "NIF dependency indexing requires a modern string table"
    );
    let le = r.take(1)?[0] != 0;
    r.u32()?;
    let blocks = r.u32()? as usize;
    ensure!(blocks <= 100000, "Invalid NIF block count");
    r.le = le;
    let b = r.take(2)?.try_into()?;
    let types = if le {
        u16::from_le_bytes(b)
    } else {
        u16::from_be_bytes(b)
    };
    for _ in 0..types {
        r.string()?;
    }
    r.take(blocks * 6)?;
    let count = r.u32()?;
    r.u32()?;
    ensure!(count <= 100000, "Invalid NIF string count");
    (0..count).map(|_| r.string()).collect()
}

/// Scaleform's compressed CFX body uses the SWF tag stream. Tag 1009 stores
/// an external image's exported name and filename as length-prefixed strings.
pub fn gfx_references(data: &[u8]) -> Result<Vec<String>> {
    use std::io::Read;
    ensure!(data.len() >= 9, "Truncated GFx header");
    let expected = u32::from_le_bytes(data[4..8].try_into()?) as usize;
    ensure!(
        (8..=64 * 1024 * 1024).contains(&expected),
        "Invalid GFx length"
    );
    let mut inflated = vec![];
    let bytes = if &data[..3] == b"CFX" || &data[..3] == b"CWS" {
        flate2::read::ZlibDecoder::new(&data[8..])
            .take((expected - 8) as u64 + 1)
            .read_to_end(&mut inflated)?;
        ensure!(
            inflated.len() == expected - 8,
            "GFx decompressed size mismatch"
        );
        &inflated[..]
    } else {
        ensure!(
            &data[..3] == b"GFX" || &data[..3] == b"FWS",
            "Unsupported GFx header"
        );
        &data[8..]
    };
    let bits = (bytes.first().context("Missing GFx rectangle")? >> 3) as usize;
    let mut pos = (5 + 4 * bits).div_ceil(8) + 4;
    let mut out = vec![];
    while pos < bytes.len() {
        let tag = u16::from_le_bytes(
            bytes
                .get(pos..pos + 2)
                .context("Truncated GFx tag")?
                .try_into()?,
        );
        pos += 2;
        let code = tag >> 6;
        let mut len = (tag & 63) as usize;
        if len == 63 {
            len = u32::from_le_bytes(
                bytes
                    .get(pos..pos + 4)
                    .context("Truncated GFx tag size")?
                    .try_into()?,
            ) as usize;
            pos += 4;
        }
        let end = pos.checked_add(len).context("GFx tag overflow")?;
        let body = bytes.get(pos..end).context("Truncated GFx tag body")?;
        pos = end;
        if code == 1009 {
            let mut offset = 10;
            for n in 0..2 {
                let len = *body.get(offset).context("Truncated GFx image name")? as usize;
                offset += 1;
                let value = std::str::from_utf8(
                    body.get(offset..offset + len)
                        .context("Truncated GFx image name")?,
                )?;
                offset += len;
                if n == 1 && !value.is_empty() {
                    out.push(value.into());
                }
            }
        } else if code == 57 || code == 71 {
            let end = body
                .iter()
                .position(|b| *b == 0)
                .context("Unterminated GFx import path")?;
            let value = std::str::from_utf8(&body[..end])?;
            if !value.is_empty() {
                out.push(value.into());
            }
        }
        if code == 0 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_streams_follow_component_and_globals_remain_inline() {
        use epic_mickey_lib_rs::scene_file::{Component, Entity, SceneFileVersion, ID};
        let entity = Entity::new(
            "JPSGeneralEntity".into(),
            "Renamed Avatar".into(),
            ID::from_u32(1),
            ID::new(0),
            0,
            0,
            vec![Component::new(
                "MickeyPlayerControllerComponent".into(),
                "Controller".into(),
                ID::new(4),
                ID::new(2),
                ID::new(0),
                vec![],
            )],
        );
        let scene = SceneFile::new(
            vec![entity],
            vec![ID::from_u32(1)],
            vec![],
            ID::new(10),
            SceneFileVersion::Version1,
        );
        let bytes = scene.to_binary(EndianType::BIG);
        let refs = inspect("environments/custom/new.bin", &bytes)
            .unwrap()
            .references;
        assert_eq!(
            refs.iter().map(|r| r.value.as_str()).collect::<Vec<_>>(),
            [
                "Palettes/_Dynamic/PlayerTools/paintstream.bin",
                "Palettes/_Dynamic/PlayerTools/thinnerstream.bin"
            ]
        );
        assert!(inspect("palettes/_dynamic.bin", &bytes)
            .unwrap()
            .references
            .is_empty());
    }
    #[test]
    fn jps_paths_do_not_include_printable_length_bytes() {
        let path = "hudelements/icons/ui_icon_shop_ammo_thinner_tex.nif";
        let mut data = vec![0; 4];
        let length = path.len() + 1;
        let size = (length + 2).div_ceil(4) * 4;
        data.extend([size as u8, length as u8]);
        data.extend(path.bytes());
        data.resize(4 + size, 0);
        let info = inspect("test.clb", &data).unwrap();
        assert_eq!(info.references.len(), 1);
        assert_eq!(info.references[0].value, path);
    }
    #[test]
    fn declared_paths_preserve_order_and_ignore_legacy_comments() {
        let info = inspect(
            "world.part",
            b"first.gsa\r\n// legacy \xff note\nsecond.gsa\nfirst.gsa\nGlobal.bnk\n",
        )
        .unwrap();
        assert_eq!(
            info.references
                .iter()
                .map(|r| r.value.as_str())
                .collect::<Vec<_>>(),
            ["first.gsa", "second.gsa", "Global.bnk"]
        );
    }
    #[test]
    fn gfx_external_image_is_read_from_its_tag_and_length_checked() {
        let mut body = vec![0; 10];
        body.push(6);
        body.extend(b"Export");
        body.push(8);
        body.extend(b"icon.dds");
        let mut data = b"GFX\x09\0\0\0\0".to_vec();
        data.extend([0, 0, 0, 0, 0]); // empty rectangle, rate and count
        data.extend(((1009u16 << 6) | body.len() as u16).to_le_bytes());
        data.extend(body);
        data.extend([0, 0]);
        let size = data.len() as u32;
        data[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(gfx_references(&data).unwrap(), ["icon.dds"]);
        data.truncate(data.len() - 4);
        assert!(gfx_references(&data).is_err());
    }
}
