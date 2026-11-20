//! Original scene activation admission and source preparation.
//! REL295124 queues exact names,295378 cancels matching slots without reclaiming
//! ring occupancy,295acc consumes at most one slot when the loader is idle.
//! A prepared source is not a published/active runtime scene.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    file_manipulator::EndianType, packfile::VirtualFile, scene_file::SceneFile,
};
use std::{collections::VecDeque, path::Path, sync::{Arc, mpsc}};

#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Queued,
    AlreadyPending,
    Full,
}
#[derive(Default)]
pub struct Requests {
    pending: VecDeque<Option<Arc<str>>>,
    preparation: Option<Preparation>,
}
struct Preparation {
    name: Arc<str>,
    receiver: mpsc::Receiver<Result<Source>>,
    result: Option<Result<Source>>,
}

impl Requests {
    pub(super) fn prepare_front(
        &mut self,
        root: &Path,
        ctx: &egui::Context,
        published: &[String],
    ) -> bool {
        let Some(name) = self.pending.front().and_then(Option::as_ref) else {
            return true;
        };
        if published.iter().any(|path| matches_scene(path, name)) {
            return true;
        }
        let preparation = self.preparation.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::channel();
            let name = Arc::clone(name);
            let requested = Arc::clone(&name);
            let root = root.to_owned();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = sender.send(Source::read(&root, requested));
                ctx.request_repaint();
            });
            Preparation { name, receiver, result: None }
        });
        preparation.poll()
    }

    pub(super) fn take_prepared(&mut self, name: &str) -> Option<Result<Source>> {
        if self.preparation.as_ref().is_some_and(|load| load.name.as_ref() == name) {
            return self.preparation.take().and_then(|load| load.result);
        }
        None
    }

    /// This is the native ring capacity, not an editor/import limit.
    pub const CAPACITY: usize = 32;
    pub fn request(&mut self, name: &str) -> Admission {
        let name = name.split('\0').next().unwrap_or("");
        if self
            .pending
            .iter()
            .flatten()
            .any(|value| value.as_ref() == name)
        {
            return Admission::AlreadyPending;
        }
        if self.pending.len() == Self::CAPACITY {
            return Admission::Full;
        }
        self.pending.push_back(Some(Arc::from(name)));
        Admission::Queued
    }
    pub fn cancel(&mut self, name: &str) {
        let name = name.split('\0').next().unwrap_or("");
        if self.preparation.as_ref().is_some_and(|load| load.name.as_ref() == name) {
            self.preparation = None;
        }
        for slot in &mut self.pending {
            if slot.as_deref() == Some(name) {
                *slot = None;
            }
        }
    }
    pub fn occupied(&self) -> usize {
        self.pending.len()
    }
    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.pending.iter().filter_map(|slot| slot.as_deref())
    }
    /// None means busy/empty; Some(None) consumes a cancelled native slot.
    /// Do not drain multiple slots just because the first one was cancelled.
    pub fn consume(&mut self, loader_busy: bool) -> Option<Option<Arc<str>>> {
        if loader_busy || self.preparation.as_mut().is_some_and(|load| !load.poll()) {
            None
        } else {
            self.pending.pop_front()
        }
    }
}

impl Preparation {
    fn poll(&mut self) -> bool {
        if self.result.is_none() {
            self.result = match self.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                    "Scene preparation stopped while loading {}", self.name
                ))),
            };
        }
        self.result.is_some()
    }
}

pub(super) fn matches_scene(path: &str, request: &str) -> bool {
    let normalize = |path: &str| {
        super::resource_path::canonical(path).unwrap_or_else(|_| path.replace('\\', "/"))
    };
    let path = normalize(path);
    let request = normalize(request);
    let path = path.trim_matches('/');
    let request = request.trim_matches('/');
    if path.eq_ignore_ascii_case(request) {
        return true;
    }
    match (path.rsplit_once('.'), request.rsplit_once('.')) {
        (Some((loaded, ext)), Some((requested, other))) => {
            ext.eq_ignore_ascii_case("bin") && other.eq_ignore_ascii_case("gsa")
                && loaded.eq_ignore_ascii_case(requested)
        }
        _ => false,
    }
}

pub struct Source {
    pub requested: Arc<str>,
    pub archive:String,
    pub path: String,
    pub scene: SceneFile,
    /// Temporary catalog for one incoming scene publication. Dropped as soon
    /// as the resources required by the live world have been prepared.
    pub files: Vec<VirtualFile>,
}
pub fn parse_scene(path: &str, bytes: &[u8]) -> Result<SceneFile> {
    // Version-two scene headers have a symmetric magic. Resolve byte order
    // from the bounded version-marker pointer, rather than from the filename.
    let endian = if bytes.get(..4) == Some(&[1, 0, 0, 1]) {
        ensure!(bytes.len() >= 8, "Truncated scene header for {path}");
        let offset: [u8;4] = bytes[4..8].try_into()?;
        let valid = |offset: u32, little: bool| {
            (offset as usize).checked_add(4).and_then(|at| bytes.get(at..at+4))
                .is_some_and(|marker| {
                    let marker: [u8;4] = marker.try_into().unwrap();
                    matches!(if little { u32::from_le_bytes(marker) } else { u32::from_be_bytes(marker) }, 0x02000002 | 0x02000001)
                })
        };
        match (valid(u32::from_be_bytes(offset), false), valid(u32::from_le_bytes(offset), true)) {
            (true, false) => EndianType::BIG,
            (false, true) => EndianType::LITTLE,
            _ => anyhow::bail!("Invalid or ambiguous scene version header for {path}"),
        }
    } else { EndianType::BIG };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SceneFile::from_binary(bytes, endian)
    }))
    .map_err(|_| anyhow::anyhow!("Scene parser panicked for {path}"))
}
impl Source {
    pub fn read(root: &Path, requested: Arc<str>) -> Result<Self> {
        let path = requested.replace('\\', "/");
        ensure!(
            !Path::new(&path).is_absolute()
                && !path.contains(':')
                && !path.split('/').any(|part| part == ".."),
            "Scene must be a relative game asset path"
        );
        let path = if path.to_ascii_lowercase().ends_with(".gsa") {
            format!("{}.bin", &path[..path.len() - 4])
        } else {
            ensure!(
                path.to_ascii_lowercase().ends_with(".bin"),
                "Unsupported scene resource extension"
            );
            path
        };
        let stem = Path::new(&path)
            .file_stem()
            .context("Scene filename missing")?
            .to_string_lossy();
        let archive = format!("{stem}.pak");
        let archive = std::fs::read_dir(root.join("packfiles"))?
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&archive)
            })
            .with_context(|| format!("Required streamed scene archive missing: {archive}"))?
            .path();
        let archive_name=archive.file_name().context("Scene archive filename missing")?.to_string_lossy().into_owned();
        let files = crate::pack_io::read(std::fs::read(archive)?)?.pak.files;
        let file = files
            .iter()
            .find(|file| file.path.replace('\\', "/").eq_ignore_ascii_case(&path))
            .with_context(|| format!("Streamed scene missing from its archive: {path}"))?;
        // Keep parser panic information at this boundary, rather than losing
        // the request identity in an unrelated worker thread failure.
        let scene = parse_scene(&file.path, &file.data)?;
        let path = file.path.clone();
        Ok(Self {
            requested,
            archive:archive_name,
            path,
            scene,
            files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_name_dedup_cancelled_slots_busy_and_native_capacity() {
        let mut requests = Requests::default();
        assert_eq!(requests.request("World/A.gsa"), Admission::Queued);
        assert_eq!(
            requests.request("World/A.gsa\0ignored"),
            Admission::AlreadyPending
        );
        assert_eq!(requests.request("world/a.gsa"), Admission::Queued);
        requests.cancel("World/A.gsa");
        assert_eq!(requests.occupied(), 2);
        assert_eq!(requests.consume(true), None);
        assert_eq!(requests.occupied(), 2);
        assert_eq!(requests.consume(false), Some(None));
        assert_eq!(
            requests.consume(false).flatten().as_deref(),
            Some("world/a.gsa")
        );
        for i in 0..Requests::CAPACITY {
            assert_eq!(requests.request(&format!("{i}.gsa")), Admission::Queued);
        }
        requests.cancel("0.gsa");
        assert_eq!(requests.request("extra.gsa"), Admission::Full);
        assert_eq!(requests.request("1.gsa"), Admission::AlreadyPending);
        assert_eq!(requests.consume(false), Some(None));
        assert_eq!(requests.request("extra.gsa"), Admission::Queued);
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_airlock_source_prepares_without_publishing_entities() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let source = Source::read(
            Path::new(&root),
            Arc::from("Environments/thepark/fantasyLand/SmallWorld/GSA/GV_ZoneA_airlock.gsa"),
        )?;
        assert!(source
            .path
            .to_ascii_lowercase()
            .ends_with("/gv_zonea_airlock.bin"));
        ensure!(
            !source.scene.scene.is_empty(),
            "Airlock has no placed objects"
        );
        ensure!(
            source.scene.objects.iter().any(|entity| entity
                .components
                .iter()
                .any(|component| component.class_name == "JPSLightComponent")),
            "Expected authored airlock lighting"
        );
        ensure!(
            source
                .files
                .iter()
                .any(|file| file.path.to_ascii_lowercase().ends_with(".nif_wii")),
            "Expected retained airlock model resources"
        );
        let path = source.path;
        let count = source.scene.scene.len();
        let bytes = source
            .files
            .iter()
            .map(|file| file.data.len())
            .sum::<usize>();
        println!(
            "Prepared {path}: {count} placed entries, {bytes} resource bytes; no world publication"
        );
        Ok(())
    }
}
