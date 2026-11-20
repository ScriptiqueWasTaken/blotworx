use anyhow::{anyhow, ensure, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    },
};

pub(super) const START_LEVEL: &str = "Levels/Main_Menu.Level";
const FORMATS: &[&str] = &["iso", "gcm", "rvz", "wia", "wbfs", "ciso", "gcz", "nfs"];
const MAX_DISC_SIZE: u64 = 9_000_000_000;

fn library() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("Local application storage is unavailable")?
        .join("Blotworx Engine")
        .join("game"))
}

pub(super) fn installed() -> Option<PathBuf> {
    let path = library().ok()?.join("DATA/files");
    valid_files(&path).then_some(path)
}

pub(super) fn valid_files(path: &Path) -> bool {
    path.join("packfiles/levels.pak").is_file()
        && path
            .join("HUDElements/Flash/cooked/startscreen.gfx")
            .is_file()
}

#[derive(Default)]
struct Progress {
    total: AtomicU64,
    copied: AtomicU64,
}

fn safe_name(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !name.is_empty()
            && !name.ends_with([' ', '.'])
            && !name
                .chars()
                .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
            && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit()),
        "Unsupported filename in the disc directory: {name:?}"
    );
    Ok(())
}

fn import(image: &Path, destination: &Path, progress: &Progress) -> Result<PathBuf> {
    let zipped = image
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
    let unpacked = zipped.then(|| unpack_image(image)).transpose()?;
    let image = unpacked.as_ref().map_or(image, |file| file.path());
    let disc = nod::Disc::new(image).context("Read Wii disc image")?;
    ensure!(
        disc.header().game_id.starts_with(b"SEM") && disc.header().is_wii(),
        "Choose an Epic Mickey Wii disc image. This demo uses the first game's campaign."
    );
    let mut partition = disc
        .open_partition_kind(nod::PartitionKind::Data)
        .context("Read the Wii game partition")?;
    let meta = partition.meta().context("Read the game directory")?;
    let fst = meta
        .fst()
        .map_err(|error| anyhow!("The Wii file table is damaged. {error}"))?;
    let mut dirs = vec![(fst.nodes.len(), PathBuf::new())];
    let mut files = Vec::new();
    for (index, node, name) in fst.iter() {
        while dirs.last().is_some_and(|(end, _)| index >= *end) {
            dirs.pop();
        }
        let (end, parent) = dirs.last().context("Invalid Wii directory bounds")?;
        let name = name.map_err(|error| anyhow!(error))?;
        safe_name(&name)?;
        let path = parent.join(name.as_ref());
        if node.is_dir() {
            let child_end = usize::try_from(node.length())?;
            ensure!(
                child_end > index && child_end <= *end,
                "Invalid Wii child directory"
            );
            dirs.push((child_end, path));
        } else {
            ensure!(node.is_file(), "Unknown Wii directory entry");
            files.push((node, path));
        }
    }
    let byte_count = files
        .iter()
        .try_fold(0u64, |sum, (node, _)| sum.checked_add(node.length()))
        .context("Invalid total length of Wii files")?;
    ensure!(
        byte_count > 0 && byte_count <= MAX_DISC_SIZE,
        "Invalid game asset size"
    );
    progress.total.store(byte_count, Ordering::Relaxed);
    let parent = destination.parent().context("Missing library directory")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".game-import-")
        .tempdir_in(parent)?;
    let game_files = staging.path().join("DATA/files");
    fs::create_dir_all(&game_files)?;
    let sys = staging.path().join("DATA/sys");
    fs::create_dir_all(&sys)?;
    for (name, bytes) in [
        ("boot.bin", meta.raw_boot.as_slice()),
        ("bi2.bin", meta.raw_bi2.as_slice()),
        ("apploader.img", &*meta.raw_apploader),
        ("main.dol", &*meta.raw_dol),
        ("fst.bin", &*meta.raw_fst),
    ] {
        fs::write(sys.join(name), bytes)?;
    }
    files.sort_unstable_by_key(|(node, _)| node.offset(true));
    for (node, path) in files {
        let output = game_files.join(&path);
        fs::create_dir_all(output.parent().context("Missing game asset directory")?)?;
        let mut output = BufWriter::with_capacity(
            128 * 1024,
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&output)
                .with_context(|| format!("Create {}", path.display()))?,
        );
        let mut file = partition
            .open_file(node)
            .with_context(|| format!("Read {}", path.display()))?;
        let length = io::copy(&mut file, &mut output)?;
        ensure!(
            length == node.length(),
            "Disc image ended early reading {}",
            path.display()
        );
        output.flush()?;
        progress.copied.fetch_add(length, Ordering::Relaxed);
    }
    ensure!(
        valid_files(&game_files),
        "The disc is missing its level or title-screen data"
    );
    ensure!(
        game_files.join("packfiles/English_NTSC.pak").is_file(),
        "This demo needs the North American Wii release with English game data"
    );
    let result = destination.join("DATA/files");
    if let Err(error) = fs::rename(staging.path(), destination) {
        // A second instance may have completed setup during extraction.
        if !valid_files(&result) {
            return Err(error).context("Finish installing Epic Mickey");
        }
    }
    Ok(result)
}

fn unpack_image(path: &Path) -> Result<tempfile::NamedTempFile> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
    let entries: Vec<_> = archive
        .file_names()
        .filter(|name| {
            Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    FORMATS
                        .iter()
                        .any(|format| extension.eq_ignore_ascii_case(format))
                })
        })
        .map(str::to_owned)
        .collect();
    ensure!(
        entries.len() == 1,
        "The ZIP must contain one Wii disc image"
    );
    let name = &entries[0];
    let file = archive.by_name(name)?;
    let size = file.size();
    ensure!(
        (1..=MAX_DISC_SIZE).contains(&size),
        "Invalid disc image size in ZIP"
    );
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap();
    let mut image = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()?;
    let copied = io::copy(&mut file.take(MAX_DISC_SIZE + 1), &mut image)?;
    ensure!(copied == size, "Disc image in ZIP is incomplete");
    image.flush()?;
    Ok(image)
}

enum Event {
    Image(PathBuf),
    Folder(PathBuf),
}

#[derive(Default)]
pub(super) struct Setup {
    job: Option<(Arc<Progress>, mpsc::Receiver<Result<PathBuf>>)>,
    error: Option<String>,
}

impl Setup {
    pub(super) fn start(&mut self, image: PathBuf) {
        if self.job.is_some() {
            return;
        }
        self.error = None;
        let progress = Arc::new(Progress::default());
        let work = progress.clone();
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("Blotworx disc import".into())
            .spawn(move || {
                let destination = library();
                let _ = tx.send(destination.and_then(|path| import(&image, &path, &work)));
            }) {
            Ok(_) => self.job = Some((progress, rx)),
            Err(error) => self.error = Some(format!("Could not start disc import. {error}")),
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        launch_error: Option<&str>,
    ) -> Option<PathBuf> {
        if let Some((_, rx)) = self.job.as_ref() {
            match rx.try_recv() {
                Ok(Ok(root)) => {
                    self.job = None;
                    return Some(root);
                }
                Ok(Err(error)) => {
                    self.job = None;
                    self.error = Some(format!("{error:#}"));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    self.error = Some("Disc import stopped. Choose the image to try again.".into());
                }
                Err(mpsc::TryRecvError::Empty) => ui
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(80)),
            }
        }
        let mut selected = None;
        if self.job.is_none() {
            if let Some(image) = ui.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .map(|file| file.path())
                    .find(|path| path.is_file())
                    .map(Path::to_owned)
            }) {
                self.start(image);
            }
        }
        layout_setup(ui, |ui| {
            ui.label(egui::RichText::new("Blotworx").size(34.).strong());
            ui.add_space(5.);
            ui.heading("Set up Epic Mickey");
            ui.add_space(12.);
            if let Some((progress, _)) = &self.job {
                let total = progress.total.load(Ordering::Relaxed);
                if total == 0 {
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label("Reading the disc image…");
                    });
                } else {
                    let done = progress.copied.load(Ordering::Relaxed);
                    ui.label(format!(
                        "Importing game files  {:.1} / {:.1} GB",
                        done as f64 / 1e9,
                        total as f64 / 1e9
                    ));
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32));
                }
            } else {
                ui.label("Choose or drop your Epic Mickey Wii disc image here.");
                ui.weak("ISO, RVZ, WBFS, WIA, CISO, GCZ, NFS or ZIP");
                ui.add_space(12.);
                if ui
                    .add_sized(
                        [ui.available_width(), 38.],
                        egui::Button::new("Choose disc image"),
                    )
                    .clicked()
                {
                    selected = rfd::FileDialog::new()
                        .add_filter("Wii disc images", FORMATS)
                        .add_filter("ZIP archives", &["zip"])
                        .pick_file()
                        .map(Event::Image);
                }
                if ui.link("Use an extracted game folder").clicked() {
                    selected = rfd::FileDialog::new().pick_folder().map(Event::Folder);
                }
                if let Some(error) = self.error.as_deref().or(launch_error) {
                    ui.add_space(8.);
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            }
        });
        match selected {
            Some(Event::Image(image)) => self.start(image),
            Some(Event::Folder(folder)) => {
                let files = super::assets::files_root(&folder);
                if valid_files(&files) {
                    return Some(files);
                }
                self.error =
                    Some("This folder does not contain the extracted Epic Mickey Wii game.".into());
            }
            None => {}
        }
        None
    }
}

fn layout_setup(ui: &mut egui::Ui, draw: impl FnOnce(&mut egui::Ui)) {
    let top = (ui.available_height() * 0.14).clamp(8., 96.);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_width();
            let outer_width = (available - 24.).clamp(1., 500.);
            let padding = (outer_width * 0.065).clamp(2., 28.);
            let width = (outer_width - 2. * padding - 2.).max(1.);
            ui.add_space(top);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                ui.add_space(((available - outer_width) * 0.5).max(0.));
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(25, 30, 40))
                    .stroke(egui::Stroke::new(1., egui::Color32::from_rgb(51, 61, 78)))
                    .corner_radius(16)
                    .inner_margin(padding)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = 7.;
                            draw(ui);
                        });
                    });
            });
            ui.add_space(12.);
        });
}

#[cfg(debug_assertions)]
pub(super) fn verify_import(image: &Path, destination: &Path) -> Result<()> {
    println!(
        "{}",
        import(image, destination, &Progress::default())?.display()
    );
    Ok(())
}
