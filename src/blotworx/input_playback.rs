//! Explicit simulation-tick inputs for live gameplay captures. These host
//! controls are not original Wii recordings or a compatibility acceptance run.
use anyhow::{ensure, Context, Result};
use std::io::{BufWriter, Write};
#[derive(Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Frame {
    pub input: super::controller::Input,
    pub spin_pressed: bool,
    /// Held cutscene skip input; movie time advances independently of ticks.
    pub movie_skip_held: bool,
    /// Explicit root-factory demonstration, not retail sketch action inputs.
    pub anvil_root_pressed: bool,
    /// Same provisional R recovery as the viewport, not retail death handling.
    pub respawn_pressed: bool,
    pub paint: bool,
    pub thinner: bool,
    pub interact_pressed: bool,
    /// Frontend Wii Minus action, independent of gameplay spin.
    pub menu_delete_pressed: bool,
    pub dialogue_choice: Option<usize>,
    /// Host frames spent in the pause menu before simulation resumes.
    pub pause_frames: u16,
    pub aim: Option<[f32; 2]>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playback {
    pub level: String,
    pub frames: Vec<Frame>,
}

/// Streams optional per-tick diagnostics to disk instead of retaining a large
/// nested JSON value for every simulated frame. Playback input itself remains
/// owned by `Playback`; only the bounded writer buffer stays resident.
pub struct TraceWriter {
    writer: BufWriter<std::fs::File>,
    partial_path: std::path::PathBuf,
    final_path: std::path::PathBuf,
    tick_count: usize,
}

impl TraceWriter {
    pub fn create(capture_path: &std::path::Path, playback: &Playback) -> Result<Self> {
        let final_path = capture_path.with_extension("ticks.json");
        let partial_path = final_path.with_extension("json.partial");
        let file = std::fs::File::create(&partial_path).with_context(|| {
            format!("Could not create playback trace {}", partial_path.display())
        })?;
        let mut writer = BufWriter::new(file);
        writer.write_all(br#"{"format":"blotworx_playback_debug_v1","level":"#)?;
        serde_json::to_writer(&mut writer, &playback.level)?;
        writer.write_all(br#", "original_reference_validated":false,"event_coverage":"post-controller animation phase","ticks":["#)?;
        Ok(Self {
            writer,
            partial_path,
            final_path,
            tick_count: 0,
        })
    }

    pub fn push(&mut self, tick: &serde_json::Value) -> Result<()> {
        if self.tick_count > 0 {
            self.writer.write_all(b",")?;
        }
        serde_json::to_writer(&mut self.writer, tick)?;
        self.tick_count += 1;
        Ok(())
    }

    pub fn finish(mut self, playback: &Playback) -> Result<std::path::PathBuf> {
        self.writer.write_all(b"],\"inputs\":[")?;
        for (index, frame) in playback.frames.iter().take(self.tick_count).enumerate() {
            if index > 0 {
                self.writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut self.writer, frame)?;
        }
        self.writer.write_all(b"]}")?;
        self.writer.flush()?;
        drop(self.writer);
        if self.final_path.exists() {
            std::fs::remove_file(&self.final_path).with_context(|| {
                format!(
                    "Could not replace playback trace {}",
                    self.final_path.display()
                )
            })?;
        }
        std::fs::rename(&self.partial_path, &self.final_path).with_context(|| {
            format!(
                "Could not publish playback trace {}",
                self.final_path.display()
            )
        })?;
        Ok(self.final_path)
    }
}

impl Playback {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let playback: Self = serde_json::from_slice(&std::fs::read(path)?)
            .with_context(|| format!("Invalid input playback {}", path.display()))?;
        ensure!(
            !playback.level.is_empty() && !playback.level.contains('\0'),
            "Invalid playback level"
        );
        ensure!(!playback.frames.is_empty(), "Input playback has no ticks");
        for (tick, frame) in playback.frames.iter().enumerate() {
            for axes in [frame.input.movement, frame.input.camera_stick] {
                ensure!(
                    axes.iter()
                        .all(|v| v.is_finite() && (-1. ..=1.).contains(v)),
                    "Invalid playback axes at tick {tick}"
                );
            }
            ensure!(
                frame.input.camera_delta.iter().all(|v| v.is_finite()),
                "Invalid playback camera delta at tick {tick}"
            );
            if let Some(aim) = frame.aim {
                ensure!(
                    aim.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                    "Invalid playback aim at tick {tick}"
                );
            }
        }
        Ok(playback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_trace_streams_complete_json_and_only_consumed_inputs() -> Result<()> {
        let stem = format!("blotworx-trace-{}", std::process::id());
        let capture = std::env::temp_dir().join(format!("{stem}.png"));
        let final_path = capture.with_extension("ticks.json");
        let partial_path = final_path.with_extension("json.partial");
        let playback = Playback {
            level: "Levels/Test.level".to_owned(),
            frames: vec![Frame::default(), Frame::default()],
        };
        let mut writer = TraceWriter::create(&capture, &playback)?;
        writer.push(&serde_json::json!({"tick": 1, "position": [1.0, 2.0, 3.0]}))?;
        let path = writer.finish(&playback)?;
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        assert_eq!(value["format"], "blotworx_playback_debug_v1");
        assert_eq!(value["level"], "Levels/Test.level");
        assert_eq!(value["ticks"].as_array().unwrap().len(), 1);
        assert_eq!(value["inputs"].as_array().unwrap().len(), 1);
        assert!(!partial_path.exists());
        let _ = std::fs::remove_file(final_path);
        Ok(())
    }
}
