//! The level's admitted archive namespace, retaining headers instead of raw
//! asset copies. Resolution preserves first-admitted asset/Wii-alias priority,
//! then admits loose resources contained in the selected game's root.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    nif_file::{NiObject, NifFile},
    packfile::{EndianDependentString, VirtualFile},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::SystemTime,
};
struct Archive {
    path: PathBuf,
    length: u64,
    modified: Option<SystemTime>,
}
struct Entry {
    archive: usize,
    member: crate::pack_io::Member,
}
pub struct Store {
    root: PathBuf,
    archives: Vec<Archive>,
    entries: Vec<Entry>,
    names: BTreeMap<String, usize>,
    palette_scenes: Option<std::sync::Arc<Vec<(String, epic_mickey_lib_rs::scene_file::SceneFile)>>>,
    pub reads: u64,
    pub bytes_read: u64,
    pub loose_reads: u64,
}
fn key(path: &str) -> Result<String> {
    // Archive names use a virtual root, not filesystem absolute paths.
    // Match the importer's root-slash aliases while still rejecting drives
    // and parent traversals that escape that virtual root.
    Ok(
        super::resource_path::canonical(path.replace('\\', "/").trim_matches('/'))?
            .to_ascii_lowercase(),
    )
}
impl Store {
    pub fn new(root: &Path, packs: &[String]) -> Result<Self> {
        let mut store = Self {
            root:root.canonicalize()?,archives:Vec::new(),entries:Vec::new(),names:BTreeMap::new(),
            palette_scenes: None,
            reads:0,bytes_read:0,loose_reads:0,
        };
        store.admit_packs(packs)?;
        Ok(store)
    }
    /// Append archive headers without retaining extracted payloads. Existing
    /// resource/alias ordering and live payload-read counters remain intact.
    pub(super) fn admit_packs(&mut self,packs:&[String])->Result<usize> {
        let pending=packs.iter().filter(|name|!self.archives.iter().any(|archive|archive.path.file_name().is_some_and(|n|n.to_string_lossy().eq_ignore_ascii_case(name)))).collect::<Vec<_>>();
        if pending.is_empty(){return Ok(0);}
        let paths=std::fs::read_dir(self.root.join("packfiles"))?.collect::<std::io::Result<Vec<_>>>()?;
        let before=self.archives.len();
        for name in pending {
            let path=paths.iter().find(|p|p.file_name().to_string_lossy().eq_ignore_ascii_case(name)).with_context(||format!("Resource archive disappeared: {name}"))?.path();
            if self.archives.iter().any(|archive|archive.path==path){continue;}
            let metadata=std::fs::metadata(&path)?;
            let members=crate::pack_io::index(&path)?.into_iter().map(|member|Ok((key(&member.path)?,member))).collect::<Result<Vec<_>>>()?;
            let archive=self.archives.len();
            for (identity,member) in members {
                if identity.starts_with("palettes/") && identity.ends_with(".bin")
                    && !self.names.contains_key(&identity)
                {
                    self.palette_scenes = None;
                }
                self.names.entry(identity).or_insert(self.entries.len());
                self.entries.push(Entry{archive,member});
            }
            self.archives.push(Archive{path,length:metadata.len(),modified:metadata.modified().ok()});
        }
        Ok(self.archives.len()-before)
    }
    fn resolve(&self, path: &str) -> Result<Option<usize>> {
        let name = key(path)?;
        Ok([
            self.names.get(&name),
            self.names.get(&format!("{name}_wii")),
        ]
        .into_iter()
        .flatten()
        .copied()
        .min())
    }
    pub fn read(&mut self, path: &str) -> Result<VirtualFile> {
        if let Some(index) = self.resolve(path)? {
            return self.read_index(index);
        }
        let name = super::resource_path::canonical(path.replace('\\', "/").trim_matches('/'))?;
        for candidate in [name.clone(), format!("{name}_wii")] {
            if let Some((path, actual)) = self.loose_path(&candidate)? {
                let data = std::fs::read(&actual)
                    .with_context(|| format!("Read loose game asset {path}"))?;
                self.reads = self.reads.wrapping_add(1);
                self.loose_reads = self.loose_reads.wrapping_add(1);
                self.bytes_read = self.bytes_read.wrapping_add(data.len() as u64);
                //Loose resources have no archive tag; retain original bytes.
                return Ok(VirtualFile::new(
                    EndianDependentString::new(String::new()),
                    false,
                    0,
                    path,
                    data,
                ));
            }
        }
        anyhow::bail!("Asset is absent from admitted archives and the game root: {path}")
    }
    fn loose_path(&self, name: &str) -> Result<Option<(String, PathBuf)>> {
        let mut current = self.root.clone();
        let mut names = Vec::new();
        for part in name.split('/') {
            let requested = current.join(part);
            let candidate = if requested.try_exists()? {
                requested
            } else {
                let mut found = None;
                for entry in std::fs::read_dir(&current)? {
                    let entry = entry?;
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(part)
                    {
                        ensure!(found.is_none(), "Ambiguous loose resource casing: {name}");
                        found = Some(entry.path());
                    }
                }
                let Some(found) = found else { return Ok(None) };
                found
            };
            let resolved = candidate.canonicalize()?;
            ensure!(
                resolved.starts_with(&self.root),
                "Loose resource escapes the game root: {name}"
            );
            names.push(
                candidate
                    .file_name()
                    .context("Loose resource has no name")?
                    .to_string_lossy()
                    .into_owned(),
            );
            current = resolved;
        }
        ensure!(
            current.is_file(),
            "Loose resource is not a regular file: {name}"
        );
        Ok(Some((names.join("/"), current)))
    }
    fn read_index(&mut self, index: usize) -> Result<VirtualFile> {
        let entry = &self.entries[index];
        let archive = &self.archives[entry.archive];
        let metadata = std::fs::metadata(&archive.path)?;
        ensure!(
            metadata.len() == archive.length && metadata.modified().ok() == archive.modified,
            "Game archive changed while the level was running; reload the level: {}",
            archive.path.display()
        );
        let data = entry
            .member
            .read(&archive.path)
            .with_context(|| format!("Read asset {}", entry.member.path))?;
        self.reads = self.reads.wrapping_add(1);
        self.bytes_read = self.bytes_read.wrapping_add(data.len() as u64);
        Ok(VirtualFile::new(
            EndianDependentString::new(entry.member.tag.clone()),
            false,
            0,
            entry.member.path.clone(),
            data,
        ))
    }
    /// Palette declarations only; model/texture payloads remain lazy.
    pub(super) fn palette_scenes(&mut self) -> Result<std::sync::Arc<Vec<(String, epic_mickey_lib_rs::scene_file::SceneFile)>>> {
        if let Some(scenes) = &self.palette_scenes {
            return Ok(scenes.clone());
        }
        let mut seen = BTreeSet::new();
        let indices = self.entries.iter().enumerate().filter_map(|(index, entry)| {
            let path = entry.member.path.replace('\\', "/").to_ascii_lowercase();
            (path.starts_with("palettes/") && path.ends_with(".bin") && seen.insert(path)).then_some(index)
        }).collect::<Vec<_>>();
        let mut scenes = Vec::with_capacity(indices.len());
        for index in indices {
            let file = self.read_index(index)?;
            let scene = super::scene_loading::parse_scene(&file.path, &file.data)?;
            scenes.push((file.path, scene));
        }
        let scenes = std::sync::Arc::new(scenes);
        self.palette_scenes = Some(scenes.clone());
        Ok(scenes)
    }
    fn texture(&self, path: &str) -> Result<Option<usize>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(index) = self.resolve(path)? {
            return Ok(Some(index));
        }
        let normalized = path.replace('\\', "/");
        let requested = Path::new(&normalized);
        let name = requested.file_name().unwrap_or_default().to_string_lossy();
        let stem = requested.file_stem().unwrap_or_default().to_string_lossy();
        Ok(self.entries.iter().position(|entry| {
            let actual = entry.member.path.replace('\\', "/");
            let actual = Path::new(&actual);
            let actual_name = actual.file_name().unwrap_or_default().to_string_lossy();
            let actual_stem = actual.file_stem().unwrap_or_default().to_string_lossy();
            actual_name.eq_ignore_ascii_case(&name)
                || actual_name.eq_ignore_ascii_case(&format!("{name}_wii"))
                || actual_stem.eq_ignore_ascii_case(&stem)
                || actual_stem.eq_ignore_ascii_case(&format!("{stem}_wii"))
        }))
    }
    /// Material dependencies only. The caller supplies the model separately;
    /// no archive payload is retained after its decoded resource takes ownership.
    pub fn particle_files(&mut self, model: &[u8]) -> Result<Vec<VirtualFile>> {
        let nif = NifFile::load_bytes(model).map_err(anyhow::Error::msg)?;
        let mut indices = BTreeSet::new();
        let mut loaded = BTreeMap::new();
        for (id, block) in nif.blocks.iter().enumerate() {
            if !matches!(block, NiObject::NiPSParticleSystem(_) | NiObject::NiMesh(_)) {
                continue;
            }
            let material = crate::render_support::nif::find_mesh_surface(&nif, id).material;
            if let Some(shader) = material.shader {
                let filename = format!("{shader}.r3mt");
                // Hose's shader lookup is reverse basename order, distinct
                // from the first-resource priority used by ordinary textures.
                let source = self
                    .entries
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, entry)| {
                        entry
                            .member
                            .path
                            .replace('\\', "/")
                            .rsplit('/')
                            .next()
                            .is_some_and(|name| name.eq_ignore_ascii_case(&filename))
                    })
                    .map(|(index, _)| index)
                    .with_context(|| format!("Particle shader absent: {filename}"))?;
                indices.insert(source);
                if !loaded.contains_key(&source) {
                    loaded.insert(source, self.read_index(source)?);
                }
                let shader = &loaded[&source];
                let text = std::str::from_utf8(&shader.data)?;
                let document = roxmltree::Document::parse(text)?;
                for map in document
                    .descendants()
                    .filter(|n| n.has_tag_name("TextureMap"))
                {
                    if let Some(path) = map.attribute("ImageName") {
                        let provider =
                            format!("effects/r3mt/{}_tex.nif_wii", path.to_ascii_lowercase());
                        if let Some((index, _)) =
                            self.entries.iter().enumerate().rev().find(|(_, entry)| {
                                entry
                                    .member
                                    .path
                                    .replace('\\', "/")
                                    .eq_ignore_ascii_case(&provider)
                            })
                        {
                            indices.insert(index);
                        }
                    }
                }
            }
            for texture in material.textures {
                if let Some(index) = self.texture(&texture.path)? {
                    indices.insert(index);
                }
                // Real default-SH resources remain available until a lighting
                // owner attaches a generated map; never invent image bytes.
            }
        }
        indices
            .into_iter()
            .map(|index| match loaded.remove(&index) {
                Some(file) => Ok(file),
                None => self.read_index(index),
            })
            .collect()
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"archives":self.archives.len(),"resources":self.entries.len(),"payload_reads":self.reads,
            "uncompressed_bytes_read":self.bytes_read,"loose_payload_reads":self.loose_reads,"retained_uncompressed_payload_bytes":0})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loose_resources_resolve_case_and_wii_alias_without_retaining_payloads() -> Result<()> {
        let target = std::env::current_dir()?.join("target");
        let root = target.join(format!(
            "loose-store-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("packfiles"))?;
        std::fs::create_dir_all(root.join("UI/Cooked"))?;
        std::fs::write(root.join("UI/Cooked/Hud.gfx"), b"original loose movie")?;
        std::fs::write(
            root.join("UI/Cooked/Icon.nif_wii"),
            b"original loose Wii texture",
        )?;
        let mut store = Store::new(&root, &[])?;
        let movie = store.read("ui/cooked/hud.gfx")?;
        assert_eq!(movie.data, b"original loose movie");
        assert!(movie.path.eq_ignore_ascii_case("UI/Cooked/Hud.gfx"));
        let texture = store.read("/UI/Sub/../Cooked/Icon.nif")?;
        assert_eq!(texture.data, b"original loose Wii texture");
        assert_eq!(store.loose_reads, 2);
        assert_eq!(store.summary()["retained_uncompressed_payload_bytes"], 0);
        for path in [
            "../outside.gfx",
            "UI/../../outside.gfx",
            "C:/outside.gfx",
            "UI/missing.gfx",
        ] {
            assert!(store.read(path).is_err());
        }
        assert_eq!(store.loose_reads, 2);
        let absolute = root.canonicalize()?;
        ensure!(
            absolute.starts_with(target.canonicalize()?),
            "Test cleanup escaped target"
        );
        std::fs::remove_dir_all(absolute)?;
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_loose_hud_movie_and_archive_priority_share_one_namespace() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut store = Store::new(&root, &["pausemenu_main.pak".into()])?;
        let movie = store.read("HUDElements/Flash/Cooked/mickeycontainer.gfx")?;
        assert_eq!(
            movie.data,
            std::fs::read(root.join("HUDElements/Flash/cooked/mickeycontainer.gfx"))?
        );
        let parsed = crate::gfx::GfxFile::load(&movie.data)?;
        assert!(!parsed.tags.is_empty());
        assert_eq!(store.loose_reads, 1);
        let archive = crate::pack_io::index(&root.join("packfiles/pausemenu_main.pak"))?;
        let member = archive
            .iter()
            .find(|m| m.path.ends_with("options.gfx"))
            .context("Original archive movie absent")?;
        let packed = store.read(&member.path)?;
        assert_eq!(
            packed.data,
            member.read(&root.join("packfiles/pausemenu_main.pak"))?
        );
        assert_eq!(
            store.loose_reads, 1,
            "Loose copy shadowed an admitted archive member"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_member_index_matches_archive_reader_and_avoids_unselected_payloads() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let root = Path::new(&root);
        let archive = root.join("packfiles/gremlinvillage_zonea.pak");
        let index = crate::pack_io::index(&archive)?;
        let name = "Effects/_Shared/fx_gremlin_warp_in.nif_wii";
        let member = index
            .iter()
            .find(|m| m.path.eq_ignore_ascii_case(name))
            .context("Original FX member absent")?;
        let indexed = member.read(&archive)?;
        let ordinary = crate::pack_io::read_filtered(std::fs::read(&archive)?, |_, p| {
            p.eq_ignore_ascii_case(name)
        })?;
        let original = &ordinary
            .pak
            .files
            .iter()
            .find(|f| f.path.eq_ignore_ascii_case(name))
            .unwrap()
            .data;
        assert_eq!(&indexed, original);
        let mut store = Store::new(
            root,
            &["gremlinvillage_zonea.pak".into(), "r3mt.pak".into()],
        )?;
        assert_eq!(store.reads, 0);
        let model = store.read("/Effects/_Shared/fx_gremlin_warp_in.nif")?;
        assert_eq!(model.data, indexed);
        assert_eq!(store.reads, 1);
        let files = store.particle_files(&model.data)?;
        assert_eq!(
            store.reads,
            1 + files.len() as u64,
            "Each selected dependency is read once, including shaders shared by layers"
        );
        let hose = super::super::brush_particles::Hose::load_attached(&model.data, &files)?;
        assert!(store.reads < (index.len() as u64));
        assert_eq!(store.summary()["retained_uncompressed_payload_bytes"], 0);
        drop(hose);
        Ok(())
    }
}
