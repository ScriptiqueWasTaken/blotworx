use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::file_manipulator::EndianType;

pub const IDENTIFIER_BYTES: usize = 68;
pub const RECORD_BYTES: usize = IDENTIFIER_BYTES + 8;
const MAX_CUES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SubtitleCue {
    pub identifier: String,
    pub start_frame: u32,
    pub end_frame: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SubtitleFile {
    pub cues: Vec<SubtitleCue>,
}

fn read_u32(bytes: &[u8], endian: EndianType) -> Result<u32> {
    let bytes: [u8; 4] = bytes.try_into().context("Truncated subtitle frame value")?;
    Ok(if endian == EndianType::BIG {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    })
}

fn write_u32(value: u32, endian: EndianType, output: &mut Vec<u8>) {
    output.extend(if endian == EndianType::BIG {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
}

fn validate_identifier(identifier: &str) -> Result<()> {
    ensure!(
        !identifier.is_empty(),
        "Subtitle identifier cannot be empty"
    );
    ensure!(
        identifier.len() <= IDENTIFIER_BYTES,
        "Subtitle identifier is longer than {IDENTIFIER_BYTES} bytes"
    );
    ensure!(
        identifier.bytes().all(|byte| byte.is_ascii_graphic()),
        "Subtitle identifier must contain printable ASCII without spaces"
    );
    Ok(())
}

impl SubtitleFile {
    pub fn template(identifier: impl Into<String>) -> Self {
        Self {
            cues: vec![SubtitleCue {
                identifier: identifier.into(),
                start_frame: 0,
                end_frame: 90,
            }],
        }
    }

    pub fn load(data: &[u8], endian: EndianType) -> Result<Self> {
        ensure!(
            data.len() % RECORD_BYTES == 0,
            "Subtitle size {} is not a multiple of the {RECORD_BYTES}-byte cue record",
            data.len()
        );
        let count = data.len() / RECORD_BYTES;
        ensure!(count <= MAX_CUES, "Subtitle cue limit exceeded");
        let mut cues = Vec::with_capacity(count);
        for (index, record) in data.chunks_exact(RECORD_BYTES).enumerate() {
            let identifier_field = &record[..IDENTIFIER_BYTES];
            let identifier_end = identifier_field
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(IDENTIFIER_BYTES);
            ensure!(
                identifier_field[identifier_end..]
                    .iter()
                    .all(|byte| *byte == 0),
                "Subtitle cue {} has nonzero bytes after its identifier terminator",
                index + 1
            );
            let identifier = std::str::from_utf8(&identifier_field[..identifier_end])
                .with_context(|| format!("Subtitle cue {} identifier is not UTF-8", index + 1))?
                .to_owned();
            validate_identifier(&identifier)
                .with_context(|| format!("Invalid subtitle cue {}", index + 1))?;
            cues.push(SubtitleCue {
                identifier,
                start_frame: read_u32(&record[68..72], endian)?,
                end_frame: read_u32(&record[72..76], endian)?,
            });
        }
        Ok(Self { cues })
    }

    pub fn save(&self, endian: EndianType) -> Result<Vec<u8>> {
        ensure!(self.cues.len() <= MAX_CUES, "Subtitle cue limit exceeded");
        let capacity = self
            .cues
            .len()
            .checked_mul(RECORD_BYTES)
            .context("Subtitle output size overflow")?;
        let mut output = Vec::with_capacity(capacity);
        for (index, cue) in self.cues.iter().enumerate() {
            validate_identifier(&cue.identifier)
                .with_context(|| format!("Invalid subtitle cue {}", index + 1))?;
            output.extend(cue.identifier.as_bytes());
            output.resize(output.len() + IDENTIFIER_BYTES - cue.identifier.len(), 0);
            write_u32(cue.start_frame, endian, &mut output);
            write_u32(cue.end_frame, endian, &mut output);
        }
        let reparsed = Self::load(&output, endian)?;
        ensure!(
            reparsed == *self,
            "Subtitle save verification did not reproduce the document"
        );
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue_bytes(name: &str, start: u32, end: u32, endian: EndianType) -> Vec<u8> {
        SubtitleFile {
            cues: vec![SubtitleCue {
                identifier: name.into(),
                start_frame: start,
                end_frame: end,
            }],
        }
        .save(endian)
        .unwrap()
    }

    #[test]
    fn subtitle_roundtrip_and_edit_are_exact() {
        let mut source = cue_bytes("Movie_Line01", 15, 82, EndianType::BIG);
        source.extend(cue_bytes("Movie_Line02", 90, 140, EndianType::BIG));
        let mut document = SubtitleFile::load(&source, EndianType::BIG).unwrap();
        assert_eq!(document.save(EndianType::BIG).unwrap(), source);
        document.cues[1].end_frame = 155;
        let saved = document.save(EndianType::BIG).unwrap();
        assert_eq!(
            SubtitleFile::load(&saved, EndianType::BIG).unwrap(),
            document
        );
    }

    #[test]
    fn unusual_retail_timing_is_preserved() {
        let source = cue_bytes("Movie_Line14", 1685, 0, EndianType::BIG);
        let document = SubtitleFile::load(&source, EndianType::BIG).unwrap();
        assert_eq!(document.save(EndianType::BIG).unwrap(), source);
    }

    #[test]
    fn little_endian_records_are_supported() {
        let source = cue_bytes("Movie_Line01", 0x1020, 0x3040, EndianType::LITTLE);
        let document = SubtitleFile::load(&source, EndianType::LITTLE).unwrap();
        assert_eq!(document.cues[0].start_frame, 0x1020);
        assert_eq!(document.save(EndianType::LITTLE).unwrap(), source);
    }

    #[test]
    fn malformed_records_are_rejected() {
        assert!(SubtitleFile::load(&[0; RECORD_BYTES - 1], EndianType::BIG).is_err());
        let mut bad_padding = cue_bytes("A", 0, 1, EndianType::BIG);
        bad_padding[4] = 1;
        assert!(SubtitleFile::load(&bad_padding, EndianType::BIG).is_err());
        let mut bad_name = cue_bytes("A", 0, 1, EndianType::BIG);
        bad_name[0] = 0xff;
        assert!(SubtitleFile::load(&bad_name, EndianType::BIG).is_err());
    }
}
