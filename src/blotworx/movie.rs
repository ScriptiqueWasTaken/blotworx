//! Loose BIKi movie handoff. Codec processes never execute game code.
//! Native request ordering/defaults: REL1f8c44, 1f878c, 1f8800.
//! Aspect policy, optional flag meanings and global MovieState events still
//! require reference validation; this is functional playback, not full parity.
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) enum FrontendSkip {
    Unskippable,
    AcceptAfter(u32),
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Request {
    pub serial: u64,
    pub filename: String,
    pub flags: [bool; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) frontend_skip: Option<FrontendSkip>,
}
#[derive(Default)]
pub struct Requests {
    pub pending: Option<Request>,
    ending: std::collections::VecDeque<String>,
    ending_total: usize,
    startup: Option<std::collections::VecDeque<(String, FrontendSkip)>>,
    title_ready: bool,
    pub submitted: u64,
    pub completed: u64,
    pub last_completed: Option<String>,
}
impl Requests {
    pub fn lua(
        &mut self,
        lua: &mlua::Lua,
        args: mlua::MultiValue,
    ) -> mlua::Result<mlua::MultiValue> {
        // The original pops at most the final four values, ignoring the entity.
        if args.len() < 2 {
            return Err(mlua::Error::external(
                "PlayMovieInternal requires entity and filename",
            ));
        }
        let start = args.len().saturating_sub(4);
        let Some(filename) = lua.coerce_string(args[start + 1].clone())? else {
            return Ok(vec![mlua::Value::Boolean(false)].into());
        };
        let filename = filename.to_str()?.to_owned();
        if filename.is_empty() {
            return Ok(vec![mlua::Value::Boolean(false)].into());
        }
        let truth = |index: usize| {
            args.get(start + index).map_or(true, |value| {
                !matches!(value, mlua::Value::Nil | mlua::Value::Boolean(false))
            })
        };
        self.submitted = self
            .submitted
            .checked_add(1)
            .ok_or_else(|| mlua::Error::external("Movie request counter overflow"))?;
        self.pending = Some(Request {
            serial: self.submitted,
            filename,
            flags: [truth(2), truth(3)],
            frontend_skip: None,
        });
        Ok(vec![mlua::Value::Boolean(true)].into())
    }
    pub(super) fn start_ending(&mut self, paths: Vec<String>) {
        self.ending_total = paths.len();
        self.ending = paths.into();
    }

    /// Retail RELc4ba8..c4ca0 reads consecutive name/value pairs. StartScreen
    /// is the terminal frontend command, while .bik entries feed playback.
    pub(super) fn start_frontend(&mut self, sequence: &str) -> Result<()> {
        let movies = if sequence == "StartScreen:0" {
            ""
        } else {
            sequence.strip_suffix(":StartScreen:0")
                .context("Frontend sequence has no supported title-screen command")?
        };
        let mut tokens = movies.split_terminator(':');
        let mut clips = std::collections::VecDeque::new();
        while let Some(filename) = tokens.next() {
            ensure!(filename.to_ascii_lowercase().ends_with(".bik"),
                "Unsupported frontend sequence entry {filename:?}");
            let control = tokens.next().context("Missing frontend movie control word")?.parse::<i32>()?;
            let skip = match control {
                -1 => FrontendSkip::Unskippable,
                0.. => FrontendSkip::AcceptAfter(control as u32),
                _ => anyhow::bail!("Unsupported frontend movie control word {control}"),
            };
            clips.push_back((filename.to_owned(), skip));
        }
        self.startup = Some(clips);
        self.title_ready = false;
        Ok(())
    }

    pub(super) fn take_next(&mut self) -> Option<Request> {
        if let Some(pending) = self.pending.take() {
            return Some(pending);
        }
        if let Some(startup) = &mut self.startup {
            if let Some((filename, skip)) = startup.pop_front() {
                self.submitted += 1;
                return Some(Request { serial: self.submitted, filename, flags: [true, true], frontend_skip: Some(skip) });
            }
            self.startup = None;
            self.title_ready = true;
        }
        let filename = self.ending.pop_front()?;
        self.submitted += 1;
        Some(Request { serial: self.submitted, filename, flags: [true, true], frontend_skip: None })
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"pending":self.pending,"submitted":self.submitted,"completed":self.completed,"last_completed":self.last_completed,"ending_total":self.ending_total,"ending_remaining":self.ending,"startup_remaining":self.startup,"title_ready":self.title_ready})
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Header {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub fps: [u32; 2],
    pub audio_tracks: u32,
    pub audio: Option<(u32, u16)>,
    pub audio_streams: Vec<super::movie_tracks::Track>,
}
impl Header {
    pub fn read(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut bytes = [0u8; 44];
        file.read_exact(&mut bytes)?;
        ensure!(
            &bytes[..4] == b"BIKi",
            "Unsupported Bink variant {:?}",
            &bytes[..4]
        );
        let word =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        ensure!(
            u64::from(word(4)) + 8 == length,
            "Bink declared size differs from file length"
        );
        let (frames, width, height) = (word(8), word(20), word(24));
        let fps = [word(28), word(32)];
        let tracks = word(40);
        ensure!(
            frames > 0 && width > 0 && height > 0 && fps[0] > 0 && fps[1] > 0,
            "Invalid Bink dimensions/frame rate"
        );
        ensure!(word(16) == frames, "Bink repeated frame count mismatch");
        let table = 44u64
            .checked_add(u64::from(tracks) * 12)
            .context("Bink table overflow")?;
        ensure!(
            table + (u64::from(frames) + 1) * 4 <= length,
            "Truncated Bink track/frame table"
        );
        let mut audio_streams=Vec::new();
        file.seek(SeekFrom::Start(44+u64::from(tracks)*4))?;
        let mut formats=Vec::new();
        for _ in 0..tracks {
            let mut b=[0;4];file.read_exact(&mut b)?;let packed=u32::from_le_bytes(b);
            let rate=packed&0xffff;ensure!(rate>0,"Invalid Bink audio rate");
            formats.push((rate,if packed&0x2000_0000!=0 {2}else{1}));
        }
        for (rate,channels) in formats {
            let mut b=[0;4];file.read_exact(&mut b)?;
            audio_streams.push(super::movie_tracks::Track {id:u32::from_le_bytes(b),rate,channels});
        }
        let audio=audio_streams.first().map(|track|(track.rate,track.channels));
        file.seek(SeekFrom::Start(table))?;
        let mut previous = 0;
        for index in 0..=frames {
            let mut b = [0; 4];
            file.read_exact(&mut b)?;
            let offset = u64::from(u32::from_le_bytes(b) & !1);
            ensure!(
                offset >= table + (u64::from(frames) + 1) * 4
                    && offset >= previous
                    && offset <= length,
                "Invalid Bink frame offset {index}"
            );
            previous = offset;
        }
        let size = u64::from(width) * u64::from(height) * 4;
        ensure!(
            usize::try_from(size).is_ok(),
            "Movie frame exceeds address space"
        );
        Ok(Self {
            frames,
            width,
            height,
            fps,
            audio_tracks: tracks,
            audio,
            audio_streams,
        })
    }
    fn duration(&self) -> f64 {
        f64::from(self.frames) * f64::from(self.fps[1]) / f64::from(self.fps[0])
    }
}
pub fn resolve(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = name.replace('\\', "/");
    let mut path = root.join("Movies");
    for part in relative.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.contains(':'),
            "Invalid movie path {name:?}"
        );
        let exact = path.join(part);
        path = if exact.exists() {
            exact
        } else {
            let mut matches = std::fs::read_dir(&path)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(part)
                });
            let found = matches
                .next()
                .with_context(|| format!("Movie path component {part:?} not found"))?
                .path();
            ensure!(
                matches.next().is_none(),
                "Ambiguous movie path component {part:?}"
            );
            found
        };
    }
    let base = root.join("Movies").canonicalize()?;
    let path = path.canonicalize()?;
    ensure!(
        path.starts_with(base) && path.is_file(),
        "Movie path escapes Movies directory"
    );
    Ok(path)
}
struct Cancellation {
    stop: AtomicBool,
    children: Mutex<Vec<Arc<Mutex<Child>>>>,
}
impl Cancellation {
    fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(children) = self.children.lock() {
            for child in children.iter() {
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}
enum Video {
    Frame(u32, Vec<u8>),
    End,
    Error(String),
}
enum Audio {
    Samples(Vec<i16>),
    End,
    Error(String),
}
struct Ready {
    header: Header,
    audio_plan:Option<super::movie_tracks::Plan>,
    video: Receiver<Video>,
    audio: Option<Receiver<Audio>>,
}
fn child(
    exe: &Path,
    path: &Path,
    audio_plan: Option<&super::movie_tracks::Plan>,
    cancel: &Arc<Cancellation>,
) -> Result<(
    Arc<Mutex<Child>>,
    std::process::ChildStdout,
    Arc<Mutex<String>>,
)> {
    let mut command = super::movie_codec::command(exe);
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-threads",
            "1",
            "-i",
        ])
        .arg(path);
    if let Some(plan) = audio_plan {
        let (rate,channels)=(plan.rate,plan.channels);
        let map=if let Some(filter)=plan.filter() {command.args(["-filter_complex",&filter]);"[bwx_movie_audio]".to_owned()}
            else {format!("0:a:{}",plan.streams[0])};
        command.args([
            "-map",
            &map,
            "-vn",
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ar",
            &rate.to_string(),
            "-ac",
            &channels.to_string(),
        ]);
    } else {
        command.args([
            "-map",
            "0:v:0",
            "-an",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
        ]);
    }
    let mut child = command
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().context("Movie codec stdout missing")?;
    let mut stderr = child.stderr.take().context("Movie codec stderr missing")?;
    let diagnostic = Arc::new(Mutex::new(String::new()));
    let output = diagnostic.clone();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = stderr.read(&mut bytes) {
            if count == 0 {
                break;
            }
            if let Ok(mut text) = output.lock() {
                if text.len() < 16384 {
                    text.push_str(&String::from_utf8_lossy(&bytes[..count]));
                    if text.len() >= 16384 {
                        text.push_str("\n[codec diagnostics truncated]");
                    }
                }
            }
        }
    });
    let child = Arc::new(Mutex::new(child));
    cancel
        .children
        .lock()
        .map_err(|_| anyhow::anyhow!("Movie process registry poisoned"))?
        .push(child.clone());
    if cancel.stop.load(Ordering::Acquire) {
        cancel.cancel();
        anyhow::bail!("Movie canceled");
    }
    Ok((child, stdout, diagnostic))
}
fn finish(child: &Arc<Mutex<Child>>, diagnostic: &Arc<Mutex<String>>) -> Result<()> {
    let status = child
        .lock()
        .map_err(|_| anyhow::anyhow!("Movie process lock poisoned"))?
        .wait()?;
    ensure!(
        status.success(),
        "Movie decoder exited {status}: {}",
        diagnostic
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default()
    );
    Ok(())
}
fn video_worker(
    exe: PathBuf,
    path: PathBuf,
    header: Header,
    cancel: Arc<Cancellation>,
    tx: SyncSender<Video>,
) {
    let result = (|| -> Result<()> {
        let (child, mut stdout, diagnostic) = child(&exe, &path, None, &cancel)?;
        let size = usize::try_from(u64::from(header.width) * u64::from(header.height) * 4)?;
        for index in 0..header.frames {
            if cancel.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut frame = vec![0; size];
            stdout
                .read_exact(&mut frame)
                .with_context(|| format!("Truncated decoded movie frame {index}"))?;
            if tx.send(Video::Frame(index, frame)).is_err() {
                return Ok(());
            }
        }
        let mut extra = [0];
        ensure!(
            stdout.read(&mut extra)? == 0,
            "Movie decoder produced unexpected extra frames"
        );
        finish(&child, &diagnostic)?;
        let _ = tx.send(Video::End);
        Ok(())
    })();
    if let Err(error) = result {
        let _ = tx.send(Video::Error(format!("{error:#}")));
    }
}
fn audio_worker(
    exe: PathBuf,
    path: PathBuf,
    cancel: Arc<Cancellation>,
    tx: SyncSender<Audio>,
    plan: super::movie_tracks::Plan,
) {
    let result = (|| -> Result<()> {
        let (child, mut stdout, diagnostic) = child(&exe, &path, Some(&plan), &cancel)?;
        let align = usize::from(plan.channels) * 2;
        loop {
            if cancel.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut bytes = vec![0; 1024 * align];
            let mut count = 0;
            while count < bytes.len() {
                let read = stdout.read(&mut bytes[count..])?;
                if read == 0 {
                    break;
                }
                count += read;
            }
            ensure!(
                count % align == 0,
                "Decoded movie audio is not frame aligned"
            );
            if count == 0 {
                break;
            }
            let samples = bytes[..count]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            if tx.send(Audio::Samples(samples)).is_err() {
                return Ok(());
            }
        }
        finish(&child, &diagnostic)?;
        let _ = tx.send(Audio::End);
        Ok(())
    })();
    if let Err(error) = result {
        let _ = tx.send(Audio::Error(format!("{error:#}")));
    }
}
pub struct Player {
    pub request: Request,
    ready: Receiver<Result<Ready, String>>,
    streams: Option<Ready>,
    cancel: Arc<Cancellation>,
    audio: Option<super::movie_audio::Stream>,
    audio_end: bool,
    video_end: bool,
    next: Option<(u32, Vec<u8>)>,
    texture: Option<egui::TextureHandle>,
    pub frame: Option<u32>,
    started: Option<Instant>,
    pub error: Option<String>,
    pub completed: bool,
    seconds: f64,
    skip: Option<super::movie_skip::Hold>,
    skip_clock: Instant,
    skipped: bool,
}
impl Player {
    pub fn new(root: PathBuf, request: Request, skip_seconds: Option<f32>) -> Self {
        let audio = if request.frontend_skip.is_some() {
            super::movie_tracks::Selection::WiiTrackZero
        } else {
            super::movie_tracks::Selection::SingleTrack
        };
        Self::with_audio_selection(root,request,skip_seconds,audio)
    }
    pub fn with_audio_selection(root:PathBuf,request:Request,skip_seconds:Option<f32>,selection:super::movie_tracks::Selection)->Self {
        let cancel = Arc::new(Cancellation {
            stop: AtomicBool::new(false),
            children: Mutex::new(Vec::new()),
        });
        let worker_cancel = cancel.clone();
        let (tx, ready) = mpsc::channel();
        let worker_request = request.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Ready> {
                ensure!(
                    worker_request.flags == [true, true],
                    "Non-default native movie options are not reconstructed: {:?}",
                    worker_request.flags
                );
                let path = resolve(&root, &worker_request.filename)?;
                let header = Header::read(&path)?;
                let audio_plan=super::movie_tracks::Plan::select(&header.audio_streams,&selection)?;
                let exe = super::movie_codec::provision()?;
                let (video_tx, video) = mpsc::sync_channel(2);
                let audio = if let Some(plan) = audio_plan.clone() {
                    let (audio_tx, audio) = mpsc::sync_channel(8);
                    let (exe, path, cancel) = (
                        exe.clone(),
                        path.clone(),
                        worker_cancel.clone(),
                    );
                    std::thread::spawn(move || audio_worker(exe, path, cancel, audio_tx,plan));
                    Some(audio)
                } else {
                    None
                };
                let h = header.clone();
                std::thread::spawn(move || video_worker(exe, path, h, worker_cancel, video_tx));
                Ok(Ready {
                    header,
                    audio_plan,
                    video,
                    audio,
                })
            })();
            let _ = tx.send(result.map_err(|error| format!("{error:#}")));
        });
        Self {
            request,
            ready,
            streams: None,
            cancel,
            audio: None,
            audio_end: false,
            video_end: false,
            next: None,
            texture: None,
            frame: None,
            started: None,
            error: None,
            completed: false,
            seconds: 0.,
            skip: skip_seconds.map(super::movie_skip::Hold::new),
            skip_clock: Instant::now(),
            skipped: false,
        }
    }
    fn update(&mut self, ctx: &egui::Context) -> Result<()> {
        if self.streams.is_none() {
            match self.ready.try_recv() {
                Ok(Ok(ready)) => {
                    self.audio = ready
                        .audio_plan.as_ref()
                        .map(|plan| super::movie_audio::Stream::open(plan.rate, plan.channels))
                        .transpose()?;
                    self.audio_end = ready.audio.is_none();
                    self.streams = Some(ready);
                }
                Ok(Err(error)) => anyhow::bail!(error),
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => anyhow::bail!("Movie setup worker disconnected"),
            }
        }
        let ready = self.streams.as_ref().unwrap();
        if let Some(audio) = &mut self.audio {
            audio.poll()?;
            while !self.audio_end && audio.queued_frames()? < u64::from(audio.rate) / 4 {
                match ready.audio.as_ref().unwrap().try_recv() {
                    Ok(Audio::Samples(samples)) => audio.queue(samples)?,
                    Ok(Audio::End) => self.audio_end = true,
                    Ok(Audio::Error(error)) => anyhow::bail!(error),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        anyhow::bail!("Movie audio worker disconnected before completion")
                    }
                }
            }
        }
        if self.next.is_none() && !self.video_end {
            match ready.video.try_recv() {
                Ok(Video::Frame(index, bytes)) => self.next = Some((index, bytes)),
                Ok(Video::End) => self.video_end = true,
                Ok(Video::Error(error)) => anyhow::bail!(error),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    anyhow::bail!("Movie video worker disconnected before completion")
                }
            }
        }
        if self.started.is_none()
            && self.next.is_some()
            && self
                .audio
                .as_ref()
                .is_none_or(|audio| audio.submitted >= u64::from(audio.rate) / 8 || self.audio_end)
        {
            if let Some(audio) = &mut self.audio {
                audio.play()?;
            }
            self.started = Some(Instant::now());
        }
        if let Some(started) = self.started {
            self.seconds = if let Some(audio) = &self.audio {
                let time = audio.position()? as f64 / f64::from(audio.rate);
                // Some Bink tracks end before the last video frame. Continue from
                // the last sample using wall time only after the device drains.
                if self.audio_end && audio.empty() {
                    let anchor = self.started.get_or_insert(started);
                    let elapsed = anchor.elapsed().as_secs_f64();
                    time.max(elapsed)
                } else {
                    time
                }
            } else {
                started.elapsed().as_secs_f64()
            };
            let target = (self.seconds * f64::from(ready.header.fps[0])
                / f64::from(ready.header.fps[1]))
            .floor() as u32;
            loop {
                if self.next.as_ref().is_none_or(|(index, _)| *index > target) {
                    break;
                }
                let (index, bytes) = self.next.take().unwrap();
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [ready.header.width as usize, ready.header.height as usize],
                    &bytes,
                );
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture(
                        "Bink cinematic",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                self.frame = Some(index);
                match ready.video.try_recv() {
                    Ok(Video::Frame(index, bytes)) => self.next = Some((index, bytes)),
                    Ok(Video::End) => self.video_end = true,
                    Ok(Video::Error(error)) => anyhow::bail!(error),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        anyhow::bail!("Movie video worker disconnected before completion")
                    }
                }
            }
            self.completed = self.video_end
                && self.seconds >= ready.header.duration()
                && self
                    .audio
                    .as_ref()
                    .is_none_or(|audio| self.audio_end && audio.empty());
        }
        Ok(())
    }
    pub fn show(&mut self, ui: &mut egui::Ui, skip_held: bool) {self.show_frame(ui,skip_held,true);}

    /// RELc58a0 consumes an Accept event after the clip's minimum time.
    /// Frontend playback has its own control word, independent of gameplay skip.
    pub(super) fn show_frontend(&mut self, ui: &mut egui::Ui, accept: bool) {
        self.show_frame(ui, false, true);
        if accept && !self.completed && self.error.is_none() && self.frame.is_some()
            && matches!(self.request.frontend_skip, Some(FrontendSkip::AcceptAfter(seconds))
                if self.seconds > f64::from(seconds))
        {
            self.skipped = true;
            self.completed = true;
            self.cancel.cancel();
            self.audio = None;
        }
    }
    pub fn show_frozen(&mut self,ui:&mut egui::Ui) {self.show_frame(ui,false,false);}
    fn show_frame(&mut self, ui: &mut egui::Ui, skip_held: bool, advance:bool) {
        if advance && self.error.is_none() && !self.completed && self.frame.is_some() {
            if self.skip.as_mut().is_some_and(|skip|skip.update(skip_held,self.skip_clock.elapsed().as_secs_f64())) {
                self.skipped=true;self.completed=true;
                self.cancel.cancel();self.audio=None;
            }
        }
        if advance && self.error.is_none() && !self.completed {
            if let Err(error) = self.update(ui.ctx()) {
                self.error = Some(format!("{error:#}"));
                self.cancel.cancel();
                self.audio = None;
            }
        }
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0., egui::Color32::BLACK);
        if let Some(texture) = &self.texture {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let target = egui::Rect::from_center_size(rect.center(), size * scale);
            ui.painter().image(
                texture.id(),
                target,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
        }
        if skip_held && !self.completed && self.frame.is_some() {
            let text=if let Some(skip)=&self.skip {format!("Hold Space / A to skip ({:.1} / {:.1}s)",skip.elapsed,skip.seconds)}else {
                "Movie skipping unavailable: missing or invalid UI_SkipMovie_SkipButtonHoldTime".into()
            };
            ui.painter().text(rect.center_bottom()-egui::vec2(0.,32.),egui::Align2::CENTER_BOTTOM,text,egui::FontId::proportional(18.),egui::Color32::WHITE);
        }
        if let Some(error) = &self.error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Movie {}: {error}", self.request.filename),
            );
        } else if self.texture.is_none() {
            ui.centered_and_justified(|ui| ui.label("Preparing cinematic…"));
        }
        ui.ctx().request_repaint();
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"request":self.request,"header":self.streams.as_ref().map(|ready|&ready.header),"audio_plan":self.streams.as_ref().and_then(|ready|ready.audio_plan.as_ref()),"frame":self.frame,"seconds":self.seconds,"completed":self.completed,"skipped":self.skipped,"skip_hold_seconds":self.skip.as_ref().map(|skip|skip.seconds),"skip_prompt_native_presentation":false,"error":self.error,"clock":if self.audio.is_some(){"audio device samples"}else{"monotonic"},"reference_parity_verified":false})
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(debug_assertions)]
pub fn validate_original_headers() -> Result<()> {
    let root = PathBuf::from(
        std::env::var("BLOTWORX_TEST_DATA").context("BLOTWORX_TEST_DATA is required")?,
    );
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(root.join("Movies"))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bik"))
        {
            let header = Header::read(&path).with_context(|| path.display().to_string())?;
            rows.push(serde_json::json!({"file":path.file_name().unwrap().to_string_lossy(),"header":header}));
        }
    }
    rows.sort_by_key(|row| row["file"].as_str().unwrap().to_owned());
    ensure!(!rows.is_empty(), "No original movies found");
    let lua = mlua::Lua::new();
    let mut requests = Requests::default();
    requests.lua(
        &lua,
        vec![
            mlua::Value::Nil,
            mlua::Value::String(lua.create_string("first.bik")?),
        ]
        .into(),
    )?;
    ensure!(
        requests.pending.as_ref().unwrap().flags == [true, true],
        "Movie defaults differ"
    );
    requests.lua(
        &lua,
        vec![
            mlua::Value::Integer(123),
            mlua::Value::Nil,
            mlua::Value::String(lua.create_string("last.bik")?),
            mlua::Value::Boolean(false),
            mlua::Value::Boolean(true),
        ]
        .into(),
    )?;
    ensure!(requests.pending.as_ref().is_some_and(|request|request.filename=="last.bik"&&request.flags==[false,true]),"Movie final-four/last-request semantics differ");
    ensure!(
        resolve(&root, "../main.dol").is_err(),
        "Movie traversal accepted"
    );
    std::fs::write(
        "target/movie-header-validation.json",
        serde_json::to_vec_pretty(
            &serde_json::json!({"original_movies":rows,"wrapper_source":"REL1f8c44/1f878c","functional_playback_verified":false}),
        )?,
    )?;
    eprintln!(
        "Movie original headers: {} parsed; request ordering/defaults/path checks PASS",
        rows.len()
    );
    Ok(())
}
