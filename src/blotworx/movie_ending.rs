//! Retail EndGame opens the sequence of Binks and posts Play_fmv_endgame
//! once (RELbae28..baee0). The Wwise bed runs across clip changes. The Bink
//! tracks carry effects that must remain audible alongside that bed.
use super::super::{
    audio_bank::{Bank, NodePrefix},
    audio_codec, audio_panner, audio_parameters, movie_tracks,
};
use super::{child, decode_video_clip, finish, resolve, Audio, Cancellation, Header, Ready, Video};
use anyhow::{ensure, Context, Result};
use std::{
    collections::VecDeque,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        mpsc::{self, SyncSender},
        Arc,
    },
};

pub(super) struct Clip {
    pub name: String,
    pub first_frame: u32,
    pub start: f64,
    path: PathBuf,
    header: Header,
}

pub(super) struct Sequence {
    clips: Arc<[Clip]>,
    pub duration: f64,
    pub soundtrack: audio_codec::Info,
}

pub(super) struct Soundtrack {
    decoder: audio_codec::Decoder,
    gain: f32,
}

impl Soundtrack {
    pub(super) fn info(&self) -> &audio_codec::Info {
        &self.decoder.info
    }

    pub(super) fn read_chunk(&mut self) -> Result<Option<Vec<i16>>> {
        let mut chunk = self.decoder.read_chunk()?;
        if self.gain != 1. {
            for sample in chunk.iter_mut().flatten() {
                *sample = (f32::from(*sample) * self.gain).round() as i16;
            }
        }
        Ok(chunk)
    }
}

pub(super) fn frame_seconds(header: &Header, frame: u32) -> f64 {
    f64::from(frame) * f64::from(header.fps[1]) / f64::from(header.fps[0])
}

impl Sequence {
    pub(super) fn clip(&self, frame: u32) -> &Clip {
        let after = self.clips.partition_point(|clip| clip.first_frame <= frame);
        &self.clips[after - 1]
    }

    pub(super) fn frame_time(&self, frame: u32) -> f64 {
        let clip = self.clip(frame);
        clip.start + frame_seconds(&clip.header, frame - clip.first_frame)
    }
}

pub(super) fn soundtrack(root: &Path, event: &str) -> Result<Soundtrack> {
    let media_root = root.join("Audio/Wii");
    let localized = media_root.join("Localize/English_NTSC");
    let bank_path = localized.join("fmv_endgame_soundtrack.bnk");
    let bytes =
        std::fs::read(&bank_path).with_context(|| format!("Read {}", bank_path.display()))?;
    let bank = Bank::parse(&bytes)?;
    let actions = bank.named_event_actions(event)?;
    let [action] = actions.as_slice() else {
        anyhow::bail!("Unexpected {event} action list");
    };
    let play = bank
        .objects
        .get(action)
        .with_context(|| format!("Missing {event} action"))?
        .action()?;
    ensure!(
        play.kind == 0x4011 && play.delay == [0; 3],
        "Unexpected {event} Play action"
    );
    let sound = bank
        .objects
        .get(&play.target)
        .with_context(|| format!("Missing {event} sound"))?;
    let source = sound.source()?;
    ensure!(
        source.plugin == 0x40001 && matches!(source.stream_type, 1 | 2),
        "Unexpected {event} audio source"
    );
    let filename = format!("{}.ogg", source.file_id);
    let path = localized.join(&filename);
    let path = if path.is_file() {
        path
    } else {
        media_root.join(filename)
    };
    let decoder = audio_codec::Decoder::open_file(&path, source.media_id)?;
    ensure!(
        decoder.info.rate == source.sample_rate,
        "{event} audio rate disagrees with its bank"
    );
    let node = NodePrefix::parse(source.node_parameters)?;
    let fixed = audio_parameters::Fixed::load(&node.ranges)?;
    ensure!(
        node.parent == 0
            && node.state_group == 0
            && fixed.pitch == 0
            && fixed.filter == 0
            && fixed.lfe == 0
            && node.ranges.iter().all(|range| range[1..] == [0., 0.]),
        "Unsupported {event} soundtrack parameters"
    );
    let gain = audio_panner::output_gain(f32::from(fixed.volume) / 10.)?;
    Ok(Soundtrack { decoder, gain })
}

pub(super) fn prepare(root: &Path, names: &[String], cancel: Arc<Cancellation>) -> Result<Ready> {
    let mut clips: Vec<Clip> = Vec::with_capacity(names.len());
    let mut duration = 0.;
    let mut first_frame = 0_u32;
    for name in names {
        let path = resolve(root, name)?;
        let header = Header::read(&path)?;
        if let Some(first) = clips.first() {
            ensure!(
                (header.width, header.height) == (first.header.width, first.header.height),
                "Ending clip {name} changes dimensions"
            );
        }
        let next_frame = first_frame
            .checked_add(header.frames)
            .context("Ending frame count overflow")?;
        let seconds = header.duration();
        clips.push(Clip {
            name: name.clone(),
            first_frame,
            start: duration,
            path,
            header,
        });
        first_frame = next_frame;
        duration += seconds;
    }
    let header = clips
        .first()
        .context("Empty ending sequence")?
        .header
        .clone();
    let decoder = soundtrack(root, "Play_fmv_endgame")?;
    ensure!(
        decoder.info().channels == 2,
        "Expected a stereo ending soundtrack"
    );
    let soundtrack = decoder.info().clone();
    let clips: Arc<[Clip]> = clips.into();
    let exe = super::super::movie_codec::provision()?;
    let (video_tx, video) = mpsc::sync_channel(4);
    let (audio_tx, audio) = mpsc::sync_channel(8);
    {
        let clips = clips.clone();
        let exe = exe.clone();
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let terminal = match decode_video(&exe, &clips, &cancel, &video_tx) {
                Ok(()) => Video::End,
                Err(error) => Video::Error(format!("{error:#}")),
            };
            let _ = video_tx.send(terminal);
        });
    }
    {
        let clips = clips.clone();
        std::thread::spawn(move || {
            let terminal = match mix_audio(&exe, &clips, decoder, &cancel, &audio_tx) {
                Ok(()) => Audio::End,
                Err(error) => Audio::Error(format!("{error:#}")),
            };
            let _ = audio_tx.send(terminal);
        });
    }
    Ok(Ready {
        header,
        audio_plan: None,
        reel_audio: None,
        ending: Some(Sequence {
            clips,
            duration,
            soundtrack,
        }),
        video,
        audio: Some(audio),
    })
}

fn decode_video(
    exe: &Path,
    clips: &[Clip],
    cancel: &Arc<Cancellation>,
    tx: &SyncSender<Video>,
) -> Result<()> {
    for clip in clips {
        if !decode_video_clip(exe, &clip.path, &clip.header, clip.first_frame, cancel, tx)? {
            return Ok(());
        }
    }
    Ok(())
}

fn mix_audio(
    exe: &Path,
    clips: &[Clip],
    mut decoder: Soundtrack,
    cancel: &Arc<Cancellation>,
    tx: &SyncSender<Audio>,
) -> Result<()> {
    let rate = decoder.info().rate;
    let mut bed = VecDeque::new();
    let mut bed_ended = false;
    let mut rendered = 0_u64;
    for clip in clips {
        let plan = movie_tracks::Plan::select(
            &clip.header.audio_streams,
            &movie_tracks::Selection::WiiTrackZero,
        )?;
        let mut effects = plan
            .map(|mut plan| {
                plan.rate = rate;
                plan.channels = 2;
                child(exe, &clip.path, Some(&plan), cancel)
            })
            .transpose()?;
        let mut pcm = Vec::with_capacity(8192);
        let end = ((clip.start + clip.header.duration()) * f64::from(rate)).round() as u64;
        while rendered < end {
            if cancel.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let frames = (end - rendered).min(2048) as usize;
            let count = frames * 2;
            while bed.len() < count && !bed_ended {
                match decoder.read_chunk()? {
                    Some(samples) => bed.extend(samples),
                    None => bed_ended = true,
                }
            }
            let mut mixed: Vec<_> = (0..count).map(|_| bed.pop_front().unwrap_or(0)).collect();
            if let Some((_, output, _)) = &mut effects {
                pcm.clear();
                let size = (&mut *output)
                    .take((count * 2) as u64)
                    .read_to_end(&mut pcm)?;
                ensure!(
                    size % 4 == 0,
                    "Unaligned stereo Bink audio in {}",
                    clip.name
                );
                for (sample, bytes) in mixed.iter_mut().zip(pcm.chunks_exact(2)) {
                    *sample = sample.saturating_add(i16::from_le_bytes([bytes[0], bytes[1]]));
                }
            }
            if tx.send(Audio::Samples(mixed)).is_err() {
                return Ok(());
            }
            rendered += frames as u64;
        }
        if let Some((process, mut output, diagnostic)) = effects {
            // Encoded Bink audio can pad a final packet past the picture cut.
            // Drain the worker while preserving the next clip's sample boundary.
            std::io::copy(&mut output, &mut std::io::sink())?;
            finish(&process, &diagnostic)?;
        }
    }
    Ok(())
}
