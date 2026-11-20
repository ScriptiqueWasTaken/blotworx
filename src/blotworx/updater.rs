use anyhow::{ensure, Context, Result};
use semver::Version;
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

const ASSET: &str = "Blotworx-Windows-x64.exe";
const RELEASES: &str = "https://api.github.com/repos/Bloticle/blotworx/releases?per_page=30";
const DOWNLOAD: &str = "https://github.com/Bloticle/blotworx/releases/download";
const LIMIT: u64 = 300 * 1024 * 1024;

#[derive(Deserialize)]
struct Build {
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    draft: bool,
    assets: Vec<Build>,
}

#[derive(Clone)]
struct Available {
    version: Version,
    title: String,
    notes: String,
    url: String,
    digest: String,
    size: u64,
}

fn cache() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("Local update storage is unavailable")?
        .join("Blotworx Engine")
        .join("Updates"))
}

fn client(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn latest() -> Result<Option<Available>> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let bytes = client(Duration::from_secs(12))
        .get(RELEASES)
        .header("User-Agent", "Blotworx")
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_vec()?;
    let releases: Vec<Release> = serde_json::from_slice(&bytes)?;
    Ok(releases
        .into_iter()
        .filter_map(|release| {
            if release.draft {
                return None;
            }
            let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
            if version <= current || release.tag_name != format!("v{version}") {
                return None;
            }
            let asset = release
                .assets
                .iter()
                .find(|build| build.name == ASSET && (1..=LIMIT).contains(&build.size))?;
            let digest = asset.digest.as_deref()?.strip_prefix("sha256:")?;
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            Some(Available {
                url: format!("{DOWNLOAD}/{}/{ASSET}", release.tag_name),
                title: release
                    .name
                    .unwrap_or_else(|| format!("Blotworx {version}")),
                notes: release.body.unwrap_or_default(),
                digest: digest.to_ascii_lowercase(),
                size: asset.size,
                version,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version)))
}

enum Message {
    Available(Result<Option<Available>>),
    Progress(u64),
    Downloaded(Result<PathBuf>),
}

fn download(build: &Available, tx: &mpsc::Sender<Message>) -> Result<PathBuf> {
    let root = cache()?;
    fs::create_dir_all(&root)?;
    let mut file = tempfile::Builder::new()
        .prefix("update-")
        .suffix(".exe")
        .tempfile_in(&root)?;
    let mut response = client(Duration::from_secs(900))
        .get(&build.url)
        .header("User-Agent", "Blotworx")
        .call()?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(build.size + 1)
        .reader();
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; 128 * 1024];
    let mut copied = 0;
    let mut reported = 0;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        copied += n as u64;
        ensure!(copied <= build.size, "Update exceeds the GitHub asset size");
        digest.update(&buffer[..n]);
        file.write_all(&buffer[..n])?;
        if copied - reported >= 1024 * 1024 {
            if tx.send(Message::Progress(copied)).is_err() {
                anyhow::bail!("Update download closed");
            }
            reported = copied;
        }
    }
    ensure!(
        copied == build.size && hex(digest.finish()) == build.digest,
        "The downloaded update did not match GitHub's checksum"
    );
    file.as_file().sync_all()?;
    Ok(file.into_temp_path().keep()?)
}

fn hex(hash: ring::digest::Digest) -> String {
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Default)]
pub(super) struct Updater {
    receiver: Option<Receiver<Message>>,
    build: Option<Available>,
    copied: Option<u64>,
    downloaded: Option<PathBuf>,
    error: Option<String>,
}

impl Updater {
    pub(super) fn on_launch(enabled: bool) -> Self {
        let mut state = Self::default();
        if enabled && cfg!(all(windows, target_arch = "x86_64")) {
            let (tx, rx) = mpsc::channel();
            if std::thread::Builder::new()
                .name("Blotworx update check".into())
                .spawn(move || {
                    clean_staging();
                    let _ = tx.send(Message::Available(latest()));
                })
                .is_ok()
            {
                state.receiver = Some(rx);
            }
        }
        state
    }

    fn start_download(&mut self, build: Available) {
        let (tx, rx) = mpsc::channel();
        self.error = None;
        match std::thread::Builder::new()
            .name("Blotworx download".into())
            .spawn(move || {
                let result = download(&build, &tx);
                let _ = tx.send(Message::Downloaded(result));
            }) {
            Ok(_) => {
                self.copied = Some(0);
                self.receiver = Some(rx);
            }
            Err(error) => self.error = Some(format!("Could not start the download. {error}")),
        }
    }

    pub(super) fn show(&mut self, context: &egui::Context) {
        if let Some(rx) = &self.receiver {
            let mut complete = false;
            for message in rx.try_iter() {
                match message {
                    Message::Available(result) => {
                        self.build = result.unwrap_or_else(|error| {
                            log::debug!("Update check failed: {error:#}");
                            None
                        });
                        complete = true;
                    }
                    Message::Progress(bytes) => self.copied = Some(bytes),
                    Message::Downloaded(result) => {
                        match result {
                            Ok(file) => self.downloaded = Some(file),
                            Err(error) => self.error = Some(format!("{error:#}")),
                        }
                        self.copied = None;
                        complete = true;
                    }
                }
            }
            if complete {
                self.receiver = None;
            } else {
                context.request_repaint_after(Duration::from_millis(160));
            }
        }
        let Some(build) = self.build.as_ref() else {
            return;
        };
        let mut dismiss = false;
        let mut install = false;
        let mut start = false;
        egui::Modal::new(egui::Id::new("blotworx-update")).show(context, |ui| {
            ui.set_width(460_f32.min((context.viewport_rect().width() - 72.).max(210.)));
            ui.weak(format!("UPDATE AVAILABLE  ·  {}", build.version));
            ui.heading(&build.title);
            ui.add_space(10.);
            if !build.notes.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(210.)
                    .show(ui, |ui| {
                        let notes: String = build.notes.chars().take(4000).collect();
                        ui.label(notes);
                    });
            }
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            ui.add_space(12.);
            if let Some(copied) = self.copied {
                ui.add(
                    egui::ProgressBar::new(copied as f32 / build.size as f32)
                        .text(format!("Downloading  {:.1} MB", copied as f64 / 1e6)),
                );
            } else {
                ui.horizontal(|ui| {
                    if self.downloaded.is_some() {
                        install = ui.button("Install and restart").clicked();
                    } else {
                        start = ui
                            .button(format!(
                                "Download update · {:.0} MB",
                                build.size as f64 / 1e6
                            ))
                            .clicked();
                    }
                    dismiss = ui.button("Not now").clicked();
                });
            }
        });
        if dismiss {
            if let Some(file) = self.downloaded.take() {
                let _ = fs::remove_file(file);
            }
            self.build = None;
        } else if start {
            self.start_download(build.clone());
        } else if install {
            match launch_installer(self.downloaded.as_ref().unwrap(), &build.digest) {
                Ok(()) => context.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error) => {
                    self.error = Some(format!("Could not start the installer. {error:#}"))
                }
            }
        }
    }
}

fn launch_installer(exe: &Path, digest: &str) -> Result<()> {
    let destination = std::env::current_exe()?;
    let dir = destination
        .parent()
        .context("Missing application directory")?;
    drop(tempfile::NamedTempFile::new_in(dir).context("Blotworx's folder is not writable")?);
    let mut command = std::process::Command::new(exe);
    command
        .arg("--apply-update")
        .arg(&destination)
        .arg(digest)
        .args(std::env::args_os().skip(1));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The downloaded helper lives in the cache. Let it use the app-local
        // Visual C++ runtime shipped beside the installed executable.
        let path = std::env::var_os("PATH").unwrap_or_default();
        let runtime_search = std::env::join_paths(
            std::iter::once(dir.to_owned()).chain(std::env::split_paths(&path)),
        )?;
        command
            .env("PATH", runtime_search)
            .env_remove(crate::startup_log::GAME_PROCESS_ENV)
            .creation_flags(0x08000000);
    }
    command.spawn()?;
    Ok(())
}

pub(super) fn finish_if_requested() -> Result<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--apply-update")) {
        return Ok(false);
    }
    let destination = PathBuf::from(args.next().context("Missing update destination")?);
    let digest = args.next().context("Missing update checksum")?;
    let relaunch: Vec<_> = args.collect();
    let current = std::env::current_exe()?;
    ensure!(
        current.parent() == Some(cache()?.as_path()) && current != destination,
        "The update helper must run from Blotworx's update storage"
    );
    ensure!(
        destination.is_absolute() && destination.is_file(),
        "Invalid update destination"
    );
    let dir = destination
        .parent()
        .context("Missing application directory")?;
    let stage = tempfile::NamedTempFile::new_in(dir)?;
    let mut source = File::open(&current)?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; 128 * 1024];
    let mut output = stage.as_file();
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n])?;
    }
    ensure!(
        digest == hex(hash.finish()).as_str(),
        "Update checksum changed before installation"
    );
    stage.as_file().sync_all()?;
    let stage = stage.into_temp_path();
    let old = tempfile::NamedTempFile::new_in(dir)?.into_temp_path();
    for attempt in 0..240 {
        match fs::rename(&destination, &old) {
            Ok(()) => break,
            Err(error) if attempt == 239 => {
                return Err(error).context("Wait for Blotworx to close")
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    if let Err(error) = fs::rename(&stage, &destination) {
        fs::rename(&old, &destination).context("Restore the previous Blotworx executable")?;
        return Err(error).context("Replace Blotworx");
    }
    if let Err(error) = std::process::Command::new(&destination)
        .args(&relaunch)
        .spawn()
    {
        fs::rename(&old, &destination).context("Restore the previous Blotworx executable")?;
        let _ = std::process::Command::new(&destination)
            .args(&relaunch)
            .spawn();
        return Err(error).context("Restart Blotworx");
    }
    Ok(true)
}

fn clean_staging() {
    if let Ok(entries) = cache().and_then(|dir| fs::read_dir(dir).map_err(Into::into)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Remove finished downloads from previous processes, never the
            // running helper or another instance's active download.
            if name.starts_with("update-")
                && name.ends_with(".exe")
                && entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|time| {
                        time.elapsed()
                            .is_ok_and(|age| age >= Duration::from_secs(3600))
                    })
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
