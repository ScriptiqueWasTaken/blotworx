//! Movie track IDs and speaker routing reconstructed from EM2 PC a22350,
//! a23090..a230c4 and a2254c..a22608. Legacy single-track behavior is retained.
use anyhow::{ensure, Context, Result};
#[derive(Clone, Debug)]
pub enum Selection {
    SingleTrack,
    PcLanguage(String),
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Track {
    pub id: u32,
    pub rate: u32,
    pub channels: u16,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Plan {
    pub streams: Vec<usize>,
    pub track_ids: Vec<u32>,
    pub rate: u32,
    pub channels: u16,
    pub source_channels: u16,
    pub output_policy: &'static str,
}
impl Plan {
    pub fn select(tracks: &[Track], selection: &Selection) -> Result<Option<Self>> {
        if tracks.is_empty() {
            return Ok(None);
        }
        if tracks.len() == 1 {
            ensure!(
                !matches!(selection, Selection::PcLanguage(_)) || tracks[0].id == 0,
                "Single-stream PC movie is missing native track ID 0"
            );
            return Ok(Some(Self {
                streams: vec![0],
                track_ids: vec![tracks[0].id],
                rate: tracks[0].rate,
                channels: tracks[0].channels,
                source_channels: tracks[0].channels,
                output_policy: "native single stream",
            }));
        }
        let Selection::PcLanguage(language) = selection else {
            anyhow::bail!("Multitrack movie requires native language selection");
        };
        // PC Localizer names 128d6d8; Movie language-group table 13d98e0.
        let names = [
            "None",
            "English_NTSC",
            "English_PAL",
            "French_NTSC",
            "French_PAL",
            "Spanish_NTSC",
            "Spanish_PAL",
            "German",
            "Italian",
            "Portuguese",
            "Japanese",
            "Korean",
            "ChineseSimple",
            "ChineseTraditional",
            "Dutch",
            "Swedish",
            "Danish",
            "Norwegian",
            "Russian",
            "Arabic",
            "Czech",
            "Hungarian",
            "BrazilianPortuguese",
            "Polish",
            "Turkish",
            "Finnish",
            "Greek",
        ];
        let language = names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(language))
            .context("Unrecognized PC movie audio language")?;
        let group = match language {
            1 | 2 => 0,
            3 | 4 => 1,
            5..=25 => language as u32 - 3,
            _ => 0,
        };
        let mut streams = Vec::new();
        let mut ids = Vec::new();
        for offset in 0..4 {
            let id = group * 4 + offset;
            let candidates = tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| track.id == id)
                .collect::<Vec<_>>();
            ensure!(
                candidates.len() == 1,
                "Movie audio track ID {id} missing or ambiguous"
            );
            let (index, track) = candidates[0];
            ensure!(
                track.channels == if offset < 2 { 2 } else { 1 },
                "Unexpected PC movie speaker layout for track {id}"
            );
            ensure!(track.rate > 0, "Invalid movie sample rate");
            streams.push(index);
            ids.push(id);
        }
        let rate = tracks[streams[0]].rate;
        ensure!(
            streams.iter().all(|&index| tracks[index].rate == rate),
            "Mixed-rate native movie streams require explicit resampling"
        );
        Ok(Some(Self {streams,track_ids:ids,rate,channels:2,source_channels:6,
            output_policy:"source 5.1 speaker routing; FFmpeg standard stereo output conversion; original DirectSound mix parity unverified"}))
    }
    pub fn filter(&self) -> Option<String> {
        if self.streams.len() != 4 {
            return None;
        }
        let inputs = self
            .streams
            .iter()
            .map(|i| format!("[0:a:{i}]"))
            .collect::<String>();
        Some(format!("{inputs}join=inputs=4:channel_layout=5.1:map=0.0-FL|0.1-FR|1.0-BL|1.1-BR|2.0-FC|3.0-LFE,aformat=channel_layouts=stereo[bwx_movie_audio]"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_order_does_not_choose_the_language() -> Result<()> {
        let tracks = [
            4, 5, 6, 7, 20, 21, 22, 23, 16, 17, 18, 19, 12, 13, 14, 15, 0, 1, 2, 3,
        ]
        .iter()
        .map(|&id| Track {
            id,
            rate: 48000,
            channels: if id % 4 < 2 { 2 } else { 1 },
        })
        .collect::<Vec<_>>();
        for (language, ids, indices) in [
            ("English_PAL", [0, 1, 2, 3], [16, 17, 18, 19]),
            ("Spanish_PAL", [12, 13, 14, 15], [12, 13, 14, 15]),
            ("Italian", [20, 21, 22, 23], [4, 5, 6, 7]),
            ("French_PAL", [4, 5, 6, 7], [0, 1, 2, 3]),
            ("German", [16, 17, 18, 19], [8, 9, 10, 11]),
        ] {
            let plan = Plan::select(&tracks, &Selection::PcLanguage(language.into()))?.unwrap();
            assert_eq!(plan.track_ids, ids);
            assert_eq!(plan.streams, indices);
            assert_eq!((plan.source_channels, plan.channels), (6, 2));
        }
        assert!(Plan::select(&tracks, &Selection::SingleTrack).is_err());
        assert!(Plan::select(&tracks, &Selection::PcLanguage("Unknown".into())).is_err());
        assert!(Plan::select(&tracks[..19], &Selection::PcLanguage("English_PAL".into())).is_err());
        Ok(())
    }
}
